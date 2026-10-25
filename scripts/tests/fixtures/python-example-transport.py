"""运行真实公开示例，以 Python 子进程隔离验证 UTF-8 传输，不冒充语言语义验收。"""

import io
import json
from pathlib import Path
import runpy
import subprocess
import sys
import tempfile
import unittest
from unittest import mock

EXAMPLE = Path(__file__).resolve().parents[2] / "examples/analyze-directory.py"
CHILD = """
import json, sys
request = json.loads(sys.stdin.buffer.read().decode('utf-8'))
assert request['repository_root'] == sys.argv[1]
assert 'files' not in request
assert sys.stdin.buffer.read() == b''
node = {'category':'callable', 'id':'rust:丁', 'language':'rust', 'native_kind':'function',
        'name':'丁', 'signature':'fn 丁()', 'code':'{}', 'source':'fn 丁() {}',
        'contains':[], 'references':[], 'extensions':{}}
completed = {'protocol':request['protocol'], 'engine_version':'0.1.1', 'pack_id':'rust', 'type':'completed', 'id':request['id'], 'outcome':'ok',
             'graph':{'root_contains':['rust:丁'], 'nodes':[node], 'external_symbols':[]}}
sys.stdout.buffer.write((''.join(json.dumps(row, ensure_ascii=False)+'\\n' for row in [completed])).encode('utf-8'))
sys.stderr.buffer.write('诊断丁\\n'.encode('utf-8'))
sys.exit(int(sys.argv[2]))
"""


class ExampleTransportTests(unittest.TestCase):
    """入口逻辑保持原样，只用可跨平台启动的真实 Python 子进程替换分析器程序。"""

    def exercise(self, status):
        """强制默认解码和 stdout 为 cp1252，并保留每条真实子进程结果。"""
        with tempfile.TemporaryDirectory(prefix="codegraph-example-") as temporary:
            directory = Path(temporary)
            root = directory / "源码丁"
            root.mkdir()
            executable = directory / "codegraph-rust"
            executable.touch()
            output = io.BytesIO()
            stdout = io.TextIOWrapper(output, encoding="cp1252")
            captured = []
            execute = subprocess.run

            def launch(command, **kwargs):
                """仅替换可执行程序，示例选择的输入、编码、捕获与退出检查全部进入真实管道。"""
                self.assertEqual(command, [str(executable.resolve()), "analyze"])
                try:
                    result = execute([sys.executable, "-c", CHILD, str(root.resolve()), str(status)], **kwargs)
                except subprocess.CalledProcessError as error:
                    captured.append(error)
                    raise
                captured.append(result)
                return result

            example = runpy.run_path(str(EXAMPLE))
            with mock.patch.object(sys, "argv", [str(EXAMPLE), str(executable), str(root)]), \
                 mock.patch.object(sys, "stdout", stdout), \
                 mock.patch.object(subprocess, "_text_encoding", return_value="cp1252"), \
                 mock.patch.object(subprocess, "run", side_effect=launch):
                if status:
                    with self.assertRaises(subprocess.CalledProcessError) as raised:
                        example["main"]()
                    self.assertEqual(raised.exception.returncode, status)
                else:
                    example["main"]()
            stdout.flush()
            forwarded = output.getvalue()
            stdout.close()
            self.assertEqual(len(captured), 1)
            self.assertEqual(captured[0].stderr, "诊断丁\n".encode("utf-8"))
            if status:
                self.assertEqual(forwarded, b"")
            else:
                self.assertEqual(forwarded, captured[0].stdout)
                self.assertEqual(json.loads(forwarded.splitlines()[0])["graph"]["nodes"][0]["name"], "丁")

    def test_utf8_request_result_and_stdout(self):
        """Unicode 项目根和原始响应完整往返，不经本地代码页转码。"""
        self.exercise(0)

    def test_failure_keeps_stderr_and_exit_code(self):
        """失败仍由 check=True 抛出原退出码和诊断，不转发不成功的协议结果。"""
        self.exercise(7)


if __name__ == "__main__":
    unittest.main()

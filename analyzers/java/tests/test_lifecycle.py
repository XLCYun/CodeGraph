"""真实 Java 公共入口的 EOF、调用方终止与管道退出边界。"""
import json
import os
import pathlib
import subprocess
import sys
import tempfile
import time
import unittest
from test_helper import PUBLIC_COMMAND, TARGET, metadata, write


class LifecycleTests(unittest.TestCase):
    """调用方负责期限和终止，语言程序没有取消帧或后台监听线程。"""

    def setUp(self):
        """创建独立源码与标准目标提示，所有变更都只在临时目录。"""
        self.temp = tempfile.TemporaryDirectory(prefix="codegraph Java 生命周期-")
        self.addCleanup(self.temp.cleanup)
        self.root = pathlib.Path(self.temp.name) / "仓库"
        self.root.mkdir()
        metadata(self.root)
        write(self.root, "src/main/java/Main.java", "class Main { void run(){} }")
        self.request = {"protocol": "codegraph.analyzer/1", "type": "analyze", "id": "lifecycle",
                        "repository_root": str(self.root), "files": ["src/main/java/Main.java"],
                        "options": {"include_tests": False, "rust_dependency_scope": "repository-only"}}

    def launch(self):
        """启动真实 JVM，清理钩子只在断言后回收，不作为被测自停证据。"""
        proc = subprocess.Popen([*PUBLIC_COMMAND, "analyze"], stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                                stderr=subprocess.PIPE, cwd=tempfile.gettempdir(),
                                env=dict(os.environ, CODEGRAPH_JAVA_TARGET_JDK=str(TARGET)))

        def cleanup():
            """保证失败用例也不会留下 JVM 或打开的管道。"""
            if proc.poll() is None:
                proc.kill()
            proc.wait(timeout=5)
            for stream in (proc.stdin, proc.stdout, proc.stderr):
                if stream:
                    stream.close()

        self.addCleanup(cleanup)
        return proc

    def complete_input(self, proc, payload=None):
        """唯一 JSON 写完后关闭 stdin；communicate 不再重复 flush 已关闭句柄。"""
        proc.stdin.write(payload if payload is not None else json.dumps(self.request).encode("utf-8"))
        proc.stdin.close()
        proc.stdin = None

    def cpu_milliseconds(self, pid):
        """实际 CPU 活动证明 EOF 后程序正在进行真实分析。"""
        if sys.platform == "win32":
            value = subprocess.check_output(["powershell.exe", "-NoProfile", "-Command",
                                            f"(Get-Process -Id {pid}).TotalProcessorTime.TotalMilliseconds"], text=True)
            return float(value.strip())
        value = subprocess.check_output(["/bin/ps", "-p", str(pid), "-o", "time="], text=True)
        total = 0.0
        for part in value.strip().split(":"):
            total = total * 60 + float(part)
        return total * 1000

    def running(self, discovery=False):
        """大源码夹具在 EOF 后仍保持活动，不能靠输入等待冒充分析。"""
        files = []
        for index in range(50):
            file = f"src/main/java/C{index}.java"
            files.append(file)
            write(self.root, file, f"class C{index} {{" + "".join(f"int f{number}(){{return {number};}}" for number in range(200)) + "}")
        if discovery:
            self.request.pop("files")
        else:
            self.request["files"] = files
        proc = self.launch()
        self.complete_input(proc)
        deadline = time.monotonic() + 15
        while proc.poll() is None and self.cpu_milliseconds(proc.pid) < 500:
            self.assertLess(time.monotonic(), deadline)
            time.sleep(0.025)
        self.assertIsNone(proc.poll(), "fixture finished before active-analysis boundary")
        return proc

    def test_waits_for_eof_then_completes(self):
        """完整 JSON 在 EOF 前不提前分析，关闭后产生唯一非空成功图。"""
        proc = self.launch()
        proc.stdin.write(json.dumps(self.request).encode("utf-8"))
        proc.stdin.flush()
        time.sleep(0.25)
        self.assertIsNone(proc.poll())
        self.complete_input(proc, b"")
        output, diagnostics = proc.communicate(timeout=15)
        self.assertEqual(proc.returncode, 0, diagnostics)
        result = json.loads(output)
        self.assertEqual(result["outcome"], "ok")
        self.assertTrue(result["graph"]["nodes"])

    def test_second_json_is_invalid_after_eof(self):
        """第二个 JSON 不是控制消息，整个输入统一拒绝。"""
        proc = self.launch()
        self.complete_input(proc, json.dumps(self.request).encode("utf-8") + b"\n{}")
        output, _ = proc.communicate(timeout=15)
        self.assertEqual(proc.returncode, 1)
        self.assertEqual(json.loads(output)["error"]["code"], "invalid_request")

    def test_caller_terminates_active_analysis_and_discovery(self):
        """调用方在真实 CPU 活动后终止 JVM，正常 EOF 没有取消分析。"""
        for discovery in (False, True):
            with self.subTest(discovery=discovery):
                proc = self.running(discovery)
                if sys.platform != "win32":
                    listing = subprocess.check_output(["/bin/ps", "-axo", "pid=,ppid="], text=True)
                    self.assertFalse(any(int(row.split()[1]) == proc.pid for row in listing.splitlines() if len(row.split()) == 2), "Java must analyze directly without a worker process")
                proc.kill()
                output, _ = proc.communicate(timeout=5)
                self.assertNotEqual(proc.returncode, 0)
                self.assertEqual(output, b"")

    def test_caller_terminates_input_wait(self):
        """调用方可以终止未完整输入的程序，不承诺伪造最终结果。"""
        proc = self.launch()
        proc.stdin.write(b"{")
        proc.stdin.flush()
        time.sleep(0.25)
        proc.kill()
        output, _ = proc.communicate(timeout=5)
        self.assertNotEqual(proc.returncode, 0)
        self.assertEqual(output, b"")

    def test_caller_terminates_stdout_backpressure(self):
        """真实大图填满 stdout 后仍由调用方终止，不依赖 stdin 控制通道。"""
        import concurrent.futures
        proc = self.running()
        pool = concurrent.futures.ThreadPoolExecutor(max_workers=1)
        try:
            first = pool.submit(proc.stdout.read, 1).result(timeout=20)
        except BaseException:
            proc.kill()
            proc.wait(timeout=5)
            raise
        finally:
            pool.shutdown(wait=True)
        self.assertEqual(first, b"{")
        time.sleep(0.2)
        self.assertIsNone(proc.poll(), "large graph must still be blocked before caller termination")
        proc.kill()
        output, _ = proc.communicate(timeout=5)
        self.assertNotEqual(proc.returncode, 0)
        with self.assertRaises(json.JSONDecodeError):
            json.loads(first + output)

    def test_broken_stdout_exits_nonzero(self):
        """JVM PrintStream 的错误状态必须传播到进程退出码。"""
        proc = self.launch()
        proc.stdout.close()
        self.complete_input(proc)
        proc.wait(timeout=15)
        self.assertEqual(proc.returncode, 1)


if __name__ == "__main__":
    unittest.main()

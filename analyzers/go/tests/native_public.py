"""真实直接入口的公共协议、来源和调用方生命周期验收。"""
import hashlib
import json
import os
from pathlib import Path
import signal
import subprocess
import tempfile
import time
import unittest


def snapshot(root):
    """记录完整仓内路径与字节，分析后比较只读边界。"""
    return {p.relative_to(root).as_posix(): hashlib.sha256(p.read_bytes()).hexdigest() if p.is_file() else 'directory'
            for p in root.rglob('*')}


class NativePublicTests(unittest.TestCase):
    """每个案例独立启动固定语言程序，stdout 只能含唯一最终结果。"""

    def setUp(self):
        """创建与程序安装隔离的临时源根。"""
        self.temp = tempfile.TemporaryDirectory(prefix='codegraph-public-' + self.PACK + '-')
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name) / 'repo space # 中文'
        self.root.mkdir()

    def environment(self):
        """SDK 是已安装目标的线索；公共 JSON 不携带 SDK、命令或路径 catalog。"""
        env = {**os.environ}
        env.pop('CODEGRAPH_TEST_DISCOVERY_PROBE', None)
        if self.PACK == 'go':
            env['CODEGRAPH_GO_TARGET_SDK'] = str(self.SDK)
        elif self.PACK == 'csharp':
            env['DOTNET_ROOT'] = str(self.SDK)
            env['DOTNET_MULTILEVEL_LOOKUP'] = '0'
        return env

    def request(self, files=(), discovery=False):
        """生成固定公共选项；空元组明确表示无需项目或 SDK 发现的空选集。"""
        request = {'protocol': 'codegraph.analyzer/1', 'type': 'analyze', 'id': '公开-é-😀',
                   'repository_root': str(self.root),
                   'options': {'include_tests': False, 'rust_dependency_scope': 'repository-only'}}
        if not discovery:
            request['files'] = list(files)
        return request

    def run_public(self, request, env=None, raw=None):
        """以 UTF-8 无尾换行字节输入并关闭 stdin，检查固定身份与退出码一致。"""
        payload = raw if raw is not None else json.dumps(request, ensure_ascii=False).encode('utf-8')
        proc = subprocess.run([str(self.ANALYZER), 'analyze'], input=payload, capture_output=True,
                              env=env or self.environment(), timeout=60)
        result = json.loads(proc.stdout.decode('utf-8', errors='strict'))
        self.assertTrue(proc.stdout.endswith(b'\n'))
        self.assertEqual(result['protocol'], 'codegraph.analyzer/1')
        self.assertEqual(result['engine_version'], '0.1.1')
        self.assertEqual(result['pack_id'], self.PACK)
        self.assertEqual(result['type'], 'completed')
        self.assertEqual(proc.returncode, 0 if result['outcome'] == 'ok' else 1, proc.stderr.decode())
        self.assertNotIn('evidence', result)
        self.assertNotIn('ready', result)
        if result['outcome'] == 'error':
            self.assertNotIn('graph', result)
            self.assertEqual(set(result['error']), {'code', 'message'})
        return result

    def prepare(self, large=False):
        """按每种语言的真实项目模型构造选集外绑定与标准 SDK 引用。"""
        if self.PACK == 'go':
            (self.root / 'go.mod').write_text('module example.test/direct\ngo 1.23.8\n', encoding='utf-8')
            text = 'package direct\nimport "strings"\nfunc Run()bool{return Hidden() && strings.Contains("a","b")}\n'
            if large:
                text += '\n'.join(f'func F{i}()int{{return {i}}}' for i in range(20000))
            (self.root / 'Main.go').write_text(text, encoding='utf-8')
            (self.root / 'Other.go').write_text('package direct\nfunc Hidden()bool{return true}\n', encoding='utf-8')
            return ['Main.go', 'Other.go']
        if self.PACK == 'csharp':
            (self.root / 'App.csproj').write_text('<Project Sdk="Microsoft.NET.Sdk"><PropertyGroup><TargetFramework>net10.0</TargetFramework></PropertyGroup></Project>', encoding='utf-8')
            text = 'class MainCode { static void Run(){ OtherCode.Hidden(); System.Console.WriteLine("x"); } }'
            if large:
                text += '\n'.join(f'class C{i}{{ int F()=>{i}; }}' for i in range(12000))
            (self.root / 'Main.cs').write_text(text, encoding='utf-8')
            (self.root / 'Other.cs').write_text('class OtherCode { public static void Hidden(){} }', encoding='utf-8')
            return ['Main.cs', 'Other.cs']
        (self.root / 'api.h').write_text('inline int Hidden(){return 1;}\n', encoding='utf-8')
        text = '#include "api.h"\nint Run(){return Hidden();}\n'
        if large:
            text += '\n'.join(f'int f{i}(){{return {i};}}' for i in range(20000))
        (self.root / 'Main.cpp').write_text(text, encoding='utf-8')
        (self.root / 'compile_commands.json').write_text(json.dumps([
            {'directory': str(self.root), 'file': 'Main.cpp', 'arguments': ['clang++', '-std=c++20', '-c', 'Main.cpp']}]), encoding='utf-8')
        return ['Main.cpp', 'api.h']

    def test_shared_conformance_and_empty_without_sdk(self):
        """公共共享向量必须使用真实入口，显式空集不触发项目配置或目标 SDK 发现。"""
        vectors = Path(__file__).resolve().parents[2] / 'common/src/tests/public-conformance.json'
        for case in json.loads(vectors.read_text()):
            with self.subTest(case=case['name']):
                request = self.request()
                request.update(case['patch'])
                result = self.run_public(request, raw=(json.dumps(request) + case['suffix']).encode())
                self.assertEqual(result['outcome'], case['outcome'], result)
                if 'error_code' in case:
                    self.assertEqual(result['error']['code'], case['error_code'], result)
        env = self.environment()
        for key in ['CODEGRAPH_GO_TARGET_SDK', 'CODEGRAPH_GO_SDK', 'GOROOT', 'DOTNET_ROOT', 'DOTNET_ROOT_ARM64',
                    'DOTNET_ROOT_X64', 'DOTNET_ROOT_X86', 'CODEGRAPH_MACOS_SDK', 'SDKROOT', 'CODEGRAPH_WINDOWS_SDK']:
            env[key] = str(self.root / 'nonexistent-sdk')
        self.assertEqual(self.run_public(self.request(), env=env)['graph'],
                         {'root_contains': [], 'nodes': [], 'external_symbols': []})

    def test_exact_selection_discovery_standard_binding_and_read_only(self):
        """发现完整选集，显式选集保留仓内图外目标，原始文件路径和字节均保持不变。"""
        files = self.prepare()
        before = snapshot(self.root)
        discovered = self.run_public(self.request(discovery=True))
        self.assertEqual(discovered['outcome'], 'ok', discovered)
        self.assertTrue(discovered['graph']['nodes'])
        explicit = self.run_public(self.request(files[:1]))
        self.assertEqual(explicit['outcome'], 'ok', explicit)
        self.assertTrue(any(item['scope'] == 'repository' for item in explicit['graph']['external_symbols']), explicit)
        if self.PACK != 'c-cpp':
            self.assertTrue(any(item['scope'] == 'dependency' for item in explicit['graph']['external_symbols']), explicit)
        self.assertEqual(snapshot(self.root), before)
        self.assertTrue(len(discovered['graph']['nodes']) > len(explicit['graph']['nodes']))
        self.assertEqual(explicit['id'], '公开-é-😀')

    def test_malformed_byte_and_field_boundaries(self):
        """严格 UTF-8、递归重复键、单 JSON、大小写、控制帧和嵌套上限都不能被宽松转换。"""
        valid = json.dumps(self.request()).encode()
        payloads = [b'{"x":"\xff"}', b'{"id":"a","id":"b"}',
                    valid.replace(b'"include_tests": false', b'"include_tests": false,"include_tests":true'),
                    valid.replace(b'"id":', b'"ID":'), valid + b'\n{}', valid + b'\x1a',
                    valid.replace(b'"repository-only"', b'1'), valid.replace(b'false', b'"false"'),
                    valid.replace(b'"repository_root":', b'"sdk_root":"/untrusted","repository_root":'),
                    valid.replace(b'"files": []', b'"files":[],"nested":' + b'[' * 70 + b']' * 70),
                    valid.replace(b'"id": "', b'"id": "\\ud800'),
                    valid + b'\n{"protocol":"codegraph.analyzer/1","type":"cancel","id":"wrong"}',
                    valid + b'\n' + json.dumps({'protocol': 'codegraph.analyzer/1', 'type': 'cancel', 'id': '公开-é-😀'}).encode(),
                    json.dumps({**self.request(), 'id': '\u3000'}).encode()]
        for payload in payloads:
            with self.subTest(payload=payload[:100]):
                result = self.run_public(None, raw=payload)
                self.assertEqual(result['outcome'], 'error', result)
                self.assertEqual(result['error']['code'], 'invalid_request', result)
        for name in ['environment', 'input', 'limits', 'command', 'runtime', 'sdk_root']:
            result = self.run_public({**self.request(), name: {}})
            self.assertEqual(result['error']['code'], 'invalid_request', result)
        request = self.request()
        request['files'] = [f'file{i}.go' for i in range(100001)]
        self.assertEqual(self.run_public(request)['outcome'], 'error')

    def test_cli_and_source_installation_overlap(self):
        """固定文本命令与未知重复参数有明确状态；源码不能与安装目录相互包含。"""
        for args in [[], ['__worker'], ['analyze', 'analyze'], ['--pack', self.PACK], ['analyze', '--version']]:
            result = subprocess.run([str(self.ANALYZER), *args], capture_output=True, timeout=10)
            self.assertEqual(result.returncode, 2, result)
            self.assertEqual(result.stdout, b'')
        for args in [['--help'], ['analyze', '--help'], ['--version']]:
            self.assertEqual(subprocess.run([str(self.ANALYZER), *args], capture_output=True, timeout=10).returncode, 0)
        for root in [self.ANALYZER.resolve().parent, self.ANALYZER.resolve().parent.parent]:
            request = self.request()
            request['repository_root'] = str(root)
            self.assertEqual(self.run_public(request)['error']['code'], 'invalid_input')

    def test_eof_and_caller_tree_termination(self):
        """完整 JSON 后 EOF 正常完成；调用方在实际发现标记后终止进程树并回收管道。"""
        self.prepare(large=True)
        with tempfile.TemporaryDirectory(prefix='codegraph-discovery-probe-') as directory:
            marker = Path(directory) / 'reached.json'
            env = self.environment()
            env['CODEGRAPH_TEST_DISCOVERY_PROBE'] = str(marker)
            process = subprocess.Popen([str(self.ANALYZER), 'analyze'], stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                                       stderr=subprocess.PIPE, env=env, start_new_session=os.name != 'nt')
            try:
                process.stdin.write(json.dumps(self.request(discovery=True)).encode())
                process.stdin.close()
                process.stdin = None
                deadline = time.monotonic() + 20
                while not marker.exists() and process.poll() is None and time.monotonic() < deadline:
                    time.sleep(0.01)
                self.assertTrue(marker.exists(), 'actual configuration discovery marker missing')
                observation = json.loads(marker.read_text())
                self.assertEqual(observation['pid'], process.pid)
                self.assertIsNone(process.poll(), 'fixture must still be analyzing at caller termination')
                if os.name == 'nt':
                    subprocess.run([str(Path(os.environ['SystemRoot']) / 'System32/taskkill.exe'), '/PID', str(process.pid), '/T', '/F'], capture_output=True, check=True)
                else:
                    os.killpg(process.pid, signal.SIGKILL)
                stdout, stderr = process.communicate(timeout=10)
                self.assertNotEqual(process.returncode, 0)
                self.assertEqual(stdout, b'')
                self.assertLessEqual(len(stderr), 262144)
                if os.name != 'nt':
                    with self.assertRaises(ProcessLookupError):
                        os.killpg(process.pid, 0)
            finally:
                if process.poll() is None:
                    process.kill()
                    process.communicate(timeout=10)

    def test_source_boundary_and_missing_configuration(self):
        """缺失配置、普通文件根、越界链接和物理别名均明确失败。"""
        source = self.root / ('Only.cs' if self.PACK == 'csharp' else 'Only.go' if self.PACK == 'go' else 'Only.cpp')
        source.write_text('invalid source', encoding='utf-8')
        self.assertEqual(self.run_public(self.request([source.name]))['outcome'], 'error')
        if os.name != 'nt':
            outside = Path(self.temp.name) / 'outside'
            outside.write_text('outside', encoding='utf-8')
            (self.root / 'escape').symlink_to(outside)
            self.assertEqual(self.run_public(self.request(['escape']))['error']['code'], 'invalid_input')
            files = self.prepare()
            alias = 'alias' + Path(files[0]).suffix
            (self.root / alias).symlink_to(self.root / files[0])
            self.assertEqual(self.run_public(self.request([files[0], alias]))['outcome'], 'error')

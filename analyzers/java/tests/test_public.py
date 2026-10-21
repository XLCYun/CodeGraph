"""以真实 JVM 验证直接公共入口和固定身份。"""
import json
import pathlib
import subprocess
import tempfile
import unittest
from test_helper import PUBLIC_COMMAND


class PublicTests(unittest.TestCase):
    """公开程序只读取单个请求，EOF 后交付唯一最终结果。"""

    def test_empty_selection_skips_bad_configuration_and_target_discovery(self):
        """显式空集不读取坏配置，不需要目标 JDK，且不输出私有证据。"""
        with tempfile.TemporaryDirectory(prefix="codegraph-java-public-") as directory:
            root = pathlib.Path(directory)
            (root / "narrant-java.json").write_text("{broken", encoding="utf-8")
            request = {"protocol": "codegraph.analyzer/1", "type": "analyze", "id": "direct",
                       "repository_root": str(root), "files": [],
                       "options": {"include_tests": False, "rust_dependency_scope": "repository-only"}}
            result = subprocess.run([*PUBLIC_COMMAND, "analyze"], input=json.dumps(request), text=True,
                                    encoding="utf-8", capture_output=True, timeout=15,
                                    cwd=tempfile.gettempdir())
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertEqual(json.loads(result.stdout), {
                "protocol": "codegraph.analyzer/1", "engine_version": "0.1.0", "pack_id": "java",
                "type": "completed", "id": "direct", "outcome": "ok",
                "graph": {"root_contains": [], "nodes": [], "external_symbols": []}})
            self.assertEqual(result.stderr, "")


class PublicConformanceTests(unittest.TestCase):
    """共享公共反例、真实 JDK 发现与配置选集不依赖私有驱动。"""

    def setUp(self):
        """创建与分析器安装互不重叠的 Unicode 项目目录。"""
        self.temp = tempfile.TemporaryDirectory(prefix="codegraph Java 公共-")
        self.addCleanup(self.temp.cleanup)
        self.root = pathlib.Path(self.temp.name) / "仓库"
        self.root.mkdir()
        self.request = {"protocol": "codegraph.analyzer/1", "type": "analyze", "id": "public",
                        "repository_root": str(self.root), "files": [],
                        "options": {"include_tests": False, "rust_dependency_scope": "repository-only"}}

    def exchange(self, payload, args=None, environment=None):
        """完整输入后的 EOF 是正常会话边界，输出只能是唯一最终结果。"""
        import os
        from test_helper import TARGET
        env = dict(os.environ, CODEGRAPH_JAVA_TARGET_JDK=str(TARGET))
        if environment:
            env.update(environment)
        result = subprocess.run([*PUBLIC_COMMAND, *(args if args is not None else ["analyze"])],
                                input=payload, text=True, encoding="utf-8", capture_output=True,
                                timeout=20, cwd=tempfile.gettempdir(), env=env)
        if args is not None:
            return result
        message = json.loads(result.stdout)
        self.assertEqual(message["protocol"], "codegraph.analyzer/1")
        self.assertEqual(message["engine_version"], "0.1.0")
        self.assertEqual(message["pack_id"], "java")
        self.assertEqual(message["type"], "completed")
        self.assertNotIn("evidence", message)
        return result, message

    def test_shared_vectors(self):
        """所有语言共用的请求数据向量在真实 JVM 执行。"""
        from test_helper import REPO
        vectors = json.loads((REPO / "analyzers/common/src/tests/public-conformance.json").read_text())
        for vector in vectors:
            with self.subTest(vector=vector["name"]):
                request = dict(self.request, **vector["patch"])
                result, message = self.exchange(json.dumps(request) + vector["suffix"])
                self.assertEqual(result.returncode, 0 if vector["outcome"] == "ok" else 1, result.stderr)
                self.assertEqual(message["outcome"], vector["outcome"])
                if "error_code" in vector:
                    self.assertEqual(message["error"]["code"], vector["error_code"])
                    self.assertNotIn("graph", message)

    def test_malformed_and_duplicate_inputs(self):
        """空输入、过深、尾随内容及嵌套重复键都不接受请求 ID。"""
        text = json.dumps(self.request, separators=(",", ":"))
        values = ["", "[]", "{}", "{broken", '{"a":' * 70 + "0" + "}" * 70,
                  text.replace('"id":"public"', '"id":"discarded","id":"public"'),
                  text.replace('"id":"public"', '"i\\u0064":"discarded","id":"public"'),
                  text.replace('"include_tests":false', '"include_tests":true,"include_tests":false'),
                  text.replace('"files":[]', '"files":[],"extra":{"key":1,"key":2}')]
        for value in values:
            with self.subTest(value=value[:80]):
                result, message = self.exchange(value)
                self.assertEqual(result.returncode, 1)
                self.assertEqual(message["id"], None)
                self.assertEqual(message["error"]["code"], "invalid_request")

    def test_cli(self):
        """语言程序实现帮助和版本，重复和旧参数退出 2。"""
        for args in [["--help"], ["analyze", "--help"], ["--version"]]:
            result = self.exchange("", args=args)
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertIn("codegraph-java", result.stdout)
            self.assertEqual(result.stderr, "")
        for args in [[], ["__worker"], ["analyze", "analyze"], ["--version", "--version"], ["analyze", "--pack-id", "java"]]:
            result = self.exchange("", args=args)
            self.assertEqual(result.returncode, 2)
            self.assertEqual(result.stdout, "")

    def test_discovery_selection_external_and_read_only(self):
        """原生配置发现、精确选集、仓内图外引用与测试开关共用真实 JDT。"""
        from test_helper import metadata, write, snapshot_files
        metadata(self.root, tests=True)
        write(self.root, "src/main/java/p/Main.java", "package p; class Main { Object run(){ Helper.ping(); return java.util.List.of(1); } }")
        write(self.root, "src/main/java/p/Helper.java", "package p; class Helper { static void ping(){} }")
        write(self.root, "src/test/java/p/MainTest.java", "package p; class MainTest {}")
        write(self.root, "stray/Stray.java", "class Stray {}")
        before = snapshot_files(self.root)
        del self.request["files"]
        result, discovered = self.exchange(json.dumps(self.request))
        self.assertEqual(result.returncode, 0, discovered)
        self.assertTrue(any(n["name"] == "Helper" for n in discovered["graph"]["nodes"]))
        self.assertFalse(any(n["name"] in ["Stray", "MainTest"] for n in discovered["graph"]["nodes"]))
        self.request["files"] = ["src/main/java/p/Main.java"]
        result, selected = self.exchange(json.dumps(self.request))
        self.assertEqual(result.returncode, 0, selected)
        self.assertFalse(any(n["name"] == "Helper" for n in selected["graph"]["nodes"]))
        self.assertTrue(any(symbol["scope"] == "repository" for symbol in selected["graph"]["external_symbols"]))
        self.assertTrue(any(symbol["scope"] == "dependency" for symbol in selected["graph"]["external_symbols"]))
        self.request["files"] = ["src/test/java/p/MainTest.java"]
        self.request["options"]["include_tests"] = True
        self.assertTrue(any(n["name"] == "MainTest" for n in self.exchange(json.dumps(self.request))[1]["graph"]["nodes"]))
        self.assertEqual(snapshot_files(self.root), before)

    def test_sdk_discovery_component_policy(self):
        """同进程发现器沿用多 major、最高稳定补丁及标准库布局规则。"""
        from test_helper import COMMAND
        result = subprocess.run([*COMMAND[:-1], "dev.codegraph.java.SdkDiscoveryTest"],
                                capture_output=True, text=True, encoding="utf-8", timeout=15)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("SDK discovery assertions passed", result.stdout)

    def test_graph_boundary_rejects_broken_results(self):
        """同进程输出边界拒绝契约反例，不修改或自动修补图。"""
        from test_helper import COMMAND
        result = subprocess.run([*COMMAND[:-1], "dev.codegraph.java.GraphChecksTest"], capture_output=True,
                                text=True, encoding="utf-8", timeout=15)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("graph boundary assertions passed", result.stdout)

    def test_private_envelope_unicode_and_limits(self):
        """私有上下文、过长 UTF-8 ID、代理项和超大选集都在公共边界拒绝。"""
        values = [dict(self.request, id="源" * 400), dict(self.request, id="\ud800"),
                  dict(self.request, files=[f"f{index}.java" for index in range(100001)]),
                  {"protocol": "codegraph.analyzer/1", "type": "analyze", "id": "private",
                   "input": {}, "environment": {}, "limits": {}}]
        for request in values:
            result, message = self.exchange(json.dumps(request))
            self.assertEqual(result.returncode, 1)
            self.assertEqual(message["id"], None)
            self.assertEqual(message["error"]["code"], "invalid_request")

    def test_public_output_is_utf8_under_non_utf8_runtime_default(self):
        """协议结果按 UTF-8 字节写出，不依赖 JVM 默认字符集或 Windows 代码页。"""
        self.request["id"] = "来源丁"
        command = [PUBLIC_COMMAND[0], "-Dfile.encoding=US-ASCII", "-Dstdout.encoding=US-ASCII", *PUBLIC_COMMAND[1:], "analyze"]
        result = subprocess.run(command, input=json.dumps(self.request).encode("utf-8"), capture_output=True, timeout=10)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(json.loads(result.stdout.decode("utf-8"))["id"], "来源丁")

    def test_invalid_utf8(self):
        """无法解码的字节不能通过平台默认字符集替换为有效请求。"""
        result = subprocess.run([*PUBLIC_COMMAND, "analyze"], input=b"\xff", capture_output=True, timeout=10)
        self.assertEqual(result.returncode, 1)
        self.assertEqual(json.loads(result.stdout)["error"]["code"], "invalid_request")

    def test_physical_alias_and_source_overlap(self):
        """Java 保留物理来源唯一和安装不重叠边界。"""
        import os
        from test_helper import HELPER, metadata, write
        self.request["repository_root"] = str(HELPER)
        result, message = self.exchange(json.dumps(self.request))
        self.assertEqual(result.returncode, 1)
        self.assertEqual(message["error"]["code"], "invalid_input")
        self.request["repository_root"] = str(self.root)
        metadata(self.root)
        write(self.root, "src/main/java/A.java", "class A {}")
        try:
            (self.root / "alias").symlink_to(self.root / "src/main/java", target_is_directory=True)
        except OSError as error:
            if os.name == "nt":
                self.skipTest(f"Windows symlink privilege unavailable: {error}")
            raise
        self.request["files"] = ["src/main/java/A.java", "alias/A.java"]
        result, message = self.exchange(json.dumps(self.request))
        self.assertEqual(result.returncode, 1)
        self.assertEqual(message["error"]["code"], "invalid_input")


if __name__ == "__main__":
    unittest.main()

"""用真实 JDT 验证配置驱动的目录发现与精确显式选集。"""

import json
import os
import pathlib
import sys
import tempfile
import unittest

from test_helper import metadata, request, run, snapshot_files, write


def discover(root, **options):
    """省略私有 files 字段，保持其余协议字段与显式模式相同。"""
    message = request(root, [], **options)
    del message["input"]["files"]
    return message


class DiscoveryTests(unittest.TestCase):
    """覆盖源码根、模块边界、空配置和发现阶段的有界生命周期。"""

    def setUp(self):
        """使用包含 URI 特殊字符及 Unicode 的可移植仓库路径。"""
        self.temp = tempfile.TemporaryDirectory(prefix="codegraph-java-discovery-")
        self.addCleanup(self.temp.cleanup)
        self.root = pathlib.Path(self.temp.name) / "repository # % 中文"
        self.root.mkdir()

    def test_roots_roles_explicit_and_readonly(self):
        """只选声明根；测试角色来自配置，显式文件不扩展到引用对象。"""
        metadata(self.root, tests=True)
        write(self.root, "src/main/java/p/Main.java", "package p; class Main { int call(){return Helper.value();} }")
        write(self.root, "src/main/java/p/Helper.java", "package p; class Helper { static int value(){return 1;} }")
        write(self.root, "src/main/java/p/RealTest.java", "package p; class RealTest {}")
        write(self.root, "src/test/java/p/Checks.java", "package p; class Checks { Main subject; }")
        write(self.root, "other/Stray.java", "this is deliberately invalid")
        before = snapshot_files(self.root)
        result = run(discover(self.root))
        self.assertEqual(result["outcome"], "ok", result)
        files = {item["path"]: item["disposition"] for item in result["evidence"]["files"]}
        self.assertEqual(files, {
            "src/main/java/p/Main.java": "analyzed", "src/main/java/p/Helper.java": "analyzed",
            "src/main/java/p/RealTest.java": "analyzed", "src/test/java/p/Checks.java": "excluded_test"})
        self.assertTrue(any(node["name"] == "RealTest" for node in result["graph"]["nodes"]))
        self.assertFalse(any(node["id"].startswith("java:src/test/") for node in result["graph"]["nodes"]))
        included = run(discover(self.root, include_tests=True))
        self.assertEqual(included["outcome"], "ok", included)
        self.assertTrue(all(item["disposition"] == "analyzed" for item in included["evidence"]["files"]))
        explicit = run(request(self.root, ["src/main/java/p/Main.java"]))
        self.assertEqual(explicit["outcome"], "ok", explicit)
        self.assertEqual([item["path"] for item in explicit["evidence"]["files"]], ["src/main/java/p/Main.java"])
        self.assertTrue(any(item["scope"] == "repository" for item in explicit["graph"]["external_symbols"]))
        self.assertEqual(before, snapshot_files(self.root))

    def test_pom_declared_modules_and_custom_roots(self):
        """聚合 POM 只接纳声明模块，父链与自定义源码根保持实际归属。"""
        write(self.root, "pom.xml", """<project><modelVersion>4.0.0</modelVersion><groupId>fixture</groupId>
<artifactId>root</artifactId><version>1</version><packaging>pom</packaging>
<properties><maven.compiler.release>17</maven.compiler.release></properties>
<modules><module>app</module><module>lib</module></modules></project>""")
        parent = "<parent><groupId>fixture</groupId><artifactId>root</artifactId><version>1</version></parent>"
        for module in ("app", "lib"):
            write(self.root, module + "/pom.xml", "<project>" + parent + "<artifactId>" + module + "</artifactId><build><sourceDirectory>code</sourceDirectory><testSourceDirectory>checks</testSourceDirectory></build></project>")
            write(self.root, module + "/code/Unit.java", "class Unit {}")
        write(self.root, "app/checks/UnitChecks.java", "class UnitChecks {}")
        write(self.root, "app/src/main/java/Ignored.java", "invalid")
        write(self.root, "undeclared/pom.xml", "invalid")
        write(self.root, "undeclared/src/main/java/Ignored.java", "invalid")
        result = run(discover(self.root))
        self.assertEqual(result["outcome"], "ok", result)
        self.assertEqual({item["path"] for item in result["evidence"]["files"]},
                         {"app/code/Unit.java", "lib/code/Unit.java", "app/checks/UnitChecks.java"})
        self.assertTrue(any(item["source_paths"] == ["pom.xml", "app/pom.xml"]
                            for item in result["evidence"]["configurations"]))

    def test_empty_missing_invalid_and_null(self):
        """空数组不读取项目或 SDK；空 POM 保留配置；省略字段仍要求有效配置。"""
        message = request(self.root, [], platforms=[])
        empty = run(message)
        self.assertEqual(empty["outcome"], "ok", empty)
        self.assertEqual(empty["evidence"], {"configurations": [], "files": [], "origins": []})
        message["environment"]["platforms"] = None
        self.assertEqual(run(message)["error"]["code"], "invalid_input")
        message["environment"]["platforms"] = []
        self.assertEqual(run(discover(self.root))["error"]["code"], "unsupported_configuration")
        for invalid in (None, "file.java", {}, [None]):
            message["input"]["files"] = invalid
            self.assertEqual(run(message)["error"]["code"], "invalid_input")
        write(self.root, "pom.xml", "<project><groupId>fixture</groupId><artifactId>empty</artifactId><version>1</version><properties><maven.compiler.release>17</maven.compiler.release></properties></project>")
        result = run(discover(self.root))
        self.assertEqual(result["outcome"], "ok", result)
        self.assertEqual(result["evidence"]["files"], [])
        self.assertEqual(len(result["evidence"]["configurations"]), 1)
        self.assertEqual(run(discover(self.root, platforms=[]))["error"]["code"], "target_unavailable")
        write(self.root, "pom.xml", "<invalid")
        self.assertEqual(run(discover(self.root))["error"]["code"], "unsupported_configuration")

    def test_overlapping_roles_keep_active_selection_and_excluded_evidence(self):
        """重叠根保持生产归属，跨配置测试选集保留原配置证据与仓内上下文。"""
        main = "shared/p/Main.java"
        helper = "shared/p/Helper.java"
        excluded = "tests/p/Only.java"
        write(self.root, main, "package p; class Main { void run(){ Helper.ping(); } }")
        write(self.root, helper, "package p; class Helper { static void ping(){} }")
        write(self.root, excluded, "package p; class Only {}")
        configs = [
            {"id": "production", "release": "17", "source_roots": [
                {"path": "shared", "role": "main"}, {"path": "shared/p", "role": "test"},
                {"path": "tests", "role": "test"}]},
            {"id": "testing", "release": "17", "source_roots": [
                {"path": "shared", "role": "test"}, {"path": "tests", "role": "test"}]},
        ]
        write(self.root, "narrant-java.json", json.dumps({"schema": "java-static-v1", "configurations": configs}))
        before = snapshot_files(self.root)
        for include_tests in (False, True):
            with self.subTest(include_tests=include_tests):
                result = run(request(self.root, [main, excluded], include_tests=include_tests))
                self.assertEqual(result["outcome"], "ok", result)
                evidence = {item["path"]: item for item in result["evidence"]["files"]}
                all_ids = ["java-config:metadata:production", "java-config:metadata:testing"]
                self.assertEqual(evidence[main]["configuration_ids"], all_ids if include_tests else all_ids[:1])
                self.assertEqual(evidence[main]["disposition"], "analyzed")
                self.assertEqual(evidence[excluded]["configuration_ids"], all_ids)
                self.assertEqual(evidence[excluded]["disposition"], "analyzed" if include_tests else "excluded_test")
                paths = {origin["selected_paths"][0] for origin in result["evidence"]["origins"]}
                self.assertEqual(paths, {main, excluded} if include_tests else {main})
                self.assertFalse(any(node["id"].startswith("java:" + helper) for node in result["graph"]["nodes"]))
                self.assertTrue(any(item["scope"] == "repository" and "Helper" in item["symbol"]
                                    for item in result["graph"]["external_symbols"]))
        only_tests = run(request(self.root, [excluded]))
        self.assertEqual(only_tests["outcome"], "ok", only_tests)
        self.assertEqual(only_tests["graph"], {"root_contains": [], "nodes": [], "external_symbols": []})
        self.assertEqual(only_tests["evidence"]["files"], [{"path": excluded, "disposition": "excluded_test",
                         "configuration_ids": ["java-config:metadata:production", "java-config:metadata:testing"]}])
        self.assertEqual(snapshot_files(self.root), before)

    def test_discovery_timeout(self):
        """省略选集时仍遵守加载阶段内部期限，取消由公开入口生命周期测试覆盖。"""
        metadata(self.root)
        for index in range(1200):
            write(self.root, f"src/main/java/C{index}.java", f"class C{index} {{}}")
        result = run(discover(self.root, timeout=1))
        self.assertEqual(result["error"]["code"], "resource_limit", result)

    @unittest.skipUnless(os.name == "posix", "requires POSIX directory traversal permissions")
    def test_inaccessible_source_root_ancestor_is_not_empty(self):
        """已存在源码根的祖先不可访问时必须报错，不能伪装成缺失目录。"""
        write(self.root, "pom.xml", "<project><groupId>fixture</groupId><artifactId>unreadable</artifactId><version>1</version><properties><maven.compiler.release>17</maven.compiler.release></properties><build><sourceDirectory>blocked/code</sourceDirectory></build></project>")
        write(self.root, "blocked/code/Hidden.java", "class Hidden {}")
        blocked = self.root / "blocked"
        original_mode = blocked.stat().st_mode & 0o7777
        blocked.chmod(0)
        try:
            try:
                (blocked / "code").stat()
            except PermissionError:
                pass
            else:
                self.skipTest("current privileges or filesystem do not enforce POSIX traversal permissions")
            result = run(discover(self.root))
            self.assertEqual(result["outcome"], "error", result)
            self.assertEqual(result["error"]["code"], "unsupported_configuration", result)
            self.assertNotIn("graph", result)
        finally:
            blocked.chmod(original_mode)

    @unittest.skipIf(sys.platform == "win32", "symlink creation needs Windows privileges")
    def test_source_root_cannot_escape_repository(self):
        """POM 源码根在枚举前验证，不沿外向目录链接发现源码。"""
        outside = self.root.parent / "external"
        outside.mkdir()
        write(outside, "Secret.java", "class Secret {}")
        (self.root / "code").symlink_to(outside, target_is_directory=True)
        write(self.root, "pom.xml", "<project><groupId>fixture</groupId><artifactId>bad</artifactId><version>1</version><properties><maven.compiler.release>17</maven.compiler.release></properties><build><sourceDirectory>code</sourceDirectory></build></project>")
        result = run(discover(self.root))
        self.assertEqual(result["error"]["code"], "unsupported_configuration", result)

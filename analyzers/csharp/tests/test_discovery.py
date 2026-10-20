"""真实 Roslyn 目录发现回归，复用既有协议驱动与固定引用夹具。"""

import hashlib
import json
import subprocess
import unittest

import test_helper as fixtures


class DiscoveryTests(unittest.TestCase):
    """验证原生 Compile 选集、测试角色、条件配置与发现阶段取消。"""

    setUp = fixtures.HelperTests.setUp
    write = fixtures.HelperTests.write
    source_set = fixtures.HelperTests.source_set
    config = fixtures.HelperTests.config
    platform = fixtures.HelperTests.platform
    request = fixtures.HelperTests.request
    run_helper = fixtures.HelperTests.run_helper

    def discovery(self, **options):
        """只省略 files 字段，确保 [] 仍为明确空选集。"""
        message = self.request([], **options)
        del message["input"]["files"]
        return message

    def test_source_set_roles_explicit_and_readonly(self):
        """source-set 的列举成员为发现边界，生产 Test 后缀不推测为测试。"""
        self.write("Main.cs", "class Main { int Call()=>Helper.Value(); }")
        self.write("Helper.cs", "class Helper { public static int Value()=>1; }")
        self.write("RealTest.cs", "class RealTest {}")
        self.write("Checks.cs", "class Checks {}")
        self.write("Stray.cs", "invalid source outside configuration")
        production = self.config("main", ["Main.cs", "Helper.cs", "RealTest.cs"])
        tests = {**self.config("tests", ["Checks.cs"]), "test_only": True}
        self.source_set([production, tests])
        before = {p.relative_to(self.root).as_posix(): hashlib.sha256(p.read_bytes()).hexdigest()
                  for p in self.root.rglob("*") if p.is_file()}
        result = self.run_helper(self.discovery())
        self.assertEqual(result["outcome"], "ok", result)
        self.assertEqual({item["path"]: item["disposition"] for item in result["evidence"]["files"]},
                         {"Main.cs": "analyzed", "Helper.cs": "analyzed", "RealTest.cs": "analyzed", "Checks.cs": "excluded_test"})
        self.assertNotIn("Checks", [n["name"] for n in result["graph"]["nodes"]])
        included = self.run_helper(self.discovery(include_tests=True))
        self.assertEqual(included["outcome"], "ok", included)
        self.assertTrue(all(item["disposition"] == "analyzed" for item in included["evidence"]["files"]))
        explicit = self.run_helper(self.request(["Main.cs"]))
        self.assertEqual(explicit["outcome"], "ok", explicit)
        self.assertEqual([item["path"] for item in explicit["evidence"]["files"]], ["Main.cs"])
        self.assertTrue(any(item["scope"] == "repository" for item in explicit["graph"]["external_symbols"]))
        self.assertEqual(before, {p.relative_to(self.root).as_posix(): hashlib.sha256(p.read_bytes()).hexdigest()
                                 for p in self.root.rglob("*") if p.is_file()})

    def test_project_default_compile_include_remove(self):
        """SDK 默认项、字面 Include 和 Remove 决定成员，仓库其余源码不加入。"""
        self.write("App/App.csproj", '<Project Sdk="Microsoft.NET.Sdk"><PropertyGroup><TargetFramework>net10.0</TargetFramework></PropertyGroup><ItemGroup><Compile Remove="Drop.cs"/><Compile Include="../Shared.cs"/></ItemGroup></Project>')
        self.write("App/Main.cs", "class Main {}")
        self.write("Shared.cs", "class Shared {}")
        for path in ("App/Drop.cs", "App/bin/Bad.cs", "App/obj/Bad.cs", "App/.hidden/Bad.cs", "Other/Stray.cs"):
            self.write(path, "invalid excluded source")
        self.write("App/obj/Bad.csproj", "invalid generated project")
        result = self.run_helper(self.discovery())
        self.assertEqual(result["outcome"], "ok", result)
        self.assertEqual({item["path"] for item in result["evidence"]["files"]}, {"App/Main.cs", "Shared.cs"})

    def test_configuration_and_tfm_compile_union(self):
        """选集包含真实配置并集，永不适用的条件 Include 不进入发现结果。"""
        self.write("App.csproj", """<Project Sdk="Microsoft.NET.Sdk"><PropertyGroup><TargetFrameworks>net8.0;net10.0</TargetFrameworks><EnableDefaultCompileItems>false</EnableDefaultCompileItems></PropertyGroup><ItemGroup>
<Compile Include="Base.cs"/><Compile Include="Eight.cs" Condition="'$(TargetFramework)' == 'net8.0'"/>
<Compile Include="Ten.cs" Condition="'$(TargetFramework)' == 'net10.0'"/>
<Compile Include="Debug.cs" Condition="'$(Configuration)' == 'Debug'"/>
<Compile Include="Release.cs" Condition="'$(Configuration)' == 'Release'"/>
<Compile Include="Never.cs" Condition="'$(TargetFramework)' == 'net99.0'"/>
</ItemGroup></Project>""")
        for name in ("Base", "Eight", "Ten", "Debug", "Release", "Never"):
            self.write(name + ".cs", "class " + name + " {}")
        result = self.run_helper(self.discovery(platforms=[self.platform(), self.platform("net8.0", "8.0.0", fixtures.REFS8)]))
        self.assertEqual(result["outcome"], "ok", result)
        self.assertEqual({item["path"] for item in result["evidence"]["files"]},
                         {name + ".cs" for name in ("Base", "Eight", "Ten", "Debug", "Release")})
        self.assertEqual(len(result["evidence"]["configurations"]), 4)
        self.assertTrue(all(item["disposition"] == "analyzed" for item in result["evidence"]["files"]))

    def test_empty_missing_and_invalid_configuration(self):
        """明确空选集不需要配置或引用；有效空配置必须返回其来源。"""
        result = self.run_helper(self.request([], platforms=[]))
        self.assertEqual(result["outcome"], "ok", result)
        self.assertEqual(result["evidence"], {"configurations": [], "files": [], "origins": []})
        self.assertEqual(self.run_helper(self.discovery())["error"]["code"], "unsupported_configuration")
        self.source_set([self.config("empty", [])])
        result = self.run_helper(self.discovery())
        self.assertEqual(result["outcome"], "ok", result)
        self.assertEqual(result["evidence"]["files"], [])
        self.assertEqual(len(result["evidence"]["configurations"]), 1)
        self.assertEqual(self.run_helper(self.discovery(platforms=[]))["error"]["code"], "target_unavailable")
        self.write("narrant.csharp.json", '{"version":"unsupported","configurations":[]}')
        self.assertEqual(self.run_helper(self.discovery())["error"]["code"], "unsupported_configuration")

    def test_null_and_nonarray_selection_rejected(self):
        """缺省、null 和数组不能在协议解析时混为一谈。"""
        for files in (None, "Main.cs", {}, [None]):
            result = subprocess.run([fixtures.DOTNET, str(fixtures.HELPER)], input=json.dumps(self.request(files)) + "\n",
                                    text=True, capture_output=True, timeout=10)
            self.assertEqual(result.returncode, 1, result)
            self.assertIn("Invalid analyze message", result.stderr)

    def test_discovery_cancellation_and_timeout(self):
        """大量目录枚举和项目加载也遵守取消与总分析期限。"""
        self.write("App.csproj", '<Project Sdk="Microsoft.NET.Sdk"><PropertyGroup><TargetFramework>net10.0</TargetFramework></PropertyGroup></Project>')
        for index in range(1200):
            self.write(f"source/C{index}.cs", f"class C{index} {{}}")
        cancel = json.dumps({"protocol": "codegraph.engine/1", "type": "cancel", "id": "fixture"})
        result = self.run_helper(self.discovery(), extra_line=cancel)
        self.assertEqual(result["outcome"], "cancelled", result)
        result = self.run_helper(self.discovery(timeout=1))
        self.assertEqual(result["error"]["code"], "resource_limit", result)

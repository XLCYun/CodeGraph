"""公共 C/C++ 入口以原始源码覆盖为先，语义上下文缺失不得丢弃审查内容。"""
import json
import os
from pathlib import Path
import subprocess
import tempfile
import unittest


class SourceReviewTests(unittest.TestCase):
    """直接启动当前原生分析器，验证无构建步骤的源码、宏节点和比较内容。"""

    @classmethod
    def setUpClass(cls):
        """原生验收显式提供真实入口，不以其他解析器的结果代替生产程序。"""
        entry = os.environ.get("CODEGRAPH_CLANG_ANALYZER")
        if not entry:
            raise unittest.SkipTest("requires CODEGRAPH_CLANG_ANALYZER native public entry")
        cls.entry = Path(entry).resolve()
        if not cls.entry.is_file():
            raise AssertionError("native public entry does not exist")

    def setUp(self):
        """每例使用独立源码根，不改真实仓库或应用数据。"""
        self.temporary = tempfile.TemporaryDirectory(prefix="codegraph-source-review-")
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)

    def put(self, path, source):
        """只在测试根写入源码或静态配置。"""
        target = self.root / path
        target.parent.mkdir(parents=True, exist_ok=True)
        target.write_bytes(source.encode("utf-8"))

    def analyze(self, files, *, discover=False, environment=None):
        """通过一次 JSON/EOF 公共调用返回完整图，并检查分析没有改写输入文件。"""
        before = {str(p.relative_to(self.root)): p.read_bytes()
                  for p in self.root.rglob("*") if p.is_file()}
        request = {
            "protocol": "codegraph.analyzer/1", "type": "analyze", "id": "source-review",
            "repository_root": str(self.root),
            "options": {"include_tests": False, "rust_dependency_scope": "repository-only"},
        }
        if not discover:
            request["files"] = files
        result = subprocess.run(
            [str(self.entry), "analyze"], input=json.dumps(request), encoding="utf-8",
            capture_output=True, timeout=30, env=environment,
        )
        response = json.loads(result.stdout)
        self.assertEqual(result.returncode, 0, response.get("error"))
        self.assertEqual(response.get("outcome"), "ok", response)
        after = {str(p.relative_to(self.root)): p.read_bytes()
                 for p in self.root.rglob("*") if p.is_file()}
        self.assertEqual(before, after, "analysis must remain read-only")
        return response["graph"]

    def test_cpp_without_database_creates_source_graph(self):
        """只给目录与选集即可提取普通函数，文件节点保留全部原文。"""
        source = "// original comment\nint value(int x) { return x + 1; }\n"
        self.put("main.cpp", source)
        graph = self.analyze(["main.cpp"])
        self.assertTrue(any(n.get("name") == "value" and n["category"] == "callable"
                            for n in graph["nodes"]))
        self.assertTrue(any(n["native_kind"] == "SourceFile" and n["source"] == source
                            for n in graph["nodes"]))

    def test_conditional_block_preserves_all_branches_as_one_node(self):
        """条件真假不决定覆盖范围；整块原文保留而不提取内部声明。"""
        source = ("#define SELECTED 1\n#if SELECTED\nint enabled(){return 1;}\n"
                  "#else\nint disabled(){return 2;}\n#endif\n")
        self.put("main.cpp", source)
        graph = self.analyze(["main.cpp"])
        blocks = [n for n in graph["nodes"] if n["native_kind"] == "ConditionalBlock"]
        self.assertEqual(len(blocks), 1)
        self.assertIn("int enabled()", blocks[0]["source"])
        self.assertIn("int disabled()", blocks[0]["source"])
        self.assertFalse(any(n.get("name") in {"enabled", "disabled"} for n in graph["nodes"]))

    def test_crlf_source_is_exact_and_line_endings_do_not_change_content(self):
        """UTF-8 与 CRLF 原文字节保留，只有换行格式变化不产生自身内容差异。"""
        source = "#ifndef BOX_H\n#define BOX_H\n// 中文注释\nstruct Box { int value(){return 1;} };\n#endif\n"
        self.put("box.h", source)
        before = self.analyze(["box.h"])
        crlf = source.replace("\n", "\r\n")
        self.put("box.h", crlf)
        self.assertEqual((self.root / "box.h").read_bytes(), crlf.encode("utf-8"))
        after = self.analyze(["box.h"])
        file = next(n for n in after["nodes"] if n["native_kind"] == "SourceFile")
        self.assertEqual(file["source"], crlf)
        self.assertEqual({n["id"]: (n["code"], n["signature"]) for n in before["nodes"]},
                         {n["id"]: (n["code"], n["signature"]) for n in after["nodes"]})

    def test_include_guard_keeps_inner_structure(self):
        """标准头文件保护只遮盖保护指令，不将整个头文件吞成一个节点。"""
        source = ("#ifndef BOX_H\n#define BOX_H\nclass Box { public: int value(){return 1;} };\n"
                  "#endif // BOX_H\n")
        self.put("box.h", source)
        graph = self.analyze(["box.h"])
        self.assertTrue(any(n.get("name") == "Box" and n["category"] == "type"
                            for n in graph["nodes"]))
        self.assertTrue(any(n.get("name") == "value" for n in graph["nodes"]))
        self.assertFalse(any(n["native_kind"] == "ConditionalBlock" for n in graph["nodes"]))
        directives = [n for n in graph["nodes"] if n["native_kind"] == "PreprocessorDirective"]
        self.assertEqual(len(directives), 3)

    def test_directive_continuation_and_raw_string_boundaries(self):
        """续行属于同一指令，字符串中的井号与条件字样不是预处理指令。"""
        source = ('#pragma once\n#define SUM(a,b) ((a) + \\\n(b))\n'
                  '#include "missing.hpp"\nconst char *text=R"tag(\n#if FAKE\n#endif\n)tag";\n')
        self.put("main.cpp", source)
        graph = self.analyze(["main.cpp"])
        directives = [n for n in graph["nodes"] if n["native_kind"] == "PreprocessorDirective"]
        self.assertEqual(len(directives), 3)
        self.assertTrue(any("SUM(a,b)" in n["source"] and "(b))" in n["source"] for n in directives))
        self.assertFalse(any(n["native_kind"] == "ConditionalBlock" for n in graph["nodes"]))

    def test_missing_headers_and_types_preserve_source(self):
        """缺少语义上下文时保留原文与可确认的函数结构。"""
        source = '#include "not-installed.hpp"\nUnknown run(Unknown input) { missing_call(); return input; }\n'
        self.put("main.cpp", source)
        graph = self.analyze(["main.cpp"])
        self.assertTrue(any(n["native_kind"] == "SourceFile" and n["source"] == source
                            for n in graph["nodes"]))
        self.assertTrue(any(n.get("name") == "run" and n["category"] == "callable"
                            for n in graph["nodes"]))
        unresolved = [r["target"] for n in graph["nodes"] for r in n["references"]
                      if r["target"]["kind"] == "unresolved"]
        self.assertTrue(any("missing_call" in t["name"] for t in unresolved))

    def test_unknown_field_type_keeps_confirmed_declaration_structure(self):
        """字段语法明确时保留字段对象，不把未知类型替换成编译器恢复出的 int。"""
        source = "class Box { Unknown value; };\n"
        self.put("main.cpp", source)
        graph = self.analyze(["main.cpp"])
        field = next(n for n in graph["nodes"] if n.get("name") == "value")
        self.assertEqual(field["category"], "value")
        self.assertIn("Unknown", field["signature"])

    def test_standard_command_database_enriches_definition_binding(self):
        """标准 command 格式只作为数据使用，绑定应指向唯一有源码定义的对象。"""
        self.put("main.cpp", "int target();\nint caller(){return target();}\nint target(){return 1;}\n")
        self.put("compile_commands.json", json.dumps([{
            "directory": str(self.root), "file": "main.cpp",
            "command": 'clang++ -std=c++17 -c "main.cpp"',
        }]))
        graph = self.analyze(["main.cpp"])
        caller = next(n for n in graph["nodes"] if n.get("name") == "caller")
        definition = next(n for n in graph["nodes"] if n.get("name") == "target" and "return" in n["code"])
        self.assertTrue(any(r["semantic"] == "code.calls" and r["target"] == {"kind": "node", "id": definition["id"]}
                            for r in caller["references"]))

    def test_macro_invocations_stay_raw_without_expansion_relationships(self):
        """宏调用留在所属函数原文中，可用编译上下文也不得导出展开后的调用。"""
        self.put("main.cpp", "#define INVOKE() target()\nint target(){return 1;}\nint caller(){return INVOKE();}\n")
        self.put("compile_commands.json", json.dumps([{
            "directory": str(self.root), "file": "main.cpp",
            "arguments": ["clang++", "-std=c++17", "main.cpp"],
        }]))
        graph = self.analyze(["main.cpp"])
        caller = next(n for n in graph["nodes"] if n.get("name") == "caller")
        target = next(n for n in graph["nodes"] if n.get("name") == "target")
        self.assertIn("INVOKE()", caller["source"])
        self.assertFalse(any(r["target"] == {"kind": "node", "id": target["id"]}
                             for r in caller["references"]))

    def test_context_binds_selected_definition_instead_of_unselected_prototype(self):
        """跨文件定义属于选集时，可靠 USR 绑定优先于图外头文件 prototype。"""
        self.put("api.h", "int target();\nint outside();\n")
        self.put("main.cpp", '#include "api.h"\nint caller(){return target()+outside();}\n')
        self.put("target.cpp", '#include "api.h"\nint target(){return 1;}\n')
        self.put("outside.cpp", '#include "api.h"\nint outside(){return 2;}\n')
        self.put("compile_commands.json", json.dumps([{
            "directory": str(self.root), "file": file,
            "arguments": ["clang++", "-std=c++20", file],
        } for file in ["main.cpp", "target.cpp", "outside.cpp"]]))
        graph = self.analyze(["main.cpp", "target.cpp"])
        caller = next(n for n in graph["nodes"] if n.get("name") == "caller")
        target = next(n for n in graph["nodes"] if n.get("name") == "target")
        self.assertTrue(any(r["target"] == {"kind": "node", "id": target["id"]}
                            for r in caller["references"]))
        outside = next(n for n in graph["external_symbols"] if n["name"] == "outside")
        self.assertEqual(outside["scope"], "repository")

    def test_guard_exception_does_not_open_nested_or_alternative_conditions(self):
        """仅外层标准保护例外，内部条件及带 else 的保护外观仍整体不透明。"""
        source = ("#pragma once\n#if !defined(BOX_H)\n#define BOX_H\n"
                  "int visible();\n#if FEATURE\nint first();\n#else\nint second();\n#endif\n#endif\n")
        self.put("box.h", source)
        graph = self.analyze(["box.h"])
        self.assertTrue(any(n.get("name") == "visible" for n in graph["nodes"]))
        self.assertEqual(sum(n["native_kind"] == "ConditionalBlock" for n in graph["nodes"]), 1)
        self.assertFalse(any(n.get("name") in {"first", "second"} for n in graph["nodes"]))
        self.put("box.h", source.replace("\n#endif\n#endif\n", "\n#endif\n#else\nint alternative();\n#endif\n"))
        graph = self.analyze(["box.h"])
        self.assertFalse(any(n.get("name") == "visible" for n in graph["nodes"]))

    def test_error_directive_and_missing_context_do_not_hide_later_source(self):
        """错误指令作为原文节点保留，不阻止后续普通声明。"""
        self.put("main.cpp", '#error missing configuration\nint visible(){return 1;}\n')
        graph = self.analyze(["main.cpp"])
        self.assertTrue(any(n.get("name") == "visible" for n in graph["nodes"]))
        self.assertTrue(any(n["native_kind"] == "PreprocessorDirective" and "#error" in n["source"]
                            for n in graph["nodes"]))

    def test_optional_context_does_not_change_node_content(self):
        """编译上下文只增强引用，不把上下文可用状态写入节点的比较内容。"""
        self.put("main.cpp", "int target(){return 1;}\nint caller(){return target();}\n")
        before = self.analyze(["main.cpp"])
        self.put("compile_commands.json", json.dumps([{
            "directory": str(self.root), "file": "main.cpp",
            "arguments": ["clang++", "-std=c++17", "main.cpp"],
        }]))
        after = self.analyze(["main.cpp"])
        def content(graph):
            """引用以外的图内容应与提取上下文无关。"""
            return {n["id"]: (n["signature"], n["code"], n["extensions"]) for n in graph["nodes"]}
        self.assertEqual(content(before), content(after))

    def test_unrecognized_declaration_survives_as_opaque_source(self):
        """无法确认的声明不消失，也不阻止后续普通函数提取。"""
        source = "UNRECOGNIZED_DECL(widget);\nint stable(){return 1;}\n"
        self.put("main.cpp", source)
        graph = self.analyze(["main.cpp"])
        self.assertTrue(any(n["category"] == "extension" and "UNRECOGNIZED_DECL" in n["source"]
                            for n in graph["nodes"]))
        self.assertTrue(any(n.get("name") == "stable" for n in graph["nodes"]))

    def test_inactive_branch_change_participates_in_comparison(self):
        """不透明节点的实现内容必须变化，不能只更新不参与差异的 source 字段。"""
        source = "#if 0\nint hidden(){return 2;}\n#else\nint visible(){return 1;}\n#endif\n"
        self.put("main.cpp", source)
        before = self.analyze(["main.cpp"])
        self.put("main.cpp", source.replace("return 2", "return 3"))
        after = self.analyze(["main.cpp"])
        first = next(n for n in before["nodes"] if n["native_kind"] == "ConditionalBlock")
        second = next(n for n in after["nodes"] if n["native_kind"] == "ConditionalBlock")
        self.assertEqual(first["id"], second["id"])
        self.assertNotEqual(first["code"], second["code"])

    def test_parameter_condition_is_not_duplicated_in_owner_signature(self):
        """参数中的条件块也保持不透明，子块变化不重复污染父对象签名。"""
        source = "int value(\n#if FEATURE\nint x\n#else\ndouble x\n#endif\n){return 1;}\n"
        self.put("main.cpp", source)
        before = self.analyze(["main.cpp"])
        self.put("main.cpp", source.replace("double x", "long x"))
        after = self.analyze(["main.cpp"])
        first = next(n for n in before["nodes"] if n.get("name") == "value")
        second = next(n for n in after["nodes"] if n.get("name") == "value")
        self.assertEqual(first["id"], second["id"])
        self.assertEqual(first["signature"], second["signature"])
        self.assertNotIn("double x", first["signature"])

    def test_discovery_without_database(self):
        """目录发现覆盖源码和头文件，并沿用测试与构建缓存的排除边界。"""
        for name in ["src/main.cpp", "api.hpp", "tests/case.cpp", "build/generated.cpp", "node_modules/dependency.cpp"]:
            self.put(name, "int value();\n")
        graph = self.analyze([], discover=True)
        files = {n["name"] for n in graph["nodes"] if n["native_kind"] == "SourceFile"}
        self.assertEqual(files, {"src/main.cpp", "api.hpp"})

    def test_empty_selection_stays_empty(self):
        """显式空选集保持空图，不以自动发现扩大范围。"""
        self.put("main.cpp", "int value;\n")
        self.assertEqual(self.analyze([]), {"root_contains": [], "nodes": [], "external_symbols": []})

    def test_directory_without_source_returns_empty_graph(self):
        """没有 C/C++ 源文件的目录不要求构建配置或目标 SDK。"""
        self.put("README.txt", "No sources here.\n")
        self.assertEqual(self.analyze([], discover=True), {"root_contains": [], "nodes": [], "external_symbols": []})

    def test_project_configuration_and_database_command_are_not_executed(self):
        """配置只可用于语义增强，缺少或不可用的上下文不得触发执行项目代码。"""
        self.put("main.cpp", "int value(){return 1;}\n")
        marker = self.root / "unexpected-execution"
        self.put("CMakeLists.txt", f'file(WRITE "{marker}" "executed")\n')
        self.put("compile_commands.json", json.dumps([{
            "directory": str(self.root), "file": "main.cpp",
            "command": f'unknown-compiler --write-marker "{marker}" main.cpp',
        }]))
        graph = self.analyze(["main.cpp"])
        self.assertTrue(any(n.get("name") == "value" for n in graph["nodes"]))
        self.assertFalse(marker.exists())

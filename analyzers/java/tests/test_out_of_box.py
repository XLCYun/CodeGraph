"""Default source/platform behavior, with real JDT through the public executable."""
import json
import os
import pathlib
import subprocess
import tempfile
import unittest
from test_helper import PUBLIC_COMMAND, RUNTIME, TARGET, write, metadata, request, run, node, platform, assert_diagnostics


class OutOfBoxTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix="java-default-")
        self.addCleanup(self.temp.cleanup)
        self.root = pathlib.Path(self.temp.name)

    def public(self, files=None, include_tests=False, env=None):
        message = {"protocol": "codegraph.analyzer/1", "type": "analyze", "id": "default",
                   "repository_root": str(self.root), "options": {"include_tests": include_tests, "rust_dependency_scope": "repository-only"}}
        if files is not None:
            message["files"] = files
        environment = dict(os.environ)
        for key in ("CODEGRAPH_JAVA_TARGET_JDK", "JAVA_HOME", "JDK_HOME"):
            environment.pop(key, None)
        environment.update(env or {})
        result = subprocess.run([*PUBLIC_COMMAND, "analyze"], input=json.dumps(message),
                                capture_output=True, text=True, encoding="utf-8", env=environment, timeout=30)
        self.last_stderr = result.stderr
        return json.loads(result.stdout)

    def test_utf8_bom_offsets_preserve_raw_file_and_declarations(self):
        text = '\ufeffpackage p; class A { String label="中文😀"; int run(){return 1;} }\r\n'
        path = 'src/main/java/p/A.java'
        file = self.root / path
        file.parent.mkdir(parents=True, exist_ok=True)
        file.write_bytes(text.encode('utf-8'))
        self.assertEqual(file.read_bytes(), text.encode('utf-8'))
        first = self.public(env={'JAVA_HOME': str(TARGET)})
        self.assertEqual(first['outcome'], 'ok', first)
        graph = first['graph']
        self.assertEqual(node(graph, '#file')['source'], text)
        self.assertEqual(node(graph, '/method:run()')['source'], 'int run(){return 1;}')
        self.assertEqual(node(graph, '/field:label')['source'], 'label="中文😀"')
        plain_text = text.removeprefix('\ufeff')
        file.write_bytes(plain_text.encode('utf-8'))
        self.assertEqual(file.read_bytes(), plain_text.encode('utf-8'))
        plain = self.public(env={'JAVA_HOME': str(TARGET)})
        for key in ('id', 'signature'):
            self.assertEqual([n[key] for n in graph['nodes']], [n[key] for n in plain['graph']['nodes']])
        self.assertTrue(node(graph, '#file')['code'].startswith('\ufeff'))
        self.assertEqual([n['code'].removeprefix('\ufeff') for n in graph['nodes']], [n['code'] for n in plain['graph']['nodes']])
        file.write_bytes(text.encode('utf-8'))
        self.assertEqual(file.read_bytes(), text.encode('utf-8'))
        syntax = run(request(self.root, [path], platforms=[]))
        self.assertEqual(syntax['outcome'], 'ok', syntax)
        self.assertEqual(node(syntax['graph'], '#file')['source'], text)
        self.assertEqual(node(syntax['graph'], '/method:run()')['source'], 'int run(){return 1;}')

    def test_default8_preserves_modern_structure_without_new_apis(self):
        target8 = pathlib.Path(os.environ['CODEGRAPH_JAVA_TARGET_8'])
        write(self.root, 'src/main/java/p/A.java', 'package p; class A { void helper(){} void run(){helper();} Object modern(){return java.util.List.of();} }')
        write(self.root, 'src/main/java/p/R.java', 'package p; record R(int value) {}')
        old = self.public(env={'JAVA_HOME': str(target8)})
        self.assertEqual(old['outcome'], 'ok', old)
        assert_diagnostics(self, self.last_stderr, ('default_platform',))
        calls = node(old['graph'], '/method:run()')['references']
        self.assertTrue(any(r['semantic'] == 'code.calls' and r['target']['kind'] == 'node' for r in calls))
        modern = node(old['graph'], '/method:modern()')['references']
        self.assertTrue(any(r['semantic'] == 'code.calls' and r['target']['kind'] == 'unresolved' for r in modern))
        newer = self.public(env={'JAVA_HOME': str(TARGET)})
        self.assertEqual(newer['outcome'], 'ok', newer)
        keys = ('id', 'native_kind', 'name', 'signature', 'source', 'code', 'contains')
        shape = lambda graph: [{key: n[key] for key in keys} for n in graph['nodes']]
        self.assertEqual(shape(old['graph']), shape(newer['graph']))
        write(self.root, 'pom.xml', '<project><modelVersion>4.0.0</modelVersion><groupId>fixture</groupId><artifactId>app</artifactId><version>1</version><build><plugins><plugin><artifactId>maven-compiler-plugin</artifactId><configuration><jdkToolchain><version>8</version></jdkToolchain></configuration></plugin></plugins></build></project>')
        toolchain = self.public(env={'JAVA_HOME': str(target8)})
        self.assertEqual(toolchain['outcome'], 'ok', toolchain)
        assert_diagnostics(self, self.last_stderr, ())
        self.assertEqual(node(toolchain['graph'], '/method:run()')['extensions']['java.platforms'][0]['api_source'], 'toolchain')
        self.assertTrue(any(r['semantic'] == 'code.calls' and r['target']['kind'] == 'unresolved' for r in node(toolchain['graph'], '/method:modern()')['references']))

    def test_unconfigured_default_helper_and_platform_source_stability(self):
        write(self.root, "src/main/java/p/A.java", "package p; class A { void helper() {} void run(){helper();} }")
        inherited_path = os.environ.get("PATH", os.defpath)
        first = self.public(env={"JAVA_HOME": str(TARGET), "PATH": inherited_path})
        second = self.public(env={"PATH": str(TARGET / "bin") + os.pathsep + inherited_path})
        self.assertEqual(first["outcome"], "ok", first)
        self.assertEqual(first["graph"], second["graph"])
        method = node(first["graph"], "/method:run()")
        self.assertTrue(any(ref["target"]["kind"] == "node" for ref in method["references"] if ref["semantic"] == "code.calls"))
        self.assertEqual(method["extensions"]["java.platforms"], [{"target": "unknown", "status": "available", "version": platform(TARGET)["version"]}])
        self.assertNotIn(str(TARGET), json.dumps(first))
        self.assertNotIn(str(self.root), json.dumps(first))

    def test_missing_platform_preserves_modern_structure_and_ids(self):
        path = "src/main/java/p/A.java"
        write(self.root, path, "package p; record A(int n) { int helper(){return n;} int run(){return helper();} }")
        metadata(self.root, release="25")
        missing = run(request(self.root, [path], platforms=[]))
        available = run(request(self.root, [path], target=RUNTIME))
        self.assertEqual(missing["outcome"], "ok", missing)
        self.assertEqual(available["outcome"], "ok", available)
        projection = lambda graph: [{k: v for k, v in n.items() if k not in ("references", "extensions")} for n in graph["nodes"]]
        self.assertEqual(projection(missing["graph"]), projection(available["graph"]))
        self.assertTrue(any(n["native_kind"] == "record" for n in missing["graph"]["nodes"]))
        self.assertFalse(any(r["semantic"] == "code.calls" and r["target"]["kind"] != "unresolved" for n in missing["graph"]["nodes"] for r in n["references"]))

    def test_missing_declared_old_target_public(self):
        path = "src/main/java/p/A.java"
        write(self.root, path, "package p; class A { void run(){ helper(); } void helper(){} }")
        metadata(self.root, release="24")
        result = self.public([path])
        self.assertEqual(result["outcome"], "ok", result)
        self.assertEqual(result["graph"]["nodes"][0]["extensions"]["java.platforms"], [{"target": "24", "status": "missing"}])

    def test_bad_metadata_exact_selection_and_discovery(self):
        write(self.root, "A.java", "class A {}")
        write(self.root, "narrant-java.json", "{broken")
        result = self.public(["A.java"])
        self.assertEqual(result["outcome"], "ok", result)
        self.assertEqual(self.public()["outcome"], "error")
        self.assertEqual(self.public([])["graph"]["nodes"], [])

    def test_discovery_infers_independent_roots_filters_noise_and_tests(self):
        for file in ("one/src/main/java/p/A.java", "two/src/main/java/p/B.java", "one/src/test/java/p/T.java", "build/p/Noise.java", "vendor/p/V.java"):
            write(self.root, file, "package p; class " + pathlib.Path(file).stem + " {}")
        result = self.public()
        self.assertEqual(result["outcome"], "ok", result)
        paths = {n["id"].split("#")[0][5:] for n in result["graph"]["nodes"]}
        self.assertEqual(paths, {"one/src/main/java/p/A.java", "two/src/main/java/p/B.java"})
        included = self.public(include_tests=True)
        self.assertEqual(len(included["graph"]["root_contains"]), 3)

    def test_declared_members_and_explicit_outside(self):
        metadata(self.root)
        write(self.root, "src/main/java/p/A.java", "package p; class A {}")
        write(self.root, "other/p/B.java", "package p; class B {}")
        self.assertEqual(len(self.public()["graph"]["root_contains"]), 1)
        result = self.public(["other/p/B.java"])
        self.assertEqual(result["outcome"], "ok", result)
        self.assertEqual(len(result["graph"]["root_contains"]), 1)

    def test_invalid_bindings_are_unresolved_but_valid_calls_survive(self):
        path = "A.java"
        write(self.root, path, 'class A { void helper(int n){} void pick(String s){} void pick(Integer i){} void run(){ helper("bad"); pick(null); helper(1); } }')
        result = self.public([path])
        self.assertEqual(result["outcome"], "ok", result)
        calls = [r["target"] for r in node(result["graph"], "/method:run()")["references"] if r["semantic"] == "code.calls"]
        self.assertIn({"kind": "unresolved", "name": "helper"}, calls)
        self.assertIn({"kind": "unresolved", "name": "pick"}, calls)
        self.assertTrue(any(t["kind"] == "node" and t["id"].endswith("/method:helper(int)") for t in calls))

    def test_invalid_constructor_and_method_references_are_unresolved(self):
        write(self.root, "A.java", 'class A { A(int n){} static void helper(int n){} void run(){ new A("bad"); java.util.function.Supplier<A> factory=A::new; java.util.function.Consumer<String> wrong=A::helper; java.util.function.IntConsumer good=A::helper; new A(1); } }')
        result = self.public(["A.java"])
        self.assertEqual(result["outcome"], "ok", result)
        calls = [r["target"] for r in node(result["graph"], "/method:run()")["references"] if r["semantic"] in ("code.calls", "code.references")]
        self.assertTrue(any(t["kind"] == "unresolved" and t["name"] == "helper" for t in calls), calls)
        self.assertTrue(any(t["kind"] == "unresolved" and t["name"] == "A" for t in calls), calls)
        self.assertTrue(any(t["kind"] == "node" and '/method:helper(int)' in t['id'] for t in calls), calls)
        self.assertTrue(any(t["kind"] == "node" and '/constructor:' in t['id'] for t in calls), calls)

    def test_jpms_visibility_downgrades_even_when_descriptor_is_not_selected(self):
        path = 'src/main/java/p/A.java'
        source = 'package p; class A { Object run() throws Exception { return java.sql.DriverManager.getConnection("jdbc:none"); } }'
        write(self.root, path, source)
        write(self.root, 'unrelated/src/main/java/module-info.java', 'module unrelated {}')
        plain = self.public([path], env={'JAVA_HOME': str(TARGET)})
        self.assertEqual(plain['outcome'], 'ok', plain)
        self.assertTrue(any(r['target']['kind'] == 'external' for r in node(plain['graph'], '/method:run()')['references']))
        write(self.root, 'src/main/java/module-info.java', 'module example { exports p; }')
        for selection in ([path], None):
            with self.subTest(selection=selection):
                result = self.public(selection, env={'JAVA_HOME': str(TARGET)})
                self.assertEqual(result['outcome'], 'ok', result)
                assert_diagnostics(self, self.last_stderr, ('default_platform', 'module_visibility_unavailable'))
                calls = [r['target'] for r in node(result['graph'], '/method:run()')['references'] if r['semantic'] == 'code.calls']
                self.assertEqual(calls, [{'kind': 'unresolved', 'name': 'getConnection'}])
                for original in plain['graph']['nodes']:
                    actual = next(n for n in result['graph']['nodes'] if n['id'] == original['id'])
                    for field in ('source', 'code', 'signature'): self.assertEqual(actual[field], original[field])
                    self.assertEqual(actual['extensions']['java.platforms'][0]['status'], 'available')
                if selection: self.assertEqual(result['graph']['root_contains'], ['java:' + path + '#file'])

    def test_syntax_damage_and_jpms_keep_confirmed_structure(self):
        write(self.root, "src/main/java/p/A.java", "package p; class A { int good(){return 1;} void broken( { } }")
        write(self.root, "src/main/java/module-info.java", "module example { exports p; }")
        result = self.public()
        self.assertEqual(result["outcome"], "ok", result)
        self.assertEqual(len(result["graph"]["root_contains"]), 2)
        self.assertTrue(any(n["id"].endswith("/method:good()") for n in result["graph"]["nodes"]))

    def test_missing_inactive_module_does_not_change_selected_binding(self):
        for module in ("selected", "inactive"):
            write(self.root, module + "/p/A.java", "package p; class A { void helper(){} void run(){helper();} }")
        configs = [{"id": "selected", "release": "17", "source_roots": [{"path": "selected", "role": "main"}]},
                   {"id": "inactive", "release": "24", "source_roots": [{"path": "inactive", "role": "main"}]}]
        write(self.root, "narrant-java.json", json.dumps({"schema": "java-static-v1", "configurations": configs}))
        result = self.public(["selected/p/A.java"], env={"JAVA_HOME": str(TARGET)})
        self.assertEqual(result["outcome"], "ok", result)
        calls = node(result["graph"], "/method:run()")["references"]
        self.assertTrue(any(r["semantic"] == "code.calls" and r["target"]["kind"] == "node" for r in calls))
        self.assertEqual(len(result["graph"]["root_contains"]), 1)

    def test_unknown_declared_release_preserves_member_roots(self):
        write(self.root, "declared/p/A.java", "/* header */ package p; class A {}")
        write(self.root, "outside/p/B.java", "package p; class B {}")
        write(self.root, "narrant-java.json", json.dumps({"schema": "java-static-v1", "source_roots": [{"path": "declared", "role": "main"}]}))
        result = self.public()
        self.assertEqual(result["outcome"], "ok", result)
        self.assertEqual(result["graph"]["root_contains"], ["java:declared/p/A.java#file"])
        self.assertEqual(result["graph"]["nodes"][0]["extensions"]["java.platforms"][0]["target"], "unknown")

    def test_production_does_not_bind_test_source_with_include_tests(self):
        write(self.root, "src/main/java/p/A.java", "package p; class A { void run(){ TestHelper.help(); } }")
        write(self.root, "src/test/java/p/TestHelper.java", "package p; class TestHelper { static void help(){} }")
        metadata(self.root, tests=True)
        result = self.public(include_tests=True, env={"JAVA_HOME": str(TARGET)})
        self.assertEqual(result["outcome"], "ok", result)
        calls = node(result["graph"], "/method:run()")["references"]
        self.assertIn({"semantic": "code.calls", "target": {"kind": "unresolved", "name": "help"}}, calls)

    def test_root_level_explicit_file_does_not_import_nested_projects(self):
        write(self.root, "A.java", "class A { void helper(){} void run(){helper();} }")
        write(self.root, "nested/src/main/java/A.java", "class A {}")
        result = self.public(["A.java"])
        self.assertEqual(result["outcome"], "ok", result)
        self.assertEqual(result["graph"]["root_contains"], ["java:A.java#file"])

    def test_inferred_test_can_bind_production_in_same_module(self):
        write(self.root, "src/main/java/p/A.java", "package p; class A { static void helper(){} }")
        write(self.root, "src/test/java/p/TestA.java", "package p; class TestA { void run(){A.helper();} }")
        result = self.public(["src/test/java/p/TestA.java"], include_tests=True)
        self.assertEqual(result["outcome"], "ok", result)
        calls = node(result["graph"], "/method:run()")["references"]
        self.assertTrue(any(r["semantic"] == "code.calls" and r["target"]["kind"] == "external" for r in calls))
        self.assertTrue(any(e["scope"] == "repository" for e in result["graph"]["external_symbols"]))

    def test_standalone_package_need_not_match_storage_directory(self):
        write(self.root, "A.java", "package arbitrary.name; class A { void helper(){} void run(){helper();} }")
        result = self.public(["A.java"])
        self.assertEqual(result["outcome"], "ok", result)
        self.assertTrue(any(n["id"].endswith("/method:run()") for n in result["graph"]["nodes"]))

    def test_malformed_source_root_recovers_selection_but_fails_discovery(self):
        write(self.root, "A.java", "class A {}")
        for invalid_root in (None, "src", 1, []):
            with self.subTest(source_root=invalid_root):
                write(self.root, "narrant-java.json", json.dumps({
                    "schema": "java-static-v1", "source_roots": [invalid_root]}))
                selected = self.public(["A.java"])
                self.assertEqual(selected["outcome"], "ok", selected)
                self.assertEqual(selected["graph"]["root_contains"], ["java:A.java#file"])
                discovered = self.public()
                self.assertEqual(discovered["outcome"], "error", discovered)
                self.assertEqual(discovered["error"]["code"], "analysis_failed")
                self.assertNotIn("graph", discovered)

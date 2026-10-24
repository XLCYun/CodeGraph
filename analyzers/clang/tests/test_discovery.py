"""真实 Clang 参与文件发现回归；不依赖文件名扩展名推断成员。"""

import json
import unittest

import test_helper as fixtures


class DiscoveryTests(unittest.TestCase):
    """验证 CDB、词法进入事件、系统分类与目录模式的协议生命周期。"""

    setUp = fixtures.NativeHelperTests.setUp
    tearDown = fixtures.NativeHelperTests.tearDown
    prepare = fixtures.NativeHelperTests.prepare
    run_request = fixtures.NativeHelperTests.run_request

    def discovery(self, sources, **options):
        """显式删除 files 字段，保留测试驱动中的 [] 语义。"""
        request = self.prepare(sources, **options)
        del request["input"]["files"]
        return request

    def test_participating_empty_macro_and_nonstandard_headers(self):
        """空、宏、无扩展名和 .inc 头参与编译；探测与未启用 include 不进入选集。"""
        request = self.discovery({
            "main.cpp": '#include "empty"\n#include "value.inc"\n#include "nested.def"\n#if __has_include("probe.h")\n#endif\n#if 0\n#include "inactive.h"\n#endif\nint run(){return VALUE+helper();}\n',
            "empty": "",
            "value.inc": "#define VALUE 1\n",
            "nested.def": '#include "api.hpp"\n',
            "api.hpp": "inline int helper(){return 2;}\n",
            "probe.h": "deliberately invalid probe-only source",
            "inactive.h": "deliberately invalid inactive source",
            "unrelated.h": "deliberately invalid unrelated source",
        }, order=(0,))
        result = self.run_request(request)
        self.assertEqual(result["outcome"], "ok", result)
        self.assertEqual({f["path"] for f in result["evidence"]["files"]},
                         {"main.cpp", "empty", "value.inc", "nested.def", "api.hpp"})
        self.assertTrue(any(n["name"] == "VALUE" for n in result["graph"]["nodes"]))
        self.assertTrue(any(n["name"] == "helper" for n in result["graph"]["nodes"]))
        request["input"]["files"] = ["main.cpp"]
        explicit = self.run_request(request)
        self.assertEqual(explicit["outcome"], "ok", explicit)
        self.assertEqual([f["path"] for f in explicit["evidence"]["files"]], ["main.cpp"])
        self.assertTrue(any(s["scope"] == "repository" for s in explicit["graph"]["external_symbols"]))

    def test_system_headers_stay_context(self):
        """仓内 isystem、pragma 系统头和批准仓外 SDK 均只作语义上下文。"""
        request = self.discovery({
            "main.cpp": '#include <system.h>\n#include <external.h>\n#include "pragma.h"\n#include "user.h"\nint run(){return platform()+external()+pragma_api()+user();}\n',
            "system/system.h": "inline int platform(){return 1;}\n",
            "pragma.h": "#pragma clang system_header\ninline int pragma_api(){return 2;}\n",
            "user.h": "inline int user(){return 3;}\n",
        }, order=(0,), extra=("-isystem", "system"))
        include = self.scratch / "include"
        include.mkdir()
        (include / "external.h").write_text("inline int external(){return 4;}\n")
        request["environment"]["platforms"][0]["metadata"]["system_include_directories"] = ["include"]
        result = self.run_request(request)
        self.assertEqual(result["outcome"], "ok", result)
        self.assertEqual({f["path"] for f in result["evidence"]["files"]}, {"main.cpp", "user.h"})
        self.assertEqual({n["name"] for n in result["graph"]["nodes"]}, {"run", "user"})
        self.assertEqual({s["scope"] for s in result["graph"]["external_symbols"]}, {"repository", "dependency"})

    def test_all_configuration_coverage_and_unusual_translation_unit(self):
        """每个实际进入配置贡献覆盖；-x 声明的任意后缀 TU 也参加发现。"""
        renamed = self.root.with_name("repo space # percent% 中文")
        self.root.rename(renamed)
        self.root = renamed
        request = self.discovery({
            "main.cpp": '#if FAST\n#include "fast.inc"\n#else\n#include "safe"\n#endif\n#include "common.h"\nint run(){return common();}\n',
            "fast.inc": "#define FAST_VALUE 1\n", "safe": "#define SAFE_VALUE 2\n",
            "common.h": "inline int common(){return 3;}\n",
        })
        (self.root / "odd.source").write_text("int unusual(){return 4;}\n")
        path = self.root / "compile_commands.json"
        commands = json.loads(path.read_text())
        commands.append({"directory": str(self.root), "file": "odd.source", "arguments": ["clang", "-x", "c++", "-std=c++20", "-c", "odd.source"]})
        path.write_text(json.dumps(commands))
        result = self.run_request(request)
        self.assertEqual(result["outcome"], "ok", result)
        covered = {f["path"]: f["configuration_ids"] for f in result["evidence"]["files"]}
        self.assertEqual({name: len(ids) for name, ids in covered.items()},
                         {"main.cpp": 2, "common.h": 2, "fast.inc": 1, "safe": 1, "odd.source": 1})
        self.assertIn("unusual", [n["name"] for n in result["graph"]["nodes"]])

    def test_empty_invalid_and_missing_configuration(self):
        """空数组不读取 CDB 或 SDK；缺配置、null 与非法参数仍明确失败。"""
        request = self.prepare({}, selected=[])
        (self.root / "compile_commands.json").unlink()
        request["environment"]["platforms"] = []
        result = self.run_request(request)
        self.assertEqual(result["outcome"], "ok", result)
        self.assertEqual(result["evidence"], {"configurations": [], "files": [], "origins": []})
        for invalid in (None, "main.cpp", {}, [None]):
            request["input"]["files"] = invalid
            self.assertEqual(self.run_request(request)["error"]["code"], "invalid_input")
        request = self.discovery({"main.cpp": "int run(){return 1;}\n"}, order=(0,))
        (self.root / "compile_commands.json").unlink()
        self.assertEqual(self.run_request(request)["error"]["code"], "unsupported_configuration")
        (self.root / "compile_commands.json").write_text("[]")
        self.assertEqual(self.run_request(request)["error"]["code"], "unsupported_configuration")
        request = self.discovery({"main.cpp": "int run(){return 1;}\n"}, extra=("@project.rsp",))
        self.assertEqual(self.run_request(request)["error"]["code"], "unsupported_configuration")

    def test_discovery_timeout_during_configuration_loading(self):
        """CDB 配置加载继续检查资源期限，调用方终止由真实公共入口测试覆盖。"""
        request = self.discovery({"main.cpp": "int run(){return 1;}\n"}, order=(0,))
        path = self.root / "compile_commands.json"
        commands = json.loads(path.read_text()) * 2000
        path.write_text(json.dumps(commands))
        request["limits"]["analysis_timeout_ms"] = 1
        result = self.run_request(request)
        self.assertEqual(result["error"]["code"], "resource_limit", result)

#!/usr/bin/env python3
"""直接调用已校验安装包的固定入口，省略 files 以使用语言项目配置。"""
import json
from pathlib import Path
import subprocess
import sys


def main():
    """调用方先验证完整包；示例只发送公开请求并核对终态与固定身份。"""
    if len(sys.argv) != 3:
        raise SystemExit("usage: analyze-directory.py /verified/pack/bin/codegraph-<id> /project")
    executable, root = (Path(value).resolve(strict=True) for value in sys.argv[1:])
    pack_id = executable.name.removesuffix(".exe").removeprefix("codegraph-")
    if pack_id not in {"rust", "typescript-javascript", "python", "java", "go", "csharp", "c-cpp"}:
        raise SystemExit("expected a fixed analyzer entry")
    request = {"protocol": "codegraph.analyzer/1", "type": "analyze", "id": "directory-example",
               "repository_root": str(root),
               "options": {"include_tests": False, "rust_dependency_scope": "repository-only"}}
    # communicate 发送完整 JSON 后关闭 stdin；正常 EOF 不取消已接受的分析。
    result = subprocess.run([str(executable), "analyze"],
                            input=json.dumps(request, ensure_ascii=False).encode("utf-8"),
                            capture_output=True, check=True)
    assert result.stdout.endswith(b"\n")
    completed = json.loads(result.stdout.decode("utf-8"))
    assert completed["protocol"] == request["protocol"]
    assert completed["engine_version"] == "0.1.2" and completed["pack_id"] == pack_id
    assert completed["id"] == request["id"] and completed["type"] == "completed"
    assert completed["outcome"] == "ok"
    assert set(completed["graph"]) == {"root_contains", "nodes", "external_symbols"}
    # 原样转发 UTF-8 协议字节，重定向 stdout 的本地代码页不能改写结果。
    sys.stdout.buffer.write(result.stdout)


if __name__ == "__main__":
    main()

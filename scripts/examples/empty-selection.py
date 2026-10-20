#!/usr/bin/env python3
"""用临时、隔离的目录运行文档中的合法空选集示例，不需要真实语言包。"""
import json
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile


def main():
    """构造受信包身份和固定源码目录，再用公开 CLI 发送一次 NDJSON 请求。"""
    binary = Path(sys.argv[1] if len(sys.argv) > 1 else "target/debug/codegraph-rust").resolve()
    with tempfile.TemporaryDirectory(prefix="codegraph-example-") as temporary:
        root = Path(temporary)
        repository = root / "repository"
        pack = root / "pack"
        repository.mkdir()
        (pack / "control").mkdir(parents=True)
        (pack / "bin").mkdir()
        entry = pack / "bin" / ("codegraph-rust.exe" if sys.platform == "win32" else "codegraph-rust")
        shutil.copy2(binary, entry)
        descriptor = {"schema_version": 1, "pack_id": "rust", "pack_version": "0.1.0",
                      "engine_version": "0.1.0", "protocol": "codegraph.analyzer/1"}
        (pack / "control/codegraph.json").write_text(json.dumps(descriptor), encoding="utf-8")
        request = {"protocol": "codegraph.analyzer/1", "type": "analyze", "id": "example-1",
                   "repository_root": str(repository), "files": [],
                   "options": {"include_tests": False, "rust_dependency_scope": "repository-only"}}
        result = subprocess.run([str(entry), "analyze"],
                                input=json.dumps(request) + "\n", text=True, capture_output=True,
                                timeout=15, check=True, cwd=repository)
        messages = [json.loads(line) for line in result.stdout.splitlines()]
        assert len(messages) == 2 and messages[0]["type"] == "ready"
        assert messages[1]["graph"] == {"root_contains": [], "nodes": [], "external_symbols": []}
        print(result.stdout, end="")


if __name__ == "__main__":
    main()

#!/usr/bin/env python3
"""Compile only the fixed CodeGraph Java analyzer engine on macOS, Linux or Windows."""

import os
import pathlib
import subprocess
import sys

root = pathlib.Path(__file__).resolve().parent
jdk = pathlib.Path(sys.argv[1]).resolve()
libs = pathlib.Path(sys.argv[2]).resolve()
classpath = subprocess.check_output(
    [sys.executable, str(root / "tools/classpath.py"), str(libs)], text=True).strip()
output = root / "build/classes"
output.mkdir(parents=True, exist_ok=True)
suffix = ".exe" if sys.platform == "win32" else ""
sources = sorted((root / "src/dev/codegraph/java").glob("*.java"))
subprocess.run([str(jdk / "bin" / ("javac" + suffix)), "-encoding", "UTF-8", "-proc:none",
                "-cp", classpath, "-d", str(output), *(str(source) for source in sources)], check=True)
print(output)

# 测试驱动单独编译，绝不进入随包 classes 目录。
if "--tests" in sys.argv[3:]:
    test_output = root / "build/test-classes"
    test_output.mkdir(parents=True, exist_ok=True)
    test_sources = sorted((root / "tests/java/dev/codegraph/java").glob("*.java"))
    subprocess.run([str(jdk / "bin" / ("javac" + suffix)), "-encoding", "UTF-8", "-proc:none",
                    "-cp", str(output) + os.pathsep + classpath, "-d", str(test_output),
                    *(str(source) for source in test_sources)], check=True)

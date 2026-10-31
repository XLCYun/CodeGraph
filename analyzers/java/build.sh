#!/bin/sh
# 仅编译 CodeGraph 引擎 自身；不会读取或执行目标项目构建逻辑。
set -eu
if [ "$#" -ne 2 ]; then
  echo 'usage: build.sh JDK_HOME LOCKED_LIB_DIRECTORY' >&2
  exit 2
fi
SCRIPT_DIRECTORY=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
exec python3 "$SCRIPT_DIRECTORY/build.py" "$1" "$2"

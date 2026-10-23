#!/bin/sh
# 使用显式固定 LLVM SDK 构建分析器，不查找或执行被分析项目的工具。
set -eu
sdk=${1:?usage: build.sh /absolute/LLVM-21.1.8}
base=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
version=$("$sdk/bin/llvm-config" --version)
[ "$version" = 21.1.8 ] || { echo 'LLVM 21.1.8 required' >&2; exit 1; }
mkdir -p "$base/build/runtime/lib" "$base/build/objects"
cp "$sdk/lib/libclang-cpp.dylib" "$base/build/runtime/lib/libclang-cpp.dylib"

# 核心只编译一次；CMake 读取同一清单，防止双平台遗漏实现文件。
set --
while IFS= read -r source || [ -n "$source" ]; do
  [ -n "$source" ] || continue
  object="$base/build/objects/$(basename "$source" .cpp).o"
  /usr/bin/clang++ -std=c++17 -O1 -g0 -fexceptions -fno-rtti \
    -I"$sdk/include" -c "$base/$source" -o "$object"
  set -- "$@" "$object"
done < "$base/sources.txt"
# 夹具只按需编译；单独的 archive 成员不会被生产入口链接。
if [ "${CODEGRAPH_BUILD_COMPONENT_TESTS:-0}" = 1 ]; then
  object="$base/build/objects/fixture.o"
  /usr/bin/clang++ -std=c++17 -O1 -g0 -fexceptions -fno-rtti \
    -I"$sdk/include" -c "$base/src/fixture.cpp" -o "$object"
  set -- "$@" "$object"
fi
library="$base/build/libcodegraph-clang-analysis.a"
# 每次从当前清单重建 archive，不保留上一轮已经移除的成员。
rm -f "$library.pending"
/usr/bin/ar rcs "$library.pending" "$@"
mv "$library.pending" "$library"
/usr/bin/clang++ -std=c++17 -O1 -g0 -fexceptions -fno-rtti \
  -I"$sdk/include" "$base/src/main.cpp" "$library" \
  -Wl,-rpath,@executable_path/runtime/lib -Wl,-rpath,@executable_path/../runtime/lib "$sdk/lib/libclang-cpp.dylib" \
  -o "$base/build/codegraph-c-cpp"

# 直接程序自行定位固定 builtin headers，目标 SDK 仍在运行时只读发现。
mkdir -p "$base/build/runtime/platform/clang-resource"
cp -R "$sdk/lib/clang/21/." "$base/build/runtime/platform/clang-resource/"
# 测试驱动按需本地生成，不进入 install/package 目标。
if [ "${CODEGRAPH_BUILD_COMPONENT_TESTS:-0}" = 1 ]; then
/usr/bin/clang++ -std=c++17 -O1 -g0 -fexceptions -fno-rtti \
  -I"$sdk/include" "$base/tests/component.cpp" "$base/tests/sdk_discovery.cpp" "$library" \
  -Wl,-rpath,@executable_path/runtime/lib "$sdk/lib/libclang-cpp.dylib" \
  -o "$base/build/codegraph-clang-components"
fi

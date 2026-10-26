# codegraph-c-cpp

Clang 21.1.8 LibTooling 原生分析器。分析 module 直接使用 `ClangTool`、AST visitor、
`SourceManager`、`Lexer`、`PPCallbacks` 和 USR 绑定证据；不运行 clang AST JSON 子进程。
固定 `bin/codegraph-c-cpp[.exe] analyze` 直接读取公共 `codegraph.analyzer/1` JSON 后 EOF，
输出一个带固定 `engine_version: "0.1.1"` 与 `pack_id: "c-cpp"` 的最终结果。
调用方负责取消与超时的整棵进程树终止；没有 ready/cancel、分析 worker 或控制监听器。

## 实现结构

`src/analyzer.hpp` 是入口使用的内部 interface，只提供公共分析和非交付语义夹具的完整执行操作，
不暴露 `Request`、`Graph` 或 AST 状态。`src/main.cpp` 只解释启动参数并调用公共操作；
`tests/component.cpp` 只启动夹具或 SDK 专项检查，不包含生产 `.cpp`。

| 文件 | 职责 |
| --- | --- |
| `src/protocol.cpp` | 一次性 JSON、公共校验、错误映射与最终输出 |
| `src/analyzer.cpp` | 单请求生命周期，选择公共源码审查或非交付语义夹具 |
| `src/source_document.cpp` | 原始逻辑指令、条件块、保护宏与等长语法视图 |
| `src/source_review.cpp` | 源码选集、原文覆盖、不透明缺口与可选引用增强 |
| `src/project.cpp`、`src/sdk_discovery.cpp` | 现有 CDB 解释、目标 SDK 发现和批准策略 |
| `src/extraction.cpp` | 同进程 Clang、VFS、AST、宏和参与文件提取 |
| `src/graph.cpp` | 跨 TU 并集后的稳定身份、引用、自身内容与图结果 |
| `src/support.cpp`、`src/source_text.cpp` | 严格 JSON、路径操作与原始 token 文本 |
| `src/detail/analysis.hpp` | module 私有状态和内部声明 |
| `src/fixture.cpp` | 非交付夹具的完整执行操作，保留内部 evidence 和细分错误码 |

`sources.txt` 是 CMake 与 `build.sh` 共用的核心源文件清单。核心先编译为静态 archive，
再由公共入口和夹具链接；archive 不安装、不随包交付。夹具操作是可选的独立成员，
生产入口不引用它。公共入口以源码覆盖为先，夹具保留严格语义验收；二者共用原生提取和图收尾。

## 固定开发环境与构建

macOS ARM64 的固定官方 SDK 及校验和见 `toolchain.json`。SDK 只用于构建 CodeGraph 引擎，
不下载被分析项目依赖。将官方 archive 校验并解包到隔离目录后：

```sh
CODEGRAPH_BUILD_COMPONENT_TESTS=1 analyzers/clang/build.sh /absolute/LLVM-21.1.8-macOS-ARM64
CODEGRAPH_LLVM_ROOT=/absolute/LLVM-21.1.8-macOS-ARM64 CODEGRAPH_CLANG_HELPER="$PWD/analyzers/clang/build/codegraph-clang-components" CODEGRAPH_CLANG_ANALYZER="$PWD/analyzers/clang/build/codegraph-c-cpp" python3 -m unittest discover -s analyzers/clang/tests -v
```

产物为 `build/codegraph-c-cpp` 和 `build/runtime/lib/libclang-cpp.dylib`。公开发布包按
`bin/codegraph-c-cpp`、`runtime/lib/libclang-cpp.dylib` 放置；二进制只使用 executable-relative
rpath，不含开发机绝对 SDK 路径。resource headers 从安装根的 `runtime/platform/clang-resource` 定位，包含固定 `lib/clang/21` 的全部内容。
`build/codegraph-clang-components` 是不安装、不随包交付的语义夹具驱动。
`build/` 被忽略，不提交二进制、动态库或下载的 SDK。当前构建与运行验收为 macOS ARM64；
Windows 增量由专门原生 CI 验收，不能把本机构建成功当作 Windows 成功。

## 公共源码审查

无需 `compile_commands.json` 或目标 SDK 就能构图。每个选中文件有一个 `SourceFile` 容器，
`source` 逐字保存完整原文。可确认的声明正常提取；缺头文件、未知类型或不完整声明不会让
选中源码消失，无法确认的声明区域保留为 `OpaqueSourceFragment`，未知引用为 `unresolved`。
源码恢复使用固定 Clang 的 C23/C++23 语法模式，不表示项目使用或可编译于这些标准。

`#` 开头的预处理指令是 `PreprocessorDirective`。一般条件块从起始指令到匹配的 `#endif`
整体保存为 `ConditionalBlock`，保留全部分支，不提取块内声明。标准外层头文件保护宏例外：
`#ifndef NAME` 或 `#if !defined(NAME)`、对应 `#define NAME` 和最终 `#endif` 分别保存，
内部结构照常提取；保护层不能带 `else`/`elif`，允许前置 `#pragma once`。
普通宏调用留在所属对象原文中，不额外建模展开对象或展开后的调用关系。

不透明节点也将原文规范化后写入参与比较的 `code`，因此非激活分支变化不会被隐藏。
父对象的自身内容用子对象锚点代替子对象原文；`source` 仅供显示，不单独驱动差异。

目录发现直接扫描 C/C++ 源码与头文件，沿用依赖排除并跳过构建缓存；测试开关对目录发现
生效。显式选集保持精确范围，包括特殊后缀文件；空目录与显式空集返回空图。
后缀、排除路径和其他边界见[完整选集政策](../../docs/engine-selection.md#c--c-resolution)。

## 可选编译上下文

已有的根目录 CDB 可增强可靠引用绑定。接受 `arguments` 数组，或只通过 LLVM 参数分词器
读取标准 `command` 字符串；两者都只是数据。绝不执行其中的程序、CMake、query-driver
或项目构建脚本。上下文不能改变源文件选集或增加源码对象，宏展开内容也不会导出。
上下文缺失、过大、参数不支持、目标 SDK 不可用或语义诊断失败时，继续返回源码图。
请求超时、资源上限、读取失败及真正的内部错误仍明确失败。

上下文仍要求白名单参数、明确的稳定 `-std=`、正确的 C/C++ 语言模式和批准的目标头文件根。
固定 LLVM 原生 `LangStandard` 解释 C89–C23、C++98–C++23 及 GNU 方言和稳定别名，
不接受 C2y/C++26 预览模式。wrapper、response file、插件、modules 和未知 include/sysroot
不能借增强步骤获得执行或额外读取权限。唯一已证实定义可以作为调用目标，不按同名猜绑定。

本语言程序只发现本机已有的目标 SDK，不下载或安装。内部 `clang-target` 保存批准的目标
triple、系统 include 根与 `clang-resource` 引用；`clang-resource` 是随包固定 LLVM 21.1.8
资源。C++ 包装头、resource headers 和目标 C headers 分别授权，VFS 不批准共同的 `/` 根，
也不复制项目源码到 scratch。上下文是否可用不进入节点的比较字段。

非交付组件夹具保留严格语义模式，覆盖标准版本、GNU 扩展、目标头、多 TU 并集、宏回调和
失败边界。其 CDB 必需、语义错误终结等规则只用于组件验收；公共入口使用上述源码审查政策。
两类测试都检查源文件路径与字节未被改写。取消由调用方终止整棵进程树。

## Windows 增量与附加标准头

Windows 构建入口已提供，须在实际 Windows 工具链中验证：

```powershell
./analyzers/clang/build.ps1 -LlvmRoot C:/isolated/clang-llvm-21.1.8 -BuildComponents
$env:CODEGRAPH_LLVM_ROOT = 'C:/isolated/clang-llvm-21.1.8'
$env:CODEGRAPH_CLANG_HELPER = 'C:/repo/analyzers/clang/build/codegraph-clang-components.exe'
$env:CODEGRAPH_CLANG_ANALYZER = 'C:/repo/analyzers/clang/build/codegraph-c-cpp.exe'
$env:CODEGRAPH_WINDOWS_SDK = 'C:/Program Files (x86)/Windows Kits/10/Include/10.0.26100.0'
$env:CODEGRAPH_MSVC_ROOT = 'C:/Program Files/Microsoft Visual Studio/2022/Enterprise/VC/Tools/MSVC/14.44.35207'
python -m unittest discover -s analyzers/clang/tests -v
```

CMake 固定 LLVM 21.1.8，优先 clang-cpp 导出目标，否则链接官方静态 Clang 依赖图；关闭 RTTI、
开启 helper 自身异常及 UTF-8 源码。输出固定为 `build/codegraph-c-cpp.exe`；若 SDK 提供动态库，
CMake 将声明的运行 DLL 放在程序旁边，发布包必须在 `bin/` 保留这些 DLL。
macOS 保留 `@executable_path/runtime/lib` 与 `@executable_path/../runtime/lib` rpath，
发布布局为 `bin/codegraph-c-cpp` 加 `runtime/lib/libclang-cpp.dylib`。

`clang-target.metadata` 新增可选 `additional_system_platform_ids`，只能引用 catalog 内
`kind: "clang-system-headers"`、`metadata: {}` 的精确批准根。Windows SDK 的 `Include/<版本>`
与 MSVC `<版本>/include` 因此分别授权，无需放宽到 Program Files。附加平台 ID 进入 evidence。
空/重复/错误种类/缺失引用会拒绝；每个 TU 只读取自身目标引用的这些根。MSVC 标准头先于 resource
与 SDK C 头。直接调用和标准宏的上下文都通过同一 VFS。

macOS CI 可设置 `CODEGRAPH_MACOS_SDK` 指向固定 Xcode SDK，测试读取 SDK 自己的版本元数据。
不设置 Windows SDK 环境时，该测试会明确 skip；这不是 Windows 验收通过。

## 审查修复第一轮

原始 token lexer 必须处理已产出的最后一个有效 token，不能把其 EOF 返回值当成“没有 token”。
优化白名单 `-O0/-O1/-O2/-O3/-Os/-Oz/-Og` 原样传入 Clang，保留 `__OPTIMIZE__` 等语义。
LinkageSpec 签名只保留 `extern` 与语言字符串。共用 Var/Field 声明通过 AST 基础 TypeLoc 取得
共享声明前缀，然后分离每个 declarator；前一个 initializer 不进入后一个节点的 code/signature。
这支持已验证的全局多变量、类字段、指针及数组声明，父节点仍只保留独立子对象锚点。

## 目录发现

公共 `files` 省略时，由本语言配置加载器确定完整选集；`[]` 是明确空选集，
`null` 无效。显式数组保持精确选择。具体成员、排除规则、测试角色与限制见
[引擎选集政策](../../docs/engine-selection.md)。新增目录发现测试由现有原生验收命令运行，
来源、精确选集和图不变量由当前进程检查；公共入口测试覆盖真实 EOF 与调用方终止。

JSON 输入不能提供 catalog、SDK 路径、命令或运行时参数。内部组件的细分失败码映射为公共协议的稳定错误码。

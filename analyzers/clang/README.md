# codegraph-c-cpp

Clang 21.1.8 LibTooling 原生分析器。分析 module 直接使用 `ClangTool`、AST visitor、
`SourceManager`、`Lexer`、`PPCallbacks` 和 USR 绑定证据；不运行 clang AST JSON 子进程。
固定 `bin/codegraph-c-cpp[.exe] analyze` 直接读取公共 `codegraph.analyzer/1` JSON 后 EOF，
输出一个带固定 `engine_version: "0.1.0"` 与 `pack_id: "c-cpp"` 的最终结果。
调用方负责取消与超时的整棵进程树终止；没有 ready/cancel、分析 worker 或控制监听器。

## 实现结构

`src/analyzer.hpp` 是入口使用的内部 interface，只提供公共分析和非交付语义夹具的完整执行操作，
不暴露 `Request`、`Graph` 或 AST 状态。`src/main.cpp` 只解释启动参数并调用公共操作；
`tests/component.cpp` 只启动夹具或 SDK 专项检查，不包含生产 `.cpp`。

| 文件 | 职责 |
| --- | --- |
| `src/protocol.cpp` | 一次性 JSON、公共校验、错误映射与最终输出 |
| `src/analyzer.cpp` | 单请求生命周期、顺序运行全部 TU 后收尾 |
| `src/project.cpp`、`src/sdk_discovery.cpp` | 现有 CDB 解释、目标 SDK 发现和批准策略 |
| `src/extraction.cpp` | 同进程 Clang、VFS、AST、宏和参与文件提取 |
| `src/graph.cpp` | 跨 TU 并集后的稳定身份、引用、自身内容与图结果 |
| `src/support.cpp`、`src/source_text.cpp` | 严格 JSON、路径操作与原始 token 文本 |
| `src/detail/analysis.hpp` | module 私有状态和内部声明 |
| `src/fixture.cpp` | 非交付夹具的完整执行操作，保留内部 evidence 和细分错误码 |

`sources.txt` 是 CMake 与 `build.sh` 共用的核心源文件清单。核心先编译为静态 archive，
再由公共入口和夹具链接；archive 不安装、不随包交付。夹具操作是可选的独立成员，
生产入口不引用它。公共与夹具原有的资源限制、校验和错误处理差异保持原样。

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

## 内部目标模型

- `clang-target`：`root` 为批准的目标 SDK，`version` 为 SDK 版本；metadata 精确包含
  `target_triple`、`system_include_directories`（root 相对目录数组）、`resource_platform_id`。
  语言标准属于每个 TU 的编译参数，catalog 不再携带 `c_standard` / `cpp_standard`。
- `clang-resource`：`root` 为固定 LLVM 的 `lib/clang/21`，`version: "21.1.8"`，metadata 为 `{}`。
- 两个来源分别入 evidence，禁止为了共同根目录而批准 `/` 或复制目标头文件到 scratch。
- C++ `c++` 路径组件下的包装头优先，然后 resource/include，再目标 C headers；其它读取被
  VFS 拒绝。实际验收包含 MacOSX27.0 SDK 的 C `<stdint.h>/<stdio.h>` 和 C++ `<vector>`。

本语言程序从本机开发工具发现目标 SDK，并读取其实际版本及头文件目录；不再限定 macOS SDK
版本白名单，也不要求平台指纹。缺少标准头仍明确失败，程序只读取静态安装发现批准的具体根。
CI 的固定 SDK 是复现测试样本；用户不必安装相同版本，也不会随支持包下载系统 SDK。

## 明确的配置边界

本侧根目录必须有 `compile_commands.json`；仅接受 `arguments` 数组，拒绝 `command`、wrapper、
response file、插件、modules、自动配置、未知参数与非批准 include/sysroot。每个 TU 的语言标准、
宏、include、target 都分别解析；绝不执行 CDB 中的程序、CMake、query-driver 或构建脚本。

每条编译命令必须明确提供 `-std=`（也支持 Clang 的 `--std=` / `--std` 写法）。标准及别名
直接由固定 LLVM 21.1.8 的 `LangStandard` 识别，包括 C89–C23、C++98–C++23 和对应 GNU 方言；
例如 `c18`、`iso9899:2011`、`gnu++17` 都保留其真实解析模式。LLVM 已归入稳定标准的历史别名
（如 `c2x` 对应 C23、`c++2b` 对应 C++23）也遵从原生映射。LLVM 的 C2y / C++26 特性标记用于
拒绝仍在预览阶段的模式及其别名，不另维护一份很窄的标准名称白名单。

语言模式按 Clang 的源文件类型表、C++ driver 规则与输入之前生效的 `-x c` / `-x c++` 判定；
`-x none` 恢复推断，`-xc` / `-xc++` 同样有效。标准必须与该 TU 的 C / C++ 模式匹配，最后一个
受支持的标准选项原样传入 Clang。缺 CDB、缺标准、非法标准、预览标准或语言错配都会明确失败，
不会补入 C17 / C++20。固定 LLVM 不保证实现每个已发布标准的全部特性；具体语法仍由真实 Clang
验证，错误终结不会携带部分图，也不会被成功解析的其它 TU 掩盖。
ISO 模式保留严格标准诊断；GNU 模式允许 Clang 原生的 `typeof`、statement expression 等 GNU
扩展，并从 LLVM 诊断表发现当前语言的标准扩展分组，将超出所选 ISO 版本的语法诊断提升为错误。
这既保留 GNU 语义，也防止 GNU 低版本通过警告悄悄接受后续标准语法。

同一请求可以含 C 与 C++ TU。成功解析为 C 的共同 header 声明统一标为 `c`；只在 C++ 上下文
出现的声明标为 `cpp`。同一 header 的多个语言标准上下文分别分析，再合并条件分支和关系；
归并仍校验原始声明、类别、native kind 和父级；不因语言模式或标准生成重复
header。普通 C record/C++ record 的同一源 struct 使用 `StructDecl`，不能把 AST 实现类名差异
误当成源码对象冲突。真正不能归并的源码映射返回 `union_conflict`。

源片段先按当前快照物理位置关联，再生成相对文件、词法 owner、名字、原始参数类型槽、
声明/定义角色及重复槽序号组成的 ID。位置和 USR 不进入生产 ID。prototype 与 definition
分别保留，唯一已证实定义可以成为调用目标；未决调用不按同名猜测。宏定义是 extension，
宏引用来自真实预处理展开回调；宏内容变化参与比较。模板只保留显式源码声明，不添加隐式实例。

测试脚本创建自身临时源码并核对分析前后完整文件摘要；helper 从原位置读取。VFS/AST 保留资源期限检查；调用方通过整棵进程树终止回收原生解析。
标准验收同时检查 `__STDC_VERSION__` / `__cplusplus`、GNU 模式和版本专属语法：C99 循环声明、
C11 `_Generic`、C23 `constexpr`，以及 C++11–C++23 的已实现特性。相同语法在较旧标准下必须失败；
共享 header 还覆盖 C99、C23、C++11、C++23 四种上下文的并集及顺序稳定性。

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

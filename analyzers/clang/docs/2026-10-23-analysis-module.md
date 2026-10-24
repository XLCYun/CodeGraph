# Clang 分析 module 重构

用户已确认候选 01 及具体分工：只重构 CodeGraph 的 Clang 分析器，保持现有行为，不修改或测试 Narrant，不实施 SDK/CDB 授权模型的候选 02。

## Spec

- 用户在当前会话批准的设计与范围。
- `docs/protocol.md`、`docs/engine-selection.md`、`docs/adr/0001-implement-public-protocol-in-native-analyzers.md`。
- 基线：`76123ec1c7644dedaaee1ec9d0bccae33bb41145`。

## Global Constraints

- 公共入口与语义夹具调用完整执行操作，不在入口编排内部阶段或包含实现文件。
- 分析 module 内部持有 Request、Graph、Clang 生命周期和完成顺序；实现按职责编译。
- 保留公共与夹具的校验、错误码、退出码、evidence、环境清理、资源限制及期限起点差异。
- SDK、CDB、VFS、选集、图身份与图内容算法不变；不引入新的可替换编译器抽象。
- 保留 macOS ARM64 / Windows x64 的固定入口、运行时布局、LLVM 编译选项与链接依赖。
- 只运行本分析器相关验证；不触发 quality-gate，不执行发布或合并。

## Task 1: 提取共享分析 module 并验证行为

### Interfaces

- 公共完整执行操作：读取一次输入、解析与分析、输出一个最终结果并返回原有状态码。
- 非交付夹具完整执行操作：保留 fixture 输入、指定 SDK、资源限制、原始错误和 evidence。
- SDK 专项测试保留内部 seam，通过声明链接真实 SDK 发现实现。
- 两个构建入口编译同一份核心源代码，再链接各入口；主 interface 不暴露内部分析状态。

### Steps

1. 使用现有固定 LLVM、CMake 和两个真实二进制运行完整 Clang 测试，保存基线。
   Expected: 全部本机可运行案例通过，Windows 专属标准头案例明确 skip。
2. 将实现分为公共协议、请求执行、通用输入/路径、项目配置、SDK 发现、Clang 提取和图收尾；保持原语句及顺序。
   Expected: 入口简短，不再包含 `.cpp`，不使用宏切换入口。
3. 同步 CMake 与 build.sh，共享已编译实现，保持产物布局；更新本分析器 README。
   Expected: macOS 构建和 CMake 构建均链接共享实现，夹具及测试代码不进入发布入口。
4. 运行完整 Clang 测试、聚焦公共协议及稳定图结果的基线对比；检查差异并提交本地变更。
   Expected: 现有行为通过验收；实际 Windows 执行是否完成单独如实报告。
5. 对本次范围进行一次独立审查；处理重要问题并保留验证证据。
   Expected: 无未处理的重要问题；不重新审查既有七语言拆分。

## Review Focus

- 跨 TU 并集、发现时机、稳定 ID、宏与 declarator 内容、图外符号不能因拆文件而改变。
- JSON 重复键/Unicode、错误映射和 evidence 的公私差异保持原样。
- 公共期限仍在 stdin 读取前开始；fixture 期限仍在解析后设置。
- CMake 和 shell 构建源列表、异常/RTTI/UTF-8、rpath 与 Windows DLL 闭包完整。
- 入口不暴露私有协议，不引入额外进程、抽象编译器或 SDK 策略变化。

## 本地验证

- macOS ARM64、固定 LLVM 21.1.8：重构前后均执行完整 Clang Python suite，46 项测试通过，1 项 Windows 标准头案例按平台跳过。
- `build.sh` 实际构建公共入口、共享 archive 和语义夹具；7 组公共/夹具结果的完整图、内部 evidence、错误、退出码及 stderr 与基线一致。
- 只包含 `analyzer.hpp` 的独立调用程序链接共享 archive，在一个进程内执行同样 7 组请求，结果均与基线一致。
- 检查链接符号，夹具操作仅存在于非交付夹具，未链接进公共二进制；shell 语法及 Git 空白检查通过。
- 标准 CMake 配置尝试未完成：本机精简 LLVM 缓存不含 `LLVMConfig.cmake` / `ClangConfig.cmake`。Windows 实际编译和运行仍需 CI；本机结果不作为 Windows 通过证据。
- 未修改或测试 Narrant，未运行其他语言的测试。

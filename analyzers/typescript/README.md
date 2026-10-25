# TypeScript / JavaScript 语言分析器

公开入口为 `bin/codegraph-typescript-javascript analyze`。JS 程序直接实现
`codegraph.analyzer/1`，固定版本 `0.1.1`。stdin 接收一个 UTF-8 JSON 对象后 EOF，
尾换行可选；stdout 只有一条带固定语言、版本和协议身份的最终结果。成功退出 0，
可处理失败退出 1，CLI 用法错误退出 2。`--help`、`analyze --help` 和 `--version`
输出文本后退出。完整契约见 [公共协议](../../docs/protocol.md)。

固定 Node 24.18.0、TypeScript 6.0.3、@types/node 22.20.1、undici-types 6.21.0。
Rust 薄入口只定位 `bin/node` 与 `runtime/analyzer/main.mjs`，设置固定参数与运行环境，
继承标准句柄并传播退出；macOS 使用 exec，Windows spawn/wait。公共 JSON、声明发现、
源码加载及构图均在 JS 主线程内完成，没有 ready、cancel、控制线程或分析 Worker。
调用方在写完输入后关闭 stdin，并负责取消、外部期限和整棵进程树终止。

- `request.mjs` 拒绝重复 JSON 键、未知字段、非法路径及安装与源码重叠，保留
  128 MiB 请求/结果和 100,000 文件上限。显式 `files: []` 不读取配置或加载声明。
- `runtime.mjs` 只从随包固定位置读取三套声明及编译器，不执行项目插件、代码、
  构建、类型获取或目标 SDK 安装。公共输入不能传入 SDK 或 catalog。
- `workspace.mjs` 只允许原有第一方源码/JSON 元数据和固定声明。workspace 包与
  Node 声明通过内存接口解析，不创建 node_modules 链接；完成前校验快照未变化。
- `config.mjs` 保留原生 JSONC、extends/references/configDir/paths、package imports/exports
  及无 dist 的项目引用重定向。省略选集使用原有配置发现；精确选集之外仅提供上下文。
- `analyze.mjs` 保留每配置的 LanguageService 和共享 DocumentRegistry，定义导航、
  诊断、type-only 路由和关系 union 语义不变。`graph.mjs` 在输出边界一次检查来源、
  六类节点身份、唯一包含、根可达性、关系端点和完整图外描述；私有证据不输出。
- `projection.mjs` 保留原生 AST、自身内容、历史稳定身份、JSX、模板、正则、ASI
  及稀疏数组等断言。别名按真实路径合并节点，来源保留词法选集；测试排除同时检查
  词法路径与真实相对路径，越界链接仍失败。
- Windows 输出保留经过短写测试的异步 `fs.write`，按 UTF-8 字节完整写出。
  stdout 背压由调用方持续排空；取消和输送故障由调用方终止进程，不恢复控制通道。

原生语义与公共入口测试复用已有固定声明包，不修改旧包字节：

```sh
CODEGRAPH_TYPESCRIPT_PACK=/absolute/extracted/pack node --test analyzers/typescript/tests/*.test.mjs
```

性能样本使用新的公共入口和一次性包布局：

```sh
CODEGRAPH_TYPESCRIPT_PACK=/absolute/extracted/pack node analyzers/typescript/tests/benchmark.mjs
```

七语言已通过直接入口和当前[分发接口](../../docs/distribution.md)构建；
随包 Node、生产模块与固定声明的布局见[包说明](../../docs/pack-layout.md)。
2026-10-22 的 PR CI 39777180923 中 macOS 成功，Windows 的 Python junction 诊断断言失败；
须取得修复后的完整自动 CI，首个固定发布仍待完成。旧结构验收仅作为有日期的历史记录。

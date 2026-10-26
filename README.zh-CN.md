# CodeGraph

[English](README.md) | [简体中文](README.zh-CN.md)

CodeGraph 提供七个可独立运行的**语言分析器（Language Analyzer）**，
将选定源码文件转换为统一的**代码图（Code Graph）**，表示某个固定源码状态下的
代码对象及其关系。

各分析器使用本语言的原生语义分析工具，共用一套 [JSON 协议](docs/protocol.md)。
调用方因此可以用相同的请求格式、结果结构和进程管理方式接入不同语言。

```text
源码目录 + 文件选集 → 选定的语言分析器 → 代码图（JSON）
```

分析器可独立于 Narrant 和 Tauri 运行，被分析目录也不必是 Git 仓库。
代码图差异计算、多语言代码图合并由调用方实现。

**当前版本**：**v0.1.1**，修复 Go 指令文本误判，支持
**macOS ARM64** 和 **Windows x64**。分析器包经双平台 CI 验证后在
[对应发布页](https://github.com/XLCYun/CodeGraph/releases/tag/v0.1.1) 提供下载。
每份清单记录构建使用的完整源码提交，包与引擎版本均为 `0.1.1`，
协议继续使用 `codegraph.analyzer/1`。目前没有包管理器安装方式。
从发布页下载对应归档和清单，按[分发约定](docs/distribution.md)及
[SHA256SUMS](https://github.com/XLCYun/CodeGraph/releases/download/v0.1.1/SHA256SUMS)
完成校验后，即可调用包内的独立入口。例如，Python 包提供
[macOS ARM64 下载](https://github.com/XLCYun/CodeGraph/releases/download/v0.1.1/python-aarch64-apple-darwin-pack.tar.gz)
和 [Windows x64 下载](https://github.com/XLCYun/CodeGraph/releases/download/v0.1.1/python-x86_64-pc-windows-msvc-pack.tar.gz)。

## 选择分析器

按源码语言选择分析器包，再为对应入口传入 `analyze` 子命令。
下表路径均相对于已安装的分析器包；Windows 入口需加 `.exe` 后缀。

| 语言 | 分析器包 ID | 分析引擎 | 公开入口 |
| --- | --- | --- | --- |
| [Rust](analyzers/rust/README.md) | `rust` | rust-analyzer | `bin/codegraph-rust` |
| [TypeScript / JavaScript](analyzers/typescript/README.md) | `typescript-javascript` | TypeScript 编译器 API | `bin/codegraph-typescript-javascript` |
| [Python](analyzers/python/README.md) | `python` | Ruff / ty | `bin/codegraph-python` |
| [Java](analyzers/java/README.md) | `java` | Eclipse JDT | `bin/codegraph-java` |
| [Go](analyzers/go/README.md) | `go` | `go/parser` / `go/types` | `bin/codegraph-go` |
| [C#](analyzers/csharp/README.md) | `csharp` | Roslyn | `bin/codegraph-csharp` |
| [C / C++](analyzers/clang/README.md) | `c-cpp` | Clang LibTooling | `bin/codegraph-c-cpp` |

调用方经各分析器的 CLI 发送 JSON 请求，并按共用协议读取结果。
各语言链接中列出了支持的项目配置和具体准备步骤。

## 运行分析器

先准备**已构建并完成校验的分析器包**，安装目录需在源码目录之外。
在本仓库根目录运行以下示例脚本，即可按项目配置发现并分析源码：

```sh
python3 scripts/examples/analyze-directory.py /absolute/verified/pack/bin/codegraph-typescript-javascript /absolute/project
```

Windows 使用本机可用的 Python 3 命令，并为入口加上 `.exe`：

```powershell
python scripts/examples/analyze-directory.py C:/verified/pack/bin/codegraph-typescript-javascript.exe C:/project
```

仓库示例脚本需要 Python **3.9+**，Python 分析器本身无需 Python 解释器。
脚本发送 UTF-8 输入，关闭 stdin，再核对最终身份、图字段和退出状态；
分析器包仍需由调用方事先校验。

若要直接调用分析器，将下面这份最小请求保存为 UTF-8 编码的 `request.json`，
并把 `repository_root` 换成已存在的源码目录绝对路径：

```json
{
  "protocol": "codegraph.analyzer/1",
  "type": "analyze",
  "id": "example-1",
  "repository_root": "/absolute/project",
  "options": {
    "include_tests": false,
    "rust_dependency_scope": "repository-only"
  }
}
```

在 **POSIX shell** 中执行：

```sh
"/absolute/verified/pack/bin/codegraph-typescript-javascript" analyze < request.json
```

此处的重定向会在输入结束后产生 stdin EOF。上面的 PowerShell 示例借助脚本发送 UTF-8 输入。
Windows JSON 路径可写成 `C:/project`，省去反斜杠转义。

除 `files` 外，所有请求字段均为必填项，包括每种语言都必须提供的两个 `options` 字段。
非 Rust 分析器会忽略 `rust_dependency_scope`。源码目录与分析器包目录不能重叠。

## 文件选择与结果

| 请求中的 `files` | 含义 |
| --- | --- |
| 省略 | 按该语言的选集政策发现源码。 |
| 非空数组，如 `["src/main.ts"]` | 完整图节点严格来自这些文件；未选源码可提供语义上下文或图外符号。 |
| `[]` | 直接返回空图，不发现项目配置或目标 SDK；仍会检查源码与安装目录的边界。 |

选集路径必须唯一，并采用相对于源码根目录的 POSIX 路径格式。
`null`、重复路径及不安全路径均无效。发现哪些源码，取决于各语言的源码选集政策、
文件归属和包含/排除规则。具体规则见[选集政策](docs/engine-selection.md)。

进程从 stdin 读取一个 UTF-8 JSON 对象，随后等待 EOF；末尾换行可省略。
**EOF 表示输入完成。** stdout 只输出一个最终 JSON 结果和换行，随后进程退出。
诊断信息写入 stderr。

成功结果包含 `type: "completed"`、`outcome: "ok"`，以及具有以下字段的 `graph`：

| 图字段 | 内容 |
| --- | --- |
| `root_contains` | 顶层代码对象的 ID。 |
| `nodes` | 代码对象，包含身份、源码、包含关系和语义引用。 |
| `external_symbols` | 完整节点选集之外，被引用对象的描述。 |

引用目标可以是图内节点、图外符号，也可以是尚未解析的名称。
未解析引用属于合法的图数据。节点类别、字段、限制及错误结果见[公共协议](docs/protocol.md)。

使用代码图之前，调用方必须收到完整的成功结果，确认 `protocol`、`engine_version`、
`pack_id` 和请求 `id` 均匹配，**并确认进程以状态码 0 退出**。
仅收到结果、仅遇到 EOF，或仅有退出码 0 而缺少匹配结果，都不算成功。

调用方负责保持源码状态一致，持续读取 stdout 和 stderr，并限制资源与执行时间。
取消、超时或传输失败时，需要终止**整棵进程树**，等待根进程与后代进程全部退出。
在此之前，保持源码快照和已安装的分析器包不变。示例脚本没有实现这套完整的生产进程管理，
具体要求见[接入约定](docs/protocol.md)。

## 分析器运行时与项目目标 SDK

**分析器包（Analyzer Package）** 包含运行分析器所需的资源。
**分析器运行时（Analyzer Runtime）** 负责执行分析器；**项目目标 SDK（Project Target SDK）**
则提供被分析项目对应的标准库、类型声明或平台头文件，两者各有用途。

TypeScript/JavaScript、Java 和 C# 包分别附带 Node、JDK 和 .NET 执行资源。
Python 分析器使用原生可执行文件，内嵌 typeshed 声明。
各分析器的运行资源见[包布局](docs/pack-layout.md)。

项目目标仍可能需要本机已有的 SDK。例如，分析器即使用随包 JDK 25 运行，
Java 17 项目仍需匹配的目标 JDK；C# 需要匹配的引用程序集，
仅安装 .NET 运行时无法提供这些程序集。目标 SDK 发现只读取已有安装，
不会安装或升级 SDK，也不会运行项目构建脚本。

C/C++ 不要求先提供 `compile_commands.json` 或安装项目头文件，就能生成源码图。
缺少语义信息时，无法确认的片段保留为不透明节点，引用标为未解析。
可用的编译数据库只增强可靠绑定，不决定选中源码是否进入图。
具体约定见 [C/C++ 选集政策](docs/engine-selection.md#c--c-resolution)。

## 构建与验证

使用 Node **24.18.0** 和 pnpm **11.13.1**，先按[分发说明](docs/distribution.md)
和[工具链清单](resources/native-toolchains.json)准备固定版本的原生工具链与宿主构建工具。
`pnpm install` 只安装 Node 构建依赖。完整构建还需要锁定的 Java JAR，
下面的 Python 3 步骤会获取并校验这些文件；Java 构建脚本也需要 Python 3。

准备完成后，在仓库根目录执行：

```sh
pnpm install --frozen-lockfile
python3 analyzers/java/tools/fetch-deps.py
pnpm build:packs --output /absolute/fresh/codegraph-packs
```

此命令为当前受支持的宿主平台构建全部七个包。加上 `--id rust` 可只构建 Rust。
构建器使用 `--id`，工具链准备与完整验证命令使用 `--language`。
已有产物目录不会被覆盖，每次构建应使用新的输出目录。

归档与清单位于 `<output>/<id>/0.1.1/<target>/`；
校验并解包后的分析器包位于 `<output>/smoke-installed/<id>/0.1.1/<target>/`。

根 Cargo workspace 只构建 TypeScript/Java 薄入口。Rust 和 Python 使用各自的
manifest，以及 Rust **1.98.0**。所有 Cargo 命令均需经 `scripts/cargo-agent.sh`
执行；保留增量构建，不同分支的 worktree 使用各自的 target 目录。
各语言的构建与测试命令见上方分析器表格中的链接。

本地脚本检查可运行 `pnpm test` 和 `pnpm check:scripts`。
按[分发说明](docs/distribution.md)准备好固定 SDK 测试夹具后，
用以下命令执行完整原生验证：

```sh
node scripts/language-packs/verify-native-analyzers.mjs artifacts --build --language all
```

本地构建和聚焦检查不能替代完整的双平台 CI。
保留的测试套件见[测试迁移说明](docs/direct-test-migration.md)。

## 文档

| 文档 | 内容 |
| --- | --- |
| [公共协议](docs/protocol.md) | 请求、结果、图结构和调用方的进程管理职责。 |
| [选集政策](docs/engine-selection.md) | 项目发现、精确选集和各语言限制。 |
| [包布局](docs/pack-layout.md) | 入口、随包运行时和必需文件。 |
| [分发说明](docs/distribution.md) | 工具准备、包校验、CI 与发布。 |
| [术语表](GLOSSARY.md) | 共用的领域术语。 |
| [迁移来源](docs/migration.md) | 提取历史和职责归属。 |

---

采用 [Apache-2.0](LICENSE) 许可。提取自 Narrant 的代码保留原有许可，
第三方组件保留各自的许可与声明。此次提取始于
[Narrant #205](https://github.com/XLCYun/narrant/issues/205)。

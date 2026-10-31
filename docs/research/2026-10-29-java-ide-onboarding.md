# Java 分析器开箱即用的 IDE 产品参照

> 历史调研：文中“不执行项目代码、不下载”的早期约束已由接受的 [ADR 0002](../adr/0002-java-project-model-import.md) 替代。当前 Java 模型导入允许调用方授权的配置、插件、辅助逻辑和依赖获取；最终行为与验收以 ADR 和语言 README 为准。


2026-10-30 更新：用户已确认项目目标未知时，采用“用户默认 JDK → 随包 JDK”增强可靠关系，并标记实际分析平台；纯结构图作为最后降级。坏配置按明确选集与目录发现区别处理。用户另明确由调用方负责授权，CodeGraph 基础部件不重复设信任门槛。详细范围见[ADR 0002 草案](../adr/0002-java-project-model-import.md)。下文关于不执行、不下载及一律结构降级的建议记录原调查边界，不代表本轮修订方案。IDE 的版本、源码证据及限制保持原样。整体交付范围尚待最后汇总确认，功能尚未实现。

截至 2026 年 10 月 29 日，建议以 **VS Code Java 为主要产品参照**，以 IntelliJ IDEA 交叉检查降级与导入边界。VS Code Java 使用 Eclipse JDT LS，和 CodeGraph 共享 JDT 技术基础，最适合回答“没有完整项目环境时，先交付什么”。它的完整体验还依靠项目导入、默认项目 JDK、依赖解析和自动构建，不能整体照搬到只读分析器。

这里采用已经确定的“项目成员优先”方向。目录分析先尊重可确认的项目成员；显式文件列表保持精确选集；缺 SDK、依赖和完整构建模型不能阻止基础源码图。不运行项目构建、处理器或脚本，不自动下载，不扩公共协议。这些边界不因参考 IDE 而改变。

## 开箱即用由哪些层组成

| 层次 | 用户看到的结果 | 所需条件 |
| --- | --- | --- |
| 源码阅读 | 文件可打开，能看原文、结构和语法问题。 | 可读取的文件与解析器；不要求项目构建成功。 |
| 项目归属 | 知道哪些目录是源码，哪些是测试，文件属于哪个模块。 | 构建模型、已有 IDE 元数据，或有边界的布局与 package 推断。 |
| 语义服务 | 补全、跳转、调用和类型关系逐步可用。 | 相应源码上下文、项目 JDK 和已解析依赖；缺少条件时只能提供部分结果。 |
| 完整项目导入 | IDE 配置模块、依赖、生成源码并构建。 | Maven、Gradle、插件、缓存、网络和执行权限可能参与。 |

VS Code 官方文档明确把 Lightweight 的能力限定在源码和 JDK 范围，Standard 才解析导入依赖并构建项目。JDT LS 的 syntax server 直接跳过构建项目导入，但仍建立源码环境；它不是“完全没有 JDK 的纯语法解析器”。[产品说明](https://code.visualstudio.com/docs/java/java-project#_lightweight-mode)、[syntax server 不导入项目](https://github.com/eclipse-jdtls/eclipse.jdt.ls/blob/273dd9439867a6498efd4bbd0dcf090e37f6ea5e/org.eclipse.jdt.ls.core/src/org/eclipse/jdt/ls/core/internal/syntaxserver/SyntaxProjectsManager.java#L116-L119)、[语法服务导航测试](https://github.com/eclipse-jdtls/eclipse.jdt.ls/blob/273dd9439867a6498efd4bbd0dcf090e37f6ea5e/org.eclipse.jdt.ls.tests.syntaxserver/src/org/eclipse/jdt/ls/core/internal/syntaxserver/SyntaxServerTest.java#L117-L219)。

因此，值得对标的是能力逐层增加、缺条件时仍保留有用内容的产品行为。CodeGraph 输出图事实，还需要比编辑器导航更严格地区分已确认关系与未知关系。

## VS Code Java 的真实场景

下表区分文件仍可编辑、语言服务确有回退、项目语义已恢复三种情况。编辑器还能显示文件，不能单独证明项目导入成功。

| 场景 | 支持条件与保留能力 | 失败或回退边界 | 用户交互与后台动作 |
| --- | --- | --- | --- |
| 单独打开一个现有 `.java` | JDT LS 可为未纳管文件建立 default project 中的链接编译单元；还有默认项目不可用时创建 working copy 的路径。 | 默认只报告语法问题，并提示这是 non-project file；不因此获得业务依赖的完整 classpath。 | 无需创建 Maven/Gradle 项目；仍使用默认 JRE container。见[单文件入口](https://github.com/eclipse-jdtls/eclipse.jdt.ls/blob/273dd9439867a6498efd4bbd0dcf090e37f6ea5e/org.eclipse.jdt.ls.core/src/org/eclipse/jdt/ls/core/internal/JDTUtils.java#L272-L329)、[诊断政策](https://github.com/eclipse-jdtls/eclipse.jdt.ls/blob/273dd9439867a6498efd4bbd0dcf090e37f6ea5e/org.eclipse.jdt.ls.core/src/org/eclipse/jdt/ls/core/internal/handlers/DiagnosticsHandler.java#L31-L81)。 |
| 打开无构建配置的目录，再打开 Java 文件 | 尝试建立 invisible project。package 与目录后缀匹配时向上推导 source root；无 package 时可用文件父目录。显式 `java.project.sourcePaths` 可覆盖推断。 | 推断必须留在 workspace 内；已有 visible project 或被识别为成熟构建项目的源码会阻止这条路径。 | 使用打开的 trigger file，并有限搜索其他可能源根；不是把整仓所有 Java 文件无差别塞进一个项目。见[invisible project 条件](https://github.com/eclipse-jdtls/eclipse.jdt.ls/blob/273dd9439867a6498efd4bbd0dcf090e37f6ea5e/org.eclipse.jdt.ls.core/src/org/eclipse/jdt/ls/core/internal/managers/InvisibleProjectImporter.java#L126-L186)、[推断规则](https://github.com/eclipse-jdtls/eclipse.jdt.ls/blob/273dd9439867a6498efd4bbd0dcf090e37f6ea5e/org.eclipse.jdt.ls.core/src/org/eclipse/jdt/ls/core/internal/managers/InvisibleProjectImporter.java#L538-L566)、[搜索上限](https://github.com/eclipse-jdtls/eclipse.jdt.ls/blob/273dd9439867a6498efd4bbd0dcf090e37f6ea5e/org.eclipse.jdt.ls.core/src/org/eclipse/jdt/ls/core/internal/managers/InvisibleProjectImporter.java#L299-L319)。 |
| 有效 Maven 项目 | 经 M2E 读取与配置项目，实际项目模型负责源码根、模块、依赖。已有 workspace 模块也可参与依赖解析。 | 不等于只静态读取几个 POM 字段；解析父模型、插件及依赖可能需要更多资源。 | 默认启用 Maven importer；调用 M2E 的 `importProjects`，不是要求先运行外部 `mvn` CLI。见[Maven 导入](https://github.com/eclipse-jdtls/eclipse.jdt.ls/blob/273dd9439867a6498efd4bbd0dcf090e37f6ea5e/org.eclipse.jdt.ls.core/src/org/eclipse/jdt/ls/core/internal/managers/MavenProjectImporter.java#L293-L395)、[M2E 职责](https://eclipse.dev/m2e/documentation/m2e-documentation.html)。 |
| 有效 Gradle 项目或多模块构建 | Buildship 同步原生构建模型，源码与模块来自 Gradle 模型；客户端设置还说明 Gradle 项目不能部分导入。 | 模型取得失败会保留错误状态；脚本执行失败不能靠读取文件名修复成等价模型。 | 使用 wrapper、指定版本或本地 Gradle，运行同步并可启动 daemon。见[同步入口](https://github.com/eclipse-jdtls/eclipse.jdt.ls/blob/273dd9439867a6498efd4bbd0dcf090e37f6ea5e/org.eclipse.jdt.ls.core/src/org/eclipse/jdt/ls/core/internal/managers/GradleProjectImporter.java#L492-L559)、[项目选择设置](https://github.com/redhat-developer/vscode-java/blob/4e49f187a903b8c7b1ed7277a3b2535691fd59f3/package.json#L506-L518)、[Gradle Tooling API](https://docs.gradle.org/current/userguide/tooling_api.html)。 |
| 项目缺第三方依赖 | 已有源文件仍能成为 compilation unit、建立工作副本并做恢复解析；缺类型和导入可以表现为诊断、跳转无目标。 | 没有“缺一个 JAR 就把整个 workspace 自动切回 Lightweight”的保证；源码模型、可用绑定和问题诊断可同时存在。 | 普通项目保留语义诊断；syntax/non-project 模式会过滤缺类型、缺导入、未定义方法等诊断。过滤错误不代表目标已确认。见[恢复与工作副本](https://github.com/eclipse-jdtls/eclipse.jdt.ls/blob/273dd9439867a6498efd4bbd0dcf090e37f6ea5e/org.eclipse.jdt.ls.core/src/org/eclipse/jdt/ls/core/internal/handlers/BaseDocumentLifeCycleHandler.java#L315-L338)、[语法诊断过滤](https://github.com/eclipse-jdtls/eclipse.jdt.ls/blob/273dd9439867a6498efd4bbd0dcf090e37f6ea5e/org.eclipse.jdt.ls.core/src/org/eclipse/jdt/ls/core/internal/handlers/BaseDiagnosticsHandler.java#L70-L114)。 |
| 坏 POM、坏 Gradle 脚本或导入失败 | 文本编辑仍存在；若文件尚未纳管，标准服务会尝试 invisible project，失败后可能进入 fake compilation unit 的单文件路径。 | **没有统一的“坏构建模型自动恢复完整项目语义”承诺。** 已有资源分支未必再走 fake 路径；源码位于带构建描述的 `src` 下时，invisible importer 还会主动避让。 | 导入错误可记录、通知或形成 marker，其他 workspace root 可继续处理。见[标准文件解析分支](https://github.com/eclipse-jdtls/eclipse.jdt.ls/blob/273dd9439867a6498efd4bbd0dcf090e37f6ea5e/org.eclipse.jdt.ls.core/src/org/eclipse/jdt/ls/core/internal/handlers/DocumentLifeCycleHandler.java#L57-L91)、[成熟项目避让](https://github.com/eclipse-jdtls/eclipse.jdt.ls/blob/273dd9439867a6498efd4bbd0dcf090e37f6ea5e/org.eclipse.jdt.ls.core/src/org/eclipse/jdt/ls/core/internal/managers/InvisibleProjectImporter.java#L465-L479)、[导入错误收集](https://github.com/eclipse-jdtls/eclipse.jdt.ls/blob/273dd9439867a6498efd4bbd0dcf090e37f6ea5e/org.eclipse.jdt.ls.core/src/org/eclipse/jdt/ls/core/internal/managers/ProjectsManager.java#L155-L187)。 |
| 缺声明目标对应的 JDK | 工具运行 JRE 与项目 JDK 分开；非法 runtime 路径会通知并跳过。源码仍可阅读，但项目语义取决于实际 VM/classpath 配置。 | JDT LS 可接受兼容但非 strictly compatible 的 VM；这不等于 CodeGraph 已证明了同 major 目标平台。 | Configure Java Runtime 可调整映射或引导下载。见[runtime 验证](https://github.com/eclipse-jdtls/eclipse.jdt.ls/blob/273dd9439867a6498efd4bbd0dcf090e37f6ea5e/org.eclipse.jdt.ls.core/src/org/eclipse/jdt/ls/core/internal/JVMConfigurator.java#L108-L203)、[兼容 VM 接受规则](https://github.com/eclipse-jdtls/eclipse.jdt.ls/blob/273dd9439867a6498efd4bbd0dcf090e37f6ea5e/org.eclipse.jdt.ls.core/src/org/eclipse/jdt/ls/core/internal/JVMConfigurator.java#L232-L250)。 |

### 项目成员与 source root 不是全仓扫描的同义词

Maven importer 会在 workspace 中搜索构建入口，发现一个入口后，普通扫描不再遍历它的全部后代，而把模型交给 M2E 解释；它另外排除 `target` 中的 POM。没有上层构建入口的 workspace 可以发现多个独立项目。这说明“一个目录包含多个项目”与“一个聚合项目接纳哪些模块”是两层决策，不能把前者理解为每个相邻 Java 目录都属于同一模块。[构建入口扫描](https://github.com/eclipse-jdtls/eclipse.jdt.ls/blob/273dd9439867a6498efd4bbd0dcf090e37f6ea5e/org.eclipse.jdt.ls.core/src/org/eclipse/jdt/ls/core/internal/managers/MavenProjectImporter.java#L94-L113)、[扫描停止规则](https://github.com/eclipse-jdtls/eclipse.jdt.ls/blob/273dd9439867a6498efd4bbd0dcf090e37f6ea5e/org.eclipse.jdt.ls.core/src/org/eclipse/jdt/ls/core/internal/managers/BasicFileDetector.java#L148-L168)。

invisible project 的包路径不匹配时推断返回空；空源码还会参考附近非空文件或路径。此类启发式适合把文件放进可用编辑环境，不是 package 与 source root 已被构建模型确认的证明。声明根与推断根应保留不同的可信程度。[空文件与 package 推断](https://github.com/eclipse-jdtls/eclipse.jdt.ls/blob/273dd9439867a6498efd4bbd0dcf090e37f6ea5e/org.eclipse.jdt.ls.core/src/org/eclipse/jdt/ls/core/internal/managers/InvisibleProjectImporter.java#L482-L566)。

### 测试和排除目录

JDT LS 的 `isTestFile` 检查源码 classpath entry 的 `isTest` 或 Maven/Gradle scope 属性，默认项目文件返回 false。它没有在这个接口中用 `FooTest.java` 文件名判定测试。syntax server 为常见 Maven/Gradle布局添加 `src/main/java`、`src/test/java`，也不能推出它像 CodeGraph 一样默认排除测试节点。[测试归属](https://github.com/eclipse-jdtls/eclipse.jdt.ls/blob/273dd9439867a6498efd4bbd0dcf090e37f6ea5e/org.eclipse.jdt.ls.core/src/org/eclipse/jdt/ls/core/internal/commands/ProjectCommand.java#L507-L583)、[语法服务的常见根](https://github.com/eclipse-jdtls/eclipse.jdt.ls/blob/273dd9439867a6498efd4bbd0dcf090e37f6ea5e/org.eclipse.jdt.ls.core/src/org/eclipse/jdt/ls/core/internal/syntaxserver/SyntaxDocumentLifeCycleHandler.java#L133-L154)。

当前稳定扩展默认的项目导入排除为 `**/node_modules/**`、`**/.metadata/**`、`**/archetype-resources/**`、`**/META-INF/maven/**`；资源刷新另过滤 `node_modules` 和 `.git`。这些是不同阶段的政策，不是一个“忽略任何 build/target 目录”的统一规则。实际模型可把生成源码列为源码根。[默认排除与刷新设置](https://github.com/redhat-developer/vscode-java/blob/4e49f187a903b8c7b1ed7277a3b2535691fd59f3/package.json#L535-L556)、[生成源码的原生测试](https://github.com/eclipse-jdtls/eclipse.jdt.ls/blob/273dd9439867a6498efd4bbd0dcf090e37f6ea5e/org.eclipse.jdt.ls.tests/src/org/eclipse/jdt/ls/core/internal/managers/GradleProjectImporterTest.java#L584-L623)。

## Lightweight Standard Hybrid 的准确含义

`java.server.launchMode` 只有以下三个产品选项。Syntax 是 Lightweight 所启动服务的内部称呼，不是第四个设置值。

| 选项 | 当前稳定版行为 |
| --- | --- |
| `LightWeight` | 只要求 syntax server；不导入构建项目，提供较快的结构与有限导航能力，可手动切换 Standard。 |
| `Standard` | 要求完整标准语言服务，承担项目导入、依赖、构建和完整语言功能。 |
| `Hybrid`，默认 | 先由辅助 syntax server 服务，按启动政策同时推进 Standard；Standard 就绪后停止 syntax server。 |

[模式设置定义](https://github.com/redhat-developer/vscode-java/blob/4e49f187a903b8c7b1ed7277a3b2535691fd59f3/package.json#L354-L369)、[服务选择与信任判断](https://github.com/redhat-developer/vscode-java/blob/4e49f187a903b8c7b1ed7277a3b2535691fd59f3/src/extension.ts#L216-L224)、[启动与接管](https://github.com/redhat-developer/vscode-java/blob/4e49f187a903b8c7b1ed7277a3b2535691fd59f3/src/extension.ts#L464-L483)。

这里有一个会影响产品判断的文档差异。官方项目管理页面仍描述 Hybrid 首次询问是否导入；**v1.57.0 的 `java.project.importOnFirstTimeStartup` 默认已经是 `automatic`**。设成 `interactive` 且首次打开包含构建文件的 workspace 时，才出现 Yes / Always / Later；未信任 workspace 则强制 Lightweight。不能据旧页面断言当前默认每次都先征得项目导入同意。[当前默认值](https://github.com/redhat-developer/vscode-java/blob/4e49f187a903b8c7b1ed7277a3b2535691fd59f3/package.json#L572-L581)、[实际判断](https://github.com/redhat-developer/vscode-java/blob/4e49f187a903b8c7b1ed7277a3b2535691fd59f3/src/extension.ts#L502-L521)、[询问动作](https://github.com/redhat-developer/vscode-java/blob/4e49f187a903b8c7b1ed7277a3b2535691fd59f3/src/extension.ts#L854-L864)。

## JDK 与离线依赖怎样参与

**没有声明目标版本时，VS Code 并没有保持“目标未知”。** unmanaged folder/单文件使用默认项目 JDK。v1.57.0 在没有配置项目默认值时，还可以把随包 tooling JRE 用作初始项目运行时。JDT LS 由默认 VM 设置编译级别，最新受支持 VM 的无设置项目甚至默认启用 preview。这是一套编辑器的默认项目政策，不能当成原项目目标版本的证据。[稳定版默认运行时选择](https://github.com/redhat-developer/vscode-java/blob/4e49f187a903b8c7b1ed7277a3b2535691fd59f3/src/requirements.ts#L92-L108)、[JDT LS 默认编译级别](https://github.com/eclipse-jdtls/eclipse.jdt.ls/blob/273dd9439867a6498efd4bbd0dcf090e37f6ea5e/org.eclipse.jdt.ls.core/src/org/eclipse/jdt/ls/core/internal/JVMConfigurator.java#L280-L348)。

| 资源 | IDE 何时使用 | 对 CodeGraph 的含义 |
| --- | --- | --- |
| 工具运行 JRE/JDK | 支持的平台扩展随包提供；v1.57.0 的最低工具运行版本为 25。Universal 版需找到满足要求的工具 JDK，否则提示安装。 | 只证明分析器可以运行。不能替代已声明目标 major。见[工具要求](https://github.com/redhat-developer/vscode-java/blob/4e49f187a903b8c7b1ed7277a3b2535691fd59f3/src/requirements.ts#L35-L90)、[启动失败提示](https://github.com/redhat-developer/vscode-java/blob/4e49f187a903b8c7b1ed7277a3b2535691fd59f3/src/requirements.ts#L111-L112)。 |
| unmanaged 的本地 JAR | 默认 `lib/**/*.jar`，也能用 referencedLibraries 的 include/exclude/sources；JDT LS 收集既有二进制并更新 classpath。 | 现成本地 JAR 可作为可选静态语义资源；并不要求运行它，也不应因找不到它拒绝基础图。见[本地库设置](https://github.com/redhat-developer/vscode-java/blob/4e49f187a903b8c7b1ed7277a3b2535691fd59f3/package.json#L651-L679)、[实际收集](https://github.com/eclipse-jdtls/eclipse.jdt.ls/blob/273dd9439867a6498efd4bbd0dcf090e37f6ea5e/org.eclipse.jdt.ls.core/src/org/eclipse/jdt/ls/core/internal/managers/UpdateClasspathJob.java#L93-L112)。 |
| Maven 本地仓库/已导入模块 | 经 M2E 的模型和依赖解析使用；M2E 也可下载缺失依赖。源码包下载是另一设置，默认 false 不代表二进制依赖下载被禁止。 | “已有缓存能用”需要知道准确依赖身份；不能据文件名扫描任意缓存 JAR 后全部加入。见[M2E 能力](https://eclipse.dev/m2e/documentation/m2e-documentation.html)、[Maven offline 与 source 设置](https://github.com/redhat-developer/vscode-java/blob/4e49f187a903b8c7b1ed7277a3b2535691fd59f3/package.json#L685-L711)。 |
| Gradle 缓存 | Gradle 按模型和缓存元数据解析。offline 只限制依赖解析访问网络，缓存缺失仍失败。 | 离线导入仍要运行 Gradle/配置脚本，不能作为 CodeGraph 不执行项目代码的替代方案。见[缓存与离线规则](https://docs.gradle.org/current/userguide/dependency_caching.html#sec:offline-mode)、[offline 参数传递](https://github.com/eclipse-jdtls/eclipse.jdt.ls/blob/273dd9439867a6498efd4bbd0dcf090e37f6ea5e/org.eclipse.jdt.ls.core/src/org/eclipse/jdt/ls/core/internal/managers/GradleProjectImporter.java#L532-L559)。 |

## 哪些 IDE 后台动作不能照搬

VS Code 的 Maven、Gradle importer、自动构建默认启用；Maven/Gradle offline 默认关闭。Maven 走 M2E 的嵌入式 Maven 能力，不能笼统说它每次都执行外部 `mvn`。Gradle 走 Buildship/Tooling API，获取模型会评估构建配置；wrapper 或指定 Gradle 版本还可能需要取得相应分发包。[当前默认设置](https://github.com/redhat-developer/vscode-java/blob/4e49f187a903b8c7b1ed7277a3b2535691fd59f3/package.json#L685-L695)、[Gradle 设置](https://github.com/redhat-developer/vscode-java/blob/4e49f187a903b8c7b1ed7277a3b2535691fd59f3/package.json#L773-L818)、[自动构建](https://github.com/redhat-developer/vscode-java/blob/4e49f187a903b8c7b1ed7277a3b2535691fd59f3/package.json#L876-L881)、[M2E FAQ](https://eclipse.dev/m2e/documentation/m2e-faq.html)、[Tooling API](https://docs.gradle.org/current/userguide/tooling_api.html)。

注解处理也属于完整导入/构建能力。版本化 README 标明 Maven annotation processing 自动支持；Gradle APT 与 Lombok 支持默认启用，Gradle 集成会登记 processor JAR、启用 APT 并设置生成目录。这不代表每次打开任意文件都执行所有处理器，但足以说明完整 IDE 语义可能依靠生成代码。[Maven 能力声明](https://github.com/redhat-developer/vscode-java/blob/4e49f187a903b8c7b1ed7277a3b2535691fd59f3/README.md#L27-L46)、[Gradle APT 默认值](https://github.com/redhat-developer/vscode-java/blob/4e49f187a903b8c7b1ed7277a3b2535691fd59f3/package.json#L836-L841)、[Lombok 默认值](https://github.com/redhat-developer/vscode-java/blob/4e49f187a903b8c7b1ed7277a3b2535691fd59f3/package.json#L401-L406)、[APT 实际配置](https://github.com/eclipse-jdtls/eclipse.jdt.ls/blob/273dd9439867a6498efd4bbd0dcf090e37f6ea5e/org.eclipse.jdt.ls.core/src/org/eclipse/jdt/ls/core/internal/managers/GradleBuildSupport.java#L238-L259)。

JDK 下载也要区分来源。Java 项目管理 UI 提供下载引导；所核对的初始化路径没有“自动安装缺失项目 JDK”的步骤。Gradle 构建若配置了 toolchain 下载仓库，则可能由 Gradle 自动取得匹配 JDK，属于构建模型带来的动作，不能理解为 VS Code 对所有 Java 目录的默认行为。[项目 JDK 下载入口](https://code.visualstudio.com/docs/java/java-project#_download-jdks)、[Gradle toolchain 自动供给](https://docs.gradle.org/current/userguide/toolchains.html#sub:auto_provisioning)。

IDE 会维护 workspace 项目、classpath、链接、索引、构建和生成输出。稳定版默认不把 Eclipse 元数据写在项目根，但这不使完整导入成为只读操作。CodeGraph 应借鉴源码根推断、隔离的项目上下文和渐进能力，保持不执行构建、不加载处理器、不下载、不写源码目录的边界。[元数据位置默认值](https://github.com/redhat-developer/vscode-java/blob/4e49f187a903b8c7b1ed7277a3b2535691fd59f3/package.json#L566-L570)、[项目与输出目录创建](https://github.com/eclipse-jdtls/eclipse.jdt.ls/blob/273dd9439867a6498efd4bbd0dcf090e37f6ea5e/org.eclipse.jdt.ls.core/src/org/eclipse/jdt/ls/core/internal/managers/ProjectsManager.java#L429-L478)。

## IntelliJ IDEA 的交叉验证

IntelliJ IDEA 使用自己的 PSI 与项目模型。下面的产品边界支持分层方向，不能证明 CodeGraph 锁定的 JDT 已有相同能力。

| 情况 | 官方行为与结论 |
| --- | --- |
| 单文件 LightEdit | 可编辑、折叠、格式化；没有常规代码补全与导航。连成熟 IDE 也会把文件可读与项目语义分开。[LightEdit 2026.2 Help](https://www.jetbrains.com/help/idea/lightedit-mode.html) |
| 未信任项目 Safe Mode | 可浏览源码，禁 Maven/Gradle/sbt 导入、依赖解析、startup tasks 等执行；信任后才完整初始化。[Project security 2026.2 Help](https://www.jetbrains.com/help/idea/project-security.html) |
| 完整构建导入 | 在独立 Java 进程执行构建脚本，根据模型配置 source/test roots、模块和依赖。离线模式也不能等同于纯静态阅读。[导入过程](https://www.jetbrains.com/help/idea/importing-process.html)、[离线工作](https://www.jetbrains.com/help/idea/working-offline.html) |
| Gradle preview | 所核对源码明确不向 Gradle 请求模型，以便慢项目或无效脚本仍能打开；默认 preview 只建立一个模块与 content root，不证明完整 sources/tests/dependencies。[preview 分支](https://github.com/JetBrains/intellij-community/blob/0698a5fe297dfa08069f015cc423a6a85ed7fc60/plugins/gradle/src/org/jetbrains/plugins/gradle/service/project/GradleProjectResolver.java#L174-L184)、[默认 preview 内容](https://github.com/JetBrains/intellij-community/blob/0698a5fe297dfa08069f015cc423a6a85ed7fc60/plugins/gradle/src/org/jetbrains/plugins/gradle/service/project/GradlePreviewCustomizer.kt#L37-L63) |
| Maven 静态预导入 | 所核对源码说明先只读 XML、做基础插值，不访问网络/daemon、不求值用户代码；模型可能不完整甚至局部不准确。后续 dynamic sync 才进行依赖解析、下载和工具调用。[官方仓库同步说明](https://github.com/JetBrains/intellij-community/blob/0698a5fe297dfa08069f015cc423a6a85ed7fc60/plugins/maven/.ai/sync-process.md#L5-L29) |

这也支持把“没有完整构建环境”作为能力降级，而不是取消源码阅读。IntelliJ 的 preview、static sync 和 VS Code 的 syntax/invisible project 是不同机制，不能归结成“任何坏配置都能得到正确项目模型”。

## 对 CodeGraph 的建议范围

以下是由产品证据推导的建议，尚未成为新规范。

第一层优先保证所选源码的基础图与原文。项目声明、目标 JDK 和项目依赖负责补充上下文；缺少这些条件不应阻止显式选中文件出现，已有错误或冲突声明的政策另见下表。完整节点仍只来自选集，未选源码只供上下文或 repository external；原有相对路径身份和自身 `code` 规则应保持稳定，不随语义增强是否可用而改变。[公共图契约](https://github.com/XLCYun/CodeGraph/blob/949a61b34971a890796c4d200762bf65cf7da2c3/docs/protocol.md#graph)、[当前 Java 原文与身份构造](https://github.com/XLCYun/CodeGraph/blob/949a61b34971a890796c4d200762bf65cf7da2c3/analyzers/java/src/dev/codegraph/java/Analyzer.java#L245-L281)。

目录发现继续遵守已经选择的项目成员优先。可静态确认的模型提供成员与模块边界；缺声明的普通目录可以借鉴常见布局和 package 推导；已识别项目内的未声明目录不应被一个全仓扫描静默加入。一个目录包含多个独立构建入口时，分别建立上下文比把同包同名源码混在一起更接近 IDE 做法。其推断细则应写成可复现例子，由实现者核查，不需要用户选择解析器内部参数。

无目标声明时，建议先保留“项目目标未知”，用固定 JDT 的结构解析能力保原文与可确认声明，暂不据任意运行 JVM 的 API 建立目标版本事实。代价是某些看似简单的调用也会 unresolved；收益是同一源码不会因机器恰好装了更高 JDK 而获得并不存在于原目标的 API。若希望像 IDE 一样有默认项目目标，应把具体版本与选择政策明确写成产品约定；这仍不能覆盖已声明 release 的同 major 匹配规则。

同一锁定 JDT 的本地探针说明这种分层有实际必要：现代 compliance 在无标准库且 `resolve=true` 时可直接抛 `Missing system library`，而语法解析可保留结构与原文；有标准库时，错误参数数量或歧义调用也可能得到带 key、非 recovered 的 binding。因此，IDE 能提供导航、或 binding 存在，都不足以直接证明可输出确定图边。这里是本地组件证据，不是对 VS Code 或 IntelliJ 的实机测试。[本地探针记录](/Users/lizhanfa/.codex/cangyun-loop/github.com/XLCYun/CodeGraph/evidence/java-capability-479678dd/summary.json)。

对于已有错误声明，建议把尚未确认的 Q2 细化为“这次选集是否依赖该声明来确定”。这比把所有问题统一列为 fatal 或统一忽略更贴合上述产品实践。

“当前不支持”还应与“必须执行后才知道结果”分开。compiler plugin 中字面的 `<release>17</release>`、同一 POM 中简单的 `${java.version}` 引用，本身可以静态读取或代入；当前 CodeGraph 拒绝整个 plugins 块、没有通用属性插值，是现有支持范围，不能据此把这些声明都称为动态。IntelliJ 的静态 Maven 预导入也明确包含基础插值。[当前 Java 模型限制](https://github.com/XLCYun/CodeGraph/blob/949a61b34971a890796c4d200762bf65cf7da2c3/analyzers/java/src/dev/codegraph/java/ProjectModel.java#L323-L324)、[当前 release 读取](https://github.com/XLCYun/CodeGraph/blob/949a61b34971a890796c4d200762bf65cf7da2c3/analyzers/java/src/dev/codegraph/java/ProjectModel.java#L401-L411)、[静态插值的产品实践](https://github.com/JetBrains/intellij-community/blob/0698a5fe297dfa08069f015cc423a6a85ed7fc60/plugins/maven/.ai/sync-process.md#L5-L13)。

相反，Gradle 用环境变量选源码目录、调用自定义函数计算依赖，或插件实际生成新源码时，仅凭声明文本未必能知道最终源集。Maven profile 的文本也可以读，但是否激活还可能依赖环境、文件或用户选择；出现 profile 不自动等于必须执行代码。要分别说明哪些信息已确定、哪些条件未知，避免把可静态读取但尚未实现的能力包装成语言本身做不到。[Gradle 模型会执行配置](https://docs.gradle.org/current/userguide/tooling_api.html)、[Maven profile 激活规则](https://maven.apache.org/guides/introduction/introduction-to-profiles.html)。

| 条件 | 建议讨论的结果 | 理由 |
| --- | --- | --- |
| 缺配置、缺 release、缺精确 JDK、缺项目 JAR 或处理器产物 | 仍给基础图；缺少条件的引用 unresolved。 | 属于已确定的开箱即用目标。 |
| 字面或简单属性可确定的 release、source roots | 在约定的静态支持范围内读取，不因位于 plugins 或 properties 下就一概称为动态。 | 这是静态模型的能力范围问题。 |
| 成员或依赖需要运行条件、自定义函数或代码生成才能确定 | 只采用能够独立证实的信息；不足以证明目录成员时明确说明无法确定选集。 | CodeGraph 跳过执行后不能声称已得到完整有效构建模型。 |
| 明确文件列表，但 JSON/XML 已损坏或声明互相冲突 | 可考虑保留这些文件的基础图，弃用有问题的语义上下文；也可选择继续明确失败。 | 文件成员已由调用方给出，是否让坏控制文件阻止原文与结构属于产品选择。 |
| 目录发现，而且坏声明直接决定模块或 source roots | 建议明确失败，而不是猜成空成功或全仓扫描。 | 调用方要的是可信项目成员，当前无法证明选集完整。 |
| 真正读取失败、路径逃逸、读取越权、资源上限、分析器包损坏、内部错误 | 保持明确失败。 | 这些不属于缺构建上下文；降级不能掩盖基础分析器失效。 |

调研时需要用户决定的主要是两件事。第一项已于 2026-10-30 选择“目标未知”；第二项仍待确认。

1. **没有目标声明时，保持目标未知，还是约定一个默认项目 Java 版本。** 前者更保守，结构先可用；后者能提供更多标准库关系，但要接受这是 CodeGraph 的默认目标，不是发现出的项目事实。
2. **显式文件遇到已存在的坏声明时，是否仍交付基础图。** 建议将“目录发现依赖坏声明应失败”与“明确选集可以舍弃坏上下文继续”分开考虑。此前的“非法明确配置失败”建议还没有用户确认，IDE 证据支持进一步讨论这个区分。

这两件事可以用“无版本的单文件”“有坏 POM 的明确选集”“坏 POM 下整目录发现”三个场景选择结果，无需用户判断 AST、binding 或语言服务器的细节。

## 2026-10-30 补充：默认平台的能力与代价

项目目标未知与实际选用默认分析平台可以同时成立。当前 CodeGraph 的 SDK 发现器已经读取 `JAVA_HOME`、`JDK_HOME`、PATH 和安装目录，但会按 major 合并并优选补丁；它尚未实现用户默认值优先和平台来源语义。薄入口仍可运行随包 JVM，只读取较旧用户 JDK 的平台资料，不必改用旧 JVM 启动分析器。

用户已确认默认平台增强：没有目标声明时先采用有效用户默认 JDK，不可用时再随包 JDK。现有节点 `extensions` 可以新增 Java 平台语义，不需要新增顶层 JSON 字段；外部消费者仍需理解新键，不能声称它们今天已经会展示默认假设。源代码未变但分析平台改变时，关系也可能改变，比较两次图需要考虑分析上下文。

纯语法降级保留原文、声明、层级、自身 code 与稳定身份，但当前构图逻辑的同文件调用、继承、实现和方法引用也会失去确定目标。例如 `run(){ helper(); }` 即使与 `helper` 同文件，也会 unresolved；以方法 ID 反查调用者时不能命中。图差异产品仍可比较方法体，但调用链和影响传播会中断。名称匹配无法区分重载或同名对象，不能作为确定边的补偿。

高 JDK 可提高 API 覆盖，但不保证还原旧平台语义。官方区分源码、二进制与行为兼容；`javac --release` 为有限历史版本约束语言与公开 API。仅设置 source/target 不具备同样效果。[Oracle 迁移说明](https://docs.oracle.com/en/java/javase/25/migrate/migrating-jdk-8-later-jdk-releases.html)、[javac release](https://docs.oracle.com/en/java/javase/25/docs/specs/man/javac.html#standard-options)。

本轮固定 JDT Core 3.47.0 的微探针进一步表明，当前独立 `ASTParser.createASTs` 路径接入 JDK 25 后，即便 source/target/compliance 8、release enabled，仍会使用新平台 API。`List.of` 获得非 recovered 绑定且没有错误；旧 JAXB 不可见。对同一合法源码里的两个项目方法重载，JDK 25 的 `ByteBuffer.flip` 返回类型会使 JDT 选择 `select(ByteBuffer)`，而 javac 的真正 release 8 视图选择 `select(Buffer)`。这证明平台替换也可能改变项目内部关系。固定 ASTParser 的环境路径装配和 ECJ CLI 历史平台装配是不同入口，不能以 CLI 能力推断独立 API 已正确处理。[ASTParser 固定源码](https://github.com/eclipse-jdt/eclipse.jdt.core/blob/R4_41/org.eclipse.jdt.core/dom/org/eclipse/jdt/core/dom/ASTParser.java#L236)、[ECJ CLI 固定源码](https://github.com/eclipse-jdt/eclipse.jdt.core/blob/R4_41/org.eclipse.jdt.core.compiler.batch/src/org/eclipse/jdt/internal/compiler/batch/Main.java#L5204)。

新证据已复制到[平台探针摘要](/Users/lizhanfa/.codex/cangyun-loop/github.com/XLCYun/CodeGraph/evidence/java-platform-fallback-479678dd/summary.json)，包含源码、完整命令、JDT/javac/javap 结果、22 项工具校验及进程退出核验。根执行者读回了新 API 和仓内重载反例并核对复制 SHA。没有新工具下载、项目模型导入、构建脚本或注解处理器执行；微样例使用 `-proc:none`，不代表公开入口或安装包验收通过。

## 版本来源与证据边界

| 对象 | 本文固定来源 |
| --- | --- |
| VS Code Java 客户端 | 稳定版 `v1.57.0`，2026-10-13 发布；源码提交 `4e49f187a903b8c7b1ed7277a3b2535691fd59f3`。[发布记录](https://github.com/redhat-developer/vscode-java/releases/tag/v1.57.0)、[版本字段](https://github.com/redhat-developer/vscode-java/blob/4e49f187a903b8c7b1ed7277a3b2535691fd59f3/package.json#L8)。截至查询还可见 2026-10-16 的 `v1.58.0` prerelease，本文没有把它当稳定版。 |
| JDT LS | 2026-10-28 提交 `273dd9439867a6498efd4bbd0dcf090e37f6ea5e`；POM 标示 `1.64.0-SNAPSHOT`。[提交](https://github.com/eclipse-jdtls/eclipse.jdt.ls/commit/273dd9439867a6498efd4bbd0dcf090e37f6ea5e)、[版本字段](https://github.com/eclipse-jdtls/eclipse.jdt.ls/blob/273dd9439867a6498efd4bbd0dcf090e37f6ea5e/pom.xml#L5-L9)。 |
| IntelliJ IDEA | 官方 `2026.2 Help`，以及 2026-10-29 的源码快照 `0698a5fe297dfa08069f015cc423a6a85ed7fc60`。产品文档与源码快照分别使用，不把快照全部细节视为已在 2026.2 二进制发布。[源码快照](https://github.com/JetBrains/intellij-community/commit/0698a5fe297dfa08069f015cc423a6a85ed7fc60)。 |
| CodeGraph | 本地调查基线 `949a61b34971a890796c4d200762bf65cf7da2c3`；建议与上游事实分节陈述。 |

JDT LS 快照和 VS Code 稳定扩展的源码分别固定。VS Code 发布流程可以构建当时的 JDT LS HEAD，或取指定 milestone；仅凭扩展 Git tag 不能确认某个已发布 VSIX 中的服务端精确提交。这里的服务端结论是固定源码行为，不是该 VSIX 的实机验收；客户端默认值则取稳定 tag。[发布流程](https://github.com/redhat-developer/vscode-java/blob/4e49f187a903b8c7b1ed7277a3b2535691fd59f3/.github/workflows/release.yml#L68-L115)。

官方网页是访问日版本，部分 Wiki 和项目管理页保留较早描述；涉及当前默认值时以上述版本化源码为准。IDE 对标部分依据文档、源码和测试预期，未启动 IDE 或执行项目构建。另行完成的 JDT 组件探针准备了锁定分析器 JAR 和固定生产者 JDK，仅用于独立语法与明确 Java 25 目标的语义对照，没有替被分析项目安装目标 SDK。CodeGraph 建议仍需在自己的固定 JDT 与跨平台公开入口上验证，不能把 IDE 的产品说明当作实现完成证明。

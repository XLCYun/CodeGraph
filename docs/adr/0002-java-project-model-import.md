---
status: accepted
---

# Java 项目模型导入与目标未知时的基础图

2026-10-30，用户确认 Issue #10 的完整范围并授权进入实现。本文记录接受的产品行为与实现边界；实现起点为 `949a61b34971a890796c4d200762bf65cf7da2c3`。IDE 对标依据见[调研笔记](../research/2026-10-29-java-ide-onboarding.md)。

## 已确认的产品行为

自动发现以可信项目成员为边界。有有效项目声明时只纳入成员；没有受支持声明时按常见目录和 package 推断。显式 `files` 可以选择声明范围外的文件，仍保持精确选集。用户已确认此项。

用户已选择：没有项目目标 Java 版本声明时，保持目标未知。这里未知的是目标版本及标准 API 上下文，文件、模块归属和源码结构可以分别确认。分析器运行 JVM、Gradle daemon JVM、解析器语法级别都不能替代项目目标证据。

## 基础图降级（最后兜底）

目标未知时先采用下节的默认平台增强。没有可用的分析平台、绑定分析无法可靠进行或上下文必须弃用时，才走本节结构路径；目标未知本身不再直接意味着关闭所有语义分析。

1. 按选集规则确定文件，读取并保留源码原文。未知目标不会自动扩大、清空选集，也不会单独导致整次分析失败。
2. 用锁定版本的 JDT 做语法解析，提取能够确认的文件、类、方法、字段和包含关系；保持既有自身 `code` 与稳定身份规则。工具使用的语法级别只决定它能读懂什么，不表示项目以该版本为目标。无法可靠恢复的区域保留原文，不补造声明。
3. 缺少可用的分析平台时使用结构分析路径。当前锁定 JDT 的现代语法绑定分析需要系统库，不能用假 Java 8 目标绕过。已声明旧目标且缺匹配平台时，也不能借随包 Java 25 的完整 API 冒充该目标。语法分析关闭类型绑定；不能可靠证明目标的调用和类型关系使用既有 unresolved 表达，不输出确定目标边。
4. 在 stderr 说明目标版本或标准库上下文缺失及其影响；成功响应仍遵循 `codegraph.analyzer/1`，不为此扩公共 JSON。图中不声称已经获得完整调用关系。
5. 后续取得可信的项目编译设置和匹配平台库时，重新分析以增强关系。节点身份和原文不会因为补齐 SDK 而改变；关系可以从 unresolved 变为确定关系。

例如，未声明目标版本的文件包含：

```java
class Demo {
    void run() { helper(); java.util.List.of("a"); }
    void helper() {}
}
```

基础图保留 `Demo`、`run`、`helper`、包含关系、源码及自身 `code`。在语法分析路径中，即使 `helper()` 与声明在同一文件，也不靠名称相同就认定调用目标；两个调用均可记录为 unresolved。尤其不能因为工具运行在 Java 25 上就认定 `List.of` 属于这个项目可用的标准 API。

内部必须分别保存这些事实，不能合并成一个笼统的“项目未知”：

| 情况 | 已知信息 | 处理 |
| --- | --- | --- |
| 无目标声明 | 项目目标未知；选集和结构仍可知。 | 优先用户默认 JDK、再随包 JDK 作明确标记的分析平台；必要时基础图降级，不把所选平台称为项目目标。 |
| 声明 Java 17，但缺匹配平台库 | 目标版本已知，平台资源不可用。 | 保留 Java 17 声明与基础图；相关引用 unresolved，不改成目标未知。 |
| 只知 source/target 或 daemon JVM | 可能知道语法、字节码或工具运行要求，未必知道准确 API 平台。 | 分别记录证据；不把它们直接等同于 `--release`。 |
| 编译设置和匹配平台库均可信 | 可以建立对应目标的语义上下文。 | 按模块和源集增强，仍排除错误、歧义和不可靠的绑定。 |

纯结构降级可用于阅读、结构比较和变更定位；完整调用图需要更多上下文。语法损坏或超出锁定解析器能力时，也不能保证恢复所有声明。

## 已确认的默认分析平台

项目目标是否已确认，与本次分析是否可以选用默认平台，是两个不同决定。可以在保留“项目目标未知”的同时，明确选一个默认平台，解释为“在这些 API 条件下得到的绑定”，而不是声称发现了项目目标。用户本轮已确认 Q4：优先用户默认 JDK、不可用时再回退随包 JDK，输出可靠关系并记录默认平台语义。

项目明确声明优先；仅在未声明目标时选用有效的用户默认 JDK，不可用时再用随包平台。正常输出该上下文中可靠的绑定，其他关系保持 unresolved，结构路径作为最后降级。不得覆盖已明确的目标版本，也不根据哪个 JDK 能消除更多错误而不断换平台。有效性检查、可读库布局和候选优先级由实现验证。分析器仍运行在随包 JVM 上，选用用户 JDK 指使用其平台资料，不要求用旧 JVM 运行分析器。

高版本 JDK 对很多旧程序兼容，但新平台 API 与旧平台并不相同。`List.of` 从 Java 9 才有；新平台也会删除旧 API。因此直接用高版本库可以让原目标不支持的调用得到绑定，也可能让旧目标原本支持的引用消失。`javac --release` 可以为支持的旧版本限制语言、输出和 API，但这不是自动提供给 JDT ASTParser 的功能。[List.of 的版本](https://docs.oracle.com/en/java/javase/25/docs/api/java.base/java/util/List.html#of(E))、[删除的 API](https://docs.oracle.com/en/java/javase/25/migrate/removed-apis.html)、[javac release 契约](https://docs.oracle.com/en/java/javase/25/docs/specs/man/javac.html#standard-options)。

固定 Core 3.47.0 的本轮微探针镜像当前 `ASTParser.createASTs` 路径，接入 JDK 25 系统库，并设置 source/target/compliance 8、release enabled。它仍接受 `List.of` 和较新的 API，并将 `ByteBuffer.flip` 的新返回类型用于选择项目内部的 `select(ByteBuffer)`；`javac --release 8` 对同一合法重载例子则选择 `select(Buffer)`。因此 release 开关、无诊断和非 recovered binding 均不足以证明历史平台匹配。若将来适配高 JDK 的历史 API 签名，须另行验证，本轮不把该能力当作已具备。[固定 ASTParser 的路径装配](https://github.com/eclipse-jdt/eclipse.jdt.core/blob/R4_41/org.eclipse.jdt.core/dom/org/eclipse/jdt/core/dom/ASTParser.java#L236)、[探针证据](/Users/lizhanfa/.codex/cangyun-loop/github.com/XLCYun/CodeGraph/evidence/java-platform-fallback-479678dd/summary.json)。

Java 节点的既有 `extensions` 允许有命名空间的语义值，可以记录分析平台版本和默认来源而不新增公共 JSON 字段；目前尚未有这种语义键。记录不包含主机绝对路径、时间戳或私有发现清单；同一平台仅从 JAVA_HOME 换为 PATH 命中不应制造语义变化。现有扩展参与图差异比较，所以平台变化本身可能造成图差异；做源码差异时应固定分析上下文。现有多配置图合并也不自动提供逐边的配置来源，文件级平台记录不能冒充逐边归属证明。

| 模式 | 保留能力 | 对使用者的影响 |
| --- | --- | --- |
| 纯结构路径 | 原文、可确认声明、层级、自身 code 和稳定身份。 | 方法体变化仍可比较；调用、继承、重载等依赖绑定的关系会缺失，调用者树与影响传播可能中断。 |
| 默认平台增强 | 在选定标准 API 及已有依赖条件下，恢复可靠的调用和类型关系。 | 更接近 IDE 的开箱体验，但关系对平台选择有条件，不能据此证明原项目版本兼容。 |
| 可信项目上下文 | 按有效模型、编译设置、平台与依赖进行语义增强。 | 更贴近真实构建；仍不能把语法错误、缺生成代码、缺依赖或歧义绑定变成确定事实。 |

默认 JDK 只能补标准平台上下文，不能替代第三方依赖、代码生成结果或模块可见性。三个模式都不承诺得到运行时所有动态调用。

## 拟采用的项目模型实现

继续直接使用 JDT Core / `ASTParser` 构图，不把完整 JDT LS 或 Eclipse workspace 作为前置条件。模型输入适配为内部模块、源集上下文，再配置 `setEnvironment` / `setCompilerOptions`。每个上下文分别包含源码与测试根、依赖和二进制路径、可见性、编码、编译设置及目标平台证据，避免把全仓合成一条 classpath。

Maven 使用成熟的 ModelBuilder 取得 effective POM，配合 Artifact Resolver 与 Maven Resolver Provider 解析依赖。父 POM、BOM、profiles、settings、mirrors、proxies、本地仓库和 reactor 可见性需要实际集成；effective POM 不等于插件执行后的全部源码根和生成产物。不自行实现通用 POM 解释器，也不声称 ModelBuilder 执行了插件目标。[模型构建阶段](https://maven.apache.org/ref/3.9.11/maven-model-builder/)、[Resolver 职责](https://maven.apache.org/resolver/what-is-resolver.html)、[Maven Provider](https://maven.apache.org/ref/3.9.11/maven-resolver-provider/)。

Gradle 使用官方 Tooling API 请求模型。标准 Eclipse/IDEA 模型可提供许多基础信息，但项目级 Java 设置不足以证明每个源集实际的 classpath、`JavaCompile.options.release` 或 toolchain。必要时增加构建端 Tooling Model 适配，取得这些有效设置；客户端 BuildAction 不能直接当作活的 Gradle Project 使用。静态读取仅用于明确支持的子集或基础降级。[Tooling API](https://docs.gradle.org/current/userguide/tooling_api.html)、[项目级 Java 设置](https://github.com/gradle/gradle/blob/v9.8.2/platforms/ide/tooling-api/src/main/java/org/gradle/tooling/model/eclipse/EclipseJavaSourceSettings.java)、[源集范围属性](https://github.com/gradle/gradle/blob/v9.8.2/platforms/ide/ide/src/main/java/org/gradle/plugins/ide/eclipse/model/internal/SourceFoldersCreator.java#L116)。

M2E、Buildship 提供完整 Eclipse 项目集成，暂不因参考 IDE 就引入全部插件和 workspace 机制。具体组件版本、依赖闭包、许可证和支持的 Gradle/JVM 组合须经实际验证后锁定。

## 模型导入与调用方职责

本节结合分支讨论、本轮技术调查和用户对 Q5 的直接意见修订。调用方负责信任提示、授权确认与其运行环境的权限；CodeGraph 是基础开源部件，正常分析调用以调用方已经处理授权为前提，不增加第二层逐仓库信任记录或额外授权信号。完整交付范围仍待最后汇总确认；目前没有执行模型导入。

CodeGraph 文档明确说明正常模型导入会执行哪些逻辑、访问哪些资源。调用方在调用前决定是否允许，负责向最终用户呈现必要提示；分析器不重复确认，也不要求新环境变量或公共 JSON 授权字段。项目配置表达项目模型，不充当授权来源。

正常导入范围拟包括启动所需 Java/Gradle 进程、取得 Gradle 分发、评估 settings/build 配置和插件、读取约定的调用环境与本机设置、解析或下载插件及依赖、写入工具缓存/锁/日志，以及编译 `buildSrc` 等构建辅助逻辑。默认只请求模型和必要依赖，不主动请求业务 build/test/release/deploy；额外生成任务另定边界。不能把“未传业务任务”说成“不执行构建代码”：Gradle 请求模型也会评估配置并可能编译辅助逻辑。`forTasks` 的空数组会触发默认任务，应省略该请求，而不是用空数组表示不跑任务。[任务参数契约](https://github.com/gradle/gradle/blob/v9.8.2/platforms/ide/tooling-api/src/main/java/org/gradle/tooling/BuildActionExecuter.java#L82)、[buildSrc](https://docs.gradle.org/current/userguide/sharing_build_logic_between_subprojects.html#sec:using_buildsrc)。

本轮不增加自动安装项目目标 JDK 的能力。Gradle 标准 toolchain 自动下载应关闭；选择本机兼容的工具 JVM。此设置不能约束任意可信脚本自行执行的下载。工具 JVM 选择也不构成项目目标证明。[禁用 toolchain 自动供给](https://docs.gradle.org/current/userguide/toolchains.html#sub:disable_auto_provision)。

现有薄入口会清理环境，只转发少量变量。实现必须明确传入 Gradle home、代理和构建所需环境，保留分析器自身的 JVM 注入保护；不能宣称脚本继承了当前实际被过滤掉的环境。隔离用户目录也会改变 init 脚本、properties、认证和缓存等构建输入，需要说明并验证实际采用的设置。

临时源码副本用于减少对原快照的修改，不提供进程权限隔离。脚本仍可能访问副本以外的路径；副本位置、Git 信息、相对路径和 settings 的变化也可能影响模型。必须固定原文件字节及原路径到副本路径的映射，并检测导入期间的变动。不能将已被脚本改写的副本 AST 偏移拼接到原文件，也不能把副本路径带进公开身份。

导入进程及 daemon 必须由本次分析拥有和回收，避免接入共享 daemon 后误停其他任务。正常完成、失败、取消均须验证其工具树实际停止，再释放临时资源。`ProjectConnection.close` 等待操作完成，取消令牌和 connector disconnect 也不等于所有 daemon 已退出；不能靠 API 返回或全局 `--stop` 证明回收完成。[close 契约](https://github.com/gradle/gradle/blob/v9.8.2/platforms/ide/tooling-api/src/main/java/org/gradle/tooling/ProjectConnection.java#L195)、[disconnect 契约](https://github.com/gradle/gradle/blob/v9.8.2/platforms/ide/tooling-api/src/main/java/org/gradle/tooling/GradleConnector.java#L169)。

## 确认进度

用户已确认 Q2：项目配置损坏或模型导入失败时，按选集方式处理。显式 `files` 保留基础图并舍弃不可信上下文；目录发现只有在其他可靠来源足以确认成员时继续，否则明确失败。读取失败、路径逃逸、资源上限或真实内部错误仍失败。不能静默把未知成员变成空成功或全仓扫描。

Q1 项目成员优先、Q3 目标未知、Q4 默认平台增强、Q2 按选集区别处理失败均已直接确认。Q5 按用户意见收敛为调用方负责授权，基础部件不重复设置授权门槛；不再追问同一个开关。剩余步骤是汇总完整交付范围与验收，确认双方理解一致。

## 拟验收的结果

- 无声明时采用用户默认/随包平台并标记来源，可靠绑定正常输出；已声明但缺匹配 SDK、缺依赖或无可用平台时保留基础图，stderr 原因准确，不把工具平台冒充项目目标。
- 平台扩展的语义稳定、有命名空间，无时间戳、临时路径或主机路径；记录未知目标与有效分析环境的区别，多配置不能被压成单一环境。
- Maven 多模块及 Gradle 多源集用有效模型确定成员、源码根和可见性，避免相同包名、重复类型或不同目标平台串联。插件产物缺失时不虚构生成声明。
- `files` 省略、空列表和精确选集保持原契约；图外上下文和生成文件不扩大完整节点选集；坏配置按确认后的政策处理。
- 目标未知示例中的声明和原文可用；无平台绑定时不靠名称匹配输出确定调用边。有平台时仍验证错误参数数量、歧义和恢复绑定等反例。
- 补齐可信目标平台只增强关系，文件原文、自身 `code`、相对路径身份和稳定 ID 保持；临时路径不会出现在图中。
- 调用方授权分工与模型导入动作在文档中明确，不引入第二层信任开关；真实覆盖配置、依赖与辅助逻辑过程。对正常、失败和取消分别检查所拥有的工具与 daemon 已退出，并核验源快照一致性。
- 新增依赖完整锁定、校验并携带许可证；以打包后的公开入口和双平台必要 CI 验证，不把 IDE 文档或独立探针当作产品验收通过。

用户已确认以上交付范围，Issue #10 进入实现；Narrant 继续暂停。

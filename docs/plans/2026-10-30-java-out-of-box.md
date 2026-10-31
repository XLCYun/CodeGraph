# Java 开箱即用实现计划

Spec: [ADR 0002](../adr/0002-java-project-model-import.md)。完整已批准 scope 与人类证据保存在共享 cangyun-loop 目录的 scope-proposal-479678dd-round5.json、scope-approval-479678dd-round5.json；2026-10-30 已登记 scope-confirm，run 为 3b64f700a9344010be9e7f6882406fde。

## Global Constraints

- 继续固定 JDT、codegraph.analyzer/1、一次输入输出。files 省略发现、空集跳过配置和 SDK、非空精确选集；完整节点仅来自选集，图外源码只作上下文。
- 项目成员优先；无受支持声明时推断常见布局/package。显式选集允许声明外文件。include_tests 有效。
- 明确项目目标优先匹配平台；无目标时先用户默认 JDK、再随包平台，目标仍未知。记录稳定的 Java 平台扩展，可靠关系正常输出；真正缺平台时保结构与原文。不得用高版本 API 冒充已声明旧目标。
- 坏模型：明确选集舍弃失败上下文并保基础图；目录发现缺可靠成员来源时失败。IO、路径逃逸、资源限制、内部故障不能伪装为正常降级。
- 调用方负责授权，正常分析含模型导入许可，不另加信任开关。允许配置/插件求值、辅助逻辑编译、分发/插件/依赖下载和缓存；不主动请求业务 build/test/release/deploy 或额外生成任务，不自动安装目标 JDK。
- 源快照、原文、自身 code、词法稳定身份、外部/unresolved 规则保持。临时副本不是沙箱，须检测变化并保留原路径映射。
- 所拥有的工具、daemon、管道在正常/失败/取消时完整回收；调用方负责取消整树。不得添加监督worker、全局停止用户daemon或把connector.close当退出证明。
- 依赖完整锁定、摘要校验与许可证；macOS ARM64和Windows x64真实公开包验收、完整最新CI、PR人工审核。Narrant暂停。

## Task 1: 源码发现、默认平台与可靠降级

实现核心源码路径，所有Java核心与现有Python/Java组件测试可修改；暂不引入Maven/Gradle新依赖，后续任务接入成熟模型。主要文件 ProjectModel.java、SdkDiscovery.java、Analyzer.java、GraphChecks.java 与相应测试。

删除缺元数据/无release/缺匹配SDK阻断明确选集的行为。保留空集快速返回。无声明时识别常见src/main/java、src/test/java、多独立布局及按package推导的源码根；有效现有声明仍限制自动发现成员，声明外显式文件建立独立上下文。不要把排除的build/.git/vendor等噪声无界扫入。

支持unknown target和available/missing platform的区别。用户默认候选按显式提示及JAVA_HOME/JDK_HOME/PATH优先，常见安装目录最高版本不能冒充默认。保留候选来源与同major精确选择，提供稳定的java.*平台扩展；不泄露绝对路径或时间戳。同平台仅JAVA_HOME/PATH来源变化不应制造语义差异。分析器仍跑随包JVM，目标SDK只读。

有可用平台时继续绑定；没有可用平台用语法路径，保留原文及能确认声明，不借假Java8运行现代解析。语法恢复只交付可确认结构，不因局部损坏丢整个源文件；JPMS至少保留文件和可确认结构，未支持的模块语义不伪造。不能仅凭non-recovered/key判断可靠绑定：覆盖错误参数和歧义调用，受错误影响的关系unresolved。

把显式坏配置降级与目录发现失败分开，真实IO/路径/资源/内部失败保持。目标仅对活动上下文匹配，不让未选模块缺SDK阻止选集。源码和AST结构相同时补齐平台不改变ID/原文/self code。

新增真实公开入口用例：无配置同文件helper调用默认绑定、无SDK纯结构、旧目标缺SDK、坏配置精确选集/发现、声明外文件、测试过滤、空集、模块隔离、歧义/错参、默认平台来源、无主机路径。调整与已批准行为冲突的旧断言，不能删除可靠语义验收。用现有固定工具资源运行针对性Java测试，报告覆盖命令和结果。

## Task 2: Maven 成熟模型与依赖

在Task1模型边界接入Maven ModelBuilder、Artifact Resolver和Provider，支持effective POM、父/BOM、属性、profiles、用户settings、mirrors/proxies/local repo、reactor与编译/测试classpath及模块可见性；不手写通用POM解释器。有效POM不是插件执行模型，不主动业务生命周期。锁定完整依赖闭包、下载来源、摘要与许可证。加入多模块/属性/依赖/缓存及坏模型真实测试。必要源快照及输入不变校验在模型导入之前建立，输出路径映射回原仓库。

## Task 3: Gradle 模型、环境与工具生命周期

用官方Tooling API支持Gradle常见Java项目，按source set导出源码根、角色、实际依赖/项目源码可见性、release/toolchain/encoding。不要将Eclipse项目级模型或daemon JVM误当逐源集目标。远端模型代码须兼容实际导入JVM，不能直接发送Java25字节码到旧JVM。必要模型适配可采用专用build-side模型。

使用已有兼容工具JVM，禁止标准JDK自动供给。模型请求省略forTasks（空数组会执行默认任务）。处理wrapper/分发、buildSrc、环境和用户设置。导入使用受管理副本和拥有型daemon/进程；退出与取消靠实际进程核查，不能只调用close或全局--stop。调整launcher环境转发但保持分析器自身JVM注入防护。正常/失败/取消用真实工具验证，业务任务哨兵不得触发，源快照变化被识别。

## Task 4: 整合、公开包验收与文档

串联全部模型与源码路径，补齐接口与错误降级的跨模块用例。更新语言README、engine-selection、protocol相关行为及ADR为最终实现。构建真实Java包并运行公开CLI，覆盖Maven/Gradle、默认SDK、空/精确/发现、原文/self code/ID、图外/unresolved与进程树回收。运行相关脚本测试、检查依赖打包许可证。完成macOS实际包验收，提交分支PR触发Windows和完整双平台CI；对CI真实失败持续修正。不将仅组件测试视作完整验收。

## Task 5: 全分支审查与交付

以基线949a61b34971a890796c4d200762bf65cf7da2c3对完整变更独立审查标准与scope，修正重要问题并做定向复验。保证最新PR head完整CI通过，附上真实验证与限制，等待人工审核合并。不得自行merge或把待审核记成已完成。

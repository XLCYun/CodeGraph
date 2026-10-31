# Java 语言分析器

该分析器使用 `dependencies.lock.json` 锁定的 Eclipse JDT 解析源码。分析器自身的运行 JDK、JDT 工具版本与被分析项目的目标 Java release 相互独立。Maven 项目通过固定 Maven 3.9.11 ModelBuilder 与 Resolver 1.9.24 导入有效模型和依赖，允许下载及缓存父 POM、BOM 和依赖。导入不启动 Maven 命令，不执行业务生命周期、插件目标或注解处理器。Gradle 项目通过官方 Tooling API 导入，规则见下文。

公开入口为 `bin/codegraph-java analyze`，由 Java 主线程直接实现 `codegraph.analyzer/1`，
固定版本 `0.1.2`。stdin 接收一个 UTF-8 JSON 对象后 EOF，尾换行可选；stdout 只有一个
带固定身份的最终结果。成功退出 0，可处理失败退出 1，用法错误退出 2。
`--help`、`analyze --help` 和 `--version` 输出文本后退出。详见 [公共协议](../../docs/protocol.md)。

薄入口只定位 `runtime/jdk/bin/java`、`runtime/analyzer/classes` 和 `runtime/analyzer/lib`，
固定参数与必要环境、继承标准句柄并传播退出；macOS exec，Windows spawn/wait。
程序没有 ready、cancel、监听线程或分析 worker。正常 EOF 表示完整输入；取消、期限与
整棵进程树终止由调用方负责。请求和结果限制为 128 MiB，选集最多 100,000 项；
显式空集先检查源码边界，随后直接返回空图，不发现配置或目标 JDK。

正常分析以调用方已经完成授权为前提，不增加仓库信任开关或授权字段。Maven 解析依赖，Gradle 配置、插件和 buildSrc 可执行代码、访问网络并写入缓存；不主动请求业务 build/test/release/deploy 或额外生成任务，不自动安装项目 JDK。临时副本不是沙箱，也不能证明与原 checkout 的任意动态行为完全等价。

成功分析的降级原因只写 stderr，stdout 仍是唯一 JSON。固定诊断代码为 `default_platform`、`platform_unavailable`、`target_platform_missing`、`compiler_unavailable`、`context_incomplete`、`model_discarded`，每种最多一次，不包含私有路径、URL、凭据或原始构建日志。完整且明确声明目标的导入不输出诊断。

## release 与目标 JDK

`java-static-v1` 接受规范整数 release，例如 `8`、`17`、`25`。支持范围直接由固定 JDT 的公开 API `JavaCore.isJavaSourceVersionSupportedByCompiler` 判定；`8` 在调用 JDT 时映射为其历史名称 `1.8`。不维护产品版本白名单，也不把未知版本降级到最高已知版本。`1.8`、带空格、前导零、属性表达式和预览版本后缀均不是合法的配置 release。

每份配置单独匹配本语言发现器提供的同 major 目标 JDK：

- major 必须与配置 release 完全相同，同一 major 使用发现到的最高稳定补丁。
- `version` 必须等于该 JDK `release` 文件中的真实 `JAVA_VERSION`。经典的 `1.8.0_XXX` 对应 release `8`；现代版本从稳定版本号读取 major。EA 版本不可用。
- Java 8 使用目标 JDK `jre/lib` 中的 `rt.jar` 及存在的 `resources.jar`、`jsse.jar`、`jce.jar`、`charsets.jar`。必须有 `rt.jar`；不扫描 `ext`，不自动加载其他 JAR。其他厂商的非经典目录布局尚不支持。
- Java 9 及以后使用目标 JDK 的 `lib/jrt-fs.jar` 与 `lib/modules`，不加入运行分析器的 JVM 标准库。

Java 25 运行时不能代替已声明的 Java 17 目标。缺少匹配平台时保留可提取的结构、原文和稳定身份，无法确认的关系保持 unresolved。无目标声明时先用用户默认 JDK，再用随包平台；目标仍记录为 unknown。预览始终禁用。不支持的声明属于坏模型：明确选集保基础图，目录发现则失败。

`SdkDiscovery` 直接读取既有 `CODEGRAPH_JAVA_TARGET_JDK`、`JAVA_HOME`、`JDK_HOME` 提示，
常见安装目录、用户 SDK 目录、PATH 中真实 java 路径及随包 JDK。只读 release 与标准库布局，
不执行探测命令，不安装 JDK。EA/preview 候选不参与选择；请求不能注入 SDK、环境或 catalog。

## 多配置

standalone 声明可为不同源码根指定不同 release：

```json
{
  "schema": "java-static-v1",
  "configurations": [
    {"id": "legacy", "release": "8", "source_roots": [{"path": "legacy/src", "role": "main"}]},
    {"id": "service", "release": "17", "source_roots": [{"path": "service/src", "role": "main"}]},
    {"id": "current", "release": "25", "source_roots": [{"path": "current/src", "role": "main"}]}
  ]
}
```

Maven 模块通过有效 POM 获得继承、属性、profiles 和编译器配置。每份配置使用独立的 JDT 语义环境与 binding 索引，来源证据列出各自的平台 ID。同一物理源码可属于多份配置，最终图仍按原有身份与关系并集规则合并；某配置中的未知关系不会被另一配置的确定关系抹去。

Maven 的 main/test 各自形成独立 source set。测试可以看到本模块生产源码；跨模块可见性由 Resolver 的依赖图决定。reactor 模块不要求先编译或安装，生产源码不会看见测试源码和 test 依赖。release、source、target、toolchain 与 encoding 分别保留。API 平台只由 release 或 toolchain 选择；仅有 source/target 时使用默认平台，目标仍为 unknown，字节码 target 不作为 API 限制。只有 release 开启 JDT 的 release 限制。节点的 `java.platforms` 记录 `api_source`、独立的 `compiler` 字段和 `dependencies_complete`。缺失依赖会标记模型不完整，Resolver 已确认的健康依赖分支仍保留。reactor 先用 Maven 两阶段构建的第一阶段取得有效坐标，再导入 BOM，属性版本不受模块声明顺序影响。

用户 `~/.m2/settings.xml`、`MAVEN_HOME/conf/settings.xml`、mirrors、活动 proxies、认证、离线模式和本地仓库参与导入。密码按标准 `~/.m2/settings-security.xml` 解密；`settings.security` 覆盖及本地 relocation 链在读取前逐项监视，远程安全文件位置明确拒绝，诊断不输出凭据。网络连接和读取设有有界超时；请求期限及调用方取消仍适用。有效 POM 不等于插件执行模型，动态插件生成的源码根不在本阶段的保证范围内。

分析前建立临时输入副本，冻结源码及构建配置，导入和 JDT 均在副本上工作。导入后、解析前后核验原输入与副本；原文件或副本被修改时失败，不把不同版本的 AST 偏移和原文拼接起来。副本不是沙箱。.git 的目录存在性保留，内部文件默认不复制；`.gradle`、`.idea`、`.tools`、`vendor` 和 `node_modules` 只复制源码及构建声明，避免复制缓存二进制。项目声明的源码成员及明确选中的文件仍然优先。空目录与文件 profile 的存在/缺失条件也会冻结、复核；实际引用的缓存内 marker 会补入副本。工具输出置于独立目录，不改变模型读取的输入目录。每文件上限 32 MiB、输入总量 512 MiB、文件数 100,000。正常及可处理失败都会关闭 Resolver 并删除副本。显式坏模型回退先复核输入，丢弃失败配置，只用明确选集重建上下文。

## Gradle 模型导入

Gradle 项目使用锁定的 Tooling API 8.14.3 和独立编译为 Java 8 字节码的模型桥。导入读取 settings 确定的项目、Java source set、JavaCompile 配置及其实际 classpath；不解析脚本文字猜测动态成员。Groovy、Kotlin DSL 和必要的 buildSrc 编译均可参与配置求值。导入关闭 configuration cache、configure-on-demand 和 isolated projects，确保整个 settings 项目树完成配置。请求不设置 `forTasks`，不会主动运行项目的 build、test、发布或额外代码生成任务。配置和插件本身仍可产生副作用；输入副本不是沙箱。

有 wrapper 时使用其分发 URL、版本及可选 SHA-256，不擅自改成默认版本；无 wrapper 时固定使用 Gradle 9.1.0 并校验官方摘要。支持尝试稳定的 Gradle 7.6+、8.x、9.x，预发布及 10+ 暂不支持。工具 JVM 按官方运行兼容表选择已有候选，并检查本机系统、架构和可执行文件。有效用户默认优先，随后是其他已有 JDK，最后才是兼容的随包 JDK。工具 JVM 不成为项目的 API 目标，不自动安装 JDK。分发每次在独立输出目录验证、展开；当前没有持久分发缓存，首次或重复导入均可能下载。

| Gradle | 可运行的 JDK major |
| --- | --- |
| 7.6–8.2 | 8–19 |
| 8.3–8.4 / 8.5–8.7 | 8–20 / 8–21 |
| 8.8–8.9 / 8.10–8.13 / 8.14+ | 8–22 / 8–23 / 8–24 |
| 9.0 / 9.1–9.3 / 9.4–9.7 / 9.8+ | 17–24 / 17–25 / 17–26 / 17–27 |

该范围依据 [Gradle 运行兼容表](https://docs.gradle.org/current/userguide/compatibility.html)。代表版本 7.6.4、8.14.3、9.1.0、9.8.2 使用真实分发测试；这不表示逐个测试了所有中间版本。自定义模型或内部属性能力不匹配时保守降级。公开包验收在 macOS ARM64 与 Windows x64 的完整 CI 中执行，不能用组件测试替代。

每个 source set 保留 release、source、target、toolchain 和 encoding。显式 JavaCompile.javaCompiler 覆盖项目 toolchain 时使用其有效 metadata；覆盖失败或无法确认的 compiler convention 会保留成员和 release 证据，但停用该配置的 API 上下文，只输出结构与原文。未声明目标且仍使用普通 Java 插件默认 convention 时，继续按用户默认平台规则处理。反射适配仅用于识别 Gradle property 的显式状态，能力缺失不能当作“未声明”。

源码可见性来自实际 JavaCompile.classpath 的 source-set output 或经过检查的普通 main JAR。过滤、重命名、额外内容或无法确认的 archive 不映射成完整项目源码；不存在的二进制保持 unresolved，并标记依赖不完整。source-set 的 include/exclude 使用 Gradle 返回的实际 Java 文件成员，不自行解释 glob。完整选集和 JDT 批量上下文都仅包含获准成员，关闭这些配置的隐式 sourcepath 搜索；明确选择过滤外文件时只分析该选集。另行替换 JavaCompile.source 且与 source set 不一致的配置仍按坏模型处理。普通测试源集按 Test 任务的 classesDirs 关联及以 test 结尾的惯用名称识别，`include_tests` 继续生效。外部二进制复制到拥有的目录，并核对原件、复制件与目录成员。配置求值生成的 Java 只作为冻结的上下文，不能扩大原仓库选集，也不能产生临时路径的公共节点。

每次导入使用独立 Gradle user home。用户 gradle.properties 保留有效值，用户 init.gradle、init.gradle.kts 和 init.d 脚本使用原路径，以保留脚本相对资源语义；副本路径和 GRADLE_USER_HOME 字符串仍会变化，不能保证任意路径依赖的自定义脚本完全等价。launcher 转发构建环境，但清除 JAVA_TOOL_OPTIONS、JDK_JAVA_OPTIONS、_JAVA_OPTIONS、CLASSPATH 等分析器注入变量。固定 JVM 参数启用 Gradle 本地库访问。

分发下载支持用户/项目 Gradle properties 中的 HTTP/HTTPS proxyHost、proxyPort、proxyUser、proxyPassword，二者共用 http.nonProxyHosts；也接受基础 HTTP_PROXY/HTTPS_PROXY/NO_PROXY 提示。wrapper 认证支持通用或主机范围的 Basic 用户名/密码、URL 用户信息以及主机范围 Bearer token；properties 优先于 URL 用户信息。重定向不把原站凭据发给不同源，也不允许 HTTPS 降为 HTTP。SOCKS、NTLM 等特殊认证未声称支持。诊断不输出 URL、凭据或原始构建日志。下载最多 512 MiB，展开最多 1 GiB / 100,000 项；捕获的配置输出最多 1 MiB。二进制上下文每文件最多 128 MiB，与源码合计受 512 MiB 限制。

当前 Tooling API 路径可能忽略 daemon JVM criteria，因此导入前按 criteria 的 major 选择已有兼容 JDK；vendor 只接受与 release 文件 IMPLEMENTOR 一致的证据及已验证的 ADOPTIUM 别名，J9 需要 Openj9 元数据。不认识或无法满足的条件不会被忽略。Java 安装自动发现和自动供给均关闭，仅提供已发现路径；测试用 HTTP 哨兵确认缺失目标不会触发 JDK 下载。

正常、失败和内部超时都会记录本次工具的 PID 与启动身份，回收并确认退出后才删除快照。`connection.close()` 不作为 daemon 已退出的证据。另用锁定 Tooling API 的客户端服务关闭方法结束本进程的非 daemon executor；这是一次性分析器内的兼容适配，不会全局停止用户 daemon。外部取消仍由既有调用方进程树所有者负责。无法确认工具退出时保留临时目录并报告 cleanup failure。

## 编译与验证

以下命令仅编译分析器自身和不随包交付的组件测试驱动。运行 JDK 必须已在本地准备完成。依赖下载器按锁内 URL 和 SHA-256 重建完整闭包，可指定独立目录：

```sh
python3 analyzers/java/tools/fetch-deps.py "$CODEGRAPH_JAVA_LIB"
python3 analyzers/java/build.py "$CODEGRAPH_JAVA_RUNTIME" "$CODEGRAPH_JAVA_LIB" --tests
python3 -m unittest discover -s analyzers/java/tests -v
```

测试默认使用仓库 `.tools/java-native` 中既有的固定运行 JDK 25、JDK 17 与锁定依赖。可用以下环境变量覆盖真实夹具：

| 变量 | 用途 |
| --- | --- |
| `CODEGRAPH_JAVA_RUNTIME` | 运行与编译分析器的固定工具 JDK |
| `CODEGRAPH_JAVA_LIB` | 与锁文件一致的 分析器 JAR 目录 |
| `CODEGRAPH_JAVA_TARGET` | Java 17 目标 JDK，兼容原测试入口 |
| `CODEGRAPH_JAVA_TARGET_8` | Java 8 目标 JDK；只读取标准库，允许与运行机器架构不同 |
| `CODEGRAPH_JAVA_TARGET_25` | Java 25 目标 JDK，默认使用固定运行 JDK 的真实 Java 25 标准库 |

Java 8 夹具未配置且未在 `.tools` 中找到时，对应测试会明确跳过；完整跨版本验收应提供三个真实目标。`test_target_versions.py` 覆盖经典标准 JAR、8／17／25 的 API 与语法隔离、预览禁用、多配置与 Maven 模块、共享源码并集、缺失／错配／重复平台、非法或超出编译器能力的 release，以及扩展 JAR 隔离。所有项目均为临时夹具，并验证关键分析前后的文件哈希不变。

当前能力来源可直接用锁定 JAR 的 `JavaCore.getAllJavaSourceVersionsSupportedByCompiler()` 查询；编译器错误分类使用同一 JDT 的 `IProblem.Syntax`、`Compliance`、`PreviewRelated` 常量，不依赖错误消息文本。

## 目录发现

公共 `files` 省略时，由本语言配置加载器确定完整选集；`[]` 是明确空选集，
`null` 无效。显式数组保持精确选择。具体成员、排除规则、测试角色与限制见
[引擎选集政策](../../docs/engine-selection.md)。新增目录发现测试由现有原生验收命令运行，
来源和图不变量由 Java 自身的 `GraphChecks` 在自然输出边界检查；内部证据不随公共结果输出。


`build.py --tests` 把历史配置、来源、只读、图与目标 SDK 用例的驱动编译到
`build/test-classes`；正式包只复制 `build/classes`。驱动没有生产兼容入口。
公共协议、EOF、调用方终止、背压与管道错误使用真实 JVM 的公开 Main 验证。
七语言已使用直接入口和统一[分发接口](../../docs/distribution.md)打包，布局见[包说明](../../docs/pack-layout.md)。


Maven 与 Gradle Tooling API（含其 shaded 组件）的精确版本许可证、版权及来源分别在 `licenses/maven/manifest.json` 和 `licenses/gradle/manifest.json` 中列明，并随语言包交付。打包前后逐项验证 `evidence`、`fullLicenseTexts` 与适用的 `pomEvidence` 引用和 SHA-256；解包测试再次验证完整清单。打包只复制锁文件中的 JAR，不带缓存目录里的其他版本或辅助插件依赖。

Gradle 集成测试从 `.tools/java-native/gradle-<version>-bin.zip` 读取本地真实分发，可用 `CODEGRAPH_GRADLE_9_ZIP`（9.1.0）、`CODEGRAPH_GRADLE_8_ZIP`（8.14.3）、`CODEGRAPH_GRADLE_7_ZIP`（7.6.4）及 `CODEGRAPH_GRADLE_CURRENT_ZIP`（9.8.2）覆盖路径。单独运行缺少对应分发时相关测试明确跳过；正式验收先验证四个归档及摘要，缺少任何一个即失败。工具链准备脚本按 `resources/gradle-test-distributions.json` 下载固定夹具。

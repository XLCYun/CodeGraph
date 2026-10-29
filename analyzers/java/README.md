# Java 语言分析器

该分析器使用 `dependencies.lock.json` 锁定的 Eclipse JDT 解析源码。分析器自身的运行 JDK、JDT 工具版本与被分析项目的目标 Java release 相互独立。Maven 项目通过固定 Maven 3.9.11 ModelBuilder 与 Resolver 1.9.24 导入有效模型和依赖，允许下载及缓存父 POM、BOM 和依赖。导入不启动 Maven 命令，不执行业务生命周期、插件目标或注解处理器。Gradle 导入另行接入。

公开入口为 `bin/codegraph-java analyze`，由 Java 主线程直接实现 `codegraph.analyzer/1`，
固定版本 `0.1.2`。stdin 接收一个 UTF-8 JSON 对象后 EOF，尾换行可选；stdout 只有一个
带固定身份的最终结果。成功退出 0，可处理失败退出 1，用法错误退出 2。
`--help`、`analyze --help` 和 `--version` 输出文本后退出。详见 [公共协议](../../docs/protocol.md)。

薄入口只定位 `runtime/jdk/bin/java`、`runtime/analyzer/classes` 和 `runtime/analyzer/lib`，
固定参数与必要环境、继承标准句柄并传播退出；macOS exec，Windows spawn/wait。
程序没有 ready、cancel、监听线程或分析 worker。正常 EOF 表示完整输入；取消、期限与
整棵进程树终止由调用方负责。请求和结果限制为 128 MiB，选集最多 100,000 项；
显式空集先检查源码边界，随后直接返回空图，不发现配置或目标 JDK。

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
2026-10-22 的 PR CI 39777180923 中 macOS 成功，Windows 因 Python junction 诊断断言失败；
完整 Windows 门禁仍须修复后的自动 CI 验证，首个固定发布尚未完成。

Maven 依赖的精确版本许可证、版权及来源在 `licenses/maven/manifest.json` 中列明，并随语言包交付。打包前后逐项验证 `evidence` 与 `fullLicenseTexts` 两组引用和 SHA-256；解包测试再次验证完整清单。打包只复制锁文件中的 JAR，不带缓存目录里的其他版本或辅助插件依赖。

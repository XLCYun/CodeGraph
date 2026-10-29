# Task 1 实现报告

完成源码发现、默认平台及结构降级。没有修改 launchers、依赖锁、打包脚本或已提交的设计文档，也没有创建子 agent。

## 实现

- `ProjectModel` 将源码归属与 SDK 匹配分开，只为活动配置选择 SDK；明确空集在读取配置与发现 SDK 前返回。
- 无受支持声明时，用 JDT 读取真实 package 声明推导源码根；常见 main/test 布局组成同模块上下文。多独立布局保持隔离，扫描跳过 `.git/.gradle/.idea/.tools/build/target/out/vendor/node_modules`。独立显式文件不要求存储路径与 package 对齐。
- 有效声明仍限制自动发现范围；声明外的明确文件建立独立上下文。可省略 metadata release，仍保留声明根。明确坏模型退回推断上下文，目录发现保留错误。新增内部 BoundaryFailure，路径越界/目录链接和 I/O 不进入模型降级分支。
- 平台候选保留来源，未知目标按 CODEGRAPH_JAVA_TARGET_JDK、JAVA_HOME、JDK_HOME、PATH、随包平台顺序选取；常见安装目录仅供已声明 major 匹配，不冒充默认。已声明目标只匹配同 major，缺失时保结构，不借用高版本 API。
- 每个节点携带稳定、排序去重的 `java.platforms` 数组：`target` 为声明 major 或 `unknown`，`status` 为 `available/missing`，可用时附真实 `version`。不含绝对路径、时间戳或环境变量名。GraphChecks 对模型与输出进行独立一致性校验。
- 无平台使用每文件 `createAST`、显式最新/声明语言级别、无环境无绑定的语法路径。有平台继续 JDT 绑定。JPMS descriptor 保留文件，不构造尚未支持的模块语义。
- 局部语法错误不再导致整文件丢失；恢复出来的声明节点不作为确定声明输出，重复声明仍拒绝。调用处错误诊断会压低绑定可信度：错参和歧义调用为 unresolved，正常调用继续解析；语法/语言级别错误所在声明的关系保守降级。
- 每次解析使用独立 parsed 集合；生产解析不引入测试根，测试解析可以看同模块生产根。

## 验证资源与命令

固定运行时与 JAR 沿用 `/private/tmp/codegraph-java-capability-probe`。真实 JDK 17.0.20.1+1 和 JDK 8u504-b01 从 `resources/native-toolchains.json` 的固定 URL 下载，先按清单 SHA-256 校验，再解包到该临时目录。没有用 JDK 25 冒充旧目标，也没有加入产品安装 SDK 的行为。

```sh
python3 analyzers/java/build.py \
  /private/tmp/codegraph-java-capability-probe/jdk-25.0.4.1+1/Contents/Home \
  /private/tmp/codegraph-java-capability-probe/lib --tests

CODEGRAPH_JAVA_RUNTIME=/private/tmp/codegraph-java-capability-probe/jdk-25.0.4.1+1/Contents/Home \
CODEGRAPH_JAVA_TARGET=/private/tmp/codegraph-java-capability-probe/jdk-17.0.20.1+1/Contents/Home \
CODEGRAPH_JAVA_TARGET_8=/private/tmp/codegraph-java-capability-probe/jdk8u504-b01/Contents/Home \
CODEGRAPH_JAVA_LIB=/private/tmp/codegraph-java-capability-probe/lib \
python3 -m unittest discover -s analyzers/java/tests -v
```

结果：74 项通过，无跳过；日志 `/tmp/task1-tests4.log`。覆盖已有可靠绑定、旧目标 API 隔离、8 的经典 JAR、跨配置 union、图边界反例、公开单次输入输出及生命周期。

之后补充了“独立文件 package 不必符合存储路径”用例，并针对最终源码重新编译、运行 `test_out_of_box.py`：14 项通过；日志 `/tmp/task1-final-targeted2.log`。此前 74 项全量运行包含其中前 13 项。旧断言仅调整与批准后的结构恢复/坏模型明确选集行为冲突的部分，未删除真实 API/可靠绑定验收。

新用例包括无配置同文件 helper、JAVA_HOME 与 PATH 同平台输出相同、无平台 record 的 ID/source/code 不变、旧目标缺 SDK、坏配置明确选集与发现差异、空集、声明外文件、独立布局与噪声排除、test 过滤、未选模块缺 SDK、生产不可见测试、测试可见生产、歧义/错参、局部损坏、JPMS 和主机路径不泄漏。真正无任何平台通过同核心组件 provider 提供空目录验证；真实公共入口的缺平台路径通过声明缺失 major 验证，因为随包运行时本身就是可发现平台。

另用固定 JDT 实测 `ASTParser.setResolveBindings(false)` + `createASTs`，显式 Java 25 选项、空环境、record 源码：实际正常回调并输出 record AST，不能声称批量 API 必然失败。探针 `/tmp/BatchSyntaxProbe.java`。最终实现选择无环境逐文件 createAST；该实际路径也由无 SDK 结构对比测试覆盖。

## 自查与后续接点

- `git diff --check` 通过。自查修复了跨配置 parsed 污染、main/test 反向可见性、根文件混入嵌套项目、无 release 声明丢根、路径边界错误误降级等问题。
- `ParseConfiguration` 仍是后续 Maven/Gradle source-set 接入边界；本任务保留原静态 POM 实现，尚未引入成熟 Maven/Gradle 模型、依赖类路径、构建副本或工具进程生命周期。
- 当前平台扩展侧重目标/可用性/版本；更丰富的模型上下文来源可在后续接入时扩展，候选来源已保存在 SdkDiscovery.Candidate 中。
- JPMS 的模块关系未实现；结构恢复不承诺推断损坏源码中无法确认的声明。独立源码根间没有猜测依赖关系。
- 尚未运行 Windows x64 公开打包验收或全仓最新 CI，这些属于后续集成验收。本任务已完成本机真实 Java 核心/公开入口验证。

## Review fix round 1（FIX_BASE d69d2be）

修正评审指出的异常分类错误：`ProjectModel.readRoots` 对 `source_roots` 中非对象元素抛普通 `Failure("unsupported_configuration", ...)`，而不再抛 `BoundaryFailure`。该处仅代表坏模型值，不是路径或 I/O 边界。真实越界、外向链接和不可访问目录继续沿原边界错误处理。

变更文件：

- `analyzers/java/src/dev/codegraph/java/ProjectModel.java`：一处异常类型修正。
- `analyzers/java/tests/test_out_of_box.py`：新增有效 JSON 的坏根元素回归，覆盖 null、字符串、数字、数组；每种值均经真实公共入口验证明确 `A.java` 选集成功、目录发现失败且无 graph。公共协议将内部 unsupported_configuration 映射为 analysis_failed，测试遵守现有映射。
- 本报告：追加修复与验证记录。

执行命令：

```sh
python3 analyzers/java/build.py /private/tmp/codegraph-java-capability-probe/jdk-25.0.4.1+1/Contents/Home /private/tmp/codegraph-java-capability-probe/lib --tests

CODEGRAPH_JAVA_RUNTIME=/private/tmp/codegraph-java-capability-probe/jdk-25.0.4.1+1/Contents/Home \
CODEGRAPH_JAVA_TARGET=/private/tmp/codegraph-java-capability-probe/jdk-17.0.20.1+1/Contents/Home \
CODEGRAPH_JAVA_LIB=/private/tmp/codegraph-java-capability-probe/lib \
PYTHONPATH=analyzers/java/tests \
python3 -m unittest -v test_out_of_box \
  test_discovery.DiscoveryTests.test_source_root_cannot_escape_repository \
  test_discovery.DiscoveryTests.test_inaccessible_source_root_ancestor_is_not_empty \
  test_helper.JavaHelperTests.test_external_metadata_symlink_rejected \
  test_public.PublicConformanceTests.test_physical_alias_and_source_overlap

git diff --check
```

构建输出：`/Users/lizhanfa/.codex/worktrees/codegraph-java-issue10/CodeGraph/analyzers/java/build/classes`。针对性测试日志：`/tmp/task1-fix-round1.log`。没有无理由重跑整个测试集。

自查：检查了读取根列表的所有错误分支及 discover 的降级判定。只有非对象根元素这一格式错误更换异常类型；绝对路径、相对路径逃逸、外向 metadata/source 链接、不可访问目录没有放宽。没有修改构造器/方法引用逻辑或扩大后续任务范围。

最终测试输出：

```text
Ran 19 tests in 13.791s

OK
```

`git diff --check` 无输出，退出码 0。

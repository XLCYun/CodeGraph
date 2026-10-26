# Python 语言分析器

独立 Rust 库固定 ty `0.0.84` 对应的 Ruff revision
`11aaa39f84c0c44bdfb4c9cc21e4038c6f74c00a`。生产依赖的数据库、AST 和 codegen
全部同源；退役前 registry Ruff `0.0.15` 的实测投影保留为固定业务快照，不再引入旧 parser。

`PythonProject::load(ProjectInput)` 接收单个已归一化项目：`repository` 是磁盘仓库根，
`project_root` 和有序 `source_roots` 是干净的仓库相对目录（空路径表示仓库根），
`python_minor` 显式为 7–14。配置解析、PEP 723 版本冲突、跨项目冲突、选集和测试过滤
由 helper 的 projection 层迁移既有规则；核心本身不发现配置或解释器。

加载只读遍历项目根及显式源码根，校验每个可达路径的真实仓库边界；`.git`、`.venv`
被排除，可达安装环境、逃逸/损坏链接、读取与 UTF-8/编码声明错误明确失败。
源码和目录被冻结到内存 System；内部链接按词法相对路径保留独立模块身份。
调用方必须在加载期间固定输入，快照复制并不是应对并发磁盘写入的原子事务。
加载后原文、AST 和导航使用同一个固定快照。

项目元数据显式设置源码根、版本、全平台、空 extra-paths、内存空 Python prefix 和
`UseUv::Off`。System 没有用户环境、配置目录、缓存、可写能力、命令执行器或可执行文件
查找。内嵌 typeshed 来自固定上游 revision，标准声明不读取用户 typeshed 或 site-packages。
上游依赖的构建脚本只在开发构建时运行，与分析时执行被分析项目没有关系。

`source` 返回原文，`parse` 返回同源 AST 并拒绝恢复/版本错误，`definitions` 接受 UTF-8
字节位置并保留原生全部定义候选和完整/焦点范围。候选保留来源原文；仓库来源为词法相对
路径，标准来源为内嵌 `stdlib/...`。明确的标准函数/类声明另有模块/词法限定名，例如
`builtins` / `str.upper`，其他声明保留原始来源与范围供后续适配。

导航可能同时包含已知目标和缺依赖重绑定，多个 overload 可以映射到相同的标准符号。
这些候选不删除、不合并，也不表示确定的运行时目标。projection 沿用既有绑定反证、
装饰器证明、字段归属和外部身份规则，再在同一进程的输出边界检查来源和最终 Code Graph。

`codegraph-python analyze` 直接接受公共 `codegraph.analyzer/1`。stdin 是一个 UTF-8 JSON
后 EOF，尾换行可选；stdout 只输出一个带固定协议、版本 `0.1.2` 和 `pack_id: python`
的最终 JSON。可处理失败退出 1，CLI 用法错误退出 2。`--help`、`analyze --help` 与
`--version` 输出文本。不存在 ready/cancel、私有协议模式、控制线程或分析 worker。

显式 `files: []` 不读取项目配置或发现 SDK。其他请求在同进程自行装配固定
`python-embedded-stdlib` 声明，版本 `0.0.84` 与上述 revision 绑定。标准 AST 来自
同一数据库的 vendored 文件，不读取磁盘 stdlib、不发现或启动 Python 解释器。
Rust/Python 只共用进程内 DTO、公共输入与必要图检查，内部来源记录不进入公共结果。

调用方负责源码状态一致、EOF 后读完结果、期限及整棵进程树取消。
截至 2026-10-17 的[既有迁移证据](https://github.com/XLCYun/narrant/blob/b594535116654e101658e8d2b94f2719e6c0b082/docs/research/existing-native-helper-migration-validation.md)记录旧结构，
仅作为旧结构的历史记录。当前[分发接口](../../docs/distribution.md)已按
[包布局](../../docs/pack-layout.md)构建七语言直接入口，保留许可材料与固定上游来源。
2026-10-22 的 PR CI 39777180923 中 macOS 成功，Windows 的 Python junction 诊断断言失败；
修复后的完整自动 CI 和首个固定发布仍待完成。

在仓库根执行（所有 Cargo 命令经全局锁与 sccache wrapper）：

```sh
scripts/cargo-agent.sh +1.98.0 check --manifest-path analyzers/python/Cargo.toml --lib --locked
scripts/cargo-agent.sh +1.98.0 build --manifest-path analyzers/python/Cargo.toml --bin codegraph-python --locked
scripts/cargo-agent.sh +1.98.0 test --manifest-path analyzers/python/Cargo.toml --locked --test initial_protocol --test lifecycle -- --test-threads=1
scripts/cargo-agent.sh +1.98.0 clippy --manifest-path analyzers/python/Cargo.toml --all-targets --locked -- -D warnings
scripts/cargo-agent.sh +1.98.0 fmt --manifest-path analyzers/python/Cargo.toml -- --check
```

旧结构验证的日期与范围见上述历史迁移记录。

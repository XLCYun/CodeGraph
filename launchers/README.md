# TS/JS 与 Java 薄入口

此独立 Cargo crate 只生成 `codegraph-typescript-javascript` 和 `codegraph-java`。
两个目标在编译期绑定语言，从真实可执行文件位置找到包根，固定启动随包运行时。
它不链接旧 Host，不读取 JSON、项目配置、SDK、catalog 或 Code Graph。

| 入口 | 运行时 | 语言资源 |
| --- | --- | --- |
| TypeScript/JavaScript | `bin/node[.exe]` | `runtime/analyzer/main.mjs` 及相邻模块；固定声明在 `runtime/` |
| Java | `runtime/jdk/bin/java[.exe]` | `runtime/analyzer/classes` 与 `runtime/analyzer/lib/*` |

标准句柄直接继承。macOS 使用 exec 替换自身；Windows 启动子进程并等待，返回运行时
退出码。调用方仍负责整棵进程树终止。CLI 参数全部转交语言程序，由它处理帮助、版本、
用法和分析。环境采用系统、临时目录、用户位置及既有 SDK 路径提示白名单，运行时注入
变量不继承。验收观察变量仅保留既有的有界测试标记能力，不参与协议或控制。

```sh
scripts/cargo-agent.sh check --manifest-path launchers/Cargo.toml --lib --locked
scripts/cargo-agent.sh test --manifest-path launchers/Cargo.toml --locked
scripts/cargo-agent.sh build --manifest-path launchers/Cargo.toml --bins --locked
```

资源必须位于同一真实安装根内；不搜索 PATH 或旧 `runtime/helper` 目录。调用方仍验证
正式包清单和哈希。完整构建器/自动 CI 的接线由后续迁移完成。

use std::{collections::BTreeMap, fs, path::PathBuf};

use crate::{
    code_graph::{
        AnalysisInput, CodeCategory, CodeGraph, CodeNode, LanguageAnalyzer, NodeId,
        ReferenceTarget, SymbolScope,
    },
    native_analyzer::{HelperCommand, HelperIdentity, NativeLanguageAnalyzer},
};

/// 直接调用公共 Host 与已构建 helper；工具链在测试边界隔离，不注册生产工厂。
pub(super) fn analyzer() -> NativeLanguageAnalyzer {
    let executable =
        PathBuf::from(std::env::var_os("CODEGRAPH_RUST_HELPER").expect("需指定实际 Rust helper"));
    let mut environment = BTreeMap::new();
    environment.insert(
        "RUSTUP_TOOLCHAIN".into(),
        std::env::var_os("CODEGRAPH_PROJECT_RUST_TOOLCHAIN").unwrap_or_else(|| "stable".into()),
    );
    NativeLanguageAnalyzer::new(
        HelperCommand {
            trusted_roots: vec![executable.parent().unwrap().into()],
            executable,
            args: vec![],
            environment,
        },
        HelperIdentity {
            helper_id: "codegraph-rust-engine".into(),
            helper_version: "0.1.0".into(),
            projection_revision: "rust-source-v1".into(),
            languages: vec!["rust".into()],
            profile: "rust-repository-v1".into(),
        },
        vec![],
    )
}

/// 真实 Rust 引擎在同次加载中发现模块；公共 Host 验证 coverage，显式选集仍不扩大。
#[test]
#[ignore = "需要已构建 Rust helper 和项目工具链，单独串行运行"]
fn rust_helper_discovers_modules_without_promoting_context_files() {
    let root = repository(
        "mod nested; #[path=\"extra.md\"] mod extra; pub fn run() { nested::nested_call(); }",
    );
    fs::write(root.path().join("src/nested.rs"), "pub fn nested_call() {}").unwrap();
    fs::write(root.path().join("src/extra.md"), "pub fn extra_call() {}").unwrap();
    fs::write(
        root.path().join("src/unrelated.rs"),
        "pub fn unrelated() {}",
    )
    .unwrap();
    let mut input = AnalysisInput {
        repository_root: root.path().into(),
        files: None,
        options: Default::default(),
    };
    let discovered = analyzer().analyze(&input).unwrap();
    let names: Vec<_> = nodes(&discovered)
        .values()
        .filter_map(|node| node.data().name.as_deref())
        .collect();
    assert!(names.contains(&"run"));
    assert!(names.contains(&"nested_call"));
    assert!(names.contains(&"extra_call"));
    assert!(!names.contains(&"unrelated"));
    input.files = Some(vec!["src/lib.rs".into()]);
    let explicit = analyzer().analyze(&input).unwrap();
    assert!(!nodes(&explicit)
        .values()
        .any(|node| node.data().name.as_deref() == Some("nested_call")));
    assert!(explicit
        .external_symbols()
        .values()
        .any(|symbol| symbol.name.contains("nested_call")));
}

/// 复现 PR31 的真实测试文件：#[test] 按写法统一排除，未标注的辅助函数保留。
#[test]
#[ignore = "需要已构建 Rust helper 和项目工具链，单独串行运行"]
fn rust_helper_excludes_test_attributes_with_glob_imports() {
    let source = "pub mod github_discovery { pub fn discover_pull_requests() {} }\nmod custom;\n";
    let directory = repository(source);
    let manifest = directory.path().join("Cargo.toml");
    fs::write(&manifest, format!("{}\n[lib]\nname=\"codegraph_lib\"\n[dependencies]\nmacros = {{ path = \"macros\" }}\n[dev-dependencies]\nhttpmock=\"0.7\"\nsecrecy=\"0.10\"\ntempfile=\"3\"\n", fs::read_to_string(&manifest).unwrap())).unwrap();
    fs::create_dir(directory.path().join("macros")).unwrap();
    fs::write(directory.path().join("macros/Cargo.toml"), "[package]\nname=\"macros\"\nversion=\"0.1.0\"\nedition=\"2021\"\n[lib]\nproc-macro=true\npath=\"lib.rs\"\n").unwrap();
    fs::write(directory.path().join("macros/lib.rs"), "use proc_macro::TokenStream; #[proc_macro_attribute] pub fn test(_: TokenStream, item: TokenStream) -> TokenStream { item }").unwrap();
    fs::write(
        directory.path().join("src/custom.rs"),
        "use macros::*;\n#[test]\npub fn custom() {}",
    )
    .unwrap();
    fs::create_dir(directory.path().join("tests")).unwrap();
    fs::write(
        directory.path().join("tests/github_discovery.rs"),
        include_str!("fixtures/pr31_github_discovery.rs"),
    )
    .unwrap();
    let analyze = |include_tests| {
        analyzer()
            .analyze(&AnalysisInput {
                repository_root: directory.path().to_owned(),
                files: Some(vec![
                    "tests/github_discovery.rs".into(),
                    "src/custom.rs".into(),
                ]),
                options: crate::code_graph::AnalysisOptions { include_tests },
            })
            .unwrap()
    };
    let filtered = analyze(false);
    let retained = nodes(&filtered);
    let test_functions = [
        "discovers_all_remotes_deduplicates_repositories_and_paginates_metadata_only",
        "rejects_code_content_endpoints_by_only_listing_pull_requests",
        "reports_offline_discovery_per_repository_without_losing_groups",
        "uses_authenticated_metadata_access_and_keeps_permission_failures_local",
        "custom",
    ];
    for name in test_functions {
        assert!(
            !retained
                .values()
                .any(|node| node.data().name.as_deref() == Some(name)),
            "默认关闭时仍生成测试节点: {name}"
        );
    }
    assert!(
        retained
            .values()
            .any(|node| node.category() == CodeCategory::Callable
                && node.data().name.as_deref() == Some("repository_with_remotes")),
        "不应排除无属性的辅助函数"
    );
    let included = analyze(true);
    let complete = nodes(&included);
    for name in test_functions {
        assert!(
            complete
                .values()
                .any(|node| node.data().name.as_deref() == Some(name)),
            "启用后应恢复: {name}"
        );
    }
    save("pr31-test-filter-excluded", &filtered);
    save("pr31-test-filter-included", &included);
}

/// 通过公共根和节点查询遍历结果，验收不依赖图内部存储。
pub(super) fn nodes(graph: &CodeGraph) -> BTreeMap<NodeId, &CodeNode> {
    let mut result = BTreeMap::new();
    let mut pending = graph.root().contains.clone();
    while let Some(id) = pending.pop() {
        let node = graph.node(&id).unwrap();
        pending.extend(node.data().contains.clone());
        result.insert(id, node);
    }
    result
}

/// 构造两个真正独立的磁盘快照，代码仅来自临时目录而非用户编辑缓冲区。
fn repository(source: &str) -> tempfile::TempDir {
    let directory = tempfile::tempdir().unwrap();
    fs::create_dir(directory.path().join("src")).unwrap();
    fs::write(
        directory.path().join("Cargo.toml"),
        "[package]\nname = \"code_graph_fixture\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    )
    .unwrap();
    fs::write(directory.path().join("src/lib.rs"), source).unwrap();
    fs::write(
        directory.path().join("src/store.rs"),
        "pub fn helper() -> u32 { 1 }\n",
    )
    .unwrap();
    directory
}

/// 只描述仓库内未选文件，标准库调用保持未知，不再读取标准库源码。
#[test]
#[ignore = "需要已构建 Rust helper 和项目工具链，单独串行运行"]
fn rust_helper_describes_external_symbols() {
    let repository = repository(
        "mod store; use std::sync::Arc; pub fn run() -> Arc<u32> { { store::helpers::Cache::读取(); Arc::new(store::helper()) } }",
    );
    fs::write(repository.path().join("src/store.rs"),
        "pub fn helper() -> u32 { 1 } pub mod helpers { pub struct Cache; impl Cache { pub fn 读取() {} } }").unwrap();
    let graph = analyzer()
        .analyze(&AnalysisInput {
            options: crate::code_graph::AnalysisOptions {
                include_tests: true,
            },
            repository_root: repository.path().to_path_buf(),
            files: Some(vec![PathBuf::from("src/lib.rs")]),
        })
        .unwrap();
    let all = nodes(&graph);
    let references = all.values().flat_map(|node| &node.data().references);
    for reference in references {
        if let ReferenceTarget::External { symbol } = &reference.target {
            let description = graph
                .external_symbols()
                .get(symbol)
                .expect("每个已解析图外目标都应有描述");
            assert!(!description.name.contains("%2F"), "{description:?}");
        }
    }
    assert!(graph
        .external_symbols()
        .values()
        .any(|symbol| symbol.name == "helper" && symbol.scope == SymbolScope::Repository));
    assert!(
        graph
            .external_symbols()
            .values()
            .all(|symbol| symbol.scope == SymbolScope::Repository),
        "{:#?}",
        graph.external_symbols()
    );
    let (symbol, description) = graph
        .external_symbols()
        .iter()
        .find(|(_, description)| description.name == "helpers::Cache::读取")
        .expect("图外名称保留语法层级及 Unicode");
    let separator = if cfg!(windows) { "%5C" } else { "%2F" };
    assert!(
        symbol.contains(&format!("src{separator}store.rs")),
        "原生相对文件 ID 编码保持稳定: {symbol}"
    );
    assert_eq!(description.scope, SymbolScope::Repository);
    assert!(all.values().flat_map(|node| &node.data().references).any(|reference| {
        reference.semantic == "code.calls"
            && matches!(&reference.target, ReferenceTarget::Unresolved { name } if name == "new")
    }));
    assert!(!all
        .values()
        .any(|node| node.data().name.as_deref() == Some("helper")));
    save("external-symbols", &graph);
}

/// 自定义入口和空源码也保留服务器确认的根，不依赖 src/lib.rs 或 tests/*.rs 的命名。
#[test]
#[ignore = "需要已构建 Rust helper 和项目工具链，单独串行运行"]
fn rust_helper_recognizes_custom_and_empty_crate_roots() {
    let directory = tempfile::tempdir().unwrap();
    fs::write(directory.path().join("Cargo.toml"),
        "[package]\nname=\"custom_root_fixture\"\nversion=\"0.1.0\"\nedition=\"2021\"\n[lib]\nname=\"custom_library\"\npath=\"library_entry.rs\"\n[[test]]\nname=\"smoke\"\npath=\"test_entry.rs\"\n").unwrap();
    fs::write(directory.path().join("library_entry.rs"), "").unwrap();
    fs::write(
        directory.path().join("test_entry.rs"),
        "#[test] fn checks() {}",
    )
    .unwrap();
    let graph = analyzer()
        .analyze(&AnalysisInput {
            options: crate::code_graph::AnalysisOptions {
                include_tests: true,
            },
            repository_root: directory.path().to_owned(),
            files: Some(vec!["library_entry.rs".into(), "test_entry.rs".into()]),
        })
        .unwrap();
    let roots: Vec<_> = graph
        .root()
        .contains
        .iter()
        .map(|id| graph.node(id).unwrap())
        .collect();
    assert_eq!(roots.len(), 2);
    assert!(roots.iter().all(|root| root.data().native_kind == "crate"));
    assert!(roots.iter().any(
        |root| root.data().name.as_deref() == Some("library_entry.rs")
            && root.data().contains.is_empty()
    ));
    assert!(roots
        .iter()
        .any(|root| root.data().name.as_deref() == Some("test_entry.rs")
            && root.data().contains.len() == 1));
}

/// 当前仓库回归覆盖较大的真实类型、模块与关联，不依赖穿刺时固定的 crate 名。
/// 可选地保存本次真实验收的完整图证据；输出目录由测试调用者指定。
fn save(name: &str, graph: &CodeGraph) {
    if let Some(directory) = std::env::var_os("CODEGRAPH_CODE_GRAPH_OUTPUT") {
        let directory = PathBuf::from(directory);
        fs::create_dir_all(&directory).unwrap();
        fs::write(
            directory.join(format!("{name}.txt")),
            format!("{graph:#?}\n"),
        )
        .unwrap();
    }
}

/// 物理目录别名经公共 Host 只产生一组稳定节点、原文与内部调用。
#[test]
#[ignore = "需要真实 Rust helper"]
fn rust_helper_deduplicates_aliases_with_stable_graph_content() {
    let root = repository("mod store; pub fn run() -> u32 { store::helper() }");
    #[cfg(unix)]
    std::os::unix::fs::symlink(root.path().join("src"), root.path().join("alias")).unwrap();
    #[cfg(windows)]
    {
        // 子命令输出随失败断言展示，不能插入 libtest 的逐名成功记录。
        let output = std::process::Command::new("cmd")
            .args(["/c", "mklink", "/J"])
            .arg(root.path().join("alias"))
            .arg(root.path().join("src"))
            .output()
            .unwrap();
        assert!(output.status.success(), "{output:?}");
    }
    let input = AnalysisInput {
        repository_root: root.path().into(),
        files: Some(vec![
            "src/lib.rs".into(),
            "alias/lib.rs".into(),
            "src/store.rs".into(),
            "alias/store.rs".into(),
        ]),
        options: Default::default(),
    };
    let native = analyzer().analyze(&input).unwrap();
    assert_eq!(native.root().contains.len(), 1);
    assert!(native.external_symbols().is_empty());
    for node in nodes(&native).values() {
        assert!(node
            .data()
            .source
            .as_ref()
            .is_some_and(|source| !source.is_empty()));
        assert!(!node.data().id.0.contains("alias/"));
    }
    assert!(nodes(&native).values().any(|node| node.data().name.as_deref() == Some("run")
        && node.data().references.iter().any(|reference| matches!(&reference.target,
            ReferenceTarget::Node(node_id) if native.node(node_id).unwrap().data().name.as_deref() == Some("helper")))));
    assert_eq!(nodes(&native).len(), 4);
}

/// 清单发现覆盖多独立项目和显式 JSON；非空 standalone 失败，空选集仍成功。
#[test]
#[ignore = "需要真实 Rust helper"]
fn rust_helper_loads_project_shapes_and_preserves_standalone_failure() {
    let root = tempfile::tempdir().unwrap();
    let analyze = |files: &[&str]| {
        analyzer().analyze(&AnalysisInput {
            repository_root: root.path().into(),
            files: Some(files.iter().map(PathBuf::from).collect()),
            options: Default::default(),
        })
    };
    assert!(analyze(&[]).unwrap().root().contains.is_empty());
    fs::write(
        root.path().join("lib.rs"),
        "fn target() {} fn run() { target(); }",
    )
    .unwrap();
    assert!(analyze(&["lib.rs"])
        .unwrap_err()
        .message
        .contains("standalone"));
    fs::write(
        root.path().join("rust-project.json"),
        r#"{"crates":[{"root_module":"lib.rs","edition":"2021","deps":[],"cfg":[]}]}"#,
    )
    .unwrap();
    let graph = analyze(&["lib.rs"]).unwrap();
    assert_eq!(nodes(&graph).len(), 2);
    assert!(nodes(&graph)
        .values()
        .all(|node| node.data().native_kind != "crate"));
    assert!(nodes(&graph).values().any(|node| node
        .data()
        .references
        .iter()
        .any(|reference| reference.semantic == "code.calls"
            && matches!(reference.target, ReferenceTarget::Node(_)))));
    fs::remove_file(root.path().join("rust-project.json")).unwrap();
    for name in ["a", "b"] {
        fs::create_dir(root.path().join(name)).unwrap();
        fs::write(root.path().join(name).join("Cargo.toml"), format!("[package]\nname=\"{name}\"\nversion=\"0.1.0\"\nedition=\"2021\"\n[lib]\npath=\"lib.rs\"\n")).unwrap();
        fs::write(
            root.path().join(name).join("lib.rs"),
            "fn target() {} fn run() { target(); }",
        )
        .unwrap();
    }
    let graph = analyze(&["a/lib.rs", "b/lib.rs"]).unwrap();
    assert_eq!(nodes(&graph).len(), 6);
    assert_eq!(graph.root().contains.len(), 2);
}

/// 正在真实 Cargo 加载期间取消，公共 Host 必须有界返回 Cancelled 并回收 helper 进程树。
#[test]
#[ignore = "需要真实 Rust helper"]
fn rust_helper_cancellation_uses_common_process_ownership() {
    use std::{
        sync::{
            atomic::{AtomicBool, Ordering},
            Arc,
        },
        time::{Duration, Instant},
    };
    let root = repository("pub fn run() {}");
    let cancelled = Arc::new(AtomicBool::new(false));
    let read = cancelled.clone();
    let analyzer = analyzer().with_cancellation(Arc::new(move || read.load(Ordering::SeqCst)));
    let started = Instant::now();
    let trigger = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(100));
        cancelled.store(true, Ordering::SeqCst);
    });
    let error = analyzer
        .analyze(&AnalysisInput {
            repository_root: root.path().into(),
            files: Some(vec!["src/lib.rs".into()]),
            options: Default::default(),
        })
        .unwrap_err();
    trigger.join().unwrap();
    assert!(error.is_cancelled(), "{error:?}");
    assert!(started.elapsed() < Duration::from_secs(10));
}

/// 用户显式覆盖优先于目录工具链；目录选择与 helper 构建版本不同仍用于实际 metadata。
#[cfg(unix)]
#[test]
#[ignore = "需要真实 Rust helper 与 stable/1.98.0 已有工具链"]
fn rust_helper_runtime_toolchain_selection_reaches_real_cargo_and_rustc() {
    use std::os::unix::fs::PermissionsExt;
    let root = repository("pub fn run() {}");
    let tools = tempfile::tempdir().unwrap();
    let log = tools.path().join("tools.log");
    let identities = tools.path().join("identities.log");
    // CI 的 HOME 与 Cargo 依赖缓存独立；从 rustup 所在目录取代理，保留 +stable 与目录覆盖语义。
    let cargo_bin = std::env::split_paths(&std::env::var_os("PATH").unwrap())
        .find(|directory| {
            ["rustup", "cargo", "rustc"]
                .iter()
                .all(|tool| directory.join(tool).is_file())
        })
        .expect("PATH 需要包含 rustup 及其 cargo/rustc 代理目录");
    for tool in ["cargo", "rustc"] {
        let wrapper = tools.path().join(tool);
        fs::write(&wrapper,format!("#!/bin/sh\nprintf '%s %s %s\\n' '{tool}' \"$RUSTUP_TOOLCHAIN\" \"$*\" >> '{}'\nif [ '{tool}' = rustc ]; then '{}' --print sysroot >> '{}'; '{}' --version >> '{}'; fi\nexec '{}' \"$@\"\n",log.display(),cargo_bin.join("rustc").display(),identities.display(),cargo_bin.join("rustc").display(),identities.display(),cargo_bin.join(tool).display())).unwrap();
        fs::set_permissions(&wrapper, fs::Permissions::from_mode(0o755)).unwrap();
    }
    let mut paths = vec![tools.path().to_path_buf()];
    paths.extend(std::env::split_paths(&std::env::var_os("PATH").unwrap()));
    let path = std::env::join_paths(paths).unwrap();
    let expected_identity = ["--print=sysroot", "--version"].map(|argument| {
        let result = std::process::Command::new(cargo_bin.join("rustc"))
            .args(["+stable", argument])
            .env_remove("RUSTUP_TOOLCHAIN")
            .output()
            .unwrap();
        assert!(result.status.success());
        String::from_utf8(result.stdout).unwrap().trim().to_owned()
    });
    let assert_identity = || {
        let actual = fs::read_to_string(&identities).unwrap();
        let lines = actual.lines().collect::<Vec<_>>();
        assert!(!lines.is_empty() && lines.len() % 2 == 0, "{actual}");
        // 原生加载并行探测，日志可能交错；每条实际身份必须精确匹配 stable 的 sysroot/版本。
        assert!(
            lines
                .iter()
                .all(|line| expected_identity.iter().any(|expected| line == expected)),
            "{actual}"
        );
        assert_eq!(
            lines
                .iter()
                .filter(|line| **line == expected_identity[0])
                .count(),
            lines.len() / 2,
            "{actual}"
        );
    };
    let input = AnalysisInput {
        repository_root: root.path().into(),
        files: Some(vec!["src/lib.rs".into()]),
        options: Default::default(),
    };
    fs::write(
        root.path().join("rust-toolchain.toml"),
        "[toolchain]\nchannel=\"1.98.0\"\n",
    )
    .unwrap();
    let mut explicit = analyzer();
    explicit
        .command
        .environment
        .insert("PATH".into(), path.clone());
    explicit
        .command
        .environment
        .insert("RUSTUP_TOOLCHAIN".into(), "stable".into());
    explicit.analyze(&input).unwrap();
    let text = fs::read_to_string(&log).unwrap();
    assert!(
        text.lines()
            .any(|line| line.starts_with("cargo stable --version")),
        "{text}"
    );
    assert!(
        text.lines()
            .any(|line| line.starts_with("cargo stable metadata")),
        "{text}"
    );
    assert!(
        text.lines()
            .any(|line| line.starts_with("rustc stable --print sysroot")),
        "{text}"
    );
    assert!(
        text.lines().all(|line| line
            .split_whitespace()
            .nth(1)
            .is_some_and(|toolchain| toolchain == "stable" || toolchain.starts_with("stable-"))),
        "{text}"
    );
    assert_identity();
    fs::write(&identities, "").unwrap();
    fs::write(&log, "").unwrap();
    fs::write(
        root.path().join("rust-toolchain.toml"),
        "[toolchain]\nchannel=\"stable\"\n",
    )
    .unwrap();
    let mut directory = analyzer();
    directory.command.environment.insert("PATH".into(), path);
    directory
        .command
        .environment
        .remove(std::ffi::OsStr::new("RUSTUP_TOOLCHAIN"));
    directory.analyze(&input).unwrap();
    let text = fs::read_to_string(&log).unwrap();
    assert!(
        text.lines().any(|line| line.starts_with("cargo  metadata")),
        "{text}"
    );
    assert!(
        text.lines()
            .any(|line| line.starts_with("rustc  --print sysroot")),
        "{text}"
    );
    assert_identity();
    // 真实代理成功运行 metadata，目录选择没有被 helper 的构建环境覆盖。
    let version = std::process::Command::new("rustc")
        .args(["--version"])
        .current_dir(root.path())
        .env_remove("RUSTUP_TOOLCHAIN")
        .output()
        .unwrap();
    assert!(version.status.success());
    eprintln!(
        "目录工具链实际版本：{}",
        String::from_utf8(version.stdout).unwrap()
    );
}

/// 真实多项目允许仅仓库范围使用不同目录工具链，标准库范围仍明确拒绝 sysroot 冲突。
#[test]
#[ignore = "需要真实 Rust helper 与 stable/1.98.0 已有工具链"]
fn rust_helper_multiple_project_toolchains_keep_scope_boundary() {
    let root = tempfile::tempdir().unwrap();
    for (name, channel) in [("a", "stable"), ("b", "1.98.0")] {
        let directory = root.path().join(name);
        fs::create_dir(&directory).unwrap();
        fs::write(directory.join("Cargo.toml"),format!("[package]\nname=\"{name}\"\nversion=\"0.1.0\"\nedition=\"2021\"\n[lib]\npath=\"lib.rs\"\n")).unwrap();
        fs::write(directory.join("lib.rs"), "pub fn run() {}").unwrap();
        fs::write(
            directory.join("rust-toolchain.toml"),
            format!("[toolchain]\nchannel=\"{channel}\"\n"),
        )
        .unwrap();
    }
    let input = AnalysisInput {
        repository_root: root.path().into(),
        files: Some(vec!["a/lib.rs".into(), "b/lib.rs".into()]),
        options: Default::default(),
    };
    let mut analyzer = analyzer();
    analyzer
        .command
        .environment
        .remove(std::ffi::OsStr::new("RUSTUP_TOOLCHAIN"));
    assert_eq!(analyzer.analyze(&input).unwrap().root().contains.len(), 2);
    let output = std::process::Command::new("rustc")
        .args(["--print", "sysroot"])
        .env_remove("RUSTUP_TOOLCHAIN")
        .current_dir(root.path().join("a"))
        .output()
        .unwrap();
    assert!(output.status.success());
    analyzer.identity.profile = "rust-standard-library-v1".into();
    analyzer.platforms = vec![super::Platform {
        id: "project-rust".into(),
        kind: "rust-standard-library".into(),
        version: "test-project".into(),
        root: PathBuf::from(String::from_utf8(output.stdout).unwrap().trim()),
        metadata: serde_json::json!({"rust_src":"lib/rustlib/src/rust/library"}),
    }];
    let error = analyzer.analyze(&input).unwrap_err();
    assert!(
        error.message.contains("多个 Rust 项目使用不同工具链"),
        "{error:?}"
    );
}

/// 仓库内逃逸链接仍保持未解析目标，不能读取仓库外的声明作为 External。
#[cfg(unix)]
#[test]
#[ignore = "需要真实 Rust helper"]
fn rust_helper_escaping_context_link_is_not_promoted_to_repository_symbol() {
    let root = repository("#[path=\"outside.rs\"] mod outside; fn run() { outside::target(); }");
    let outside = tempfile::tempdir().unwrap();
    fs::write(outside.path().join("lib.rs"), "pub fn target() {}").unwrap();
    std::os::unix::fs::symlink(
        outside.path().join("lib.rs"),
        root.path().join("src/outside.rs"),
    )
    .unwrap();
    let graph = analyzer()
        .analyze(&AnalysisInput {
            repository_root: root.path().into(),
            files: Some(vec!["src/lib.rs".into()]),
            options: Default::default(),
        })
        .unwrap();
    let all = nodes(&graph);
    let run = all
        .values()
        .find(|node| node.data().name.as_deref() == Some("run"))
        .unwrap();
    assert!(run.data().references.iter().any(
        |reference| matches!(&reference.target,ReferenceTarget::Unresolved{name} if name=="target")
    ));
    assert!(graph.external_symbols().is_empty());
}

/// 等真实 helper 进入工具链子进程后再取消，PID 观察证明公共 ProcessTree 回收该后代。
#[cfg(unix)]
#[test]
#[ignore = "需要真实 Rust helper"]
fn rust_helper_cancellation_reaps_active_toolchain_probe() {
    use std::{
        os::unix::fs::PermissionsExt,
        sync::Arc,
        time::{Duration, Instant},
    };
    let root = repository("pub fn run() {}");
    let tools = tempfile::tempdir().unwrap();
    let marker = tools.path().join("probe.pid");
    let command = tools.path().join("cargo");
    fs::write(
        &command,
        format!(
            "#!/bin/sh\necho $$ > '{}'\nexec /bin/sleep 30\n",
            marker.display()
        ),
    )
    .unwrap();
    fs::set_permissions(&command, fs::Permissions::from_mode(0o755)).unwrap();
    let mut analyzer = analyzer();
    let mut paths = vec![tools.path().into()];
    paths.extend(std::env::split_paths(&std::env::var_os("PATH").unwrap()));
    analyzer
        .command
        .environment
        .insert("PATH".into(), std::env::join_paths(paths).unwrap());
    let observed = marker.clone();
    let started = Instant::now();
    let error = analyzer
        .with_cancellation(Arc::new(move || observed.is_file()))
        .analyze(&AnalysisInput {
            repository_root: root.path().into(),
            files: Some(vec!["src/lib.rs".into()]),
            options: Default::default(),
        })
        .unwrap_err();
    assert!(error.is_cancelled(), "{error:?}");
    assert!(started.elapsed() < Duration::from_secs(10));
    let pid = fs::read_to_string(&marker)
        .unwrap()
        .trim()
        .parse::<i32>()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        // SAFETY: signal 0 只检查刚由本夹具记录的后代 PID 是否存在。
        if unsafe { libc::kill(pid, 0) } == -1 {
            break;
        }
        assert!(Instant::now() < deadline, "工具链后代未回收：{pid}");
        std::thread::sleep(Duration::from_millis(10));
    }
}

/// 旧语法断言通过真实 helper 与公共 Host 构图，diff 不绕过统一 CodeGraph 校验。
fn syntax_graph(source: &str, include_tests: bool) -> CodeGraph {
    let root = repository(source);
    analyzer()
        .analyze(&AnalysisInput {
            repository_root: root.path().into(),
            files: Some(vec!["src/lib.rs".into()]),
            options: crate::code_graph::AnalysisOptions { include_tests },
        })
        .unwrap()
}

/// 原生语法失败仍由真实 helper 输出 error，生命周期控制不能把失败误报为取消。
#[test]
#[ignore = "需要实际 Rust helper，单独串行运行"]
fn rust_helper_lifecycle_native_failure_remains_error() {
    let root = repository("fn broken( {");
    let error = analyzer()
        .analyze(&AnalysisInput {
            repository_root: root.path().into(),
            files: Some(vec!["src/lib.rs".into()]),
            options: Default::default(),
        })
        .unwrap_err();
    assert!(!error.is_cancelled(), "{error}");
    assert!(error.message.contains("无法解析"), "{error}");
}

use super::*;
use std::{fs, path::PathBuf, sync::Arc};

/// 使用正常构建的真实支持包，测试不覆盖用户安装或修改真实项目。
fn analyzer() -> RustPackAnalyzer {
    let root = PathBuf::from(std::env::var_os("CODEGRAPH_RUST_PACK").expect("真实 Rust 支持包"));
    RustPackAnalyzer::from_pack(&root).unwrap()
}

/// 生产工厂正在探测真实工具链时取消，公共唯一进程 owner 必须收回阻塞后代。
#[cfg(unix)]
#[test]
#[ignore = "需要真实 Rust 支持包"]
fn production_cancellation_reaps_toolchain_probe() {
    use std::{
        os::unix::fs::PermissionsExt,
        time::{Duration, Instant},
    };
    let root = tempfile::tempdir().unwrap();
    fs::write(root.path().join("Cargo.toml"),"[package]\nname=\"cancel_fixture\"\nversion=\"0.1.0\"\nedition=\"2021\"\n[lib]\npath=\"lib.rs\"\n").unwrap();
    fs::write(root.path().join("lib.rs"), "pub fn entry() {}").unwrap();
    let tools = tempfile::tempdir().unwrap();
    let marker = tools.path().join("pid");
    let wrapper = tools.path().join("cargo");
    fs::write(
        &wrapper,
        format!(
            "#!/bin/sh\necho $$ > '{}'\nexec /bin/sleep 30\n",
            marker.display()
        ),
    )
    .unwrap();
    fs::set_permissions(&wrapper, fs::Permissions::from_mode(0o755)).unwrap();
    let mut analyzer = analyzer();
    let mut paths = vec![tools.path().to_owned()];
    paths.extend(std::env::split_paths(
        &analyzer.environment[std::ffi::OsStr::new("PATH")],
    ));
    analyzer
        .environment
        .insert("PATH".into(), std::env::join_paths(paths).unwrap());
    let observed = marker.clone();
    let started = Instant::now();
    let error = analyzer
        .analyze(
            &AnalysisInput {
                repository_root: root.path().into(),
                files: vec!["lib.rs".into()],
                options: Default::default(),
            },
            Arc::new(move || observed.is_file()),
            RustDependencyScope::RepositoryOnly,
        )
        .unwrap_err();
    assert!(error.is_cancelled());
    assert!(started.elapsed() < Duration::from_secs(10));
    let pid = fs::read_to_string(marker)
        .unwrap()
        .trim()
        .parse::<i32>()
        .unwrap();
    // SAFETY: signal 0 只检查本夹具记录的进程是否已被公共 owner 回收。
    assert_eq!(unsafe { libc::kill(pid, 0) }, -1);
}

/// 空选集的三档都不启动工具或 helper，取消仍是独立失败。
#[test]
fn empty_selection_keeps_all_scopes_without_project_tools() {
    let root = tempfile::tempdir().unwrap();
    for folder in ["bin", "control"] {
        fs::create_dir(root.path().join(folder)).unwrap();
    }
    fs::write(
        root.path().join(if cfg!(windows) {
            "bin/codegraph-rust-engine.exe"
        } else {
            "bin/codegraph-rust-engine"
        }),
        "not executable",
    )
    .unwrap();
    fs::write(
        root.path().join("control/native.json"),
        r#"{"schema_version":1,"helper_version":"0.1.0","platforms":[]}"#,
    )
    .unwrap();
    let mut analyzer = RustPackAnalyzer::from_pack(root.path()).unwrap();
    analyzer
        .environment
        .insert("PATH".into(), "missing-tools".into());
    let repository = tempfile::tempdir().unwrap();
    let input = AnalysisInput {
        repository_root: repository.path().into(),
        files: vec![],
        options: Default::default(),
    };
    for scope in [
        RustDependencyScope::RepositoryOnly,
        RustDependencyScope::StandardLibrary,
        RustDependencyScope::AllDependencies,
    ] {
        assert!(analyzer
            .analyze(&input, Arc::new(|| false), scope)
            .unwrap()
            .root()
            .contains
            .is_empty());
        assert!(analyzer
            .analyze(&input, Arc::new(|| true), scope)
            .unwrap_err()
            .is_cancelled());
    }
}

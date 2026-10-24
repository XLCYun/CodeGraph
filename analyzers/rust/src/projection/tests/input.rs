use super::*;

/// 使用绝对临时目录作为输入，不访问实际项目工具链。
fn input() -> CompleteAnalysisInput {
    CompleteAnalysisInput {
        repository_root: std::env::temp_dir(),
        files: None,
        include_tests: false,
        dependency_scope: DependencyScope::RepositoryOnly,
        platforms: vec![],
        observation: None,
    }
}

/// 平台种类固定为 Rust 标准库，声明路径仍需通过实际工具链核验。
fn platform() -> RustPlatform {
    RustPlatform {
        id: "project-rust".into(),
        version: "project".into(),
        root: std::env::temp_dir(),
        rust_src: "lib/rustlib/src/rust/library".into(),
    }
}

/// 原生输入仍拒绝路径歧义、重复选集与过大选集，发现和显式空集均合法。
#[test]
fn native_selection_keeps_path_and_resource_constraints() {
    let mut input = input();
    input.validate().unwrap();
    input.files = Some(vec![]);
    input.validate().unwrap();
    for files in [
        vec!["../lib.rs"],
        vec!["src\\lib.rs"],
        vec!["lib.rs", "lib.rs"],
    ] {
        input.files = Some(files.into_iter().map(str::to_owned).collect());
        assert!(input.validate().is_err());
    }
    input.files = Some(vec![
        "lib.rs".into();
        codegraph_engine_common::selection::MAX_FILES + 1
    ]);
    assert!(
        input
            .validate()
            .unwrap_err()
            .to_string()
            .contains("数量上限")
    );
    input.files = None;
    input.repository_root = "relative".into();
    assert!(input.validate().is_err());
}

/// 仅仓库不接受平台，标准库与全部依赖至多接受一个声明。
#[test]
fn platform_count_follows_native_dependency_scope() {
    let mut input = input();
    input.platforms.push(platform());
    assert!(input.validate().is_err());
    for scope in [
        DependencyScope::StandardLibrary,
        DependencyScope::AllDependencies,
    ] {
        input.dependency_scope = scope;
        input.validate().unwrap();
        input.platforms.push(platform());
        assert!(input.validate().is_err());
        input.platforms.pop();
    }
}

/// 类型化平台保留身份和源码相对路径检查，不能扩大 sysroot 授权。
#[test]
fn platform_identity_and_relative_source_remain_required() {
    let mut input = input();
    input.dependency_scope = DependencyScope::StandardLibrary;
    input.platforms.push(platform());
    input.validate().unwrap();
    for source in [
        "",
        "../library",
        "/library",
        "lib/./library",
        "lib//library",
        "lib\\library",
        "C:library",
        "lib\0library",
    ] {
        input.platforms[0].rust_src = source.into();
        assert!(input.validate().is_err(), "{source:?}");
    }
    input.platforms[0] = platform();
    input.platforms[0].id.clear();
    assert!(input.validate().is_err());
    input.platforms[0] = platform();
    input.platforms[0].version.clear();
    assert!(input.validate().is_err());
    input.platforms[0] = platform();
    input.platforms[0].root = "relative".into();
    assert!(input.validate().is_err());
}

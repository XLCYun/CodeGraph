//! 实际公共入口使用真实工具链副本，验证项目与目标 SDK 的来源边界。
use serde_json::{Value, json};
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
    process::{Command, Output, Stdio},
};

/// 复制既有测试工具链，不修改原 SDK 或下载任何新组件。
fn copy_tree(source: &Path, destination: &Path) {
    fs::create_dir_all(destination).unwrap();
    for entry in fs::read_dir(source).unwrap() {
        let entry = entry.unwrap();
        // 文档不参与 rustc/sysroot/rust-src；仅复制完整的工具与库，避免重复数 GiB 文档。
        if entry.file_name() == "share" {
            continue;
        }
        let target = destination.join(entry.file_name());
        if entry.path().is_dir() {
            copy_tree(&entry.path(), &target);
        } else {
            fs::copy(entry.path(), target).unwrap();
        }
    }
}

/// 使用已配置且带 rust-src 的真实项目工具链，不误用仅供旧 Cargo metadata 的夹具。
fn existing_toolchain() -> PathBuf {
    let toolchain =
        std::env::var_os("CODEGRAPH_PROJECT_RUST_TOOLCHAIN").unwrap_or_else(|| "stable".into());
    let output = Command::new("rustc")
        .env("RUSTUP_TOOLCHAIN", toolchain)
        .args(["--print", "sysroot"])
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    PathBuf::from(String::from_utf8(output.stdout).unwrap().trim())
}

/// 配置真实 Cargo 库项目，标准引用必须来自既有目标工具链。
fn project(repository: &Path) {
    fs::create_dir_all(repository).unwrap();
    fs::write(
        repository.join("Cargo.toml"),
        "[package]\nname='sdk_boundary'\nversion='0.1.0'\nedition='2021'\n[lib]\npath='lib.rs'\n",
    )
    .unwrap();
    fs::write(
        repository.join("lib.rs"),
        "pub fn run() -> String { String::new() }\n",
    )
    .unwrap();
}

/// 子进程从正常 PATH 和 RUSTUP_TOOLCHAIN 选择副本，公共 JSON 不携带 SDK 路径。
fn exchange(repository: &Path, sdk: &Path, scope: &str, empty: bool) -> (Output, Value) {
    let request = json!({"protocol":"codegraph.analyzer/1","type":"analyze","id":"sdk-boundary","repository_root":repository,"files":if empty {vec![]} else {vec!["lib.rs"]},"options":{"include_tests":false,"rust_dependency_scope":scope}});
    let mut paths = vec![sdk.join("bin")];
    paths.extend(std::env::split_paths(
        &std::env::var_os("PATH").unwrap_or_default(),
    ));
    let before: Vec<_> = ["Cargo.toml", "lib.rs"]
        .iter()
        .map(|name| fs::read(repository.join(name)).unwrap())
        .collect();
    let mut entries: Vec<_> = fs::read_dir(repository)
        .unwrap()
        .map(|entry| entry.unwrap().file_name())
        .collect();
    entries.sort();
    let mut child = Command::new(env!("CARGO_BIN_EXE_codegraph-rust"))
        .arg("analyze")
        .current_dir(std::env::temp_dir())
        .env("PATH", std::env::join_paths(paths).unwrap())
        .env("RUSTUP_TOOLCHAIN", sdk)
        .env("CARGO_NET_OFFLINE", "true")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(request.to_string().as_bytes())
        .unwrap();
    let output = child.wait_with_output().unwrap();
    let after: Vec<_> = ["Cargo.toml", "lib.rs"]
        .iter()
        .map(|name| fs::read(repository.join(name)).unwrap())
        .collect();
    assert_eq!(before, after, "分析不得改写项目输入");
    let mut final_entries: Vec<_> = fs::read_dir(repository)
        .unwrap()
        .map(|entry| entry.unwrap().file_name())
        .collect();
    final_entries.sort();
    assert_eq!(entries, final_entries, "分析不得创建项目锁文件或构建产物");
    let value: Value = serde_json::from_slice(&output.stdout)
        .unwrap_or_else(|error| panic!("{error}: {output:?}"));
    assert!(output.stdout.ends_with(b"\n"));
    assert_eq!(value["protocol"], "codegraph.analyzer/1");
    assert_eq!(value["engine_version"], "0.1.0");
    assert_eq!(value["pack_id"], "rust");
    assert_eq!(value["id"], "sdk-boundary");
    (output, value)
}

/// 真实错误分类与详细诊断缺一不可，不能用其他加载失败掩盖 SDK 边界。
fn rejected(repository: &Path, sdk: &Path, diagnostic: &str) {
    for scope in ["standard-library", "all-dependencies"] {
        let (output, value) = exchange(repository, sdk, scope, false);
        assert_eq!(
            output.status.code(),
            Some(1),
            "{scope}: {value}; {output:?}"
        );
        assert_eq!(value["outcome"], "error");
        assert_eq!(value["error"]["code"], "analysis_failed");
        assert!(value.get("graph").is_none());
        assert!(
            String::from_utf8_lossy(&output.stderr).contains(diagnostic),
            "{scope}: {output:?}"
        );
        let (output, value) = exchange(repository, sdk, scope, true);
        assert!(output.status.success(), "{output:?}");
        assert_eq!(value["graph"]["nodes"], json!([]));
    }
}

/// SDK 位于源码内或源码位于 SDK 内时，两种实际依赖模式均拒绝重叠。
#[test]
fn public_rust_rejects_both_sdk_repository_containment_directions() {
    let owned = tempfile::tempdir().unwrap();
    let repository = owned.path().join("repository");
    let sdk = repository.join("sdk");
    copy_tree(&existing_toolchain(), &sdk);
    project(&repository);
    rejected(&repository, &sdk, "目标 SDK 根不能与分析仓库重叠");
    let nested = sdk.join("project");
    project(&nested);
    rejected(&nested, &sdk, "目标 SDK 根不能与分析仓库重叠");
}

/// 正常外部 SDK 的实际标准库引用仍可分析，不能通过一律拒绝来修复边界。
#[test]
fn public_rust_accepts_genuine_external_sdk() {
    let repository = tempfile::tempdir().unwrap();
    project(repository.path());
    let sdk = existing_toolchain();
    for scope in ["repository-only", "standard-library", "all-dependencies"] {
        let (output, value) = exchange(repository.path(), &sdk, scope, false);
        assert!(output.status.success(), "{scope}: {value}; {output:?}");
        assert!(
            value["graph"]["nodes"]
                .as_array()
                .unwrap()
                .iter()
                .any(|node| node["name"] == "run")
        );
        assert!(
            value["graph"]["nodes"]
                .as_array()
                .unwrap()
                .iter()
                .all(|node| node["id"]
                    .as_str()
                    .unwrap()
                    .starts_with("rust:repo:lib.rs::"))
        );
        assert!(
            value["graph"]["nodes"]
                .as_array()
                .unwrap()
                .iter()
                .any(|node| node["source"]
                    .as_str()
                    .is_some_and(|source| source.contains("String::new()")))
        );
    }
}

/// core/std 的真实文件链接越出已接受 sysroot 时不得加载，原 SDK 全程只读。
#[cfg(unix)]
#[test]
fn public_rust_rejects_standard_source_links_escaping_sysroot() {
    let owned = tempfile::tempdir().unwrap();
    let sdk = owned.path().join("sdk");
    let original = existing_toolchain();
    copy_tree(&original, &sdk);
    let repository = owned.path().join("repository");
    project(&repository);
    for name in ["core", "std"] {
        let relative = format!("lib/rustlib/src/rust/library/{name}/src/lib.rs");
        let file = sdk.join(&relative);
        let bytes = fs::read(&file).unwrap();
        fs::remove_file(&file).unwrap();
        std::os::unix::fs::symlink(original.join(&relative), &file).unwrap();
        rejected(&repository, &sdk, "Rust 标准声明不存在或逃逸 sysroot");
        fs::remove_file(&file).unwrap();
        fs::write(file, bytes).unwrap();
    }
}

//! Rust 程序自行装配项目工具链政策并在同一进程内构图。
use crate::{DependencyScope, projection::CompleteAnalysisInput};
use codegraph_engine_common::{
    graph,
    model::CodeGraph,
    public_protocol::{CheckedRequest, RustDependencyScope},
};
use std::collections::BTreeMap;

/// 保留已有网络与工具链白名单，禁止环境中的包装器、编译参数或进程注入。
pub fn environment(
    lookup: impl Fn(&str) -> Option<std::ffi::OsString>,
) -> BTreeMap<std::ffi::OsString, std::ffi::OsString> {
    let mut environment = BTreeMap::new();
    for key in [
        "PATH",
        "HOME",
        "USERPROFILE",
        "CARGO_HOME",
        "RUSTUP_HOME",
        "RUSTUP_TOOLCHAIN",
        "CARGO_NET_OFFLINE",
        "CARGO_NET_RETRY",
        "CARGO_NET_GIT_FETCH_WITH_CLI",
        "CARGO_HTTP_PROXY",
        "CARGO_HTTP_TIMEOUT",
        "CARGO_HTTP_CAINFO",
        "CARGO_HTTP_SSL_VERIFY",
        "CARGO_HTTP_CHECK_REVOKE",
        "CARGO_HTTP_MULTIPLEXING",
        "CARGO_HTTP_LOW_SPEED_LIMIT",
        "HTTP_PROXY",
        "HTTPS_PROXY",
        "ALL_PROXY",
        "NO_PROXY",
        "http_proxy",
        "https_proxy",
        "all_proxy",
        "no_proxy",
        "SSL_CERT_FILE",
        "SSL_CERT_DIR",
        "SystemRoot",
        "CODEGRAPH_TEST_DISCOVERY_PROBE",
    ] {
        if let Some(value) = lookup(key) {
            environment.insert(key.into(), value);
        }
    }
    environment.insert("RUSTUP_AUTO_INSTALL".into(), "0".into());
    environment.insert("CARGO_CACHE_RUSTC_INFO".into(), "0".into());
    environment
}

/// 只在主入口尚未启动原生库或其他线程时冻结环境；库测试不修改全局环境。
pub fn isolate_environment() {
    let environment = environment(|key| std::env::var_os(key));
    for (key, _) in std::env::vars_os() {
        if !environment.contains_key(&key) {
            // SAFETY: main 的第一步，尚未建立线程或进入原生加载，后续代码只读环境。
            unsafe {
                std::env::remove_var(key);
            }
        }
    }
    for (key, value) in environment {
        // SAFETY: 同上，冻结操作在启动任何原生线程之前完成。
        unsafe {
            std::env::set_var(key, value);
        }
    }
}

/// 公开选项映射为内部加载政策，实际 Cargo/sysroot/rust-src 由同进程发现。
pub fn analyze(input: &CheckedRequest) -> anyhow::Result<CodeGraph> {
    if let Some(path) = std::env::var_os(codegraph_engine_common::observation::VARIABLE) {
        codegraph_engine_common::observation::destination(
            std::path::Path::new(&path),
            &input.repository,
            std::slice::from_ref(&input.installation),
        )?;
    }
    let dependency_scope = match input.request.options.rust_dependency_scope {
        RustDependencyScope::RepositoryOnly => DependencyScope::RepositoryOnly,
        RustDependencyScope::StandardLibrary => DependencyScope::StandardLibrary,
        RustDependencyScope::AllDependencies => DependencyScope::AllDependencies,
    };
    let context = CompleteAnalysisInput {
        repository_root: input.repository.clone(),
        files: input.request.files.clone(),
        include_tests: input.request.options.include_tests,
        dependency_scope,
        platforms: vec![],
        observation: input
            .request
            .files
            .is_none()
            .then(|| {
                codegraph_engine_common::observation::Observation::from_environment(
                    "codegraph-rust",
                    &input.request.id,
                )
            })
            .flatten(),
    };
    graph::finish(crate::projection::build(&context)?, input, "rust", &[])
}

#[cfg(test)]
#[path = "tests/runtime.rs"]
mod tests;

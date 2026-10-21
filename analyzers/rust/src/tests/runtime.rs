use super::*;

/// 工具链与显式网络政策保留，编译器包装器及项目参数不能混入原生工具环境。
#[test]
fn rust_tool_environment_keeps_network_policy_and_blocks_injection() {
    let values = [
        ("RUSTUP_TOOLCHAIN", "existing"),
        ("CARGO_NET_OFFLINE", "true"),
        ("CARGO_HTTP_PROXY", "proxy"),
        ("RUSTC_WRAPPER", "unsafe"),
        ("RUSTFLAGS", "unsafe"),
    ];
    let output = environment(|key| {
        values
            .iter()
            .find(|(name, _)| *name == key)
            .map(|(_, value)| (*value).into())
    });
    assert_eq!(
        output
            .get(std::ffi::OsStr::new("RUSTUP_TOOLCHAIN"))
            .unwrap(),
        "existing"
    );
    assert_eq!(
        output
            .get(std::ffi::OsStr::new("CARGO_NET_OFFLINE"))
            .unwrap(),
        "true"
    );
    assert_eq!(
        output
            .get(std::ffi::OsStr::new("RUSTUP_AUTO_INSTALL"))
            .unwrap(),
        "0"
    );
    assert!(!output.contains_key(std::ffi::OsStr::new("RUSTC_WRAPPER")));
    assert!(!output.contains_key(std::ffi::OsStr::new("RUSTFLAGS")));
}

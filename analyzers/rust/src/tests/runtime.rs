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

/// 公共适配器区分省略与 null，原生输入无需再经过私有 JSON 解码。
#[test]
fn files_omission_empty_and_null_have_distinct_meanings() {
    let mut input = serde_json::json!({
        "protocol":"codegraph.analyzer/1", "type":"analyze", "id":"selection",
        "repository_root":std::env::temp_dir(),
        "options":{"include_tests":false,"rust_dependency_scope":"repository-only"}
    });
    let decoded =
        codegraph_engine_common::public_protocol::decode(input.to_string().as_bytes()).unwrap();
    assert!(decoded.files.is_none());
    input["files"] = serde_json::json!([]);
    let decoded =
        codegraph_engine_common::public_protocol::decode(input.to_string().as_bytes()).unwrap();
    assert_eq!(decoded.files, Some(Vec::new()));
    for invalid in [
        serde_json::Value::Null,
        serde_json::json!({}),
        serde_json::json!("a.rs"),
        serde_json::json!([null]),
    ] {
        input["files"] = invalid;
        assert!(
            codegraph_engine_common::public_protocol::decode(input.to_string().as_bytes()).is_err()
        );
    }
}

use super::*;
use serde_json::json;

/// 合法空选集仍携带全部选项，三个 Rust 范围都保留原语义。
#[test]
fn empty_selection_and_all_rust_scopes_decode() {
    for scope in ["repository-only", "standard-library", "all-dependencies"] {
        let value = json!({"protocol":PROTOCOL,"type":"analyze","id":"one","repository_root":std::env::temp_dir(),"files":[],"options":{"include_tests":false,"rust_dependency_scope":scope}});
        assert!(matches!(
            decode(&serde_json::to_vec(&value).unwrap()).unwrap(),
            Message::Analyze(_)
        ));
    }
}

/// 省略选集请求语言配置发现，null 和错误数组类型不能混成同一模式。
#[test]
fn omitted_selection_decodes_but_null_and_malformed_selections_fail() {
    let mut value = json!({"protocol":PROTOCOL,"type":"analyze","id":"one","repository_root":std::env::temp_dir(),"options":{"include_tests":false,"rust_dependency_scope":"repository-only"}});
    assert!(matches!(
        decode(&serde_json::to_vec(&value).unwrap()).unwrap(),
        Message::Analyze(_)
    ));
    for files in [
        Value::Null,
        json!("main.rs"),
        json!({}),
        json!([1]),
        json!([null]),
    ] {
        value["files"] = files;
        assert!(decode(&serde_json::to_vec(&value).unwrap()).is_err());
    }
}

/// 递归拒绝额外字段和重复键，不能允许 options 隐藏环境注入。
#[test]
fn rejects_duplicate_unknown_and_wrongly_typed_fields() {
    for bytes in [
        r#"{"protocol":"codegraph.analyzer/1","type":"cancel","id":"x","id":"x"}"#,
        r#"{"protocol":"codegraph.analyzer/1","type":"cancel","id":"x","environment":{}}"#,
        r#"{"protocol":"codegraph.analyzer/1","type":"cancel","id":1}"#,
        r#"{"protocol":"codegraph.analyzer/1","type":"cancel","id":""}"#,
        r#"{"protocol":"codegraph.analyzer/1","type":"cancel","id":"x","hidden":{"nested":{"a":1,"a":2}}}"#,
    ] {
        assert!(decode(bytes.as_bytes()).is_err(), "{bytes}");
    }
    let value = json!({"protocol":PROTOCOL,"type":"analyze","id":"one","repository_root":std::env::temp_dir(),"files":[],"options":{"include_tests":false,"rust_dependency_scope":"repository-only","command":"evil"}});
    assert!(decode(&serde_json::to_vec(&value).unwrap()).is_err());
    let value = serde_json::to_string(&value)
        .unwrap()
        .replace("\"command\":\"evil\",", "")
        .replace(
            "\"include_tests\":false",
            "\"include_tests\":false,\"include_tests\":true",
        );
    assert!(decode(value.as_bytes()).is_err());
}

/// 协议不匹配有独立类别，截断、非法路径和重复选集不能进入分析。
#[test]
fn rejects_version_truncation_and_paths() {
    assert_eq!(
        decode(br#"{"protocol":"codegraph.analyzer/2","type":"cancel","id":"x"}"#)
            .err()
            .unwrap()
            .code,
        "incompatible_protocol"
    );
    assert!(decode(br#"{"protocol":"#).is_err());
    for files in [
        vec!["../escape"],
        vec!["/escape"],
        vec!["a\\b"],
        vec!["a", "a"],
        vec!["a//b"],
        vec!["C:/x"],
    ] {
        let value = json!({"protocol":PROTOCOL,"type":"analyze","id":"one","repository_root":std::env::temp_dir(),"files":files,"options":{"include_tests":false,"rust_dependency_scope":"standard-library"}});
        assert!(decode(&serde_json::to_vec(&value).unwrap()).is_err());
    }
}

use super::*;

/// 私有输入和公开输入都区分省略与 null，不能靠宽松 Option 解码接受非法选集。
#[test]
fn files_omission_empty_and_null_have_distinct_meanings() {
    let mut input = serde_json::json!({"repository_root":std::env::temp_dir(),"include_tests":false,"profile":"rust-repository-v1"});
    let decoded: Input = serde_json::from_value(input.clone()).unwrap();
    assert!(decoded.files.is_none());
    input["files"] = serde_json::json!([]);
    let decoded: Input = serde_json::from_value(input.clone()).unwrap();
    assert_eq!(decoded.files, Some(Vec::new()));
    for invalid in [
        serde_json::Value::Null,
        serde_json::json!({}),
        serde_json::json!("a.rs"),
        serde_json::json!([null]),
    ] {
        input["files"] = invalid;
        assert!(serde_json::from_value::<Input>(input.clone()).is_err());
    }
}

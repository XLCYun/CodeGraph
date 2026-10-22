use crate::public_protocol::{PROTOCOL, decode};
use serde_json::json;

/// 公共一次性请求保留省略、空集和拒绝 null 的既定选集含义。
#[test]
fn public_selection_and_recursive_duplicate_keys_are_strict() {
    let value = json!({"protocol": PROTOCOL, "type":"analyze", "id":"request", "repository_root":std::env::temp_dir(), "options":{"include_tests":false,"rust_dependency_scope":"repository-only"}});
    let bytes = serde_json::to_vec(&value).unwrap();
    assert!(decode(&bytes).unwrap().files.is_none());
    let mut empty = value.clone();
    empty["files"] = json!([]);
    assert_eq!(
        decode(&serde_json::to_vec(&empty).unwrap()).unwrap().files,
        Some(vec![])
    );
    empty["files"] = json!(null);
    assert!(decode(&serde_json::to_vec(&empty).unwrap()).is_err());
    let duplicate = String::from_utf8(bytes).unwrap().replace(
        "\"include_tests\":false",
        "\"include_tests\":true,\"include_tests\":false",
    );
    assert!(decode(duplicate.as_bytes()).is_err());
}

/// 来源和安装边界对空集同样生效，真实文件与链接都不能越出源码根。
#[test]
fn repository_and_package_boundaries_apply_before_the_empty_fast_path() {
    use crate::public_protocol::check;
    let repository = tempfile::tempdir().unwrap();
    let package = tempfile::tempdir().unwrap();
    std::fs::create_dir(package.path().join("bin")).unwrap();
    let executable = package.path().join("bin/codegraph-rust");
    std::fs::write(&executable, "fixture").unwrap();
    let mut request = json!({"protocol":PROTOCOL,"type":"analyze","id":"paths","repository_root":repository.path(),"files":[],"options":{"include_tests":false,"rust_dependency_scope":"repository-only"}});
    check(decode(request.to_string().as_bytes()).unwrap(), &executable).unwrap();
    request["repository_root"] = json!(package.path());
    assert_eq!(
        check(decode(request.to_string().as_bytes()).unwrap(), &executable)
            .unwrap_err()
            .code,
        "invalid_input"
    );
    #[cfg(unix)]
    {
        use crate::public_protocol::repository_file;
        let outside = tempfile::tempdir().unwrap();
        std::fs::write(outside.path().join("source.rs"), "").unwrap();
        std::os::unix::fs::symlink(
            outside.path().join("source.rs"),
            repository.path().join("linked.rs"),
        )
        .unwrap();
        assert!(repository_file(&repository.path().canonicalize().unwrap(), "linked.rs").is_err());
    }
}

/// JSON 嵌套和 UTF-8 解码有界，异常数据不能被当成有效请求。
#[test]
fn json_nesting_and_utf8_are_bounded() {
    assert!(decode(format!("{}0{}", "[".repeat(256), "]".repeat(256)).as_bytes()).is_err());
    assert!(decode(&[0xff]).is_err());
}

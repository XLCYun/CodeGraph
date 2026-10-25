use super::decode_final;
use serde_json::{json, Value};

/// 构造本平台的退出 1 状态，仅测试最终响应验证。
fn failed_status() -> std::process::ExitStatus {
    #[cfg(unix)]
    {
        use std::os::unix::process::ExitStatusExt;
        std::process::ExitStatus::from_raw(256)
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::ExitStatusExt;
        std::process::ExitStatus::from_raw(1)
    }
}

/// 错误也必须经过同一个最终身份校验入口。
fn failure(
    code: &str,
    id: Value,
    request: Value,
) -> Result<crate::code_graph::CodeGraph, crate::code_graph::AnalysisError> {
    let value = json!({"protocol":"codegraph.analyzer/1","engine_version":"0.1.1","pack_id":"python","type":"completed","id":id,"outcome":"error","error":{"code":code,"message":"backend failure"}});
    let bytes = format!("{value}\n");
    decode_final(
        bytes.as_bytes(),
        failed_status(),
        b"path escape diagnostic",
        "python",
        &request,
    )
}

/// 已接受分析的外来或 null ID 必须先触发身份断言，不能降格成普通分析错误。
#[test]
fn accepted_errors_reject_foreign_or_missing_request_ids() {
    for id in [json!("foreign"), Value::Null] {
        assert!(std::panic::catch_unwind(|| failure(
            "analysis_failed",
            id,
            json!({"id":"public-integration"})
        ))
        .is_err());
    }
    let error = failure(
        "analysis_failed",
        json!("public-integration"),
        json!({"id":"public-integration"}),
    )
    .unwrap_err();
    assert_eq!(error.code.as_deref(), Some("analysis_failed"));
    assert_eq!(error.message, "backend failure");
    assert_eq!(error.diagnostic, "path escape diagnostic");
    assert!(!error.cancelled);
}

/// 拒收封套只允许 null 或独立合法且完全相同的请求 ID。
#[test]
fn early_errors_preserve_the_narrow_null_or_exact_valid_id_exception() {
    for code in ["invalid_request", "incompatible_protocol"] {
        for id in [Value::Null, json!("public-integration")] {
            assert_eq!(
                failure(code, id, json!({"id":"public-integration"}))
                    .unwrap_err()
                    .code
                    .as_deref(),
                Some(code)
            );
        }
        assert!(std::panic::catch_unwind(|| failure(
            code,
            json!("foreign"),
            json!({"id":"public-integration"})
        ))
        .is_err());
        for id in [
            json!(""),
            json!(" \t"),
            json!("\u{0085}"),
            json!("\u{feff}"),
            json!("\0"),
            json!("x".repeat(1025)),
            json!(7),
            json!([]),
        ] {
            assert!(
                std::panic::catch_unwind(|| failure(code, id.clone(), json!({"id":id}))).is_err()
            );
        }
        assert_eq!(
            failure(code, Value::Null, json!({"id":7}))
                .unwrap_err()
                .code
                .as_deref(),
            Some(code)
        );
    }
}

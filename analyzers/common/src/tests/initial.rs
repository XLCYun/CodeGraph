//! 两个真实原生语言程序共用公共协议样本，不启用旧私有消息入口。
use serde_json::{Value, json};
use std::{
    io::Write,
    process::{Command, Output, Stdio},
};

/// 编译到实际公开入口，身份来自构建目标而不是请求或可执行文件改名。
fn identity() -> (&'static str, &'static str) {
    match (
        option_env!("CARGO_BIN_EXE_codegraph-python"),
        option_env!("CARGO_BIN_EXE_codegraph-rust"),
    ) {
        (Some(path), _) => (path, "python"),
        (None, Some(path)) => (path, "rust"),
        (None, None) => panic!("公共协议测试必须绑定一个真实语言入口"),
    }
}

/// 真实 stdin 写完即关闭，普通 EOF 必须触发唯一最终结果。
fn run(bytes: &[u8]) -> Output {
    run_project(bytes, true)
}

/// 原生项目验证使用调用方工具环境，空集反例另用空 PATH。
fn run_project(bytes: &[u8], empty_path: bool) -> Output {
    let mut command = Command::new(identity().0);
    command.arg("analyze");
    if empty_path {
        command.env("PATH", "");
    }
    let mut child = command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(bytes).unwrap();
    child.wait_with_output().unwrap()
}

/// 共享的代表性反例在两个固定语言入口都保持相同退出、身份和无部分图语义。
#[test]
fn shared_public_conformance_vectors() {
    let root = tempfile::tempdir().unwrap();
    // 显式空集不得读取无效配置或发现任何 SDK。
    std::fs::write(root.path().join("Cargo.toml"), "invalid [").unwrap();
    std::fs::write(root.path().join("ty.toml"), "invalid [").unwrap();
    let base = json!({"protocol":"codegraph.analyzer/1","type":"analyze","id":"conformance","repository_root":root.path(),"files":[],"options":{"include_tests":false,"rust_dependency_scope":"repository-only"}});
    let cases: Vec<Value> = serde_json::from_str(include_str!("public-conformance.json")).unwrap();
    for case in cases {
        let mut request = base.clone();
        request
            .as_object_mut()
            .unwrap()
            .extend(case["patch"].as_object().unwrap().clone());
        let bytes = format!("{}{}", request, case["suffix"].as_str().unwrap());
        let output = run(bytes.as_bytes());
        let value: Value = serde_json::from_slice(&output.stdout)
            .unwrap_or_else(|_| panic!("{}: {:?}", case["name"], output));
        assert_eq!(
            output.stdout.iter().filter(|b| **b == b'\n').count(),
            1,
            "{}",
            case["name"]
        );
        assert_eq!(value["protocol"], "codegraph.analyzer/1");
        assert_eq!(value["engine_version"], "0.1.0");
        assert_eq!(value["pack_id"], identity().1);
        assert_eq!(value["type"], "completed");
        assert_eq!(value["outcome"], case["outcome"], "{}", case["name"]);
        assert!(value.get("evidence").is_none());
        if case["outcome"] == "ok" {
            assert!(output.status.success(), "{}", case["name"]);
            assert_eq!(value["id"], "conformance");
            assert_eq!(
                value["graph"],
                json!({"root_contains":[],"nodes":[],"external_symbols":[]})
            );
        } else {
            assert_eq!(output.status.code(), Some(1));
            assert_eq!(
                value["error"]["code"], case["error_code"],
                "{}",
                case["name"]
            );
            assert!(value.get("graph").is_none());
        }
    }
}

/// 递归重复键、非 JSON、空输入和私有上下文均不能伪造有效请求身份。
#[test]
fn malformed_requests_and_private_envelopes_are_rejected() {
    for bytes in [b"".as_slice(), b"not JSON", br#"{"protocol":"codegraph.analyzer/1","id":"a","id":"b"}"#,
        br#"{"protocol":"codegraph.analyzer/1","type":"analyze","id":"a","input":{},"environment":{},"limits":{}}"#,
        br#"{"protocol":"codegraph.analyzer/1","type":"analyze","id":"a","repository_root":"/","options":{"include_tests":true,"include_tests":false,"rust_dependency_scope":"repository-only"}}"#] {
        let output = run(bytes);
        let value: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(output.status.code(), Some(1));
        assert_eq!(value["id"], Value::Null);
        assert_eq!(value["pack_id"], identity().1);
        assert_eq!(value["outcome"], "error");
        assert!(value.get("graph").is_none());
    }
}

/// CLI 帮助和版本只输出文本；旧 selector、worker 和重复参数均属于用法错误。
#[test]
fn fixed_cli_rejects_selectors_workers_and_repeated_arguments() {
    for args in [vec!["--help"], vec!["analyze", "--help"], vec!["--version"]] {
        let output = Command::new(identity().0).args(args).output().unwrap();
        assert!(output.status.success());
        assert!(
            String::from_utf8(output.stdout)
                .unwrap()
                .contains(&format!("codegraph-{}", identity().1))
        );
    }
    for args in [
        vec![],
        vec!["__worker"],
        vec!["--pack", "rust", "analyze"],
        vec!["analyze", "analyze"],
        vec!["--help", "--help"],
        vec!["analyze", "--version"],
    ] {
        let output = Command::new(identity().0).args(args).output().unwrap();
        assert_eq!(output.status.code(), Some(2));
        assert!(output.stdout.is_empty());
    }
}

/// 真实非空配置发现与精确选集都从公共入口完成，源码与配置不被写回。
#[test]
fn real_project_success_and_handled_failure_have_only_public_final_data() {
    let root = tempfile::tempdir().unwrap();
    let (source, content) = if identity().1 == "rust" {
        std::fs::write(root.path().join("Cargo.toml"), "[package]\nname='public_fixture'\nversion='0.1.0'\nedition='2021'\n[lib]\npath='lib.rs'\n").unwrap();
        (
            "lib.rs",
            "pub fn helper() -> usize { 1 }\npub fn run() -> usize { helper() }\n",
        )
    } else {
        std::fs::write(
            root.path().join("ty.toml"),
            "[environment]\npython-version='3.11'\n",
        )
        .unwrap();
        (
            "main.py",
            "def helper():\n    return 1\ndef run():\n    return helper()\n",
        )
    };
    std::fs::write(root.path().join(source), content).unwrap();
    let before: std::collections::BTreeMap<_, _> = std::fs::read_dir(root.path())
        .unwrap()
        .map(|entry| {
            let path = entry.unwrap().path();
            (path.clone(), std::fs::read(path).unwrap())
        })
        .collect();
    let mut request = json!({"protocol":"codegraph.analyzer/1","type":"analyze","id":"real-project","repository_root":root.path(),"options":{"include_tests":false,"rust_dependency_scope":"repository-only"}});
    for explicit in [false, true] {
        if explicit {
            request["files"] = json!([source]);
        }
        let output = run_project(request.to_string().as_bytes(), false);
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let result: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(result["id"], "real-project");
        assert_eq!(result["pack_id"], identity().1);
        assert_eq!(result["outcome"], "ok");
        assert!(result.get("evidence").is_none());
        assert!(
            result["graph"]["nodes"]
                .as_array()
                .unwrap()
                .iter()
                .any(|node| node["name"] == "run")
        );
        let after: std::collections::BTreeMap<_, _> = std::fs::read_dir(root.path())
            .unwrap()
            .map(|entry| {
                let path = entry.unwrap().path();
                (path.clone(), std::fs::read(path).unwrap())
            })
            .collect();
        assert_eq!(before, after, "分析改写了源码目录");
    }
    std::fs::write(root.path().join(source), "invalid source {").unwrap();
    let output = run_project(request.to_string().as_bytes(), false);
    assert_eq!(output.status.code(), Some(1));
    let result: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(result["outcome"], "error");
    assert_eq!(result["error"]["code"], "analysis_failed");
    assert!(result.get("graph").is_none());
}

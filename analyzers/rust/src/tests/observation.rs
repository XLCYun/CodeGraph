//! 真实公共入口将请求身份保留在发现观察中，精确选集与空集不写观察标记。
use serde_json::{Value, json};
use std::{
    fs,
    io::Write,
    process::{Command, Stdio},
};

/// 观察对象迁到适配器后，仍只在配置发现阶段记录真实请求身份和加载里程碑。
#[test]
fn public_discovery_observation_preserves_request_identity_and_selection_mode() {
    let repository = tempfile::tempdir().unwrap();
    fs::write(
        repository.path().join("Cargo.toml"),
        "[package]\nname='observation'\nversion='0.1.0'\nedition='2021'\n[lib]\npath='lib.rs'\n",
    )
    .unwrap();
    fs::write(repository.path().join("lib.rs"), "pub fn run() {}\n").unwrap();
    let before =
        ["Cargo.toml", "lib.rs"].map(|name| fs::read(repository.path().join(name)).unwrap());
    for files in [None, Some(vec!["lib.rs"]), Some(vec![])] {
        let tools = tempfile::Builder::new()
            .prefix("codegraph-discovery-probe-")
            .tempdir()
            .unwrap();
        let marker = tools.path().join("reached.json");
        let mut request = json!({
            "protocol":"codegraph.analyzer/1", "type":"analyze", "id":"observation-request",
            "repository_root":repository.path(),
            "options":{"include_tests":false,"rust_dependency_scope":"repository-only"}
        });
        if let Some(files) = &files {
            request["files"] = json!(files);
        }
        let mut child = Command::new(env!("CARGO_BIN_EXE_codegraph-rust"))
            .arg("analyze")
            .env("CODEGRAPH_TEST_DISCOVERY_PROBE", &marker)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let pid = child.id();
        child
            .stdin
            .take()
            .unwrap()
            .write_all(request.to_string().as_bytes())
            .unwrap();
        let output = child.wait_with_output().unwrap();
        assert!(output.status.success(), "{output:?}");
        let result: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(result["id"], "observation-request");
        assert_eq!(result["outcome"], "ok");
        assert!(result.get("evidence").is_none());
        if files.is_none() {
            let observation: Value = serde_json::from_slice(&fs::read(&marker).unwrap()).unwrap();
            assert_eq!(observation["schema_version"], 1);
            assert_eq!(observation["helper_id"], "codegraph-rust");
            assert_eq!(observation["request_id"], "observation-request");
            assert_eq!(observation["pid"], pid);
            assert_eq!(observation["milestone"], "workspace-metadata-loaded");
        } else {
            assert!(!marker.exists(), "精确选集和空集不能写入发现标记");
            assert_eq!(fs::read_dir(tools.path()).unwrap().count(), 0);
        }
        let after =
            ["Cargo.toml", "lib.rs"].map(|name| fs::read(repository.path().join(name)).unwrap());
        assert_eq!(after, before);
        assert_eq!(fs::read_dir(repository.path()).unwrap().count(), 2);
    }
}

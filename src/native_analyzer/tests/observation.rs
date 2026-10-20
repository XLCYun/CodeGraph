use super::*;

/// 观察目的地只能位于独立的新临时目录，不能接受旧标记、项目或包树。
#[test]
fn destination_preserves_input_and_existing_paths() {
    let root = tempfile::tempdir().unwrap();
    let repository = root.path().join("repository");
    let package = root.path().join("package");
    fs::create_dir(&repository).unwrap();
    fs::create_dir(&package).unwrap();
    let probe = tempfile::Builder::new()
        .prefix("codegraph-discovery-probe-")
        .tempdir()
        .unwrap();
    let target = probe.path().join("reached.json");
    assert_eq!(
        destination(&target, &repository, &[package.clone()]).unwrap(),
        probe.path().canonicalize().unwrap().join("reached.json")
    );
    fs::write(&target, "original").unwrap();
    assert!(destination(&target, &repository, &[package.clone()]).is_err());
    assert_eq!(fs::read_to_string(&target).unwrap(), "original");
    assert!(destination(Path::new("reached.json"), &repository, &[package.clone()]).is_err());
    for protected in [&repository, &package] {
        let nested = protected.join("codegraph-discovery-probe-invalid");
        fs::create_dir(&nested).unwrap();
        let inside = destination(
            &nested.join("reached.json"),
            &repository,
            &[package.clone()],
        );
        assert!(inside.is_err());
    }
}

/// 原子观察写入固定身份且只能一次，已有文件即使发生在 Host 校验后也不能被覆盖。
#[test]
fn marker_is_bounded_atomic_and_never_overwrites() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("reached.json");
    let observation = Observation {
        path: path.clone(),
        helper: "codegraph-rust-engine".into(),
        request: "analysis-1".into(),
        recorded: Cell::new(false),
    };
    observation.record("workspace-metadata-loaded").unwrap();
    let bytes = fs::read(&path).unwrap();
    let value: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(value["pid"], std::process::id());
    assert_eq!(value["request_id"], "analysis-1");
    assert_eq!(value["helper_id"], "codegraph-rust-engine");
    observation.record("another").unwrap();
    assert_eq!(fs::read(&path).unwrap(), bytes);
    let duplicate = Observation {
        recorded: Cell::new(false),
        ..observation
    };
    assert!(duplicate.record("replacement").is_err());
    assert_eq!(fs::read(&path).unwrap(), bytes);
    assert!(!path.with_extension("pending").exists());
    let oversized = Observation {
        path: directory.path().join("oversized.json"),
        helper: "x".repeat(4096),
        request: "analysis-1".into(),
        recorded: Cell::new(false),
    };
    assert!(oversized.record("loaded").is_err());
    assert!(!oversized.path.exists());
}

/// 已有符号链接和指向受保护目录的父链接不能把验收写入重定向到源码或包。
#[test]
#[cfg(unix)]
fn symlink_destinations_and_parents_are_rejected() {
    use std::os::unix::fs::symlink;
    let root = tempfile::tempdir().unwrap();
    let repository = root.path().join("repository");
    fs::create_dir(&repository).unwrap();
    let probe = tempfile::Builder::new()
        .prefix("codegraph-discovery-probe-")
        .tempdir()
        .unwrap();
    let target = probe.path().join("reached.json");
    symlink(repository.join("missing"), &target).unwrap();
    assert!(destination(&target, &repository, &[]).is_err());
    let link = root.path().join("codegraph-discovery-probe-link");
    symlink(&repository, &link).unwrap();
    assert!(destination(&link.join("reached.json"), &repository, &[]).is_err());
    let nested = repository.join("codegraph-discovery-probe-nested");
    fs::create_dir(&nested).unwrap();
    let redirected = destination(
        &link.join("codegraph-discovery-probe-nested/reached.json"),
        &repository,
        &[],
    );
    assert!(redirected.is_err());
    let writer = Observation {
        path: target,
        helper: "engine".into(),
        request: "request".into(),
        recorded: Cell::new(false),
    };
    assert!(writer.record("loaded").is_err());
    assert!(!repository.join("missing").exists());
}

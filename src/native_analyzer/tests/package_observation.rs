//! 使用生产 Python 适配器区分完整安装包边界与更窄的可执行文件信任目录。
use super::*;
use crate::adapters::{Adapter, Python};

/// 只准备生产工厂必需的包控制与固定 helper 路径，不运行替身引擎。
struct PythonPackage {
    root: tempfile::TempDir,
    repository: tempfile::TempDir,
    analyzer: NativeLanguageAnalyzer,
}

impl PythonPackage {
    /// 包与输入分别位于系统临时目录，复现真实临时安装的边界条件。
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        for directory in ["bin", "control"] {
            fs::create_dir(root.path().join(directory)).unwrap();
        }
        fs::write(
            root.path().join(if cfg!(windows) {
                "bin/codegraph-python-engine.exe"
            } else {
                "bin/codegraph-python-engine"
            }),
            "fixture",
        )
        .unwrap();
        fs::write(root.path().join("control/native.json"), r#"{"schema_version":1,"helper_version":"0.1.0","platforms":[{"id":"python-stdlib","kind":"python-embedded-stdlib","version":"0.0.84","root":"bin","metadata":{"source_revision":"11aaa39f84c0c44bdfb4c9cc21e4038c6f74c00a"}}]}"#).unwrap();
        let analyzer = Python.engine(root.path()).unwrap();
        assert_eq!(
            analyzer.command.trusted_roots,
            vec![root.path().join("bin").canonicalize().unwrap()],
            "完整包保护不能扩大 Python 可执行文件信任目录"
        );
        Self {
            root,
            repository: tempfile::tempdir().unwrap(),
            analyzer,
        }
    }
}

/// 生产适配器提供的边界必须覆盖 bin 之外的包目录，拒绝后不能创建任何观察文件。
#[test]
fn python_adapter_protects_entire_package_from_observation() {
    let fixture = PythonPackage::new();
    let directory = fixture
        .root
        .path()
        .join("codegraph-discovery-probe-inside-package");
    fs::create_dir(&directory).unwrap();
    let target = directory.join("reached.json");
    let result = fixture
        .analyzer
        .observation_destination(&target, fixture.repository.path());
    assert!(result.is_err(), "包内观察位置被错误批准：{result:?}");
    assert_eq!(fs::read_dir(directory).unwrap().count(), 0);
}

/// 生产适配器仍接受独立 scratch，且旧目标和输入内位置保持拒绝。
#[test]
fn python_adapter_observation_accepts_only_independent_scratch() {
    let fixture = PythonPackage::new();
    let probe = tempfile::Builder::new()
        .prefix("codegraph-discovery-probe-")
        .tempdir()
        .unwrap();
    let target = probe.path().join("reached.json");
    assert_eq!(
        fixture
            .analyzer
            .observation_destination(&target, fixture.repository.path())
            .unwrap(),
        probe.path().canonicalize().unwrap().join("reached.json")
    );
    fs::write(&target, "original").unwrap();
    assert!(fixture
        .analyzer
        .observation_destination(&target, fixture.repository.path())
        .is_err());
    assert_eq!(fs::read_to_string(&target).unwrap(), "original");
    let nested = fixture
        .repository
        .path()
        .join("codegraph-discovery-probe-in-input");
    fs::create_dir(&nested).unwrap();
    assert!(fixture
        .analyzer
        .observation_destination(&nested.join("reached.json"), fixture.repository.path())
        .is_err());
    assert_eq!(fs::read_dir(nested).unwrap().count(), 0);
}

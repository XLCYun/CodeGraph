use super::*;

/// 入口位置约束不依赖当前目录或可执行文件名称。
#[test]
fn executable_parent_defines_package_root() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().canonicalize().unwrap();
    std::fs::create_dir(root.join("bin")).unwrap();
    let executable = root.join("bin/renamed-entry");
    std::fs::write(&executable, "fixture").unwrap();
    assert_eq!(root_from_executable(&executable).unwrap(), root);
    std::fs::write(root.join("loose-entry"), "fixture").unwrap();
    assert!(root_from_executable(&root.join("loose-entry")).is_err());
    assert!(root_from_executable(&root.join("bin")).is_err());
}

/// 包外符号链接入口按真实位置解析，不能借链接旁的控制文件伪装包。
#[cfg(unix)]
#[test]
fn symlink_uses_real_executable_location() {
    let actual = tempfile::tempdir().unwrap();
    let alias = tempfile::tempdir().unwrap();
    std::fs::create_dir(actual.path().join("bin")).unwrap();
    std::fs::write(actual.path().join("bin/entry"), "fixture").unwrap();
    std::os::unix::fs::symlink(actual.path().join("bin/entry"), alias.path().join("entry"))
        .unwrap();
    assert_eq!(
        root_from_executable(&alias.path().join("entry")).unwrap(),
        actual.path().canonicalize().unwrap()
    );
}

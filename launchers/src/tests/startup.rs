use super::*;
use std::{
    fs,
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
};

static NEXT: AtomicU64 = AtomicU64::new(0);

/// 每个用例建立可删除的最小包布局，不依赖个人安装目录。
struct Package(PathBuf);
impl Package {
    /// 准备 Node/JVM 与两个固定资源位置，保留 Unicode 和空格。
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "codegraph 薄入口 {}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        for relative in [
            "bin",
            "runtime/jdk/bin",
            "runtime/analyzer/classes",
            "runtime/analyzer/lib",
        ] {
            fs::create_dir_all(root.join(relative)).unwrap();
        }
        for relative in [
            if cfg!(windows) {
                "bin/node.exe"
            } else {
                "bin/node"
            },
            if cfg!(windows) {
                "runtime/jdk/bin/java.exe"
            } else {
                "runtime/jdk/bin/java"
            },
            "runtime/analyzer/main.mjs",
        ] {
            fs::write(root.join(relative), b"fixture").unwrap();
        }
        Self(dunce::canonicalize(root).unwrap())
    }
}
impl Drop for Package {
    /// 仅删除用例自己的临时目录。
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).unwrap();
    }
}

/// 两个入口固定自己的程序、资源与必要参数，运行时注入环境被清除。
#[test]
fn fixed_runtime_commands_use_analyzer_resources() {
    let package = Package::new();
    let ts = command(&package.0, Language::TypeScriptJavaScript).unwrap();
    assert_eq!(
        Path::new(ts.get_program()),
        package.0.join(if cfg!(windows) {
            "bin/node.exe"
        } else {
            "bin/node"
        })
    );
    assert_eq!(ts.get_args().next().unwrap(), "--disable-proto=throw");
    assert_eq!(
        Path::new(ts.get_args().nth(1).unwrap()),
        package.0.join("runtime/analyzer/main.mjs")
    );
    let java = command(&package.0, Language::Java).unwrap();
    let args = java.get_args().collect::<Vec<_>>();
    assert_eq!(args.last().unwrap(), &"dev.codegraph.java.Main");
    let classpath = std::env::split_paths(args[2]).collect::<Vec<_>>();
    assert_eq!(classpath[0], package.0.join("runtime/analyzer/classes"));
    assert_eq!(classpath[1], package.0.join("runtime/analyzer/lib/*"));
}

/// 缺失资源应成为启动失败，不搜索 PATH 或相邻旧 helper 目录。
#[test]
fn missing_runtime_is_a_startup_error() {
    let package = Package::new();
    fs::remove_file(package.0.join(if cfg!(windows) {
        "bin/node.exe"
    } else {
        "bin/node"
    }))
    .unwrap();
    assert!(command(&package.0, Language::TypeScriptJavaScript).is_err());
}

/// 即使 Node 或 JVM 有外部环境注入，命令仍只继承明确白名单。
#[test]
fn runtime_environment_is_bounded() {
    let package = Package::new();
    for language in [Language::TypeScriptJavaScript, Language::Java] {
        let command = command(&package.0, language).unwrap();
        for (key, _) in command.get_envs() {
            assert!(ENVIRONMENT.iter().any(|allowed| key == *allowed));
        }
        assert!(command
            .get_envs()
            .all(|(key, _)| key != "NODE_OPTIONS" && key != "JAVA_TOOL_OPTIONS"));
    }
}

/// 运行时路径可以包含 Unicode 和空格，但不能链接到另一个安装目录。
#[cfg(unix)]
#[test]
fn resource_symlink_cannot_escape_package() {
    use std::os::unix::fs::symlink;
    let package = Package::new();
    fs::remove_file(package.0.join("bin/node")).unwrap();
    symlink("/bin/sh", package.0.join("bin/node")).unwrap();
    assert!(command(&package.0, Language::TypeScriptJavaScript).is_err());
}

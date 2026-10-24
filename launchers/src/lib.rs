//! 固定语言薄入口，只负责定位随包运行时、继承标准句柄和传播退出。
use std::{
    io,
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

/// 编译期固定语言，不从请求、环境、文件名或参数推断。
#[derive(Clone, Copy)]
pub enum Language {
    /// 随包 Node 与固定 JS 程序。
    TypeScriptJavaScript,
    /// 随包 JVM 与固定 Java 主类。
    Java,
}

/// 仅保留系统运行、临时目录、用户位置与既有 SDK 路径提示。
const ENVIRONMENT: &[&str] = &[
    "SystemRoot",
    "WINDIR",
    "TEMP",
    "TMP",
    "TMPDIR",
    "HOME",
    "USERPROFILE",
    "PATH",
    "ProgramFiles",
    "ProgramFiles(x86)",
    "CODEGRAPH_JAVA_TARGET_JDK",
    "JAVA_HOME",
    "JDK_HOME",
    "CODEGRAPH_TEST_DISCOVERY_PROBE",
];

/// 资源必须属于当前包；不回退到 PATH、旧 helper 或可注入的控制文件。
fn resource(root: &Path, relative: &str) -> io::Result<PathBuf> {
    let path = dunce::canonicalize(root.join(relative))?;
    if !path.starts_with(root) {
        return Err(io::Error::other("随包资源越过安装目录"));
    }
    Ok(path)
}

/// 构造启动命令；不读取 JSON、SDK、配置或 Code Graph。
fn command(root: &Path, language: Language) -> io::Result<Command> {
    let mut command = match language {
        Language::TypeScriptJavaScript => {
            let mut command = Command::new(resource(
                root,
                if cfg!(windows) {
                    "bin/node.exe"
                } else {
                    "bin/node"
                },
            )?);
            command
                .arg("--disable-proto=throw")
                .arg(resource(root, "runtime/analyzer/main.mjs")?);
            command
        }
        Language::Java => {
            let mut command = Command::new(resource(
                root,
                if cfg!(windows) {
                    "runtime/jdk/bin/java.exe"
                } else {
                    "runtime/jdk/bin/java"
                },
            )?);
            let classes = resource(root, "runtime/analyzer/classes")?;
            let libraries = resource(root, "runtime/analyzer/lib")?.join("*");
            let classpath = std::env::join_paths([classes, libraries]).map_err(io::Error::other)?;
            command
                .args(["-Dfile.encoding=UTF-8", "-cp"])
                .arg(classpath)
                .arg("dev.codegraph.java.Main");
            command
        }
    };
    let environment = ENVIRONMENT
        .iter()
        .filter_map(|key| std::env::var_os(key).map(|value| (*key, value)));
    command.env_clear().envs(environment);
    command
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit());
    Ok(command)
}

/// 从真实入口位置定位包根，支持任意 cwd 和指向入口的符号链接。
fn package_root() -> io::Result<PathBuf> {
    let executable = dunce::canonicalize(std::env::current_exe()?)?;
    let bin = executable
        .parent()
        .ok_or_else(|| io::Error::other("入口缺少目录"))?;
    if bin.file_name().is_none_or(|name| name != "bin") {
        return Err(io::Error::other("公开入口必须位于包内 bin 目录"));
    }
    bin.parent()
        .map(Path::to_path_buf)
        .ok_or_else(|| io::Error::other("入口缺少安装根"))
}

/// macOS 使用 exec 保留 PID 和标准句柄；Windows 等待子进程并原样返回退出码。
fn launch(language: Language) -> io::Result<i32> {
    let mut command = command(&package_root()?, language)?;
    command.args(std::env::args_os().skip(1));
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        Err(command.exec())
    }
    #[cfg(windows)]
    {
        Ok(command.status()?.code().unwrap_or(1))
    }
}

/// 启动失败只写 stderr；协议和 CLI 行为全部由实际语言程序实现。
pub fn main(language: Language) -> i32 {
    match launch(language) {
        Ok(status) => status,
        Err(error) => {
            eprintln!("无法启动随包分析程序：{error}");
            1
        }
    }
}

#[cfg(test)]
#[path = "tests/startup.rs"]
mod tests;

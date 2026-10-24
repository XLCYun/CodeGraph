//! 非交付验收调用方：原字节桥接标准管道，独立状态目录保留树归属直到显式回收。
#![allow(dead_code)]
mod child_process;

use std::{
    fs,
    io::{self, Read, Write},
    path::{Path, PathBuf},
    process::{Command, ExitStatus, Stdio},
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

/// 独立状态通道采用先写后改名，不能把半个状态读成完整公共结果。
fn publish(directory: &Path, name: &str, bytes: &[u8]) -> io::Result<()> {
    let pending = directory.join(format!("{name}.pending"));
    fs::write(&pending, bytes)?;
    fs::rename(pending, directory.join(name))
}

/// 根进程真实状态独立于 probe 退出码；公共 stdout 中不增加控制记录。
fn root_status(directory: &Path, status: ExitStatus) -> io::Result<()> {
    #[cfg(unix)]
    let signal = {
        use std::os::unix::process::ExitStatusExt;
        status.signal()
    };
    #[cfg(not(unix))]
    let signal: Option<i32> = None;
    let number = |value: Option<i32>| value.map_or("null".into(), |value| value.to_string());
    publish(
        directory,
        "root-exited.json",
        format!(
            "{{\"code\":{},\"signal\":{}}}",
            number(status.code()),
            number(signal)
        )
        .as_bytes(),
    )
}

/// 字节只向原标准管道转发，EOF 与读写失败走独立状态文件。
fn relay<R: Read, W: Write>(
    mut reader: R,
    mut writer: W,
    directory: &Path,
    name: &str,
) -> io::Result<()> {
    let result = io::copy(&mut reader, &mut writer).and_then(|_| writer.flush());
    if let Err(error) = &result {
        publish(
            directory,
            &format!("{name}-error"),
            error.to_string().as_bytes(),
        )?;
    }
    publish(directory, &format!("{name}-closed"), b"")?;
    result
}

/// 回收后限时排空并 join 三个真实管道，阻塞线程不能伪装为完成。
fn join_pipes(pipes: Vec<(&str, JoinHandle<io::Result<()>>)>) -> io::Result<()> {
    let deadline = Instant::now() + Duration::from_secs(3);
    while pipes.iter().any(|(_, pipe)| !pipe.is_finished()) {
        if Instant::now() >= deadline {
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "owner pipe drain/join timed out",
            ));
        }
        thread::sleep(Duration::from_millis(5));
    }
    for (name, pipe) in pipes {
        match pipe
            .join()
            .map_err(|_| io::Error::other("owner pipe relay panicked"))?
        {
            // 根退出造成 stdin 的 BrokenPipe 由 Node 独立读取，不能掩盖原超时或解析错误。
            Err(error) if name == "stdin" && error.kind() == io::ErrorKind::BrokenPipe => {}
            other => other?,
        }
    }
    Ok(())
}

/// 只在测试调用方的清理命令或自身有界保护触发后释放 retained ProcessTree/Job。
fn run(
    directory: &Path,
    lifetime: Duration,
    executable: PathBuf,
    arguments: Vec<std::ffi::OsString>,
) -> io::Result<()> {
    let mut command = Command::new(executable);
    command
        .args(arguments)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut owner = child_process::ProcessTree::spawn(&mut command)?;
    let input = owner.take_stdin().unwrap();
    let output = owner.take_stdout().unwrap();
    let diagnostic = owner.take_stderr().unwrap();
    let input_directory = directory.to_owned();
    let output_directory = directory.to_owned();
    let error_directory = directory.to_owned();
    let pipes = vec![
        (
            "stdin",
            thread::spawn(move || relay(io::stdin().lock(), input, &input_directory, "stdin")),
        ),
        (
            "stdout",
            thread::spawn(move || relay(output, io::stdout().lock(), &output_directory, "stdout")),
        ),
        (
            "stderr",
            thread::spawn(move || {
                relay(diagnostic, io::stderr().lock(), &error_directory, "stderr")
            }),
        ),
    ];
    let deadline = Instant::now() + lifetime;
    let mut status_published = false;
    let mut failures = Vec::new();
    loop {
        if !status_published {
            if let Some(status) = owner.status()? {
                root_status(directory, status)?;
                status_published = true;
            }
        }
        if directory.join("cleanup").exists() {
            break;
        }
        if Instant::now() >= deadline {
            failures.push("owner lifetime deadline exceeded".to_owned());
            break;
        }
        thread::sleep(Duration::from_millis(5));
    }
    if let Err(error) = owner.reap() {
        failures.push(format!("tree reap: {error}"));
    }
    if !status_published {
        if let Some(status) = owner.status()? {
            root_status(directory, status)?;
        }
    }
    if let Err(error) = join_pipes(pipes) {
        failures.push(error.to_string());
    }
    if !failures.is_empty() {
        return Err(io::Error::other(failures.join("; ")));
    }
    publish(directory, "cleanup-complete", b"")
}

/// 只用于仓内验收；工具自身失败写独立通道，绝不注入分析器 stdout/stderr。
fn main() {
    let mut args = std::env::args_os().skip(1);
    let directory = PathBuf::from(args.next().expect("owner state directory"));
    let lifetime = Duration::from_millis(
        args.next()
            .and_then(|value| value.to_str()?.parse().ok())
            .expect("owner lifetime milliseconds"),
    );
    let executable = PathBuf::from(args.next().expect("owned executable"));
    if let Err(error) = run(&directory, lifetime, executable, args.collect()) {
        let _ = publish(&directory, "cleanup-error", error.to_string().as_bytes());
        std::process::exit(1);
    }
}

//! 一次性协议的生命周期由调用方拥有，EOF 是完整请求的正常结束。
use serde_json::{Value, json};
use std::{
    io::{Read, Write},
    process::{Command, Stdio},
    thread,
    time::Duration,
};

/// 当前 integration target 绑定的真实原生语言程序。
fn executable() -> &'static str {
    option_env!("CARGO_BIN_EXE_codegraph-python")
        .or(option_env!("CARGO_BIN_EXE_codegraph-rust"))
        .unwrap()
}

/// 请求没有 EOF 时不能忽略后续第二 JSON 提前分析。
#[test]
fn waits_for_eof_then_rejects_a_second_request() {
    let root = tempfile::tempdir().unwrap();
    let mut child = Command::new(executable())
        .arg("analyze")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let mut input = child.stdin.take().unwrap();
    let request = json!({"protocol":"codegraph.analyzer/1","type":"analyze","id":"lifecycle","repository_root":root.path(),"files":[],"options":{"include_tests":false,"rust_dependency_scope":"repository-only"}});
    writeln!(input, "{request}").unwrap();
    input.flush().unwrap();
    thread::sleep(Duration::from_millis(100));
    assert!(child.try_wait().unwrap().is_none(), "未等 EOF 就提前完成");
    writeln!(input, "{{}}").unwrap();
    drop(input);
    let output = child.wait_with_output().unwrap();
    assert_eq!(output.status.code(), Some(1));
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["error"]["code"], "invalid_request");
    assert!(value.get("graph").is_none());
}

/// 调用方终止尚未完成输入的进程，不需要另发 cancel JSON 或等待控制线程。
#[test]
fn caller_can_terminate_an_input_wait_without_terminal_json() {
    let mut child = Command::new(executable())
        .arg("analyze")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let input = child.stdin.take().unwrap();
    child.kill().unwrap();
    assert!(!child.wait().unwrap().success());
    drop(input);
    let mut output = String::new();
    child
        .stdout
        .take()
        .unwrap()
        .read_to_string(&mut output)
        .unwrap();
    assert!(output.is_empty());
}

/// 调用方给分析器建立自己的进程组，EOF 后真实工具查询仍运行，再由调用方终止整组。
#[cfg(unix)]
#[test]
fn caller_termination_owns_active_native_analysis_and_tool_descendants() {
    use std::{
        fs,
        os::unix::{fs::PermissionsExt, process::CommandExt},
        time::Instant,
    };
    let repository = tempfile::tempdir().unwrap();
    let tools = tempfile::tempdir().unwrap();
    let rust = option_env!("CARGO_BIN_EXE_codegraph-rust").is_some();
    let marker = tools.path().join("active");
    if rust {
        fs::write(
            repository.path().join("Cargo.toml"),
            "[package]\nname='lifecycle'\nversion='0.1.0'\nedition='2021'\n[lib]\npath='lib.rs'\n",
        )
        .unwrap();
        fs::write(repository.path().join("lib.rs"), "pub fn run() {}\n").unwrap();
        let cargo = tools.path().join("cargo");
        fs::write(
            &cargo,
            format!(
                "#!/bin/sh\nprintf '%s %s' \"$$\" \"$PPID\" > '{}'\nexec /bin/sleep 30\n",
                marker.display()
            ),
        )
        .unwrap();
        fs::set_permissions(&cargo, fs::Permissions::from_mode(0o755)).unwrap();
    } else {
        fs::write(
            repository.path().join("ty.toml"),
            "[environment]\npython-version='3.11'\n",
        )
        .unwrap();
        let source: String = (0..15000)
            .map(|i| format!("def value_{i}(): return len([])\n"))
            .collect();
        fs::write(repository.path().join("main.py"), source).unwrap();
    }
    let mut command = Command::new(executable());
    command
        .arg("analyze")
        .process_group(0)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    if rust {
        let mut paths = vec![tools.path().to_owned()];
        paths.extend(std::env::split_paths(
            &std::env::var_os("PATH").unwrap_or_default(),
        ));
        command.env("PATH", std::env::join_paths(paths).unwrap());
    }
    let mut child = command.spawn().unwrap();
    let request = json!({"protocol":"codegraph.analyzer/1","type":"analyze","id":"active","repository_root":repository.path(),"options":{"include_tests":false,"rust_dependency_scope":"repository-only"}});
    child
        .stdin
        .take()
        .unwrap()
        .write_all(request.to_string().as_bytes())
        .unwrap();
    let mut descendants = Vec::new();
    if rust {
        let start = Instant::now();
        loop {
            if let Ok(text) = fs::read_to_string(&marker) {
                let ids: Vec<i32> = text
                    .split_whitespace()
                    .filter_map(|s| s.parse().ok())
                    .collect();
                if ids.len() == 2 {
                    assert_eq!(
                        ids[1],
                        child.id() as i32,
                        "真实 Cargo 查询不能绕经分析 worker"
                    );
                    descendants.push(ids[0]);
                    break;
                }
            }
            if start.elapsed() > Duration::from_secs(10) {
                let _ = child.kill();
                let _ = child.wait();
                panic!("EOF 后没有启动真实 Cargo 查询");
            }
            thread::sleep(Duration::from_millis(5));
        }
    } else {
        thread::sleep(Duration::from_millis(150));
    }
    assert!(
        child.try_wait().unwrap().is_none(),
        "活动 fixture 已意外完成"
    );
    // SAFETY: 负 PID 只作用于测试调用方创建的专属组，不依赖分析器控制线程。
    assert_eq!(
        unsafe { libc::kill(-(child.id() as i32), libc::SIGKILL) },
        0
    );
    assert!(!child.wait().unwrap().success());
    let start = Instant::now();
    while descendants
        .iter()
        .any(|pid| unsafe { libc::kill(*pid, 0) } == 0)
    {
        assert!(
            start.elapsed() < Duration::from_secs(5),
            "工具后代未被调用方回收"
        );
        thread::sleep(Duration::from_millis(10));
    }
    let mut output = String::new();
    child
        .stdout
        .take()
        .unwrap()
        .read_to_string(&mut output)
        .unwrap();
    assert!(output.is_empty(), "强制终止不能被伪装为成功图");
}

/// 最终结果被 stdout 背压阻塞时，调用方仍能直接终止进程并拒绝部分 JSON。
#[test]
fn caller_can_terminate_a_blocked_final_output() {
    let repository = tempfile::tempdir().unwrap();
    let rust = option_env!("CARGO_BIN_EXE_codegraph-rust").is_some();
    if rust {
        std::fs::write(repository.path().join("Cargo.toml"), "[package]\nname='backpressure'\nversion='0.1.0'\nedition='2021'\n[lib]\npath='lib.rs'\n").unwrap();
    } else {
        std::fs::write(
            repository.path().join("ty.toml"),
            "[environment]\npython-version='3.11'\n",
        )
        .unwrap();
    }
    let source: String = (0..500)
        .map(|i| {
            if rust {
                format!("pub fn value_{i}() -> usize {{ {i} }}\n")
            } else {
                format!("def value_{i}(): return {i}\n")
            }
        })
        .collect();
    std::fs::write(
        repository
            .path()
            .join(if rust { "lib.rs" } else { "main.py" }),
        source,
    )
    .unwrap();
    let mut child = Command::new(executable())
        .arg("analyze")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let request = json!({"protocol":"codegraph.analyzer/1","type":"analyze","id":"backpressure","repository_root":repository.path(),"options":{"include_tests":false,"rust_dependency_scope":"repository-only"}});
    child
        .stdin
        .take()
        .unwrap()
        .write_all(request.to_string().as_bytes())
        .unwrap();
    let mut output = child.stdout.take().unwrap();
    let mut first = [0];
    output.read_exact(&mut first).unwrap();
    thread::sleep(Duration::from_millis(100));
    assert!(
        child.try_wait().unwrap().is_none(),
        "结果 fixture 没有形成 stdout 背压"
    );
    child.kill().unwrap();
    assert!(!child.wait().unwrap().success());
    let mut partial = first.to_vec();
    output.read_to_end(&mut partial).unwrap();
    assert!(
        serde_json::from_slice::<Value>(&partial).is_err(),
        "部分结果不能成为成功响应"
    );
}

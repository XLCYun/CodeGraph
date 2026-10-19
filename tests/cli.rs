//! 公共二进制的真实 stdio、包选择与进程生命周期验收。
use serde_json::{json, Value};
use std::{
    fs,
    io::{BufRead, BufReader, Write},
    path::{Path, PathBuf},
    process::{Child, Command, Output, Stdio},
    time::{Duration, Instant},
};

/// 每个场景独占源码和受信包，避免污染用户项目或真实安装。
struct Fixture {
    _directory: tempfile::TempDir,
    pack: PathBuf,
    repository: PathBuf,
}
impl Fixture {
    /// Node 只充当固定入口测试工具；运行参数仍完全由 CLI 工厂决定。
    fn new(mode: &str) -> Self {
        let directory = tempfile::tempdir().unwrap();
        let pack = directory.path().join("pack");
        let repository = directory.path().join("repository");
        fs::create_dir_all(&repository).unwrap();
        fs::write(repository.join("main.ts"), "function run() { return 1; }").unwrap();
        for folder in [
            "bin",
            "control",
            "runtime/helper",
            "runtime/typescript",
            "runtime/node-types",
            "runtime/undici",
        ] {
            fs::create_dir_all(pack.join(folder)).unwrap();
        }
        let output = Command::new("node")
            .args(["-p", "process.execPath"])
            .output()
            .expect("CLI tests require Node.js");
        assert!(output.status.success());
        let node = PathBuf::from(String::from_utf8(output.stdout).unwrap().trim())
            .canonicalize()
            .unwrap();
        let executable = pack.join(if cfg!(windows) {
            "bin/node.exe"
        } else {
            "bin/node"
        });
        fs::hard_link(&node, &executable)
            .or_else(|_| fs::copy(&node, &executable).map(|_| ()))
            .unwrap();
        // Homebrew Node 26 的入口依赖同安装根的 libnode；保持该真实运行时相对布局。
        #[cfg(target_os = "macos")]
        if let Some(root) = node.parent().and_then(Path::parent) {
            if let Ok(entries) = fs::read_dir(root.join("lib")) {
                fs::create_dir_all(pack.join("lib")).unwrap();
                for entry in entries
                    .flatten()
                    .filter(|entry| entry.file_name().to_string_lossy().starts_with("libnode."))
                {
                    let target = pack.join("lib").join(entry.file_name());
                    fs::hard_link(entry.path(), &target)
                        .or_else(|_| fs::copy(entry.path(), &target).map(|_| ()))
                        .unwrap();
                }
            }
        }

        fs::write(
            pack.join("runtime/helper/main.mjs"),
            include_str!("fixtures/cli-helper.mjs"),
        )
        .unwrap();
        fs::write(pack.join("runtime/helper/mode"), mode).unwrap();
        let mut platforms = vec![];
        for (index, (package, root, marker)) in [
            ("typescript", "runtime/typescript", "lib.d.ts"),
            ("@types/node", "runtime/node-types", "index.d.ts"),
            ("undici-types", "runtime/undici", "index.d.ts"),
        ]
        .into_iter()
        .enumerate()
        {
            fs::write(pack.join(root).join(marker), "").unwrap();
            platforms.push(json!({"id":format!("sdk-{index}"),"kind":"typescript-declarations","version":"1.0.0","root":root,"metadata":{"package":package}}));
        }
        fs::write(
            pack.join("control/native.json"),
            serde_json::to_vec(
                &json!({"schema_version":1,"helper_version":"0.1.0","platforms":platforms}),
            )
            .unwrap(),
        )
        .unwrap();
        fs::write(pack.join("control/codegraph.json"),serde_json::to_vec(&json!({"schema_version":1,"pack_id":"typescript-javascript","pack_version":"0.1.0","engine_version":"0.1.0","protocol":"codegraph.analyzer/1"})).unwrap()).unwrap();
        Self {
            _directory: directory,
            pack,
            repository,
        }
    }
    /// 通过唯一公共 CLI 启动，测试不向引擎直接传 helper 命令。
    fn spawn(&self) -> Child {
        let mut command = Command::new(env!("CARGO_BIN_EXE_codegraph"));
        command
            .args(["analyze", "--pack", "typescript-javascript", "--pack-root"])
            .arg(&self.pack)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            command.process_group(0);
        }
        command.spawn().unwrap()
    }
    /// 必需选项全部显式给出，测试 JSON 与文档示例一致。
    fn request(&self) -> Value {
        json!({"protocol":"codegraph.analyzer/1","type":"analyze","id":"request-1","repository_root":self.repository,"files":["main.ts"],"options":{"include_tests":false,"rust_dependency_scope":"standard-library"}})
    }
    /// 对有限输入关闭 stdin，确保 `< request.json` 用法能够正常完成。
    fn run(&self, bytes: &[u8]) -> (std::process::ExitStatus, Vec<Value>) {
        let output = self.run_output(bytes);
        let lines = String::from_utf8(output.stdout)
            .unwrap()
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        (output.status, lines)
    }
    /// 协议失败用例保留原始 stdout/stderr，断言失败时能定位具体输入和退出路径。
    fn run_output(&self, bytes: &[u8]) -> Output {
        let mut child = self.spawn();
        child.stdin.take().unwrap().write_all(bytes).unwrap();
        wait(&mut child, Duration::from_secs(12));
        let output = child.wait_with_output().unwrap();
        assert!(output.stderr.len() <= 256 * 1024);
        output
    }
    /// 返回 helper 的隔离测试标记路径。
    fn marker(&self, name: &str) -> PathBuf {
        self.pack.join("runtime/helper").join(name)
    }
}

/// 有限等待真实退出，超时先终止进程再使测试失败。
fn wait(child: &mut Child, timeout: Duration) {
    let deadline = Instant::now() + timeout;
    while child.try_wait().unwrap().is_none() {
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            panic!("CLI did not exit within {timeout:?}");
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

/// 原子等待读取已完整写入的 PID 标记。
fn pid(path: &Path) -> u32 {
    let deadline = Instant::now() + Duration::from_secs(8);
    loop {
        if let Some(pid) = fs::read_to_string(path)
            .ok()
            .and_then(|value| value.parse().ok())
        {
            return pid;
        }
        assert!(Instant::now() < deadline, "missing {}", path.display());
        std::thread::sleep(Duration::from_millis(10));
    }
}

/// 换行是公开 NDJSON 契约的一部分。
fn line(value: &Value) -> Vec<u8> {
    let mut bytes = serde_json::to_vec(value).unwrap();
    bytes.push(b'\n');
    bytes
}

/// 成功必须经过真实工厂、Host 来源验证和进程树退出屏障，不泄露私有 evidence。
#[test]
fn real_factory_host_graph_and_eof() {
    let fixture = Fixture::new("ok");
    let (status, messages) = fixture.run(&line(&fixture.request()));
    assert!(status.success(), "{messages:?}");
    assert_eq!(messages.len(), 2);
    assert_eq!(messages[0]["engine_version"], "0.1.0");
    assert_eq!(messages[0]["pack_id"], "typescript-javascript");
    let result = &messages[1];
    assert_eq!(result["outcome"], "ok");
    assert!(result.get("evidence").is_none());
    assert_eq!(
        result["graph"]["nodes"][0]["source"],
        "function run() { return 1; }"
    );
    assert_eq!(
        result["graph"]["nodes"][0]["extensions"]["typescript.future"]["nested"][1],
        true
    );
    assert_eq!(
        result["graph"]["external_symbols"][0]["scope"],
        "dependency"
    );
    let scratch = fs::read_to_string(fixture.marker("scratch")).unwrap();
    assert!(!Path::new(&scratch).exists());
    assert!(!result.to_string().contains(&scratch));
}

/// 空选集只返回合法空图，不需要运行语言 helper 或发现目标 SDK。
#[test]
fn empty_selection_does_not_start_helper() {
    let fixture = Fixture::new("ok");
    let mut request = fixture.request();
    request["files"] = json!([]);
    let (status, messages) = fixture.run(&line(&request));
    assert!(status.success(), "{messages:?}");
    assert_eq!(
        messages[1]["graph"],
        json!({"root_contains":[],"nodes":[],"external_symbols":[]})
    );
    assert!(!fixture.marker("helper-pid").exists());
}

/// 截断、重复请求、错误取消 ID 与版本不匹配都是明确失败。
#[test]
fn invalid_protocol_and_pack_inputs_fail() {
    let fixture = Fixture::new("ok");
    let request = fixture.request();
    let mut duplicate = line(&request);
    duplicate.extend(line(&request));
    let wrong_cancel =
        line(&json!({"protocol":"codegraph.analyzer/1","type":"cancel","id":"wrong"}));
    for (case, input) in [
        ("truncated", b"{\"protocol\":".to_vec()),
        ("duplicate", duplicate),
        ("wrong cancel", wrong_cancel),
        (
            "wrong protocol",
            line(&json!({"protocol":"codegraph.analyzer/2","type":"analyze","id":"bad"})),
        ),
        ("empty stdin", b"".to_vec()),
    ] {
        let output = fixture.run_output(&input);
        let context = format!("case={case}, input={input:?}, output={output:?}");
        let messages: Vec<Value> = String::from_utf8_lossy(&output.stdout)
            .lines()
            .map(|line| serde_json::from_str(line).expect(&context))
            .collect();
        assert_eq!(output.status.code(), Some(1), "{context}");
        assert_eq!(messages.len(), 2, "{context}");
        assert_eq!(messages[0]["type"], "ready", "{context}");
        assert_eq!(messages[1]["type"], "completed", "{context}");
        assert_eq!(messages[1]["outcome"], "error", "{context}");
        assert!(messages.last().unwrap().get("graph").is_none());
    }
    fs::write(fixture.pack.join("control/codegraph.json"),r#"{"schema_version":1,"pack_id":"go","pack_version":"0.1.0","engine_version":"0.1.0","protocol":"codegraph.analyzer/1"}"#).unwrap();
    let (_, messages) = fixture.run(&line(&request));
    assert_eq!(messages[1]["error"]["code"], "invalid_pack");
}

/// helper 身份或图无效不能成为成功空图，公共说明也不带安装绝对路径。
#[test]
fn helper_mismatch_and_invalid_graph_fail() {
    for mode in ["mismatch", "invalid-graph"] {
        let fixture = Fixture::new(mode);
        let (status, messages) = fixture.run(&line(&fixture.request()));
        assert!(!status.success());
        assert_eq!(messages[1]["error"]["code"], "analysis_failed");
        assert!(!messages[1]
            .to_string()
            .contains(fixture.pack.to_str().unwrap()));
    }
}

/// Unix 检查进程确实消失，既覆盖正常 helper 也覆盖 setsid 脱组后代。
#[cfg(unix)]
fn gone(pids: &[u32]) {
    let deadline = Instant::now() + Duration::from_secs(8);
    loop {
        // SAFETY: 信号 0 只观察本测试已经记录的进程。
        if pids
            .iter()
            .all(|pid| unsafe { libc::kill(*pid as i32, 0) } != 0)
        {
            return;
        }
        assert!(Instant::now() < deadline, "processes survived: {pids:?}");
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// 显式取消可协作和不响应的 helper 都应在有限时间回收。
#[test]
fn cancellation_reaps_nested_helpers() {
    for mode in ["stall", "ignore-cancel"] {
        let fixture = Fixture::new(mode);
        let mut child = fixture.spawn();
        let mut input = child.stdin.take().unwrap();
        input.write_all(&line(&fixture.request())).unwrap();
        let helper = pid(&fixture.marker("helper-pid"));
        let worker = pid(&fixture.marker("worker-pid"));
        let descendant = pid(&fixture.marker("descendant-pid"));
        // 先给两个 owner 至少一次观察机会，明确测试的是已观察到的脱组进程。
        std::thread::sleep(Duration::from_millis(100));
        input
            .write_all(&line(
                &json!({"protocol":"codegraph.analyzer/1","type":"cancel","id":"request-1"}),
            ))
            .unwrap();
        wait(&mut child, Duration::from_secs(10));
        let output = child.wait_with_output().unwrap();
        let messages: Vec<Value> = String::from_utf8(output.stdout)
            .unwrap()
            .lines()
            .map(|l| serde_json::from_str(l).unwrap())
            .collect();
        assert_eq!(
            messages.last().unwrap()["outcome"],
            "cancelled",
            "mode={mode} helper={helper} worker={worker} descendant={descendant} {messages:?}; stderr={}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(output.status.code(), Some(130));
        let scratch = fs::read_to_string(fixture.marker("scratch")).unwrap();
        assert!(
            !Path::new(&scratch).exists(),
            "scratch survived cancellation"
        );
        #[cfg(unix)]
        gone(&[helper, worker, descendant]);
        #[cfg(windows)]
        let _ = (helper, worker, descendant);
    }
}

/// 外层消费者强杀 CLI 的进程组，私有工作进程仍通过父管道 EOF 回收嵌套组。
#[cfg(unix)]
#[test]
fn parent_kill_reaps_worker_helper_and_detached_descendant() {
    let fixture = Fixture::new("ignore-cancel");
    let mut child = fixture.spawn();
    child
        .stdin
        .as_mut()
        .unwrap()
        .write_all(&line(&fixture.request()))
        .unwrap();
    let pids = [
        pid(&fixture.marker("worker-pid")),
        pid(&fixture.marker("helper-pid")),
        pid(&fixture.marker("descendant-pid")),
    ];
    std::thread::sleep(Duration::from_millis(100));
    // SAFETY: 该组由本测试为 CLI 单独创建，私有 worker 和 helper 各自另建组。
    assert_eq!(
        unsafe { libc::kill(-(child.id() as i32), libc::SIGKILL) },
        0
    );
    child.wait().unwrap();
    gone(&pids);
    let scratch = fs::read_to_string(fixture.marker("scratch")).unwrap();
    assert!(
        !Path::new(&scratch).exists(),
        "scratch survived parent kill"
    );
}

/// 消费者不排空 stdout 时，写入期限仍有限，工作进程已回收。
#[test]
fn blocked_stdout_has_bounded_exit() {
    let fixture = Fixture::new("large");
    let mut child = fixture.spawn();
    let output = child.stdout.take().unwrap();
    let mut reader = BufReader::new(output);
    let mut ready = String::new();
    reader.read_line(&mut ready).unwrap();
    child
        .stdin
        .as_mut()
        .unwrap()
        .write_all(&line(&fixture.request()))
        .unwrap();
    let helper = pid(&fixture.marker("helper-pid"));
    let worker = pid(&fixture.marker("worker-pid"));
    wait(&mut child, Duration::from_secs(12));
    assert!(!child.wait().unwrap().success());
    #[cfg(unix)]
    gone(&[helper, worker]);
    #[cfg(windows)]
    let _ = (helper, worker);
    drop(reader);
}

/// 只结束请求行而不关闭消费者 stdin，结果完成后也必须回收读线程。
#[test]
fn open_consumer_stdin_does_not_hold_completed_process() {
    let fixture = Fixture::new("ok");
    let mut child = fixture.spawn();
    let mut input = child.stdin.take().unwrap();
    input.write_all(&line(&fixture.request())).unwrap();
    wait(&mut child, Duration::from_secs(8));
    let output = child.wait_with_output().unwrap();
    assert!(output.status.success());
    drop(input);
}

/// 超过公开 128 MiB 请求上限时立即失败；不等换行也不开始语言分析。
#[test]
fn oversized_request_fails_before_helper_start() {
    let fixture = Fixture::new("ok");
    let mut child = fixture.spawn();
    let mut input = child.stdin.take().unwrap();
    let block = vec![b' '; 1024 * 1024];
    for _ in 0..130 {
        if input.write_all(&block).is_err() {
            break;
        }
    }
    drop(input);
    wait(&mut child, Duration::from_secs(8));
    let output = child.wait_with_output().unwrap();
    assert!(!output.status.success());
    let messages: Vec<Value> = String::from_utf8(output.stdout)
        .unwrap()
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    assert_eq!(messages.last().unwrap()["error"]["code"], "resource_limit");
    assert!(!fixture.marker("helper-pid").exists());
}

/// 受信包读取被 FIFO 阻塞时，取消仍覆盖 factory 阶段，不依赖 helper 已经启动。
#[cfg(unix)]
#[test]
fn cancellation_bounds_blocked_factory() {
    use std::os::unix::ffi::OsStrExt;
    let fixture = Fixture::new("ok");
    let native = fixture.pack.join("control/native.json");
    fs::remove_file(&native).unwrap();
    let name = std::ffi::CString::new(native.as_os_str().as_bytes()).unwrap();
    // SAFETY: 只在本测试临时目录创建 FIFO，模拟阻塞的包文件读取。
    assert_eq!(unsafe { libc::mkfifo(name.as_ptr(), 0o600) }, 0);
    let mut child = fixture.spawn();
    let mut input = child.stdin.take().unwrap();
    input.write_all(&line(&fixture.request())).unwrap();
    // writer 的非阻塞 open 只有在 factory 已打开读端后才成功；保留句柄使读取继续阻塞。
    use std::os::unix::fs::OpenOptionsExt;
    let deadline = Instant::now() + Duration::from_secs(8);
    let _blocked_writer = loop {
        match fs::OpenOptions::new()
            .write(true)
            .custom_flags(libc::O_NONBLOCK)
            .open(&native)
        {
            Ok(writer) => break writer,
            Err(_) => {
                assert!(Instant::now() < deadline, "factory did not open FIFO");
                std::thread::sleep(Duration::from_millis(10));
            }
        }
    };
    input
        .write_all(&line(
            &json!({"protocol":"codegraph.analyzer/1","type":"cancel","id":"request-1"}),
        ))
        .unwrap();
    wait(&mut child, Duration::from_secs(10));
    let output = child.wait_with_output().unwrap();
    let messages: Vec<Value> = String::from_utf8(output.stdout)
        .unwrap()
        .lines()
        .map(|value| serde_json::from_str(value).unwrap())
        .collect();
    assert_eq!(
        messages.last().unwrap()["outcome"],
        "cancelled",
        "messages={messages:?}; status={:?}; stderr={}",
        output.status,
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(!fixture.marker("helper-pid").exists());
}

/// Windows 强杀公共 CLI 时，kill-on-close 的外层 Job 仍负责所有嵌套 helper。
#[cfg(windows)]
#[test]
fn windows_parent_kill_reaps_nested_jobs() {
    use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
    use windows_sys::Win32::{
        Foundation::WAIT_OBJECT_0,
        System::Threading::{OpenProcess, WaitForSingleObject},
    };
    let fixture = Fixture::new("ignore-cancel");
    let mut child = fixture.spawn();
    child
        .stdin
        .as_mut()
        .unwrap()
        .write_all(&line(&fixture.request()))
        .unwrap();
    let pids = [
        pid(&fixture.marker("worker-pid")),
        pid(&fixture.marker("helper-pid")),
        pid(&fixture.marker("descendant-pid")),
    ];
    let handles: Vec<OwnedHandle> = pids
        .iter()
        .map(|&pid| {
            // SAFETY: 仅打开本测试已观察进程的 SYNCHRONIZE 句柄，避免 PID 复用判断。
            let handle = unsafe { OpenProcess(0x00100000, 0, pid) };
            assert!(!handle.is_null());
            unsafe { OwnedHandle::from_raw_handle(handle) }
        })
        .collect();
    child.kill().unwrap();
    child.wait().unwrap();
    for handle in handles {
        // SAFETY: 句柄仍由当前作用域持有；八秒界限覆盖 Job 异步终止。
        assert_eq!(
            unsafe { WaitForSingleObject(handle.as_raw_handle(), 8000) },
            WAIT_OBJECT_0
        );
    }
}

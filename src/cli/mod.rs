//! 公共进程监督 stdio，私有工作进程执行完整分析；父退出仍有独立回收路径。
mod engine;
mod graph;
pub(crate) mod protocol;
mod transport;
mod worker;

use crate::{child_process::ProcessTree, pack::PackId};
use protocol::{Failure, Message, PROTOCOL};
use std::{
    path::PathBuf,
    process::{Command, Stdio},
    sync::mpsc::{self, SyncSender},
    time::{Duration, Instant},
};
use transport::{Event, Source, Transport, POLL};

const STARTUP: Duration = Duration::from_secs(60);
const ANALYSIS: Duration = Duration::from_secs(30 * 60);
const CANCEL: Duration = Duration::from_secs(2);
const EXIT: Duration = Duration::from_secs(5);

/// 命令行只选择受信安装包，不允许注入任意命令或运行环境。
#[derive(Clone)]
pub(super) struct Arguments {
    pub pack: PackId,
    pub root: PathBuf,
}

/// 解析明确的命令行形式，重复和未知参数都失败。
pub(crate) fn main() -> i32 {
    let mut args: Vec<_> = std::env::args_os().skip(1).collect();
    if args == ["--version"] {
        println!("codegraph {}", env!("CARGO_PKG_VERSION"));
        return 0;
    }
    if args == ["--help"] || args == ["-h"] || args == ["analyze", "--help"] {
        println!("CodeGraph {}\n\nUsage: codegraph analyze --pack <id> --pack-root <absolute-path>\n       codegraph --help | --version\n\nPacks: rust, typescript-javascript, python, java, go, csharp, c-cpp\nProtocol: codegraph.analyzer/1 (UTF-8 NDJSON on stdin/stdout)\nRead docs/protocol.md for requests, cancellation, and limits.", env!("CARGO_PKG_VERSION"));
        return 0;
    }
    let private = args.first().is_some_and(|arg| arg == "__worker");
    if args
        .first()
        .is_none_or(|arg| arg != "analyze" && arg != "__worker")
    {
        eprintln!("Use codegraph --help for usage.");
        return 2;
    }
    args.remove(0);
    let mut pack = None;
    let mut root = None;
    if args.len() != 4 {
        eprintln!("Expected --pack and --pack-root exactly once.");
        return 2;
    }
    for pair in args.chunks_exact(2) {
        if pair[0] == "--pack" && pack.is_none() {
            pack = pair[1].to_str().and_then(|text| {
                PackId::NATIVE_MIGRATED
                    .iter()
                    .copied()
                    .find(|id| id.slug() == text)
            });
            if pack.is_none() {
                eprintln!("Unknown pack ID.");
                return 2;
            }
        } else if pair[0] == "--pack-root" && root.is_none() {
            let path = PathBuf::from(&pair[1]);
            if !path.is_absolute() || path.to_str().is_none() {
                eprintln!("Pack root must be an absolute UTF-8 path.");
                return 2;
            }
            root = Some(path);
        } else {
            eprintln!("Unknown or repeated option.");
            return 2;
        }
    }
    let (Some(pack), Some(root)) = (pack, root) else {
        return 2;
    };
    let arguments = Arguments { pack, root };
    let result = if private {
        worker::run(arguments)
    } else {
        supervise(arguments)
    };
    match result {
        Ok(code) => code,
        Err(error) => {
            eprintln!("CodeGraph transport or process cleanup failed: {error}");
            1
        }
    }
}

/// 公共监督进程始终保有工作进程管道；消费者的正常 EOF 不会传为父死亡。
fn supervise(arguments: Arguments) -> std::io::Result<i32> {
    let input = std::io::stdin();
    let output = std::io::stdout();
    transport::nonblocking(&input)?;
    transport::nonblocking(&output)?;
    let mut io = Transport::new();
    io.reader(input, Source::Consumer)?;
    let output = io.writer(output, Source::Output)?;
    let mut pending = 0usize;
    let mut output_deadline = None;
    send(
        &output,
        &serde_json::json!({"protocol":PROTOCOL,"type":"ready","engine_version":env!("CARGO_PKG_VERSION"),"pack_id":arguments.pack.slug()}),
    )?;
    pending += 1;
    output_deadline.get_or_insert(Instant::now() + EXIT);
    let mut child: Option<ProcessTree> = None;
    let mut requests = None;
    let mut request_id: Option<String> = None;
    let mut deadline = Instant::now() + STARTUP;
    let mut cancellation = None;
    let mut terminal = None;
    let mut worker_eof = false;
    let mut finished = false;
    let mut exit_code = 0;
    loop {
        match io.events.recv_timeout(POLL) {
            Ok(Event::Line(Source::Consumer, bytes)) if !finished => {
                let decoded = protocol::decode(&bytes);
                let error = match decoded {
                    Ok(Message::Analyze(request)) if request_id.is_none() => {
                        request_id = Some(request.id.clone());
                        let (process, sender) = spawn_worker(&arguments, &mut io)?;
                        child = Some(process);
                        requests = Some(sender);
                        let mut bytes = bytes;
                        bytes.push(b'\n');
                        requests
                            .as_ref()
                            .unwrap()
                            .try_send(bytes)
                            .map_err(std::io::Error::other)?;
                        deadline = Instant::now() + ANALYSIS;
                        None
                    }
                    Ok(Message::Cancel(cancel))
                        if request_id.as_deref() == Some(&cancel.id) && cancellation.is_none() =>
                    {
                        cancellation = Some(Instant::now() + CANCEL + EXIT);
                        if let Some(sender) = &requests {
                            let mut bytes = bytes;
                            bytes.push(b'\n');
                            let _ = sender.try_send(bytes);
                        }
                        None
                    }
                    Ok(_) => Some(Failure::new(
                        "invalid_request",
                        "重复分析、重复取消或取消请求 ID 不匹配",
                    )),
                    Err(error) => Some(error),
                };
                if let Some(error) = error {
                    terminal = Some(protocol::failed(request_id.as_deref(), &error));
                }
            }
            Ok(Event::Eof(Source::Consumer)) if request_id.is_none() && !finished => {
                terminal = Some(protocol::failed(
                    None,
                    &Failure::new("invalid_request", "未收到 analyze 请求"),
                ));
            }
            Ok(Event::Line(Source::Worker, bytes)) if !finished => {
                if terminal.is_some() {
                    terminal = Some(protocol::failed(
                        request_id.as_deref(),
                        &Failure::new("internal_failure", "工作进程发送多次结果"),
                    ));
                } else {
                    let crate::strict_json::UniqueValue(value) =
                        serde_json::from_slice(&bytes).map_err(std::io::Error::other)?;
                    if value["protocol"] != PROTOCOL
                        || value["type"] != "completed"
                        || value["id"].as_str() != request_id.as_deref()
                    {
                        terminal = Some(protocol::failed(
                            request_id.as_deref(),
                            &Failure::new("internal_failure", "工作进程结果身份不匹配"),
                        ));
                    } else {
                        terminal = Some(value);
                    }
                }
                deadline = Instant::now() + EXIT;
            }
            Ok(Event::Eof(Source::Worker)) => worker_eof = true,
            Ok(Event::Written(Source::Output)) => {
                pending = pending.saturating_sub(1);
                if pending == 0 {
                    output_deadline = None;
                }
            }
            Ok(Event::Error(error)) if !finished => {
                terminal = Some(protocol::failed(request_id.as_deref(), &error))
            }
            Ok(Event::Error(_)) => {
                cleanup(&mut child)?;
                io.finish()?;
                return Ok(1);
            }
            Ok(_) | Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(_) => {
                cleanup(&mut child)?;
                return Err(std::io::Error::other("事件队列关闭"));
            }
        }
        if output_deadline.is_some_and(|time| Instant::now() >= time) {
            cleanup(&mut child)?;
            io.finish()?;
            return Ok(1);
        }
        if finished {
            if pending == 0 {
                break;
            }
            continue;
        }
        if cancellation.is_some_and(|time| Instant::now() >= time) && terminal.is_none() {
            cleanup(&mut child)?;
            terminal = Some(
                serde_json::json!({"protocol":PROTOCOL,"type":"completed","id":request_id,"outcome":"cancelled"}),
            );
        }
        if Instant::now() >= deadline && terminal.is_none() {
            terminal = Some(protocol::failed(
                request_id.as_deref(),
                &Failure::new("timeout", "等待请求或分析超过固定期限"),
            ));
        }
        let status = if let Some(process) = child.as_mut() {
            process.status()?
        } else {
            None
        };
        if status.is_some() && worker_eof && terminal.is_none() {
            terminal = Some(if cancellation.is_some() {
                // 不可协作 factory 被取消看门狗终止时可能只有 EOF；仍需回收后报告取消。
                serde_json::json!({"protocol":PROTOCOL,"type":"completed","id":request_id,"outcome":"cancelled"})
            } else {
                protocol::failed(
                    request_id.as_deref(),
                    &Failure::new("analysis_failed", "工作进程未返回完整结果"),
                )
            });
        }
        let is_success = terminal
            .as_ref()
            .is_some_and(|value| value["outcome"] == "ok");
        if is_success && !(worker_eof && status.is_some()) && Instant::now() < deadline {
            continue;
        }
        if let Some(mut value) = terminal.take() {
            if is_success && (!worker_eof || !status.is_some_and(|s| s.success())) {
                value = protocol::failed(
                    request_id.as_deref(),
                    &Failure::new("analysis_failed", "工作进程未正常完成退出屏障"),
                );
            }
            if cleanup(&mut child).is_err() {
                value = protocol::failed(
                    request_id.as_deref(),
                    &Failure::new("cleanup_failed", "分析进程树回收失败"),
                );
            }
            requests.take();
            exit_code = if value["outcome"] == "ok" {
                0
            } else if value["outcome"] == "cancelled" {
                130
            } else {
                1
            };
            send(&output, &value)?;
            pending += 1;
            output_deadline = Some(Instant::now() + EXIT);
            finished = true;
        }
    }
    cleanup(&mut child)?;
    io.finish()?;
    Ok(exit_code)
}

/// 私有子进程沿用同一二进制；消费者不能提供不同的 helper 命令。
fn spawn_worker(
    arguments: &Arguments,
    io: &mut Transport,
) -> std::io::Result<(ProcessTree, SyncSender<Vec<u8>>)> {
    let mut command = Command::new(std::env::current_exe()?);
    command
        .arg("__worker")
        .arg("--pack")
        .arg(arguments.pack.slug())
        .arg("--pack-root")
        .arg(&arguments.root)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    let mut process = ProcessTree::spawn(&mut command)?;
    let input = process
        .take_stdin()
        .ok_or_else(|| std::io::Error::other("缺少 worker stdin"))?;
    let output = process
        .take_stdout()
        .ok_or_else(|| std::io::Error::other("缺少 worker stdout"))?;
    transport::nonblocking(&input)?;
    transport::nonblocking(&output)?;
    io.reader(output, Source::Worker)?;
    let sender = io.writer(input, Source::Request)?;
    Ok((process, sender))
}

/// 成功封套写出之前必须确认整棵工作进程树已经回收。
fn cleanup(child: &mut Option<ProcessTree>) -> std::io::Result<()> {
    if let Some(mut process) = child.take() {
        process.reap()?;
    }
    Ok(())
}

/// 有界发送失败会成为进程失败，不能静默丢掉响应。
fn send(sender: &SyncSender<Vec<u8>>, value: &serde_json::Value) -> std::io::Result<()> {
    let bytes = match transport::encode(value) {
        Ok(bytes) => bytes,
        Err(error) => transport::encode(&protocol::failed(value["id"].as_str(), &error))
            .map_err(|_| std::io::Error::other("错误响应序列化失败"))?,
    };
    sender.try_send(bytes).map_err(std::io::Error::other)
}

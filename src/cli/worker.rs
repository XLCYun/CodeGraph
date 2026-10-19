//! 私有工作进程的主线程持续监听父管道，即使 factory 阻塞也能执行有界取消。
use super::{
    engine,
    protocol::{self, Failure, Message},
    transport::{self, Event, Source, Transport, POLL},
    Arguments, CANCEL, EXIT,
};
use crate::child_process::CurrentProcessDomain;
use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc, Arc,
    },
    time::Instant,
};

/// 父退出触发管道 EOF；不能把消费者请求之后的普通 EOF直接传到本层。
pub(super) fn run(arguments: Arguments) -> std::io::Result<i32> {
    let mut domain = CurrentProcessDomain::establish()?;
    let input = std::io::stdin();
    let output = std::io::stdout();
    transport::nonblocking(&input)?;
    transport::nonblocking(&output)?;
    let mut io = Transport::new();
    io.reader(input, Source::Consumer)?;
    let writer = io.writer(output, Source::Output)?;
    let cancelled = Arc::new(AtomicBool::new(false));
    let (result_tx, result_rx) = mpsc::sync_channel(1);
    let mut thread = None;
    let mut id = None;
    let mut stopping = None;
    let mut sent = None;
    let start = Instant::now();
    loop {
        domain.snapshot();
        match io.events.recv_timeout(POLL) {
            Ok(Event::Line(_, bytes)) if sent.is_none() => match protocol::decode(&bytes) {
                Ok(Message::Analyze(request)) if thread.is_none() => {
                    id = Some(request.id.clone());
                    let observed = cancelled.clone();
                    let arguments = arguments.clone();
                    let sender = result_tx.clone();
                    thread = Some(
                        std::thread::Builder::new()
                            .name("codegraph-engine".into())
                            .spawn(move || {
                                let result = engine::analyze(
                                    arguments.pack,
                                    &arguments.root,
                                    &request,
                                    Arc::new(move || observed.load(Ordering::Acquire)),
                                );
                                let result = result.unwrap_or_else(|failure| {
                                    protocol::failed(Some(&request.id), &failure)
                                });
                                let _ = sender.send(result);
                            })?,
                    );
                }
                Ok(Message::Cancel(cancel)) if id.as_deref() == Some(&cancel.id) => {
                    cancelled.store(true, Ordering::Release);
                    stopping.get_or_insert(Instant::now() + CANCEL + EXIT);
                }
                _ => {
                    cancelled.store(true, Ordering::Release);
                    stopping.get_or_insert(Instant::now() + CANCEL + EXIT);
                }
            },
            Ok(Event::Eof(_)) | Ok(Event::Error(_)) => {
                cancelled.store(true, Ordering::Release);
                stopping.get_or_insert(Instant::now() + CANCEL + EXIT);
            }
            Ok(Event::Written(Source::Output)) if sent.is_some() => {
                if let Some(thread) = thread.take() {
                    thread
                        .join()
                        .map_err(|_| std::io::Error::other("分析线程异常退出"))?;
                }
                io.finish()?;
                domain.completed()?;
                return Ok(0);
            }
            _ => {}
        }
        if sent.is_none() {
            if let Ok(value) = result_rx.try_recv() {
                super::send(&writer, &value)?;
                sent = Some(Instant::now() + EXIT);
            } else if thread.as_ref().is_some_and(|thread| thread.is_finished()) {
                let value = protocol::failed(
                    id.as_deref(),
                    &Failure::new("internal_failure", "分析线程异常退出"),
                );
                super::send(&writer, &value)?;
                sent = Some(Instant::now() + EXIT);
            }
        }
        if stopping.is_some_and(|time| Instant::now() >= time)
            || sent.is_some_and(|time| Instant::now() >= time)
            || (thread.is_none() && start.elapsed() > super::STARTUP)
        {
            // 即使加载 SDK 的线程卡在系统调用中，也先杀已观察后代和自身域，不遗留线程。
            domain.snapshot();
            domain.terminate()?;
            return Err(std::io::Error::other("工作进程终止未生效"));
        }
    }
}

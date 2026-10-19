//! 可中断 stdio。所有队列和行有上限，主循环从不执行可能阻塞的管道读写。
use super::protocol::{Failure, MAX_BYTES};
use std::{
    io::{self, Read, Write},
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver, SyncSender},
        Arc,
    },
    thread::JoinHandle,
    time::{Duration, Instant},
};

pub(super) const POLL: Duration = Duration::from_millis(10);

/// 区分消费者输入、内部工作进程输出和两个写入确认。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Source {
    Consumer,
    Worker,
    Output,
    Request,
}

/// 所有读写结果经有限事件队列回到唯一状态机。
pub(super) enum Event {
    Line(Source, Vec<u8>),
    Eof(Source),
    Written(Source),
    Error(Failure),
}

/// 统一拥有工作线程，任何提前退出都先停止并回收线程。
pub(super) struct Transport {
    stop: Arc<AtomicBool>,
    workers: Vec<JoinHandle<()>>,
    sender: SyncSender<Event>,
    pub events: Receiver<Event>,
}

impl Transport {
    /// 有限队列使大量小行也不能无限占用内存。
    pub fn new() -> Self {
        let (sender, events) = mpsc::sync_channel(4);
        Self {
            stop: Arc::new(AtomicBool::new(false)),
            workers: Vec::new(),
            sender,
            events,
        }
    }

    /// 独占读取端，按 NDJSON 换行切分；EOF 后不会反复发送空事件。
    pub fn reader(
        &mut self,
        mut pipe: impl Read + Send + 'static,
        source: Source,
    ) -> io::Result<()> {
        let sender = self.sender.clone();
        let stop = self.stop.clone();
        self.workers.push(
            std::thread::Builder::new()
                .name("codegraph-reader".into())
                .spawn(move || {
                    let mut buffer = [0u8; 8192];
                    let mut line = Vec::new();
                    while !stop.load(Ordering::Acquire) {
                        match pipe.read(&mut buffer) {
                            Ok(0) => {
                                emit(
                                    &sender,
                                    &stop,
                                    if line.is_empty() {
                                        Event::Eof(source)
                                    } else {
                                        Event::Error(Failure::new(
                                            "invalid_request",
                                            "NDJSON 在换行前截断",
                                        ))
                                    },
                                );
                                return;
                            }
                            Ok(count) => {
                                for &byte in &buffer[..count] {
                                    if byte == b'\n' {
                                        if !emit(
                                            &sender,
                                            &stop,
                                            Event::Line(source, std::mem::take(&mut line)),
                                        ) {
                                            return;
                                        }
                                    } else if line.len() == MAX_BYTES {
                                        emit(
                                            &sender,
                                            &stop,
                                            Event::Error(Failure::new(
                                                "resource_limit",
                                                "协议消息超过 128 MiB",
                                            )),
                                        );
                                        return;
                                    } else {
                                        line.push(byte);
                                    }
                                }
                            }
                            Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                                std::thread::sleep(POLL)
                            }
                            Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
                            Err(_) => {
                                emit(
                                    &sender,
                                    &stop,
                                    Event::Error(Failure::new("io_failure", "协议输入读取失败")),
                                );
                                return;
                            }
                        }
                    }
                })?,
        );
        Ok(())
    }

    /// 每个写线程最多积压一条消息，并向状态机报告真正写完的时间。
    pub fn writer(
        &mut self,
        mut pipe: impl Write + Send + 'static,
        source: Source,
    ) -> io::Result<SyncSender<Vec<u8>>> {
        let (sender, input) = mpsc::sync_channel::<Vec<u8>>(1);
        let events = self.sender.clone();
        let stop = self.stop.clone();
        self.workers.push(
            std::thread::Builder::new()
                .name("codegraph-writer".into())
                .spawn(move || {
                    while !stop.load(Ordering::Acquire) {
                        let data = match input.recv_timeout(POLL) {
                            Ok(data) => data,
                            Err(mpsc::RecvTimeoutError::Timeout) => continue,
                            Err(_) => return,
                        };
                        let mut remaining = data.as_slice();
                        while !remaining.is_empty() && !stop.load(Ordering::Acquire) {
                            match pipe.write(remaining) {
                                Ok(0) => {
                                    emit(
                                        &events,
                                        &stop,
                                        Event::Error(Failure::new(
                                            "io_failure",
                                            "协议输出写入零字节",
                                        )),
                                    );
                                    return;
                                }
                                Ok(count) => remaining = &remaining[count..],
                                Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                                    std::thread::sleep(POLL)
                                }
                                Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
                                Err(_) => {
                                    emit(
                                        &events,
                                        &stop,
                                        Event::Error(Failure::new(
                                            "io_failure",
                                            "协议输出写入失败",
                                        )),
                                    );
                                    return;
                                }
                            }
                        }
                        if !emit(&events, &stop, Event::Written(source)) {
                            return;
                        }
                    }
                })?,
        );
        Ok(sender)
    }

    /// Windows 同步句柄需主动取消；Unix 非阻塞线程会在下一轮停止。
    pub fn finish(&mut self) -> io::Result<()> {
        self.stop.store(true, Ordering::Release);
        let deadline = Instant::now() + Duration::from_secs(5);
        while self.workers.iter().any(|worker| !worker.is_finished()) {
            #[cfg(windows)]
            for worker in &self.workers {
                use std::os::windows::io::AsRawHandle;
                // SAFETY: 句柄由尚未 join 的线程持有，只取消其当前同步 I/O。
                unsafe {
                    windows_sys::Win32::System::IO::CancelSynchronousIo(worker.as_raw_handle());
                }
            }
            if Instant::now() >= deadline {
                return Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    "协议 I/O 线程退出超时",
                ));
            }
            std::thread::sleep(POLL);
        }
        for worker in self.workers.drain(..) {
            worker
                .join()
                .map_err(|_| io::Error::other("协议 I/O 线程异常退出"))?;
        }
        Ok(())
    }
}
impl Drop for Transport {
    /// 失败和 panic 同样不能留下持有 stdio 的线程。
    fn drop(&mut self) {
        let _ = self.finish();
    }
}

/// 接收方停止时有限队列不能卡住生产者。
fn emit(sender: &SyncSender<Event>, stop: &AtomicBool, mut event: Event) -> bool {
    while !stop.load(Ordering::Acquire) {
        match sender.try_send(event) {
            Ok(()) => return true,
            Err(mpsc::TrySendError::Disconnected(_)) => return false,
            Err(mpsc::TrySendError::Full(value)) => {
                event = value;
                std::thread::sleep(POLL);
            }
        }
    }
    false
}

/// Unix 借用句柄设置非阻塞；管道所有权仍由读取或写入线程持有。
#[cfg(unix)]
pub(super) fn nonblocking(pipe: &impl std::os::fd::AsRawFd) -> io::Result<()> {
    let fd = pipe.as_raw_fd();
    // SAFETY: FD 来自存活的自有管道，仅追加非阻塞标志。
    let flags = unsafe { libc::fcntl(fd, libc::F_GETFL) };
    if flags < 0 || unsafe { libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) } < 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

/// Windows 保留同步管道，在 owner 清理时用 CancelSynchronousIo 解阻。
#[cfg(windows)]
pub(super) fn nonblocking<T>(_: &T) -> io::Result<()> {
    Ok(())
}

/// 序列化后添加唯一换行；结果超过限制时必须失败，不能截断图。
pub(super) fn encode(value: &serde_json::Value) -> Result<Vec<u8>, Failure> {
    let mut bytes = serde_json::to_vec(value)
        .map_err(|_| Failure::new("internal_failure", "结果序列化失败"))?;
    if bytes.len() > MAX_BYTES {
        return Err(Failure::new("resource_limit", "结果超过 128 MiB"));
    }
    bytes.push(b'\n');
    Ok(bytes)
}

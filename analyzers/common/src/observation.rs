//! 仅验收显式启用的只写观察；不改变任一 NDJSON 协议，也不暂停或控制分析。
use std::{
    cell::Cell,
    fs, io,
    path::{Path, PathBuf},
};

pub const VARIABLE: &str = "CODEGRAPH_TEST_DISCOVERY_PROBE";

/// Host 只批准临时目录中新建的独立观察位置，规范化父目录后再传给引擎。
pub fn destination(path: &Path, repository: &Path, packages: &[PathBuf]) -> io::Result<PathBuf> {
    let invalid = || io::Error::other("无效的独立验收观察目录");
    if !path.is_absolute() || path.file_name().is_none_or(|name| name != "reached.json") {
        return Err(invalid());
    }
    let parent = path.parent().ok_or_else(invalid)?;
    if fs::symlink_metadata(parent)?.file_type().is_symlink() {
        return Err(invalid());
    }
    let parent = parent.canonicalize()?;
    let temporary = std::env::temp_dir().canonicalize()?;
    if !parent.starts_with(temporary)
        || !parent
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.starts_with("codegraph-discovery-probe-"))
        || fs::read_dir(&parent)?.next().is_some()
    {
        return Err(invalid());
    }
    for protected in std::iter::once(repository).chain(packages.iter().map(PathBuf::as_path)) {
        let protected = protected.canonicalize()?;
        if parent.starts_with(&protected) || protected.starts_with(&parent) {
            return Err(invalid());
        }
    }
    let path = parent.join("reached.json");
    match fs::symlink_metadata(&path) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(path),
        _ => Err(invalid()),
    }
}

/// 一次真实请求的一次观察；缺少环境时构造和记录均不访问文件系统。
pub struct Observation {
    path: PathBuf,
    helper: String,
    request: String,
    recorded: Cell<bool>,
}

impl Observation {
    /// 身份取自当前真实请求和固定引擎，目的路径只由 Host 的验收通道提供。
    pub fn from_environment(helper: &str, request: &str) -> Option<Self> {
        std::env::var_os(VARIABLE).map(|path| Self {
            path: path.into(),
            helper: helper.into(),
            request: request.into(),
            recorded: Cell::new(false),
        })
    }

    /// 在调用点完成实际读取后原子发布有界标记；硬链接绝不覆盖已有目标或符号链接。
    pub fn record(&self, milestone: &str) -> io::Result<()> {
        if self.recorded.get() {
            return Ok(());
        }
        let bytes = serde_json::to_vec(&serde_json::json!({
            "schema_version": 1, "helper_id": self.helper, "request_id": self.request,
            "pid": std::process::id(), "milestone": milestone,
        }))?;
        if bytes.len() > 4096 {
            return Err(io::Error::other("验收观察超过限额"));
        }
        let pending = self.path.with_extension("pending");
        fs::create_dir(&pending)?;
        let temporary = pending.join("marker.json");
        let result = (|| {
            use std::io::Write;
            let mut file = fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&temporary)?;
            file.write_all(&bytes)?;
            drop(file);
            fs::hard_link(&temporary, &self.path)
        })();
        let _ = fs::remove_dir_all(&pending);
        result?;
        self.recorded.set(true);
        Ok(())
    }
}

#[cfg(test)]
#[path = "tests/observation.rs"]
mod tests;

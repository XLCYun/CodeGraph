//! 非交付测试调用方，直接运行公开入口并拥有整个进程树。
use crate::{
    child_process::ProcessTree,
    code_graph::{AnalysisError, AnalysisInput, CodeGraph, LanguageAnalyzer},
};
use std::{
    collections::BTreeMap,
    ffi::OsString,
    io::{Read, Write},
    path::PathBuf,
    process::{Command, Stdio},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::{Duration, Instant},
};

/// 测试调用方期限，与分析器内部实现无关。
pub struct AnalysisLimits {
    pub analysis_timeout: Duration,
}
impl Default for AnalysisLimits {
    /// 保持充分的真实加载时间，避免正常大型语义夹具误超时。
    fn default() -> Self {
        Self {
            analysis_timeout: Duration::from_secs(120),
        }
    }
}
/// 固定程序、公开选项和调用方进程树策略，不包含私有协议字段。
pub struct PublicAnalyzer {
    pub executable: PathBuf,
    pub environment: BTreeMap<OsString, OsString>,
    pub scope: &'static str,
    pack: &'static str,
    limits: AnalysisLimits,
    cancellation: Option<Arc<dyn Fn() -> bool + Send + Sync>>,
}
impl PublicAnalyzer {
    /// 包位置由验收配置指定；不读取项目内的命令或 SDK 路径。
    pub fn new(pack: &'static str) -> Self {
        let artifacts = PathBuf::from(
            std::env::var_os("CODEGRAPH_NATIVE_PACKS").expect("CODEGRAPH_NATIVE_PACKS required"),
        );
        let target = if cfg!(windows) {
            "x86_64-pc-windows-msvc"
        } else {
            "aarch64-apple-darwin"
        };
        let suffix = if cfg!(windows) { ".exe" } else { "" };
        let executable = artifacts.join(format!(
            "smoke-installed/{pack}/0.1.2/{target}/bin/codegraph-{pack}{suffix}"
        ));
        assert!(
            executable.is_file(),
            "Missing public entry: {}",
            executable.display()
        );
        let mut environment: BTreeMap<OsString, OsString> = std::env::vars_os()
            .filter(|(key, _)| {
                !key.to_string_lossy()
                    .to_uppercase()
                    .starts_with("CODEGRAPH_")
            })
            .collect();
        environment.remove(std::ffi::OsStr::new("RUSTUP_TOOLCHAIN"));
        if pack == "rust" {
            environment.insert(
                "RUSTUP_TOOLCHAIN".into(),
                std::env::var_os("CODEGRAPH_PROJECT_RUST_TOOLCHAIN")
                    .unwrap_or_else(|| "stable".into()),
            );
        }
        Self {
            executable,
            environment,
            scope: "repository-only",
            pack,
            limits: Default::default(),
            cancellation: None,
        }
    }
    /// 只改变调用方期限，绝不编码私有 max_result 或 SDK 输入。
    pub fn with_limits(mut self, limits: AnalysisLimits) -> Self {
        self.limits = limits;
        self
    }
    /// 外部观察器在主动终止进程树后得到取消错误。
    pub fn with_cancellation(mut self, cancellation: Arc<dyn Fn() -> bool + Send + Sync>) -> Self {
        self.cancellation = Some(cancellation);
        self
    }
}
/// 排空实际管道并只保存有界前缀，避免 stdout/stderr 背压死锁。
pub(crate) fn drain(mut reader: impl Read, maximum: usize, overflow: Arc<AtomicBool>) -> Vec<u8> {
    let mut bytes = Vec::new();
    let mut chunk = [0; 8192];
    loop {
        let count = reader.read(&mut chunk).unwrap();
        if count == 0 {
            break;
        }
        let remaining = maximum.saturating_sub(bytes.len());
        bytes.extend_from_slice(&chunk[..count.min(remaining)]);
        if count > remaining {
            overflow.store(true, Ordering::Release);
        }
    }
    bytes
}
impl LanguageAnalyzer for PublicAnalyzer {
    /// 一个完整 UTF-8 JSON 后关闭 stdin，排空输出并同时核对结果、身份与退出状态。
    fn analyze(&self, input: &AnalysisInput) -> Result<CodeGraph, AnalysisError> {
        let request = serde_json::json!({"protocol":"codegraph.analyzer/1","type":"analyze","id":"public-integration","repository_root":input.repository_root,"options":{"include_tests":input.options.include_tests,"rust_dependency_scope":self.scope}});
        let mut request = request.as_object().unwrap().clone();
        if let Some(files) = &input.files {
            request.insert(
                "files".into(),
                files
                    .iter()
                    .map(|file| {
                        serde_json::Value::String(file.to_string_lossy().replace('\\', "/"))
                    })
                    .collect(),
            );
        }
        let mut command = Command::new(&self.executable);
        command
            .arg("analyze")
            .current_dir(std::env::temp_dir())
            .env_clear()
            .envs(&self.environment)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let mut tree = ProcessTree::spawn(&mut command).unwrap();
        let exceeded = Arc::new(AtomicBool::new(false));
        let stdout = tree.take_stdout().unwrap();
        let stderr = tree.take_stderr().unwrap();
        let flag = exceeded.clone();
        let output = std::thread::spawn(move || drain(stdout, 128 * 1024 * 1024, flag));
        let diagnostic =
            std::thread::spawn(move || drain(stderr, 256 * 1024, Arc::new(AtomicBool::new(false))));
        let mut stdin = tree.take_stdin().unwrap();
        stdin
            .write_all(&serde_json::to_vec(&request).unwrap())
            .unwrap();
        drop(stdin);
        let started = Instant::now();
        let reason = loop {
            if exceeded.load(Ordering::Acquire) {
                break Some(AnalysisError::new("Public output exceeds limit"));
            }
            if let Some(status) = tree.status().unwrap() {
                break Some(AnalysisError::new(format!("exit:{status}")));
            }
            if self.cancellation.as_ref().is_some_and(|cancel| cancel()) {
                break Some(AnalysisError {
                    message: "调用方终止进程树".into(),
                    code: None,
                    diagnostic: String::new(),
                    cancelled: true,
                });
            }
            if started.elapsed() >= self.limits.analysis_timeout {
                break Some(AnalysisError::new("调用方分析超时"));
            }
            std::thread::sleep(Duration::from_millis(5));
        };
        let status = tree.status().unwrap();
        tree.reap().unwrap();
        let bytes = output.join().unwrap();
        let stderr = diagnostic.join().unwrap();
        if status.is_none() {
            return Err(reason.unwrap());
        }
        let status = status.unwrap();
        decode_final(
            &bytes,
            status,
            &stderr,
            self.pack,
            &serde_json::Value::Object(request),
        )
    }
}

/// 与真实进程调用共用最终响应断言，便于精确验证非交付测试调用方的身份政策。
fn decode_final(
    bytes: &[u8],
    status: std::process::ExitStatus,
    stderr: &[u8],
    pack: &str,
    request: &serde_json::Value,
) -> Result<CodeGraph, AnalysisError> {
    let result: serde_json::Value = serde_json::from_slice(&bytes).unwrap_or_else(|error| {
        panic!(
            "Missing/malformed public result: {error}; {}",
            String::from_utf8_lossy(&stderr)
        )
    });
    assert!(bytes.ends_with(b"\n"));
    assert_eq!(result["protocol"], "codegraph.analyzer/1");
    assert_eq!(result["engine_version"], "0.1.2");
    assert_eq!(result["pack_id"], pack);
    assert_eq!(result["type"], "completed");
    assert!(
        result.get("id").is_some(),
        "Public final result is missing its id"
    );
    let early_error = result["outcome"] == "error"
        && matches!(
            result["error"]["code"].as_str(),
            Some("invalid_request" | "incompatible_protocol")
        );
    let valid_id = request["id"].as_str().is_some_and(|id| {
        !id.contains('\0')
            && id.len() <= 1024
            && id.chars().any(|ch| !ch.is_whitespace() && ch != '\u{feff}')
    });
    if early_error {
        assert!(
            result["id"].is_null() || valid_id && result["id"] == request["id"],
            "Rejected request returned an unaccepted or mismatched id"
        );
    } else {
        assert_eq!(
            result["id"], request["id"],
            "Accepted analysis returned a mismatched id"
        );
    }
    if result["outcome"] == "error" {
        assert!(!status.success());
        assert!(result.get("graph").is_none());
        // 公共说明可以概括诊断；旧语义断言继续核对同一实际程序写到 stderr 的细节。
        return Err(AnalysisError {
            message: result["error"]["message"].as_str().unwrap().to_owned(),
            code: Some(result["error"]["code"].as_str().unwrap().to_owned()),
            diagnostic: String::from_utf8_lossy(&stderr).into_owned(),
            cancelled: false,
        });
    }
    assert!(
        status.success(),
        "{status}; {result}; {}",
        String::from_utf8_lossy(&stderr)
    );
    assert_eq!(result["outcome"], "ok");
    Ok(CodeGraph::decode(result["graph"].clone()))
}

#[path = "tests/public.rs"]
mod tests;

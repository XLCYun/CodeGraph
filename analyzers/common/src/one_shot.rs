//! 单进程读取完整 JSON 至 EOF，再直接执行一次语言分析并输出一个最终响应。
use crate::{
    model::CodeGraph,
    public_protocol::{self, CheckedRequest, Failure, MAX_BYTES},
};
use serde_json::{Value, json};
use std::io::{Read, Write};

/// 固定语言入口统一处理帮助和公共传输，不建立控制线程、worker 或常驻会话。
pub fn run(
    pack_id: &'static str,
    analyze: impl FnOnce(&CheckedRequest) -> anyhow::Result<CodeGraph>,
) -> i32 {
    let args = std::env::args_os().skip(1).collect::<Vec<_>>();
    match args
        .iter()
        .map(|s| s.to_str())
        .collect::<Vec<_>>()
        .as_slice()
    {
        [Some("--help")] | [Some("analyze"), Some("--help")] => {
            println!(
                "codegraph-{pack_id} {}\nUsage: codegraph-{pack_id} analyze\nRead one UTF-8 JSON request followed by EOF; write one final JSON result.\nOptions: --help, --version",
                public_protocol::ENGINE_VERSION
            );
            return 0;
        }
        [Some("--version")] => {
            println!("codegraph-{pack_id} {}", public_protocol::ENGINE_VERSION);
            return 0;
        }
        [Some("analyze")] => {}
        _ => {
            eprintln!("Usage: codegraph-{pack_id} analyze | --help | --version");
            return 2;
        }
    }
    let mut bytes = Vec::new();
    let decoded = std::io::stdin()
        .lock()
        .take((MAX_BYTES + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|_| Failure::new("io_failure", "无法读取请求"))
        .and_then(|_| public_protocol::decode(&bytes));
    let mut id = None;
    let result = decoded.and_then(|request| {
        id = Some(request.id.clone());
        let executable =
            std::env::current_exe().map_err(|_| Failure::new("invalid_pack", "无法定位分析器"))?;
        let checked = public_protocol::check(request, &executable)?;
        // 显式空选集仅经过公共边界；不读取配置、不探测 SDK，也不进入语言核心。
        if checked.request.files.as_ref().is_some_and(Vec::is_empty) {
            return Ok(CodeGraph::empty());
        }
        analyze(&checked).map_err(|error| {
            eprintln!("{error:#}");
            Failure::new(
                "analysis_failed",
                "语言分析、来源或图校验失败；请检查源码和项目目标工具链",
            )
        })
    });
    let (mut value, mut code) = match result {
        Ok(graph) => (
            json!({"protocol":public_protocol::PROTOCOL,"engine_version":public_protocol::ENGINE_VERSION,"pack_id":pack_id,"type":"completed","id":id,"outcome":"ok","graph":graph}),
            0,
        ),
        Err(error) => (failed(pack_id, id.as_deref(), error), 1),
    };
    if serde_json::to_vec(&value).map_or(true, |bytes| bytes.len() >= MAX_BYTES) {
        value = failed(
            pack_id,
            id.as_deref(),
            Failure::new("resource_limit", "分析结果超过字节上限"),
        );
        code = 1;
    }
    if let Err(error) = send(&value) {
        eprintln!("结果写入失败：{error}");
        return 1;
    }
    code
}

/// 每个失败同样绑定编译期身份，非法输入无法覆盖版本和语言。
fn failed(pack_id: &str, id: Option<&str>, error: Failure) -> Value {
    json!({"protocol":public_protocol::PROTOCOL,"engine_version":public_protocol::ENGINE_VERSION,"pack_id":pack_id,"type":"completed","id":id,"outcome":"error","error":error})
}

/// 只发送一个完整 JSON 与换行，普通 EOF 不改变分析或输出生命周期。
fn send(value: &Value) -> std::io::Result<()> {
    let mut output = std::io::stdout().lock();
    serde_json::to_writer(&mut output, value)?;
    writeln!(output)?;
    output.flush()
}

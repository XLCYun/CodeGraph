//! 公共 NDJSON schema。所有消费者都只依赖这个协议，不接触 helper 封套。
use crate::{code_graph::RustDependencyScope, strict_json::UniqueValue};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::PathBuf;

pub(crate) const PROTOCOL: &str = "codegraph.analyzer/1";
pub(crate) const MAX_BYTES: usize = 128 * 1024 * 1024;

/// 一次分析的明确选项；不允许环境、helper 命令或 SDK 覆盖。
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Options {
    pub include_tests: bool,
    pub rust_dependency_scope: RustDependencyScope,
}

/// 消费者固定源码状态，选集使用仓库相对 POSIX 路径。
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Analyze {
    pub protocol: String,
    #[serde(rename = "type")]
    pub kind: String,
    pub id: String,
    pub repository_root: PathBuf,
    #[serde(
        default,
        deserialize_with = "crate::selection::deserialize_files",
        skip_serializing_if = "Option::is_none"
    )]
    pub files: Option<Vec<String>>,
    pub options: Options,
}

/// 取消只能作用于当前唯一请求。
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Cancel {
    pub protocol: String,
    #[serde(rename = "type")]
    pub kind: String,
    pub id: String,
}

/// 解码后的消息由会话状态机检查顺序和请求 ID。
pub(crate) enum Message {
    Analyze(Analyze),
    Cancel(Cancel),
}

/// 错误类别可供程序稳定分支；说明不包含内部路径或 helper evidence。
#[derive(Clone, Debug, Serialize)]
pub(crate) struct Failure {
    pub code: &'static str,
    pub message: String,
}
impl Failure {
    /// 构造已选择类别的公共错误。
    pub(crate) fn new(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }
}

/// 先递归检查重复键，再执行每层 schema 校验；保持 JSON 默认嵌套深度上限。
pub(crate) fn decode(bytes: &[u8]) -> Result<Message, Failure> {
    let invalid = || Failure::new("invalid_request", "请求 JSON、字段或路径无效");
    let UniqueValue(value) = serde_json::from_slice(bytes).map_err(|_| invalid())?;
    if value.get("protocol").and_then(Value::as_str) != Some(PROTOCOL) {
        return Err(Failure::new(
            "incompatible_protocol",
            "不支持的 CodeGraph 协议版本",
        ));
    }
    let id = value
        .get("id")
        .and_then(Value::as_str)
        .ok_or_else(invalid)?;
    if id.trim().is_empty() || id.len() > 1024 || id.contains('\0') {
        return Err(invalid());
    }
    match value.get("type").and_then(Value::as_str) {
        Some("analyze") => {
            let request: Analyze = serde_json::from_value(value).map_err(|_| invalid())?;
            if !request.repository_root.is_absolute()
                || request
                    .repository_root
                    .to_str()
                    .is_none_or(|s| s.contains('\0'))
            {
                return Err(invalid());
            }
            let mut seen = std::collections::BTreeSet::new();
            for file in request.files.iter().flatten() {
                if file.contains(['\0', '\\', ':'])
                    || file
                        .split('/')
                        .any(|s| s.is_empty() || s == "." || s == "..")
                    || !seen.insert(file)
                {
                    return Err(invalid());
                }
            }
            Ok(Message::Analyze(request))
        }
        Some("cancel") => Ok(Message::Cancel(
            serde_json::from_value(value).map_err(|_| invalid())?,
        )),
        _ => Err(invalid()),
    }
}

/// 公共失败封套；无法可靠取得请求身份时显式返回 null。
pub(crate) fn failed(id: Option<&str>, failure: &Failure) -> Value {
    serde_json::json!({"protocol":PROTOCOL,"type":"completed","id":id,"outcome":"error","error":failure})
}

#[cfg(test)]
#[path = "tests/protocol.rs"]
mod tests;

//! NDJSON 消息的严格 schema 与递归重复键检查。

use super::{failure, Result};
use crate::strict_json::UniqueValue;
use serde::Deserialize;
use serde_json::Value;
use std::collections::BTreeMap;

pub(super) const PROTOCOL: &str = "codegraph.engine/1";
pub(super) const REQUEST_ID: &str = "analysis-1";

/// ready 的版本、语言及政策声明，不接受任何未定义字段。
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Ready {
    pub protocol: String,
    #[serde(rename = "type")]
    pub message_type: String,
    pub helper_id: String,
    pub helper_version: String,
    pub projection_revision: String,
    pub languages: Vec<String>,
    pub profiles: Vec<String>,
}

/// 已验证成功分支的候选数据，尚未经过公共来源及图检查。
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Completed {
    pub protocol: String,
    #[serde(rename = "type")]
    pub message_type: String,
    pub id: String,
    pub outcome: String,
    pub graph: Graph,
    pub evidence: Evidence,
}

/// 后端明确失败分支，不能混入成功数据。
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Failed {
    pub protocol: String,
    #[serde(rename = "type")]
    pub message_type: String,
    pub id: String,
    pub outcome: String,
    pub error: BackendError,
}

/// 后端明确取消分支，必须由 Host 先记录取消原因。
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Cancelled {
    pub protocol: String,
    #[serde(rename = "type")]
    pub message_type: String,
    pub id: String,
    pub outcome: String,
}

/// helper 返回的错误只能使用公共约定的错误码和仓库相对路径。
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct BackendError {
    pub code: String,
    pub message: String,
    pub paths: Vec<String>,
}

/// 图构造数据只有虚拟根子对象和节点集合。
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Graph {
    pub root_contains: Vec<String>,
    pub nodes: Vec<Node>,
    pub external_symbols: Vec<ExternalSymbol>,
}

/// 每个最终 External 目标的显示名称及真实来源范围。
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ExternalSymbol {
    pub symbol: String,
    pub name: String,
    pub scope: ExternalScope,
}

/// 来源由 helper 的解析证据确定，Host 不通过不透明 ID 猜测。
#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum ExternalScope {
    Repository,
    Dependency,
}

/// 显式可空字段的包装，缺少字段与发送 null 不等价。
#[derive(Deserialize)]
#[serde(transparent)]
pub(super) struct Nullable(pub Option<String>);

/// 六种公共类别只由 category 字段决定。
#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum Category {
    Container,
    Type,
    Callable,
    Value,
    ExecutionRegion,
    Extension,
}

/// 节点的每个字段必须出现，extensions 只保留语言语义。
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Node {
    pub category: Category,
    pub id: String,
    pub language: String,
    pub native_kind: String,
    pub name: Nullable,
    pub signature: Nullable,
    pub code: Nullable,
    pub source: Nullable,
    pub contains: Vec<String>,
    pub references: Vec<Reference>,
    pub extensions: BTreeMap<String, Value>,
}

/// 关系事实由语义和确切目标构成。
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Reference {
    pub semantic: String,
    pub target: Target,
}

/// 互斥目标分支不能混用 id、symbol 和 name。
#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub(super) enum Target {
    Node { id: String },
    External { symbol: String },
    Unresolved { name: String },
}

/// 来源控制元数据不进入 CodeGraph。
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Evidence {
    pub configurations: Vec<Configuration>,
    pub files: Vec<FileEvidence>,
    pub origins: Vec<Origin>,
}

/// 一次实际分析配置的静态来源和目标平台。
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Configuration {
    pub id: String,
    pub source_paths: Vec<String>,
    pub platform_ids: Vec<String>,
}

/// 每个请求文件都必须有覆盖记录。
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct FileEvidence {
    pub path: String,
    pub disposition: Disposition,
    pub configuration_ids: Vec<String>,
}

/// 测试排除和非活动文件不能用于节点来源。
#[derive(Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(super) enum Disposition {
    Analyzed,
    ExcludedTest,
    Inactive,
}

/// 每个节点的真实选集出处，允许多个文件共同支持逻辑容器。
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Origin {
    pub node_id: String,
    pub selected_paths: Vec<String>,
}

/// 可接受消息由阶段逻辑进一步限制。
pub(super) enum Message {
    Ready(Ready),
    Ok(Completed),
    Error(Failed),
    Cancelled(Cancelled),
}

/// 严格 JSON 解码后按 type/outcome 选择互斥 schema，拒绝额外字段。
pub(super) fn decode(bytes: &[u8]) -> Result<Message> {
    let UniqueValue(value) = serde_json::from_slice(bytes).map_err(failure)?;
    if value.get("protocol").and_then(Value::as_str) != Some(PROTOCOL) {
        return Err(failure("不支持的 helper 协议"));
    }
    let message = match value.get("type").and_then(Value::as_str) {
        Some("ready") => Message::Ready(serde_json::from_value(value).map_err(failure)?),
        Some("completed") => match value.get("outcome").and_then(Value::as_str) {
            Some("ok") => {
                if let Some(nodes) = value.pointer("/graph/nodes").and_then(Value::as_array) {
                    for node in nodes {
                        for key in ["name", "signature", "code", "source"] {
                            if node.get(key).is_none() {
                                return Err(failure(format!("节点缺少必需字段 {key}")));
                            }
                        }
                    }
                }
                Message::Ok(serde_json::from_value(value).map_err(failure)?)
            }
            Some("error") => Message::Error(serde_json::from_value(value).map_err(failure)?),
            Some("cancelled") => {
                Message::Cancelled(serde_json::from_value(value).map_err(failure)?)
            }
            _ => return Err(failure("未知 completed outcome")),
        },
        _ => return Err(failure("未知 helper 消息类型")),
    };
    Ok(message)
}

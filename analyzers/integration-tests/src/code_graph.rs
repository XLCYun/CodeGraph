//! 只读公共结果视图；仅反序列化和建索引，不改写或补全任何图数据。
use std::{collections::BTreeMap, path::PathBuf};
#[path = "node.rs"]
mod node;
pub use node::{CodeCategory, CodeNode, CodeNodeData, NodeId, ReferenceTarget};

/// 选集与测试开关原样编码到公共请求。
#[derive(Default)]
pub struct AnalysisOptions {
    pub include_tests: bool,
}
/// 临时源码目录和调用方指定的精确选集。
pub struct AnalysisInput {
    pub repository_root: PathBuf,
    pub files: Option<Vec<PathBuf>>,
    pub options: AnalysisOptions,
}
/// 测试专用的直接调用接口，不向产品或下游公开。
pub trait LanguageAnalyzer {
    fn analyze(&self, input: &AnalysisInput) -> Result<CodeGraph, AnalysisError>;
}
/// 调用方失败视图，保留公共错误码、实际说明与 stderr，不推断语言诊断。
#[derive(Debug)]
pub struct AnalysisError {
    pub message: String,
    pub code: Option<String>,
    pub diagnostic: String,
    pub cancelled: bool,
}
impl AnalysisError {
    /// 保留原始公共错误或运输故障。
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            code: None,
            diagnostic: String::new(),
            cancelled: false,
        }
    }
    /// 测试调用方确已终止整树才标记取消。
    pub fn is_cancelled(&self) -> bool {
        self.cancelled
    }
}
impl std::fmt::Display for AnalysisError {
    /// 展示实际诊断，帮助定位语义回归。
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)?;
        if !self.diagnostic.is_empty() {
            write!(f, "\n{}", self.diagnostic)?;
        }
        Ok(())
    }
}
/// 保持旧断言的根只读 API；contains 的成员与顺序来自公共 JSON。
#[derive(Debug, Default, PartialEq, Eq)]
pub struct CodeRoot {
    pub contains: Vec<NodeId>,
}
/// 图外说明原样保存，symbol 仅作为索引键。
#[derive(Debug, PartialEq, Eq, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExternalSymbol {
    pub name: String,
    pub scope: SymbolScope,
}
/// 公共图外来源分类，不推断或重新分类。
#[derive(Debug, PartialEq, Eq, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SymbolScope {
    Repository,
    Dependency,
}
/// 公共图数据的只读索引，不含构图、来源发现或修复功能。
#[derive(Debug)]
pub struct CodeGraph {
    root: CodeRoot,
    nodes: BTreeMap<NodeId, CodeNode>,
    external_symbols: BTreeMap<String, ExternalSymbol>,
}

impl<'de> serde::Deserialize<'de> for ReferenceTarget {
    /// 仅将公开 tagged target 对应到测试枚举，字符串保持原字节。
    fn deserialize<D: serde::Deserializer<'de>>(decoder: D) -> Result<Self, D::Error> {
        #[derive(serde::Deserialize)]
        #[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
        enum Target {
            Node { id: NodeId },
            External { symbol: String },
            Unresolved { name: String },
        }
        Ok(
            match <Target as serde::Deserialize>::deserialize(decoder)? {
                Target::Node { id } => Self::Node(id),
                Target::External { symbol } => Self::External { symbol },
                Target::Unresolved { name } => Self::Unresolved { name },
            },
        )
    }
}
impl CodeGraph {
    /// 只消费原生程序的公共 graph，重复身份显式失败，不自动去重。
    pub fn decode(value: serde_json::Value) -> Self {
        let mut graph = value.as_object().unwrap().clone();
        assert_eq!(graph.len(), 3);
        let root = CodeRoot {
            contains: serde_json::from_value(graph.remove("root_contains").unwrap()).unwrap(),
        };
        let mut nodes = BTreeMap::new();
        for value in graph.remove("nodes").unwrap().as_array().unwrap() {
            let mut fields = value.as_object().unwrap().clone();
            let category = fields.remove("category").unwrap();
            let data: CodeNodeData = serde_json::from_value(fields.into()).unwrap();
            let node = match category.as_str().unwrap() {
                "container" => CodeNode::Container(data),
                "type" => CodeNode::Type(data),
                "callable" => CodeNode::Callable(data),
                "value" => CodeNode::Value(data),
                "execution_region" => CodeNode::ExecutionRegion(data),
                "extension" => CodeNode::Extension(data),
                other => panic!("Unknown public category: {other}"),
            };
            assert!(
                nodes.insert(node.data().id.clone(), node).is_none(),
                "Duplicate public node"
            );
        }
        let mut external_symbols = BTreeMap::new();
        for value in graph
            .remove("external_symbols")
            .unwrap()
            .as_array()
            .unwrap()
        {
            let mut fields = value.as_object().unwrap().clone();
            let symbol = fields
                .remove("symbol")
                .unwrap()
                .as_str()
                .unwrap()
                .to_owned();
            assert!(
                external_symbols
                    .insert(symbol, serde_json::from_value(fields.into()).unwrap())
                    .is_none(),
                "Duplicate external symbol"
            );
        }
        Self {
            root,
            nodes,
            external_symbols,
        }
    }
    /// 读取原始根成员，不补充可达性或丢弃孤立节点。
    pub fn root(&self) -> &CodeRoot {
        &self.root
    }
    /// 按原始身份查找节点，不重新编码 ID。
    pub fn node(&self, id: &NodeId) -> Option<&CodeNode> {
        self.nodes.get(id)
    }
    /// 返回原始图外说明的索引。
    pub fn external_symbols(&self) -> &BTreeMap<String, ExternalSymbol> {
        &self.external_symbols
    }
}

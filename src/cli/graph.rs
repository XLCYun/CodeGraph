//! 只序列化经过 Host 验证的图；内部来源证据、SDK 和 scratch 不进入结果。
use crate::code_graph::{CodeCategory, CodeGraph, ReferenceTarget, SymbolScope};
use serde_json::{json, Value};

/// 保留 helper 的节点字段及六类语义，稳定排序来自已校验的节点索引。
pub(crate) fn value(graph: &CodeGraph) -> Value {
    let nodes: Vec<_> = graph.nodes().map(|node| {
        let data = node.data();
        let category = match node.category() {
            CodeCategory::Container => "container", CodeCategory::Type => "type",
            CodeCategory::Callable => "callable", CodeCategory::Value => "value",
            CodeCategory::ExecutionRegion => "execution_region", CodeCategory::Extension => "extension",
        };
        let references: Vec<_> = data.references.iter().map(|reference| {
            let target = match &reference.target {
                ReferenceTarget::Node(id) => json!({"kind":"node","id":id.0}),
                ReferenceTarget::External { symbol } => json!({"kind":"external","symbol":symbol}),
                ReferenceTarget::Unresolved { name } => json!({"kind":"unresolved","name":name}),
            };
            json!({"semantic":reference.semantic,"target":target})
        }).collect();
        json!({"category":category,"id":data.id.0,"language":data.language.0,"native_kind":data.native_kind,
            "name":data.name,"signature":data.signature,"code":data.code,"source":data.source,
            "contains":data.contains.iter().map(|id| &id.0).collect::<Vec<_>>(),"references":references,"extensions":data.extensions})
    }).collect();
    let symbols: Vec<_> = graph.external_symbols().iter().map(|(symbol, data)| json!({"symbol":symbol,"name":data.name,"scope": match data.scope { SymbolScope::Repository => "repository", SymbolScope::Dependency => "dependency" }})).collect();
    json!({"root_contains":graph.root().contains.iter().map(|id| &id.0).collect::<Vec<_>>(),"nodes":nodes,"external_symbols":symbols})
}

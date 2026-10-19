//! 多语言代码图的生成与校验；差异计算由消费者负责。

mod analyzer;
mod error;
mod graph;
mod node;
mod validation;

pub use analyzer::{AnalysisInput, AnalysisOptions, LanguageAnalyzer, RustDependencyScope};
pub use error::{AnalysisError, InvalidGraph};
pub use graph::{CodeGraph, CodeRoot, ExternalSymbol, SymbolScope};
pub use node::{
    CodeCategory, CodeNode, CodeNodeData, CodeReference, LanguageId, NodeId, ReferenceTarget,
};

#[cfg(test)]
#[path = "tests/mod.rs"]
mod tests;

#[cfg(test)]
pub use node::ExtensionFields;

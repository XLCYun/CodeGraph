//! 完整 factory、目标 SDK 和 Host 调用都在受监督的私有工作进程中执行。
use super::{
    graph,
    protocol::{Analyze, Failure, PROTOCOL},
};
use crate::{
    code_graph::{AnalysisInput, AnalysisOptions, CodeGraph, LanguageAnalyzer},
    native_analyzer::CancellationCheck,
    native_languages::{from_pack, RustPackAnalyzer},
    pack::{self, PackId},
};
use std::path::Path;

/// 执行一次经过公共契约验证的请求，成功数据只含完整已验证图。
pub(super) fn analyze(
    pack: PackId,
    root: &Path,
    request: &Analyze,
    cancelled: CancellationCheck,
) -> Result<serde_json::Value, Failure> {
    pack::validate(pack, root).map_err(|message| Failure::new("invalid_pack", message))?;
    let repository = request
        .repository_root
        .canonicalize()
        .map_err(|_| Failure::new("invalid_input", "仓库目录不可读取"))?;
    let pack_root = root
        .canonicalize()
        .map_err(|_| Failure::new("invalid_pack", "支持包目录不可读取"))?;
    if !repository.is_dir()
        || repository.starts_with(&pack_root)
        || pack_root.starts_with(&repository)
    {
        return Err(Failure::new(
            "invalid_input",
            "仓库和受信支持包必须是独立目录",
        ));
    }
    let input = AnalysisInput {
        repository_root: repository,
        files: request.files.iter().map(Into::into).collect(),
        options: AnalysisOptions {
            include_tests: request.options.include_tests,
        },
    };
    let graph = if cancelled() {
        return Ok(
            serde_json::json!({"protocol":PROTOCOL,"type":"completed","id":request.id,"outcome":"cancelled"}),
        );
    } else if input.files.is_empty() {
        // 空语言组选集无需发现 SDK 或启动 helper；不是分析失败的降级路径。
        CodeGraph::new(Default::default(), vec![])
            .map_err(|_| Failure::new("internal_failure", "空图校验失败"))?
    } else {
        let result = if pack == PackId::Rust {
            RustPackAnalyzer::from_pack(root).and_then(|analyzer| {
                analyzer.analyze(
                    &input,
                    cancelled.clone(),
                    request.options.rust_dependency_scope,
                )
            })
        } else {
            from_pack(pack, root).and_then(|analyzer| {
                analyzer
                    .with_cancellation(cancelled.clone())
                    .analyze(&input)
            })
        };
        match result {
            Ok(graph) => graph,
            Err(error) if error.is_cancelled() => {
                return Ok(
                    serde_json::json!({"protocol":PROTOCOL,"type":"completed","id":request.id,"outcome":"cancelled"}),
                )
            }
            // 内部错误可能包含 SDK、scratch 和编译器诊断；公共说明不泄露这些路径。
            Err(_) => {
                return Err(Failure::new(
                    "analysis_failed",
                    "语言分析、来源验证或图校验失败；请检查支持包、源码和项目目标工具链",
                ))
            }
        }
    };
    Ok(
        serde_json::json!({"protocol":PROTOCOL,"type":"completed","id":request.id,"outcome":"ok","graph":graph::value(&graph)}),
    )
}

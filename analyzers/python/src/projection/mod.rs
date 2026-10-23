//! Python 对象和运行时关系政策迁自既有实现，原文/AST/导航全部来自固定原生数据库。
mod bindings;
mod discovery;
mod fields;
mod known;
mod project;
mod references;
mod syntax;

use crate::protocol::Request;
use anyhow::{Context, Result};
use codegraph_engine_common::graph::{Configuration, Disposition, Evidence, Origin, Projection};
use codegraph_engine_common::model::{CodeGraph, NodeId, ReferenceTarget};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
    time::{Duration, Instant},
};

/// 保留可读语言上下文，提取失败不变成空图。
fn failure(message: impl std::fmt::Display) -> anyhow::Error {
    anyhow::anyhow!("Python 结构分析：{message}")
}

/// 文件级测试约定只观察本仓库相对路径，实际路径也须核验。
fn is_test(relative: &Path) -> bool {
    relative
        .components()
        .any(|part| part.as_os_str() == "tests")
        || relative
            .file_stem()
            .and_then(|value| value.to_str())
            .is_some_and(|name| {
                name.starts_with("test_") || name.ends_with("_test") || name == "conftest"
            })
}

/// 每侧按原有配置/PEP723/版本组合加载独立数据库；上下文不自动变成图成员。
pub fn build(request: &Request) -> Result<Projection> {
    request.validate()?;
    let repository = request.input.repository_root.canonicalize()?;
    let deadline = Instant::now()
        .checked_add(Duration::from_millis(request.limits.analysis_timeout_ms))
        .context("Python 分析期限越界")?;
    let (mut prepared, mut files) = discovery::PreparedProjects::prepare(
        &repository,
        request.input.files.as_deref(),
        request.input.include_tests,
        deadline,
        &request.id,
    )?;
    let mut configurations = Vec::new();
    if files
        .iter()
        .any(|file| file.disposition == Disposition::ExcludedTest)
    {
        configurations.push(Configuration {
            id: "python-excluded".into(),
            source_paths: vec![],
            platform_ids: vec![],
        });
    }
    let mut root_contains = Vec::new();
    let mut nodes = Vec::new();
    let mut externals = BTreeMap::new();
    let mut origins = Vec::new();
    while let Some(mut context) = prepared.next_analysis(&request.environment.platforms[0].id)? {
        let (parsed, analyzed_files) = context.parse_selected()?;
        files.extend(analyzed_files);
        let selected_roots: BTreeMap<_, _> = parsed
            .iter()
            .filter_map(|(path, document)| {
                document.root.clone().map(|root| {
                    (
                        root,
                        path.strip_prefix(&repository)
                            .unwrap()
                            .to_string_lossy()
                            .replace('\\', "/"),
                    )
                })
            })
            .collect();
        let (documents, symbols) =
            references::analyze(&context, parsed, request.input.include_tests)?;
        configurations.push(context.configuration);
        for document in documents {
            if let Some(root) = &document.root {
                let path = &selected_roots[root];
                origins.extend(document.nodes.iter().map(|node| Origin {
                    node_id: node.data().id.0.clone(),
                    selected_paths: vec![path.clone()],
                }));
            }
            root_contains.extend(document.root);
            nodes.extend(document.nodes);
        }
        externals.extend(symbols);
    }
    let ids: BTreeSet<_> = nodes.iter().map(|node| node.data().id.clone()).collect();
    for node in &mut nodes {
        for reference in &mut syntax::node_data_mut(node).references {
            if let ReferenceTarget::External { symbol } = &reference.target {
                let id = NodeId(symbol.clone());
                if ids.contains(&id) {
                    reference.target = ReferenceTarget::Node { id };
                }
            }
        }
    }
    externals.retain(|symbol, _| !ids.contains(&NodeId(symbol.clone())));
    prepared.finish(&request.environment.platforms[0].id, &mut configurations)?;
    Ok(Projection {
        graph: CodeGraph {
            root_contains,
            nodes,
            external_symbols: externals.into_values().collect(),
        },
        evidence: Evidence {
            configurations,
            files,
            origins,
        },
    })
}

#[cfg(test)]
#[path = "tests/project.rs"]
mod tests;

#[cfg(test)]
#[path = "tests/discovery.rs"]
mod discovery_tests;

/// 既有语义 fixture 保持 JSON 断言；生产构图与检查不经过 JSON 往返。
#[cfg(test)]
pub fn analyze(request: &Request) -> Result<serde_json::Value> {
    Ok(serde_json::to_value(build(request)?)?)
}

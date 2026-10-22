//! 从同源原生 AST/导航构造完整 Rust 图与内部来源，输出前由本进程校验。

mod model;
mod references;
mod symbols;
mod syntax;
pub(crate) mod test_code;

use crate::{DependencyScope, RustProject, protocol::Request};
use anyhow::{Context, Result, ensure};
use codegraph_engine_common::graph::{
    Configuration, Disposition, Evidence, FileEvidence, Origin, Projection,
};
use std::{collections::BTreeMap, path::PathBuf};

/// 图投影只保存精确的物理文件与字节位置，不保留 LSP 传输位置。
#[derive(Clone)]
struct Navigation {
    key: String,
    offset: ::syntax::TextSize,
}

/// 为迁入的投影规则统一构造带上下文的错误。
fn failure(message: impl std::fmt::Display) -> anyhow::Error {
    anyhow::anyhow!(message.to_string())
}

/// 每个请求独立加载并输出全图，任何查询错误立即丢弃项目和全部候选数据。
pub fn build(request: &Request) -> Result<Projection> {
    request.validate()?;
    let repository = dunce::canonicalize(&request.input.repository_root)?;
    ensure!(repository.is_dir(), "仓库根必须为目录");
    let mut selected: BTreeMap<PathBuf, Vec<String>> = BTreeMap::new();
    for relative in request.input.files.iter().flatten() {
        let path = dunce::canonicalize(repository.join(relative))
            .with_context(|| format!("无法读取 {relative}"))?;
        ensure!(
            path.starts_with(&repository)
                && path.is_file()
                && path.extension().is_some_and(|extension| extension == "rs"),
            "选集必须为仓库内 Rust 源码"
        );
        selected.entry(path).or_default().push(relative.clone());
    }
    if request.input.files.as_ref().is_some_and(Vec::is_empty) {
        return Ok(Projection {
            graph: codegraph_engine_common::model::CodeGraph::empty(),
            evidence: Evidence::default(),
        });
    }
    let manifests = crate::discovery::manifests(&repository)?;
    ensure!(
        !manifests.is_empty(),
        "未发现 Rust workspace，无法完整加载 standalone 源码"
    );
    let scope = match request.input.profile.as_str() {
        "rust-repository-v1" => DependencyScope::RepositoryOnly,
        "rust-standard-library-v1" => DependencyScope::StandardLibrary,
        "rust-all-dependencies-v1" => DependencyScope::AllDependencies,
        _ => unreachable!("请求已验证"),
    };
    let policies = crate::discovery::policies(
        &repository,
        &manifests,
        scope,
        request.environment.platforms.first(),
    )?;
    let observation = request
        .input
        .files
        .is_none()
        .then(|| {
            codegraph_engine_common::observation::Observation::from_environment(
                "codegraph-rust",
                &request.id,
            )
        })
        .flatten();
    let project = RustProject::load_projects(
        &manifests,
        &policies,
        &selected.keys().cloned().collect::<Vec<_>>(),
        observation.as_ref(),
    )?;
    let discovered = request
        .input
        .files
        .is_none()
        .then(|| project.discover_sources(&repository))
        .transpose()?;
    if let Some(discovered) = &discovered {
        for path in discovered.keys() {
            let relative = path
                .strip_prefix(&repository)?
                .to_str()
                .context("发现的源码路径不是 UTF-8")?;
            #[cfg(windows)]
            let relative = relative.replace('\\', "/");
            #[cfg(not(windows))]
            let relative = relative.to_owned();
            crate::protocol::relative_path(&relative)?;
            selected.entry(path.clone()).or_default().push(relative);
        }
    }
    let options = model::AnalysisOptions {
        include_tests: request.input.include_tests,
    };
    let documents = selected
        .keys()
        .map(|path| syntax::Document::parse(&repository, path.clone(), &project, options))
        .collect::<Result<Vec<_>>>()?;
    let (graph, test_files) = references::analyze(
        &project,
        &repository,
        &documents,
        options,
        discovered.as_ref(),
    )?;
    let mut origins = Vec::new();
    for node in &graph.nodes {
        let id = &node.data().id;
        let document = documents
            .iter()
            .find(|document| {
                document
                    .declarations
                    .iter()
                    .any(|declaration| &declaration.node.data().id == id)
                    || syntax::file_scope(&repository, &document.path)
                        .is_some_and(|scope| id.0 == format!("{scope}::crate"))
            })
            .context("输出节点缺少选集出处")?;
        origins.push(Origin {
            node_id: id.0.clone(),
            selected_paths: selected[&document.path].clone(),
        });
    }
    let files: Vec<_> = documents
        .iter()
        .flat_map(|document| {
            let analyzed = !test_files.get(&document.key).copied().unwrap_or(false);
            selected[&document.path]
                .iter()
                .map(move |path| FileEvidence {
                    path: path.clone(),
                    disposition: if analyzed {
                        Disposition::Analyzed
                    } else {
                        Disposition::ExcludedTest
                    },
                    configuration_ids: vec!["rust-projects".into()],
                })
        })
        .collect();
    // selected 源码固定到一次请求结束；变化不能被误报成同一侧成功结果。
    for document in &documents {
        ensure!(
            std::fs::read_to_string(&document.path)? == document.source,
            "分析期间选中源码已改变：{}",
            document.path.display()
        );
    }
    let source_paths: Vec<_> = manifests
        .iter()
        .filter_map(|path| path.strip_prefix(&repository).ok())
        .map(|path| path.to_string_lossy().replace('\\', "/"))
        .collect();
    let platform_ids: Vec<_> = request
        .environment
        .platforms
        .iter()
        .map(|platform| platform.id.clone())
        .collect();
    Ok(Projection {
        graph,
        evidence: Evidence {
            configurations: vec![Configuration {
                id: "rust-projects".into(),
                source_paths,
                platform_ids,
            }],
            files,
            origins,
        },
    })
}

#[cfg(test)]
#[path = "tests/syntax.rs"]
mod syntax_tests;

/// 历史语义测试读取同一投影的 JSON 视图；生产入口始终使用类型化图。
#[cfg(test)]
pub fn analyze(request: &Request) -> Result<serde_json::Value> {
    Ok(serde_json::to_value(build(request)?)?)
}

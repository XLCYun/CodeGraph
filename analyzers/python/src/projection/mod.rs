//! Python 对象和运行时关系政策迁自既有实现，原文/AST/导航全部来自固定原生数据库。
mod bindings;
mod discovery;
mod fields;
mod known;
mod project;
mod references;
mod syntax;

use crate::{ProjectInput, protocol::Request};
use anyhow::{Context, Result, ensure};
use codegraph_engine_common::graph::{
    Configuration, Disposition, Evidence, FileEvidence, Origin, Projection,
};
use codegraph_engine_common::model::{CodeGraph, NodeId, ReferenceTarget};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
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
    let discovered = request
        .input
        .files
        .is_none()
        .then(|| discovery::load(&repository, deadline, &request.id))
        .transpose()?;
    let selected_files = request
        .input
        .files
        .as_ref()
        .unwrap_or_else(|| &discovered.as_ref().unwrap().files);
    let mut groups = BTreeMap::new();
    let mut files = Vec::new();
    let mut configurations = Vec::new();
    let mut snapshots = discovered
        .as_ref()
        .map(|value| value.snapshots.clone())
        .unwrap_or_default();
    for relative in selected_files {
        crate::loading::checkpoint(Some(deadline))?;
        let path = repository.join(relative);
        let actual = project::checked_path(&repository, &path)?;
        ensure!(
            path.is_file()
                && !project::environment_path(&repository, &path)
                && !project::environment_path(&repository, &actual),
            "选中的 Python 路径不是仓库源码文件"
        );
        if !request.input.include_tests
            && (is_test(Path::new(relative)) || is_test(actual.strip_prefix(&repository)?))
        {
            files.push(FileEvidence {
                path: relative.clone(),
                disposition: Disposition::ExcludedTest,
                configuration_ids: vec!["python-excluded".into()],
            });
            continue;
        }
        let source = if let Some(discovered) = &discovered {
            discovered.projects[&discovered.owners[relative]].source(Path::new(relative))?
        } else {
            project::read_source(&path)?
        };
        // 发现成员沿用实际接纳它的项目；PEP 723 仍按文件原文选择版本。
        let policy = if let Some(discovered) = &discovered {
            discovered.policies[&discovered.owners[relative]].with_source(&source)?
        } else {
            project::Project::for_file(&repository, &path, &source)?
        };
        snapshots.insert(path.clone(), source.clone());
        groups
            .entry(policy)
            .or_insert_with(Vec::new)
            .push((path, source));
    }
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
    let mut projected_roots = BTreeSet::new();
    for (index, (policy, selected)) in groups.into_iter().enumerate() {
        crate::loading::checkpoint(Some(deadline))?;
        projected_roots.insert(policy.root.clone());
        let configuration_id = format!("python-project-{index}");
        let mut source_paths = Vec::new();
        for name in ["pyproject.toml", "ty.toml"] {
            let path = policy.root.join(name);
            if path.exists() {
                project::checked_path(&repository, &path)?;
                let source = std::fs::read_to_string(&path)?;
                if let Some(previous) = snapshots.insert(path.clone(), source.clone()) {
                    ensure!(previous == source, "分析期间配置已改变：{}", path.display());
                }
                source_paths.push(
                    path.strip_prefix(&repository)?
                        .to_string_lossy()
                        .replace('\\', "/"),
                );
            }
        }
        configurations.push(Configuration {
            id: configuration_id.clone(),
            source_paths,
            platform_ids: vec![request.environment.platforms[0].id.clone()],
        });
        let native = if let Some(discovered) = &discovered {
            discovered
                .projects
                .get(&policy.root)
                .context("Python 成员未归属发现项目")?
                .with_minor(policy.minor)?
        } else {
            crate::loading::load_configured(
                ProjectInput {
                    repository: repository.clone(),
                    project_root: policy.root.strip_prefix(&repository)?.into(),
                    source_roots: policy
                        .source_roots
                        .iter()
                        .map(|path| path.strip_prefix(&repository).map(PathBuf::from))
                        .collect::<std::result::Result<Vec<_>, _>>()?,
                    python_minor: policy.minor,
                },
                None,
                Some(deadline),
            )?
        };
        let mut parsed = BTreeMap::new();
        for (path, source) in selected {
            crate::loading::checkpoint(Some(deadline))?;
            let relative = path.strip_prefix(&repository)?;
            ensure!(
                native.source(relative)? == source,
                "分析期间选中源码已改变：{}",
                relative.display()
            );
            let ast = native
                .parse(relative)
                .with_context(|| format!("{}：Python 语法提取失败", relative.display()))?;
            let document =
                syntax::Document::from_ast(relative, &source, ast.syntax().body.clone())?;
            files.push(FileEvidence {
                path: relative.to_string_lossy().replace('\\', "/"),
                disposition: Disposition::Analyzed,
                configuration_ids: vec![configuration_id.clone()],
            });
            parsed.insert(path, document);
        }
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
        let (documents, symbols) = references::analyze(
            &native,
            &repository,
            &policy,
            parsed,
            request.input.include_tests,
            discovered.as_ref().map(|value| &value.policies),
        )?;
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
    if let Some(discovered) = &discovered {
        for project in discovered.projects.values() {
            project.ensure_unchanged(deadline)?;
        }
        for root in discovered
            .projects
            .keys()
            .filter(|root| !projected_roots.contains(*root))
        {
            let source_paths: Vec<_> = ["ty.toml", "pyproject.toml"]
                .into_iter()
                .map(|name| root.join(name))
                .filter(|path| snapshots.contains_key(path))
                .map(|path| {
                    path.strip_prefix(&repository)
                        .unwrap()
                        .to_string_lossy()
                        .replace('\\', "/")
                })
                .collect();
            configurations.push(Configuration {
                id: format!("python-empty-{}", configurations.len()),
                source_paths,
                platform_ids: vec![request.environment.platforms[0].id.clone()],
            });
        }
    }
    for (path, source) in snapshots {
        ensure!(
            std::fs::read_to_string(&path)? == source,
            "分析期间源码或配置已改变：{}",
            path.display()
        );
    }
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

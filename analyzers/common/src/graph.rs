//! 在语言构图边界一次检查选集来源和图不变量，不启动复核进程或自动修图。
use crate::{
    model::{CodeGraph, NodeId, ReferenceTarget},
    public_protocol::{CheckedRequest, repository_file},
    selection::MAX_FILES,
};
use anyhow::{Result, ensure};
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};

/// 投影与来源是进程内数据，只有 graph 写入公共最终响应。
#[derive(Serialize)]
pub struct Projection {
    pub graph: CodeGraph,
    pub evidence: Evidence,
}

/// 每个配置、文件和节点都保留实际出处，避免选集外源码进入完整图。
#[derive(Default, Serialize)]
pub struct Evidence {
    pub configurations: Vec<Configuration>,
    pub files: Vec<FileEvidence>,
    pub origins: Vec<Origin>,
}

/// 真实配置与内部批准的目标平台，不接受请求注入平台目录。
#[derive(Serialize)]
pub struct Configuration {
    pub id: String,
    pub source_paths: Vec<String>,
    pub platform_ids: Vec<String>,
}

/// 每个选中文件恰有一条覆盖记录，即使它是空文件或被测试开关排除。
#[derive(Serialize)]
pub struct FileEvidence {
    pub path: String,
    pub disposition: Disposition,
    pub configuration_ids: Vec<String>,
}

/// 只有已分析文件能支持节点出处。
#[derive(Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Disposition {
    Analyzed,
    ExcludedTest,
    Inactive,
}

/// 节点身份与至少一个选中源码的真实对应关系。
#[derive(Serialize)]
pub struct Origin {
    pub node_id: String,
    pub selected_paths: Vec<String>,
}

/// 身份列表必须非空、无 NUL 且不重复。
fn unique<'a>(values: impl IntoIterator<Item = &'a String>) -> Result<BTreeSet<&'a str>> {
    let mut seen = BTreeSet::new();
    for value in values {
        ensure!(
            !value.is_empty() && !value.contains('\0') && seen.insert(value.as_str()),
            "身份列表含空值或重复项"
        );
    }
    Ok(seen)
}

/// 语义键具有明确命名空间，不能把展示信息当成不透明通用键。
fn namespaced(value: &str) -> bool {
    !value.contains('\0')
        && value
            .split_once('.')
            .is_some_and(|(a, b)| !a.is_empty() && !b.is_empty())
}

/// 检查内部来源后交还完整公共图，所有失败均丢弃整次结果。
pub fn finish(
    projection: Projection,
    input: &CheckedRequest,
    language: &str,
    platforms: &[String],
) -> Result<CodeGraph> {
    let Projection { graph, evidence } = projection;
    ensure!(evidence.files.len() <= MAX_FILES, "文件覆盖超过数量上限");
    // 显式选集只建立一次成员索引，后续覆盖查询不再重复扫描原始列表。
    let selected = input.request.files.as_ref().map(unique).transpose()?;
    let configurations = unique(evidence.configurations.iter().map(|c| &c.id))?;
    ensure!(
        selected.is_some() || !configurations.is_empty(),
        "发现结果缺少实际项目配置"
    );
    let platforms = unique(platforms)?;
    for configuration in &evidence.configurations {
        for path in unique(&configuration.source_paths)? {
            repository_file(&input.repository, path)?;
        }
        ensure!(
            unique(&configuration.platform_ids)?.is_subset(&platforms),
            "配置引用未批准的平台"
        );
    }
    let mut files = BTreeMap::new();
    for file in &evidence.files {
        ensure!(
            selected
                .as_ref()
                .is_none_or(|selected| selected.contains(file.path.as_str()))
                && files.insert(file.path.as_str(), file.disposition).is_none(),
            "文件覆盖出现未请求或重复路径"
        );
        repository_file(&input.repository, &file.path)?;
        let configurations_for_file = unique(&file.configuration_ids)?;
        ensure!(
            !configurations_for_file.is_empty()
                && configurations_for_file.is_subset(&configurations),
            "文件覆盖缺少实际配置"
        );
        ensure!(
            !input.request.options.include_tests || file.disposition != Disposition::ExcludedTest,
            "测试开关与覆盖记录冲突"
        );
    }
    ensure!(
        selected
            .as_ref()
            .is_none_or(|selected| selected.len() == files.len()),
        "成功结果遗漏选中文件"
    );
    let ids = unique(graph.nodes.iter().map(|n| &n.data().id.0))?;
    let origins = unique(evidence.origins.iter().map(|o| &o.node_id))?;
    ensure!(ids == origins, "节点与来源不是一一对应");
    for origin in &evidence.origins {
        let paths = unique(&origin.selected_paths)?;
        ensure!(
            !paths.is_empty()
                && paths
                    .iter()
                    .all(|p| files.get(p) == Some(&Disposition::Analyzed)),
            "节点来源不属于已分析选集"
        );
    }
    validate(&graph, language)?;
    Ok(graph)
}

/// 构图只接受唯一节点、有效端点、单一父节点、根可达性和完整图外描述。
pub fn validate(graph: &CodeGraph, language: &str) -> Result<()> {
    let ids = unique(graph.nodes.iter().map(|n| &n.data().id.0))?;
    let nodes: BTreeMap<_, _> = graph.nodes.iter().map(|n| (&n.data().id, n)).collect();
    let mut parents = BTreeSet::new();
    let mut register = |child: &NodeId| -> Result<()> {
        ensure!(
            nodes.contains_key(child) && parents.insert(child.clone()),
            "包含目标缺失或具有重复父节点"
        );
        Ok(())
    };
    for child in &graph.root_contains {
        register(child)?;
    }
    let mut referenced = BTreeSet::new();
    for node in &graph.nodes {
        let data = node.data();
        ensure!(
            data.language.0 == language
                && !data.native_kind.is_empty()
                && data
                    .id
                    .0
                    .strip_prefix(&format!("{language}:"))
                    .is_some_and(|s| !s.is_empty()),
            "节点语言、身份或原生种类无效"
        );
        ensure!(
            data.extensions.keys().all(|key| namespaced(key)),
            "扩展键缺少命名空间"
        );
        for child in &data.contains {
            register(child)?;
        }
        let mut references = BTreeSet::new();
        for reference in &data.references {
            ensure!(
                namespaced(&reference.semantic) && references.insert(reference),
                "引用语义无效或引用重复"
            );
            match &reference.target {
                ReferenceTarget::Node { id } => ensure!(nodes.contains_key(id), "引用图内目标缺失"),
                ReferenceTarget::External { symbol } => {
                    ensure!(
                        !ids.contains(symbol.as_str())
                            && !symbol.contains('\0')
                            && symbol
                                .split_once(':')
                                .is_some_and(|(a, b)| !a.is_empty() && !b.is_empty()),
                        "图外身份无效或实际位于图内"
                    );
                    referenced.insert(symbol.as_str());
                }
                ReferenceTarget::Unresolved { name } => ensure!(
                    !name.is_empty() && !name.contains('\0'),
                    "未解析名称为空或无效"
                ),
            }
        }
    }
    let mut reachable = BTreeSet::new();
    let mut pending: Vec<_> = graph.root_contains.iter().collect();
    while let Some(id) = pending.pop() {
        ensure!(reachable.insert(id), "包含关系形成环");
        pending.extend(nodes[id].data().contains.iter());
    }
    ensure!(reachable.len() == nodes.len(), "节点不能从根到达");
    let external = unique(graph.external_symbols.iter().map(|s| &s.symbol))?;
    ensure!(
        external == referenced
            && graph
                .external_symbols
                .iter()
                .all(|s| !s.name.is_empty() && !s.name.contains('\0')),
        "图外描述缺失、重复或未被引用"
    );
    Ok(())
}

#[cfg(test)]
#[path = "tests/graph.rs"]
mod tests;

#[cfg(test)]
#[path = "tests/historical_graph.rs"]
mod historical_tests;

#[cfg(test)]
#[path = "tests/historical_evidence.rs"]
mod historical_evidence_tests;

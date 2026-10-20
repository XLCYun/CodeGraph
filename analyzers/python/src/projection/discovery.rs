//! 项目配置从仓库根发现，原生 src 索引只在冻结内存中确定成员。
use super::{
    failure,
    project::{self, Project},
};
use crate::{ProjectInput, PythonProject, loading};
use anyhow::{Result, ensure};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
    time::Instant,
};

/// 选集、数据库和配置快照一起交给投影，不能再扫描一次磁盘生成图。
pub(super) struct Discovery {
    pub files: Vec<String>,
    pub projects: BTreeMap<PathBuf, PythonProject>,
    pub snapshots: BTreeMap<PathBuf, String>,
    pub owners: BTreeMap<String, PathBuf>,
    pub policies: BTreeMap<PathBuf, Project>,
}

/// 元数据遍历排除安装环境和工具树；普通源码是否入选交给原生 src 规则。
fn eligible(entry: &walkdir::DirEntry) -> bool {
    !matches!(
        entry.file_name().to_str(),
        Some(
            ".git"
                | ".venv"
                | "node_modules"
                | ".tools"
                | "site-packages"
                | "dist-packages"
                | "__pycache__"
        )
    ) && !(entry.file_type().is_dir() && entry.path().join("pyvenv.cfg").is_file())
}

/// 只接受快照能提供的 src 设置；显式要求外部 ignore 发现时明确失败。
fn source_options(configuration: &toml::Value) -> Result<toml::Value> {
    let mut src = configuration
        .get("src")
        .cloned()
        .unwrap_or(toml::Value::Table(Default::default()));
    let table = src
        .as_table_mut()
        .ok_or_else(|| failure("Python src 必须为表"))?;
    if let Some(value) = table.get("respect-ignore-files") {
        ensure!(
            value.as_bool() == Some(false),
            "只读 Python 发现不支持 respect-ignore-files=true；请使用 src.include/exclude 或显式设为 false"
        );
    }
    table.insert("respect-ignore-files".into(), toml::Value::Boolean(false));
    Ok(src)
}

/// 按祖先政策接纳并冻结项目，成员仅来自同一数据库的原生索引。
pub(super) fn load(repository: &Path, deadline: Instant) -> Result<Discovery> {
    let mut scopes = BTreeMap::new();
    let mut snapshots = BTreeMap::new();
    let mut projects = BTreeMap::new();
    let mut entry_views: BTreeMap<PathBuf, PythonProject> = BTreeMap::new();
    let mut roots = BTreeMap::new();
    for entry in walkdir::WalkDir::new(repository)
        .follow_links(true)
        .into_iter()
        .filter_entry(eligible)
    {
        loading::checkpoint(Some(deadline))?;
        let entry = entry?;
        project::checked_path(repository, entry.path())?;
        if !entry.file_type().is_dir() {
            continue;
        }
        let directory = entry.path();
        if directory != repository {
            let parent = entry_views
                .iter()
                .filter(|(root, _)| directory.starts_with(root))
                .max_by_key(|(root, _)| root.components().count())
                .map(|(_, view)| view)
                .ok_or_else(|| failure("Python 嵌套项目没有包含项目"))?;
            // 控制文件保持 ty.toml 优先；被祖先排除的目录不能靠内嵌配置重新入选。
            let control = ["ty.toml", "pyproject.toml"]
                .into_iter()
                .map(|name| directory.join(name))
                .find(|path| path.exists());
            if let Some(control) = control {
                project::checked_path(repository, &control)?;
                if !parent.admits_configuration(control.strip_prefix(repository)?)? {
                    continue;
                }
            }
        }
        let mut read = |name| -> Result<Option<toml::Value>> {
            let path = directory.join(name);
            Ok(
                project::read_toml_snapshot(repository, &path)?.map(|(value, source)| {
                    snapshots.insert(path, source);
                    value
                }),
            )
        };
        let ty = read("ty.toml")?;
        let pyproject = read("pyproject.toml")?;
        let inline = pyproject
            .as_ref()
            .and_then(|value| value.get("tool"))
            .and_then(|value| value.get("ty"));
        if directory != repository
            && ty.is_none()
            && inline.is_none()
            && pyproject
                .as_ref()
                .and_then(|value| value.get("project"))
                .is_none()
        {
            continue;
        }
        let requires = pyproject
            .as_ref()
            .and_then(|value| value.get("project"))
            .and_then(|value| value.get("requires-python"))
            .map(|value| {
                value
                    .as_str()
                    .map(str::to_owned)
                    .ok_or_else(|| failure("Python 配置项必须是字符串"))
            })
            .transpose()?;
        let configuration = ty
            .or_else(|| inline.cloned())
            .unwrap_or(toml::Value::Table(Default::default()));
        let policy = Project::from_configuration(
            repository,
            directory.to_owned(),
            &configuration,
            requires,
            "",
        )?;
        let options = source_options(&configuration)?;
        // 显式 include 可独立选择项目源码；省略时只从 own roots 收集。
        let member_roots = if options.get("include").is_some() {
            policy
                .source_roots
                .iter()
                .chain(std::iter::once(&policy.root))
                .cloned()
                .collect()
        } else {
            policy.member_roots.clone()
        };
        let native = loading::load_configured(
            ProjectInput {
                repository: repository.to_owned(),
                project_root: policy.root.strip_prefix(repository)?.into(),
                source_roots: policy
                    .source_roots
                    .iter()
                    .map(|path| path.strip_prefix(repository).map(PathBuf::from))
                    .collect::<std::result::Result<_, _>>()?,
                python_minor: policy.minor,
            },
            Some(options),
            Some(deadline),
        )?;
        let member_roots = member_roots
            .iter()
            .map(|path| path.strip_prefix(repository).map(PathBuf::from))
            .collect::<std::result::Result<Vec<_>, _>>()?;
        entry_views.insert(policy.root.clone(), native.configuration_view()?);
        roots.insert(policy.root.clone(), member_roots);
        projects.insert(policy.root.clone(), native);
        scopes.insert(policy.root.clone(), policy);
    }
    let mut files = BTreeSet::new();
    let mut owners = BTreeMap::new();
    for (root, native) in &mut projects {
        loading::checkpoint(Some(deadline))?;
        for relative in native.discover(&roots[root], deadline)? {
            loading::checkpoint(Some(deadline))?;
            let path = repository.join(&relative);
            // 已识别子项目按自己的根和 src 政策处理，父项目不能第二次投影它。
            if scopes
                .keys()
                .any(|child| child != root && child.starts_with(root) && path.starts_with(child))
            {
                continue;
            }
            ensure!(
                matches!(
                    relative.extension().and_then(|value| value.to_str()),
                    Some("py" | "pyi")
                ),
                "Python 发现成员不是 .py/.pyi"
            );
            let relative = relative
                .to_str()
                .ok_or_else(|| failure("Python 路径不是 UTF-8"))?
                .replace('\\', "/");
            owners
                .entry(relative.clone())
                .or_insert_with(|| root.clone());
            files.insert(relative);
            ensure!(
                files.len() <= codegraph_engine_common::selection::MAX_FILES,
                "文件选集超过数量上限"
            );
        }
    }
    Ok(Discovery {
        files: files.into_iter().collect(),
        projects,
        snapshots,
        owners,
        policies: scopes,
    })
}

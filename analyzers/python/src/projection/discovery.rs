//! 项目配置从仓库根发现，原生 src 索引只在冻结内存中确定成员。
use super::{
    failure, is_test,
    project::{self, Project},
    syntax::{self, Document},
};
use crate::{DefinitionCandidate, ProjectInput, PythonProject, SourceOrigin, loading};
use anyhow::{Context, Result, ensure};
use codegraph_engine_common::graph::{Configuration, Disposition, FileEvidence};
use ruff_db::parsed::ParsedModuleRef;
use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
    time::Instant,
};

/// 单次分析持有成员归属、项目数据库和快照，调用方只消费完整的分析上下文。
pub(super) struct PreparedProjects {
    repository: PathBuf,
    deadline: Instant,
    discovered: bool,
    projects: BTreeMap<PathBuf, PythonProject>,
    snapshots: BTreeMap<PathBuf, String>,
    owners: BTreeMap<String, PathBuf>,
    policies: BTreeMap<PathBuf, Project>,
    groups: BTreeMap<Project, Vec<(PathBuf, String)>>,
    projected_roots: BTreeSet<PathBuf>,
    next_index: usize,
}

/// 一个项目/目标版本的原生数据库、选中文件及配置证据，均由同一准备过程配对。
pub(super) struct AnalysisContext<'a> {
    pub native: PythonProject,
    pub repository: &'a Path,
    pub configuration: Configuration,
    prepared: &'a PreparedProjects,
    policy: Project,
    selected: Vec<(PathBuf, String)>,
}

impl PreparedProjects {
    /// 显式选集不启用目录发现；省略选集时沿用原生接纳者，再按脚本版本分组。
    pub fn prepare(
        repository: &Path,
        selected: Option<&[String]>,
        include_tests: bool,
        deadline: Instant,
        request_id: &str,
    ) -> Result<(Self, Vec<FileEvidence>)> {
        let (mut prepared, selected) = if let Some(selected) = selected {
            (Self::empty(repository, deadline), selected.to_vec())
        } else {
            discover(repository, deadline, request_id)?
        };
        let mut excluded = Vec::new();
        for relative in selected {
            loading::checkpoint(Some(deadline))?;
            let path = repository.join(&relative);
            let actual = project::checked_path(repository, &path)?;
            ensure!(
                path.is_file()
                    && !project::environment_path(repository, &path)
                    && !project::environment_path(repository, &actual),
                "选中的 Python 路径不是仓库源码文件"
            );
            if !include_tests
                && (is_test(Path::new(&relative)) || is_test(actual.strip_prefix(repository)?))
            {
                excluded.push(FileEvidence {
                    path: relative,
                    disposition: Disposition::ExcludedTest,
                    configuration_ids: vec!["python-excluded".into()],
                });
                continue;
            }
            let (source, policy) = prepared.prepare_file(&relative, &path)?;
            prepared.snapshots.insert(path.clone(), source.clone());
            prepared
                .groups
                .entry(policy)
                .or_default()
                .push((path, source));
        }
        Ok((prepared, excluded))
    }

    /// 无发现状态用于显式选集，包括不读取任何项目配置的显式空集。
    fn empty(repository: &Path, deadline: Instant) -> Self {
        Self {
            repository: repository.to_owned(),
            deadline,
            discovered: false,
            projects: BTreeMap::new(),
            snapshots: BTreeMap::new(),
            owners: BTreeMap::new(),
            policies: BTreeMap::new(),
            groups: BTreeMap::new(),
            projected_roots: BTreeSet::new(),
            next_index: 0,
        }
    }

    /// 成员原文和配置归属必须一起解析，发现模式不能重新启用未接纳的嵌套配置。
    fn prepare_file(&self, relative: &str, path: &Path) -> Result<(String, Project)> {
        if self.discovered {
            let owner = &self.owners[relative];
            let source = self.projects[owner].source(Path::new(relative))?;
            let policy = self.policies[owner].with_source(&source)?;
            Ok((source, policy))
        } else {
            let source = project::read_source(path)?;
            let policy = Project::for_file(&self.repository, path, &source)?;
            Ok((source, policy))
        }
    }

    /// 按既有政策顺序准备下一组，配置证据和目标版本数据库在此统一绑定。
    pub fn next_analysis(&mut self, platform_id: &str) -> Result<Option<AnalysisContext<'_>>> {
        let Some((policy, selected)) = self.groups.pop_first() else {
            return Ok(None);
        };
        loading::checkpoint(Some(self.deadline))?;
        self.projected_roots.insert(policy.root.clone());
        let configuration_id = format!("python-project-{}", self.next_index);
        self.next_index += 1;
        let mut source_paths = Vec::new();
        for name in ["pyproject.toml", "ty.toml"] {
            let path = policy.root.join(name);
            if path.exists() {
                project::checked_path(&self.repository, &path)?;
                let source = std::fs::read_to_string(&path)?;
                if let Some(previous) = self.snapshots.insert(path.clone(), source.clone()) {
                    ensure!(previous == source, "分析期间配置已改变：{}", path.display());
                }
                source_paths.push(
                    path.strip_prefix(&self.repository)?
                        .to_string_lossy()
                        .replace('\\', "/"),
                );
            }
        }
        let native = if self.discovered {
            self.projects
                .get(&policy.root)
                .context("Python 成员未归属发现项目")?
                .with_minor(policy.minor)?
        } else {
            loading::load_configured(
                ProjectInput {
                    repository: self.repository.clone(),
                    project_root: policy.root.strip_prefix(&self.repository)?.into(),
                    source_roots: policy
                        .source_roots
                        .iter()
                        .map(|path| path.strip_prefix(&self.repository).map(PathBuf::from))
                        .collect::<std::result::Result<Vec<_>, _>>()?,
                    python_minor: policy.minor,
                },
                None,
                Some(self.deadline),
            )?
        };
        Ok(Some(AnalysisContext {
            native,
            repository: &self.repository,
            configuration: Configuration {
                id: configuration_id,
                source_paths,
                platform_ids: vec![platform_id.to_owned()],
            },
            prepared: self,
            policy,
            selected,
        }))
    }

    /// 最后核验全部发现上下文及已记录字节，并补齐没有输出成员的配置证据。
    pub fn finish(self, platform_id: &str, configurations: &mut Vec<Configuration>) -> Result<()> {
        if self.discovered {
            for project in self.projects.values() {
                project.ensure_unchanged(self.deadline)?;
            }
            for root in self
                .projects
                .keys()
                .filter(|root| !self.projected_roots.contains(*root))
            {
                let source_paths = ["ty.toml", "pyproject.toml"]
                    .into_iter()
                    .map(|name| root.join(name))
                    .filter(|path| self.snapshots.contains_key(path))
                    .map(|path| {
                        path.strip_prefix(&self.repository)
                            .unwrap()
                            .to_string_lossy()
                            .replace('\\', "/")
                    })
                    .collect();
                configurations.push(Configuration {
                    id: format!("python-empty-{}", configurations.len()),
                    source_paths,
                    platform_ids: vec![platform_id.to_owned()],
                });
            }
        }
        for (path, source) in &self.snapshots {
            ensure!(
                std::fs::read_to_string(path)? == *source,
                "分析期间源码或配置已改变：{}",
                path.display()
            );
        }
        Ok(())
    }
}

impl AnalysisContext<'_> {
    /// 消费选中原文并与此版本数据库逐个核对后提取 AST，上下文不会扩充输出选集。
    pub fn parse_selected(&mut self) -> Result<(BTreeMap<PathBuf, Document>, Vec<FileEvidence>)> {
        let mut parsed = BTreeMap::new();
        let mut files = Vec::new();
        for (path, source) in std::mem::take(&mut self.selected) {
            loading::checkpoint(Some(self.prepared.deadline))?;
            let relative = path.strip_prefix(self.repository)?;
            ensure!(
                self.native.source(relative)? == source,
                "分析期间选中源码已改变：{}",
                relative.display()
            );
            let ast = self
                .native
                .parse(relative)
                .with_context(|| format!("{}：Python 语法提取失败", relative.display()))?;
            let document =
                syntax::Document::from_ast(relative, &source, ast.syntax().body.clone())?;
            files.push(FileEvidence {
                path: relative.to_string_lossy().replace('\\', "/"),
                disposition: Disposition::Analyzed,
                configuration_ids: vec![self.configuration.id.clone()],
            });
            parsed.insert(path, document);
        }
        Ok((parsed, files))
    }

    /// 目标自身的配置/脚本版本决定候选 AST，同时保留跨独立项目的关系限制。
    pub fn target_ast(&self, location: &DefinitionCandidate) -> Result<(ParsedModuleRef, bool)> {
        let mut target_minor = self.policy.minor;
        let mut conflicting = false;
        if let SourceOrigin::Repository(relative) = &location.origin {
            let actual = self.repository.join(relative);
            let target_project = if self.prepared.discovered {
                self.prepared
                    .policies
                    .iter()
                    .filter(|(root, _)| actual.starts_with(root))
                    .max_by_key(|(root, _)| root.components().count())
                    .context("Python 上下文缺少发现项目")?
                    .1
                    .with_source(&location.source)?
            } else {
                Project::for_file(self.repository, &actual, &location.source)?
            };
            target_minor = target_project.minor;
            conflicting =
                target_project.root != self.policy.root && target_project.root != self.repository;
        }
        let relative = match &location.origin {
            SourceOrigin::Repository(path) => path.as_path(),
            SourceOrigin::StandardDeclaration(path) => {
                Path::new(path.strip_prefix("stdlib/").unwrap_or(path))
            }
        };
        let ast = self
            .native
            .candidate_ast(location, target_minor)
            .with_context(|| format!("{}：Python 目标语法提取失败", relative.display()))?;
        Ok((ast, conflicting))
    }
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
fn discover(
    repository: &Path,
    deadline: Instant,
    request_id: &str,
) -> Result<(PreparedProjects, Vec<String>)> {
    let observation = codegraph_engine_common::observation::Observation::from_environment(
        "codegraph-python",
        request_id,
    );
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
        // 只有真实配置字节已读入并解析后才报告，不能把默认配置或启动当作加载证据。
        if (ty.is_some() || pyproject.is_some())
            && let Some(observation) = &observation
        {
            observation.record("project-configuration-read")?;
        }
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
            // 目的地的独立项目优先于任何外来 own-root 提名；仓库根只提供兜底归属。
            if scopes
                .keys()
                .filter(|owner| owner.as_path() != repository && path.starts_with(owner))
                .max_by_key(|owner| owner.components().count())
                .is_some_and(|owner| owner != root)
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
    Ok((
        PreparedProjects {
            discovered: true,
            projects,
            snapshots,
            owners,
            policies: scopes,
            ..PreparedProjects::empty(repository, deadline)
        },
        files.into_iter().collect(),
    ))
}

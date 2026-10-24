//! 固定侧输入先做边界校验，再冻结到独立的只读 System；不发现环境或执行项目。
use crate::{ProjectInput, PythonProject, system::SnapshotSystem};
use anyhow::{Context, Result, ensure};
use ruff_db::system::{
    InMemorySystem, MemoryFileSystem, SystemPath, SystemPathBuf, WritableSystem,
};
use ruff_ranged_value::ValueSource;
use std::{
    collections::BTreeSet,
    fs,
    path::{Component, Path, PathBuf},
    time::Instant,
};
use ty_project::{Db, ProjectDatabase, ProjectMetadata, UseUv, metadata::options::Options};

/// 固定虚拟命名空间按原生组件归一化，Windows include 匹配不能收到混合分隔符。
pub(crate) fn snapshot_root() -> SystemPathBuf {
    SystemPath::absolute("/narrant/repository", "/")
}

/// 空解释器环境使用同一原生路径约定，不引入宿主驱动器或实际安装目录。
fn environment_root() -> SystemPathBuf {
    SystemPath::absolute("/narrant/environment", "/")
}

/// 源码路径只允许干净的仓库相对路径；空路径表示项目/源码根。
pub(crate) fn relative(path: &Path) -> Result<()> {
    ensure!(
        path.components()
            .all(|part| matches!(part, Component::Normal(_))),
        "Python 路径必须是规范仓库相对路径：{}",
        path.display()
    );
    ensure!(path.to_str().is_some(), "Python 路径不是 UTF-8");
    Ok(())
}

/// 用固定内存根承载词法相对路径，避免临时 checkout 与平台绝对路径污染身份。
pub(crate) fn system_path(path: &Path) -> Result<SystemPathBuf> {
    relative(path)?;
    let mut output = snapshot_root();
    for component in path.components() {
        output.push(component.as_os_str().to_str().unwrap());
    }
    Ok(output)
}

/// 读取前解析链接并校验真实安装环境；快照与导航仍使用原来的词法相对路径。
fn check_path(repository: &Path, path: &Path) -> Result<()> {
    let actual = path
        .canonicalize()
        .with_context(|| format!("Python 路径无法读取：{}", path.display()))?;
    ensure!(
        actual.starts_with(repository),
        "Python 路径逃逸仓库：{}",
        path.display()
    );
    check_environment(repository, &actual)
}

/// 安装环境不能作为仓库源码进入语义上下文。
fn check_environment(repository: &Path, path: &Path) -> Result<()> {
    ensure!(
        !path
            .strip_prefix(repository)?
            .components()
            .any(|part| matches!(
                part.as_os_str().to_str(),
                Some("site-packages" | "dist-packages" | ".venv")
            ))
            && !path
                .ancestors()
                .take_while(|ancestor| ancestor.starts_with(repository))
                .any(|ancestor| ancestor.join("pyvenv.cfg").is_file()),
        "Python 可达源码包含安装环境：{}",
        path.display()
    );
    Ok(())
}

/// 保留 UTF-8 BOM 和原始换行；首个有效编码 cookie 不得偷偷切换解释方式。
fn read_source(path: &Path) -> Result<String> {
    let source = fs::read_to_string(path)
        .with_context(|| format!("Python 源码不是可读 UTF-8：{}", path.display()))?;
    ensure!(
        source.len() <= u32::MAX as usize,
        "Python 源码超过解析器大小限制"
    );
    for line in source
        .trim_start_matches('\u{feff}')
        .split('\n')
        .flat_map(|line| line.strip_suffix('\r').unwrap_or(line).split('\r'))
        .take(2)
    {
        let line = line.trim_start_matches([' ', '\t', '\x0c']);
        if !line.starts_with('#') && !line.is_empty() {
            break;
        }
        let cookie = line.match_indices("coding").find_map(|(index, _)| {
            if !line
                .as_bytes()
                .get(index + 6)
                .is_some_and(|byte| matches!(byte, b':' | b'='))
            {
                return None;
            }
            let value = line[index + 7..]
                .trim_start_matches([' ', '\t'])
                .chars()
                .take_while(|character| {
                    character.is_ascii_alphanumeric() || matches!(character, '-' | '_' | '.')
                })
                .collect::<String>();
            (!value.is_empty()).then_some(value)
        });
        if let Some(cookie) = cookie {
            let encoding = cookie.to_ascii_lowercase().replace('_', "-");
            ensure!(
                matches!(encoding.as_str(), "utf-8" | "utf8") || encoding.starts_with("utf-8-"),
                "Python 不支持编码 {encoding}：{}",
                path.display()
            );
            break;
        }
    }
    Ok(source)
}

/// 单个项目只读取明确源码根及项目根，配置发现留给应用层既有政策。
pub(crate) fn load(input: ProjectInput) -> Result<PythonProject> {
    load_configured(input, None, None)
}

/// 发现和投影共用一次冻结；配置仅作为受控内存覆盖，不开启磁盘自动发现。
pub(crate) fn load_configured(
    input: ProjectInput,
    source_options: Option<toml::Value>,
    deadline: Option<Instant>,
) -> Result<PythonProject> {
    ensure!(
        (7..=14).contains(&input.python_minor),
        "Python 目标必须为 3.7–3.14"
    );
    ensure!(!input.source_roots.is_empty(), "Python 源码根不能为空");
    let repository = input.repository.canonicalize()?;
    ensure!(repository.is_dir(), "Python 仓库根不是目录");
    let project = system_path(&input.project_root)?;
    let memory = InMemorySystem::from_memory_fs(MemoryFileSystem::with_current_directory(&project));
    let mut files = BTreeSet::new();
    for root in input
        .source_roots
        .iter()
        .chain(std::iter::once(&input.project_root))
    {
        relative(root)?;
        let root = repository.join(root);
        check_path(&repository, &root)?;
        check_environment(&repository, &root)?;
        ensure!(root.is_dir(), "Python 源码根不是目录");
        for entry in walkdir::WalkDir::new(&root)
            .follow_links(true)
            .into_iter()
            .filter_entry(|entry| !matches!(entry.file_name().to_str(), Some(".git" | ".venv")))
        {
            checkpoint(deadline)?;
            let entry = entry?;
            check_path(&repository, entry.path())?;
            check_environment(&repository, entry.path())?;
            let relative = entry.path().strip_prefix(&repository)?.to_owned();
            if entry.file_type().is_dir() {
                memory.create_directory_all(&system_path(&relative)?)?;
            }
            if entry.file_type().is_file()
                && matches!(
                    entry.path().extension().and_then(|value| value.to_str()),
                    Some("py" | "pyi")
                )
                && files.insert(relative.clone())
            {
                memory
                    .fs()
                    .write_file_all(system_path(&relative)?, read_source(entry.path())?)?;
            }
        }
    }
    // 显式空 prefix 阻止 PythonEnvironment 进入解释器或 .venv 发现分支。
    let prefix = environment_root();
    for minor in 7..=14 {
        memory.create_directory_all(
            &prefix
                .join("lib")
                .join(format!("python3.{minor}"))
                .join("site-packages"),
        )?;
    }
    memory.create_directory_all(&prefix.join("Lib").join("site-packages"))?;
    from_snapshot(input, SnapshotSystem(memory), files, source_options)
}

/// 不同 PEP 723 版本只创建数据库配置，源码仍来自同一只读内存快照。
fn from_snapshot(
    input: ProjectInput,
    snapshot: SnapshotSystem,
    files: BTreeSet<PathBuf>,
    source_options: Option<toml::Value>,
) -> Result<PythonProject> {
    let project = system_path(&input.project_root)?;
    let mut environment = toml::Table::new();
    environment.insert(
        "root".into(),
        toml::Value::Array(
            input
                .source_roots
                .iter()
                .map(|root| system_path(root).map(|path| toml::Value::String(path.to_string())))
                .collect::<Result<Vec<_>>>()?,
        ),
    );
    environment.insert(
        "python".into(),
        toml::Value::String(environment_root().to_string()),
    );
    environment.insert(
        "python-version".into(),
        toml::Value::String(format!("3.{}", input.python_minor)),
    );
    environment.insert("python-platform".into(), toml::Value::String("all".into()));
    environment.insert("extra-paths".into(), toml::Value::Array(Vec::new()));
    let mut options = toml::Table::new();
    options.insert("environment".into(), toml::Value::Table(environment));
    if let Some(src) = &source_options {
        options.insert("src".into(), src.clone());
    }
    let options = Options::from_toml_str(&toml::to_string(&options)?, ValueSource::Cli)?;
    let mut metadata = ProjectMetadata::new("narrant", project).with_use_uv(UseUv::Off);
    metadata.set_override_options(options);
    let db = ProjectDatabase::fallible(metadata, snapshot.clone())?;
    Ok(PythonProject {
        db,
        files,
        snapshot,
        input,
        source_options,
    })
}

/// 遍历和收集期间持续检查期限；控制线程仍独立处理取消和 EOF。
pub(crate) fn checkpoint(deadline: Option<Instant>) -> Result<()> {
    ensure!(
        deadline.is_none_or(|end| Instant::now() < end),
        "Python 分析超过期限"
    );
    Ok(())
}

impl PythonProject {
    /// 配置入口只受原生 exclude 约束；忽略 include 和脚本开关，仍复用原快照。
    pub(crate) fn configuration_view(&self) -> Result<Self> {
        let mut options = self
            .source_options
            .clone()
            .context("Python 发现缺少 src 政策")?;
        let table = options.as_table_mut().context("Python src 必须为表")?;
        table.remove("include");
        table.remove("exclude-scripts");
        from_snapshot(
            self.input.clone(),
            self.snapshot.clone(),
            self.files.clone(),
            Some(options),
        )
    }

    /// 公开原生 API 同时检查控制文件及祖先排除；不假称提供 ignore-file 语义。
    pub(crate) fn admits_configuration(&self, path: &Path) -> Result<bool> {
        Ok(self
            .db
            .project()
            .is_file_included(&self.db, &system_path(path)?)
            .is_included())
    }

    /// 完成前核验整个已加载上下文，投影和发现都不能混入并发更新后的源码。
    pub(crate) fn ensure_unchanged(&self, deadline: Instant) -> Result<()> {
        let repository = self.input.repository.canonicalize()?;
        for relative in &self.files {
            checkpoint(Some(deadline))?;
            let path = repository.join(relative);
            check_path(&repository, &path)?;
            ensure!(
                fs::read_to_string(&path)? == self.source(relative)?,
                "分析期间源码已改变：{}",
                relative.display()
            );
        }
        Ok(())
    }

    /// 原生索引应用 src 规则；成员根独立于 extra-paths 和已加载上下文。
    pub(crate) fn discover(
        &mut self,
        roots: &[PathBuf],
        deadline: Instant,
    ) -> Result<Vec<PathBuf>> {
        let project = self.db.project();
        project.set_included_paths(
            &mut self.db,
            roots
                .iter()
                .map(|path| system_path(path))
                .collect::<Result<_>>()?,
        );
        project.set_force_exclude(&mut self.db, true);
        let mut output = Vec::new();
        for file in project.files(&self.db).iter() {
            checkpoint(Some(deadline))?;
            let path = file
                .path(&self.db)
                .as_system_path()
                .context("Python 成员不是仓库源码")?;
            let relative = path.strip_prefix(snapshot_root())?.as_std_path().to_owned();
            ensure!(self.files.contains(&relative), "Python 索引引入快照外源码");
            output.push(relative);
            ensure!(
                output.len() <= codegraph_engine_common::selection::MAX_FILES,
                "文件选集超过数量上限"
            );
        }
        output.sort();
        Ok(output)
    }

    /// 目标版本变化不触发第二次磁盘加载，配置与源文件保持同一快照。
    pub(crate) fn with_minor(&self, minor: u8) -> Result<Self> {
        if self.input.python_minor == minor {
            return Ok(self.clone());
        }
        let mut input = self.input.clone();
        input.python_minor = minor;
        from_snapshot(
            input,
            self.snapshot.clone(),
            self.files.clone(),
            self.source_options.clone(),
        )
    }
}

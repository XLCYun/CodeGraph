//! Cargo 与工具链政策是明确的类型，调用者不能注入命令或任意环境覆盖。

use anyhow::{Context, Result, bail, ensure};
use hir_def::{
    AdtId, ExpressionStoreOwnerId, GenericDefId, ModuleDefId,
    expr_store::ExpressionStore,
    nameres::{DefMap, crate_def_map, diagnostics::DefDiagnosticKind},
};
use ide::RootDatabase;
use project_model::{
    CargoConfig, ProjectManifest, ProjectWorkspace, ProjectWorkspaceKind, RustLibSource, TargetKind,
};
use std::{
    collections::{BTreeMap, BTreeSet, HashSet},
    path::{Path, PathBuf},
    sync::Mutex,
};
use vfs::loader::{Handle, LoadingProgress};
use vfs::{AbsPathBuf, Vfs};

/// 数据库和工作区成员身份在同一次加载中冻结，VFS 上下文不自动成为发现选集。
pub(super) struct LoadedProject {
    pub database: RootDatabase,
    pub vfs: Vfs,
    pub member_roots: BTreeMap<PathBuf, MemberRoot>,
}

/// 记录目标的真实归属，既支持仓库子目录调用，也不能静默丢弃仓内清单声明的越界根。
pub(super) struct MemberRoot {
    pub test_only: bool,
    owners: BTreeSet<PathBuf>,
}

/// 物理模块与可执行 include 的归属分别保留，避免原生导航的调用文件代理丢失测试角色。
#[derive(Default)]
pub(crate) struct DiscoveredSource {
    pub module: bool,
    pub includes: BTreeSet<IncludeOrigin>,
}

/// 原始调用位置与展开链测试条件共同标记归属，同一物理位置的不同展开角色不能折叠。
#[derive(PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct IncludeOrigin {
    pub path: PathBuf,
    pub offset: syntax::TextSize,
    pub test_only: bool,
}

/// 与现有 Rust 设置对应的三档语义依赖范围。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DependencyScope {
    RepositoryOnly,
    StandardLibrary,
    AllDependencies,
}

/// 全部依赖档沿用用户网络政策，其余两档始终离线。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NetworkPolicy {
    Offline,
    Allow,
}

/// 已由项目工具链发现阶段确认的路径，与 helper 的构建工具链相互独立。
#[derive(Clone, Debug)]
pub struct ProjectToolchain {
    pub sysroot: PathBuf,
    pub rust_src: PathBuf,
}

/// 单次加载的明确政策；测试 cfg 不等同于后续图中测试节点的过滤开关。
#[derive(Clone, Debug)]
pub struct LoadPolicy {
    pub dependency_scope: DependencyScope,
    pub network: NetworkPolicy,
    pub test_cfg: bool,
    pub project_toolchain: Option<ProjectToolchain>,
}

impl LoadPolicy {
    /// 构造默认的仅仓库离线分析，沿用旧入口启用 cfg(test) 的语义上下文。
    pub fn repository_only() -> Self {
        Self {
            dependency_scope: DependencyScope::RepositoryOnly,
            network: NetworkPolicy::Offline,
            test_cfg: true,
            project_toolchain: None,
        }
    }

    /// 只对必需的项目组件做存在检查，不使用会自动安装 rust-src 的 discover 分支。
    fn cargo_config(&self) -> Result<CargoConfig> {
        let mut config = CargoConfig {
            set_test: self.test_cfg,
            no_deps: self.dependency_scope == DependencyScope::RepositoryOnly,
            ..CargoConfig::default()
        };
        config.extra_env.insert(
            "PATH".into(),
            Some(
                crate::discovery::runtime_path()?
                    .into_string()
                    .map_err(|_| anyhow::anyhow!("项目 PATH 必须为 UTF-8"))?,
            ),
        );
        config
            .extra_env
            .insert("RUSTUP_AUTO_INSTALL".into(), Some("0".into()));
        // 即使 target 已存在，Cargo 的版本探测也不能在其中写入 .rustc_info.json。
        config
            .extra_env
            .insert("CARGO_CACHE_RUSTC_INFO".into(), Some("0".into()));
        // 用户/工厂提供的 RUSTUP_TOOLCHAIN 继续影响 metadata 与 rustc；构建隔离由测试入口负责。
        if self.dependency_scope != DependencyScope::AllDependencies
            || self.network == NetworkPolicy::Offline
        {
            config
                .extra_env
                .insert("CARGO_NET_OFFLINE".into(), Some("true".into()));
        }
        if self.dependency_scope == DependencyScope::StandardLibrary {
            // 只限制项目 metadata；不能用 no_deps 截断标准库内部依赖。
            config.metadata_extra_args.push("--no-deps".into());
        }
        if self.dependency_scope != DependencyScope::RepositoryOnly {
            let toolchain = self
                .project_toolchain
                .as_ref()
                .context("标准库分析需要已发现的项目工具链与 rust-src")?;
            ensure!(
                toolchain.sysroot.is_dir(),
                "项目 sysroot 不存在：{}",
                toolchain.sysroot.display()
            );
            ensure!(
                toolchain.rust_src.join("core/src/lib.rs").is_file()
                    && toolchain.rust_src.join("std/src/lib.rs").is_file(),
                "项目工具链缺少 rust-src：{}；不会自动安装组件",
                toolchain.rust_src.display()
            );
            let sysroot = absolute_utf8(&toolchain.sysroot)?;
            let sources = absolute_utf8(&toolchain.rust_src)?;
            // Cargo/rustc 共同使用已按用户覆盖发现并验证的项目 sysroot。
            config
                .extra_env
                .insert("RUSTUP_TOOLCHAIN".into(), Some(sysroot.to_string()));
            config.sysroot = Some(RustLibSource::Path(sysroot));
            config.sysroot_src = Some(sources);
        }
        Ok(config)
    }
}

/// 一次加载事务隔离原生内部异常；失败即丢弃数据库，不探测输入、不重试或返回部分结果。
pub(super) fn load(manifest: &Path, policy: &LoadPolicy) -> Result<LoadedProject> {
    load_projects(&[manifest.to_owned()], std::slice::from_ref(policy), &[])
}

/// 将多个真实项目一次装入同一 VFS/数据库；仅组合固定上游公开的 crate 图与文件加载原语。
pub(super) fn load_projects(
    manifests: &[PathBuf],
    policies: &[LoadPolicy],
    selected: &[PathBuf],
) -> Result<LoadedProject> {
    crate::native_transaction("Rust 多项目工作区未完整加载", || {
        ensure!(
            manifests.len() == policies.len(),
            "项目清单与加载政策数量不一致"
        );
        let mut workspaces = Vec::new();
        let mut locks = Vec::new();
        for (manifest, policy) in manifests.iter().zip(policies) {
            let mut config = policy.cargo_config()?;
            let manifest = ProjectManifest::from_manifest_file(absolute_utf8(manifest)?)?;
            let workspace = if policy.dependency_scope == DependencyScope::AllDependencies
                && matches!(manifest, ProjectManifest::CargoToml(_))
            {
                let directory = tempfile::tempdir()?;
                let mut discovery = config.clone();
                discovery.metadata_extra_args.push("--no-deps".into());
                let standard = load_complete(
                    manifest.clone(),
                    &discovery,
                    &LoadPolicy {
                        dependency_scope: DependencyScope::StandardLibrary,
                        ..policy.clone()
                    },
                )?;
                redirect_project_lockfile(&mut config, &standard, directory.path())?;
                let complete =
                    if standard.toolchain.as_ref().is_some_and(|version| {
                        version.major == 1 && (82..95).contains(&version.minor)
                    }) {
                        let complete = crate::readonly_metadata::complete(standard, &config)?;
                        ensure_complete(&complete, &[], policy)?;
                        complete
                    } else {
                        config.sysroot = None;
                        config.sysroot_src = None;
                        let mut complete = load_complete(
                            manifest,
                            &config,
                            &LoadPolicy {
                                dependency_scope: DependencyScope::RepositoryOnly,
                                ..policy.clone()
                            },
                        )?;
                        complete.sysroot = standard.sysroot;
                        complete
                    };
                locks.push(directory);
                complete
            } else {
                load_complete(manifest, &config, policy)?
            };
            workspaces.push(workspace);
        }
        ensure!(
            !workspaces.is_empty(),
            "未发现 Rust workspace，无法完整加载 standalone 源码"
        );
        assemble(workspaces, &policies[0].cargo_config()?, selected)
    })
    .context("Rust 工作区未完整加载")
}

/// 对齐上游 reload::ws_to_crate_graph 与 load-cargo 的固定侧装载，不启用监听、宏服务或构建。
fn assemble(
    workspaces: Vec<ProjectWorkspace>,
    config: &CargoConfig,
    selected: &[PathBuf],
) -> Result<LoadedProject> {
    let member_roots = member_roots(&workspaces)?;
    let (sender, receiver) = crossbeam_channel::unbounded();
    let mut loader = vfs_notify::NotifyHandle::spawn(sender);
    let mut vfs = Vfs::default();
    let mut graph = base_db::CrateGraphBuilder::default();
    let mut disabled_macros = Vec::new();
    for workspace in &workspaces {
        let (other, mut macros) = workspace.to_crate_graph(
            &mut |path| {
                let contents = loader.load_sync(path);
                let path = vfs::VfsPath::from(path.to_path_buf());
                vfs.set_file_contents(path.clone(), contents);
                vfs.file_id(&path)
                    .and_then(|(id, excluded)| (excluded == vfs::FileExcluded::No).then_some(id))
            },
            &config.extra_env,
        );
        graph.extend(other, &mut macros);
        disabled_macros.extend(
            macros
                .into_keys()
                .map(|id| (id, Err(base_db::ProcMacroLoadingError::Disabled))),
        );
    }
    let folders = load_cargo::ProjectFolders::new(&workspaces, &[], None);
    loader.set_config(vfs::loader::Config {
        load: folders.load,
        watch: vec![],
        version: 0,
    });
    for message in receiver {
        match message {
            vfs::loader::Message::Progress {
                n_done: LoadingProgress::Finished,
                ..
            } => break,
            vfs::loader::Message::Loaded { files } | vfs::loader::Message::Changed { files } => {
                for (path, contents) in files {
                    vfs.set_file_contents(path.into(), contents);
                }
            }
            _ => {}
        }
    }
    // 选集决定输出，未激活 cfg 的选中文件也必须从同一 VFS 获得源码和 AST。
    for path in selected {
        let absolute = absolute_utf8(path)?;
        let contents =
            std::fs::read(&absolute).with_context(|| format!("选中源码无法读取：{absolute}"))?;
        vfs.set_file_contents(absolute.into(), Some(contents));
    }
    let mut change = ide_db::ChangeWithProcMacros::default();
    for (_, file) in vfs.take_changes() {
        if let vfs::Change::Create(bytes, _) | vfs::Change::Modify(bytes, _) = file.change
            && let Ok(text) = String::from_utf8(bytes)
        {
            change.change_file(file.file_id, Some(text));
        }
    }
    change.set_roots(folders.source_root_config.partition(&vfs));
    change.set_crate_graph(graph);
    change.set_proc_macros(disabled_macros.into_iter().collect());
    let mut database = RootDatabase::new(None);
    database.enable_proc_attr_macros();
    database.apply_change(change);
    ensure!(
        member_roots.is_empty() || vfs.iter().next().is_some(),
        "Rust 工作区未加载任何成员源码"
    );
    ensure_sources_complete(&database, &vfs)?;
    Ok(LoadedProject {
        database,
        vfs,
        member_roots,
    })
}

/// 只记录 Cargo workspace 成员或静态项目明确标记的成员，路径依赖仍只是上下文。
fn member_roots(workspaces: &[ProjectWorkspace]) -> Result<BTreeMap<PathBuf, MemberRoot>> {
    let mut roots = BTreeMap::new();
    for workspace in workspaces {
        match &workspace.kind {
            ProjectWorkspaceKind::Cargo { cargo, .. } => {
                for package in cargo.packages().filter(|&package| cargo[package].is_member) {
                    let owner = dunce::canonicalize(cargo[package].manifest.parent().as_str())?;
                    for &target in &cargo[package].targets {
                        let target = &cargo[target];
                        let path = dunce::canonicalize(target.root.as_str())?;
                        let test_only = matches!(target.kind, TargetKind::Test | TargetKind::Bench);
                        record_member(&mut roots, path, owner.clone(), test_only);
                    }
                }
            }
            ProjectWorkspaceKind::Json(project) => {
                let owner = dunce::canonicalize(project.project_root().as_str())?;
                for (_, krate) in project
                    .crates()
                    .filter(|(_, krate)| krate.is_workspace_member)
                {
                    record_member(
                        &mut roots,
                        dunce::canonicalize(krate.root_module.as_str())?,
                        owner.clone(),
                        false,
                    );
                }
            }
            _ => {}
        }
    }
    Ok(roots)
}

/// 多个配置或生产/测试目标复用物理根时合并归属，任一生产目标都使根继续可见。
fn record_member(
    roots: &mut BTreeMap<PathBuf, MemberRoot>,
    path: PathBuf,
    owner: PathBuf,
    test_only: bool,
) {
    let member = roots.entry(path).or_insert_with(|| MemberRoot {
        test_only,
        owners: BTreeSet::new(),
    });
    member.test_only &= test_only;
    member.owners.insert(owner);
}

/// 从成员 crate、块模块图及实际 include 展开发现源码，不枚举 VFS 或按扩展名扫描目录。
pub(super) fn discover_sources(
    database: &RootDatabase,
    vfs: &Vfs,
    member_roots: &BTreeMap<PathBuf, MemberRoot>,
    repository: &Path,
) -> Result<BTreeMap<PathBuf, DiscoveredSource>> {
    let mut files: BTreeMap<PathBuf, DiscoveredSource> = BTreeMap::new();
    let mut crates = Vec::new();
    let mut expected = BTreeSet::new();
    for (path, member) in member_roots {
        if path.starts_with(repository)
            || member
                .owners
                .iter()
                .any(|owner| owner.starts_with(repository))
        {
            ensure!(path.starts_with(repository), "仓内项目声明的目标根越出仓库");
            expected.insert(path.clone());
            ensure!(
                expected.len() <= codegraph_engine_common::selection::MAX_FILES,
                "发现文件超过数量上限"
            );
        }
    }
    let mut remaining = expected.clone();
    for &krate in base_db::all_crates(database).iter() {
        let root = vfs.file_path(krate.root_file_id(database).file_id(database));
        let root = root.as_path().context("crate 根没有磁盘路径")?;
        let root = dunce::canonicalize(root.as_str())?;
        if expected.contains(&root) {
            remaining.remove(&root);
            crates.push(krate);
        }
    }
    ensure!(remaining.is_empty(), "声明的工作区成员目标未完整加载");
    let mut calls = HashSet::new();
    visit_native_scopes(database, crates.into_iter(), |scope| {
        match scope {
            NativeScope::Module(map) => {
                for (_, module) in map.modules() {
                    if let Some(file) = module.origin.file_id() {
                        let path = discovered_source_path(database, vfs, repository, file)?;
                        files.entry(path).or_default().module = true;
                        ensure!(
                            files.len() <= codegraph_engine_common::selection::MAX_FILES,
                            "发现文件超过数量上限"
                        );
                    }
                    calls.extend(module.scope.all_macro_calls());
                }
            }
            NativeScope::Expression(owner) => {
                calls.extend(
                    ExpressionStore::with_source_map(database, owner)
                        .1
                        .macro_calls()
                        .map(|(_, call)| call),
                );
            }
        }
        Ok(())
    })?;
    for call in calls {
        let location = call.loc(database);
        // 只有原生已解析的可执行 include 返回文件；字符串/字节数据和未解析调用没有此身份。
        let Some(file) = location.include_file_id(database, call) else {
            continue;
        };
        let path = discovered_source_path(database, vfs, repository, file)?;
        let mut expansion = location.to_node(database);
        let origin = expansion.clone().original_file_range_rooted(database);
        // 先检查每层原生 AST 的模块/表达式祖先，再回到上一层调用；只保留最外层位置会丢失生成的属性。
        let test_only = loop {
            if expansion
                .value
                .ancestors()
                .any(|node| crate::projection::test_code::is_test_only(&node))
            {
                break true;
            }
            let Some(parent) = expansion.file_id.macro_file() else {
                break false;
            };
            expansion = parent.loc(database).to_node(database);
        };
        let origin_path = discovered_source_path(database, vfs, repository, origin.file_id)?;
        files
            .entry(path)
            .or_default()
            .includes
            .insert(IncludeOrigin {
                path: origin_path,
                offset: origin.range.start(),
                test_only,
            });
        ensure!(
            files.len() <= codegraph_engine_common::selection::MAX_FILES,
            "发现文件超过数量上限"
        );
    }
    Ok(files)
}

/// 模块和 include 使用相同的真实路径边界，原文可读性由随后同快照的 Document 校验。
fn discovered_source_path(
    database: &RootDatabase,
    vfs: &Vfs,
    repository: &Path,
    file: base_db::EditionedFileId,
) -> Result<PathBuf> {
    let path = vfs.file_path(file.file_id(database));
    let path = path.as_path().context("源码没有磁盘路径")?;
    let path = dunce::canonicalize(path.as_str())?;
    ensure!(
        path.starts_with(repository) && path.is_file(),
        "发现的源码越出仓库或不是普通文件"
    );
    ensure!(path.to_str().is_some(), "发现的源码路径必须为 UTF-8");
    Ok(path)
}

/// 上游会丢弃读取错误；沿原生 crate 与 block 模块图检查活动声明的源码候选。
fn ensure_sources_complete(database: &RootDatabase, vfs: &Vfs) -> Result<()> {
    visit_native_scopes(
        database,
        base_db::all_crates(database).iter().copied(),
        |scope| match scope {
            NativeScope::Module(map) => ensure_module_candidates_complete(database, vfs, map),
            NativeScope::Expression(_) => Ok(()),
        },
    )
}

/// 遍历回调区分模块与表达式；完整性检查无需为 include 归属额外计算来源映射。
enum NativeScope<'a> {
    Module(&'a DefMap),
    Expression(ExpressionStoreOwnerId),
}

/// 共用原生作用域遍历，涵盖函数、关联项、闭包、常量和签名中的块。
fn visit_native_scopes(
    database: &RootDatabase,
    crates: impl Iterator<Item = base_db::Crate>,
    mut visit: impl FnMut(NativeScope<'_>) -> Result<()>,
) -> Result<()> {
    // 读取/遍历失败的文件不在 VFS 中，使用原生 cfg、#[path] 与嵌套目录解析结果定位。
    let mut maps: Vec<_> = crates.map(|krate| crate_def_map(database, krate)).collect();
    let mut definitions = Vec::new();
    let mut stores = Vec::new();
    let mut checked_stores = HashSet::new();
    let mut checked_blocks = HashSet::new();
    loop {
        if let Some(map) = maps.pop() {
            visit(NativeScope::Module(map))?;
            for (_, module) in map.modules() {
                definitions.extend(module.scope.declarations());
                definitions.extend(module.scope.unnamed_consts().map(ModuleDefId::from));
                for implementation in module.scope.impls() {
                    stores.push(implementation.into());
                    definitions.extend(
                        implementation
                            .impl_items(database)
                            .items
                            .iter()
                            .map(|&(_, item)| ModuleDefId::from(item)),
                    );
                }
            }
        } else if let Some(definition) = definitions.pop() {
            queue_definition_stores(database, definition, &mut definitions, &mut stores);
        } else if let Some(owner) = stores.pop() {
            if checked_stores.insert(owner) {
                visit(NativeScope::Expression(owner))?;
                for (block, map) in ExpressionStore::of(database, owner).blocks(database) {
                    if checked_blocks.insert(block) {
                        maps.push(map);
                    }
                }
            }
        } else {
            break;
        }
    }
    Ok(())
}

/// 沿原生声明取得 body、签名和字段中的块，关联项和块内嵌套声明都继续检查。
fn queue_definition_stores(
    database: &RootDatabase,
    definition: ModuleDefId,
    definitions: &mut Vec<ModuleDefId>,
    stores: &mut Vec<ExpressionStoreOwnerId>,
) {
    match definition {
        ModuleDefId::FunctionId(id) => {
            stores.extend([
                ExpressionStoreOwnerId::Signature(id.into()),
                ExpressionStoreOwnerId::Body(id.into()),
            ]);
        }
        ModuleDefId::ConstId(id) => {
            stores.extend([
                ExpressionStoreOwnerId::Signature(id.into()),
                ExpressionStoreOwnerId::Body(id.into()),
            ]);
        }
        ModuleDefId::StaticId(id) => {
            stores.extend([
                ExpressionStoreOwnerId::Signature(id.into()),
                ExpressionStoreOwnerId::Body(id.into()),
            ]);
        }
        ModuleDefId::EnumVariantId(id) => {
            stores.extend([
                ExpressionStoreOwnerId::VariantFields(id.into()),
                ExpressionStoreOwnerId::Body(id.into()),
            ]);
        }
        ModuleDefId::AdtId(AdtId::EnumId(id)) => {
            stores.push(GenericDefId::from(id).into());
            definitions.extend(
                id.enum_variants(database)
                    .variants
                    .values()
                    .map(|&(variant, _)| ModuleDefId::from(variant)),
            );
        }
        ModuleDefId::AdtId(AdtId::StructId(id)) => {
            stores.extend([
                ExpressionStoreOwnerId::Signature(id.into()),
                ExpressionStoreOwnerId::VariantFields(id.into()),
            ]);
        }
        ModuleDefId::AdtId(AdtId::UnionId(id)) => {
            stores.extend([
                ExpressionStoreOwnerId::Signature(id.into()),
                ExpressionStoreOwnerId::VariantFields(id.into()),
            ]);
        }
        ModuleDefId::TraitId(id) => {
            stores.push(GenericDefId::from(id).into());
            definitions.extend(
                id.trait_items(database)
                    .items
                    .iter()
                    .map(|&(_, item)| ModuleDefId::from(item)),
            );
        }
        ModuleDefId::TypeAliasId(id) => stores.push(GenericDefId::from(id).into()),
        ModuleDefId::ModuleId(_) | ModuleDefId::MacroId(_) | ModuleDefId::BuiltinType(_) => {}
    }
}

/// crate 与 block 的 DefMap 使用相同候选检查，真正缺失模块仍保留为项目诊断。
fn ensure_module_candidates_complete(
    database: &RootDatabase,
    vfs: &Vfs,
    map: &DefMap,
) -> Result<()> {
    for diagnostic in map.diagnostics() {
        let DefDiagnosticKind::UnresolvedModule { ast, candidates } = &diagnostic.kind else {
            continue;
        };
        let anchor = ast
            .file_id
            .original_file_respecting_includes(database)
            .file_id(database);
        let directory = vfs
            .file_path(anchor)
            .parent()
            .context("未解析模块的源码没有父目录")?;
        for candidate in candidates {
            let candidate = directory.join(candidate).context("模块候选路径不可用")?;
            let path = candidate.as_path().context("模块候选没有真实源码路径")?;
            match std::fs::read_to_string(path) {
                // 真正缺失的模块属于项目诊断；现有源码被 loader 漏掉才是加载错误。
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => {
                    return Err(error).with_context(|| format!("Rust 模块源码未完整加载：{path}"));
                }
                Ok(_) => bail!("Rust 模块源码未完整加载到 VFS：{path}"),
            }
        }
    }
    Ok(())
}

/// 对单次原生加载收集进度错误，不能把上游的部分 workspace 当成完整结果。
fn load_complete(
    manifest: ProjectManifest,
    config: &CargoConfig,
    policy: &LoadPolicy,
) -> Result<ProjectWorkspace> {
    let progress = Mutex::new(Vec::new());
    let workspace = ProjectWorkspace::load(manifest, config, &|message| {
        progress.lock().expect("加载进度锁被破坏").push(message);
    })
    .with_context(|| {
        if config
            .metadata_extra_args
            .iter()
            .any(|arg| arg == "--locked")
        {
            "只读分析不能更新此项目的 Cargo.lock；请先准备与清单一致的锁文件"
        } else {
            "Rust 原生项目加载失败"
        }
    })?;
    ensure_complete(
        &workspace,
        &progress.into_inner().expect("加载进度锁被破坏"),
        policy,
    )
    .with_context(|| {
        if config
            .metadata_extra_args
            .iter()
            .any(|arg| arg == "--locked")
        {
            "只读分析不能更新此项目的 Cargo.lock；请先准备与清单一致的锁文件"
        } else {
            "Rust 原生项目加载未完成"
        }
    })?;
    Ok(workspace)
}

/// 沿用固定上游按真实 Cargo 版本选择的锁文件机制，补齐缺失锁文件的只读处理。
fn redirect_project_lockfile(
    config: &mut CargoConfig,
    workspace: &ProjectWorkspace,
    directory: &Path,
) -> Result<()> {
    let version = workspace
        .toolchain
        .as_ref()
        .context("无法确定项目 Cargo 工具链版本")?;
    if (version.major, version.minor) < (1, 82) {
        // 老 Cargo 不支持独立锁文件；已有有效锁文件仍可解析，只拒绝需要写回的情况。
        config.metadata_extra_args.push("--locked".into());
        return Ok(());
    }
    let ProjectWorkspaceKind::Cargo { cargo, .. } = &workspace.kind else {
        bail!("只读锁文件处理需要 Cargo workspace");
    };
    let original = cargo.workspace_root().join("Cargo.lock");
    let temporary = directory.join("Cargo.lock");
    if Path::new(original.as_str()).exists() {
        std::fs::copy(&original, &temporary).context("无法复制项目的 Cargo.lock 解析约束")?;
    }
    let temporary = temporary
        .to_str()
        .context("临时 metadata 锁文件路径必须为 UTF-8")?;
    if (version.major, version.minor) >= (1, 97) {
        config.extra_env.insert(
            "CARGO_RESOLVER_LOCKFILE_PATH".into(),
            Some(temporary.into()),
        );
    } else {
        config.extra_env.insert(
            "__CARGO_TEST_CHANNEL_OVERRIDE_DO_NOT_USE_THIS".into(),
            Some("nightly".into()),
        );
        config.metadata_extra_args.push("-Zunstable-options".into());
        if (version.major, version.minor) >= (1, 95) {
            config.extra_env.insert(
                "CARGO_RESOLVER_LOCKFILE_PATH".into(),
                Some(temporary.into()),
            );
            config.metadata_extra_args.push("-Zlockfile-path".into());
        } else {
            config
                .metadata_extra_args
                .extend(["--lockfile-path".into(), temporary.into()]);
        }
    }
    Ok(())
}

/// 原生 API 的 Ok 可能包含 metadata 回退；这些情况必须转成可观察失败。
fn ensure_complete(
    workspace: &ProjectWorkspace,
    progress: &[String],
    policy: &LoadPolicy,
) -> Result<()> {
    if let ProjectWorkspaceKind::Cargo {
        error: Some(error), ..
    } = &workspace.kind
    {
        bail!("Cargo 工作区未完整加载：{error:#}");
    }
    if let ProjectWorkspaceKind::Cargo { cargo, .. } = &workspace.kind {
        for package in cargo.packages() {
            for target in &cargo[package].targets {
                let root = &cargo[*target].root;
                std::fs::read_to_string(root)
                    .with_context(|| format!("Cargo crate 根源码未完整加载：{root}"))?;
            }
        }
    }
    if let ProjectWorkspaceKind::Json(project) = &workspace.kind {
        for (_, krate) in project.crates() {
            std::fs::read_to_string(&krate.root_module)
                .with_context(|| format!("JSON crate 根源码未完整加载：{}", krate.root_module))?;
        }
    }
    if let Some(error) = workspace
        .sysroot
        .error()
        .or_else(|| workspace.sysroot.metadata_error())
    {
        bail!("Rust sysroot 未完整加载：{error}");
    }
    if let Err(error) = &workspace.target {
        bail!("Rust 目标数据未完整加载：{error}");
    }
    ensure!(
        workspace.toolchain.is_some() && !workspace.rustc_cfg.is_empty(),
        "项目工具链或目标 cfg 未完整加载"
    );
    if let Some(error) = progress
        .iter()
        .find(|line| line.starts_with("cargo metadata: failed"))
    {
        bail!("Rust metadata 发生降级：{error}");
    }
    if policy.dependency_scope != DependencyScope::RepositoryOnly
        && matches!(workspace.kind, ProjectWorkspaceKind::Cargo { .. })
    {
        ensure!(
            workspace.sysroot.num_packages() > 0,
            "Rust 标准库未完整加载"
        );
    }
    Ok(())
}

/// 将真实路径转换为上游统一磁盘身份；安全简化 Windows 前缀，保留长路径与特殊名称。
fn absolute_utf8(path: &Path) -> Result<AbsPathBuf> {
    let absolute =
        dunce::canonicalize(path).with_context(|| format!("路径不可用：{}", path.display()))?;
    ensure!(
        absolute.to_str().is_some(),
        "路径必须为 UTF-8 绝对路径：{}",
        path.display()
    );
    Ok(AbsPathBuf::assert_utf8(absolute))
}

#[cfg(test)]
#[path = "tests/loading.rs"]
mod tests;

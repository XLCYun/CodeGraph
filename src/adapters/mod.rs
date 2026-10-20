//! 固定语言适配器装配私有引擎与目标 SDK；共享运行时不选择语言。
mod c_cpp;
mod csharp;
mod go;
mod java;
mod platforms;
mod python;
mod rust;
mod typescript;
pub use c_cpp::CCpp;
pub use csharp::CSharp;
pub use go::Go;
pub use java::Java;
pub use python::Python;
pub use rust::Rust;
#[cfg(test)]
use rust::RustPackAnalyzer;
pub use typescript::TypeScriptJavaScript;

use crate::{
    code_graph::{AnalysisError, AnalysisInput, CodeGraph, LanguageAnalyzer, RustDependencyScope},
    native_analyzer::{
        CancellationCheck, HelperCommand, HelperIdentity, NativeLanguageAnalyzer, Platform,
    },
    pack::PackId,
};
use serde::Deserialize;
#[cfg(test)]
use std::fs;
use std::{
    collections::BTreeMap,
    path::{Component, Path, PathBuf},
};

/// 各适配器的静态身份；包描述不能改写入口、语言、投影或 SDK 政策。
pub(crate) struct EngineDefinition {
    helper: &'static str,
    projection: &'static str,
    languages: &'static [&'static str],
    profile: &'static str,
    platform_kinds: &'static [&'static str],
    requires_platforms: bool,
}

/// 只由本库实现的语言绑定；监督层拿到已绑定的类型，不做运行时语言分派。
pub(crate) trait Adapter: Copy + Send + 'static {
    const ID: PackId;
    const ENGINE: EngineDefinition;

    /// 在已经规范化的包根内构造该语言的固定引擎命令。
    fn command(self, root: &Path) -> Result<HelperCommand, AnalysisError>;

    /// 默认语言只消费随包平台，不接受额外的目标 SDK 声明。
    fn target_platforms(self, _root: &Path, _kind: &str) -> Result<Vec<Platform>, AnalysisError> {
        Err(failure("分析器不支持该目标 SDK 声明"))
    }

    /// 共享读取与边界校验只接受具体适配器的固定政策。
    fn engine(self, root: &Path) -> Result<NativeLanguageAnalyzer, AnalysisError> {
        let root = root.canonicalize().map_err(failure)?;
        let bytes =
            crate::pack::read_control(&inside(&root, "control/native.json")?).map_err(failure)?;
        let crate::strict_json::UniqueValue(value) =
            serde_json::from_slice(&bytes).map_err(failure)?;
        let control: PackControl = serde_json::from_value(value).map_err(failure)?;
        if control.schema_version != 1 || control.helper_version != env!("CARGO_PKG_VERSION") {
            return Err(failure("分析器引擎控制文件版本不匹配"));
        }
        let command = self.command(&root)?;
        if !command.executable.is_file() {
            return Err(failure("分析器包缺少引擎执行入口"));
        }
        let mut platforms = Vec::new();
        for platform in control.platforms {
            if !Self::ENGINE
                .platform_kinds
                .contains(&platform.kind.as_str())
            {
                return Err(failure("分析器包的平台类型与语言不符"));
            }
            let platform_root = inside(&root, &platform.root)?;
            if !platform_root.is_dir() {
                return Err(failure("分析器包目标声明目录不存在"));
            }
            platforms.push(Platform {
                id: platform.id,
                kind: platform.kind,
                version: platform.version,
                root: platform_root,
                metadata: platform.metadata,
            });
        }
        if let Some(sdk) = control.target_sdk {
            platforms.extend(self.target_platforms(&root, &sdk.kind)?);
        }
        if platforms.is_empty() && Self::ENGINE.requires_platforms {
            return Err(failure(
                "target_unavailable: 分析器包未提供经过验证的目标标准库",
            ));
        }
        Ok(NativeLanguageAnalyzer::new(
            command,
            HelperIdentity {
                helper_id: Self::ENGINE.helper.into(),
                helper_version: control.helper_version,
                projection_revision: Self::ENGINE.projection.into(),
                languages: Self::ENGINE
                    .languages
                    .iter()
                    .map(|language| (*language).into())
                    .collect(),
                profile: Self::ENGINE.profile.into(),
            },
            platforms,
        )
        .with_package_root(root))
    }

    /// 普通语言通过私有 Host 完成来源与图校验，Rust 在自己的适配器覆盖依赖政策。
    fn analyze(
        self,
        root: &Path,
        input: &AnalysisInput,
        cancelled: CancellationCheck,
        _scope: RustDependencyScope,
    ) -> Result<CodeGraph, AnalysisError> {
        self.engine(root)?
            .with_cancellation(cancelled)
            .analyze(input)
    }
}

/// 私有引擎描述只声明平台，不能扩展成任意执行命令。
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PackControl {
    schema_version: u32,
    helper_version: String,
    platforms: Vec<BundledPlatform>,
    #[serde(default)]
    target_sdk: Option<TargetSdk>,
}

/// 目标 SDK 的具体版本与目录仍从本机已有安装发现。
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct TargetSdk {
    kind: String,
}

/// 随包目标声明只能指向包内已验证目录。
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct BundledPlatform {
    id: String,
    kind: String,
    version: String,
    root: String,
    metadata: serde_json::Value,
}

/// 只保留 Windows 装载器所需环境；各语言自行补充固定运行时设置。
fn command(root: &Path, entry: &str) -> Result<HelperCommand, AnalysisError> {
    let mut environment = BTreeMap::new();
    if cfg!(windows) {
        if let Some(system_root) = std::env::var_os("SystemRoot") {
            environment.insert("SystemRoot".into(), system_root);
        }
    }
    Ok(HelperCommand {
        executable: inside(root, entry)?,
        args: vec![],
        environment,
        trusted_roots: vec![root.into()],
    })
}

/// 固定入口和目标目录必须留在支持包中，不接受链接逃逸或路径组件歧义。
fn inside(root: &Path, relative: &str) -> Result<PathBuf, AnalysisError> {
    let path = Path::new(relative);
    if relative.is_empty()
        || relative.contains('\\')
        || relative.contains(':')
        || path
            .components()
            .any(|part| !matches!(part, Component::Normal(_)))
    {
        return Err(failure("支持包相对路径无效"));
    }
    let resolved = root.join(path).canonicalize().map_err(failure)?;
    if !resolved.starts_with(root) {
        return Err(failure("支持包入口越界"));
    }
    Ok(resolved)
}

/// 能力准备失败保留明确诊断，不能映射为空代码图。
fn failure(error: impl std::fmt::Display) -> AnalysisError {
    AnalysisError::new(format!("原生语言支持包：{error}"))
}

#[cfg(test)]
#[path = "tests/factory.rs"]
mod tests;

#[cfg(test)]
#[path = "tests/typescript_graph.rs"]
mod typescript_graph;

#[cfg(test)]
#[path = "tests/csharp_graph.rs"]
mod csharp_graph;

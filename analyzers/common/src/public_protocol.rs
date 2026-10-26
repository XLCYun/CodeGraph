//! 语言程序直接消费的公共一次性请求，拒绝未知字段及递归重复键。
use crate::strict_json::UniqueValue;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    collections::BTreeSet,
    path::{Path, PathBuf},
};

pub const PROTOCOL: &str = "codegraph.analyzer/1";
pub const ENGINE_VERSION: &str = "0.1.2";
pub const MAX_BYTES: usize = 128 * 1024 * 1024;

/// Rust 的三档依赖政策；其他语言接受该公共字段但不应用它。
#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum RustDependencyScope {
    RepositoryOnly,
    StandardLibrary,
    AllDependencies,
}

/// 固定分析选项不允许注入命令、解释器、编译参数或 SDK 路径。
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Options {
    pub include_tests: bool,
    pub rust_dependency_scope: RustDependencyScope,
}

/// 一次进程只接收一个公共分析请求，files 缺省表示配置发现。
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    pub protocol: String,
    #[serde(rename = "type")]
    pub kind: String,
    pub id: String,
    pub repository_root: PathBuf,
    #[serde(
        default,
        deserialize_with = "crate::selection::deserialize_files",
        skip_serializing_if = "Option::is_none"
    )]
    pub files: Option<Vec<String>>,
    pub options: Options,
}

/// 已接收完整请求后使用的规范化源码及安装边界，不进入线格式。
#[derive(Debug)]
pub struct CheckedRequest {
    pub request: Request,
    pub repository: PathBuf,
    pub installation: PathBuf,
}

/// 失败只携带稳定类别和说明，完整响应不允许附带图。
#[derive(Clone, Debug, Serialize)]
pub struct Failure {
    pub code: &'static str,
    pub message: String,
}
impl Failure {
    /// 保留错误分类，避免用空图掩盖输入、读取或分析故障。
    pub fn new(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }
}

/// EOF 后严格解码单个 JSON，serde 的默认深度上限同时约束递归对象。
pub fn decode(bytes: &[u8]) -> Result<Request, Failure> {
    let invalid = || Failure::new("invalid_request", "请求 JSON、字段或路径无效");
    if bytes.len() > MAX_BYTES {
        return Err(Failure::new("resource_limit", "请求超过字节上限"));
    }
    let UniqueValue(value) = serde_json::from_slice(bytes).map_err(|_| invalid())?;
    if value.get("protocol").and_then(Value::as_str) != Some(PROTOCOL) {
        return Err(Failure::new(
            "incompatible_protocol",
            "不支持的 CodeGraph 协议版本",
        ));
    }
    let request: Request = serde_json::from_value(value).map_err(|_| invalid())?;
    if request.kind != "analyze"
        || request.id.trim().is_empty()
        || request.id.len() > 1024
        || request.id.contains('\0')
        || !request.repository_root.is_absolute()
        || request
            .repository_root
            .to_str()
            .is_none_or(|s| s.contains('\0'))
    {
        return Err(invalid());
    }
    let mut seen = BTreeSet::new();
    for path in request.files.iter().flatten() {
        relative_path(path).map_err(|_| invalid())?;
        if !seen.insert(path) {
            return Err(invalid());
        }
    }
    Ok(request)
}

/// 所有系统使用相同的 POSIX 相对路径语法，拒绝盘符、空段与穿越。
pub fn relative_path(path: &str) -> anyhow::Result<()> {
    anyhow::ensure!(
        !path.contains(['\\', ':', '\0'])
            && !path
                .split('/')
                .any(|p| p.is_empty() || p == "." || p == ".."),
        "无效仓库相对路径"
    );
    Ok(())
}

/// 选集和配置共用真实文件边界，链接不能将读取引到仓库外。
pub fn repository_file(repository: &Path, relative: &str) -> anyhow::Result<PathBuf> {
    relative_path(relative)?;
    let actual = dunce::canonicalize(repository.join(relative))?;
    anyhow::ensure!(
        actual.starts_with(repository) && actual.is_file(),
        "文件不属于仓库内普通文件"
    );
    std::fs::File::open(&actual)?;
    Ok(actual)
}

/// 包中 bin 的父目录是安装根；本地构建程序以自身所在目录为根。
pub fn installation_root(executable: &Path) -> anyhow::Result<PathBuf> {
    let executable = dunce::canonicalize(executable)?;
    let parent = executable
        .parent()
        .ok_or_else(|| anyhow::anyhow!("分析器没有安装目录"))?;
    Ok(if parent.file_name().is_some_and(|name| name == "bin") {
        parent
            .parent()
            .ok_or_else(|| anyhow::anyhow!("分析器安装目录无效"))?
    } else {
        parent
    }
    .to_owned())
}

/// 即使是显式空集也检查源码根与安装不重叠，随后才允许免 SDK 返回。
pub fn check(request: Request, executable: &Path) -> Result<CheckedRequest, Failure> {
    let installation = installation_root(executable)
        .map_err(|_| Failure::new("invalid_pack", "分析器安装目录不可读取"))?;
    let repository = dunce::canonicalize(&request.repository_root)
        .map_err(|_| Failure::new("invalid_input", "仓库目录不可读取"))?;
    if !repository.is_dir()
        || repository.starts_with(&installation)
        || installation.starts_with(&repository)
    {
        return Err(Failure::new(
            "invalid_input",
            "仓库和分析器安装必须是独立目录",
        ));
    }
    for path in request.files.iter().flatten() {
        repository_file(&repository, path)
            .map_err(|_| Failure::new("invalid_input", "选集路径不可读取或越出仓库"))?;
    }
    Ok(CheckedRequest {
        request,
        repository,
        installation,
    })
}

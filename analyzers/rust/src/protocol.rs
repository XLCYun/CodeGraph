//! 一次一侧的严格 NDJSON 请求；runtime 政策只来自固定 profile 和批准的平台。
use anyhow::{Result, ensure};
use serde::Deserialize;
use std::{collections::BTreeSet, path::PathBuf};

pub use codegraph_engine_common::PROTOCOL;
/// Host 发来的唯一分析请求，不接受命令或任意环境对象。
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    pub protocol: String,
    #[serde(rename = "type")]
    pub message_type: String,
    pub id: String,
    pub input: Input,
    pub environment: Environment,
    pub limits: Limits,
}
/// 当前固定侧的选集和语言政策。
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Input {
    pub repository_root: PathBuf,
    #[serde(
        default,
        deserialize_with = "codegraph_engine_common::selection::deserialize_files"
    )]
    pub files: Option<Vec<String>>,
    pub include_tests: bool,
    pub profile: String,
}
/// Host 授权的 scratch 和声明平台。
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Environment {
    pub scratch_root: PathBuf,
    pub platforms: Vec<Platform>,
}
/// 显式 Rust 标准库路径；不推导不受信任命令。
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Platform {
    pub id: String,
    pub kind: String,
    pub version: String,
    pub root: PathBuf,
    pub metadata: RustMetadata,
}
/// Rust 平台唯一允许的配置字段是 sysroot 内的源码相对路径。
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RustMetadata {
    pub rust_src: String,
}
/// Host 负责整个进程树的期限与诊断上限，helper 在发送前检查结果上限。
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Limits {
    pub analysis_timeout_ms: u64,
    pub max_result_bytes: usize,
    pub max_diagnostic_bytes: usize,
}
impl Request {
    /// 独立启动 helper 也必须拒绝不规范路径、重复选集、未知 profile 与多平台歧义。
    pub fn validate(&self) -> Result<()> {
        ensure!(
            self.protocol == PROTOCOL && self.message_type == "analyze" && !self.id.is_empty(),
            "无效分析请求"
        );
        ensure!(
            self.input.repository_root.is_absolute() && self.environment.scratch_root.is_absolute(),
            "根路径必须为绝对路径"
        );
        ensure!(
            matches!(
                self.input.profile.as_str(),
                "rust-repository-v1" | "rust-standard-library-v1" | "rust-all-dependencies-v1"
            ),
            "未知 Rust profile"
        );
        ensure!(
            self.environment.platforms.len()
                == usize::from(self.input.profile != "rust-repository-v1"),
            "Rust profile 的平台数量不匹配"
        );
        ensure!(
            self.limits.analysis_timeout_ms > 0
                && self.limits.max_result_bytes > 0
                && self.limits.max_diagnostic_bytes > 0,
            "无效协议限额"
        );
        ensure!(
            self.input
                .files
                .as_ref()
                .is_none_or(|files| files.len() <= codegraph_engine_common::selection::MAX_FILES),
            "文件选集超过数量上限"
        );
        for platform in &self.environment.platforms {
            ensure!(
                !platform.id.is_empty()
                    && !platform.version.is_empty()
                    && platform.kind == "rust-standard-library"
                    && platform.root.is_absolute(),
                "无效 Rust 平台"
            );
            let source = &platform.metadata.rust_src;
            ensure!(
                !source.contains(['\\', ':', '\0'])
                    && !source
                        .split('/')
                        .any(|part| part.is_empty() || part == "." || part == ".."),
                "rust_src 必须为平台内相对路径"
            );
        }
        let mut files = BTreeSet::new();
        for path in self.input.files.iter().flatten() {
            relative_path(path)?;
            ensure!(files.insert(path), "选集含重复相对路径");
        }
        Ok(())
    }
}

/// 显式与发现文件都遵循跨平台相对路径语法，不能让本机路径规则接受歧义。
pub(crate) fn relative_path(path: &str) -> Result<()> {
    ensure!(
        !path.contains(['\\', ':', '\0'])
            && !path
                .split('/')
                .any(|part| part.is_empty() || part == "." || part == ".."),
        "选集含无效相对路径"
    );
    Ok(())
}

pub use codegraph_engine_common::one_shot::validate_cancel;

#[cfg(test)]
#[path = "tests/protocol.rs"]
mod tests;

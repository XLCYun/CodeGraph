//! 完整分析只接收原生选集、依赖范围和真实平台声明，不再重建协议信封。
use crate::DependencyScope;
use anyhow::{Result, ensure};
use codegraph_engine_common::observation::Observation;
use std::{collections::BTreeSet, path::PathBuf};

/// 同进程完整投影的输入；公共 JSON、进程期限与输出限额由公共入口负责。
pub(crate) struct CompleteAnalysisInput {
    pub repository_root: PathBuf,
    pub files: Option<Vec<String>>,
    pub include_tests: bool,
    pub dependency_scope: DependencyScope,
    pub platforms: Vec<RustPlatform>,
    pub observation: Option<Observation>,
}

/// 已声明的 Rust 标准库平台；具体工具链和源码身份仍由发现阶段核对。
pub(crate) struct RustPlatform {
    pub id: String,
    pub version: String,
    pub root: PathBuf,
    pub rust_src: String,
}

impl CompleteAnalysisInput {
    /// 保留原生选集和平台的实际约束；依赖范围与平台种类由类型表达。
    pub(super) fn validate(&self) -> Result<()> {
        ensure!(self.repository_root.is_absolute(), "根路径必须为绝对路径");
        ensure!(
            self.platforms.len()
                <= usize::from(self.dependency_scope != DependencyScope::RepositoryOnly),
            "Rust profile 的平台数量不匹配"
        );
        ensure!(
            self.files
                .as_ref()
                .is_none_or(|files| files.len() <= codegraph_engine_common::selection::MAX_FILES),
            "文件选集超过数量上限"
        );
        for platform in &self.platforms {
            ensure!(
                !platform.id.is_empty()
                    && !platform.version.is_empty()
                    && platform.root.is_absolute(),
                "无效 Rust 平台"
            );
            let source = &platform.rust_src;
            ensure!(
                !source.contains(['\\', ':', '\0'])
                    && !source
                        .split('/')
                        .any(|part| part.is_empty() || part == "." || part == ".."),
                "rust_src 必须为平台内相对路径"
            );
        }
        let mut files = BTreeSet::new();
        for path in self.files.iter().flatten() {
            relative_path(path)?;
            ensure!(files.insert(path), "选集含重复相对路径");
        }
        Ok(())
    }
}

/// 显式与发现文件都遵循跨平台相对路径语法，不能让本机路径规则接受歧义。
pub(super) fn relative_path(path: &str) -> Result<()> {
    ensure!(
        !path.contains(['\\', ':', '\0'])
            && !path
                .split('/')
                .any(|part| part.is_empty() || part == "." || part == ".."),
        "选集含无效相对路径"
    );
    Ok(())
}

#[cfg(test)]
#[path = "tests/input.rs"]
mod tests;

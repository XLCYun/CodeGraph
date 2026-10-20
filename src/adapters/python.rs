//! python 的固定入口、引擎身份与运行时装配。
use super::*;

/// 编译时绑定 python，不从文件名或命令行推导语言。
#[derive(Clone, Copy)]
pub struct Python;
impl Python {
    /// 进入共享的协议与生命周期运行时。
    pub fn main(self) -> i32 {
        crate::cli::main(self)
    }
}
impl Adapter for Python {
    const ID: PackId = PackId::Python;
    const ENGINE: EngineDefinition = EngineDefinition {
        helper: "codegraph-python-engine",
        projection: "python-source-v1",
        languages: &["python"],
        profile: "python-static-v1",
        platform_kinds: &["python-embedded-stdlib"],
        requires_platforms: true,
    };

    /// 只使用本语言固定的随包入口和参数。
    fn command(self, root: &Path) -> Result<HelperCommand, AnalysisError> {
        let mut command = command(
            root,
            if cfg!(windows) {
                "bin/codegraph-python-engine.exe"
            } else {
                "bin/codegraph-python-engine"
            },
        )?;
        command.trusted_roots = vec![inside(root, "bin")?];
        Ok(command)
    }
}

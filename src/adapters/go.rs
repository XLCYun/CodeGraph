//! go 的固定入口、引擎身份与运行时装配。
use super::*;

/// 编译时绑定 go，不从文件名或命令行推导语言。
#[derive(Clone, Copy)]
pub struct Go;
impl Go {
    /// 进入共享的协议与生命周期运行时。
    pub fn main(self) -> i32 {
        crate::cli::main(self)
    }
}
impl Adapter for Go {
    const ID: PackId = PackId::Go;
    const ENGINE: EngineDefinition = EngineDefinition {
        helper: "codegraph-go-engine",
        projection: "go-source-v1",
        languages: &["go"],
        profile: "go-static-v1",
        platform_kinds: &["go-sdk"],
        requires_platforms: true,
    };

    /// 只使用本语言固定的随包入口和参数。
    fn command(self, root: &Path) -> Result<HelperCommand, AnalysisError> {
        command(
            root,
            if cfg!(windows) {
                "bin/codegraph-go-engine.exe"
            } else {
                "bin/codegraph-go-engine"
            },
        )
    }

    /// 检查声明的 SDK 种类，再发现已有目标工具链。
    fn target_platforms(self, root: &Path, kind: &str) -> Result<Vec<Platform>, AnalysisError> {
        let _ = root;
        if kind != "go" {
            return Err(failure("Go 分析器要求 go 目标 SDK"));
        }
        platforms::go()
    }
}

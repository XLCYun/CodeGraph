//! c-cpp 的固定入口、引擎身份与运行时装配。
use super::*;

/// 编译时绑定 c-cpp，不从文件名或命令行推导语言。
#[derive(Clone, Copy)]
pub struct CCpp;
impl CCpp {
    /// 进入共享的协议与生命周期运行时。
    pub fn main(self) -> i32 {
        crate::cli::main(self)
    }
}
impl Adapter for CCpp {
    const ID: PackId = PackId::CCpp;
    const ENGINE: EngineDefinition = EngineDefinition {
        helper: "codegraph-clang-engine",
        projection: "clang-source-v1",
        languages: &["c", "cpp"],
        profile: "clang-static-v1",
        platform_kinds: &["clang-resource", "clang-target", "clang-system-headers"],
        requires_platforms: true,
    };

    /// 只使用本语言固定的随包入口和参数。
    fn command(self, root: &Path) -> Result<HelperCommand, AnalysisError> {
        command(
            root,
            if cfg!(windows) {
                "bin/codegraph-clang-engine.exe"
            } else {
                "bin/codegraph-clang-engine"
            },
        )
    }

    /// 检查声明的 SDK 种类，再发现已有目标工具链。
    fn target_platforms(self, root: &Path, kind: &str) -> Result<Vec<Platform>, AnalysisError> {
        let _ = root;
        match kind {
            "macos" if cfg!(target_os = "macos") => platforms::macos(),
            "windows-msvc" if cfg!(windows) => platforms::windows(),
            _ => Err(failure("C/C++ 分析器的目标 SDK 与宿主不匹配")),
        }
    }
}

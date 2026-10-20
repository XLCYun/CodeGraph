//! csharp 的固定入口、引擎身份与运行时装配。
use super::*;

/// 编译时绑定 csharp，不从文件名或命令行推导语言。
#[derive(Clone, Copy)]
pub struct CSharp;
impl CSharp {
    /// 进入共享的协议与生命周期运行时。
    pub fn main(self) -> i32 {
        crate::cli::main(self)
    }
}
impl Adapter for CSharp {
    const ID: PackId = PackId::Csharp;
    const ENGINE: EngineDefinition = EngineDefinition {
        helper: "codegraph-csharp-engine",
        projection: "csharp-source-v1",
        languages: &["csharp"],
        profile: "csharp-static-v1",
        platform_kinds: &["dotnet-reference-assemblies"],
        requires_platforms: true,
    };

    /// 只使用本语言固定的随包入口和参数。
    fn command(self, root: &Path) -> Result<HelperCommand, AnalysisError> {
        let mut command = command(
            root,
            if cfg!(windows) {
                "bin/dotnet.exe"
            } else {
                "bin/dotnet"
            },
        )?;
        command.args =
            vec![inside(root, "runtime/helper/CodeGraph.CSharp.Engine.dll")?.into_os_string()];
        command
            .environment
            .insert("DOTNET_ROOT".into(), root.join("bin").into_os_string());
        command
            .environment
            .insert("DOTNET_MULTILEVEL_LOOKUP".into(), "0".into());
        command
            .environment
            .insert("DOTNET_EnableDiagnostics".into(), "0".into());
        Ok(command)
    }

    /// 检查声明的 SDK 种类，再发现已有目标工具链。
    fn target_platforms(self, root: &Path, kind: &str) -> Result<Vec<Platform>, AnalysisError> {
        let _ = root;
        if kind != "dotnet" {
            return Err(failure("C# 分析器要求 dotnet 目标 SDK"));
        }
        platforms::dotnet()
    }
}

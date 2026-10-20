//! java 的固定入口、引擎身份与运行时装配。
use super::*;

/// 编译时绑定 java，不从文件名或命令行推导语言。
#[derive(Clone, Copy)]
pub struct Java;
impl Java {
    /// 进入共享的协议与生命周期运行时。
    pub fn main(self) -> i32 {
        crate::cli::main(self)
    }
}
impl Adapter for Java {
    const ID: PackId = PackId::Java;
    const ENGINE: EngineDefinition = EngineDefinition {
        helper: "codegraph-java-engine",
        projection: "java-source-v1",
        languages: &["java"],
        profile: "java-static-v1",
        platform_kinds: &["java-jdk"],
        requires_platforms: true,
    };

    /// 只使用本语言固定的随包入口和参数。
    fn command(self, root: &Path) -> Result<HelperCommand, AnalysisError> {
        let mut command = command(
            root,
            if cfg!(windows) {
                "runtime/jdk/bin/java.exe"
            } else {
                "runtime/jdk/bin/java"
            },
        )?;
        let classes = inside(root, "runtime/helper/classes")?;
        let libraries = inside(root, "runtime/helper/lib")?.join("*");
        command.args = vec![
            "-cp".into(),
            std::env::join_paths([classes, libraries]).map_err(failure)?,
            "dev.codegraph.java.Main".into(),
        ];
        Ok(command)
    }

    /// 检查声明的 SDK 种类，再发现已有目标工具链。
    fn target_platforms(self, root: &Path, kind: &str) -> Result<Vec<Platform>, AnalysisError> {
        if kind != "java" {
            return Err(failure("Java 分析器要求 java 目标 SDK"));
        }
        platforms::java(&root.join("runtime/jdk"))
    }
}

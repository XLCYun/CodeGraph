//! typescript-javascript 的固定入口、引擎身份与运行时装配。
use super::*;

/// 编译时绑定 typescript-javascript，不从文件名或命令行推导语言。
#[derive(Clone, Copy)]
pub struct TypeScriptJavaScript;
impl TypeScriptJavaScript {
    /// 进入共享的协议与生命周期运行时。
    pub fn main(self) -> i32 {
        crate::cli::main(self)
    }
}
impl Adapter for TypeScriptJavaScript {
    const ID: PackId = PackId::TypeScriptJavascript;
    const ENGINE: EngineDefinition = EngineDefinition {
        helper: "codegraph-typescript-engine",
        projection: "typescript-source-v1",
        languages: &["typescript", "javascript"],
        profile: "typescript-static-v1",
        platform_kinds: &["typescript-declarations"],
        requires_platforms: true,
    };

    /// 只使用本语言固定的随包入口和参数。
    fn command(self, root: &Path) -> Result<HelperCommand, AnalysisError> {
        let mut command = command(
            root,
            if cfg!(windows) {
                "bin/node.exe"
            } else {
                "bin/node"
            },
        )?;
        command.args = vec![
            "--disable-proto=throw".into(),
            dunce::simplified(&inside(root, "runtime/helper/main.mjs")?)
                .as_os_str()
                .to_owned(),
        ];
        Ok(command)
    }
}

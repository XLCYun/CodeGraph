use serde::{Deserialize, Serialize};

/// CodeGraph 声明的独立语言能力包，TS 与 JS 共用一套工具。
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum PackId {
    Rust,
    #[serde(rename = "typescript-javascript")]
    TypeScriptJavascript,
    Python,
    Java,
    Go,
    Csharp,
    #[serde(rename = "c-cpp")]
    CCpp,
}

impl PackId {
    /// 只有已接通生产工厂的语言才能进入真实 Native 支持包验收。
    pub const NATIVE_MIGRATED: &'static [Self] = &[
        Self::Rust,
        Self::Python,
        Self::TypeScriptJavascript,
        Self::Java,
        Self::Go,
        Self::Csharp,
        Self::CCpp,
    ];

    /// 稳定标识同时用于清单和应用拥有的相对目录。
    pub fn slug(self) -> &'static str {
        match self {
            Self::Rust => "rust",
            Self::TypeScriptJavascript => "typescript-javascript",
            Self::Python => "python",
            Self::Java => "java",
            Self::Go => "go",
            Self::Csharp => "csharp",
            Self::CCpp => "c-cpp",
        }
    }

    /// 当前包的必需入口，归档必须提供这些普通文件。
    pub fn required(self, target: &str) -> Vec<&'static str> {
        let windows = target == "x86_64-pc-windows-msvc";
        match self {
            Self::Rust => vec![
                "control/native.json",
                "licenses/Rust-dependencies.json",
                if windows {
                    "bin/codegraph-rust-engine.exe"
                } else {
                    "bin/codegraph-rust-engine"
                },
            ],
            Self::Java => vec![
                "control/native.json",
                if windows {
                    "runtime/jdk/bin/java.exe"
                } else {
                    "runtime/jdk/bin/java"
                },
                "runtime/helper/classes/dev/codegraph/java/Main.class",
            ],
            Self::Go => vec![
                "control/native.json",
                if windows {
                    "bin/codegraph-go-engine.exe"
                } else {
                    "bin/codegraph-go-engine"
                },
            ],
            Self::Csharp => vec![
                "control/native.json",
                if windows {
                    "bin/dotnet.exe"
                } else {
                    "bin/dotnet"
                },
                "runtime/helper/CodeGraph.CSharp.Engine.dll",
                "runtime/helper/CodeGraph.CSharp.Engine.deps.json",
                "runtime/helper/CodeGraph.CSharp.Engine.runtimeconfig.json",
                "runtime/helper/Microsoft.CodeAnalysis.dll",
                "runtime/helper/Microsoft.CodeAnalysis.CSharp.dll",
            ],
            Self::CCpp => vec![
                "control/native.json",
                if windows {
                    "bin/codegraph-clang-engine.exe"
                } else {
                    "bin/codegraph-clang-engine"
                },
            ],
            Self::Python => vec![
                if windows {
                    "bin/codegraph-python-engine.exe"
                } else {
                    "bin/codegraph-python-engine"
                },
                "control/native.json",
                "licenses/Python-dependencies.json",
                "licenses/ruff-LICENSE",
                "licenses/typeshed-LICENSE",
            ],
            Self::TypeScriptJavascript => vec![
                if windows { "bin/node.exe" } else { "bin/node" },
                "control/native.json",
                "runtime/helper/main.mjs",
                "runtime/helper/analyze.mjs",
                "runtime/helper/projection.mjs",
                "runtime/helper/config.mjs",
                "runtime/helper/workspace.mjs",
                "runtime/helper/syntax.mjs",
                "runtime/typescript/lib/lib.d.ts",
                "runtime/typescript/lib/typescript.js",
                "runtime/@types/node/index.d.ts",
                "runtime/undici-types/index.d.ts",
            ],
        }
    }
}

/// 公共包描述只绑定版本与协议；不能声明执行命令、环境或 SDK 路径。
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Descriptor {
    schema_version: u32,
    pack_id: PackId,
    pack_version: String,
    engine_version: String,
    protocol: String,
}

/// 控制文件也是边界输入，使用有限读取并拒绝符号链接指向包外。
pub(crate) fn read_control(path: &std::path::Path) -> std::io::Result<Vec<u8>> {
    use std::io::Read;
    let mut data = Vec::new();
    std::fs::File::open(path)?
        .take(1024 * 1024 + 1)
        .read_to_end(&mut data)?;
    if data.len() > 1024 * 1024 {
        return Err(std::io::Error::other("支持包控制文件超过 1 MiB"));
    }
    Ok(data)
}

/// 固定包身份必须与编译时语言和当前引擎完全一致，不接受候选包冒充发布版本。
pub(crate) fn validate(id: PackId, root: &std::path::Path) -> Result<(), String> {
    let root = root.canonicalize().map_err(|_| "支持包目录不可读取")?;
    let path = root
        .join("control/codegraph.json")
        .canonicalize()
        .map_err(|_| "缺少 control/codegraph.json")?;
    if !path.starts_with(&root) {
        return Err("公共包描述越界".into());
    }
    let bytes = read_control(&path).map_err(|_| "公共包描述不可读取或过大")?;
    let crate::strict_json::UniqueValue(value) =
        serde_json::from_slice(&bytes).map_err(|_| "公共包描述 JSON 无效或含重复字段")?;
    let descriptor: Descriptor = serde_json::from_value(value).map_err(|_| "公共包描述字段无效")?;
    if descriptor.schema_version != 1
        || descriptor.pack_id != id
        || descriptor.pack_version != env!("CARGO_PKG_VERSION")
        || descriptor.engine_version != env!("CARGO_PKG_VERSION")
        || descriptor.protocol != crate::cli::protocol::PROTOCOL
    {
        return Err("支持包 ID、版本或协议与 CodeGraph 不匹配".into());
    }
    Ok(())
}

/// 从实际可执行文件定位包根，不使用 cwd、argv[0] 或消费者提供的目录。
pub(crate) fn executable_root() -> std::io::Result<std::path::PathBuf> {
    root_from_executable(&std::env::current_exe()?)
}

/// 规范化后的入口必须直接位于包内 bin 目录；目录别名不能改变包身份。
fn root_from_executable(executable: &std::path::Path) -> std::io::Result<std::path::PathBuf> {
    let executable = executable.canonicalize()?;
    let bin = executable
        .parent()
        .ok_or_else(|| std::io::Error::other("分析器缺少 bin 目录"))?;
    if !executable.is_file() || bin.file_name() != Some(std::ffi::OsStr::new("bin")) {
        return Err(std::io::Error::other("分析器必须位于包的 bin 目录"));
    }
    let root = bin
        .parent()
        .ok_or_else(|| std::io::Error::other("分析器缺少包根目录"))?
        .canonicalize()?;
    if !root.is_dir() || !root.is_absolute() || root.to_str().is_none() {
        return Err(std::io::Error::other("分析器包根必须是绝对 UTF-8 目录"));
    }
    Ok(root)
}

#[cfg(test)]
#[path = "pack/tests/location.rs"]
mod tests;

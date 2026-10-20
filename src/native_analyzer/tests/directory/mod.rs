//! 四种真实原生引擎通过共享 Host 验证目录选集、来源和公共图。
use crate::{
    code_graph::{AnalysisInput, CodeGraph},
    native_analyzer::{HelperCommand, HelperIdentity, NativeLanguageAnalyzer, Platform},
};
use serde_json::Value;
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::Path,
};

mod clang;
mod csharp;
mod go;
mod java;

/// 只读取验收脚本生成的真实工具配置，不从待分析项目获得运行命令。
fn analyzer(language: &str) -> NativeLanguageAnalyzer {
    let config = std::env::var_os("CODEGRAPH_NATIVE_HELPERS_CONFIG")
        .expect("必须提供真实原生 helper 验收配置");
    let config: Value = serde_json::from_slice(&fs::read(config).unwrap()).unwrap();
    let config = &config[language];
    let (projection, profile, languages) = match language {
        "go" => ("go-source-v1", "go-static-v1", vec!["go"]),
        "java" => ("java-source-v1", "java-static-v1", vec!["java"]),
        "csharp" => ("csharp-source-v1", "csharp-static-v1", vec!["csharp"]),
        "clang" => ("clang-source-v1", "clang-static-v1", vec!["c", "cpp"]),
        _ => panic!("未注册的验收语言"),
    };
    let mut environment = BTreeMap::new();
    for (key, value) in config["environment"].as_object().unwrap() {
        environment.insert(key.into(), value.as_str().unwrap().into());
    }
    #[cfg(windows)]
    for key in ["SystemRoot", "WINDIR", "SystemDrive"] {
        if let Some(value) = std::env::var_os(key) {
            environment.insert(key.into(), value);
        }
    }
    NativeLanguageAnalyzer::new(
        HelperCommand {
            executable: config["executable"].as_str().unwrap().into(),
            args: config["args"]
                .as_array()
                .unwrap()
                .iter()
                .map(|v| v.as_str().unwrap().into())
                .collect(),
            environment,
            trusted_roots: config["trusted_roots"]
                .as_array()
                .unwrap()
                .iter()
                .map(|v| v.as_str().unwrap().into())
                .collect(),
        },
        HelperIdentity {
            helper_id: format!("codegraph-{language}-engine"),
            helper_version: "0.1.0".into(),
            projection_revision: projection.into(),
            profile: profile.into(),
            languages: languages.into_iter().map(str::to_owned).collect(),
        },
        config["platforms"]
            .as_array()
            .unwrap()
            .iter()
            .map(|value| Platform {
                id: value["id"].as_str().unwrap().into(),
                kind: value["kind"].as_str().unwrap().into(),
                version: value["version"].as_str().unwrap().into(),
                root: value["root"].as_str().unwrap().into(),
                metadata: value["metadata"].clone(),
            })
            .collect(),
    )
}

/// 特殊字符目录覆盖 Windows 规范路径及 UTF-8 管道，不修改产品仓库。
fn repository(sources: &[(&str, &str)]) -> tempfile::TempDir {
    let directory = tempfile::Builder::new()
        .prefix("directory # % 中文 ")
        .tempdir()
        .unwrap();
    for (path, text) in sources {
        let file = directory.path().join(path);
        fs::create_dir_all(file.parent().unwrap()).unwrap();
        fs::write(file, text).unwrap();
    }
    directory
}

/// 省略文件列表代表由真实加载器建立选集。
fn input(root: &Path) -> AnalysisInput {
    AnalysisInput {
        repository_root: root.canonicalize().unwrap(),
        files: None,
        options: Default::default(),
    }
}

/// 沿公共包含边收集名称，不依赖任何后端私有节点身份编码。
fn names(graph: &CodeGraph) -> BTreeSet<String> {
    let mut names = BTreeSet::new();
    let mut pending = graph.root().contains.clone();
    while let Some(id) = pending.pop() {
        let node = graph.node(&id).unwrap().data();
        if let Some(name) = &node.name {
            names.insert(name.clone());
        }
        pending.extend(node.contains.iter().cloned());
    }
    names
}

/// 分析后逐字校验输入，防止只读分析偷偷改写配置或源码。
fn unchanged(root: &Path, sources: &[(&str, &str)]) {
    for (path, source) in sources {
        assert_eq!(fs::read_to_string(root.join(path)).unwrap(), *source);
    }
}

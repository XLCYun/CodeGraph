//! 四种真实原生引擎通过公共入口验证目录选集、来源和公共图。
use crate::{
    code_graph::{AnalysisInput, CodeGraph},
    public::PublicAnalyzer,
};
use serde_json::Value;
use std::{collections::BTreeSet, fs, path::Path};
mod clang;
mod csharp;
mod go;
mod java;

/// 真实固定公共入口替代旧 Host 装配；不提供任何私有平台消息。
fn analyzer(language: &'static str) -> PublicAnalyzer {
    PublicAnalyzer::new(if language == "clang" {
        "c-cpp"
    } else {
        language
    })
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

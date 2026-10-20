use super::*;
use crate::code_graph::{AnalysisInput, LanguageAnalyzer, ReferenceTarget};

/// 使用真实包与标准 SDK 发现入口，覆盖生产工厂规范路径后的完整 C# 会话。
#[test]
#[ignore = "需要真实 C# 支持包和已安装 net10.0 引用程序集"]
fn factory_preserves_source_and_local_calls() {
    let pack = PathBuf::from(
        std::env::var_os("CODEGRAPH_CSHARP_PACK").expect("必须提供真实 C# 支持包目录"),
    );
    let directory = tempfile::Builder::new()
        .prefix("codegraph csharp factory ")
        .tempdir()
        .unwrap();
    let source = "public static class Sample { public static int target() { return 1; } public static int caller() { return target(); } }\n";
    let project = "<Project Sdk=\"Microsoft.NET.Sdk\"><PropertyGroup><TargetFramework>net10.0</TargetFramework></PropertyGroup></Project>\n";
    fs::write(directory.path().join("Sample.cs"), source).unwrap();
    fs::write(directory.path().join("Sample.csproj"), project).unwrap();
    let analyzer = CSharp
        .engine(&pack)
        .unwrap_or_else(|error| panic!("C# factory preparation failed: {error}"));
    let graph = analyzer
        .analyze(&AnalysisInput {
            repository_root: directory.path().canonicalize().unwrap(),
            files: Some(vec!["Sample.cs".into()]),
            options: Default::default(),
        })
        // 内部验收保留宿主诊断；公共 CLI 仍只输出脱敏的 analysis_failed。
        .unwrap_or_else(|error| panic!("C# factory analysis failed: {error}"));
    let mut nodes = Vec::new();
    let mut pending = graph.root().contains.clone();
    while let Some(id) = pending.pop() {
        let node = graph.node(&id).unwrap();
        pending.extend(node.data().contains.iter().cloned());
        nodes.push(node.data());
    }
    let target = nodes
        .iter()
        .find(|node| node.name.as_deref() == Some("target"))
        .expect("真实目标方法必须进入图");
    let caller = nodes
        .iter()
        .find(|node| node.name.as_deref() == Some("caller"))
        .expect("真实调用方法必须进入图");
    assert!(caller.source.as_ref().unwrap().contains("target()"));
    assert!(caller.references.iter().any(|reference| {
        reference.semantic == "code.calls"
            && reference.target == ReferenceTarget::Node(target.id.clone())
    }));
    assert_eq!(
        fs::read_to_string(directory.path().join("Sample.cs")).unwrap(),
        source
    );
    assert_eq!(
        fs::read_to_string(directory.path().join("Sample.csproj")).unwrap(),
        project
    );
    assert_eq!(fs::read_dir(directory.path()).unwrap().count(), 2);
}

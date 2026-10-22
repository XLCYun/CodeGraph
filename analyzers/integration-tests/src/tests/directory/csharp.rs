use super::*;
use crate::code_graph::LanguageAnalyzer;

/// 真实 Roslyn 的 Compile 成员及测试项目角色通过公共入口来源校验。
#[test]
#[ignore = "需要真实 Roslyn helper 与固定目标引用，串行运行"]
fn compile_discovery_preserves_exact_selection_and_test_roles() {
    let sources = [
        ("App/App.csproj", "<Project Sdk=\"Microsoft.NET.Sdk\"><PropertyGroup><TargetFramework>net10.0</TargetFramework></PropertyGroup><ItemGroup><Compile Remove=\"Drop.cs\"/></ItemGroup></Project>"),
        ("App/Main.cs", "class Main { int Call()=>Helper.Value(); }"),
        ("App/Helper.cs", "class Helper { public static int Value()=>1; }"),
        ("App/Drop.cs", "invalid excluded source"),
        ("Checks/Checks.csproj", "<Project Sdk=\"Microsoft.NET.Sdk\"><PropertyGroup><TargetFramework>net10.0</TargetFramework><IsTestProject>true</IsTestProject></PropertyGroup></Project>"),
        ("Checks/Checks.cs", "class Checks {}"),
        ("other/Stray.cs", "invalid excluded source"),
    ];
    let repository = repository(&sources);
    let analyzer = analyzer("csharp");
    let mut input = input(repository.path());
    let graph = analyzer.analyze(&input).unwrap();
    assert!(names(&graph).contains("Main") && names(&graph).contains("Helper"));
    assert!(!names(&graph).contains("Checks"));
    input.options.include_tests = true;
    assert!(names(&analyzer.analyze(&input).unwrap()).contains("Checks"));
    input.files = Some(vec!["App/Main.cs".into()]);
    let graph = analyzer.analyze(&input).unwrap();
    assert!(!names(&graph).contains("Helper"));
    assert!(!graph.external_symbols().is_empty());
    input.files = Some(vec![]);
    assert!(analyzer.analyze(&input).unwrap().root().contains.is_empty());
    unchanged(repository.path(), &sources);
    let empty = super::repository(&[("Empty.csproj", "<Project Sdk=\"Microsoft.NET.Sdk\"><PropertyGroup><TargetFramework>net10.0</TargetFramework></PropertyGroup></Project>")]);
    assert!(analyzer
        .analyze(&super::input(empty.path()))
        .unwrap()
        .root()
        .contains
        .is_empty());
}

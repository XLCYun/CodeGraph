use super::*;
use crate::code_graph::LanguageAnalyzer;

/// 真实 Go loader 发现工作区成员，Host 接受完整证据并保持 replacement 和测试边界。
#[test]
#[ignore = "需要真实 Go helper 与固定目标 SDK，串行运行"]
fn workspace_discovery_preserves_exact_selection_and_test_roles() {
    let sources = [
        ("go.work", "go 1.23.8\nuse (\n ./app\n ./lib\n)\n"),
        (
            "app/go.mod",
            "module fixture/app\ngo 1.23.8\nreplace fixture/context => ../context\n",
        ),
        ("lib/go.mod", "module fixture/lib\ngo 1.23.8\n"),
        ("context/go.mod", "module fixture/context\ngo 1.23.8\n"),
        (
            "app/main.go",
            "package app\nimport \"fixture/context\"\nfunc Main(){context.Hidden()}\n",
        ),
        (
            "app/main_test.go",
            "package app\nfunc TestMember(){Main()}\n",
        ),
        (
            "app/future.go",
            "//go:build go1.999\n\npackage app\nfunc Future(){}\n",
        ),
        ("lib/lib.go", "package lib\nfunc Library(){}\n"),
        ("context/hidden.go", "package context\nfunc Hidden(){}\n"),
        ("app/testdata/ignored.go", "invalid excluded source"),
    ];
    let repository = repository(&sources);
    let analyzer = analyzer("go");
    let mut input = input(repository.path());
    let graph = analyzer.analyze(&input).unwrap();
    let observed = names(&graph);
    assert!(observed.contains("Main") && observed.contains("Library"));
    assert!(
        !observed.contains("Hidden")
            && !observed.contains("TestMember")
            && !observed.contains("Future")
    );
    assert!(!graph.external_symbols().is_empty());
    input.options.include_tests = true;
    assert!(names(&analyzer.analyze(&input).unwrap()).contains("TestMember"));
    input.files = Some(vec!["app/main.go".into()]);
    assert!(!names(&analyzer.analyze(&input).unwrap()).contains("Library"));
    input.files = Some(vec![]);
    assert!(analyzer.analyze(&input).unwrap().root().contains.is_empty());
    unchanged(repository.path(), &sources);
    let empty = super::repository(&[("go.mod", "module fixture/empty\ngo 1.23.8\n")]);
    assert!(analyzer
        .analyze(&super::input(empty.path()))
        .unwrap()
        .root()
        .contains
        .is_empty());
}

use super::*;
use crate::code_graph::LanguageAnalyzer;

/// 真实 JDT 发现声明根，生产测试后缀、明确测试根与图外源码上下文不会混淆。
#[test]
#[ignore = "需要真实 JDT helper 与固定目标 JDK，串行运行"]
fn source_root_discovery_preserves_exact_selection_and_test_roles() {
    let sources = [
        (
            "narrant-java.json",
            r#"{"schema":"java-static-v1","release":"17","source_roots":[{"path":"src","role":"main"},{"path":"checks","role":"test"}]}"#,
        ),
        (
            "src/Main.java",
            "class Main { int call(){return Helper.value();} }",
        ),
        (
            "src/Helper.java",
            "class Helper { static int value(){return 1;} }",
        ),
        ("src/RealTest.java", "class RealTest {}"),
        ("checks/Checks.java", "class Checks {}"),
        ("other/Stray.java", "invalid excluded source"),
    ];
    let repository = repository(&sources);
    let analyzer = analyzer("java");
    let mut input = input(repository.path());
    let graph = analyzer.analyze(&input).unwrap();
    let observed = names(&graph);
    assert!(
        observed.contains("Main") && observed.contains("Helper") && observed.contains("RealTest")
    );
    assert!(!observed.contains("Checks"));
    input.options.include_tests = true;
    assert!(names(&analyzer.analyze(&input).unwrap()).contains("Checks"));
    input.files = Some(vec!["src/Main.java".into()]);
    let graph = analyzer.analyze(&input).unwrap();
    assert!(!names(&graph).contains("Helper"));
    assert!(!graph.external_symbols().is_empty());
    input.files = Some(vec![]);
    assert!(analyzer.analyze(&input).unwrap().root().contains.is_empty());
    unchanged(repository.path(), &sources);
    let empty = super::repository(&[("pom.xml", "<project><groupId>fixture</groupId><artifactId>empty</artifactId><version>1</version><properties><maven.compiler.release>17</maven.compiler.release></properties></project>")]);
    assert!(analyzer
        .analyze(&super::input(empty.path()))
        .unwrap()
        .root()
        .contains
        .is_empty());
}

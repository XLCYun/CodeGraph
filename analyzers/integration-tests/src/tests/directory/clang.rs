use super::*;
use crate::code_graph::LanguageAnalyzer;
use serde_json::json;

/// 公共 Clang 无 CDB 仍覆盖源码，显式特殊后缀与可用上下文保留各自职责。
#[test]
#[ignore = "需要真实 LibTooling helper 与批准目标头，串行运行"]
fn participation_discovery_preserves_headers_and_exact_selection() {
    let sources = [
        ("unit.cpp", "#include \"empty.h\"\n#include \"api.hpp\"\n#if __has_include(\"probe.h\")\n#endif\nint caller(){return target();}\n"),
        ("empty.h", ""),
        ("api.hpp", "inline int target(){return 1;}\n"),
        ("probe.h", "invalid probe-only source"),
        ("unusual.custom", "int explicit_source(){return 2;}\n"),
    ];
    let repository = repository(&sources);
    let analyzer = analyzer("clang");
    let mut input = input(repository.path());
    let graph = analyzer.analyze(&input).unwrap();
    let discovered = names(&graph);
    assert!(discovered.contains("caller") && discovered.contains("target"));
    assert!(discovered.contains("probe.h") && discovered.contains("empty.h"));
    assert!(!discovered.contains("explicit_source"));
    let cdb = json!([{"directory": repository.path(), "file":"unit.cpp", "arguments":["clang", "-x", "c++", "-std=c++20", "-c", "unit.cpp"]}]);
    fs::write(
        repository.path().join("compile_commands.json"),
        serde_json::to_vec(&cdb).unwrap(),
    )
    .unwrap();
    input.files = Some(vec!["unit.cpp".into()]);
    let graph = analyzer.analyze(&input).unwrap();
    assert!(!names(&graph).contains("target"));
    assert!(!graph.external_symbols().is_empty());
    input.files = Some(vec!["unusual.custom".into()]);
    assert!(names(&analyzer.analyze(&input).unwrap()).contains("explicit_source"));
    input.files = Some(vec![]);
    assert!(analyzer.analyze(&input).unwrap().root().contains.is_empty());
    unchanged(repository.path(), &sources);
    assert_eq!(
        serde_json::from_slice::<Value>(
            &fs::read(repository.path().join("compile_commands.json")).unwrap()
        )
        .unwrap(),
        cdb
    );
}

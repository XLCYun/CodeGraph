use super::*;
use crate::code_graph::LanguageAnalyzer;
use serde_json::json;

/// 真实 Clang 将 TU 与实际头文件交给 Host；无后缀源码不会被共享扩展名规则丢弃。
#[test]
#[ignore = "需要真实 LibTooling helper 与批准目标头，串行运行"]
fn participation_discovery_preserves_headers_and_exact_selection() {
    let sources = [
        ("unit.custom", "#include \"empty\"\n#include \"api.inc\"\n#if __has_include(\"probe.h\")\n#endif\nint caller(){return target();}\n"),
        ("empty", ""),
        ("api.inc", "inline int target(){return 1;}\n"),
        ("probe.h", "invalid probe-only source"),
    ];
    let repository = repository(&sources);
    let cdb = json!([{"directory": repository.path(), "file":"unit.custom", "arguments":["clang", "-x", "c++", "-std=c++20", "-c", "unit.custom"]}]);
    fs::write(
        repository.path().join("compile_commands.json"),
        serde_json::to_vec(&cdb).unwrap(),
    )
    .unwrap();
    let analyzer = analyzer("clang");
    let mut input = input(repository.path());
    let graph = analyzer.analyze(&input).unwrap();
    assert!(names(&graph).contains("caller") && names(&graph).contains("target"));
    input.files = Some(vec!["unit.custom".into()]);
    let graph = analyzer.analyze(&input).unwrap();
    assert!(!names(&graph).contains("target"));
    assert!(!graph.external_symbols().is_empty());
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

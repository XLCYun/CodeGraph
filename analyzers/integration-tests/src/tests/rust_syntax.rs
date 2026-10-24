use crate::code_graph::{AnalysisInput, CodeGraph, LanguageAnalyzer, ReferenceTarget};

/// 真实 JSON 项目通过单侧 helper 与公共入口，保持旧语法断言的顶层对象范围。
fn graph(source: &str) -> CodeGraph {
    try_graph(source).unwrap()
}

/// 有效声明与语法失败均走相同原生加载路径，不使用宿主 parser。
fn try_graph(source: &str) -> Result<CodeGraph, crate::code_graph::AnalysisError> {
    let root = tempfile::tempdir().unwrap();
    std::fs::write(root.path().join("lib.rs"), source).unwrap();
    std::fs::write(
        root.path().join("rust-project.json"),
        r#"{"crates":[{"root_module":"lib.rs","edition":"2024","deps":[],"cfg":[]}]}"#,
    )
    .unwrap();
    super::rust_graph::analyzer().analyze(&AnalysisInput {
        repository_root: root.path().into(),
        files: Some(vec!["lib.rs".into()]),
        options: Default::default(),
    })
}

/// 同行多个声明与多字节标识符按字节范围准确截取，不拼接签名、等号或实现。
#[test]
#[ignore = "需要真实 Rust 优化 helper"]
fn source_ranges_keep_complete_declarations_without_neighboring_items() {
    let source = "fn 甲() {} const 值: u8 = 7; struct S { pub 字段: u8, other: u8 }";
    let graph = graph(source);
    let nodes: Vec<_> = graph
        .root()
        .contains
        .iter()
        .map(|id| graph.node(id).unwrap())
        .collect();
    assert_eq!(
        nodes
            .iter()
            .find(|node| node.data().name.as_deref() == Some("甲"))
            .unwrap()
            .data()
            .source
            .as_deref(),
        Some("fn 甲() {}")
    );
    assert_eq!(
        nodes
            .iter()
            .find(|node| node.data().name.as_deref() == Some("值"))
            .unwrap()
            .data()
            .source
            .as_deref(),
        Some("const 值: u8 = 7;")
    );
    let structure = nodes
        .iter()
        .find(|node| node.data().name.as_deref() == Some("S"))
        .unwrap();
    let field = structure
        .data()
        .contains
        .iter()
        .map(|id| graph.node(id).unwrap())
        .find(|node| node.data().name.as_deref() == Some("字段"))
        .unwrap();
    assert_eq!(field.data().source.as_deref(), Some("pub 字段: u8"));
}

/// 真实闭包关系仍归属最近的具名声明，未建模的闭包不拥有节点。
#[test]
#[ignore = "需要真实 Rust 优化 helper"]
fn closure_references_belong_to_the_nearest_modeled_declaration() {
    let graph = graph("fn run() { let f = || { fn helper() { inside(); } outside(); }; }");
    let nodes = super::rust_graph::nodes(&graph);
    for (name, owner) in [("inside", "helper"), ("outside", "run")] {
        let node = nodes
            .values()
            .find(|node| node.data().name.as_deref() == Some(owner))
            .unwrap();
        assert!(node.data().references.iter().any(|reference| matches!(&reference.target,ReferenceTarget::Unresolved { name: actual } if actual==name)));
    }
}

/// 未启用分支也完整列出显式声明，而源代码语法错误不能返回部分图。
#[test]
#[ignore = "需要真实 Rust 优化 helper"]
fn preserves_cfg_declarations_and_rejects_parse_failures() {
    assert_eq!(
        graph("#[cfg(any())] fn inactive() {} fn active() {}")
            .root()
            .contains
            .len(),
        2
    );
    assert!(try_graph("fn broken( {").is_err());
}

/// 注释中的 CR、LS、PS 不截断后续声明，公共入口仍验证真实 UTF-8 源码锚点。
#[test]
#[ignore = "需要真实 Rust 优化 helper"]
fn source_line_breaks_keep_complete_declarations() {
    let source = "/* 中😀\r\u{2028}\u{2029} */ fn after() {}\r\nfn next() {}";
    let graph = graph(source);
    let nodes = super::rust_graph::nodes(&graph);
    for (name, excerpt) in [("after", "fn after() {}"), ("next", "fn next() {}")] {
        let declaration = nodes
            .values()
            .find(|node| node.data().name.as_deref() == Some(name))
            .unwrap();
        assert!(declaration
            .data()
            .source
            .as_deref()
            .unwrap()
            .ends_with(excerpt));
    }
}

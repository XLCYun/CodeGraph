use super::*;
use crate::model::{CodeNode, CodeNodeData, LanguageId};

/// 对外图必须有根可达的唯一节点，不能忽略孤立环或漏掉引用描述。
#[test]
fn graph_invariants_reject_orphans_duplicates_and_missing_targets() {
    let node = CodeNode::Callable(CodeNodeData {
        id: NodeId("rust:run".into()),
        language: LanguageId("rust".into()),
        native_kind: "function".into(),
        name: Some("run".into()),
        signature: None,
        code: None,
        source: None,
        contains: vec![],
        references: vec![],
        extensions: Default::default(),
    });
    let mut graph = CodeGraph {
        root_contains: vec![NodeId("rust:run".into())],
        nodes: vec![node.clone()],
        external_symbols: vec![],
    };
    validate(&graph, "rust").unwrap();
    graph.nodes.push(node);
    assert!(validate(&graph, "rust").is_err());
    graph.nodes.pop();
    graph.root_contains.clear();
    assert!(validate(&graph, "rust").is_err());
    graph.root_contains.push(NodeId("rust:missing".into()));
    assert!(validate(&graph, "rust").is_err());
}

/// 已解析图外符号必须有且只有一份描述，正常 unresolved 则无需描述。
#[test]
fn external_descriptors_and_unresolved_targets_preserve_public_semantics() {
    use crate::model::{CodeReference, ExternalSymbol, SymbolScope};
    let mut graph = CodeGraph {
        root_contains: vec![NodeId("rust:run".into())],
        nodes: vec![CodeNode::Callable(CodeNodeData {
            id: NodeId("rust:run".into()),
            language: LanguageId("rust".into()),
            native_kind: "function".into(),
            name: None,
            signature: None,
            code: None,
            source: None,
            contains: vec![],
            references: vec![
                CodeReference {
                    semantic: "code.calls".into(),
                    target: ReferenceTarget::External {
                        symbol: "rust:other".into(),
                    },
                },
                CodeReference {
                    semantic: "code.calls".into(),
                    target: ReferenceTarget::Unresolved {
                        name: "missing".into(),
                    },
                },
            ],
            extensions: Default::default(),
        })],
        external_symbols: vec![],
    };
    assert!(validate(&graph, "rust").is_err());
    graph.external_symbols.push(ExternalSymbol {
        symbol: "rust:other".into(),
        name: "other".into(),
        scope: SymbolScope::Repository,
    });
    validate(&graph, "rust").unwrap();
    graph.external_symbols.push(ExternalSymbol {
        symbol: "rust:unused".into(),
        name: "unused".into(),
        scope: SymbolScope::Dependency,
    });
    assert!(validate(&graph, "rust").is_err());
}

/// 内部覆盖不能省略显式文件、伪造配置或让被测试开关排除的文件支持节点。
#[test]
fn selection_coverage_and_origins_remain_required_before_output() {
    use crate::public_protocol::{check, decode};
    let repository = tempfile::tempdir().unwrap();
    let package = tempfile::tempdir().unwrap();
    std::fs::write(repository.path().join("main.rs"), "").unwrap();
    std::fs::create_dir(package.path().join("bin")).unwrap();
    let executable = package.path().join("bin/codegraph-rust");
    std::fs::write(&executable, "fixture").unwrap();
    let request = serde_json::json!({"protocol":"codegraph.analyzer/1","type":"analyze","id":"coverage","repository_root":repository.path(),"files":["main.rs"],"options":{"include_tests":false,"rust_dependency_scope":"repository-only"}});
    let input = check(decode(request.to_string().as_bytes()).unwrap(), &executable).unwrap();
    assert!(
        finish(
            Projection {
                graph: CodeGraph::empty(),
                evidence: Evidence::default()
            },
            &input,
            "rust",
            &[]
        )
        .is_err()
    );
    let make = |disposition, configurations: Vec<String>| Projection {
        graph: CodeGraph::empty(),
        evidence: Evidence {
            configurations: vec![Configuration {
                id: "project".into(),
                source_paths: vec![],
                platform_ids: vec![],
            }],
            files: vec![FileEvidence {
                path: "main.rs".into(),
                disposition,
                configuration_ids: configurations,
            }],
            origins: vec![],
        },
    };
    finish(
        make(Disposition::Analyzed, vec!["project".into()]),
        &input,
        "rust",
        &[],
    )
    .unwrap();
    assert!(
        finish(
            make(Disposition::Analyzed, vec!["unknown".into()]),
            &input,
            "rust",
            &[]
        )
        .is_err()
    );
    let mut invalid = make(Disposition::ExcludedTest, vec!["project".into()]);
    invalid.evidence.origins.push(Origin {
        node_id: "rust:run".into(),
        selected_paths: vec!["main.rs".into()],
    });
    invalid.graph.root_contains.push(NodeId("rust:run".into()));
    invalid.graph.nodes.push(CodeNode::Callable(CodeNodeData {
        id: NodeId("rust:run".into()),
        language: LanguageId("rust".into()),
        native_kind: "function".into(),
        name: None,
        signature: None,
        code: None,
        source: None,
        contains: vec![],
        references: vec![],
        extensions: Default::default(),
    }));
    assert!(finish(invalid, &input, "rust", &[]).is_err());
}

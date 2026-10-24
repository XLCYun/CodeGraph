//! 旧根 19 个非法来源/图变体直接测试当前语言内的 finish，不恢复私有 JSON 解析器。
use super::*;
use crate::model::{
    CodeNode, CodeNodeData, CodeReference, ExternalSymbol, LanguageId, SymbolScope,
};

/// 同进程构图证据夹具，所有字段都传给现有生产校验器，不通过公共结果视图修复。
fn projection() -> Projection {
    Projection {
        graph: CodeGraph {
            root_contains: vec![NodeId("go:Run".into())],
            nodes: vec![CodeNode::Callable(CodeNodeData {
                id: NodeId("go:Run".into()),
                language: LanguageId("go".into()),
                native_kind: "function".into(),
                name: Some("函数🙂".into()),
                signature: Some("func Run()".into()),
                code: Some("return\n中文".into()),
                source: Some("func Run() {}".into()),
                contains: vec![],
                references: vec![
                    CodeReference {
                        semantic: "code.calls".into(),
                        target: ReferenceTarget::Node {
                            id: NodeId("go:Run".into()),
                        },
                    },
                    CodeReference {
                        semantic: "code.calls".into(),
                        target: ReferenceTarget::External {
                            symbol: "go:fmt#Println".into(),
                        },
                    },
                    CodeReference {
                        semantic: "code.calls".into(),
                        target: ReferenceTarget::Unresolved {
                            name: "Missing.Call".into(),
                        },
                    },
                ],
                extensions: BTreeMap::from([("go.receiver".into(), serde_json::Value::Null)]),
            })],
            external_symbols: vec![ExternalSymbol {
                symbol: "go:fmt#Println".into(),
                name: "fmt.Println".into(),
                scope: SymbolScope::Dependency,
            }],
        },
        evidence: Evidence {
            configurations: vec![Configuration {
                id: "module".into(),
                source_paths: vec!["go.mod".into()],
                platform_ids: vec!["stdlib".into()],
            }],
            files: ["empty.go", "中文.go"]
                .into_iter()
                .map(|path| FileEvidence {
                    path: path.into(),
                    disposition: Disposition::Analyzed,
                    configuration_ids: vec!["module".into()],
                })
                .collect(),
            origins: vec![Origin {
                node_id: "go:Run".into(),
                selected_paths: vec!["中文.go".into()],
            }],
        },
    }
}

/// 只创建真实仓内源码/配置和包位置，公共请求仍只有公开字段。
fn input() -> (tempfile::TempDir, tempfile::TempDir, CheckedRequest) {
    let repository = tempfile::tempdir().unwrap();
    let package = tempfile::tempdir().unwrap();
    for path in ["empty.go", "中文.go", "go.mod"] {
        std::fs::write(repository.path().join(path), "").unwrap();
    }
    std::fs::create_dir(package.path().join("bin")).unwrap();
    let executable = package.path().join("bin/codegraph-go");
    std::fs::write(&executable, "fixture").unwrap();
    let request = serde_json::json!({"protocol":"codegraph.analyzer/1","type":"analyze","id":"historical-evidence","repository_root":repository.path(),"files":["empty.go","中文.go"],"options":{"include_tests":false,"rust_dependency_scope":"repository-only"}});
    let input = crate::public_protocol::check(
        crate::public_protocol::decode(request.to_string().as_bytes()).unwrap(),
        &executable,
    )
    .unwrap();
    (repository, package, input)
}

/// 原 Unicode、空文件覆盖、自引用、图外和未解析说明仍由真实 finish 保留。
#[test]
fn historical_complete_coverage_reference_cycles_and_external_descriptors() {
    let (_repository, _package, input) = input();
    let graph = finish(projection(), &input, "go", &["stdlib".into()]).unwrap();
    let data = graph.nodes[0].data();
    assert_eq!(data.name.as_deref(), Some("函数🙂"));
    assert!(
        data.references
            .iter()
            .any(|reference| matches!(reference.target, ReferenceTarget::Node { .. }))
    );
    assert!(matches!(
        graph.external_symbols[0].scope,
        SymbolScope::Dependency
    ));
}

/// 逐项保留旧根 G1/S3 的 19 个非法变体，不能把部分图当作成功或空图。
#[test]
fn historical_invalid_graphs_and_cross_evidence_variants() {
    let (_repository, _package, input) = input();
    for case in 0..19 {
        let mut value = projection();
        match case {
            0 => value.graph.nodes.push(value.graph.nodes[0].clone()),
            1 => {
                let CodeNode::Callable(data) = &mut value.graph.nodes[0] else {
                    unreachable!()
                };
                data.references = vec![CodeReference {
                    semantic: "code.calls".into(),
                    target: ReferenceTarget::Node {
                        id: NodeId("go:missing".into()),
                    },
                }];
            }
            2 => value.graph.root_contains.push(NodeId("go:Run".into())),
            3 => {
                let CodeNode::Callable(data) = &mut value.graph.nodes[0] else {
                    unreachable!()
                };
                data.contains.push(NodeId("go:Run".into()));
            }
            4 => value.graph.root_contains.clear(),
            5 => value.evidence.configurations.push(Configuration {
                id: "module".into(),
                source_paths: vec!["go.mod".into()],
                platform_ids: vec!["stdlib".into()],
            }),
            6 => value.evidence.configurations[0].platform_ids = vec!["unapproved".into()],
            7 => value.evidence.files[1].disposition = Disposition::ExcludedTest,
            8 => {
                value.evidence.files.pop();
            }
            9 => value.evidence.origins[0].selected_paths = vec!["outside.go".into()],
            10 => value.evidence.origins.clear(),
            11 => value.evidence.files[0].configuration_ids.clear(),
            12 => value.evidence.configurations[0].source_paths = vec!["missing.mod".into()],
            13 => {
                let CodeNode::Callable(data) = &mut value.graph.nodes[0] else {
                    unreachable!()
                };
                data.language = LanguageId("java".into());
            }
            14 => {
                let CodeNode::Callable(data) = &mut value.graph.nodes[0] else {
                    unreachable!()
                };
                data.extensions = BTreeMap::from([("unscoped".into(), serde_json::json!(1))]);
            }
            15 => value.graph.external_symbols.push(ExternalSymbol {
                symbol: "go:extra".into(),
                name: "extra".into(),
                scope: SymbolScope::Repository,
            }),
            16 => {
                let CodeNode::Callable(data) = &mut value.graph.nodes[0] else {
                    unreachable!()
                };
                data.references = vec![CodeReference {
                    semantic: "code.calls".into(),
                    target: ReferenceTarget::External {
                        symbol: "go:missing".into(),
                    },
                }];
            }
            17 => {
                value.evidence.origins[0].selected_paths = vec!["中文.go".into(), "中文.go".into()]
            }
            18 => {
                let CodeNode::Callable(data) = &mut value.graph.nodes[0] else {
                    unreachable!()
                };
                data.references = vec![CodeReference {
                    semantic: "code.calls".into(),
                    target: ReferenceTarget::External {
                        symbol: "go:Run".into(),
                    },
                }];
                value.graph.external_symbols = vec![ExternalSymbol {
                    symbol: "go:Run".into(),
                    name: "Run".into(),
                    scope: SymbolScope::Repository,
                }];
            }
            _ => unreachable!(),
        }
        assert!(
            finish(value, &input, "go", &["stdlib".into()]).is_err(),
            "非法变体 {case} 被接受"
        );
    }
}

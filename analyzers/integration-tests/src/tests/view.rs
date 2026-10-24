use crate::code_graph::{CodeGraph, NodeId, ReferenceTarget};

/// 索引视图保留源数据、原顺序及不可达节点，不做图修复来迁就历史断言。
#[test]
fn public_view_preserves_original_identity_content_edges_and_orphans() {
    let node = |id, children| serde_json::json!({"id":id,"category":"callable","language":"python","native_kind":"function","name":"来源丁","signature":"def 丁()","code":"return '  x  '","source":"# doc\r\ndef 丁(): return '  x  '\r\n","contains":children,"references":[{"semantic":"code.calls","target":{"kind":"unresolved","name":"unresolved exact"}}],"extensions":{"python.content":{"raw":[null,true,3]}}});
    let selected = node("python:unchanged%2F丁", serde_json::json!([]));
    let orphan = node("python:orphan", serde_json::json!([]));
    let data = serde_json::json!({"root_contains":["python:unchanged%2F丁"],"nodes":[orphan,selected],"external_symbols":[]});
    let before = data.clone();
    let graph = CodeGraph::decode(data.clone());
    assert_eq!(data, before);
    assert_eq!(
        graph.root().contains,
        vec![NodeId("python:unchanged%2F丁".into())]
    );
    let actual = graph
        .node(&NodeId("python:unchanged%2F丁".into()))
        .unwrap()
        .data();
    assert_eq!(
        actual.source.as_deref(),
        before["nodes"][1]["source"].as_str()
    );
    assert_eq!(actual.code.as_deref(), Some("return '  x  '"));
    assert_eq!(actual.id.0, "python:unchanged%2F丁");
    assert_eq!(actual.language.0, "python");
    assert_eq!(actual.native_kind, "function");
    assert_eq!(actual.name.as_deref(), Some("来源丁"));
    assert_eq!(actual.signature.as_deref(), Some("def 丁()"));
    assert!(actual.contains.is_empty());
    assert_eq!(actual.references.len(), 1);
    assert_eq!(actual.references[0].semantic, "code.calls");
    assert_eq!(
        actual.references[0].target,
        ReferenceTarget::Unresolved {
            name: "unresolved exact".into()
        }
    );
    assert_eq!(
        actual.extensions["python.content"],
        before["nodes"][1]["extensions"]["python.content"]
    );
    assert!(graph.node(&NodeId("python:orphan".into())).is_some());
}

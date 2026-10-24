//! 旧根图的 16 项构图不变量直接作用于当前语言输出校验；多语言合图归消费者。
use crate::model::{
    CodeCategory, CodeCategory::*, CodeNode, CodeNodeData, CodeReference, LanguageId, NodeId,
    ReferenceTarget,
};

/// 构造当前语言的固定身份，不参与真实语言图转换。
fn id(name: &str) -> NodeId {
    NodeId(format!("rust:{name}"))
}
/// 仅供不变量反例使用的六类节点 fixture。
fn data(name: &str, children: &[&str]) -> CodeNodeData {
    CodeNodeData {
        id: id(name),
        language: LanguageId("rust".into()),
        native_kind: "declaration".into(),
        name: Some(name.into()),
        signature: Some(format!("declare {name}")),
        code: None,
        source: None,
        contains: children.iter().map(|name| id(name)).collect(),
        references: vec![],
        extensions: Default::default(),
    }
}
/// 枚举类别保留原测试矩阵。
fn node(category: CodeCategory, name: &str, children: &[&str]) -> CodeNode {
    let data = data(name, children);
    match category {
        Container => CodeNode::Container(data),
        Type => CodeNode::Type(data),
        Callable => CodeNode::Callable(data),
        Value => CodeNode::Value(data),
        ExecutionRegion => CodeNode::ExecutionRegion(data),
        Extension => CodeNode::Extension(data),
    }
}
/// 图根 fixture 只保存显式成员。
#[derive(Default)]
struct CodeRoot {
    contains: Vec<NodeId>,
}
/// 指向当前被测生产 validator 的 fixture 包装，不引入第二套校验。
struct CodeGraph(crate::model::CodeGraph);
impl std::fmt::Debug for CodeGraph {
    /// 失败诊断原样显示 fixture，不要求扩展生产 DTO 的公开 trait。
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        serde_json::to_value(&self.0).unwrap().fmt(f)
    }
}

impl CodeGraph {
    /// 用真实输出边界判定反例，不修复、合并或重写任何成员。
    fn new(root: CodeRoot, nodes: Vec<CodeNode>) -> anyhow::Result<Self> {
        let graph = crate::model::CodeGraph {
            root_contains: root.contains,
            nodes,
            external_symbols: vec![],
        };
        super::validate(&graph, "rust")?;
        Ok(Self(graph))
    }
    /// 只读根成员以保留原断言 API。
    fn root(&self) -> CodeRoot {
        CodeRoot {
            contains: self.0.root_contains.clone(),
        }
    }
    /// 查询原始节点，不自动过滤、去重或排序。
    fn node(&self, id: &NodeId) -> Option<&CodeNode> {
        self.0.nodes.iter().find(|node| node.data().id == *id)
    }
    /// 按原向量遍历 fixture 节点。
    fn nodes(&self) -> impl Iterator<Item = &CodeNode> {
        self.0.nodes.iter()
    }
}
/// 显式根 fixture。
fn root(roots: &[&str]) -> CodeRoot {
    CodeRoot {
        contains: roots.iter().map(|name| id(name)).collect(),
    }
}
/// 合法图仍通过当前输出校验。
fn graph(roots: &[&str], nodes: Vec<CodeNode>) -> CodeGraph {
    CodeGraph::new(root(roots), nodes).unwrap()
}
/// 当前公共 tagged target 的本地调用 fixture。
fn calls(name: &str) -> CodeReference {
    CodeReference {
        semantic: "code.calls".into(),
        target: ReferenceTarget::Node { id: id(name) },
    }
}

/// 没有声明时仍能构建有唯一入口的空图。
#[test]
fn empty_graph_has_a_root_and_no_nodes() {
    let graph = CodeGraph::new(CodeRoot::default(), vec![]).unwrap();
    assert!(graph.root().contains.is_empty());
    assert!(graph.node(&id("missing")).is_none());
    assert!(graph.nodes().next().is_none());
}

/// 六类节点及嵌套方法、闭包均能从根按 ID 访问，类别由节点自身确定。
#[test]
fn all_categories_are_reachable_through_the_root() {
    let nodes = vec![
        node(
            Container,
            "module",
            &["type", "function", "value", "region", "extension"],
        ),
        node(Type, "type", &["method"]),
        node(Callable, "method", &[]),
        node(Callable, "function", &["closure"]),
        node(Callable, "closure", &[]),
        node(Value, "value", &[]),
        node(ExecutionRegion, "region", &[]),
        node(Extension, "extension", &[]),
    ];
    let graph = graph(&["module"], nodes.clone());
    let mut pending = graph.root().contains.clone();
    let mut visited = Vec::new();
    while let Some(id) = pending.pop() {
        let found = graph.node(&id).unwrap();
        let expected = nodes.iter().find(|node| node.data().id == id).unwrap();
        assert_eq!(found.category(), expected.category());
        assert_eq!(found.data(), expected.data());
        pending.extend(found.data().contains.clone());
        visited.push(id);
    }
    assert_eq!(visited.len(), nodes.len());
    for (name, category) in [
        ("module", Container),
        ("type", Type),
        ("method", Callable),
        ("function", Callable),
        ("closure", Callable),
        ("value", Value),
        ("region", ExecutionRegion),
        ("extension", Extension),
    ] {
        assert_eq!(graph.node(&id(name)).unwrap().category(), category);
    }
}

/// 图内身份必须全局唯一，不允许用不同语言或类别掩盖冲突。
#[test]
fn duplicate_ids_are_rejected_across_languages_and_categories() {
    let mut other = data("same", &[]);
    other.language = LanguageId("other".into());
    let error = CodeGraph::new(
        root(&["same"]),
        vec![node(Callable, "same", &[]), CodeNode::Type(other)],
    )
    .unwrap_err();
    assert!(!error.to_string().is_empty());
}

/// 虚拟根不能持有不存在的节点 ID。
#[test]
fn dangling_root_child_is_rejected() {
    assert!(CodeGraph::new(root(&["missing"]), vec![]).is_err());
}

/// 真实代码节点也不能包含不存在的子节点。
#[test]
fn dangling_nested_child_is_rejected() {
    assert!(
        CodeGraph::new(
            root(&["parent"]),
            vec![node(Container, "parent", &["missing"])],
        )
        .is_err()
    );
}

/// 已声明为图内引用的目标必须存在，不能被当成图外符号放行。
#[test]
fn dangling_local_reference_is_rejected() {
    let mut owner = data("owner", &[]);
    owner.references.push(calls("missing"));
    assert!(CodeGraph::new(root(&["owner"]), vec![CodeNode::Callable(owner)]).is_err());
}

/// 同一节点不能同时位于虚拟根和一个代码容器下。
#[test]
fn root_and_nested_placement_cannot_share_a_node() {
    assert!(
        CodeGraph::new(
            root(&["parent", "child"]),
            vec![
                node(Container, "parent", &["child"]),
                node(Type, "child", &[])
            ],
        )
        .is_err()
    );
}

/// 两个父节点对同一声明的语义关联应通过引用表达，不能重复包含。
#[test]
fn multiple_parents_are_rejected() {
    assert!(
        CodeGraph::new(
            root(&["first", "second"]),
            vec![
                node(Container, "first", &["child"]),
                node(Container, "second", &["child"]),
                node(Type, "child", &[]),
            ],
        )
        .is_err()
    );
}

/// 同一个父节点中重复列出子节点也属于重复声明位置。
#[test]
fn duplicate_children_in_one_container_are_rejected() {
    assert!(
        CodeGraph::new(
            root(&["parent"]),
            vec![
                node(Container, "parent", &["child", "child"]),
                node(Type, "child", &[]),
            ],
        )
        .is_err()
    );
}

/// 虚拟根同样遵守直接子节点唯一的约束。
#[test]
fn duplicate_root_children_are_rejected() {
    assert!(CodeGraph::new(root(&["child", "child"]), vec![node(Type, "child", &[])],).is_err());
}

/// 节点不能通过包含关系包含自身。
#[test]
fn self_containment_is_rejected() {
    assert!(CodeGraph::new(CodeRoot::default(), vec![node(Type, "self", &["self"])]).is_err());
}

/// 连到根的包含环也会违反单一声明位置，必须被拒绝。
#[test]
fn rooted_containment_cycle_is_rejected() {
    assert!(
        CodeGraph::new(
            root(&["a"]),
            vec![node(Container, "a", &["b"]), node(Container, "b", &["a"])],
        )
        .is_err()
    );
}

/// 脱离根的环不能仅靠每个节点恰有一个父节点而通过校验。
#[test]
fn disconnected_containment_cycle_is_rejected() {
    assert!(
        CodeGraph::new(
            root(&["visible"]),
            vec![
                node(Container, "visible", &[]),
                node(Container, "a", &["b"]),
                node(Container, "b", &["a"]),
            ],
        )
        .is_err()
    );
}

/// 能通过引用到达节点，仍不能替代它在声明树中的位置。
#[test]
fn reference_reachability_does_not_replace_containment() {
    let mut owner = data("owner", &[]);
    owner.references.push(calls("orphan"));
    assert!(
        CodeGraph::new(
            root(&["owner"]),
            vec![CodeNode::Callable(owner), node(Callable, "orphan", &[])],
        )
        .is_err()
    );
}

/// 自调用和相互调用是合法引用，不应被包含环的校验误拒绝。
#[test]
fn self_and_mutual_references_are_allowed() {
    let mut a = data("a", &[]);
    a.references = vec![calls("a"), calls("b")];
    let mut b = data("b", &[]);
    b.references = vec![calls("a")];
    let graph = graph(
        &["a", "b"],
        vec![CodeNode::Callable(a.clone()), CodeNode::Callable(b)],
    );
    assert_eq!(graph.node(&id("a")).unwrap().data(), &a);
}

/// 深层包含链使用图索引与显式遍历，不依赖递归对象或调用栈深度。
#[test]
fn deep_declaration_chains_can_be_validated() {
    let depth = 4096;
    let nodes = (0..depth)
        .map(|index| {
            let mut item = data(&index.to_string(), &[]);
            if index + 1 < depth {
                item.contains.push(id(&(index + 1).to_string()));
            }
            CodeNode::Container(item)
        })
        .collect();
    let graph = graph(&["0"], nodes);
    assert!(graph.node(&id(&(depth - 1).to_string())).is_some());
}

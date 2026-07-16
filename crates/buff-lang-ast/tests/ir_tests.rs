//! Integration tests for the Buff IR dataflow graph (`buff_lang_ast::ir`).
//!
//! These exercise the public API surface: graph construction, AST lowering,
//! dependency wiring, I/O boundary detection, topological sort, and Display.
//!
//! See `src/ir.rs` `#[cfg(test)] mod tests` for additional unit-level coverage
//! of the same types.

use buff_lang_ast::{
    AstLowerer, BinaryOp, Block, Decl, DispatchDecision, Expr, FuncDecl, Ident, IoNode,
    IrCycleError, IrGraph, IrNode, Literal, MemorySpace, NodeId, Span, Stmt,
};

// ---------------------------------------------------------------------------
// Test helpers
// ---------------------------------------------------------------------------

fn span() -> Span {
    Span::dummy()
}

fn int_lit(n: i64) -> Expr {
    Expr::Literal(Literal::Int(n), span())
}

fn ident_expr(name: &str) -> Expr {
    Expr::Ident(Ident::new(name, span()), span())
}

/// Wrap a list of statements into a single `fn test_fn() { ... }` declaration
/// so the lowerer can process it as a function body.
fn wrap_fn(stmts: Vec<Stmt>) -> Decl {
    Decl::FuncDecl(FuncDecl {
        name: Ident::new("test_fn", span()),
        params: Vec::new(),
        return_type: None,
        body: Block {
            stmts,
            span: span(),
        },
        is_async: false,
        is_unsafe: false,
        is_extern: false,
        attributes: Vec::new(),
        span: span(),
    })
}

fn lower(stmts: Vec<Stmt>) -> IrGraph {
    let mut lowerer = AstLowerer::new();
    lowerer.lower(&[wrap_fn(stmts)])
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

/// Test 1: `IrGraph::new()` produces an empty graph.
#[test]
fn test_empty_graph() {
    let g = IrGraph::new();
    assert_eq!(g.len(), 0, "fresh graph should have 0 nodes");
    assert!(g.is_empty());
    assert!(g.nodes.is_empty());
    assert!(g.edges.is_empty());
    assert!(g.reverse_edges.is_empty());
    assert!(g.entry_nodes.is_empty());
    assert!(g.exit_nodes.is_empty());
}

/// Test 2: adding a single ComputeNode registers it in the graph.
#[test]
fn test_add_compute_node() {
    let mut g = IrGraph::new();
    let id = g.add_node(IrNode::compute(buff_lang_ast::ComputeNode {
        id: NodeId(0),
        source_expr: Some(int_lit(42)),
        source_stmt: None,
        defs: vec![Ident::new("x", span())],
        uses: Vec::new(),
        span: span(),
        description: "lit".to_string(),
    }));
    assert_eq!(id, NodeId(0));
    assert_eq!(g.len(), 1);
    assert!(g.nodes.contains_key(&id));
    // finalize() should mark it as both entry and exit (no deps, no dependents).
    g.finalize();
    assert!(g.entry_nodes.contains(&id));
    assert!(g.exit_nodes.contains(&id));
}

/// Test 3: `add_dependency(B, A)` makes `edges[A]` contain B and
/// `reverse_edges[B]` contain A.
#[test]
fn test_dependency_edge() {
    let mut g = IrGraph::new();
    let a = g.add_node(IrNode::compute(buff_lang_ast::ComputeNode {
        id: NodeId(0),
        source_expr: None,
        source_stmt: None,
        defs: Vec::new(),
        uses: Vec::new(),
        span: span(),
        description: "a".to_string(),
    }));
    let b = g.add_node(IrNode::compute(buff_lang_ast::ComputeNode {
        id: NodeId(0),
        source_expr: None,
        source_stmt: None,
        defs: Vec::new(),
        uses: Vec::new(),
        span: span(),
        description: "b".to_string(),
    }));
    g.add_dependency(b, a); // b depends on a
    assert!(
        g.edges[&a].contains(&b),
        "edges[a] should contain b (b is a's dependent)"
    );
    assert!(
        g.reverse_edges[&b].contains(&a),
        "reverse_edges[b] should contain a (a is b's dependency)"
    );
    // The convenience accessors agree.
    assert!(g.dependents(a).contains(&b));
    assert!(g.dependencies(b).contains(&a));
}

/// Test 4: lowering `let x = 42` produces one ComputeNode with defs=["x"]
/// and no dependencies.
#[test]
fn test_let_binding_lowering() {
    let g = lower(vec![Stmt::LetDecl {
        name: Ident::new("x", span()),
        value: int_lit(42),
        mutable: false,
        ty: None,
        span: span(),
    }]);
    assert_eq!(g.len(), 1);
    let node = &g.nodes[&NodeId(0)];
    match node {
        IrNode::Compute(c) => {
            assert_eq!(c.defs.len(), 1);
            assert_eq!(c.defs[0].name, "x");
            assert!(c.uses.is_empty(), "literal has no uses");
        }
        other => panic!("expected ComputeNode, got {other:?}"),
    }
    assert!(g.reverse_edges[&NodeId(0)].is_empty());
}

/// Test 5: lowering `let a = 1; let b = a + 2; let c = b * 3` produces 3
/// nodes with edges b->a, c->b and NO transitive edge c->a.
#[test]
fn test_chain_of_lets() {
    let a = Stmt::LetDecl {
        name: Ident::new("a", span()),
        value: int_lit(1),
        mutable: false,
        ty: None,
        span: span(),
    };
    let b = Stmt::LetDecl {
        name: Ident::new("b", span()),
        value: Expr::BinaryOp {
            op: BinaryOp::Add,
            lhs: Box::new(ident_expr("a")),
            rhs: Box::new(int_lit(2)),
            span: span(),
        },
        mutable: false,
        ty: None,
        span: span(),
    };
    let c = Stmt::LetDecl {
        name: Ident::new("c", span()),
        value: Expr::BinaryOp {
            op: BinaryOp::Mul,
            lhs: Box::new(ident_expr("b")),
            rhs: Box::new(int_lit(3)),
            span: span(),
        },
        mutable: false,
        ty: None,
        span: span(),
    };
    let g = lower(vec![a, b, c]);

    assert_eq!(g.len(), 3, "expected exactly 3 statement-level nodes");
    // b (%1) depends on a (%0)
    assert!(g.reverse_edges[&NodeId(1)].contains(&NodeId(0)));
    // c (%2) depends on b (%1)
    assert!(g.reverse_edges[&NodeId(2)].contains(&NodeId(1)));
    // No transitive edge c -> a
    assert!(
        !g.reverse_edges[&NodeId(2)].contains(&NodeId(0)),
        "no transitive dependency edge expected"
    );
}

/// Test 6: sibling bindings with no data relationship produce no edges.
#[test]
fn test_dependency_graph_correctness() {
    let g = lower(vec![
        Stmt::LetDecl {
            name: Ident::new("a", span()),
            value: int_lit(1),
            mutable: false,
            ty: None,
            span: span(),
        },
        Stmt::LetDecl {
            name: Ident::new("b", span()),
            value: int_lit(2),
            mutable: false,
            ty: None,
            span: span(),
        },
    ]);
    assert_eq!(g.len(), 2);
    let total_edges: usize = g.edges.values().map(|s| s.len()).sum();
    assert_eq!(total_edges, 0, "unrelated lets must have zero edges");
    // Both nodes are both entry and exit.
    assert_eq!(g.entry_nodes.len(), 2);
    assert_eq!(g.exit_nodes.len(), 2);
}

/// Test 7: lowering `let data = http_get(url)` with http_get marked async
/// produces an IoNode with `is_suspension_point = true`.
#[test]
fn test_io_node_creation() {
    let call = Expr::FuncCall {
        callee: Box::new(ident_expr("http_get")),
        args: vec![ident_expr("url")],
        span: span(),
    };
    let stmt = Stmt::LetDecl {
        name: Ident::new("data", span()),
        value: call,
        mutable: false,
        ty: None,
        span: span(),
    };
    let mut lowerer = AstLowerer::new();
    lowerer.mark_async("http_get");
    let g = lowerer.lower(&[wrap_fn(vec![stmt])]);

    assert_eq!(g.len(), 1);
    match &g.nodes[&NodeId(0)] {
        IrNode::IONode(io) => {
            assert_eq!(io.callee.name, "http_get");
            assert!(io.is_suspension_point, "IoNode must be a suspension point");
            assert_eq!(io.defs.len(), 1);
            assert_eq!(io.defs[0].name, "data");
            assert_eq!(io.uses.len(), 1);
            assert_eq!(io.uses[0].name, "url");
        }
        other => panic!("expected IoNode, got {other:?}"),
    }
    // Sanity: the helper predicate agrees.
    assert!(g.nodes[&NodeId(0)].is_suspension_point());
}

/// Test 8: lowering `let data = http_get(url); let len = data.length()`
/// produces an IoNode for http_get, a ComputeNode for length, and a
/// dependency edge from length -> http_get.
#[test]
fn test_io_node_in_dependent_chain() {
    let fetch = Stmt::LetDecl {
        name: Ident::new("data", span()),
        value: Expr::FuncCall {
            callee: Box::new(ident_expr("http_get")),
            args: vec![ident_expr("url")],
            span: span(),
        },
        mutable: false,
        ty: None,
        span: span(),
    };
    let length = Stmt::LetDecl {
        name: Ident::new("len", span()),
        value: Expr::MethodCall {
            receiver: Box::new(ident_expr("data")),
            method: Ident::new("length", span()),
            args: Vec::new(),
            span: span(),
        },
        mutable: false,
        ty: None,
        span: span(),
    };
    let mut lowerer = AstLowerer::new();
    lowerer.mark_async("http_get");
    let g = lowerer.lower(&[wrap_fn(vec![fetch, length])]);

    assert_eq!(g.len(), 2);
    assert!(
        matches!(g.nodes[&NodeId(0)], IrNode::IONode(_)),
        "http_get -> IoNode"
    );
    assert!(
        matches!(g.nodes[&NodeId(1)], IrNode::Compute(_)),
        "length() -> ComputeNode"
    );
    // len (%1) depends on data (%0).
    assert!(
        g.reverse_edges[&NodeId(1)].contains(&NodeId(0)),
        "length node must depend on http_get node"
    );
    assert_eq!(g.entry_nodes, vec![NodeId(0)]);
    assert_eq!(g.exit_nodes, vec![NodeId(1)]);
}

/// Test 9: topological order places every dependency before its dependent.
#[test]
fn test_topological_order() {
    // let a = 1; let b = a + 1; let c = b + 1;
    let a = Stmt::LetDecl {
        name: Ident::new("a", span()),
        value: int_lit(1),
        mutable: false,
        ty: None,
        span: span(),
    };
    let b = Stmt::LetDecl {
        name: Ident::new("b", span()),
        value: Expr::BinaryOp {
            op: BinaryOp::Add,
            lhs: Box::new(ident_expr("a")),
            rhs: Box::new(int_lit(1)),
            span: span(),
        },
        mutable: false,
        ty: None,
        span: span(),
    };
    let c = Stmt::LetDecl {
        name: Ident::new("c", span()),
        value: Expr::BinaryOp {
            op: BinaryOp::Add,
            lhs: Box::new(ident_expr("b")),
            rhs: Box::new(int_lit(1)),
            span: span(),
        },
        mutable: false,
        ty: None,
        span: span(),
    };
    let g = lower(vec![a, b, c]);
    let order = g
        .topological_order()
        .expect("acyclic lowering must topo-sort cleanly");
    assert_eq!(order.len(), 3);
    let position = |id: NodeId| order.iter().position(|&x| x == id);
    assert!(position(NodeId(0)) < position(NodeId(1)));
    assert!(position(NodeId(1)) < position(NodeId(2)));
}

/// Test 10: after `finalize`, entry_nodes are those with no dependencies and
/// exit_nodes are those with no dependents.
#[test]
fn test_finalize_entry_exit() {
    // Diamond: a -> b, a -> c, b -> d, c -> d
    let mut g = IrGraph::new();
    let mk = |label: &str| {
        IrNode::compute(buff_lang_ast::ComputeNode {
            id: NodeId(0),
            source_expr: None,
            source_stmt: None,
            defs: Vec::new(),
            uses: Vec::new(),
            span: span(),
            description: label.to_string(),
        })
    };
    let a = g.add_node(mk("a"));
    let b = g.add_node(mk("b"));
    let c = g.add_node(mk("c"));
    let d = g.add_node(mk("d"));
    g.add_dependency(b, a);
    g.add_dependency(c, a);
    g.add_dependency(d, b);
    g.add_dependency(d, c);
    g.finalize();

    assert_eq!(g.entry_nodes, vec![a], "only a has no dependencies");
    assert_eq!(g.exit_nodes, vec![d], "only d has no dependents");
}

/// Test 11: `IrGraph` Display produces human-readable output.
#[test]
fn test_display_readable() {
    let g = lower(vec![
        Stmt::LetDecl {
            name: Ident::new("a", span()),
            value: int_lit(1),
            mutable: false,
            ty: None,
            span: span(),
        },
        Stmt::LetDecl {
            name: Ident::new("b", span()),
            value: ident_expr("a"),
            mutable: false,
            ty: None,
            span: span(),
        },
    ]);
    let s = g.to_string();
    assert!(s.starts_with("IR Graph (2 nodes, 1 edges):"), "got: {s}");
    assert!(s.contains("%0"), "should reference node %0");
    assert!(s.contains("%1"), "should reference node %1");
    assert!(s.contains("defs=[a]"), "should list defs");
    assert!(s.contains("%0 -> %1"), "should show edge");
    assert!(s.contains("Entry: [%0]"));
    assert!(s.contains("Exit: [%1]"));
}

/// Test 12: a manually constructed cycle makes `topological_order` return Err.
#[test]
fn test_cycle_detection() {
    let mut g = IrGraph::new();
    let mk = |label: &str| {
        IrNode::compute(buff_lang_ast::ComputeNode {
            id: NodeId(0),
            source_expr: None,
            source_stmt: None,
            defs: Vec::new(),
            uses: Vec::new(),
            span: span(),
            description: label.to_string(),
        })
    };
    let a = g.add_node(mk("a"));
    let b = g.add_node(mk("b"));
    g.add_dependency(a, b); // a depends on b
    g.add_dependency(b, a); // b depends on a  -> cycle!
    let result = g.topological_order();
    assert!(
        matches!(result, Err(IrCycleError)),
        "cycle must be detected"
    );
}

/// Test 13: dependencies/dependents accessors return sorted, deduplicated Vecs.
#[test]
fn test_dependencies_and_dependents_accessors() {
    let mut g = IrGraph::new();
    let mk = |label: &str| {
        IrNode::compute(buff_lang_ast::ComputeNode {
            id: NodeId(0),
            source_expr: None,
            source_stmt: None,
            defs: Vec::new(),
            uses: Vec::new(),
            span: span(),
            description: label.to_string(),
        })
    };
    let a = g.add_node(mk("a"));
    let b = g.add_node(mk("b"));
    let c = g.add_node(mk("c"));
    // c depends on both a and b.
    g.add_dependency(c, a);
    g.add_dependency(c, b);
    // Adding the same edge twice is idempotent.
    g.add_dependency(c, a);

    let deps = g.dependencies(c);
    assert_eq!(deps, vec![a, b], "dependencies must be sorted and deduped");
    assert_eq!(g.dependents(a), vec![c]);
    assert_eq!(g.dependents(b), vec![c]);
}

/// Test 14: `mark_async` + auto-registration from `async fn` decls both work.
#[test]
fn test_async_registration_and_display_of_ionode() {
    // async fn producer() { return 0; }
    let producer = Decl::FuncDecl(FuncDecl {
        name: Ident::new("producer", span()),
        params: Vec::new(),
        return_type: None,
        body: Block {
            stmts: vec![Stmt::Return(Some(int_lit(0)), span())],
            span: span(),
        },
        is_async: true,
        is_unsafe: false,
        is_extern: false,
        attributes: Vec::new(),
        span: span(),
    });
    // fn caller() { let r = producer(); }
    let caller = Decl::FuncDecl(FuncDecl {
        name: Ident::new("caller", span()),
        params: Vec::new(),
        return_type: None,
        body: Block {
            stmts: vec![Stmt::LetDecl {
                name: Ident::new("r", span()),
                value: Expr::FuncCall {
                    callee: Box::new(ident_expr("producer")),
                    args: Vec::new(),
                    span: span(),
                },
                mutable: false,
                ty: None,
                span: span(),
            }],
            span: span(),
        },
        is_async: false,
        is_unsafe: false,
        is_extern: false,
        attributes: Vec::new(),
        span: span(),
    });
    let mut lowerer = AstLowerer::new();
    let g = lowerer.lower(&[producer, caller]);

    let io_nodes: Vec<&IoNode> = g
        .nodes
        .values()
        .filter_map(|n| match n {
            IrNode::IONode(io) => Some(io),
            _ => None,
        })
        .collect();
    assert_eq!(io_nodes.len(), 1, "exactly one IoNode expected");
    assert_eq!(io_nodes[0].callee.name, "producer");
    assert!(io_nodes[0].is_suspension_point);

    // Display of an IoNode should mention the callee.
    let io_display = io_nodes[0].to_string();
    assert!(io_display.contains("IO(producer)"), "got: {io_display}");
}

/// Test 15: placeholder node types (Transfer/Schedule) can be added to the
/// graph and round-trip through Display (v1.0 scheduler will populate them).
#[test]
fn test_placeholder_node_types() {
    let mut g = IrGraph::new();
    let transfer = g.add_node(IrNode::Transfer(buff_lang_ast::TransferNode {
        id: NodeId(0),
        var: Ident::new("buf", span()),
        from: MemorySpace::Cpu,
        to: MemorySpace::GpuLocal,
        span: span(),
    }));
    let schedule = g.add_node(IrNode::Schedule(buff_lang_ast::ScheduleNode {
        id: NodeId(0),
        governed: vec![transfer],
        decision: DispatchDecision::SequentialCpu,
        span: span(),
    }));
    g.finalize();
    assert_eq!(g.len(), 2);

    let t_disp = g.nodes[&transfer].to_string();
    assert!(t_disp.contains("Transfer(buf"), "got: {t_disp}");
    assert!(t_disp.contains("Cpu -> GpuLocal"));

    let s_disp = g.nodes[&schedule].to_string();
    assert!(
        s_disp.contains("Schedule(decision=SequentialCpu"),
        "got: {s_disp}"
    );
    assert!(s_disp.contains("%0")); // governs the transfer node
}

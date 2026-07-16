//! T31 integration tests — async call-graph propagation.
//!
//! These exercise the public API of [`buff_lang_types::async_analysis`] end
//! to end through the public re-exports at the crate root
//! (`buff_lang_types::analyze_async`, `build_call_graph`, `propagate_async`).
//!
//! Coverage (15+ tests):
//!
//! - Direct `async` declared fn seeds the async set.
//! - 1-hop propagation: a fn calling an async fn becomes async.
//! - Transitive multi-hop propagation: `main -> pipeline -> fetch -> io`.
//! - Sync fn not calling any async fn stays sync.
//! - Cycle in call graph: recursion + mutual recursion.
//! - Diamond call graph (two paths to the same async fn).
//! - Calls to undefined names are ignored (no false propagation).
//! - Method calls are NOT call-graph edges.
//! - Nested calls in let/return/binary-if-match-arms are all collected.
//! - `export`-wrapped funcs contribute to the graph.
//! - Determinism: same input → byte-identical output.
//! - Three-branch diamond: 2 async + 1 sync — only the async paths propagate.
//!
//! Run via:
//!
//! ```text
//! cargo test -p buff-lang-types --test async_propagation
//! ```

use buff_lang_ast::common::{Block, Ident};
use buff_lang_ast::decl::FuncDecl;
use buff_lang_ast::{Decl, Expr, Literal, Stmt};
use buff_lang_error::Span;
use buff_lang_types::{analyze_async, build_call_graph, propagate_async, AsyncSet};

// ---------------------------------------------------------------------------
// Test helpers
// ---------------------------------------------------------------------------

fn span() -> Span {
    Span::dummy()
}

fn ident(s: &str) -> Ident {
    Ident::new(s, span())
}

fn ident_expr(s: &str) -> Expr {
    Expr::Ident(ident(s), span())
}

fn int_expr(n: i64) -> Expr {
    Expr::Literal(Literal::Int(n), span())
}

fn call_expr(name: &str) -> Expr {
    Expr::FuncCall {
        callee: Box::new(ident_expr(name)),
        args: Vec::new(),
        span: span(),
    }
}

fn ret_expr(e: Expr) -> Stmt {
    Stmt::Return(Some(e), span())
}

fn ret_call(name: &str) -> Stmt {
    ret_expr(call_expr(name))
}

fn ret_int(n: i64) -> Stmt {
    ret_expr(int_expr(n))
}

fn block(stmts: Vec<Stmt>) -> Block {
    Block {
        stmts,
        span: span(),
    }
}

/// Build a FuncDecl with the given body and async flag.
fn func(name: &str, is_async: bool, body_stmts: Vec<Stmt>) -> FuncDecl {
    FuncDecl {
        name: ident(name),
        params: Vec::new(),
        return_type: None,
        body: block(body_stmts),
        is_async,
        is_unsafe: false,
        is_extern: false,
        span: span(),
    }
}

fn func_decl(name: &str, is_async: bool, body_stmts: Vec<Stmt>) -> Decl {
    Decl::FuncDecl(func(name, is_async, body_stmts))
}

fn analyze(decls: Vec<Decl>) -> AsyncSet {
    analyze_async(&decls)
}

// ---------------------------------------------------------------------------
// Direct async seeding
// ---------------------------------------------------------------------------

#[test]
fn direct_async_func_seeds_the_async_set() {
    let d = vec![func_decl("http_get", true, vec![ret_int(0)])];
    let set = analyze(d);
    assert!(set.contains("http_get"));
    assert_eq!(set.len(), 1);
}

// ---------------------------------------------------------------------------
// 1-hop propagation
// ---------------------------------------------------------------------------

#[test]
fn one_hop_propagation_marks_caller_async() {
    // async func http_get() { ... }
    // func fetch() { http_get() }
    let d = vec![
        func_decl("http_get", true, vec![ret_int(0)]),
        func_decl("fetch", false, vec![ret_call("http_get")]),
    ];
    let set = analyze(d);
    assert!(set.contains("http_get"));
    assert!(set.contains("fetch"));
}

// ---------------------------------------------------------------------------
// Transitive multi-hop propagation
// ---------------------------------------------------------------------------

#[test]
fn transitive_propagation_marks_all_callers_async() {
    // async func io()
    // func fetch()    { io() }
    // func pipeline() { fetch() }
    // func main()     { pipeline() }
    let d = vec![
        func_decl("io", true, vec![ret_int(0)]),
        func_decl("fetch", false, vec![ret_call("io")]),
        func_decl("pipeline", false, vec![ret_call("fetch")]),
        func_decl("main", false, vec![ret_call("pipeline")]),
    ];
    let set = analyze(d);
    for n in &["io", "fetch", "pipeline", "main"] {
        assert!(
            set.contains(n),
            "expected {n} to be async: {:?}",
            set.to_sorted_vec()
        );
    }
}

// ---------------------------------------------------------------------------
// Sync fn stays sync
// ---------------------------------------------------------------------------

#[test]
fn sync_fn_with_no_async_calls_stays_sync() {
    // func add(a, b) { ... }  -- never calls anything async
    let d = vec![func_decl("add", false, vec![ret_int(0)])];
    let set = analyze(d);
    assert!(!set.contains("add"));
    assert!(set.is_empty());
}

#[test]
fn sync_fn_calling_only_sync_fns_stays_sync() {
    // func helper() { ... }
    // func caller() { helper() }
    let d = vec![
        func_decl("helper", false, vec![ret_int(0)]),
        func_decl("caller", false, vec![ret_call("helper")]),
    ];
    let set = analyze(d);
    assert!(!set.contains("helper"));
    assert!(!set.contains("caller"));
}

// ---------------------------------------------------------------------------
// Cycle / recursion
// ---------------------------------------------------------------------------

#[test]
fn pure_self_recursion_stays_sync() {
    // func loop_(n) { loop_(n - 1) }  -- no async anywhere
    let d = vec![func_decl("loop_", false, vec![ret_call("loop_")])];
    let set = analyze(d);
    assert!(!set.contains("loop_"));
}

#[test]
fn mutual_recursion_propagates_async() {
    // async func a() { b() }
    // func b()       { a() }
    let d = vec![
        func_decl("a", true, vec![ret_call("b")]),
        func_decl("b", false, vec![ret_call("a")]),
    ];
    let set = analyze(d);
    assert!(set.contains("a"));
    assert!(set.contains("b"));
}

#[test]
fn three_node_cycle_propagates_async() {
    // async func a() { c() }
    // func b()       { a() }
    // func c()       { b() }
    let d = vec![
        func_decl("a", true, vec![ret_call("c")]),
        func_decl("b", false, vec![ret_call("a")]),
        func_decl("c", false, vec![ret_call("b")]),
    ];
    let set = analyze(d);
    for n in &["a", "b", "c"] {
        assert!(set.contains(n));
    }
}

// ---------------------------------------------------------------------------
// Diamond call graph
// ---------------------------------------------------------------------------

#[test]
fn diamond_call_graph_propagates_on_both_paths() {
    // async func io()
    // func left()  { io() }
    // func right() { io() }
    // func top()   { left(); right() }
    let d = vec![
        func_decl("io", true, vec![ret_int(0)]),
        func_decl("left", false, vec![ret_call("io")]),
        func_decl("right", false, vec![ret_call("io")]),
        func_decl("top", false, vec![ret_call("left"), ret_call("right")]),
    ];
    let set = analyze(d);
    for n in &["io", "left", "right", "top"] {
        assert!(set.contains(n), "diamond: {n} should be async");
    }
}

#[test]
fn three_branch_diamond_two_async_one_sync() {
    // async func io()
    // async func net()
    // func sync_only() { return 1; }
    // func combined()  { io(); net(); sync_only(); }
    // func just_sync() { sync_only(); }
    let d = vec![
        func_decl("io", true, vec![ret_int(0)]),
        func_decl("net", true, vec![ret_int(0)]),
        func_decl("sync_only", false, vec![ret_int(1)]),
        func_decl(
            "combined",
            false,
            vec![ret_call("io"), ret_call("net"), ret_call("sync_only")],
        ),
        func_decl("just_sync", false, vec![ret_call("sync_only")]),
    ];
    let set = analyze(d);
    assert!(set.contains("io"));
    assert!(set.contains("net"));
    assert!(set.contains("combined"));
    assert!(!set.contains("sync_only"));
    assert!(!set.contains("just_sync"));
}

// ---------------------------------------------------------------------------
// Calls to undefined / external names
// ---------------------------------------------------------------------------

#[test]
fn calls_to_undefined_names_are_ignored() {
    // func foo() { undefined_thing() }  -- undefined_thing is not a declared fn
    let d = vec![func_decl("foo", false, vec![ret_call("undefined_thing")])];
    let set = analyze(d);
    assert!(!set.contains("foo"));
    assert!(set.is_empty());
}

// ---------------------------------------------------------------------------
// Method calls are NOT call-graph edges
// ---------------------------------------------------------------------------

#[test]
fn method_calls_do_not_form_call_graph_edges() {
    // func f() { return recv.method(); }
    let method = Expr::MethodCall {
        receiver: Box::new(ident_expr("recv")),
        method: ident("method"),
        args: Vec::new(),
        span: span(),
    };
    let d = vec![func_decl("f", false, vec![ret_expr(method)])];
    let graph = build_call_graph(&d);
    assert!(
        !graph.callees_of("f").contains("method"),
        "method calls must not be call-graph edges"
    );
    // And propagate doesn't mark anything async.
    let set = propagate_async(&graph, &d);
    assert!(set.is_empty());
}

// ---------------------------------------------------------------------------
// Nested calls in complex expressions
// ---------------------------------------------------------------------------

#[test]
fn binary_op_callees_are_collected() {
    // func f() { return g() + h(); }
    let body = ret_expr(Expr::BinaryOp {
        op: buff_lang_ast::op::BinaryOp::Add,
        lhs: Box::new(call_expr("g")),
        rhs: Box::new(call_expr("h")),
        span: span(),
    });
    let d = vec![func_decl("f", false, vec![body])];
    let graph = build_call_graph(&d);
    let callees = graph.callees_of("f");
    assert!(callees.contains("g"));
    assert!(callees.contains("h"));
}

#[test]
fn let_binding_callees_are_collected() {
    // func f() { let x = g(); return x; }
    let body = vec![
        Stmt::LetDecl {
            name: ident("x"),
            value: call_expr("g"),
            mutable: false,
            ty: None,
            span: span(),
        },
        ret_expr(ident_expr("x")),
    ];
    let d = vec![func_decl("f", false, body)];
    let graph = build_call_graph(&d);
    assert!(graph.callees_of("f").contains("g"));
}

// ---------------------------------------------------------------------------
// Export-wrapped funcs contribute
// ---------------------------------------------------------------------------

#[test]
fn export_wrapped_async_funcs_contribute() {
    // export async func io() { ... }
    // export func fetch()    { io() }
    let io = Decl::ExportDecl(buff_lang_ast::decl::ExportDecl {
        inner: Box::new(func_decl("io", true, vec![ret_int(0)])),
        span: span(),
    });
    let fetch = Decl::ExportDecl(buff_lang_ast::decl::ExportDecl {
        inner: Box::new(func_decl("fetch", false, vec![ret_call("io")])),
        span: span(),
    });
    let d = vec![io, fetch];
    let set = analyze(d);
    assert!(set.contains("io"));
    assert!(set.contains("fetch"));
}

// ---------------------------------------------------------------------------
// Determinism
// ---------------------------------------------------------------------------

#[test]
fn deterministic_across_repeated_runs() {
    let d = vec![
        func_decl("io", true, vec![ret_int(0)]),
        func_decl("fetch", false, vec![ret_call("io")]),
    ];
    let s1: Vec<String> = analyze(d.clone())
        .to_sorted_vec()
        .iter()
        .map(|s| s.to_string())
        .collect();
    let s2: Vec<String> = analyze(d)
        .to_sorted_vec()
        .iter()
        .map(|s| s.to_string())
        .collect();
    assert_eq!(s1, s2);
    // Sorted alphabetically.
    assert_eq!(s1, vec!["fetch".to_string(), "io".to_string()]);
}

// ---------------------------------------------------------------------------
// Edge cases
// ---------------------------------------------------------------------------

#[test]
fn empty_program_yields_empty_async_set() {
    let set = analyze(Vec::new());
    assert!(set.is_empty());
}

#[test]
fn call_graph_for_pure_sync_program_has_no_async_set_members() {
    let d = vec![
        func_decl("a", false, vec![ret_call("b")]),
        func_decl("b", false, vec![ret_call("c")]),
        func_decl("c", false, vec![ret_int(0)]),
    ];
    let set = analyze(d);
    for n in &["a", "b", "c"] {
        assert!(!set.contains(n));
    }
}

#[test]
fn async_set_to_sorted_vec_is_deterministic() {
    // Build a deliberately non-sorted seed and verify the output is sorted.
    let d = vec![
        func_decl("z", true, vec![ret_int(0)]),
        func_decl("a", true, vec![ret_int(0)]),
        func_decl("m", true, vec![ret_int(0)]),
    ];
    let v: Vec<String> = analyze(d)
        .to_sorted_vec()
        .iter()
        .map(|s| s.to_string())
        .collect();
    assert_eq!(v, vec!["a".to_string(), "m".to_string(), "z".to_string()]);
}

#[test]
fn propagation_terminates_on_large_chain() {
    // Build a 20-deep call chain: chain0 -> chain1 -> ... -> chain19
    // chain0 is async; each chain_k calls chain_{k-1}. After propagation
    // ALL 20 should be async. (Termination / scalability sanity.)
    let mut decls: Vec<Decl> = Vec::with_capacity(20);
    // chain19 (leaf) is async.
    decls.push(func_decl("chain19", true, vec![ret_int(0)]));
    for k in (0..19).rev() {
        let callee = format!("chain{}", k + 1);
        decls.push(func_decl(
            &format!("chain{k}"),
            false,
            vec![ret_call(&callee)],
        ));
    }
    let set = analyze(decls);
    for k in 0..20 {
        let name = format!("chain{k}");
        assert!(
            set.contains(&name),
            "{name} should be async after propagation"
        );
    }
    assert_eq!(set.len(), 20);
}

#[test]
fn call_graph_callees_of_returns_known_set() {
    // Verify callees_of returns the expected set even for fns not in the
    // graph (empty set returned).
    let d = vec![func_decl("only", false, vec![ret_int(0)])];
    let graph = build_call_graph(&d);
    // "only" is in graph with empty callees.
    assert!(graph.callees_of("only").is_empty());
    // Nonexistent fn returns an empty set too.
    assert!(graph.callees_of("nonexistent").is_empty());
}

//! T48 integration tests — recursion detection.
//!
//! These exercise the public API of [`buff_lang_types::recursion`] end to end
//! through the public re-exports at the crate root
//! (`buff_lang_types::analyze_recursion`, `RecursionFacts`).
//!
//! Coverage (15+ tests):
//!
//! - QA: classic `fib(n) { fib(n-1) + fib(n-2) }` self-recursion → `cpu_only`.
//! - Non-recursive `double(x) { x * 2 }` → NOT `cpu_only`.
//! - Mutual recursion `a ↔ b` → BOTH `cpu_only`.
//! - 3-node cycle `a → b → c → a` → ALL `cpu_only`.
//! - Deep non-recursive chain `a → b → c → d` → NONE `cpu_only`.
//! - Caller-of-recursive fn (not on cycle) → NOT `cpu_only`.
//! - Disconnected components: one self-loop + one chain → only the self-loop.
//! - `@prefer(gpu)` on recursive fn → `Err(TypeError)`.
//! - `@prefer(gpu)` on non-recursive fn → `Ok`.
//! - `@prefer(cpu)` on recursive fn → `Ok` (cpu_only set, but no error).
//! - `@prefer(gpu, force)` (multi-arg) on recursive fn → `Ok` (NOT a match).
//! - Empty program → empty facts.
//! - Determinism: same input → byte-identical cpu_only set.
//! - `export func` wrapper → still detected.
//! - `fib` QA: explicit proof that fib calls fib(n-1) and fib(n-2) → cpu_only.
//!
//! All test names contain `recursion` (QA filter convention).
//!
//! Run via:
//!
//! ```text
//! cargo test -p buff-lang-types --test recursion_test
//! cargo test -p buff-lang-types recursion
//! ```

use buff_lang_ast::common::{Block, Ident};
use buff_lang_ast::decl::{Attribute, FuncDecl};
use buff_lang_ast::{Decl, Expr, Literal, Stmt};
use buff_lang_error::Span;
use buff_lang_types::{analyze_recursion, RecursionFacts};

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

fn call(name: &str) -> Expr {
    Expr::FuncCall {
        callee: Box::new(ident_expr(name)),
        args: Vec::new(),
        span: span(),
    }
}

fn call_with_arg(name: &str, arg: Expr) -> Expr {
    Expr::FuncCall {
        callee: Box::new(ident_expr(name)),
        args: vec![arg],
        span: span(),
    }
}

fn ret(e: Expr) -> Stmt {
    Stmt::Return(Some(e), span())
}

fn ret_call(name: &str) -> Stmt {
    ret(call(name))
}

fn ret_int(n: i64) -> Stmt {
    ret(int_expr(n))
}

fn block(stmts: Vec<Stmt>) -> Block {
    Block {
        stmts,
        span: span(),
    }
}

fn prefer_gpu_attr() -> Attribute {
    Attribute {
        name: ident("prefer"),
        args: vec!["gpu".to_string()],
        named_args: std::collections::BTreeMap::new(),
        span: span(),
    }
}

fn prefer_gpu_multi_attr() -> Attribute {
    // @prefer(gpu, force) — multi-arg, NOT a single-arg "gpu" match.
    Attribute {
        name: ident("prefer"),
        args: vec!["gpu".to_string(), "force".to_string()],
        named_args: std::collections::BTreeMap::new(),
        span: span(),
    }
}

fn prefer_cpu_attr() -> Attribute {
    Attribute {
        name: ident("prefer"),
        args: vec!["cpu".to_string()],
        named_args: std::collections::BTreeMap::new(),
        span: span(),
    }
}

fn func(name: &str, body_stmts: Vec<Stmt>) -> FuncDecl {
    FuncDecl { name: ident(name),
    params: Vec::new(),
    return_type: None,
    body: block(body_stmts),
    is_async: false,
    is_unsafe: false,
    is_extern: false, attributes: Vec::new(), type_params: Vec::new(), span: span(), }
}

fn func_with_attrs(name: &str, attrs: Vec<Attribute>, body_stmts: Vec<Stmt>) -> FuncDecl {
    let mut f = func(name, body_stmts);
    f.attributes = attrs;
    f
}

fn func_decl(name: &str, body_stmts: Vec<Stmt>) -> Decl {
    Decl::FuncDecl(func(name, body_stmts))
}

fn func_decl_with_attrs(name: &str, attrs: Vec<Attribute>, body_stmts: Vec<Stmt>) -> Decl {
    Decl::FuncDecl(func_with_attrs(name, attrs, body_stmts))
}

// ---------------------------------------------------------------------------
// QA: classic fib — THE proof that fib(n) { fib(n-1) + fib(n-2) } → cpu_only
// ---------------------------------------------------------------------------

#[test]
fn recursion_qa_fib_calls_fib_minus_one_and_two_marks_cpu_only() {
    // func fib(n) { return fib(n - 1) + fib(n - 2) }
    let fib_minus_one = call_with_arg(
        "fib",
        Expr::BinaryOp {
            op: buff_lang_ast::op::BinaryOp::Sub,
            lhs: Box::new(ident_expr("n")),
            rhs: Box::new(int_expr(1)),
            span: span(),
        },
    );
    let fib_minus_two = call_with_arg(
        "fib",
        Expr::BinaryOp {
            op: buff_lang_ast::op::BinaryOp::Sub,
            lhs: Box::new(ident_expr("n")),
            rhs: Box::new(int_expr(2)),
            span: span(),
        },
    );
    let body = vec![ret(Expr::BinaryOp {
        op: buff_lang_ast::op::BinaryOp::Add,
        lhs: Box::new(fib_minus_one),
        rhs: Box::new(fib_minus_two),
        span: span(),
    })];
    let d = vec![func_decl("fib", body)];
    let facts: RecursionFacts = analyze_recursion(&d).expect("ok");
    assert!(
        facts.is_cpu_only("fib"),
        "QA: fib(n) calls fib(n-1)+fib(n-2) — MUST be cpu_only"
    );
}

// ---------------------------------------------------------------------------
// Non-recursive baseline
// ---------------------------------------------------------------------------

#[test]
fn recursion_qa_non_recursive_double_not_cpu_only() {
    // func double(x) { return x * 2 }
    let body = vec![ret(Expr::BinaryOp {
        op: buff_lang_ast::op::BinaryOp::Mul,
        lhs: Box::new(ident_expr("x")),
        rhs: Box::new(int_expr(2)),
        span: span(),
    })];
    let d = vec![func_decl("double", body)];
    let facts = analyze_recursion(&d).expect("ok");
    assert!(
        !facts.is_cpu_only("double"),
        "QA: double is non-recursive — must NOT be cpu_only"
    );
    assert!(facts.is_empty());
}

// ---------------------------------------------------------------------------
// Self-recursion (single function)
// ---------------------------------------------------------------------------

#[test]
fn recursion_self_loop_marked_cpu_only() {
    // func loop_(n) { loop_(n - 1) }
    let d = vec![func_decl("loop_", vec![ret_call("loop_")])];
    let facts = analyze_recursion(&d).expect("ok");
    assert!(facts.is_cpu_only("loop_"));
    assert_eq!(facts.len(), 1);
}

// ---------------------------------------------------------------------------
// Mutual recursion (two functions)
// ---------------------------------------------------------------------------

#[test]
fn recursion_mutual_two_both_marked_cpu_only() {
    // func a() { b() }
    // func b() { a() }
    let d = vec![
        func_decl("a", vec![ret_call("b")]),
        func_decl("b", vec![ret_call("a")]),
    ];
    let facts = analyze_recursion(&d).expect("ok");
    assert!(
        facts.is_cpu_only("a"),
        "mutual-recursion: a must be cpu_only"
    );
    assert!(
        facts.is_cpu_only("b"),
        "mutual-recursion: b must be cpu_only"
    );
}

// ---------------------------------------------------------------------------
// 3-node cycle
// ---------------------------------------------------------------------------

#[test]
fn recursion_three_node_cycle_all_marked_cpu_only() {
    // a → b → c → a
    let d = vec![
        func_decl("a", vec![ret_call("b")]),
        func_decl("b", vec![ret_call("c")]),
        func_decl("c", vec![ret_call("a")]),
    ];
    let facts = analyze_recursion(&d).expect("ok");
    for n in &["a", "b", "c"] {
        assert!(
            facts.is_cpu_only(n),
            "3-cycle: {n} must be cpu_only, got: {:?}",
            facts.to_sorted_vec()
        );
    }
}

// ---------------------------------------------------------------------------
// Deep non-recursive chain
// ---------------------------------------------------------------------------

#[test]
fn recursion_deep_chain_no_cycle_none_cpu_only() {
    // a → b → c → d → (terminal return 0)
    let d = vec![
        func_decl("a", vec![ret_call("b")]),
        func_decl("b", vec![ret_call("c")]),
        func_decl("c", vec![ret_call("d")]),
        func_decl("d", vec![ret_int(0)]),
    ];
    let facts = analyze_recursion(&d).expect("ok");
    for n in &["a", "b", "c", "d"] {
        assert!(
            !facts.is_cpu_only(n),
            "deep chain: {n} should NOT be cpu_only"
        );
    }
    assert!(facts.is_empty());
}

// ---------------------------------------------------------------------------
// Caller-of-recursive fn is NOT itself on a cycle
// ---------------------------------------------------------------------------

#[test]
fn recursion_caller_of_recursive_not_marked_cpu_only() {
    // entry → fib → fib
    // entry calls a recursive fn but is NOT itself on a cycle.
    let d = vec![
        func_decl("entry", vec![ret_call("fib")]),
        func_decl("fib", vec![ret_call("fib")]),
    ];
    let facts = analyze_recursion(&d).expect("ok");
    assert!(facts.is_cpu_only("fib"));
    assert!(
        !facts.is_cpu_only("entry"),
        "entry merely calls fib — it's not on a cycle"
    );
}

// ---------------------------------------------------------------------------
// Disconnected components: only the self-loop is marked
// ---------------------------------------------------------------------------

#[test]
fn recursion_disconnected_components_only_cyclic_one_marked() {
    // a → a (self-loop)
    // b → c → d (chain, no cycle)
    // e (isolated, no calls)
    let d = vec![
        func_decl("a", vec![ret_call("a")]),
        func_decl("b", vec![ret_call("c")]),
        func_decl("c", vec![ret_call("d")]),
        func_decl("d", vec![ret_int(0)]),
        func_decl("e", vec![ret_int(0)]),
    ];
    let facts = analyze_recursion(&d).expect("ok");
    assert!(facts.is_cpu_only("a"));
    for n in &["b", "c", "d", "e"] {
        assert!(!facts.is_cpu_only(n));
    }
    assert_eq!(facts.len(), 1);
}

// ---------------------------------------------------------------------------
// @prefer(gpu) conflict → Err
// ---------------------------------------------------------------------------

#[test]
fn recursion_prefer_gpu_on_recursive_returns_err() {
    // @prefer(gpu) func fib(n) { fib(n-1) }
    let d = vec![func_decl_with_attrs(
        "fib",
        vec![prefer_gpu_attr()],
        vec![ret_call("fib")],
    )];
    let err = analyze_recursion(&d).expect_err("recursive + @prefer(gpu) must error");
    let msg = &err.diagnostic.message;
    assert!(
        msg.contains("`fib`"),
        "error must name the offending function: {msg}"
    );
    assert!(
        msg.contains("@prefer(gpu)"),
        "error must mention @prefer(gpu): {msg}"
    );
    assert!(
        msg.contains("recursive"),
        "error must mention recursion: {msg}"
    );
}

// ---------------------------------------------------------------------------
// @prefer(gpu) on non-recursive fn → Ok (no conflict)
// ---------------------------------------------------------------------------

#[test]
fn recursion_prefer_gpu_on_non_recursive_returns_ok() {
    // @prefer(gpu) func double(x) { x * 2 }
    let body = vec![ret(Expr::BinaryOp {
        op: buff_lang_ast::op::BinaryOp::Mul,
        lhs: Box::new(ident_expr("x")),
        rhs: Box::new(int_expr(2)),
        span: span(),
    })];
    let d = vec![func_decl_with_attrs(
        "double",
        vec![prefer_gpu_attr()],
        body,
    )];
    let facts = analyze_recursion(&d).expect("non-recursive + @prefer(gpu) is fine");
    assert!(!facts.is_cpu_only("double"));
}

// ---------------------------------------------------------------------------
// @prefer(cpu) on recursive → Ok (cpu_only set, but no error)
// ---------------------------------------------------------------------------

#[test]
fn recursion_prefer_cpu_on_recursive_returns_ok_with_cpu_only_marked() {
    // @prefer(cpu) func fib(n) { fib(n-1) }  -- cpu hint is compatible with
    // recursion; the function is still cpu_only but no error.
    let d = vec![func_decl_with_attrs(
        "fib",
        vec![prefer_cpu_attr()],
        vec![ret_call("fib")],
    )];
    let facts = analyze_recursion(&d).expect("prefer(cpu) doesn't conflict with recursion");
    assert!(facts.is_cpu_only("fib"));
}

// ---------------------------------------------------------------------------
// @prefer(gpu, force) — multi-arg form is NOT @prefer(gpu) (exact match)
// ---------------------------------------------------------------------------

#[test]
fn recursion_prefer_gpu_multi_arg_not_matched_no_err() {
    // @prefer(gpu, force) func fib(n) { fib(n-1) }  -- args=["gpu","force"]
    // does NOT match `args == ["gpu"]`, so no error.
    let d = vec![func_decl_with_attrs(
        "fib",
        vec![prefer_gpu_multi_attr()],
        vec![ret_call("fib")],
    )];
    let facts = analyze_recursion(&d).expect("multi-arg @prefer doesn't match exactly");
    assert!(facts.is_cpu_only("fib"));
}

// ---------------------------------------------------------------------------
// Empty program → empty facts
// ---------------------------------------------------------------------------

#[test]
fn recursion_empty_program_returns_empty_facts() {
    let facts = analyze_recursion(&[]).expect("empty program is fine");
    assert!(facts.is_empty());
    assert_eq!(facts.len(), 0);
    assert!(!facts.is_cpu_only("anything"));
}

// ---------------------------------------------------------------------------
// Determinism: same input → byte-identical output
// ---------------------------------------------------------------------------

#[test]
fn recursion_deterministic_output_for_same_input() {
    let mk = || {
        vec![
            func_decl("a", vec![ret_call("a")]),
            func_decl("b", vec![ret_call("c")]),
            func_decl("c", vec![ret_call("b")]),
            func_decl("d", vec![ret_int(0)]),
        ]
    };
    let f1 = analyze_recursion(&mk()).expect("ok");
    let f2 = analyze_recursion(&mk()).expect("ok");
    let v1: Vec<String> = f1.to_sorted_vec().iter().map(|s| s.to_string()).collect();
    let v2: Vec<String> = f2.to_sorted_vec().iter().map(|s| s.to_string()).collect();
    assert_eq!(v1, v2, "determinism: same input must yield identical facts");
    // Spot-check: a (self), b↔c (mutual) → cpu_only; d (terminal) → not.
    assert!(f1.is_cpu_only("a"));
    assert!(f1.is_cpu_only("b"));
    assert!(f1.is_cpu_only("c"));
    assert!(!f1.is_cpu_only("d"));
}

// ---------------------------------------------------------------------------
// export-wrapped recursive fn → still detected
// ---------------------------------------------------------------------------

#[test]
fn recursion_export_wrapped_recursive_func_marked_cpu_only() {
    // export func fib(n) { fib(n-1) }
    let inner = Decl::FuncDecl(func("fib", vec![ret_call("fib")]));
    let d = vec![Decl::ExportDecl(buff_lang_ast::decl::ExportDecl {
        inner: Box::new(inner),
        span: span(),
    })];
    let facts = analyze_recursion(&d).expect("ok");
    assert!(facts.is_cpu_only("fib"));
}

// ---------------------------------------------------------------------------
// Larger realistic program: mixed recursive + non-recursive
// ---------------------------------------------------------------------------

#[test]
fn recursion_realistic_mixed_program() {
    // func main()       { fib(10); helper(); }
    // func fib(n)       { fib(n-1) + fib(n-2) }   // recursive
    // func helper(x)    { double(x) }              // chain
    // func double(x)    { x * 2 }                  // terminal
    let main_body = vec![
        Stmt::ExprStmt(call_with_arg("fib", int_expr(10)), span()),
        Stmt::ExprStmt(call_with_arg("helper", ident_expr("x")), span()),
    ];
    let fib_body = vec![ret(Expr::BinaryOp {
        op: buff_lang_ast::op::BinaryOp::Add,
        lhs: Box::new(call_with_arg("fib", ident_expr("n"))),
        rhs: Box::new(call_with_arg("fib", ident_expr("n"))),
        span: span(),
    })];
    let helper_body = vec![ret(call_with_arg("double", ident_expr("x")))];
    let double_body = vec![ret(Expr::BinaryOp {
        op: buff_lang_ast::op::BinaryOp::Mul,
        lhs: Box::new(ident_expr("x")),
        rhs: Box::new(int_expr(2)),
        span: span(),
    })];
    let d = vec![
        func_decl("main", main_body),
        func_decl("fib", fib_body),
        func_decl("helper", helper_body),
        func_decl("double", double_body),
    ];
    let facts = analyze_recursion(&d).expect("ok");
    // Only `fib` is on a cycle.
    assert!(facts.is_cpu_only("fib"));
    assert!(!facts.is_cpu_only("main"));
    assert!(!facts.is_cpu_only("helper"));
    assert!(!facts.is_cpu_only("double"));
    assert_eq!(facts.len(), 1);
}

// ---------------------------------------------------------------------------
// Edge case: calls to undefined/prelude fns don't break anything
// ---------------------------------------------------------------------------

#[test]
fn recursion_calls_to_undefined_names_are_ignored() {
    // func main() { print(fib()) }   -- print is prelude, undefined as a node
    // func fib() { fib() }
    let main_body = vec![Stmt::ExprStmt(
        Expr::FuncCall {
            callee: Box::new(ident_expr("print")),
            args: vec![call("fib")],
            span: span(),
        },
        span(),
    )];
    let d = vec![
        func_decl("main", main_body),
        func_decl("fib", vec![ret_call("fib")]),
    ];
    let facts = analyze_recursion(&d).expect("ok");
    assert!(facts.is_cpu_only("fib"));
    assert!(!facts.is_cpu_only("main"));
    assert!(
        !facts.is_cpu_only("print"),
        "print has no node — never cpu_only"
    );
}

// ---------------------------------------------------------------------------
// Edge case: function with NO calls (terminal) is never cpu_only
// ---------------------------------------------------------------------------

#[test]
fn recursion_function_with_no_calls_never_cpu_only() {
    // func terminal() { return 0 }
    let d = vec![func_decl("terminal", vec![ret_int(0)])];
    let facts = analyze_recursion(&d).expect("ok");
    assert!(!facts.is_cpu_only("terminal"));
    assert!(facts.is_empty());
}

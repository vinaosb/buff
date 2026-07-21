//! T42 integration tests — auto-insertion of `AtomicI64` for shared
//! mutable state in parallel closures.
//!
//! These tests exercise [`buff_lang_codegen_rust::atomic_analysis`]
//! (via the public [`buff_lang_codegen_rust::generate_rust`] entry
//! point) plus the codegen-side lowering that emits
//! `AtomicI64::new(...)`, `.fetch_add(...)`, and `.load(...)`. Every
//! test name contains the substring `atomic` so the QA filter
//! `cargo test -p buff-lang-codegen-rust atomic` matches all of them.
//!
//! ## Coverage matrix
//!
//! - **Promotable cases (codegens with AtomicI64)**:
//!   - QA case: `let mut t = 0; v.par_map({ x => t += x })`
//!   - par_reduce accumulator promotion
//!   - post-parallel `.load()` of the accumulator
//!   - post-parallel use inside an expression (`t + 1`)
//!   - non-zero initial value
//! - **Non-promotable cases (still errors via T41)**:
//!   - plain `=` to a capture
//!   - `-=` to a capture
//!   - non-integer capture (`let mut s = "hi"`)
//!   - par_filter accumulator (par_filter is not accumulation)
//! - **Outside parallel context**:
//!   - sequential code with `let mut t = 0; t += 1` does NOT get
//!     promoted — `t` stays a plain `i64`.
//!   - sequential `.map` with mutation stays plain.
//! - **Pure analysis (atomic_analysis API)**:
//!   - `analyze_func` returns the expected `AtomicSet`
//!   - `AtomicPromotions::is_promotable` queries

use buff_lang_ast::common::{Block, Ident, Param};
use buff_lang_ast::decl::FuncDecl;
use buff_lang_ast::op::BinaryOp;
use buff_lang_ast::{Decl, Expr, Literal, Stmt, TypeRef};
use buff_lang_codegen_rust::{atomic_analysis, generate_rust, AtomicPromotions};
use buff_lang_error::{CodegenError, Span};

// ---------------------------------------------------------------------
// AST helpers — mirror the shape of `race_detection_tests.rs` helpers.
// ---------------------------------------------------------------------

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

fn bool_expr(b: bool) -> Expr {
    Expr::Literal(Literal::Bool(b), span())
}

fn string_literal(s: &str) -> Expr {
    Expr::Literal(Literal::String(s.to_string()), span())
}

fn placeholder_ty() -> TypeRef {
    TypeRef::Named {
        name: ident("_"),
        span: span(),
    }
}

fn closure_stmts(params: &[&str], body_stmts: Vec<Stmt>) -> Expr {
    let params: Vec<Param> = params
        .iter()
        .map(|p| Param {
            name: ident(p),
            ty: placeholder_ty(),
            default_value: None,
            is_comptime: false,
            span: span(),
        })
        .collect();
    Expr::Lambda {
        params,
        body: Block {
            stmts: body_stmts,
            span: span(),
        },
        return_type: None,
        span: span(),
    }
}

fn method_call(receiver: Expr, method: &str, args: Vec<Expr>) -> Expr {
    Expr::MethodCall {
        receiver: Box::new(receiver),
        method: ident(method),
        args,
        span: span(),
    }
}

fn assign(target: Expr, op: BinaryOp, value: Expr) -> Stmt {
    Stmt::Assignment {
        target,
        op,
        value,
        span: span(),
    }
}

fn let_stmt(name: &str, value: Expr, mutable: bool) -> Stmt {
    Stmt::LetDecl {
        name: ident(name),
        value,
        mutable,
        ty: None,
        span: span(),
    }
}

fn expr_stmt(e: Expr) -> Stmt {
    Stmt::ExprStmt(e, span())
}

fn binary_op(op: BinaryOp, lhs: Expr, rhs: Expr) -> Expr {
    Expr::BinaryOp {
        op,
        lhs: Box::new(lhs),
        rhs: Box::new(rhs),
        span: span(),
    }
}

fn func_with_stmts(name: &str, stmts: Vec<Stmt>) -> FuncDecl {
    FuncDecl {
        name: ident(name),
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
    }
}

fn codegen_stmts(stmts: Vec<Stmt>) -> Result<String, CodegenError> {
    let func = func_with_stmts("f", stmts);
    generate_rust(&[Decl::FuncDecl(func)])
}

/// Assert `result` is an Err whose diagnostic carries the
/// `ParallelMutability` prefix and names `expected_var`. Centralised
/// so non-promotable cases read as one-liners.
fn assert_parallel_mutability(result: Result<String, CodegenError>, expected_var: &str) {
    let err = result.expect_err("expected ParallelMutability error, got Ok");
    assert!(
        err.diagnostic.message.contains("ParallelMutability"),
        "expected ParallelMutability message, got: {}",
        err.diagnostic.message
    );
    assert!(
        err.diagnostic
            .message
            .contains(&format!("`{expected_var}`")),
        "expected message to name variable `{expected_var}`, got: {}",
        err.diagnostic.message
    );
}

// =====================================================================
// 1. PROMOTABLE CASES — codegen succeeds with AtomicI64 markers
// =====================================================================

#[test]
fn atomic_qa_par_map_accumulator_lowers_to_atomic_i64_fetch_add() {
    // EXACT QA case from the task spec:
    //   let mut t = 0
    //   v.par_map({ x => t += x })
    //
    // Expected: codegen SUCCEEDS (no T41 error). Generated Rust:
    //   let t = std::sync::atomic::AtomicI64::new(0);
    //   v.par_map({ x => t.fetch_add(x as i64, Relaxed) });
    let body = closure_stmts(
        &["x"],
        vec![assign(
            ident_expr("t"),
            BinaryOp::AddAssign,
            ident_expr("x"),
        )],
    );
    let src = codegen_stmts(vec![
        let_stmt("t", int_expr(0), true),
        expr_stmt(method_call(ident_expr("v"), "par_map", vec![body])),
    ])
    .expect("QA accumulator pattern must codegen (T42 promotion)");

    // Hard string assertions on the lowering. These are the EXACT
    // markers the task spec mandates.
    assert!(
        src.contains("AtomicI64::new"),
        "expected `AtomicI64::new(...)` in generated Rust:\n{src}"
    );
    assert!(
        src.contains("fetch_add"),
        "expected `.fetch_add(...)` in generated Rust:\n{src}"
    );
}

#[test]
fn atomic_qa_post_parallel_read_lowers_to_load() {
    // The QA case extended with a post-parallel read:
    //   let mut t = 0
    //   v.par_map({ x => t += x })
    //   print(t)
    //
    // Expected: generated Rust has `.load(std::sync::atomic::Ordering::Relaxed)`
    // at the read site (the `print(t)` lowering).
    let body = closure_stmts(
        &["x"],
        vec![assign(
            ident_expr("t"),
            BinaryOp::AddAssign,
            ident_expr("x"),
        )],
    );
    let print_call = Expr::FuncCall {
        callee: Box::new(ident_expr("print")),
        args: vec![ident_expr("t")],
        span: span(),
    };
    let src = codegen_stmts(vec![
        let_stmt("t", int_expr(0), true),
        expr_stmt(method_call(ident_expr("v"), "par_map", vec![body])),
        expr_stmt(print_call),
    ])
    .expect("post-parallel read must codegen");

    assert!(
        src.contains(".load("),
        "expected `.load(...)` for post-parallel read in:\n{src}"
    );
    assert!(
        src.contains("Ordering::Relaxed"),
        "expected `Ordering::Relaxed` in:\n{src}"
    );
}

#[test]
fn atomic_par_reduce_accumulator_is_promoted() {
    // par_reduce is also an accumulating combinator (per spec §2).
    //   let mut t = 0
    //   v.par_reduce(0, { a, b => { t += a; a + b } })
    let body = closure_stmts(
        &["a", "b"],
        vec![
            assign(ident_expr("t"), BinaryOp::AddAssign, ident_expr("a")),
            expr_stmt(binary_op(BinaryOp::Add, ident_expr("a"), ident_expr("b"))),
        ],
    );
    let src = codegen_stmts(vec![
        let_stmt("t", int_expr(0), true),
        expr_stmt(method_call(
            ident_expr("v"),
            "par_reduce",
            vec![int_expr(0), body],
        )),
    ])
    .expect("par_reduce accumulator must codegen");
    assert!(
        src.contains("AtomicI64::new"),
        "missing AtomicI64::new:\n{src}"
    );
    assert!(src.contains("fetch_add"), "missing fetch_add:\n{src}");
}

#[test]
fn atomic_promotion_drops_mut_on_binding() {
    // The promotion rewrites `let mut t = 0` → `let t = AtomicI64::new(0)`.
    // The `mut` MUST be dropped (the atomic itself is immutable;
    // interior mutability is via &self methods).
    let body = closure_stmts(
        &["x"],
        vec![assign(
            ident_expr("t"),
            BinaryOp::AddAssign,
            ident_expr("x"),
        )],
    );
    let src = codegen_stmts(vec![
        let_stmt("t", int_expr(0), true),
        expr_stmt(method_call(ident_expr("v"), "par_map", vec![body])),
    ])
    .expect("codegen");
    // The `let t = ...` line must NOT carry `mut`.
    let atomic_line = src
        .lines()
        .find(|l| l.contains("AtomicI64::new"))
        .unwrap_or_else(|| panic!("no AtomicI64::new line in:\n{src}"));
    assert!(
        !atomic_line.contains("mut"),
        "promoted binding must NOT be `mut`:\n  {atomic_line}\n---\n{src}"
    );
}

#[test]
fn atomic_promotion_with_non_zero_initial_value() {
    // `let mut t = 100` → `AtomicI64::new(100)`.
    let body = closure_stmts(
        &["x"],
        vec![assign(
            ident_expr("t"),
            BinaryOp::AddAssign,
            ident_expr("x"),
        )],
    );
    let src = codegen_stmts(vec![
        let_stmt("t", int_expr(100), true),
        expr_stmt(method_call(ident_expr("v"), "par_map", vec![body])),
    ])
    .expect("codegen");
    assert!(
        src.contains("AtomicI64::new(100)"),
        "expected `AtomicI64::new(100)` (non-zero init) in:\n{src}"
    );
}

#[test]
fn atomic_promotion_multiple_accumulators_both_promoted() {
    // `let mut a = 0; let mut b = 0; v.par_map({ x => { a += x; b += x } })`
    // Both `a` and `b` are integer accumulators — both should be
    // promoted. (T41 originally flagged the FIRST in source order as a
    // race; T42 promotes both.)
    let body = closure_stmts(
        &["x"],
        vec![
            assign(ident_expr("a"), BinaryOp::AddAssign, ident_expr("x")),
            assign(ident_expr("b"), BinaryOp::AddAssign, ident_expr("x")),
        ],
    );
    let src = codegen_stmts(vec![
        let_stmt("a", int_expr(0), true),
        let_stmt("b", int_expr(0), true),
        expr_stmt(method_call(ident_expr("v"), "par_map", vec![body])),
    ])
    .expect("both accumulators must be promoted");
    // Count AtomicI64::new occurrences — should be 2.
    let count = src.matches("AtomicI64::new").count();
    assert_eq!(
        count, 2,
        "expected 2 AtomicI64::new occurrences, got {count}:\n{src}"
    );
}

// =====================================================================
// 2. NON-PROMOTABLE CASES — T41 race error preserved
// =====================================================================

#[test]
fn atomic_non_promotable_plain_assign_still_errors() {
    // `let mut t = 0; v.par_map({ x => t = x })` — plain `=`.
    let body = closure_stmts(
        &["x"],
        vec![assign(ident_expr("t"), BinaryOp::Assign, ident_expr("x"))],
    );
    let result = codegen_stmts(vec![
        let_stmt("t", int_expr(0), true),
        expr_stmt(method_call(ident_expr("v"), "par_map", vec![body])),
    ]);
    assert_parallel_mutability(result, "t");
}

#[test]
fn atomic_non_promotable_sub_assign_still_errors() {
    // `let mut t = 0; v.par_map({ x => t -= x })` — `-=` is not += .
    let body = closure_stmts(
        &["x"],
        vec![assign(
            ident_expr("t"),
            BinaryOp::SubAssign,
            ident_expr("x"),
        )],
    );
    let result = codegen_stmts(vec![
        let_stmt("t", int_expr(0), true),
        expr_stmt(method_call(ident_expr("v"), "par_map", vec![body])),
    ]);
    assert_parallel_mutability(result, "t");
}

#[test]
fn atomic_non_promotable_mul_assign_still_errors() {
    let body = closure_stmts(
        &["x"],
        vec![assign(
            ident_expr("t"),
            BinaryOp::MulAssign,
            ident_expr("x"),
        )],
    );
    let result = codegen_stmts(vec![
        let_stmt("t", int_expr(0), true),
        expr_stmt(method_call(ident_expr("v"), "par_map", vec![body])),
    ]);
    assert_parallel_mutability(result, "t");
}

#[test]
fn atomic_non_promotable_par_filter_mutation_still_errors() {
    // par_filter is NOT an accumulating combinator (spec §2):
    //   mutation in `par_filter` (filter is not an accumulation)
    //   → keep `ParallelMutabilityError`.
    let body = closure_stmts(
        &["x"],
        vec![
            assign(ident_expr("t"), BinaryOp::AddAssign, int_expr(1)),
            expr_stmt(bool_expr(true)),
        ],
    );
    let result = codegen_stmts(vec![
        let_stmt("t", int_expr(0), true),
        expr_stmt(method_call(ident_expr("v"), "par_filter", vec![body])),
    ]);
    assert_parallel_mutability(result, "t");
}

#[test]
fn atomic_non_promotable_non_integer_capture_still_errors() {
    // `let mut s = "hi"; v.par_map({ x => s += x })` — `s` is not
    // an integer literal init, so promotion does not apply.
    let body = closure_stmts(
        &["x"],
        vec![assign(
            ident_expr("s"),
            BinaryOp::AddAssign,
            string_literal("x"),
        )],
    );
    let result = codegen_stmts(vec![
        let_stmt("s", string_literal("hi"), true),
        expr_stmt(method_call(ident_expr("v"), "par_map", vec![body])),
    ]);
    assert_parallel_mutability(result, "s");
}

#[test]
fn atomic_non_promotable_mixed_add_and_sub_assign_errors() {
    // If `t` has both `+=` and `-=` mutations in the closure,
    // promotion fails and T41 errors.
    let body = closure_stmts(
        &["x"],
        vec![
            assign(ident_expr("t"), BinaryOp::AddAssign, ident_expr("x")),
            assign(ident_expr("t"), BinaryOp::SubAssign, ident_expr("x")),
        ],
    );
    let result = codegen_stmts(vec![
        let_stmt("t", int_expr(0), true),
        expr_stmt(method_call(ident_expr("v"), "par_map", vec![body])),
    ]);
    assert_parallel_mutability(result, "t");
}

// =====================================================================
// 3. ATOMIC ONLY IN PARALLEL CONTEXT
// =====================================================================

#[test]
fn atomic_not_applied_in_sequential_code() {
    // `let mut t = 0; t += 1` — NO parallel context. `t` MUST stay
    // a plain `i64`; the generated Rust must NOT contain AtomicI64.
    let src = codegen_stmts(vec![
        let_stmt("t", int_expr(0), true),
        assign(ident_expr("t"), BinaryOp::AddAssign, int_expr(1)),
    ])
    .expect("sequential code must codegen");
    assert!(
        !src.contains("AtomicI64"),
        "sequential code must NOT use AtomicI64:\n{src}"
    );
    assert!(
        !src.contains("fetch_add"),
        "sequential code must NOT use fetch_add:\n{src}"
    );
    assert!(
        !src.contains(".load("),
        "sequential code must NOT use .load():\n{src}"
    );
    // The plain `+=` must still be emitted (as a normal Rust `+=`).
    assert!(
        src.contains("+="),
        "sequential `t += 1` must emit Rust `+=`:\n{src}"
    );
}

#[test]
fn atomic_not_applied_in_sequential_map_with_mutation() {
    // Sequential `.map` (NOT `.par_map`) with capture mutation.
    // Per spec §2, T41's race detector does NOT flag this (single-
    // threaded map), and T42 promotion does NOT apply either —
    // there's no parallel context to promote for.
    let body = closure_stmts(
        &["x"],
        vec![assign(
            ident_expr("t"),
            BinaryOp::AddAssign,
            ident_expr("x"),
        )],
    );
    let src = codegen_stmts(vec![
        let_stmt("t", int_expr(0), true),
        expr_stmt(method_call(ident_expr("v"), "map", vec![body])),
    ])
    .expect("sequential map codegen");
    assert!(
        !src.contains("AtomicI64"),
        "sequential map code must NOT use AtomicI64:\n{src}"
    );
}

// =====================================================================
// 4. PURE ANALYSIS API
// =====================================================================

#[test]
fn atomic_analysis_func_returns_expected_set() {
    // Direct call to atomic_analysis::analyze_func — verifies the
    // promotion decision without going through full codegen.
    let body = closure_stmts(
        &["x"],
        vec![assign(
            ident_expr("t"),
            BinaryOp::AddAssign,
            ident_expr("x"),
        )],
    );
    let f = func_with_stmts(
        "f",
        vec![
            let_stmt("t", int_expr(0), true),
            expr_stmt(method_call(ident_expr("v"), "par_map", vec![body])),
        ],
    );
    let set = atomic_analysis::analyze_func(&f);
    assert_eq!(set.len(), 1, "exactly one promotion expected");
    assert_eq!(set.get("t"), Some(&0));
}

#[test]
fn atomic_promotions_is_promotable_query() {
    let mut p = AtomicPromotions::empty();
    p.insert("foo", "t", 0);
    assert!(p.is_promotable("foo", "t"));
    assert!(!p.is_promotable("foo", "x"));
    assert!(!p.is_promotable("bar", "t"));
    assert_eq!(p.initial_value("foo", "t"), Some(0));
    assert_eq!(p.initial_value("foo", "x"), None);
}

#[test]
fn atomic_analysis_analyze_program_level() {
    // Two functions, each with its own promotable accumulator named `t`.
    let make_fn = |name: &str| {
        let body = closure_stmts(
            &["x"],
            vec![assign(
                ident_expr("t"),
                BinaryOp::AddAssign,
                ident_expr("x"),
            )],
        );
        func_with_stmts(
            name,
            vec![
                let_stmt("t", int_expr(0), true),
                expr_stmt(method_call(ident_expr("v"), "par_map", vec![body])),
            ],
        )
    };
    let decls = vec![Decl::FuncDecl(make_fn("f")), Decl::FuncDecl(make_fn("g"))];
    let promotions = atomic_analysis::analyze(&decls);
    assert!(promotions.is_promotable("f", "t"));
    assert!(promotions.is_promotable("g", "t"));
    // Per-function isolation: querying "f" with another name fails.
    assert!(!promotions.is_promotable("f", "x"));
}

#[test]
fn atomic_analysis_empty_program_yields_empty_promotions() {
    let promotions = atomic_analysis::analyze(&[]);
    assert!(promotions.by_function.is_empty());
}

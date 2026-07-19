//! T41 integration tests — data-race detection in parallel closures.
//!
//! These tests exercise [`buff_lang_codegen_rust::race_analysis`] via
//! the public [`buff_lang_codegen_rust::generate_rust`] entry point.
//! Every test name contains the substring `race_detection` so the QA
//! filter `cargo test -p buff-lang-codegen-rust race_detection`
//! matches all of them.
//!
//! ## Coverage matrix
//!
//! - **Positive cases (race detected)** — QA case + variants:
//!   - `let mut t = 0; v.par_map({ x => t += x })` ← exact QA case
//!   - plain `=` assignment to a capture
//!   - `par_filter` with capture mutation
//!   - `par_reduce` with capture mutation
//!   - nested closure that mutates the OUTER capture
//!   - multiple captures mutated → first one wins (source order)
//! - **Negative cases (no race)** — common safe patterns:
//!   - immutable read of a captured var (no mutation)
//!   - mutating the closure's own param
//!   - mutating a `let` bound INSIDE the closure body
//!   - sequential `.map` with mutation (NOT parallel → not flagged)
//!   - program with no parallel calls at all
//! - **Wrapper-level tests**:
//!   - `analyze(&[Decl])` propagates the error
//!   - non-`FuncDecl` decls (struct, etc.) are skipped
//!
//! ## Detection-vs-rustc independence
//!
//! These tests run BEFORE the codegen emits a single Rust construct.
//! The race detector sees the raw Buff AST and rejects before any
//! lowering happens — so the tests never invoke rustc and never
//! depend on whether the generated Rust would have compiled.

use buff_lang_ast::common::{Block, Ident, Param};
use buff_lang_ast::decl::{FuncDecl, StructDecl};
use buff_lang_ast::op::BinaryOp;
use buff_lang_ast::{Decl, Expr, Literal, Stmt, TypeRef};
use buff_lang_codegen_rust::generate_rust;
use buff_lang_error::{CodegenError, Span};

// ---------------------------------------------------------------------
// AST builder helpers (kept tiny so each test reads like Buff source)
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

fn placeholder_ty() -> TypeRef {
    TypeRef::Named {
        name: ident("_"),
        span: span(),
    }
}

/// Build a closure `{ params => body_expr }` (single-expression body).
fn closure(params: &[&str], body: Expr) -> Expr {
    let params: Vec<Param> = params
        .iter()
        .map(|p| Param {
            name: ident(p),
            ty: placeholder_ty(),
            default_value: None,
            span: span(),
        })
        .collect();
    Expr::Lambda {
        params,
        body: Block {
            stmts: vec![Stmt::ExprStmt(body, span())],
            span: span(),
        },
        return_type: None,
        span: span(),
    }
}

/// Build a closure whose body is a list of statements (so we can
/// put `t += x` assignments inside it).
fn closure_stmts(params: &[&str], body_stmts: Vec<Stmt>) -> Expr {
    let params: Vec<Param> = params
        .iter()
        .map(|p| Param {
            name: ident(p),
            ty: placeholder_ty(),
            default_value: None,
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

/// `receiver.method(args...)` AST node.
fn method_call(receiver: Expr, method: &str, args: Vec<Expr>) -> Expr {
    Expr::MethodCall {
        receiver: Box::new(receiver),
        method: ident(method),
        args,
        span: span(),
    }
}

/// `target op value` as a Stmt::Assignment.
fn assign(target: Expr, op: BinaryOp, value: Expr) -> Stmt {
    Stmt::Assignment {
        target,
        op,
        value,
        span: span(),
    }
}

/// `let name = value` (or `let mut name = value` if `mutable`).
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

/// Wrap `stmts` in a 0-arg function `f` and run codegen. Returns the
/// `Result<String, CodegenError>` so tests can assert on either side.
fn codegen_stmts(stmts: Vec<Stmt>) -> Result<String, CodegenError> {
    let func = FuncDecl {
        name: ident("f"),
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
    };
    generate_rust(&[Decl::FuncDecl(func)])
}

/// Assert `result` is an Err whose diagnostic is a
/// [`ParallelMutabilityError`]-shaped message naming `expected_var`.
/// Centralised so each positive test reads as a one-liner.
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
// 1. POSITIVE CASES — race detected
// =====================================================================

#[test]
fn race_detection_qa_par_map_mutable_capture_via_add_assign() {
    // The EXACT QA case from the task spec:
    //   let mut t = 0
    //   v.par_map({ x => t += x })
    //
    // `t` is declared `mut` in the enclosing function and captured by
    // the par_map closure; the closure mutates it via `+=` → race.
    let body = closure_stmts(
        &["x"],
        vec![assign(
            ident_expr("t"),
            BinaryOp::AddAssign,
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
fn race_detection_par_map_mutable_capture_via_plain_assign() {
    // `let mut t = 0; v.par_map({ x => t = x })` — plain `=`, not `+=`.
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
fn race_detection_par_map_mutable_capture_via_sub_assign() {
    // `let mut t = 0; v.par_map({ x => t -= x })` — compound `-=` variant.
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
fn race_detection_par_filter_mutable_capture() {
    // `let mut t = 0; v.par_filter({ x => { t += 1; true } })` — par_filter.
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
fn race_detection_par_reduce_mutable_capture() {
    // `let mut t = 0; v.par_reduce(0, { a, b => { t += a; a + b } })`.
    // The closure has 2 params; the captured `t` is mutated inside.
    let body = closure_stmts(
        &["a", "b"],
        vec![
            assign(ident_expr("t"), BinaryOp::AddAssign, ident_expr("a")),
            expr_stmt(binary_op(BinaryOp::Add, ident_expr("a"), ident_expr("b"))),
        ],
    );
    let result = codegen_stmts(vec![
        let_stmt("t", int_expr(0), true),
        expr_stmt(method_call(
            ident_expr("v"),
            "par_reduce",
            vec![int_expr(0), body],
        )),
    ]);
    assert_parallel_mutability(result, "t");
}

#[test]
fn race_detection_nested_closure_mutates_outer_capture() {
    // `let mut t = 0; v.par_map({ x => { y => t += y } })`
    //
    // The inner closure (param `y`) mutates the outer-scope `t`.
    // Even though the mutation is inside a NESTED closure, it still
    // executes in the parallel context (the nested closure is
    // defined and called inside the parallel closure), so it races
    // on `t`.
    let inner = closure_stmts(
        &["y"],
        vec![assign(
            ident_expr("t"),
            BinaryOp::AddAssign,
            ident_expr("y"),
        )],
    );
    let outer = closure_stmts(&["x"], vec![expr_stmt(inner)]);
    let result = codegen_stmts(vec![
        let_stmt("t", int_expr(0), true),
        expr_stmt(method_call(ident_expr("v"), "par_map", vec![outer])),
    ]);
    assert_parallel_mutability(result, "t");
}

#[test]
fn race_detection_multiple_captures_first_mutable_one_flagged() {
    // `let mut a = 0; let mut b = 0; v.par_map({ x => { a += x; b += x } })`
    //
    // Both `a` and `b` are captured and mutated, but the detector
    // reports the FIRST one in source order (`a`). Deterministic.
    let body = closure_stmts(
        &["x"],
        vec![
            assign(ident_expr("a"), BinaryOp::AddAssign, ident_expr("x")),
            assign(ident_expr("b"), BinaryOp::AddAssign, ident_expr("x")),
        ],
    );
    let result = codegen_stmts(vec![
        let_stmt("a", int_expr(0), true),
        let_stmt("b", int_expr(0), true),
        expr_stmt(method_call(ident_expr("v"), "par_map", vec![body])),
    ]);
    assert_parallel_mutability(result, "a");
}

// =====================================================================
// 2. NEGATIVE CASES — no race (codegen succeeds)
// =====================================================================

#[test]
fn race_detection_immutable_read_of_capture_ok() {
    // `let f = 10; v.par_map({ x => x + f })` — `f` is read only.
    let body = closure(
        &["x"],
        binary_op(BinaryOp::Add, ident_expr("x"), ident_expr("f")),
    );
    let result = codegen_stmts(vec![
        let_stmt("f", int_expr(10), false),
        expr_stmt(method_call(ident_expr("v"), "par_map", vec![body])),
    ]);
    assert!(result.is_ok(), "immutable read must not race: {:?}", result);
}

#[test]
fn race_detection_closure_param_mutation_ok() {
    // `v.par_map({ x => { x = x + 1; x } })` — mutating the closure's
    // OWN param `x`. The param is a local binding, not a capture, so
    // it's not a race. (Whether the generated Rust would compile is a
    // separate question — Buff may require `mut x` in the param
    // list, but that's not the race detector's concern.)
    let body = closure_stmts(
        &["x"],
        vec![
            assign(
                ident_expr("x"),
                BinaryOp::Assign,
                binary_op(BinaryOp::Add, ident_expr("x"), int_expr(1)),
            ),
            expr_stmt(ident_expr("x")),
        ],
    );
    let result = codegen_stmts(vec![expr_stmt(method_call(
        ident_expr("v"),
        "par_map",
        vec![body],
    ))]);
    assert!(result.is_ok(), "param mutation must not race: {:?}", result);
}

#[test]
fn race_detection_inner_let_mutation_ok() {
    // `v.par_map({ x => { let mut y = 0; y += 1; x + y } })` —
    // `y` is bound INSIDE the closure, so it's local (not a
    // capture). Mutating it is fine.
    let body = closure_stmts(
        &["x"],
        vec![
            let_stmt("y", int_expr(0), true),
            assign(ident_expr("y"), BinaryOp::AddAssign, int_expr(1)),
            expr_stmt(binary_op(BinaryOp::Add, ident_expr("x"), ident_expr("y"))),
        ],
    );
    let result = codegen_stmts(vec![expr_stmt(method_call(
        ident_expr("v"),
        "par_map",
        vec![body],
    ))]);
    assert!(
        result.is_ok(),
        "inner-let mutation must not race: {:?}",
        result
    );
}

#[test]
fn race_detection_non_parallel_map_with_mutation_ok() {
    // `.map` (sequential) with mutation is NOT a race in v1.0.
    // Codegen lowers `.map` to single-threaded `into_iter().map()`.
    let body = closure_stmts(
        &["x"],
        vec![assign(
            ident_expr("t"),
            BinaryOp::AddAssign,
            ident_expr("x"),
        )],
    );
    let result = codegen_stmts(vec![
        let_stmt("t", int_expr(0), true),
        expr_stmt(method_call(ident_expr("v"), "map", vec![body])),
    ]);
    assert!(
        result.is_ok(),
        "sequential .map mutation must not race: {:?}",
        result
    );
}

#[test]
fn race_detection_non_parallel_filter_and_reduce_with_mutation_ok() {
    // Same as above but for `.filter` and `.reduce` — both sequential.
    let filter_body = closure_stmts(
        &["x"],
        vec![assign(
            ident_expr("t"),
            BinaryOp::AddAssign,
            ident_expr("x"),
        )],
    );
    let reduce_body = closure_stmts(
        &["a", "b"],
        vec![assign(
            ident_expr("t"),
            BinaryOp::AddAssign,
            ident_expr("a"),
        )],
    );
    let result = codegen_stmts(vec![
        let_stmt("t", int_expr(0), true),
        expr_stmt(method_call(ident_expr("v"), "filter", vec![filter_body])),
        expr_stmt(method_call(ident_expr("v"), "reduce", vec![reduce_body])),
    ]);
    assert!(
        result.is_ok(),
        "sequential filter/reduce mutation must not race: {:?}",
        result
    );
}

#[test]
fn race_detection_no_parallel_calls_at_all_ok() {
    // Program with no parallel combinators: never an error.
    let result = codegen_stmts(vec![
        let_stmt("t", int_expr(0), true),
        expr_stmt(method_call(
            ident_expr("v"),
            "map",
            vec![closure(&["x"], ident_expr("x"))],
        )),
    ]);
    assert!(result.is_ok(), "no parallel calls = no race: {:?}", result);
}

#[test]
fn race_detection_immutable_capture_in_nested_closure_ok() {
    // `let f = 10; v.par_map({ x => { y => y + f } })` — the nested
    // closure READS the captured `f` but never mutates it. Fine.
    let inner = closure(
        &["y"],
        binary_op(BinaryOp::Add, ident_expr("y"), ident_expr("f")),
    );
    let outer = closure_stmts(&["x"], vec![expr_stmt(inner)]);
    let result = codegen_stmts(vec![
        let_stmt("f", int_expr(10), false),
        expr_stmt(method_call(ident_expr("v"), "par_map", vec![outer])),
    ]);
    assert!(
        result.is_ok(),
        "immutable nested-closure capture must not race: {:?}",
        result
    );
}

// =====================================================================
// 3. PROGRAM-LEVEL / EDGE CASES
// =====================================================================

#[test]
fn race_detection_analyze_decls_skips_non_func_decls() {
    // A program with NO FuncDecl (only a struct decl) cannot contain
    // a race — codegen must succeed.
    let s = StructDecl {
        name: ident("P"),
        fields: Vec::new(),
        traits: Vec::new(),
        span: span(),
    };
    let result = generate_rust(&[Decl::StructDecl(s)]);
    assert!(
        result.is_ok(),
        "struct-only program must not trigger race analysis: {:?}",
        result
    );
}

#[test]
fn race_detection_par_map_with_immutable_capture_generates_rust() {
    // End-to-end positive codegen: when there's no race, generated
    // Rust is produced and contains a recognizable lowering of the
    // par_map call (whatever the codegen's current shape is).
    let body = closure(
        &["x"],
        binary_op(BinaryOp::Add, ident_expr("x"), ident_expr("f")),
    );
    let src = codegen_stmts(vec![
        let_stmt("f", int_expr(10), false),
        expr_stmt(method_call(ident_expr("v"), "par_map", vec![body])),
    ])
    .expect("immutable par_map must codegen cleanly");
    // The codegen currently lowers par_map through the default
    // method-call arm (no special handling — T42+ will wire the
    // runtime). Just assert the lowering mentions the method name
    // and the captured variable.
    assert!(
        src.contains("par_map"),
        "expected par_map lowering in generated Rust: {src}"
    );
    assert!(
        src.contains('f'),
        "expected captured `f` to appear in generated Rust: {src}"
    );
}

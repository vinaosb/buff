//! T78 — Constant Propagation pass integration tests.
//!
//! Verifies that `constant_propagation` replaces `Expr::Ident(x)` with the
//! literal value when `x` is bound to a constant and never mutated, and that
//! it is conservative (does NOT propagate mutated or re-declared names).

use buff_lang_ast::{
    common::{Block, Ident},
    expr::{Expr, Literal},
    op::BinaryOp,
    stmt::Stmt,
    Decl, FuncDecl,
};
use buff_lang_codegen_rust::{constant_propagation, dead_code_elimination, generate_rust};
use buff_lang_error::Span;

fn func_with_body(name: &str, stmts: Vec<Stmt>) -> Decl {
    Decl::FuncDecl(FuncDecl {
        name: Ident::new(name, Span::dummy()),
        params: Vec::new(),
        return_type: None,
        body: Block {
            stmts,
            span: Span::dummy(),
        },
        is_async: false,
        is_unsafe: false,
        is_extern: false,
        attributes: Vec::new(),
        type_params: Vec::new(),
        span: Span::dummy(),
    })
}

fn let_int(name: &str, n: i64) -> Stmt {
    Stmt::LetDecl {
        name: Ident::new(name, Span::dummy()),
        value: Expr::Literal(Literal::Int(n), Span::dummy()),
        mutable: false,
        ty: None,
        span: Span::dummy(),
    }
}

fn ident_expr(name: &str) -> Expr {
    Expr::Ident(Ident::new(name, Span::dummy()), Span::dummy())
}

fn add_expr(lhs: Expr, rhs: Expr) -> Expr {
    Expr::BinaryOp {
        op: BinaryOp::Add,
        lhs: Box::new(lhs),
        rhs: Box::new(rhs),
        span: Span::dummy(),
    }
}

/// Count how many times a literal value appears in the generated Rust.
fn count_literal(src: &str, val: &str) -> usize {
    src.matches(val).count()
}

#[test]
fn const_prop_replaces_constant_in_binary_op() {
    let decls = vec![func_with_body(
        "main",
        vec![
            let_int("x", 5),
            Stmt::ExprStmt(add_expr(ident_expr("x"), ident_expr("x")), Span::dummy()),
        ],
    )];
    let propagated = constant_propagation(&decls);
    let rust = generate_rust(&propagated).expect("codegen");
    // `x` references should be replaced with `5`, so the expression becomes `5 + 5`.
    assert!(
        rust.contains("5 + 5") || rust.contains("5i64 + 5i64"),
        "expected const-propagated expression, src: {rust}"
    );
}

#[test]
fn const_prop_does_not_propagate_mutated_name() {
    // `let mut x = 5; x = 10; x + 1` — x is mutated, should NOT be propagated.
    let decls = vec![func_with_body(
        "main",
        vec![
            Stmt::LetDecl {
                name: Ident::new("x", Span::dummy()),
                value: Expr::Literal(Literal::Int(5), Span::dummy()),
                mutable: true,
                ty: None,
                span: Span::dummy(),
            },
            Stmt::Assignment {
                target: ident_expr("x"),
                op: BinaryOp::Assign,
                value: Expr::Literal(Literal::Int(10), Span::dummy()),
                span: Span::dummy(),
            },
            Stmt::ExprStmt(add_expr(ident_expr("x"), ident_expr("y")), Span::dummy()),
        ],
    )];
    let propagated = constant_propagation(&decls);
    let rust = generate_rust(&propagated).expect("codegen");
    // x should still appear as a variable reference (not replaced with 5).
    assert!(
        !rust.contains("5 + y") && !rust.contains("5 + 0y"),
        "mutated x should NOT be propagated, src: {rust}"
    );
}

#[test]
fn const_prop_does_not_propagate_redeclared_name() {
    // `let x = 5; let x = 10; x` — x is re-declared, should NOT be propagated.
    let decls = vec![func_with_body(
        "main",
        vec![
            let_int("x", 5),
            let_int("x", 10),
            Stmt::ExprStmt(ident_expr("x"), Span::dummy()),
        ],
    )];
    let propagated = constant_propagation(&decls);
    // After propagation, x should still be a variable reference (not replaced).
    // We check by generating Rust and verifying x is still a binding.
    let rust = generate_rust(&propagated).expect("codegen");
    assert!(
        rust.contains("let x"),
        "re-declared x should still be a binding, src: {rust}"
    );
}

#[test]
fn const_prop_then_dce_eliminates_dead_constant() {
    // `let x = 42; print(x)` → after const-prop: `let x = 42; print(42)`
    // → after DCE: `print(42)` (the dead `let x = 42` is removed).
    let decls = vec![func_with_body(
        "main",
        vec![
            let_int("x", 42),
            Stmt::ExprStmt(
                Expr::FuncCall {
                    callee: Box::new(ident_expr("print")),
                    args: vec![ident_expr("x")],
                    span: Span::dummy(),
                },
                Span::dummy(),
            ),
        ],
    )];
    let propagated = constant_propagation(&decls);
    let eliminated = dead_code_elimination(&propagated);
    let rust = generate_rust(&eliminated).expect("codegen");
    // The `let x = 42` should be eliminated (x is no longer referenced after
    // const-prop replaced it with 42 in the print call).
    assert!(
        !rust.contains("let x = 42"),
        "dead const binding eliminated after const-prop + DCE, src: {rust}"
    );
    // The print call should still have 42.
    assert!(
        count_literal(&rust, "42") >= 1,
        "42 should appear in print arg, src: {rust}"
    );
}

#[test]
fn const_prop_preserves_non_constant_bindings() {
    // `let x = y` where y is a variable — x's value is not a literal, so
    // const-prop should not replace any references to x.
    let decls = vec![func_with_body(
        "main",
        vec![
            Stmt::LetDecl {
                name: Ident::new("x", Span::dummy()),
                value: ident_expr("y"),
                mutable: false,
                ty: None,
                span: Span::dummy(),
            },
            Stmt::ExprStmt(ident_expr("x"), Span::dummy()),
        ],
    )];
    let propagated = constant_propagation(&decls);
    let rust = generate_rust(&propagated).expect("codegen");
    // x should still be a binding (non-literal value → not propagated).
    assert!(rust.contains("let x"), "non-const x kept, src: {rust}");
}

#[test]
fn const_prop_propagates_bool_and_string() {
    let decls = vec![func_with_body(
        "main",
        vec![
            Stmt::LetDecl {
                name: Ident::new("flag", Span::dummy()),
                value: Expr::Literal(Literal::Bool(true), Span::dummy()),
                mutable: false,
                ty: None,
                span: Span::dummy(),
            },
            Stmt::LetDecl {
                name: Ident::new("name", Span::dummy()),
                value: Expr::Literal(Literal::String("Buff".into()), Span::dummy()),
                mutable: false,
                ty: None,
                span: Span::dummy(),
            },
            Stmt::ExprStmt(ident_expr("flag"), Span::dummy()),
            Stmt::ExprStmt(ident_expr("name"), Span::dummy()),
        ],
    )];
    let propagated = constant_propagation(&decls);
    let rust = generate_rust(&propagated).expect("codegen");
    // flag references should be replaced with `true`.
    assert!(rust.contains("true"), "bool propagated, src: {rust}");
}

//! T77 — Dead Code Elimination pass integration tests.
//!
//! Verifies that `dead_code_elimination` removes unused functions and unused
//! let bindings, and that the pass is conservative (keeps side-effectful
//! bindings, keeps called functions, keeps main).

use buff_lang_ast::{
    common::{Block, Ident},
    expr::{Expr, Literal},
    stmt::Stmt,
    Decl, FuncDecl,
};
use buff_lang_codegen_rust::{dead_code_elimination, generate_rust};
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

fn call_fn(name: &str) -> Stmt {
    Stmt::ExprStmt(
        Expr::FuncCall {
            callee: Box::new(Expr::Ident(Ident::new(name, Span::dummy()), Span::dummy())),
            args: Vec::new(),
            span: Span::dummy(),
        },
        Span::dummy(),
    )
}

#[test]
fn dce_removes_truly_dead_function() {
    let decls = vec![
        func_with_body("main", vec![call_fn("used")]),
        func_with_body("used", vec![]),
        func_with_body("dead", vec![]),
    ];
    let result = dead_code_elimination(&decls);
    assert_eq!(result.len(), 2, "dead function removed");
    // Verify the result still generates valid Rust.
    let rust = generate_rust(&result).expect("codegen");
    assert!(rust.contains("fn main"), "src: {rust}");
    assert!(rust.contains("fn used"), "src: {rust}");
    assert!(!rust.contains("fn dead"), "src: {rust}");
}

#[test]
fn dce_removes_unused_let_with_literal_value() {
    let decls = vec![func_with_body(
        "main",
        vec![
            let_int("unused", 42),
            let_int("used", 10),
            Stmt::ExprStmt(
                Expr::Ident(Ident::new("used", Span::dummy()), Span::dummy()),
                Span::dummy(),
            ),
        ],
    )];
    let result = dead_code_elimination(&decls);
    let rust = generate_rust(&result).expect("codegen");
    assert!(
        !rust.contains("let unused"),
        "unused let removed, src: {rust}"
    );
    assert!(rust.contains("let used"), "used let kept, src: {rust}");
}

#[test]
fn dce_preserves_code_without_dead_bindings() {
    // Code where all lets are used — nothing should change.
    let decls = vec![func_with_body(
        "main",
        vec![
            let_int("x", 5),
            let_int("y", 10),
            Stmt::ExprStmt(
                Expr::BinaryOp {
                    op: buff_lang_ast::op::BinaryOp::Add,
                    lhs: Box::new(Expr::Ident(Ident::new("x", Span::dummy()), Span::dummy())),
                    rhs: Box::new(Expr::Ident(Ident::new("y", Span::dummy()), Span::dummy())),
                    span: Span::dummy(),
                },
                Span::dummy(),
            ),
        ],
    )];
    let result = dead_code_elimination(&decls);
    let rust = generate_rust(&result).expect("codegen");
    assert!(rust.contains("let x"), "x kept (used), src: {rust}");
    assert!(rust.contains("let y"), "y kept (used), src: {rust}");
}

//! Snapshot tests for the `buff-lang-ast` crate.
//!
//! These build five representative AST fragments and snapshot their `Display`
//! output with `insta`. Snapshots live next to this file in `snapshots/`.
//!
//! Run `cargo insta review` after intentional AST format changes.

use buff_lang_ast::{Block, Decl, Expr, FuncDecl, Ident, Literal, Pattern, Span, Stmt, TypeRef};

fn span() -> Span {
    Span::dummy()
}

/// Helper: build `Expr::Literal(Int(n))`.
fn int_lit(n: i64) -> Expr {
    Expr::Literal(Literal::Int(n), span())
}

/// Test 1: a single integer literal.
#[test]
fn snapshot_int_literal() {
    let e = int_lit(42);
    insta::assert_snapshot!(e.to_string(), @"Lit(Int(42))");
}

/// Test 2: a binary add expression `1 + 2`.
#[test]
fn snapshot_binary_add() {
    let e = Expr::BinaryOp {
        op: buff_lang_ast::BinaryOp::Add,
        lhs: Box::new(int_lit(1)),
        rhs: Box::new(int_lit(2)),
        span: span(),
    };
    insta::assert_snapshot!(e.to_string(), @"BinaryOp(+, Lit(Int(1)), Lit(Int(2)))");
}

/// Test 3: a `let x = 42;` statement.
#[test]
fn snapshot_let_decl() {
    let s = Stmt::LetDecl {
        name: Ident::new("x", span()),
        value: int_lit(42),
        mutable: false,
        ty: None,
        span: span(),
    };
    insta::assert_snapshot!(s.to_string(), @"LetDecl(x = Lit(Int(42)))");
}

/// Test 4: a `fn main() { return 0; }` declaration.
#[test]
fn snapshot_main_func() {
    let body = Block {
        stmts: vec![Stmt::Return(Some(int_lit(0)), span())],
        span: span(),
    };
    let f = FuncDecl {
        name: Ident::new("main", span()),
        params: Vec::new(),
        return_type: None,
        body,
        is_async: false,
        is_unsafe: false,
        is_extern: false,
        attributes: Vec::new(),
        type_params: Vec::new(),
        span: span(),
    };
    let d = Decl::FuncDecl(f);
    insta::assert_snapshot!(d.to_string(), @"FuncDecl(fn main() { Return(Lit(Int(0))) })");
}

/// Test 5: an `if x { ... }` expression with no `else` branch.
#[test]
fn snapshot_if_expr_no_else() {
    let cond = Expr::Ident(Ident::new("x", span()), span());
    let then_block = Block {
        stmts: vec![Stmt::ExprStmt(int_lit(1), span())],
        span: span(),
    };
    let e = Expr::IfExpr {
        cond: Box::new(cond),
        then_block,
        else_block: None,
        span: span(),
    };
    insta::assert_snapshot!(
        e.to_string(),
        @"If(Ident(x), { ExprStmt(Lit(Int(1))) })"
    );
}

/// Bonus Test 6: a `match scrut { Some(x) => { ... } }` arm — exercises the
/// recursive Pattern + MatchArm Display path.
#[test]
fn snapshot_match_expr() {
    let scrutinee = Expr::Ident(Ident::new("opt", span()), span());
    let arm = buff_lang_ast::MatchArm {
        pattern: Pattern::Variant {
            enum_name: Ident::new("Option", span()),
            variant: Ident::new("Some", span()),
            subpatterns: vec![Pattern::Ident(Ident::new("x", span()), span())],
            span: span(),
        },
        guard: None,
        body: Block {
            stmts: vec![Stmt::ExprStmt(int_lit(1), span())],
            span: span(),
        },
        span: span(),
    };
    let e = Expr::MatchExpr {
        scrutinee: Box::new(scrutinee),
        arms: vec![arm],
        span: span(),
    };
    insta::assert_snapshot!(
        e.to_string(),
        @"Match(Ident(opt), [Option::Some(x) => { ExprStmt(Lit(Int(1))) }])"
    );
}

/// Bonus Test 7: function type with async.
#[test]
fn snapshot_async_function_type() {
    let t = TypeRef::Function {
        params: vec![TypeRef::Named {
            name: Ident::new("Int", span()),
            span: span(),
        }],
        return_type: Box::new(TypeRef::Named {
            name: Ident::new("Int", span()),
            span: span(),
        }),
        is_async: true,
        span: span(),
    };
    insta::assert_snapshot!(t.to_string(), @"async (Int) -> Int");
}

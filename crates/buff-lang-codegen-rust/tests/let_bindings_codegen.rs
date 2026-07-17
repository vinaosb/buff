//! T72 integration tests — codegen for if-let / for-let pattern bindings.
//!
//! Each test hand-builds an `Expr::IfLet` or `Stmt::ForLet`, runs it through
//! [`buff_lang_codegen_rust::generate_rust`], and asserts the resulting Rust
//! source contains the expected `if let` / `while let` syntax AND re-parses
//! as valid Rust (so a bad codegen shape is caught early).
//!
//! Run via:
//!
//! ```text
//! cargo test -p buff-lang-codegen-rust let_bindings
//! ```

use buff_lang_ast::common::{Block, Ident, Param};
use buff_lang_ast::decl::FuncDecl;
use buff_lang_ast::{Decl, Expr, Literal, Pattern, Stmt, TypeRef};
use buff_lang_error::Span;

use buff_lang_codegen_rust::generate_rust;

fn span() -> Span {
    Span::dummy()
}

fn ident(s: &str) -> Ident {
    Ident::new(s, span())
}

fn ident_expr(s: &str) -> Expr {
    Expr::Ident(ident(s), span())
}

/// Build the `Some(x)` variant-pattern used by most tests.
fn some_x_pattern() -> Pattern {
    Pattern::Variant {
        enum_name: ident("Option"),
        variant: ident("Some"),
        subpatterns: vec![Pattern::Ident(ident("x"), span())],
        span: span(),
    }
}

/// Wrap a list of statements in a zero-arg `fn f() -> Void { ... }` declaration.
fn func_with_stmts(stmts: Vec<Stmt>) -> Decl {
    Decl::FuncDecl(FuncDecl {
        name: ident("f"),
        params: Vec::<Param>::new(),
        return_type: Some(TypeRef::Named {
            name: ident("Void"),
            span: span(),
        }),
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

/// Assert the generated source re-parses as a valid Rust file.
fn must_reparse(src: &str) {
    syn::parse_str::<syn::File>(src)
        .unwrap_or_else(|e| panic!("generated source must re-parse: {e}\n--- src ---\n{src}"));
}

// ---------------------------------------------------------------------------
// if let codegen.
// ---------------------------------------------------------------------------

#[test]
fn let_bindings_codegen_if_let_some() {
    // `if let Some(x) = opt { print(x) }` → Rust `if let Some(x) = opt { ... }`.
    let then_block = Block {
        stmts: vec![Stmt::ExprStmt(
            Expr::FuncCall {
                callee: Box::new(ident_expr("print")),
                args: vec![ident_expr("x")],
                span: span(),
            },
            span(),
        )],
        span: span(),
    };
    let expr = Expr::IfLet {
        pattern: some_x_pattern(),
        value: Box::new(ident_expr("opt")),
        then_block,
        else_block: None,
        span: span(),
    };
    let stmt = Stmt::ExprStmt(expr, span());
    let src = generate_rust(&[func_with_stmts(vec![stmt])]).expect("codegen must succeed");
    assert!(
        src.contains("if let Some(x) = opt"),
        "expected Rust `if let Some(x) = opt`, got:\n{src}"
    );
    must_reparse(&src);
}

#[test]
fn let_bindings_codegen_if_let_with_else() {
    // `if let Some(x) = opt { print(x) } else { print("none") }` — the else
    // block lowers to a Rust `else { ... }` arm.
    let then_block = Block {
        stmts: vec![Stmt::ExprStmt(
            Expr::FuncCall {
                callee: Box::new(ident_expr("print")),
                args: vec![ident_expr("x")],
                span: span(),
            },
            span(),
        )],
        span: span(),
    };
    let else_block = Block {
        stmts: vec![Stmt::ExprStmt(
            Expr::FuncCall {
                callee: Box::new(ident_expr("print")),
                args: vec![Expr::Literal(Literal::String("none".to_string()), span())],
                span: span(),
            },
            span(),
        )],
        span: span(),
    };
    let expr = Expr::IfLet {
        pattern: some_x_pattern(),
        value: Box::new(ident_expr("opt")),
        then_block,
        else_block: Some(else_block),
        span: span(),
    };
    let stmt = Stmt::ExprStmt(expr, span());
    let src = generate_rust(&[func_with_stmts(vec![stmt])]).expect("codegen must succeed");
    assert!(
        src.contains("if let Some(x) = opt"),
        "expected `if let Some(x) = opt`, got:\n{src}"
    );
    assert!(src.contains("else"), "expected an `else` arm, got:\n{src}");
    must_reparse(&src);
}

#[test]
fn let_bindings_codegen_if_let_wildcard_pattern() {
    // `if let _ = opt { print("matched") }` → Rust `if let _ = opt { ... }`.
    let then_block = Block {
        stmts: vec![Stmt::ExprStmt(
            Expr::FuncCall {
                callee: Box::new(ident_expr("print")),
                args: vec![Expr::Literal(
                    Literal::String("matched".to_string()),
                    span(),
                )],
                span: span(),
            },
            span(),
        )],
        span: span(),
    };
    let expr = Expr::IfLet {
        pattern: Pattern::Wildcard(span()),
        value: Box::new(ident_expr("opt")),
        then_block,
        else_block: None,
        span: span(),
    };
    let stmt = Stmt::ExprStmt(expr, span());
    let src = generate_rust(&[func_with_stmts(vec![stmt])]).expect("codegen must succeed");
    assert!(
        src.contains("if let _ = opt"),
        "expected `if let _ = opt`, got:\n{src}"
    );
    must_reparse(&src);
}

// ---------------------------------------------------------------------------
// for let codegen (lowers to Rust `while let`).
// ---------------------------------------------------------------------------

#[test]
fn let_bindings_codegen_for_let_while_let() {
    // `for let Some(x) = iter.next() { print(x) }` → Rust
    // `while let Some(x) = iter.next() { ... }`.
    let body = Block {
        stmts: vec![Stmt::ExprStmt(
            Expr::FuncCall {
                callee: Box::new(ident_expr("print")),
                args: vec![ident_expr("x")],
                span: span(),
            },
            span(),
        )],
        span: span(),
    };
    let stmt = Stmt::ForLet {
        pattern: some_x_pattern(),
        value: Expr::MethodCall {
            receiver: Box::new(ident_expr("iter")),
            method: ident("next"),
            args: Vec::new(),
            span: span(),
        },
        body,
        span: span(),
    };
    let src = generate_rust(&[func_with_stmts(vec![stmt])]).expect("codegen must succeed");
    assert!(
        src.contains("while let Some(x) ="),
        "expected Rust `while let Some(x) =`, got:\n{src}"
    );
    assert!(
        src.contains("iter.next()"),
        "expected `iter.next()` value, got:\n{src}"
    );
    must_reparse(&src);
}

#[test]
fn let_bindings_codegen_for_let_wildcard_pattern() {
    // `for let _ = stream.poll() { count = count + 1 }` — degenerate but
    // structurally valid; lowers to `while let _ = stream.poll() { ... }`.
    let body = Block {
        stmts: vec![Stmt::Assignment {
            target: ident_expr("count"),
            op: buff_lang_ast::op::BinaryOp::Assign,
            value: Expr::BinaryOp {
                op: buff_lang_ast::op::BinaryOp::Add,
                lhs: Box::new(ident_expr("count")),
                rhs: Box::new(Expr::Literal(Literal::Int(1), span())),
                span: span(),
            },
            span: span(),
        }],
        span: span(),
    };
    let stmt = Stmt::ForLet {
        pattern: Pattern::Wildcard(span()),
        value: Expr::MethodCall {
            receiver: Box::new(ident_expr("stream")),
            method: ident("poll"),
            args: Vec::new(),
            span: span(),
        },
        body,
        span: span(),
    };
    let src = generate_rust(&[func_with_stmts(vec![stmt])]).expect("codegen must succeed");
    assert!(
        src.contains("while let _ ="),
        "expected `while let _ =`, got:\n{src}"
    );
    must_reparse(&src);
}

//! T71 integration tests — codegen for destructuring `let` bindings.
//!
//! Each test hand-builds a `Stmt::LetPattern`, runs it through
//! [`buff_lang_codegen_rust::generate_rust`], and asserts the resulting Rust
//! source contains the expected destructuring syntax.
//!
//! Run via:
//!
//! ```text
//! cargo test -p buff-lang-codegen-rust --test destructuring_codegen
//! ```

use buff_lang_ast::common::{Block, Ident, Param};
use buff_lang_ast::decl::FuncDecl;
use buff_lang_ast::{Decl, Expr, Pattern, Stmt, TypeRef};
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

/// Wrap a list of statements in a zero-arg `fn f() { ... }` declaration.
fn func_with_stmts(stmts: Vec<Stmt>) -> Decl {
    Decl::FuncDecl(FuncDecl { name: ident("f"),
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
    is_extern: false, attributes: Vec::new(), type_params: Vec::new(), span: span(), })
}

// ---------------------------------------------------------------------------
// Tuple destructuring codegen.
// ---------------------------------------------------------------------------

#[test]
fn destructuring_codegen_tuple() {
    // `let (a, b) = pair` → Rust `let (a, b) = pair;`.
    let stmt = Stmt::LetPattern {
        pattern: Pattern::Tuple(
            vec![
                Pattern::Ident(ident("a"), span()),
                Pattern::Ident(ident("b"), span()),
            ],
            span(),
        ),
        value: ident_expr("pair"),
        mutable: false,
        ty: None,
        span: span(),
    };
    let src = generate_rust(&[func_with_stmts(vec![stmt])]).expect("codegen must succeed");
    assert!(
        src.contains("let (a, b) = pair;"),
        "expected Rust tuple destructuring, got:\n{src}"
    );
}

#[test]
fn destructuring_codegen_tuple_with_wildcard() {
    // `let (a, _) = pair` → Rust `let (a, _) = pair;`.
    let stmt = Stmt::LetPattern {
        pattern: Pattern::Tuple(
            vec![
                Pattern::Ident(ident("a"), span()),
                Pattern::Wildcard(span()),
            ],
            span(),
        ),
        value: ident_expr("pair"),
        mutable: false,
        ty: None,
        span: span(),
    };
    let src = generate_rust(&[func_with_stmts(vec![stmt])]).expect("codegen must succeed");
    assert!(
        src.contains("let (a, _) ="),
        "expected wildcard in tuple destructuring, got:\n{src}"
    );
}

// ---------------------------------------------------------------------------
// Struct destructuring codegen.
// ---------------------------------------------------------------------------

#[test]
fn destructuring_codegen_struct_shorthand() {
    // `let Point { x, y } = p` → Rust `let Point { x, y } = p;`
    // (shorthand reproduced because field name == binding name).
    let stmt = Stmt::LetPattern {
        pattern: Pattern::Struct {
            name: ident("Point"),
            fields: vec![
                (ident("x"), Pattern::Ident(ident("x"), span())),
                (ident("y"), Pattern::Ident(ident("y"), span())),
            ],
            span: span(),
        },
        value: ident_expr("p"),
        mutable: false,
        ty: None,
        span: span(),
    };
    let src = generate_rust(&[func_with_stmts(vec![stmt])]).expect("codegen must succeed");
    assert!(
        src.contains("let Point { x, y } = p;"),
        "expected Rust struct destructuring shorthand, got:\n{src}"
    );
}

#[test]
fn destructuring_codegen_struct_explicit_field() {
    // `let Point { x: a, y: b } = p` → Rust `let Point { x: a, y: b } = p;`.
    let stmt = Stmt::LetPattern {
        pattern: Pattern::Struct {
            name: ident("Point"),
            fields: vec![
                (ident("x"), Pattern::Ident(ident("a"), span())),
                (ident("y"), Pattern::Ident(ident("b"), span())),
            ],
            span: span(),
        },
        value: ident_expr("p"),
        mutable: false,
        ty: None,
        span: span(),
    };
    let src = generate_rust(&[func_with_stmts(vec![stmt])]).expect("codegen must succeed");
    assert!(
        src.contains("let Point { x: a, y: b } = p;"),
        "expected Rust struct destructuring, got:\n{src}"
    );
}

#[test]
fn destructuring_codegen_mutable_tuple() {
    // `let mut (a, b) = pair` → each binding mutable: `let (mut a, mut b) = pair;`.
    let stmt = Stmt::LetPattern {
        pattern: Pattern::Tuple(
            vec![
                Pattern::Ident(ident("a"), span()),
                Pattern::Ident(ident("b"), span()),
            ],
            span(),
        ),
        value: ident_expr("pair"),
        mutable: true,
        ty: None,
        span: span(),
    };
    let src = generate_rust(&[func_with_stmts(vec![stmt])]).expect("codegen must succeed");
    assert!(
        src.contains("let (mut a, mut b) ="),
        "expected mutable bindings, got:\n{src}"
    );
}

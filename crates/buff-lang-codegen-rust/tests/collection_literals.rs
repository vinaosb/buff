//! T67 acceptance tests — Collection literals.
//!
//! Verifies that `[1, 2, 3]` → `vec![1, 2, 3]` and `{"k": v}` →
//! `std::collections::HashMap::from([("k", v)])` codegen works end-to-end.
//!
//! The functionality was already implemented by T23 (ArrayLit) and T25
//! (MapLit); this file adds the acceptance-test name `collection_literals`
//! so the T67 acceptance command runs ≥3 tests.
//!
//! Run via:
//!
//! ```text
//! cargo test -p buff-lang-codegen-rust collection_literals
//! ```

use buff_lang_ast::common::{Block, Ident};
use buff_lang_ast::decl::FuncDecl;
use buff_lang_ast::{Decl, Expr, Literal, Stmt};
use buff_lang_error::Span;

use buff_lang_codegen_rust::generate_rust;

fn span() -> Span {
    Span::dummy()
}

fn ident(s: &str) -> Ident {
    Ident::new(s, span())
}

fn int_expr(n: i64) -> Expr {
    Expr::Literal(Literal::Int(n), span())
}

fn string_expr(s: &str) -> Expr {
    Expr::Literal(Literal::String(s.to_string()), span())
}

/// Build an `Expr::ArrayLit` from a list of element expressions.
fn array_lit(elements: Vec<Expr>) -> Expr {
    Expr::ArrayLit {
        elements,
        span: span(),
    }
}

/// Build an `Expr::MapLit` from a list of `(key, value)` pairs.
fn map_lit(entries: Vec<(Expr, Expr)>) -> Expr {
    Expr::MapLit {
        entries,
        span: span(),
    }
}

/// Wrap a list of statements in a no-arg function called `f`.
fn codegen_stmts(stmts: Vec<Stmt>) -> String {
    let func = FuncDecl { name: ident("f"),
    params: Vec::new(),
    return_type: None,
    body: Block {
        stmts,
        span: span(),
    },
    is_async: false,
    is_unsafe: false,
    is_extern: false, attributes: Vec::new(), type_params: Vec::new(), span: span(), };
    generate_rust(&[Decl::FuncDecl(func)]).expect("codegen must succeed")
}

/// Like [`codegen_stmts`] but emits a single expression statement.
fn codegen_one_expr(expr: Expr) -> String {
    codegen_stmts(vec![Stmt::ExprStmt(expr, span())])
}

/// Assert the generated source re-parses as a valid Rust file.
fn must_reparse(src: &str) {
    syn::parse_str::<syn::File>(src)
        .unwrap_or_else(|e| panic!("generated source must re-parse: {e}\n--- src ---\n{src}"));
}

// ---------------------------------------------------------------------------
// 1. Array literal `[1, 2, 3]` -> `vec![1, 2, 3]`
// ---------------------------------------------------------------------------

#[test]
fn collection_literals_array_ints() {
    let src = codegen_one_expr(array_lit(vec![int_expr(1), int_expr(2), int_expr(3)]));
    assert!(
        src.contains("vec![1, 2, 3]"),
        "expected `vec![1, 2, 3]` in: {src}"
    );
    must_reparse(&src);
}

#[test]
fn collection_literals_empty_array() {
    let src = codegen_one_expr(array_lit(vec![]));
    assert!(src.contains("vec![]"), "expected `vec![]` in: {src}");
    must_reparse(&src);
}

// ---------------------------------------------------------------------------
// 2. Map literal `{"k": v}` -> `HashMap::from([("k", v)])`
// ---------------------------------------------------------------------------

#[test]
fn collection_literals_map_string_key() {
    let src = codegen_one_expr(map_lit(vec![(string_expr("k"), int_expr(42))]));
    assert!(
        src.contains("std::collections::HashMap::from([(\"k\", 42)])"),
        "expected `HashMap::from([(\"k\", 42)])` in: {src}"
    );
    must_reparse(&src);
}

#[test]
fn collection_literals_empty_map() {
    let src = codegen_one_expr(map_lit(vec![]));
    assert!(
        src.contains("std::collections::HashMap::from([])"),
        "expected `HashMap::from([])` for empty map in: {src}"
    );
    must_reparse(&src);
}

// ---------------------------------------------------------------------------
// 3. Multi-entry map literal
// ---------------------------------------------------------------------------

#[test]
fn collection_literals_map_multi_entry() {
    let src = codegen_one_expr(map_lit(vec![
        (string_expr("name"), string_expr("Alice")),
        (string_expr("age"), int_expr(30)),
    ]));
    assert!(
        src.contains("std::collections::HashMap::from(["),
        "expected `HashMap::from([` prefix in: {src}"
    );
    assert!(
        src.contains("(\"name\", \"Alice\")"),
        "expected tuple `(\"name\", \"Alice\")` in: {src}"
    );
    assert!(
        src.contains("(\"age\", 30)"),
        "expected tuple `(\"age\", 30)` in: {src}"
    );
    must_reparse(&src);
}

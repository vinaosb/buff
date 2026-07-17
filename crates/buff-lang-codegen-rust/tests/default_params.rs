//! T106 integration tests — Codegen for default parameter values.
//!
//! Verifies that a call omitting trailing defaulted params has the default
//! expressions FILLED into the call site (Rust has no native default-param
//! support, so the expansion happens at codegen). The canonical QA case is
//! `fetch("url")` where `fetch(url, timeout = 30)` → `fetch("url", 30)`.
//!
//! Also covers: all-params-supplied (no fill), multiple defaults, partial
//! omission, named-arg + default interaction, and no-fill regressions
//! (callee with no defaults / unknown callee).
//!
//! Run via:
//!
//! ```text
//! cargo test -p buff-lang-codegen-rust default_params
//! ```

use buff_lang_ast::common::{Block, Ident, Param};
use buff_lang_ast::decl::FuncDecl;
use buff_lang_ast::{Decl, Expr, Literal, Stmt, TypeRef};
use buff_lang_codegen_rust::generate_rust;
use buff_lang_error::Span;

fn span() -> Span {
    Span::dummy()
}

fn ident(s: &str) -> Ident {
    Ident::new(s, span())
}

fn str_expr(s: &str) -> Expr {
    Expr::Literal(Literal::String(s.to_string()), span())
}

fn int_expr(n: i64) -> Expr {
    Expr::Literal(Literal::Int(n), span())
}

fn named_expr(name: &str, value: Expr) -> Expr {
    Expr::NamedArg {
        name: ident(name),
        value: Box::new(value),
        span: span(),
    }
}

fn named_type(name: &str) -> TypeRef {
    TypeRef::Named {
        name: ident(name),
        span: span(),
    }
}

fn expr_stmt(e: Expr) -> Stmt {
    Stmt::ExprStmt(e, span())
}

/// Build a free-function decl. Each param is `(name, type_name, default)` —
/// `default = None` means required, `Some(expr)` means defaulted.
fn func_decl_with_defaults(
    name: &str,
    params: &[(&str, &str, Option<Expr>)],
    body_stmts: Vec<Stmt>,
) -> Decl {
    Decl::FuncDecl(FuncDecl {
        name: ident(name),
        params: params
            .iter()
            .map(|(n, t, dv)| Param {
                name: ident(n),
                ty: named_type(t),
                default_value: dv.clone(),
                span: span(),
            })
            .collect(),
        return_type: None,
        body: Block {
            stmts: body_stmts,
            span: span(),
        },
        is_async: false,
        is_unsafe: false,
        is_extern: false,
        attributes: Vec::new(),
        span: span(),
    })
}

/// Build a paramless body-less `main` whose body is the given stmts.
fn main_with(stmts: Vec<Stmt>) -> Decl {
    func_decl_with_defaults("main", &[], stmts)
}

// ---------------------------------------------------------------------------
// QA case: fetch("url") where fetch(url, timeout = 30) → fetch("url", 30)
// ---------------------------------------------------------------------------

#[test]
fn default_params_codegen_fills_omitted() {
    let fetch = func_decl_with_defaults(
        "fetch",
        &[
            ("url", "String", None),
            ("timeout", "Int", Some(int_expr(30))),
        ],
        Vec::new(),
    );
    let main = main_with(vec![expr_stmt(Expr::FuncCall {
        callee: Box::new(Expr::Ident(ident("fetch"), span())),
        args: vec![str_expr("url")],
        span: span(),
    })]);
    let src = generate_rust(&[fetch, main]).expect("codegen should succeed");
    assert!(
        src.contains(r#"fetch("url", 30)"#),
        "expected `fetch(\"url\", 30)` (default filled) in generated Rust, got: {src}"
    );
    let _file: syn::File = syn::parse_str(&src).expect("generated Rust should parse");
}

// ---------------------------------------------------------------------------
// All params supplied → no fill: fetch("url", 60) → fetch("url", 60)
// ---------------------------------------------------------------------------

#[test]
fn default_params_codegen_all_supplied_no_fill() {
    let fetch = func_decl_with_defaults(
        "fetch",
        &[
            ("url", "String", None),
            ("timeout", "Int", Some(int_expr(30))),
        ],
        Vec::new(),
    );
    let main = main_with(vec![expr_stmt(Expr::FuncCall {
        callee: Box::new(Expr::Ident(ident("fetch"), span())),
        args: vec![str_expr("url"), int_expr(60)],
        span: span(),
    })]);
    let src = generate_rust(&[fetch, main]).expect("codegen should succeed");
    assert!(
        src.contains(r#"fetch("url", 60)"#),
        "expected caller-supplied `fetch(\"url\", 60)` unchanged, got: {src}"
    );
    assert!(
        !src.contains(r#"fetch("url", 30)"#),
        "should NOT fill default when caller supplied the arg: {src}"
    );
    let _file: syn::File = syn::parse_str(&src).expect("generated Rust should parse");
}

// ---------------------------------------------------------------------------
// Multiple defaults, both omitted → both filled in declaration order.
// ---------------------------------------------------------------------------

#[test]
fn default_params_codegen_multiple_defaults_fill() {
    let cfg = func_decl_with_defaults(
        "cfg",
        &[
            ("host", "String", None),
            ("port", "Int", Some(int_expr(80))),
            ("retries", "Int", Some(int_expr(3))),
        ],
        Vec::new(),
    );
    let main = main_with(vec![expr_stmt(Expr::FuncCall {
        callee: Box::new(Expr::Ident(ident("cfg"), span())),
        args: vec![str_expr("db")],
        span: span(),
    })]);
    let src = generate_rust(&[cfg, main]).expect("codegen should succeed");
    assert!(
        src.contains(r#"cfg("db", 80, 3)"#),
        "expected both defaults filled `cfg(\"db\", 80, 3)`, got: {src}"
    );
    let _file: syn::File = syn::parse_str(&src).expect("generated Rust should parse");
}

// ---------------------------------------------------------------------------
// Partial omit: supply the first default, omit the last.
// cfg("db", 8080) where port=80, retries=3 → cfg("db", 8080, 3)
// ---------------------------------------------------------------------------

#[test]
fn default_params_codegen_partial_omit_fills_only_trailing() {
    let cfg = func_decl_with_defaults(
        "cfg",
        &[
            ("host", "String", None),
            ("port", "Int", Some(int_expr(80))),
            ("retries", "Int", Some(int_expr(3))),
        ],
        Vec::new(),
    );
    let main = main_with(vec![expr_stmt(Expr::FuncCall {
        callee: Box::new(Expr::Ident(ident("cfg"), span())),
        args: vec![str_expr("db"), int_expr(8080)],
        span: span(),
    })]);
    let src = generate_rust(&[cfg, main]).expect("codegen should succeed");
    assert!(
        src.contains(r#"cfg("db", 8080, 3)"#),
        "expected only trailing default filled `cfg(\"db\", 8080, 3)`, got: {src}"
    );
    let _file: syn::File = syn::parse_str(&src).expect("generated Rust should parse");
}

// ---------------------------------------------------------------------------
// Named arg + default interaction: fetch(url: "x") with timeout=30
// → reorder to fetch("x"), then fill default → fetch("x", 30)
// ---------------------------------------------------------------------------

#[test]
fn default_params_codegen_named_arg_with_default_fill() {
    let fetch = func_decl_with_defaults(
        "fetch",
        &[
            ("url", "String", None),
            ("timeout", "Int", Some(int_expr(30))),
        ],
        Vec::new(),
    );
    let main = main_with(vec![expr_stmt(Expr::FuncCall {
        callee: Box::new(Expr::Ident(ident("fetch"), span())),
        args: vec![named_expr("url", str_expr("x"))],
        span: span(),
    })]);
    let src = generate_rust(&[fetch, main]).expect("codegen should succeed");
    assert!(
        src.contains(r#"fetch("x", 30)"#),
        "expected named-arg reorder + default fill `fetch(\"x\", 30)`, got: {src}"
    );
    let _file: syn::File = syn::parse_str(&src).expect("generated Rust should parse");
}

// ---------------------------------------------------------------------------
// No-default func called with too few args → NO fill (Rust diagnoses).
// ---------------------------------------------------------------------------

#[test]
fn default_params_codegen_no_default_func_no_fill() {
    let add = func_decl_with_defaults("add", &[("a", "Int", None), ("b", "Int", None)], Vec::new());
    let main = main_with(vec![expr_stmt(Expr::FuncCall {
        callee: Box::new(Expr::Ident(ident("add"), span())),
        args: vec![int_expr(1)],
        span: span(),
    })]);
    let src = generate_rust(&[add, main]).expect("codegen should succeed");
    // No defaults → nothing filled; the call stays `add(1)` (Rust will
    // diagnose the missing arg, which is the correct behaviour — Buff v0.5
    // does no arity checking at codegen).
    assert!(
        src.contains("add(1)"),
        "expected no fill `add(1)` for a no-default callee, got: {src}"
    );
    let _file: syn::File = syn::parse_str(&src).expect("generated Rust should parse");
}

// ---------------------------------------------------------------------------
// Unknown callee (no fn decl in compilation unit) → no fill.
// ---------------------------------------------------------------------------

#[test]
fn default_params_codegen_unknown_callee_no_fill() {
    let main = main_with(vec![expr_stmt(Expr::FuncCall {
        callee: Box::new(Expr::Ident(ident("other"), span())),
        args: vec![str_expr("x")],
        span: span(),
    })]);
    let src = generate_rust(&[main]).expect("codegen should succeed");
    // No matching fn decl → no default info → call unchanged.
    assert!(
        src.contains(r#"other("x")"#),
        "expected unknown callee unchanged `other(\"x\")`, got: {src}"
    );
    let _file: syn::File = syn::parse_str(&src).expect("generated Rust should parse");
}

// ---------------------------------------------------------------------------
// String default fill: greet("bob") with greeting="hi" → greet("bob", "hi")
// ---------------------------------------------------------------------------

#[test]
fn default_params_codegen_string_default_fill() {
    let greet = func_decl_with_defaults(
        "greet",
        &[
            ("name", "String", None),
            ("greeting", "String", Some(str_expr("hi"))),
        ],
        Vec::new(),
    );
    let main = main_with(vec![expr_stmt(Expr::FuncCall {
        callee: Box::new(Expr::Ident(ident("greet"), span())),
        args: vec![str_expr("bob")],
        span: span(),
    })]);
    let src = generate_rust(&[greet, main]).expect("codegen should succeed");
    assert!(
        src.contains(r#"greet("bob", "hi")"#),
        "expected string default filled `greet(\"bob\", \"hi\")`, got: {src}"
    );
    let _file: syn::File = syn::parse_str(&src).expect("generated Rust should parse");
}

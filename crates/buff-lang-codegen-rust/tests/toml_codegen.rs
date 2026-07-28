//! T124e integration tests â€” `Toml` prelude namespace module codegen.
//!
//! Verifies that the Rust codegen:
//! - Lowers `Toml.parse(s)` to
//!   `toml::from_str::<std::collections::HashMap<String, toml::Value>>(s)`
//!   `.unwrap_or_default()` (panic-free â€” empty Map on parse failure).
//! - Lowers `Toml.stringify(v)` to
//!   `toml::to_string(&v).unwrap_or_default()` (panic-free â€” empty
//!   String on serialization failure).
//! - Records `toml` in `extern_crates` whenever the program uses `Toml`.
//! - Emits the `toml::from_str` / `toml::to_string` fully-qualified
//!   paths so NO `use` import is required in the generated source.
//!
//! Acceptance criterion from the task spec:
//!
//! ```text
//! Toml.parse("name = \"buff\"\nversion = \"1.0\"\n")
//!   ->  HashMap { "name" => Value::String("buff"),
//!                 "version" => Value::String("1.0") }
//! Toml.stringify(map)  ->  "name = \"buff\"\nversion = \"1.0\"\n"
//! ```
//!
//! Run via:
//!
//! ```text
//! cargo test -p buff-lang-codegen-rust --test toml_codegen
//! ```
//!
//! # Why AST-constructed tests (not source-parsed)
//!
//! `Toml` is a prelude namespace (like `Log` / `DateTime`), so source
//! parsing of `Toml.parse(...)` requires no new keyword / AST node â€”
//! the existing `MethodCall` shape handles it. We construct ASTs by
//! hand here for the same reasons `regex_codegen.rs` (T124d) does:
//! direct AST construction decouples the codegen-pinning snapshots
//! from any future parser-restructuring work, and lets us test
//! specific edge cases (e.g. wrong arity) without writing Buff source
//! that the parser may reject for orthogonal reasons.

use buff_lang_ast::common::{Block, Ident, Param};
use buff_lang_ast::decl::FuncDecl;
use buff_lang_ast::{Decl, Expr, Literal, Stmt, TypeRef};
use buff_lang_codegen_rust::{generate_rust, RustCodegen};
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

fn ident_expr(s: &str) -> Expr {
    Expr::Ident(ident(s), span())
}

fn named_type(name: &str) -> TypeRef {
    TypeRef::Named {
        name: ident(name),
        span: span(),
    }
}

/// Build a free-function decl `func <name>(<params...>) { <body> }`.
fn func_decl(name: &str, params: &[(&str, &str)], body_stmts: Vec<Stmt>) -> Decl {
    Decl::FuncDecl(FuncDecl {
        name: ident(name),
        params: params
            .iter()
            .map(|(n, t)| Param {
                name: ident(n),
                ty: named_type(t),
                default_value: None,
                is_comptime: false,
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
        type_params: Vec::new(),
        span: span(),
    })
}

fn expr_stmt(e: Expr) -> Stmt {
    Stmt::ExprStmt(e, span())
}

fn let_stmt(name: &str, value: Expr) -> Stmt {
    Stmt::LetDecl {
        name: ident(name),
        value,
        mutable: false,
        ty: None,
        span: span(),
    }
}

/// `Toml.<method>(args...)` AST node (associated-function call shape).
/// The receiver is the bare `Toml` namespace Ident.
fn toml_assoc_call(method: &str, args: Vec<Expr>) -> Expr {
    Expr::MethodCall {
        receiver: Box::new(ident_expr("Toml")),
        method: ident(method),
        args,
        span: span(),
    }
}

/// Generate Rust for a single helper function `f` containing `stmts`.
fn codegen_stmts_in(name: &str, stmts: Vec<Stmt>) -> String {
    let func = func_decl(name, &[], stmts);
    generate_rust(&[func]).expect("codegen must succeed")
}

/// Generate Rust for a single helper function `f` containing one expr stmt.
fn codegen_one_expr_in(name: &str, expr: Expr) -> String {
    codegen_stmts_in(name, vec![expr_stmt(expr)])
}

/// Assert the generated source re-parses as a valid Rust file (syn-level).
fn must_reparse(src: &str) {
    syn::parse_str::<syn::File>(src)
        .unwrap_or_else(|e| panic!("generated source must re-parse: {e}\n--- src ---\n{src}"));
}

// ---------------------------------------------------------------------------
// 1. Toml.parse(s) -> toml::from_str::<HashMap<String, toml::Value>>(s)
//                        .unwrap_or_default()
// ---------------------------------------------------------------------------

#[test]
fn toml_codegen_parse_string_literal() {
    let src = codegen_one_expr_in("f", toml_assoc_call("parse", vec![str_expr("a = 1\n")]));
    assert!(
        src.contains("toml::from_str"),
        "expected `toml::from_str` in: {src}"
    );
    // The turbofish pins the concrete Map<String, toml::Value> type so
    // the generated Rust is fully typed without a let-binding annotation.
    assert!(
        src.contains("::<std::collections::HashMap<String, toml::Value>>"),
        "expected turbofish pinning `HashMap<String, toml::Value>` in: {src}"
    );
    // unwrap_or_default (NOT bare unwrap) â€” panicking-generated-code rule.
    assert!(
        src.contains(".unwrap_or_default()"),
        "expected `.unwrap_or_default()` (panic-free fallback) in: {src}"
    );
    // No bare `.unwrap()` on the parse result.
    assert!(
        !src.contains(".unwrap()"),
        "expected NO bare `.unwrap()` in Toml.parse output: {src}"
    );
    must_reparse(&src);
}

#[test]
fn toml_codegen_parse_via_ident_arg() {
    // Toml.parse(my_string_var) â€” non-literal arg borrows via &.
    let src = codegen_one_expr_in(
        "f",
        toml_assoc_call("parse", vec![ident_expr("my_string_var")]),
    );
    // The ident should be borrowed (&my_string_var) so Rust's Deref
    // coercion turns String into &str (the type `toml::from_str` takes).
    assert!(
        src.contains("&my_string_var"),
        "expected `&my_string_var` (borrow coercion for String -> &str) in: {src}"
    );
    must_reparse(&src);
}

// ---------------------------------------------------------------------------
// 2. Toml.stringify(v) -> toml::to_string(&v).unwrap_or_default()
// ---------------------------------------------------------------------------

#[test]
fn toml_codegen_stringify_ident_arg() {
    // Toml.stringify(my_map_var) â€” the arg is borrowed via & so Rust's
    // serde-Serialize bound on `toml::to_string(&T)` is satisfied for
    // any Map<String, ?> value.
    let src = codegen_one_expr_in(
        "f",
        toml_assoc_call("stringify", vec![ident_expr("my_map_var")]),
    );
    assert!(
        src.contains("toml::to_string"),
        "expected `toml::to_string` in: {src}"
    );
    assert!(
        src.contains("&my_map_var"),
        "expected `&my_map_var` (Serialize requires &T) in: {src}"
    );
    // unwrap_or_default (NOT bare unwrap) â€” panicking-generated-code rule.
    assert!(
        src.contains(".unwrap_or_default()"),
        "expected `.unwrap_or_default()` (panic-free fallback) in: {src}"
    );
    must_reparse(&src);
}

#[test]
fn toml_codegen_stringify_string_literal_arg() {
    // Toml.stringify("foo") â€” string literals are valid Serialize values
    // (str impls Serialize via toml). The generated code borrows via
    // `&"foo"` so the type-checker sees `&&'static str` which derefs to
    // `&str` for the Serialize bound.
    let src = codegen_one_expr_in("f", toml_assoc_call("stringify", vec![str_expr("foo")]));
    assert!(
        src.contains("toml::to_string"),
        "expected `toml::to_string` in: {src}"
    );
    must_reparse(&src);
}

// ---------------------------------------------------------------------------
// 3. extern_crates registration.
// ---------------------------------------------------------------------------

#[test]
fn toml_codegen_registers_toml_extern_crate() {
    // A program with any Toml.* call registers the toml crate.
    let main = func_decl(
        "main",
        &[],
        vec![expr_stmt(toml_assoc_call(
            "parse",
            vec![str_expr("a = 1\n")],
        ))],
    );
    let mut codegen = RustCodegen::new();
    let _ = codegen.generate(&[main]).expect("codegen must succeed");
    let extern_crates = codegen.extern_crates();
    assert!(
        extern_crates.contains("toml"),
        "extern_crates should contain `toml`, got: {:?}",
        extern_crates
    );
}

#[test]
fn toml_codegen_registers_toml_via_stringify() {
    // A program with Toml.stringify(...) but no Toml.parse(...) should
    // still register toml (the walker flags any Toml.* call).
    let main = func_decl(
        "main",
        &[],
        vec![expr_stmt(toml_assoc_call(
            "stringify",
            vec![ident_expr("m")],
        ))],
    );
    let mut codegen = RustCodegen::new();
    let _ = codegen.generate(&[main]).expect("codegen must succeed");
    let extern_crates = codegen.extern_crates();
    assert!(
        extern_crates.contains("toml"),
        "extern_crates should contain `toml` (stringify walker), got: {:?}",
        extern_crates
    );
}

#[test]
fn toml_codegen_no_toml_extern_crate_when_unused() {
    // A program with no Toml calls should not register toml.
    let main = func_decl(
        "main",
        &[],
        vec![expr_stmt(Expr::FuncCall {
            callee: Box::new(ident_expr("print")),
            args: vec![str_expr("hi")],
            span: span(),
        })],
    );
    let mut codegen = RustCodegen::new();
    let _ = codegen.generate(&[main]).expect("codegen must succeed");
    let extern_crates = codegen.extern_crates();
    assert!(
        !extern_crates.contains("toml"),
        "extern_crates should NOT contain `toml` when Toml is unused, got: {:?}",
        extern_crates
    );
}

// ---------------------------------------------------------------------------
// 4. Error cases.
// ---------------------------------------------------------------------------

#[test]
fn toml_codegen_rejects_parse_with_wrong_arity() {
    // Toml.parse() with no args â€” should error.
    let result = std::panic::catch_unwind(|| {
        let _ = codegen_one_expr_in("f", toml_assoc_call("parse", vec![]));
    });
    assert!(
        result.is_err(),
        "expected codegen to reject `Toml.parse()` (no pattern arg)"
    );
}

#[test]
fn toml_codegen_rejects_stringify_with_wrong_arity() {
    // Toml.stringify() with no args â€” should error.
    let result = std::panic::catch_unwind(|| {
        let _ = codegen_one_expr_in("f", toml_assoc_call("stringify", vec![]));
    });
    assert!(
        result.is_err(),
        "expected codegen to reject `Toml.stringify()` (no value arg)"
    );
}

// ---------------------------------------------------------------------------
// 5. insta snapshots â€” byte-stable codegen pinning.
// ---------------------------------------------------------------------------

#[test]
fn toml_codegen_parse_snapshot() {
    // Snapshot the canonical parse lowering.
    let src = codegen_one_expr_in("f", toml_assoc_call("parse", vec![str_expr("a = 1\n")]));
    insta::assert_snapshot!(src);
}

#[test]
fn toml_codegen_parse_ident_snapshot() {
    // Snapshot the parse lowering with a non-literal arg (shows the
    // borrow coercion for String -> &str).
    let src = codegen_one_expr_in("f", toml_assoc_call("parse", vec![ident_expr("input")]));
    insta::assert_snapshot!(src);
}

#[test]
fn toml_codegen_stringify_snapshot() {
    // Snapshot the canonical stringify lowering with an ident arg.
    let src = codegen_one_expr_in("f", toml_assoc_call("stringify", vec![ident_expr("m")]));
    insta::assert_snapshot!(src);
}

#[test]
fn toml_codegen_full_program_snapshot() {
    // End-to-end snapshot: a `main` that exercises the canonical
    // round-trip (parse + stringify). Pins the full shape of the
    // generated Rust for a typical Toml-using program (the acceptance
    // criterion from the task spec).
    let main = func_decl(
        "main",
        &[],
        vec![
            let_stmt(
                "m",
                toml_assoc_call("parse", vec![str_expr("name = \"buff\"\n")]),
            ),
            expr_stmt(toml_assoc_call("stringify", vec![ident_expr("m")])),
        ],
    );
    let mut codegen = RustCodegen::new();
    let file = codegen.generate(&[main]).expect("codegen must succeed");
    let src = buff_lang_codegen_rust::format_file(&file);
    insta::assert_snapshot!(src);
}

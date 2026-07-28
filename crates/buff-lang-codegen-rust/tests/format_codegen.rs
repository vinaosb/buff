//! T124i integration tests - `Yaml` + `Csv` prelude namespace module
//! codegen.
//!
//! Verifies that the Rust codegen:
//! - Lowers `Yaml.parse(s)` to
//!   `serde_yml::from_str::<std::collections::HashMap<String,
//!   serde_yml::Value>>(s).unwrap_or_default()` (panic-free - empty
//!   Map on parse failure, mirroring the Toml.parse stance from T124e).
//! - Lowers `Yaml.stringify(v)` to
//!   `serde_yml::to_string(&v).unwrap_or_default()` (panic-free -
//!   empty String on serialization failure).
//! - Lowers `Csv.parse(s)` to a block expression that builds a
//!   `csv::ReaderBuilder` with `.has_headers(false)` and collects
//!   rows into `Vec<Vec<String>>` via `.filter_map(|r| r.ok())` (panic-
//!   free - malformed rows are skipped, NEVER surfaced as panics).
//! - Lowers `Csv.stringify(rows)` to a block expression that builds a
//!   `csv::Writer` over `Vec<u8>`, writes each row via `write_record`,
//!   and converts the buffer to String (panic-free - empty String on
//!   failure).
//! - Records `serde_yml` in `extern_crates` whenever the program uses
//!   `Yaml`, and `csv` whenever the program uses `Csv`.
//! - Emits fully-qualified `serde_yml::*` / `csv::*` paths so NO `use`
//!   import is required in the generated source.
//!
//! Acceptance criterion from the task spec:
//!
//! ```text
//! Yaml.parse("name: buff\nversion: 1\n")
//!   ->  HashMap { "name" => Value::String("buff"),
//!                 "version" => Value::Sequence(...) }
//! Yaml.stringify(map)  ->  "name: buff\nversion: 1\n"
//! Csv.parse("a,b,c\n1,2,3\n")
//!   ->  Vec<Vec<String>> { ["a","b","c"], ["1","2","3"] }
//! Csv.stringify(rows)   ->  "a,b,c\n1,2,3\n"
//! ```
//!
//! Run via:
//!
//! ```text
//! cargo test -p buff-lang-codegen-rust --test format_codegen
//! ```
//!
//! # Why AST-constructed tests (not source-parsed)
//!
//! `Yaml` / `Csv` are prelude namespaces (like `Toml` / `Log`), so
//! source parsing of `Yaml.parse(...)` requires no new keyword / AST
//! node - the existing `MethodCall` shape handles it. We construct
//! ASTs by hand here for the same reasons `toml_codegen.rs` (T124e)
//! and `regex_codegen.rs` (T124d) do: direct AST construction
//! decouples the codegen-pinning snapshots from any future parser-
//! restructuring work, and lets us test specific edge cases (e.g.
//! wrong arity) without writing Buff source that the parser may
//! reject for orthogonal reasons.

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

/// `<namespace>.<method>(args...)` AST node (associated-function call
/// shape). The receiver is the bare namespace Ident (e.g. `Yaml`,
/// `Csv`).
fn ns_assoc_call(namespace: &str, method: &str, args: Vec<Expr>) -> Expr {
    Expr::MethodCall {
        receiver: Box::new(ident_expr(namespace)),
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

// ===========================================================================
// 1. Yaml.parse(s) -> serde_yml::from_str::<HashMap<String,
//    serde_yml::Value>>(s).unwrap_or_default()
// ===========================================================================

#[test]
fn yaml_codegen_parse_string_literal() {
    let src = codegen_one_expr_in(
        "f",
        ns_assoc_call("Yaml", "parse", vec![str_expr("a: 1\n")]),
    );
    assert!(
        src.contains("serde_yml::from_str"),
        "expected `serde_yml::from_str` in: {src}"
    );
    // The turbofish pins the concrete Map<String, serde_yml::Value> type
    // so the generated Rust is fully typed without a let-binding annotation.
    assert!(
        src.contains("::<std::collections::HashMap<String, serde_yml::Value>>"),
        "expected turbofish pinning `HashMap<String, serde_yml::Value>` in: {src}"
    );
    // unwrap_or_default (NOT bare unwrap) - panicking-generated-code rule.
    assert!(
        src.contains(".unwrap_or_default()"),
        "expected `.unwrap_or_default()` (panic-free fallback) in: {src}"
    );
    // No bare `.unwrap()` on the parse result.
    assert!(
        !src.contains(".unwrap()"),
        "expected NO bare `.unwrap()` in Yaml.parse output: {src}"
    );
    // The deprecated `serde_yaml` (with `a`) MUST NOT appear anywhere.
    assert!(
        !src.contains("serde_yaml"),
        "expected NO deprecated `serde_yaml` spelling in: {src}"
    );
    must_reparse(&src);
}

#[test]
fn yaml_codegen_parse_via_ident_arg() {
    // Yaml.parse(my_string_var) - non-literal arg borrows via &.
    let src = codegen_one_expr_in(
        "f",
        ns_assoc_call("Yaml", "parse", vec![ident_expr("my_string_var")]),
    );
    // The ident should be borrowed (&my_string_var) so Rust's Deref
    // coercion turns String into &str (the type `serde_yml::from_str`
    // takes).
    assert!(
        src.contains("&my_string_var"),
        "expected `&my_string_var` (borrow coercion for String -> &str) in: {src}"
    );
    must_reparse(&src);
}

// ===========================================================================
// 2. Yaml.stringify(v) -> serde_yml::to_string(&v).unwrap_or_default()
// ===========================================================================

#[test]
fn yaml_codegen_stringify_ident_arg() {
    // Yaml.stringify(my_map_var) - the arg is borrowed via & so Rust's
    // serde-Serialize bound on `serde_yml::to_string(&T)` is satisfied
    // for any Map<String, ?> value.
    let src = codegen_one_expr_in(
        "f",
        ns_assoc_call("Yaml", "stringify", vec![ident_expr("my_map_var")]),
    );
    assert!(
        src.contains("serde_yml::to_string"),
        "expected `serde_yml::to_string` in: {src}"
    );
    assert!(
        src.contains("&my_map_var"),
        "expected `&my_map_var` (Serialize requires &T) in: {src}"
    );
    // unwrap_or_default (NOT bare unwrap) - panicking-generated-code rule.
    assert!(
        src.contains(".unwrap_or_default()"),
        "expected `.unwrap_or_default()` (panic-free fallback) in: {src}"
    );
    // The deprecated `serde_yaml` (with `a`) MUST NOT appear anywhere.
    assert!(
        !src.contains("serde_yaml"),
        "expected NO deprecated `serde_yaml` spelling in: {src}"
    );
    must_reparse(&src);
}

#[test]
fn yaml_codegen_stringify_string_literal_arg() {
    // Yaml.stringify("foo") - string literals are valid Serialize values
    // (str impls Serialize via serde_yml). The generated code borrows via
    // `&"foo"`.
    let src = codegen_one_expr_in(
        "f",
        ns_assoc_call("Yaml", "stringify", vec![str_expr("foo")]),
    );
    assert!(
        src.contains("serde_yml::to_string"),
        "expected `serde_yml::to_string` in: {src}"
    );
    must_reparse(&src);
}

// ===========================================================================
// 3. Csv.parse(s) -> csv::ReaderBuilder block -> Vec<Vec<String>>
// ===========================================================================

#[test]
fn csv_codegen_parse_string_literal() {
    let src = codegen_one_expr_in(
        "f",
        ns_assoc_call("Csv", "parse", vec![str_expr("a,b\n1,2\n")]),
    );
    // csv::ReaderBuilder is the documented csv 1.x parse entry point.
    assert!(
        src.contains("csv::ReaderBuilder::new()"),
        "expected `csv::ReaderBuilder::new()` in: {src}"
    );
    // Per spec, Csv.parse surfaces EVERY row uniformly (no header
    // special-casing). The generated code MUST disable header handling.
    assert!(
        src.contains(".has_headers(false)"),
        "expected `.has_headers(false)` (uniform rows per spec) in: {src}"
    );
    // The reader takes bytes, so `as_bytes()` must appear.
    assert!(
        src.contains(".as_bytes()"),
        "expected `.as_bytes()` (csv reader takes bytes) in: {src}"
    );
    // Records iterator is consumed.
    assert!(
        src.contains(".records()"),
        "expected `.records()` iterator in: {src}"
    );
    // Malformed rows skipped via .filter_map(|r| r.ok()) - panic-free.
    assert!(
        src.contains(".filter_map(|r| r.ok())"),
        "expected `.filter_map(|r| r.ok())` (panic-free malformed-row skip) in: {src}"
    );
    // Final collect to Vec<Vec<String>> (the surface Vector<Vector<String>>).
    assert!(
        src.contains(".collect::<Vec<Vec<String>>>()"),
        "expected `.collect::<Vec<Vec<String>>>()` turbofish in: {src}"
    );
    // No bare `.unwrap()` (panicking-generated-code rule).
    assert!(
        !src.contains(".unwrap()"),
        "expected NO bare `.unwrap()` in Csv.parse output: {src}"
    );
    must_reparse(&src);
}

#[test]
fn csv_codegen_parse_via_ident_arg() {
    // Csv.parse(my_string_var) - non-literal arg uses .as_bytes() on
    // the owned String.
    let src = codegen_one_expr_in(
        "f",
        ns_assoc_call("Csv", "parse", vec![ident_expr("input")]),
    );
    // The ident appears as the .as_bytes() receiver.
    assert!(
        src.contains("input.as_bytes()"),
        "expected `input.as_bytes()` in: {src}"
    );
    must_reparse(&src);
}

// ===========================================================================
// 4. Csv.stringify(rows) -> csv::Writer block -> String
// ===========================================================================

#[test]
fn csv_codegen_stringify_ident_arg() {
    let src = codegen_one_expr_in(
        "f",
        ns_assoc_call("Csv", "stringify", vec![ident_expr("rows")]),
    );
    // csv::Writer::from_writer over Vec<u8> is the documented csv 1.x
    // serialization entry point.
    assert!(
        src.contains("csv::Writer::from_writer"),
        "expected `csv::Writer::from_writer` in: {src}"
    );
    // The buffer type is Vec<u8>.
    assert!(
        src.contains("Vec::<u8>::new()"),
        "expected `Vec::<u8>::new()` turbofish in: {src}"
    );
    // Each row is written via write_record.
    assert!(
        src.contains(".write_record("),
        "expected `.write_record(` in: {src}"
    );
    // The .ok() discards write_record's Result (panic-free).
    assert!(
        src.contains(".ok();"),
        "expected `.ok();` (panic-free write_record discard) in: {src}"
    );
    // The arg is borrowed via &rows (Buff's move-by-default would
    // otherwise consume the Vec; borrowing lets the caller keep using it).
    assert!(
        src.contains("&rows"),
        "expected `&rows` (borrow so caller keeps the rows Vec) in: {src}"
    );
    // Final buffer lift via String::from_utf8 + unwrap_or_default.
    assert!(
        src.contains("String::from_utf8("),
        "expected `String::from_utf8(` in: {src}"
    );
    assert!(
        src.contains(".into_inner().unwrap_or_default()"),
        "expected `.into_inner().unwrap_or_default()` in: {src}"
    );
    // No bare `.unwrap()` (panicking-generated-code rule).
    assert!(
        !src.contains(".unwrap()"),
        "expected NO bare `.unwrap()` in Csv.stringify output: {src}"
    );
    must_reparse(&src);
}

// ===========================================================================
// 5. extern_crates registration.
// ===========================================================================

#[test]
fn yaml_codegen_registers_serde_yml_extern_crate() {
    // A program with any Yaml.* call registers the serde_yml crate
    // (note: with underscore, NOT the deprecated serde_yaml).
    let main = func_decl(
        "main",
        &[],
        vec![expr_stmt(ns_assoc_call(
            "Yaml",
            "parse",
            vec![str_expr("a: 1\n")],
        ))],
    );
    let mut codegen = RustCodegen::new();
    let _ = codegen.generate(&[main]).expect("codegen must succeed");
    let extern_crates = codegen.extern_crates();
    assert!(
        extern_crates.contains("serde_yml"),
        "extern_crates should contain `serde_yml`, got: {:?}",
        extern_crates
    );
    // The deprecated crate name MUST NOT be registered.
    assert!(
        !extern_crates.contains("serde_yaml"),
        "extern_crates should NOT contain the deprecated `serde_yaml` spelling, got: {:?}",
        extern_crates
    );
}

#[test]
fn yaml_codegen_registers_serde_yml_via_stringify() {
    // A program with Yaml.stringify(...) but no Yaml.parse(...) should
    // still register serde_yml (the walker flags any Yaml.* call).
    let main = func_decl(
        "main",
        &[],
        vec![expr_stmt(ns_assoc_call(
            "Yaml",
            "stringify",
            vec![ident_expr("m")],
        ))],
    );
    let mut codegen = RustCodegen::new();
    let _ = codegen.generate(&[main]).expect("codegen must succeed");
    let extern_crates = codegen.extern_crates();
    assert!(
        extern_crates.contains("serde_yml"),
        "extern_crates should contain `serde_yml` (stringify walker), got: {:?}",
        extern_crates
    );
}

#[test]
fn csv_codegen_registers_csv_extern_crate() {
    // A program with any Csv.* call registers the csv crate.
    let main = func_decl(
        "main",
        &[],
        vec![expr_stmt(ns_assoc_call(
            "Csv",
            "parse",
            vec![str_expr("a,b\n1,2\n")],
        ))],
    );
    let mut codegen = RustCodegen::new();
    let _ = codegen.generate(&[main]).expect("codegen must succeed");
    let extern_crates = codegen.extern_crates();
    assert!(
        extern_crates.contains("csv"),
        "extern_crates should contain `csv`, got: {:?}",
        extern_crates
    );
}

#[test]
fn csv_codegen_registers_csv_via_stringify() {
    // A program with Csv.stringify(...) but no Csv.parse(...) should
    // still register csv (the walker flags any Csv.* call).
    let main = func_decl(
        "main",
        &[],
        vec![expr_stmt(ns_assoc_call(
            "Csv",
            "stringify",
            vec![ident_expr("rows")],
        ))],
    );
    let mut codegen = RustCodegen::new();
    let _ = codegen.generate(&[main]).expect("codegen must succeed");
    let extern_crates = codegen.extern_crates();
    assert!(
        extern_crates.contains("csv"),
        "extern_crates should contain `csv` (stringify walker), got: {:?}",
        extern_crates
    );
}

#[test]
fn format_codegen_no_extern_crate_when_unused() {
    // A program with no Yaml/Csv calls should not register either crate.
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
        !extern_crates.contains("serde_yml"),
        "extern_crates should NOT contain `serde_yml` when Yaml is unused, got: {:?}",
        extern_crates
    );
    assert!(
        !extern_crates.contains("csv"),
        "extern_crates should NOT contain `csv` when Csv is unused, got: {:?}",
        extern_crates
    );
}

// ===========================================================================
// 6. Error cases (wrong arity).
// ===========================================================================

#[test]
fn yaml_codegen_rejects_parse_with_wrong_arity() {
    // Yaml.parse() with no args - should error via one_arg(self)?.
    let result = std::panic::catch_unwind(|| {
        let _ = codegen_one_expr_in("f", ns_assoc_call("Yaml", "parse", vec![]));
    });
    assert!(
        result.is_err(),
        "expected codegen to reject `Yaml.parse()` (no string arg)"
    );
}

#[test]
fn yaml_codegen_rejects_stringify_with_wrong_arity() {
    // Yaml.stringify() with no args - should error.
    let result = std::panic::catch_unwind(|| {
        let _ = codegen_one_expr_in("f", ns_assoc_call("Yaml", "stringify", vec![]));
    });
    assert!(
        result.is_err(),
        "expected codegen to reject `Yaml.stringify()` (no value arg)"
    );
}

#[test]
fn csv_codegen_rejects_parse_with_wrong_arity() {
    // Csv.parse() with no args - should error.
    let result = std::panic::catch_unwind(|| {
        let _ = codegen_one_expr_in("f", ns_assoc_call("Csv", "parse", vec![]));
    });
    assert!(
        result.is_err(),
        "expected codegen to reject `Csv.parse()` (no string arg)"
    );
}

#[test]
fn csv_codegen_rejects_stringify_with_wrong_arity() {
    // Csv.stringify() with no args - should error.
    let result = std::panic::catch_unwind(|| {
        let _ = codegen_one_expr_in("f", ns_assoc_call("Csv", "stringify", vec![]));
    });
    assert!(
        result.is_err(),
        "expected codegen to reject `Csv.stringify()` (no rows arg)"
    );
}

// ===========================================================================
// 7. insta snapshots - byte-stable codegen pinning.
// ===========================================================================

#[test]
fn yaml_codegen_parse_snapshot() {
    // Snapshot the canonical Yaml.parse lowering with a string literal.
    let src = codegen_one_expr_in(
        "f",
        ns_assoc_call("Yaml", "parse", vec![str_expr("a: 1\n")]),
    );
    insta::assert_snapshot!(src);
}

#[test]
fn yaml_codegen_stringify_snapshot() {
    // Snapshot the canonical Yaml.stringify lowering with an ident arg.
    let src = codegen_one_expr_in(
        "f",
        ns_assoc_call("Yaml", "stringify", vec![ident_expr("m")]),
    );
    insta::assert_snapshot!(src);
}

#[test]
fn csv_codegen_parse_snapshot() {
    // Snapshot the canonical Csv.parse lowering with a string literal.
    let src = codegen_one_expr_in(
        "f",
        ns_assoc_call("Csv", "parse", vec![str_expr("a,b\n1,2\n")]),
    );
    insta::assert_snapshot!(src);
}

#[test]
fn csv_codegen_stringify_snapshot() {
    // Snapshot the canonical Csv.stringify lowering with an ident arg.
    let src = codegen_one_expr_in(
        "f",
        ns_assoc_call("Csv", "stringify", vec![ident_expr("rows")]),
    );
    insta::assert_snapshot!(src);
}

#[test]
fn format_codegen_full_program_snapshot() {
    // End-to-end snapshot: a `main` that exercises BOTH modules'
    // canonical round-trips (Yaml parse + stringify, Csv parse +
    // stringify). Pins the full shape of the generated Rust for a
    // typical format-using program (the acceptance criterion from the
    // task spec).
    let main = func_decl(
        "main",
        &[],
        vec![
            let_stmt(
                "m",
                ns_assoc_call("Yaml", "parse", vec![str_expr("name: buff\n")]),
            ),
            expr_stmt(ns_assoc_call("Yaml", "stringify", vec![ident_expr("m")])),
            let_stmt(
                "rows",
                ns_assoc_call("Csv", "parse", vec![str_expr("a,b\n1,2\n")]),
            ),
            expr_stmt(ns_assoc_call("Csv", "stringify", vec![ident_expr("rows")])),
        ],
    );
    let mut codegen = RustCodegen::new();
    let file = codegen.generate(&[main]).expect("codegen must succeed");
    let src = buff_lang_codegen_rust::format_file(&file);
    insta::assert_snapshot!(src);
}

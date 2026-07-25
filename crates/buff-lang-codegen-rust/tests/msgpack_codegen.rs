//! T51 integration tests - MessagePack prelude namespace codegen.
//!
//! Verifies that the Rust codegen lowers the three T51 MsgPack
//! associated functions:
//!
//! - **MsgPack.serialize(value)** -> `buff_msgpack::serialize(&value)
//!   .unwrap_or_default()` (Vec<u8> / Vector<Byte>; empty Vec on
//!   failure - panic-free via `.unwrap_or_default()`, NOT bare
//!   `.unwrap()`).
//! - **MsgPack.deserialize(bytes)** -> `buff_msgpack::deserialize
//!   (&bytes).unwrap_or_default()` (serde_json::Value::Null on
//!   failure - panic-free; Value impls Default).
//! - **MsgPack.roundtrip(value)** -> `buff_msgpack::roundtrip(&value)`
//!   (Option<Value> - None on either step failing; the runtime fn
//!   already returns Option so no `.unwrap_or_default()` collapse).
//!
//! Acceptance lowering (per the T51 task spec):
//!
//! ```text
//! MsgPack.serialize(v)   -> buff_msgpack::serialize(&v).unwrap_or_default()
//! MsgPack.deserialize(b)  -> buff_msgpack::deserialize(&b).unwrap_or_default()
//! MsgPack.roundtrip(v)    -> buff_msgpack::roundtrip(&v)
//! ```
//!
//! Run via:
//!
//! ```text
//! cargo test -p buff-lang-codegen-rust --test msgpack_codegen
//! ```
//!
//! # Why AST-constructed tests (not source-parsed)
//!
//! MsgPack is a prelude namespace (associated functions on a
//! namespace-only type), so source parsing requires no new keyword /
//! AST node - the existing `MethodCall` shape handles them. We
//! construct ASTs by hand here for the same reasons `crypto_codegen
//! .rs` (T124k), `fs_codegen.rs` (T124j), `format_codegen.rs`
//! (T124i), `web_codegen.rs` (T124h), `system_codegen.rs` (T124g),
//! `regex_codegen.rs` (T124d), `toml_codegen.rs` (T124e), and
//! `utility_codegen.rs` (T124f) do: direct AST construction
//! decouples the codegen-pinning snapshots from any future
//! parser-restructuring work, and lets us test specific edge cases
//! (e.g. wrong arity, ident vs literal arg) without writing Buff
//! source that the parser may reject for orthogonal reasons.

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
/// shape). The receiver is the bare namespace Ident (e.g. `MsgPack`).
fn ns_assoc_call(namespace: &str, method: &str, args: Vec<Expr>) -> Expr {
    Expr::MethodCall {
        receiver: Box::new(ident_expr(namespace)),
        method: ident(method),
        args,
        span: span(),
    }
}

fn codegen_stmts_in(name: &str, stmts: Vec<Stmt>) -> String {
    let func = func_decl(name, &[], stmts);
    generate_rust(&[func]).expect("codegen must succeed")
}

fn codegen_one_expr_in(name: &str, expr: Expr) -> String {
    codegen_stmts_in(name, vec![expr_stmt(expr)])
}

fn must_reparse(src: &str) {
    syn::parse_str::<syn::File>(src)
        .unwrap_or_else(|e| panic!("generated source must re-parse: {e}\n--- src ---\n{src}"));
}

// ===========================================================================
// 1. MsgPack.serialize - one-arg, returns Vec<u8> via .unwrap_or_default().
// ===========================================================================

#[test]
fn msgpack_codegen_serialize_with_literal_calls_buff_msgpack_serialize() {
    let src = codegen_one_expr_in(
        "f",
        ns_assoc_call("MsgPack", "serialize", vec![str_expr("hello")]),
    );
    assert!(
        src.contains("buff_msgpack::serialize"),
        "expected `buff_msgpack::serialize(` in: {src}"
    );
    assert!(
        src.contains(".unwrap_or_default()"),
        "expected `.unwrap_or_default()` (panic-free Vec collapse) in: {src}"
    );
    assert!(
        !src.contains(".unwrap()"),
        "expected NO bare `.unwrap()` in MsgPack.serialize output: {src}"
    );
    must_reparse(&src);
}

#[test]
fn msgpack_codegen_serialize_with_ident_arg_splices_ident() {
    let src = codegen_one_expr_in(
        "f",
        ns_assoc_call("MsgPack", "serialize", vec![ident_expr("value")]),
    );
    assert!(
        src.contains("buff_msgpack::serialize"),
        "expected `buff_msgpack::serialize(` in: {src}"
    );
    assert!(
        src.contains("&value"),
        "expected `&value` (ident arg splice by ref) in: {src}"
    );
    assert!(
        !src.contains(".unwrap()"),
        "expected NO bare `.unwrap()` in MsgPack.serialize output: {src}"
    );
    must_reparse(&src);
}

// ===========================================================================
// 2. MsgPack.deserialize - one-arg, returns Value via .unwrap_or_default().
// ===========================================================================

#[test]
fn msgpack_codegen_deserialize_with_literal_calls_buff_msgpack_deserialize() {
    let src = codegen_one_expr_in(
        "f",
        ns_assoc_call("MsgPack", "deserialize", vec![str_expr("not real msgpack")]),
    );
    assert!(
        src.contains("buff_msgpack::deserialize"),
        "expected `buff_msgpack::deserialize(` in: {src}"
    );
    assert!(
        src.contains(".unwrap_or_default()"),
        "expected `.unwrap_or_default()` (panic-free Value collapse) in: {src}"
    );
    assert!(
        !src.contains(".unwrap()"),
        "expected NO bare `.unwrap()` in MsgPack.deserialize output: {src}"
    );
    must_reparse(&src);
}

#[test]
fn msgpack_codegen_deserialize_with_ident_arg_splices_ident() {
    let src = codegen_one_expr_in(
        "f",
        ns_assoc_call("MsgPack", "deserialize", vec![ident_expr("bytes")]),
    );
    assert!(
        src.contains("buff_msgpack::deserialize"),
        "expected `buff_msgpack::deserialize(` in: {src}"
    );
    assert!(
        src.contains("&bytes"),
        "expected `&bytes` (ident arg splice by ref) in: {src}"
    );
    assert!(
        !src.contains(".unwrap()"),
        "expected NO bare `.unwrap()` in MsgPack.deserialize output: {src}"
    );
    must_reparse(&src);
}

// ===========================================================================
// 3. MsgPack.roundtrip - one-arg, returns Option<Value> directly (no unwrap).
// ===========================================================================

#[test]
fn msgpack_codegen_roundtrip_with_literal_calls_buff_msgpack_roundtrip() {
    let src = codegen_one_expr_in(
        "f",
        ns_assoc_call("MsgPack", "roundtrip", vec![str_expr("hello")]),
    );
    assert!(
        src.contains("buff_msgpack::roundtrip"),
        "expected `buff_msgpack::roundtrip(` in: {src}"
    );
    // roundtrip returns Option<Value> directly — NO .unwrap_or_default()
    // collapse on the codegen side (the runtime fn is already Option).
    assert!(
        !src.contains(".unwrap_or_default()"),
        "expected NO `.unwrap_or_default()` for roundtrip in: {src}"
    );
    assert!(
        !src.contains(".unwrap()"),
        "expected NO bare `.unwrap()` in MsgPack.roundtrip output: {src}"
    );
    must_reparse(&src);
}

#[test]
fn msgpack_codegen_roundtrip_with_ident_arg_splices_ident() {
    let src = codegen_one_expr_in(
        "f",
        ns_assoc_call("MsgPack", "roundtrip", vec![ident_expr("value")]),
    );
    assert!(
        src.contains("buff_msgpack::roundtrip"),
        "expected `buff_msgpack::roundtrip(` in: {src}"
    );
    assert!(
        src.contains("&value"),
        "expected `&value` (ident arg splice by ref) in: {src}"
    );
    assert!(
        !src.contains(".unwrap()"),
        "expected NO bare `.unwrap()` in MsgPack.roundtrip output: {src}"
    );
    must_reparse(&src);
}

// ===========================================================================
// 4. extern_crates registration - MsgPack walker flags buff-msgpack +
//    rmp-serde + serde_json.
// ===========================================================================

#[test]
fn msgpack_codegen_registers_buff_msgpack_and_deps_for_serialize() {
    let main = func_decl(
        "main",
        &[],
        vec![expr_stmt(ns_assoc_call(
            "MsgPack",
            "serialize",
            vec![str_expr("hi")],
        ))],
    );
    let mut codegen = RustCodegen::new();
    let _ = codegen.generate(&[main]).expect("codegen must succeed");
    let extern_crates = codegen.extern_crates();
    assert!(
        extern_crates.contains("buff-msgpack"),
        "extern_crates should contain `buff-msgpack`, got: {:?}",
        extern_crates
    );
    assert!(
        extern_crates.contains("rmp-serde"),
        "extern_crates should contain `rmp-serde`, got: {:?}",
        extern_crates
    );
    assert!(
        extern_crates.contains("serde_json"),
        "extern_crates should contain `serde_json`, got: {:?}",
        extern_crates
    );
}

#[test]
fn msgpack_codegen_registers_buff_msgpack_and_deps_for_deserialize() {
    let main = func_decl(
        "main",
        &[],
        vec![expr_stmt(ns_assoc_call(
            "MsgPack",
            "deserialize",
            vec![ident_expr("bytes")],
        ))],
    );
    let mut codegen = RustCodegen::new();
    let _ = codegen.generate(&[main]).expect("codegen must succeed");
    let extern_crates = codegen.extern_crates();
    assert!(
        extern_crates.contains("buff-msgpack"),
        "extern_crates should contain `buff-msgpack`, got: {:?}",
        extern_crates
    );
    assert!(
        extern_crates.contains("rmp-serde"),
        "extern_crates should contain `rmp-serde`, got: {:?}",
        extern_crates
    );
}

#[test]
fn msgpack_codegen_registers_buff_msgpack_and_deps_for_roundtrip() {
    let main = func_decl(
        "main",
        &[],
        vec![expr_stmt(ns_assoc_call(
            "MsgPack",
            "roundtrip",
            vec![ident_expr("value")],
        ))],
    );
    let mut codegen = RustCodegen::new();
    let _ = codegen.generate(&[main]).expect("codegen must succeed");
    let extern_crates = codegen.extern_crates();
    assert!(
        extern_crates.contains("buff-msgpack"),
        "extern_crates should contain `buff-msgpack`, got: {:?}",
        extern_crates
    );
    assert!(
        extern_crates.contains("serde_json"),
        "extern_crates should contain `serde_json`, got: {:?}",
        extern_crates
    );
}

#[test]
fn msgpack_codegen_no_extern_crate_when_unused() {
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
        !extern_crates.contains("buff-msgpack"),
        "extern_crates should NOT contain `buff-msgpack` when MsgPack is unused, got: {:?}",
        extern_crates
    );
    assert!(
        !extern_crates.contains("rmp-serde"),
        "extern_crates should NOT contain `rmp-serde` when MsgPack is unused, got: {:?}",
        extern_crates
    );
}

#[test]
fn msgpack_codegen_combined_program_registers_all_deps() {
    let main = func_decl(
        "main",
        &[],
        vec![
            let_stmt(
                "bytes",
                ns_assoc_call("MsgPack", "serialize", vec![str_expr("x")]),
            ),
            let_stmt(
                "back",
                ns_assoc_call("MsgPack", "deserialize", vec![ident_expr("bytes")]),
            ),
            let_stmt(
                "ok",
                ns_assoc_call("MsgPack", "roundtrip", vec![str_expr("x")]),
            ),
        ],
    );
    let mut codegen = RustCodegen::new();
    let _ = codegen.generate(&[main]).expect("codegen must succeed");
    let extern_crates = codegen.extern_crates();
    assert!(
        extern_crates.contains("buff-msgpack"),
        "extern_crates should contain `buff-msgpack`, got: {:?}",
        extern_crates
    );
    assert!(
        extern_crates.contains("rmp-serde"),
        "extern_crates should contain `rmp-serde`, got: {:?}",
        extern_crates
    );
    assert!(
        extern_crates.contains("serde_json"),
        "extern_crates should contain `serde_json`, got: {:?}",
        extern_crates
    );
}

// ===========================================================================
// 5. Error cases - arity mismatch surfaces a clear CodegenError.
// ===========================================================================

#[test]
fn msgpack_codegen_rejects_serialize_with_zero_args() {
    let result = std::panic::catch_unwind(|| {
        let _ = codegen_one_expr_in("f", ns_assoc_call("MsgPack", "serialize", vec![]));
    });
    assert!(
        result.is_err(),
        "expected codegen to reject `MsgPack.serialize()` (no value arg)"
    );
}

#[test]
fn msgpack_codegen_rejects_serialize_with_two_args() {
    let result = std::panic::catch_unwind(|| {
        let _ = codegen_one_expr_in(
            "f",
            ns_assoc_call("MsgPack", "serialize", vec![str_expr("a"), str_expr("b")]),
        );
    });
    assert!(
        result.is_err(),
        "expected codegen to reject `MsgPack.serialize(\"a\", \"b\")` (expected 1 arg)"
    );
}

#[test]
fn msgpack_codegen_rejects_deserialize_with_zero_args() {
    let result = std::panic::catch_unwind(|| {
        let _ = codegen_one_expr_in("f", ns_assoc_call("MsgPack", "deserialize", vec![]));
    });
    assert!(
        result.is_err(),
        "expected codegen to reject `MsgPack.deserialize()` (no bytes arg)"
    );
}

#[test]
fn msgpack_codegen_rejects_roundtrip_with_zero_args() {
    let result = std::panic::catch_unwind(|| {
        let _ = codegen_one_expr_in("f", ns_assoc_call("MsgPack", "roundtrip", vec![]));
    });
    assert!(
        result.is_err(),
        "expected codegen to reject `MsgPack.roundtrip()` (no value arg)"
    );
}

// ===========================================================================
// 6. insta snapshots - byte-stable codegen pinning.
// ===========================================================================

#[test]
fn msgpack_codegen_serialize_snapshot() {
    let src = codegen_one_expr_in(
        "f",
        ns_assoc_call("MsgPack", "serialize", vec![str_expr("hello")]),
    );
    insta::assert_snapshot!(src);
}

#[test]
fn msgpack_codegen_deserialize_snapshot() {
    let src = codegen_one_expr_in(
        "f",
        ns_assoc_call("MsgPack", "deserialize", vec![ident_expr("bytes")]),
    );
    insta::assert_snapshot!(src);
}

#[test]
fn msgpack_codegen_roundtrip_snapshot() {
    let src = codegen_one_expr_in(
        "f",
        ns_assoc_call("MsgPack", "roundtrip", vec![ident_expr("value")]),
    );
    insta::assert_snapshot!(src);
}

#[test]
fn msgpack_codegen_full_program_snapshot() {
    let main = func_decl(
        "main",
        &[],
        vec![
            let_stmt(
                "bytes",
                ns_assoc_call("MsgPack", "serialize", vec![str_expr("payload")]),
            ),
            let_stmt(
                "back",
                ns_assoc_call("MsgPack", "deserialize", vec![ident_expr("bytes")]),
            ),
            let_stmt(
                "ok",
                ns_assoc_call("MsgPack", "roundtrip", vec![str_expr("payload")]),
            ),
        ],
    );
    let mut codegen = RustCodegen::new();
    let file = codegen.generate(&[main]).expect("codegen must succeed");
    let src = buff_lang_codegen_rust::format_file(&file);
    insta::assert_snapshot!(src);
}

//! T52 integration tests - buff-protobuf prelude types codegen.
//!
//! Verifies that the Rust codegen lowers the T52 protobuf surface:
//!
//! - **Protobuf** namespace (`Protobuf.serialize(value) -> Bytes`,
//!   `Protobuf.deserialize(bytes) -> Value`,
//!   `Protobuf.roundtrip(value) -> Option<Value>`)
//! - **Message** constructors (`Message.new(value) -> Message`,
//!   `Message.from_bytes(bytes) -> Message`,
//!   `Message.decode(bytes) -> Message`)
//! - **Message** instance methods (`msg.byte_size() -> Int`,
//!   `msg.type_url() -> String`, `msg.payload() -> Value`,
//!   `msg.encode() -> Vector<Byte>`)
//!
//! Each namespace function wraps the `buff_protobuf` crate's safe API
//! (the well-known `google.protobuf.Struct` schema is the dynamic
//! message surface — NO `.proto` build-time codegen in MVP). All
//! fallible calls collapse via `.unwrap_or_default()` (panic-free —
//! Value::Null on decode failure, empty Vec on encode failure, Default
//! Message on construction failure). Instance methods are infallible
//! except `payload()` which uses `.unwrap_or_default()`.
//!
//! Run via:
//!
//! ```text
//! cargo test -p buff-lang-codegen-rust --test protobuf_codegen
//! ```
//!
//! # Why AST-constructed tests (not source-parsed)
//!
//! All types here are prelude types (associated functions + instance
//! methods), so source parsing requires no new keyword / AST node —
//! the existing `MethodCall` shape handles them. We construct ASTs by
//! hand here for the same reasons `msgpack_codegen.rs` (T51),
//! `nlp_codegen.rs` (T46), `geo_codegen.rs` (T45),
//! `crypto_codegen.rs` (T124k), `fs_codegen.rs` (T124j),
//! `format_codegen.rs` (T124i), `web_codegen.rs` (T124h),
//! `system_codegen.rs` (T124g), `regex_codegen.rs` (T124d),
//! `toml_codegen.rs` (T124e), and `utility_codegen.rs` (T124f) do:
//! direct AST construction decouples the codegen-pinning snapshots from
//! any future parser-restructuring work, and lets us test specific edge
//! cases (e.g. wrong arity, ident vs literal arg) without writing Buff
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

fn ident_expr(s: &str) -> Expr {
    Expr::Ident(ident(s), span())
}

fn string_expr(s: &str) -> Expr {
    Expr::Literal(Literal::String(s.to_string()), span())
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
/// shape). The receiver is the bare namespace Ident (e.g. `Protobuf`).
fn ns_assoc_call(namespace: &str, method: &str, args: Vec<Expr>) -> Expr {
    Expr::MethodCall {
        receiver: Box::new(ident_expr(namespace)),
        method: ident(method),
        args,
        span: span(),
    }
}

/// `recv.<method>(args...)` AST node (instance-method call shape).
/// The receiver is a variable Ident (e.g. `msg`).
fn instance_call(recv: &str, method: &str, args: Vec<Expr>) -> Expr {
    Expr::MethodCall {
        receiver: Box::new(ident_expr(recv)),
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
// 1. Protobuf.serialize — one-arg, returns Vec<u8> via .unwrap_or_default().
// ===========================================================================

#[test]
fn protobuf_codegen_serialize_with_literal_calls_buff_protobuf_serialize() {
    let src = codegen_one_expr_in(
        "f",
        ns_assoc_call("Protobuf", "serialize", vec![string_expr("hello")]),
    );
    assert!(
        src.contains("buff_protobuf::serialize"),
        "expected `buff_protobuf::serialize(` in: {src}"
    );
    assert!(
        src.contains(".unwrap_or_default()"),
        "expected `.unwrap_or_default()` (panic-free Vec collapse) in: {src}"
    );
    assert!(
        !src.contains(".unwrap()"),
        "expected NO bare `.unwrap()` in Protobuf.serialize output: {src}"
    );
    must_reparse(&src);
}

#[test]
fn protobuf_codegen_serialize_with_ident_arg_splices_ident() {
    let src = codegen_one_expr_in(
        "f",
        ns_assoc_call("Protobuf", "serialize", vec![ident_expr("value")]),
    );
    assert!(
        src.contains("buff_protobuf::serialize"),
        "expected `buff_protobuf::serialize(` in: {src}"
    );
    assert!(
        src.contains("&value"),
        "expected `&value` (ident arg splice by ref) in: {src}"
    );
    assert!(
        !src.contains(".unwrap()"),
        "expected NO bare `.unwrap()` in Protobuf.serialize output: {src}"
    );
    must_reparse(&src);
}

// ===========================================================================
// 2. Protobuf.deserialize — one-arg, returns Value via .unwrap_or_default().
// ===========================================================================

#[test]
fn protobuf_codegen_deserialize_with_literal_calls_buff_protobuf_deserialize() {
    let src = codegen_one_expr_in(
        "f",
        ns_assoc_call(
            "Protobuf",
            "deserialize",
            vec![string_expr("not real protobuf")],
        ),
    );
    assert!(
        src.contains("buff_protobuf::deserialize"),
        "expected `buff_protobuf::deserialize(` in: {src}"
    );
    assert!(
        src.contains(".unwrap_or_default()"),
        "expected `.unwrap_or_default()` (panic-free Value collapse) in: {src}"
    );
    assert!(
        !src.contains(".unwrap()"),
        "expected NO bare `.unwrap()` in Protobuf.deserialize output: {src}"
    );
    must_reparse(&src);
}

#[test]
fn protobuf_codegen_deserialize_with_ident_arg_splices_ident() {
    let src = codegen_one_expr_in(
        "f",
        ns_assoc_call("Protobuf", "deserialize", vec![ident_expr("bytes")]),
    );
    assert!(
        src.contains("buff_protobuf::deserialize"),
        "expected `buff_protobuf::deserialize(` in: {src}"
    );
    assert!(
        src.contains("&bytes"),
        "expected `&bytes` (ident arg splice by ref) in: {src}"
    );
    assert!(
        !src.contains(".unwrap()"),
        "expected NO bare `.unwrap()` in Protobuf.deserialize output: {src}"
    );
    must_reparse(&src);
}

// ===========================================================================
// 3. Protobuf.roundtrip — one-arg, returns Option<Value> directly (no unwrap).
// ===========================================================================

#[test]
fn protobuf_codegen_roundtrip_with_literal_calls_buff_protobuf_roundtrip() {
    let src = codegen_one_expr_in(
        "f",
        ns_assoc_call("Protobuf", "roundtrip", vec![string_expr("hello")]),
    );
    assert!(
        src.contains("buff_protobuf::roundtrip"),
        "expected `buff_protobuf::roundtrip(` in: {src}"
    );
    // roundtrip returns Option<Value> directly — NO .unwrap_or_default()
    // collapse on the codegen side (the runtime fn is already Option).
    assert!(
        !src.contains(".unwrap_or_default()"),
        "expected NO `.unwrap_or_default()` for roundtrip in: {src}"
    );
    assert!(
        !src.contains(".unwrap()"),
        "expected NO bare `.unwrap()` in Protobuf.roundtrip output: {src}"
    );
    must_reparse(&src);
}

#[test]
fn protobuf_codegen_roundtrip_with_ident_arg_splices_ident() {
    let src = codegen_one_expr_in(
        "f",
        ns_assoc_call("Protobuf", "roundtrip", vec![ident_expr("value")]),
    );
    assert!(
        src.contains("buff_protobuf::roundtrip"),
        "expected `buff_protobuf::roundtrip(` in: {src}"
    );
    assert!(
        src.contains("&value"),
        "expected `&value` (ident arg splice by ref) in: {src}"
    );
    assert!(
        !src.contains(".unwrap()"),
        "expected NO bare `.unwrap()` in Protobuf.roundtrip output: {src}"
    );
    must_reparse(&src);
}

// ===========================================================================
// 4. Message constructors — Message.new / Message.from_bytes / Message.decode.
//    Each returns a Message via .unwrap_or_default() (panic-free — Message
//    impls Default as an empty-payload message).
// ===========================================================================

#[test]
fn protobuf_codegen_message_new_with_literal_calls_buff_protobuf_message_new() {
    let src = codegen_one_expr_in(
        "f",
        ns_assoc_call("Message", "new", vec![string_expr("payload")]),
    );
    assert!(
        src.contains("buff_protobuf::Message::new"),
        "expected `buff_protobuf::Message::new(` in: {src}"
    );
    assert!(
        src.contains(".unwrap_or_default()"),
        "expected `.unwrap_or_default()` (panic-free Message collapse) in: {src}"
    );
    assert!(
        !src.contains(".unwrap()"),
        "expected NO bare `.unwrap()` in Message.new output: {src}"
    );
    must_reparse(&src);
}

#[test]
fn protobuf_codegen_message_new_with_ident_arg_splices_ref() {
    let src = codegen_one_expr_in(
        "f",
        ns_assoc_call("Message", "new", vec![ident_expr("value")]),
    );
    assert!(
        src.contains("buff_protobuf::Message::new"),
        "expected `buff_protobuf::Message::new(` in: {src}"
    );
    assert!(
        src.contains("&value"),
        "expected `&value` (ident arg splice by ref) in: {src}"
    );
    assert!(
        !src.contains(".unwrap()"),
        "expected NO bare `.unwrap()` in Message.new output: {src}"
    );
    must_reparse(&src);
}

#[test]
fn protobuf_codegen_message_from_bytes_calls_buff_protobuf_message_from_bytes() {
    let src = codegen_one_expr_in(
        "f",
        ns_assoc_call("Message", "from_bytes", vec![ident_expr("raw_bytes")]),
    );
    assert!(
        src.contains("buff_protobuf::Message::from_bytes"),
        "expected `buff_protobuf::Message::from_bytes(` in: {src}"
    );
    assert!(
        src.contains(".unwrap_or_default()"),
        "expected `.unwrap_or_default()` (panic-free Message collapse) in: {src}"
    );
    assert!(
        !src.contains(".unwrap()"),
        "expected NO bare `.unwrap()` in Message.from_bytes output: {src}"
    );
    must_reparse(&src);
}

#[test]
fn protobuf_codegen_message_decode_calls_buff_protobuf_message_decode() {
    let src = codegen_one_expr_in(
        "f",
        ns_assoc_call("Message", "decode", vec![ident_expr("bytes")]),
    );
    assert!(
        src.contains("buff_protobuf::Message::decode"),
        "expected `buff_protobuf::Message::decode(` in: {src}"
    );
    assert!(
        src.contains(".unwrap_or_default()"),
        "expected `.unwrap_or_default()` (panic-free Message collapse) in: {src}"
    );
    assert!(
        !src.contains(".unwrap()"),
        "expected NO bare `.unwrap()` in Message.decode output: {src}"
    );
    must_reparse(&src);
}

// ===========================================================================
// 5. Message instance methods — byte_size / type_url / payload / encode.
// ===========================================================================

#[test]
fn protobuf_codegen_message_byte_size_calls_method_and_casts_to_i64() {
    let src = codegen_one_expr_in("f", instance_call("msg", "byte_size", vec![]));
    assert!(
        src.contains(".byte_size()"),
        "expected `.byte_size()` in: {src}"
    );
    assert!(
        src.contains(" as i64"),
        "expected ` as i64` (cast usize -> Int<64>) in: {src}"
    );
    assert!(
        !src.contains(".unwrap()"),
        "expected NO bare `.unwrap()` in Message.byte_size output: {src}"
    );
    must_reparse(&src);
}

#[test]
fn protobuf_codegen_message_type_url_calls_method_and_lifts_to_string() {
    let src = codegen_one_expr_in("f", instance_call("msg", "type_url", vec![]));
    assert!(
        src.contains(".type_url()"),
        "expected `.type_url()` in: {src}"
    );
    assert!(
        src.contains(".to_string()"),
        "expected `.to_string()` (lift &str -> String per FFI guide R2) in: {src}"
    );
    assert!(
        !src.contains(".unwrap()"),
        "expected NO bare `.unwrap()` in Message.type_url output: {src}"
    );
    must_reparse(&src);
}

#[test]
fn protobuf_codegen_message_payload_calls_method_with_unwrap_or_default() {
    let src = codegen_one_expr_in("f", instance_call("msg", "payload", vec![]));
    assert!(
        src.contains(".payload()"),
        "expected `.payload()` in: {src}"
    );
    assert!(
        src.contains(".unwrap_or_default()"),
        "expected `.unwrap_or_default()` (panic-free Value::Null collapse) in: {src}"
    );
    assert!(
        !src.contains(".unwrap()"),
        "expected NO bare `.unwrap()` in Message.payload output: {src}"
    );
    must_reparse(&src);
}

#[test]
fn protobuf_codegen_message_encode_calls_method_and_lifts_to_vec() {
    let src = codegen_one_expr_in("f", instance_call("msg", "encode", vec![]));
    assert!(src.contains(".encode()"), "expected `.encode()` in: {src}");
    assert!(
        src.contains(".to_vec()"),
        "expected `.to_vec()` (lift &[u8] -> Vec<u8> per FFI guide R2) in: {src}"
    );
    assert!(
        !src.contains(".unwrap()"),
        "expected NO bare `.unwrap()` in Message.encode output: {src}"
    );
    must_reparse(&src);
}

// ===========================================================================
// 6. extern_crates registration — Protobuf + Message walkers flag
//    buff-protobuf + prost + prost-types + serde_json.
// ===========================================================================

#[test]
fn protobuf_codegen_registers_buff_protobuf_and_deps_for_serialize() {
    let main = func_decl(
        "main",
        &[],
        vec![expr_stmt(ns_assoc_call(
            "Protobuf",
            "serialize",
            vec![string_expr("hi")],
        ))],
    );
    let mut codegen = RustCodegen::new();
    let _ = codegen.generate(&[main]).expect("codegen must succeed");
    let extern_crates = codegen.extern_crates();
    assert!(
        extern_crates.contains("buff-protobuf"),
        "extern_crates should contain `buff-protobuf`, got: {:?}",
        extern_crates
    );
    assert!(
        extern_crates.contains("prost"),
        "extern_crates should contain `prost`, got: {:?}",
        extern_crates
    );
    assert!(
        extern_crates.contains("prost-types"),
        "extern_crates should contain `prost-types`, got: {:?}",
        extern_crates
    );
    assert!(
        extern_crates.contains("serde_json"),
        "extern_crates should contain `serde_json`, got: {:?}",
        extern_crates
    );
}

#[test]
fn protobuf_codegen_registers_buff_protobuf_and_deps_for_deserialize() {
    let main = func_decl(
        "main",
        &[],
        vec![expr_stmt(ns_assoc_call(
            "Protobuf",
            "deserialize",
            vec![ident_expr("bytes")],
        ))],
    );
    let mut codegen = RustCodegen::new();
    let _ = codegen.generate(&[main]).expect("codegen must succeed");
    let extern_crates = codegen.extern_crates();
    assert!(
        extern_crates.contains("buff-protobuf"),
        "extern_crates should contain `buff-protobuf`, got: {:?}",
        extern_crates
    );
    assert!(
        extern_crates.contains("prost"),
        "extern_crates should contain `prost`, got: {:?}",
        extern_crates
    );
}

#[test]
fn protobuf_codegen_registers_buff_protobuf_and_deps_for_roundtrip() {
    let main = func_decl(
        "main",
        &[],
        vec![expr_stmt(ns_assoc_call(
            "Protobuf",
            "roundtrip",
            vec![ident_expr("value")],
        ))],
    );
    let mut codegen = RustCodegen::new();
    let _ = codegen.generate(&[main]).expect("codegen must succeed");
    let extern_crates = codegen.extern_crates();
    assert!(
        extern_crates.contains("buff-protobuf"),
        "extern_crates should contain `buff-protobuf`, got: {:?}",
        extern_crates
    );
    assert!(
        extern_crates.contains("serde_json"),
        "extern_crates should contain `serde_json`, got: {:?}",
        extern_crates
    );
}

#[test]
fn protobuf_codegen_registers_buff_protobuf_via_message_namespace() {
    // Using Message.* (not Protobuf.*) should ALSO register buff-protobuf
    // because the walker checks BOTH namespaces (a Message value arises
    // only via Message.new which encodes via Protobuf.serialize internally).
    let main = func_decl(
        "main",
        &[],
        vec![expr_stmt(ns_assoc_call(
            "Message",
            "new",
            vec![ident_expr("value")],
        ))],
    );
    let mut codegen = RustCodegen::new();
    let _ = codegen.generate(&[main]).expect("codegen must succeed");
    let extern_crates = codegen.extern_crates();
    assert!(
        extern_crates.contains("buff-protobuf"),
        "extern_crates should contain `buff-protobuf` (via Message walker), got: {:?}",
        extern_crates
    );
    assert!(
        extern_crates.contains("prost"),
        "extern_crates should contain `prost` (via Message walker), got: {:?}",
        extern_crates
    );
}

#[test]
fn protobuf_codegen_no_extern_crate_when_unused() {
    let main = func_decl(
        "main",
        &[],
        vec![expr_stmt(Expr::FuncCall {
            callee: Box::new(ident_expr("print")),
            args: vec![string_expr("hi")],
            span: span(),
        })],
    );
    let mut codegen = RustCodegen::new();
    let _ = codegen.generate(&[main]).expect("codegen must succeed");
    let extern_crates = codegen.extern_crates();
    assert!(
        !extern_crates.contains("buff-protobuf"),
        "extern_crates should NOT contain `buff-protobuf` when Protobuf is unused, got: {:?}",
        extern_crates
    );
    assert!(
        !extern_crates.contains("prost"),
        "extern_crates should NOT contain `prost` when Protobuf is unused, got: {:?}",
        extern_crates
    );
}

#[test]
fn protobuf_codegen_combined_program_registers_all_deps() {
    let main = func_decl(
        "main",
        &[],
        vec![
            let_stmt(
                "bytes",
                ns_assoc_call("Protobuf", "serialize", vec![string_expr("x")]),
            ),
            let_stmt(
                "back",
                ns_assoc_call("Protobuf", "deserialize", vec![ident_expr("bytes")]),
            ),
            let_stmt(
                "ok",
                ns_assoc_call("Protobuf", "roundtrip", vec![string_expr("x")]),
            ),
            let_stmt(
                "msg",
                ns_assoc_call("Message", "new", vec![string_expr("x")]),
            ),
        ],
    );
    let mut codegen = RustCodegen::new();
    let _ = codegen.generate(&[main]).expect("codegen must succeed");
    let extern_crates = codegen.extern_crates();
    assert!(
        extern_crates.contains("buff-protobuf"),
        "extern_crates should contain `buff-protobuf`, got: {:?}",
        extern_crates
    );
    assert!(
        extern_crates.contains("prost"),
        "extern_crates should contain `prost`, got: {:?}",
        extern_crates
    );
    assert!(
        extern_crates.contains("prost-types"),
        "extern_crates should contain `prost-types`, got: {:?}",
        extern_crates
    );
    assert!(
        extern_crates.contains("serde_json"),
        "extern_crates should contain `serde_json`, got: {:?}",
        extern_crates
    );
}

// ===========================================================================
// 7. Error cases — arity mismatch surfaces a clear CodegenError.
// ===========================================================================

#[test]
fn protobuf_codegen_rejects_serialize_with_zero_args() {
    let result = std::panic::catch_unwind(|| {
        let _ = codegen_one_expr_in("f", ns_assoc_call("Protobuf", "serialize", vec![]));
    });
    assert!(
        result.is_err(),
        "expected codegen to reject `Protobuf.serialize()` (no value arg)"
    );
}

#[test]
fn protobuf_codegen_rejects_serialize_with_two_args() {
    let result = std::panic::catch_unwind(|| {
        let _ = codegen_one_expr_in(
            "f",
            ns_assoc_call(
                "Protobuf",
                "serialize",
                vec![string_expr("a"), string_expr("b")],
            ),
        );
    });
    assert!(
        result.is_err(),
        "expected codegen to reject `Protobuf.serialize(\"a\", \"b\")` (expected 1 arg)"
    );
}

#[test]
fn protobuf_codegen_rejects_deserialize_with_zero_args() {
    let result = std::panic::catch_unwind(|| {
        let _ = codegen_one_expr_in("f", ns_assoc_call("Protobuf", "deserialize", vec![]));
    });
    assert!(
        result.is_err(),
        "expected codegen to reject `Protobuf.deserialize()` (no bytes arg)"
    );
}

#[test]
fn protobuf_codegen_rejects_roundtrip_with_zero_args() {
    let result = std::panic::catch_unwind(|| {
        let _ = codegen_one_expr_in("f", ns_assoc_call("Protobuf", "roundtrip", vec![]));
    });
    assert!(
        result.is_err(),
        "expected codegen to reject `Protobuf.roundtrip()` (no value arg)"
    );
}

#[test]
fn protobuf_codegen_rejects_message_new_with_zero_args() {
    let result = std::panic::catch_unwind(|| {
        let _ = codegen_one_expr_in("f", ns_assoc_call("Message", "new", vec![]));
    });
    assert!(
        result.is_err(),
        "expected codegen to reject `Message.new()` (no value arg)"
    );
}

// ===========================================================================
// 8. insta snapshots — byte-stable codegen pinning.
// ===========================================================================

#[test]
fn protobuf_codegen_serialize_snapshot() {
    let src = codegen_one_expr_in(
        "f",
        ns_assoc_call("Protobuf", "serialize", vec![string_expr("hello")]),
    );
    insta::assert_snapshot!(src);
}

#[test]
fn protobuf_codegen_deserialize_snapshot() {
    let src = codegen_one_expr_in(
        "f",
        ns_assoc_call("Protobuf", "deserialize", vec![ident_expr("bytes")]),
    );
    insta::assert_snapshot!(src);
}

#[test]
fn protobuf_codegen_roundtrip_snapshot() {
    let src = codegen_one_expr_in(
        "f",
        ns_assoc_call("Protobuf", "roundtrip", vec![ident_expr("value")]),
    );
    insta::assert_snapshot!(src);
}

#[test]
fn protobuf_codegen_message_new_snapshot() {
    let src = codegen_one_expr_in(
        "f",
        ns_assoc_call("Message", "new", vec![ident_expr("value")]),
    );
    insta::assert_snapshot!(src);
}

#[test]
fn protobuf_codegen_message_byte_size_snapshot() {
    let src = codegen_one_expr_in("f", instance_call("msg", "byte_size", vec![]));
    insta::assert_snapshot!(src);
}

#[test]
fn protobuf_codegen_message_type_url_snapshot() {
    let src = codegen_one_expr_in("f", instance_call("msg", "type_url", vec![]));
    insta::assert_snapshot!(src);
}

#[test]
fn protobuf_codegen_message_payload_snapshot() {
    let src = codegen_one_expr_in("f", instance_call("msg", "payload", vec![]));
    insta::assert_snapshot!(src);
}

#[test]
fn protobuf_codegen_message_encode_snapshot() {
    let src = codegen_one_expr_in("f", instance_call("msg", "encode", vec![]));
    insta::assert_snapshot!(src);
}

#[test]
fn protobuf_codegen_full_program_snapshot() {
    let main = func_decl(
        "main",
        &[],
        vec![
            let_stmt(
                "bytes",
                ns_assoc_call("Protobuf", "serialize", vec![string_expr("payload")]),
            ),
            let_stmt(
                "back",
                ns_assoc_call("Protobuf", "deserialize", vec![ident_expr("bytes")]),
            ),
            let_stmt(
                "msg",
                ns_assoc_call("Message", "new", vec![string_expr("payload")]),
            ),
            let_stmt(
                "m2",
                ns_assoc_call("Message", "from_bytes", vec![ident_expr("bytes")]),
            ),
            expr_stmt(instance_call("msg", "byte_size", vec![])),
            expr_stmt(instance_call("msg", "type_url", vec![])),
            expr_stmt(instance_call("msg", "payload", vec![])),
            expr_stmt(instance_call("msg", "encode", vec![])),
        ],
    );
    let mut codegen = RustCodegen::new();
    let file = codegen.generate(&[main]).expect("codegen must succeed");
    let src = buff_lang_codegen_rust::format_file(&file);
    insta::assert_snapshot!(src);
}

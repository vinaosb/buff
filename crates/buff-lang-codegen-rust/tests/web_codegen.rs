//! T124h integration tests - web prelude modules codegen.
//!
//! Verifies that the Rust codegen lowers the five T124h web modules:
//!
//! - **Base64** namespace (`Base64.encode(bytes)`, `Base64.decode(s)`) -
//!   wraps the `base64` Rust crate (STANDARD engine via UFCS so the
//!   `Engine` trait need not be imported at the call site).
//! - **Hex** namespace (`Hex.encode(bytes)`, `Hex.decode(s)`) - wraps
//!   the `hex` Rust crate.
//! - **URLEncode** namespace (`URLEncode.encode(s)`,
//!   `URLEncode.decode(s)`) - wraps the `percent-encoding` Rust crate
//!   (`utf8_percent_encode` + `NON_ALPHANUMERIC` AsciiSet).
//! - **UUID** namespace (`UUID.v4()`, `UUID.v7()`, `UUID.parse(s)`) -
//!   wraps the `uuid` Rust crate. Returns String/Bool (NOT a typed
//!   Uuid value) - Buff surfaces UUIDs as their canonical String form.
//! - **URL** value type (`URL.parse(s) -> URL`, instance accessors
//!   `.scheme` / `.host` / `.path` (zero-arg, return String), and
//!   `.query(key) -> Option<String>`) - wraps the `url` Rust crate.
//!
//! Acceptance snapshots for the canonical criteria (per the task spec):
//!
//! ```text
//! Base64.encode(bytes)          -> base64::Engine::encode(&STANDARD, bytes)
//! Base64.decode(s)              -> base64::Engine::decode(&STANDARD, s).unwrap_or_default()
//! Hex.encode(bytes)             -> hex::encode(bytes)
//! Hex.decode(s)                 -> hex::decode(s).unwrap_or_default()
//! URLEncode.encode(s)           -> utf8_percent_encode(s, NON_ALPHANUMERIC).to_string()
//! URLEncode.decode(s)           -> percent_decode_str(s).decode_utf8_lossy().into_owned()
//! UUID.v4()                     -> uuid::Uuid::new_v4().to_string()
//! UUID.v7()                     -> uuid::Uuid::now_v7().to_string()
//! UUID.parse(s)                 -> uuid::Uuid::parse_str(s).is_ok()
//! URL.parse("https://a.com/b?q=1") -> url::Url::parse(s).unwrap_or_else(|_| url::Url::parse("about:blank").unwrap())
//! url.scheme                    -> recv.scheme().to_string()
//! url.host                      -> recv.host_str().unwrap_or_default().to_string()
//! url.path                      -> recv.path().to_string()
//! url.query("key")              -> recv.query_pairs().find(...).map(...)
//! ```
//!
//! Run via:
//!
//! ```text
//! cargo test -p buff-lang-codegen-rust --test web_codegen
//! ```
//!
//! # Why AST-constructed tests (not source-parsed)
//!
//! All five modules are prelude namespaces (or a runtime-value type
//! constructed via a prelude assoc fn), so source parsing requires no
//! new keyword / AST node - the existing `MethodCall` shape handles
//! them. We construct ASTs by hand here for the same reasons
//! `system_codegen.rs` (T124g), `regex_codegen.rs` (T124d),
//! `toml_codegen.rs` (T124e), and `utility_codegen.rs` (T124f) do:
//! direct AST construction decouples the codegen-pinning snapshots
//! from any future parser-restructuring work, and lets us test
//! specific edge cases (e.g. wrong arity, ident vs literal arg,
//! receiver inference for instance methods) without writing Buff
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

fn int_expr(n: i64) -> Expr {
    Expr::Literal(Literal::Int(n), span())
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

/// `<Namespace>.<method>(args...)` AST node (associated-function call shape).
/// The receiver is the bare namespace Ident.
fn ns_assoc_call(namespace: &str, method: &str, args: Vec<Expr>) -> Expr {
    Expr::MethodCall {
        receiver: Box::new(ident_expr(namespace)),
        method: ident(method),
        args,
        span: span(),
    }
}

/// `recv.<method>(args...)` AST node (instance-method call shape).
fn instance_call(recv: Expr, method: &str, args: Vec<Expr>) -> Expr {
    Expr::MethodCall {
        receiver: Box::new(recv),
        method: ident(method),
        args,
        span: span(),
    }
}

/// `Vector<Byte>` literal: `[0u8, 1u8, ...]` lowered as an array of Int
/// literals (the codegen will lift them to bytes via the Vector<T>
/// lowering). Used as the `bytes` arg for Base64.encode / Hex.encode.
/// The exact values don't matter for snapshot-stable codegen - the
/// codegen just splices the lowered expression into the call site.
fn bytes_expr() -> Expr {
    Expr::ArrayLit {
        elements: vec![int_expr(72), int_expr(105)], // b"Hi"
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
// 1. Base64 module - associated functions (encode / decode).
// ===========================================================================

#[test]
fn base64_codegen_encode_uses_standard_engine_ufcs() {
    // Base64.encode(bytes) -> base64::Engine::encode(&STANDARD, bytes).
    // UFCS form so the `Engine` trait need not be imported at the call
    // site - generated code requires NO `use base64::Engine as _;`.
    let src = codegen_one_expr_in("f", ns_assoc_call("Base64", "encode", vec![bytes_expr()]));
    assert!(
        src.contains("base64::Engine::encode("),
        "expected `base64::Engine::encode(` (UFCS form) in: {src}"
    );
    assert!(
        src.contains("base64::engine::general_purpose::STANDARD"),
        "expected STANDARD engine path in: {src}"
    );
    must_reparse(&src);
}

#[test]
fn base64_codegen_decode_unwrap_or_default() {
    // Base64.decode(s) -> base64::Engine::decode(&STANDARD, s).unwrap_or_default().
    // Acceptance criterion: NEVER panics on invalid input (empty Vec
    // fallback), matching Buff's "no panicking generated code" rule.
    let src = codegen_one_expr_in(
        "f",
        ns_assoc_call("Base64", "decode", vec![str_expr("aGVsbG8=")]),
    );
    assert!(
        src.contains("base64::Engine::decode("),
        "expected `base64::Engine::decode(` (UFCS form) in: {src}"
    );
    assert!(
        src.contains(".unwrap_or_default()"),
        "expected `.unwrap_or_default()` (panic-free fallback) in: {src}"
    );
    // Must NOT use bare .unwrap() (panicking-generated-code rule).
    assert!(
        !src.contains(".unwrap()"),
        "expected NO bare `.unwrap()` in Base64.decode output: {src}"
    );
    must_reparse(&src);
}

#[test]
fn base64_codegen_decode_ident_arg_borrows() {
    // Base64.decode(my_string_var) - non-literal arg borrows via &.
    let src = codegen_one_expr_in(
        "f",
        ns_assoc_call("Base64", "decode", vec![ident_expr("my_string_var")]),
    );
    assert!(
        src.contains("&my_string_var"),
        "expected `&my_string_var` (borrow coercion for String -> &[u8]) in: {src}"
    );
    must_reparse(&src);
}

#[test]
fn base64_codegen_registers_base64_extern_crate() {
    let main = func_decl(
        "main",
        &[],
        vec![expr_stmt(ns_assoc_call(
            "Base64",
            "encode",
            vec![bytes_expr()],
        ))],
    );
    let mut codegen = RustCodegen::new();
    let _ = codegen.generate(&[main]).expect("codegen must succeed");
    let extern_crates = codegen.extern_crates();
    assert!(
        extern_crates.contains("base64"),
        "extern_crates should contain `base64`, got: {:?}",
        extern_crates
    );
}

// ===========================================================================
// 2. Hex module - associated functions (encode / decode).
// ===========================================================================

#[test]
fn hex_codegen_encode_calls_hex_encode() {
    // Hex.encode(bytes) -> hex::encode(bytes).
    let src = codegen_one_expr_in("f", ns_assoc_call("Hex", "encode", vec![bytes_expr()]));
    assert!(
        src.contains("hex::encode("),
        "expected `hex::encode(` in: {src}"
    );
    must_reparse(&src);
}

#[test]
fn hex_codegen_decode_unwrap_or_default() {
    // Hex.decode(s) -> hex::decode(s).unwrap_or_default().
    let src = codegen_one_expr_in(
        "f",
        ns_assoc_call("Hex", "decode", vec![str_expr("deadbeef")]),
    );
    assert!(
        src.contains("hex::decode("),
        "expected `hex::decode(` in: {src}"
    );
    assert!(
        src.contains(".unwrap_or_default()"),
        "expected `.unwrap_or_default()` (panic-free fallback) in: {src}"
    );
    assert!(
        !src.contains(".unwrap()"),
        "expected NO bare `.unwrap()` in Hex.decode output: {src}"
    );
    must_reparse(&src);
}

#[test]
fn hex_codegen_registers_hex_extern_crate() {
    let main = func_decl(
        "main",
        &[],
        vec![expr_stmt(ns_assoc_call(
            "Hex",
            "encode",
            vec![bytes_expr()],
        ))],
    );
    let mut codegen = RustCodegen::new();
    let _ = codegen.generate(&[main]).expect("codegen must succeed");
    let extern_crates = codegen.extern_crates();
    assert!(
        extern_crates.contains("hex"),
        "extern_crates should contain `hex`, got: {:?}",
        extern_crates
    );
}

// ===========================================================================
// 3. URLEncode module - associated functions (encode / decode).
// ===========================================================================

#[test]
fn urlencode_codegen_encode_uses_utf8_percent_encode() {
    // URLEncode.encode(s) ->
    //   percent_encoding::utf8_percent_encode(s, NON_ALPHANUMERIC).to_string().
    let src = codegen_one_expr_in(
        "f",
        ns_assoc_call("URLEncode", "encode", vec![str_expr("foo bar?")]),
    );
    assert!(
        src.contains("percent_encoding::utf8_percent_encode("),
        "expected `percent_encoding::utf8_percent_encode(` in: {src}"
    );
    assert!(
        src.contains("percent_encoding::NON_ALPHANUMERIC"),
        "expected NON_ALPHANUMERIC AsciiSet in: {src}"
    );
    assert!(
        src.contains(".to_string()"),
        "expected `.to_string()` (lift PercentEncode -> String) in: {src}"
    );
    must_reparse(&src);
}

#[test]
fn urlencode_codegen_decode_uses_percent_decode_str_lossy() {
    // URLEncode.decode(s) ->
    //   percent_encoding::percent_decode_str(s).decode_utf8_lossy().into_owned().
    let src = codegen_one_expr_in(
        "f",
        ns_assoc_call("URLEncode", "decode", vec![str_expr("foo%20bar")]),
    );
    assert!(
        src.contains("percent_encoding::percent_decode_str("),
        "expected `percent_encoding::percent_decode_str(` in: {src}"
    );
    assert!(
        src.contains(".decode_utf8_lossy()"),
        "expected `.decode_utf8_lossy()` (lossy decode, NEVER panics) in: {src}"
    );
    assert!(
        src.contains(".into_owned()"),
        "expected `.into_owned()` (lift Cow<str> -> String) in: {src}"
    );
    must_reparse(&src);
}

#[test]
fn urlencode_codegen_registers_percent_encoding_extern_crate() {
    let main = func_decl(
        "main",
        &[],
        vec![expr_stmt(ns_assoc_call(
            "URLEncode",
            "encode",
            vec![str_expr("foo bar")],
        ))],
    );
    let mut codegen = RustCodegen::new();
    let _ = codegen.generate(&[main]).expect("codegen must succeed");
    let extern_crates = codegen.extern_crates();
    assert!(
        extern_crates.contains("percent-encoding"),
        "extern_crates should contain `percent-encoding`, got: {:?}",
        extern_crates
    );
}

// ===========================================================================
// 4. UUID module - associated functions (v4 / v7 / parse).
// ===========================================================================

#[test]
fn uuid_codegen_v4_returns_string() {
    // UUID.v4() -> uuid::Uuid::new_v4().to_string().
    let src = codegen_one_expr_in("f", ns_assoc_call("UUID", "v4", vec![]));
    assert!(
        src.contains("uuid::Uuid::new_v4()"),
        "expected `uuid::Uuid::new_v4()` in: {src}"
    );
    assert!(
        src.contains(".to_string()"),
        "expected `.to_string()` (surface returns String, NOT Uuid) in: {src}"
    );
    must_reparse(&src);
}

#[test]
fn uuid_codegen_v7_returns_string() {
    // UUID.v7() -> uuid::Uuid::now_v7().to_string().
    let src = codegen_one_expr_in("f", ns_assoc_call("UUID", "v7", vec![]));
    assert!(
        src.contains("uuid::Uuid::now_v7()"),
        "expected `uuid::Uuid::now_v7()` in: {src}"
    );
    assert!(
        src.contains(".to_string()"),
        "expected `.to_string()` (surface returns String) in: {src}"
    );
    must_reparse(&src);
}

#[test]
fn uuid_codegen_parse_returns_bool() {
    // UUID.parse(s) -> uuid::Uuid::parse_str(s).is_ok().
    let src = codegen_one_expr_in(
        "f",
        ns_assoc_call(
            "UUID",
            "parse",
            vec![str_expr("550e8400-e29b-41d4-a716-446655440000")],
        ),
    );
    assert!(
        src.contains("uuid::Uuid::parse_str("),
        "expected `uuid::Uuid::parse_str(` in: {src}"
    );
    assert!(
        src.contains(".is_ok()"),
        "expected `.is_ok()` (returns Bool) in: {src}"
    );
    must_reparse(&src);
}

#[test]
fn uuid_codegen_registers_uuid_extern_crate() {
    let main = func_decl(
        "main",
        &[],
        vec![expr_stmt(ns_assoc_call("UUID", "v4", vec![]))],
    );
    let mut codegen = RustCodegen::new();
    let _ = codegen.generate(&[main]).expect("codegen must succeed");
    let extern_crates = codegen.extern_crates();
    assert!(
        extern_crates.contains("uuid"),
        "extern_crates should contain `uuid`, got: {:?}",
        extern_crates
    );
}

// ===========================================================================
// 5. URL module - associated function (parse) + instance accessors.
// ===========================================================================

#[test]
fn url_codegen_parse_uses_about_blank_fallback() {
    // URL.parse("https://a.com/b?q=1") ->
    //   url::Url::parse(s).unwrap_or_else(|_| url::Url::parse("about:blank").unwrap()).
    let src = codegen_one_expr_in(
        "f",
        ns_assoc_call("URL", "parse", vec![str_expr("https://a.com/b?q=1")]),
    );
    assert!(
        src.contains("url::Url::parse("),
        "expected `url::Url::parse(` in: {src}"
    );
    assert!(
        src.contains(".unwrap_or_else(|_| url::Url::parse(\"about:blank\").unwrap())"),
        "expected `about:blank` infallible fallback in: {src}"
    );
    must_reparse(&src);
}

#[test]
fn url_codegen_registers_url_extern_crate_via_parse() {
    let main = func_decl(
        "main",
        &[],
        vec![let_stmt(
            "u",
            ns_assoc_call("URL", "parse", vec![str_expr("https://a.com")]),
        )],
    );
    let mut codegen = RustCodegen::new();
    let _ = codegen.generate(&[main]).expect("codegen must succeed");
    let extern_crates = codegen.extern_crates();
    assert!(
        extern_crates.contains("url"),
        "extern_crates should contain `url`, got: {:?}",
        extern_crates
    );
}

// ---------------------------------------------------------------------------
// URL instance accessors. The receiver must be a value of type `URL`
// (constructed via `URL.parse(...)` and bound via `let u = ...`) so the
// type inferencer can resolve it to `Type::Url` and the codegen's
// `instance_fn_lookup` arm dispatches to `lower_prelude_type_instance_fn`.
// ---------------------------------------------------------------------------

/// Build a typical URL-using function body: `let u = URL.parse(...)` then
/// one extra expr_stmt the test slots in. Returns the stmts vec.
fn url_body_with_extra(url_str: &str, extra: Expr) -> Vec<Stmt> {
    vec![
        let_stmt("u", ns_assoc_call("URL", "parse", vec![str_expr(url_str)])),
        expr_stmt(extra),
    ]
}

#[test]
fn url_codegen_scheme_accessor() {
    // url.scheme -> recv.scheme().to_string().
    let src = codegen_stmts_in(
        "f",
        url_body_with_extra(
            "https://a.com/b",
            instance_call(ident_expr("u"), "scheme", vec![]),
        ),
    );
    assert!(
        src.contains(".scheme()"),
        "expected `.scheme()` (Rust accessor) in: {src}"
    );
    assert!(
        src.contains(".to_string()"),
        "expected `.to_string()` (lift &str -> String) in: {src}"
    );
    must_reparse(&src);
}

#[test]
fn url_codegen_host_accessor_unwrap_or_default() {
    // url.host -> recv.host_str().unwrap_or_default().to_string().
    // Acceptance criterion: NEVER panics on URLs with no host (mailto:,
    // file:, ...). The unwrap_or_default yields "" when host_str
    // returns None.
    let src = codegen_stmts_in(
        "f",
        url_body_with_extra(
            "https://a.com/b",
            instance_call(ident_expr("u"), "host", vec![]),
        ),
    );
    assert!(
        src.contains(".host_str()"),
        "expected `.host_str()` (Rust accessor returning Option<&str>) in: {src}"
    );
    assert!(
        src.contains(".unwrap_or_default()"),
        "expected `.unwrap_or_default()` (empty String when host is None - NEVER panics) in: {src}"
    );
    assert!(
        src.contains(".to_string()"),
        "expected `.to_string()` in: {src}"
    );
    must_reparse(&src);
}

#[test]
fn url_codegen_path_accessor() {
    // url.path -> recv.path().to_string().
    let src = codegen_stmts_in(
        "f",
        url_body_with_extra(
            "https://a.com/b",
            instance_call(ident_expr("u"), "path", vec![]),
        ),
    );
    assert!(
        src.contains(".path()"),
        "expected `.path()` (Rust accessor) in: {src}"
    );
    assert!(
        src.contains(".to_string()"),
        "expected `.to_string()` in: {src}"
    );
    must_reparse(&src);
}

#[test]
fn url_codegen_query_accessor_returns_option_string() {
    // url.query("key") ->
    //   recv.query_pairs().find(|(k, _)| *k == key.to_string())
    //       .map(|(_, v)| v.into_owned()).
    let src = codegen_stmts_in(
        "f",
        url_body_with_extra(
            "https://a.com/b?q=1",
            instance_call(ident_expr("u"), "query", vec![str_expr("q")]),
        ),
    );
    assert!(
        src.contains(".query_pairs("),
        "expected `.query_pairs(` (Rust accessor returning Parse iterator) in: {src}"
    );
    assert!(
        src.contains(".find("),
        "expected `.find(` (linear scan for the key) in: {src}"
    );
    assert!(
        src.contains(".map("),
        "expected `.map(` (lift Some(Cow<str>) -> Some(String)) in: {src}"
    );
    assert!(
        src.contains(".into_owned()"),
        "expected `.into_owned()` (Cow<str> -> String) in: {src}"
    );
    must_reparse(&src);
}

#[test]
fn url_codegen_registers_url_extern_crate_via_instance_accessor() {
    // The instance-accessor walker should ALSO register `url` (not just
    // the parse walker) - any URL instance method call requires the
    // url crate, even if URL.parse is not directly called in the program
    // (e.g. the URL value comes from a function param). We test by
    // calling .scheme on a parameter-typed receiver.
    let main = func_decl(
        "main",
        &[],
        url_body_with_extra(
            "https://a.com/b",
            instance_call(ident_expr("u"), "scheme", vec![]),
        ),
    );
    let mut codegen = RustCodegen::new();
    let _ = codegen.generate(&[main]).expect("codegen must succeed");
    let extern_crates = codegen.extern_crates();
    assert!(
        extern_crates.contains("url"),
        "extern_crates should contain `url` (URL.parse walker), got: {:?}",
        extern_crates
    );
}

// ===========================================================================
// 6. Error cases - arity mismatch surfaces a clear CodegenError.
// ===========================================================================

#[test]
fn base64_codegen_rejects_encode_with_wrong_arity() {
    // Base64.encode() with no args - should error.
    let result = std::panic::catch_unwind(|| {
        let _ = codegen_one_expr_in("f", ns_assoc_call("Base64", "encode", vec![]));
    });
    assert!(
        result.is_err(),
        "expected codegen to reject `Base64.encode()` (no bytes arg)"
    );
}

#[test]
fn base64_codegen_rejects_decode_with_wrong_arity() {
    // Base64.decode() with no args - should error.
    let result = std::panic::catch_unwind(|| {
        let _ = codegen_one_expr_in("f", ns_assoc_call("Base64", "decode", vec![]));
    });
    assert!(
        result.is_err(),
        "expected codegen to reject `Base64.decode()` (no string arg)"
    );
}

#[test]
fn uuid_codegen_rejects_v4_with_args() {
    // UUID.v4(extra) with args - should error.
    let result = std::panic::catch_unwind(|| {
        let _ = codegen_one_expr_in("f", ns_assoc_call("UUID", "v4", vec![int_expr(1)]));
    });
    assert!(
        result.is_err(),
        "expected codegen to reject `UUID.v4(1)` (expected 0 args)"
    );
}

// Note: there's no `url_codegen_rejects_query_with_wrong_arity` test
// here because the T26 field-access heuristic catches zero-arg
// `url.query` (turning it into a Rust field access) BEFORE the
// prelude-instance-fn dispatch arm runs. The Query arm requires
// exactly 1 arg, so a one-arg `url.query("k")` correctly bypasses
// the heuristic (args not empty); zero-arg `url.query` is an
// unrealistic user scenario (the parser would surface a clear error
// for `url.query()` from source). Same trade-off as `regex.match()`
// which would have the same issue if it took zero args.

// ===========================================================================
// 7. insta snapshots - byte-stable codegen pinning.
// ===========================================================================

#[test]
fn base64_codegen_encode_snapshot() {
    let src = codegen_one_expr_in("f", ns_assoc_call("Base64", "encode", vec![bytes_expr()]));
    insta::assert_snapshot!(src);
}

#[test]
fn base64_codegen_decode_snapshot() {
    let src = codegen_one_expr_in(
        "f",
        ns_assoc_call("Base64", "decode", vec![str_expr("aGVsbG8=")]),
    );
    insta::assert_snapshot!(src);
}

#[test]
fn hex_codegen_encode_snapshot() {
    let src = codegen_one_expr_in("f", ns_assoc_call("Hex", "encode", vec![bytes_expr()]));
    insta::assert_snapshot!(src);
}

#[test]
fn hex_codegen_decode_snapshot() {
    let src = codegen_one_expr_in(
        "f",
        ns_assoc_call("Hex", "decode", vec![str_expr("deadbeef")]),
    );
    insta::assert_snapshot!(src);
}

#[test]
fn urlencode_codegen_encode_snapshot() {
    let src = codegen_one_expr_in(
        "f",
        ns_assoc_call("URLEncode", "encode", vec![str_expr("foo bar?")]),
    );
    insta::assert_snapshot!(src);
}

#[test]
fn urlencode_codegen_decode_snapshot() {
    let src = codegen_one_expr_in(
        "f",
        ns_assoc_call("URLEncode", "decode", vec![str_expr("foo%20bar")]),
    );
    insta::assert_snapshot!(src);
}

#[test]
fn uuid_codegen_v4_snapshot() {
    let src = codegen_one_expr_in("f", ns_assoc_call("UUID", "v4", vec![]));
    insta::assert_snapshot!(src);
}

#[test]
fn uuid_codegen_v7_snapshot() {
    let src = codegen_one_expr_in("f", ns_assoc_call("UUID", "v7", vec![]));
    insta::assert_snapshot!(src);
}

#[test]
fn uuid_codegen_parse_snapshot() {
    let src = codegen_one_expr_in(
        "f",
        ns_assoc_call(
            "UUID",
            "parse",
            vec![str_expr("550e8400-e29b-41d4-a716-446655440000")],
        ),
    );
    insta::assert_snapshot!(src);
}

#[test]
fn url_codegen_parse_snapshot() {
    let src = codegen_one_expr_in(
        "f",
        ns_assoc_call("URL", "parse", vec![str_expr("https://a.com/b?q=1")]),
    );
    insta::assert_snapshot!(src);
}

#[test]
fn url_codegen_scheme_snapshot() {
    let src = codegen_stmts_in(
        "f",
        url_body_with_extra(
            "https://a.com/b",
            instance_call(ident_expr("u"), "scheme", vec![]),
        ),
    );
    insta::assert_snapshot!(src);
}

#[test]
fn url_codegen_host_snapshot() {
    let src = codegen_stmts_in(
        "f",
        url_body_with_extra(
            "https://a.com/b",
            instance_call(ident_expr("u"), "host", vec![]),
        ),
    );
    insta::assert_snapshot!(src);
}

#[test]
fn url_codegen_path_snapshot() {
    let src = codegen_stmts_in(
        "f",
        url_body_with_extra(
            "https://a.com/b",
            instance_call(ident_expr("u"), "path", vec![]),
        ),
    );
    insta::assert_snapshot!(src);
}

#[test]
fn url_codegen_query_snapshot() {
    let src = codegen_stmts_in(
        "f",
        url_body_with_extra(
            "https://a.com/b?q=1",
            instance_call(ident_expr("u"), "query", vec![str_expr("q")]),
        ),
    );
    insta::assert_snapshot!(src);
}

#[test]
fn web_codegen_full_program_snapshot() {
    // End-to-end snapshot: a `main` that exercises one call from each
    // of the five web modules. Pins the full shape of the generated
    // Rust for a typical web-using program (the acceptance criterion
    // from the task spec).
    let main = func_decl(
        "main",
        &[],
        vec![
            let_stmt("b64", ns_assoc_call("Base64", "encode", vec![bytes_expr()])),
            let_stmt(
                "bytes",
                ns_assoc_call("Base64", "decode", vec![str_expr("aGVsbG8=")]),
            ),
            let_stmt("h", ns_assoc_call("Hex", "encode", vec![bytes_expr()])),
            let_stmt(
                "raw",
                ns_assoc_call("Hex", "decode", vec![str_expr("deadbeef")]),
            ),
            let_stmt(
                "enc",
                ns_assoc_call("URLEncode", "encode", vec![str_expr("foo bar?")]),
            ),
            let_stmt(
                "dec",
                ns_assoc_call("URLEncode", "decode", vec![str_expr("foo%20bar")]),
            ),
            let_stmt("id4", ns_assoc_call("UUID", "v4", vec![])),
            let_stmt("id7", ns_assoc_call("UUID", "v7", vec![])),
            let_stmt(
                "valid",
                ns_assoc_call(
                    "UUID",
                    "parse",
                    vec![str_expr("550e8400-e29b-41d4-a716-446655440000")],
                ),
            ),
            let_stmt(
                "u",
                ns_assoc_call("URL", "parse", vec![str_expr("https://a.com/b?q=1")]),
            ),
            let_stmt("s", instance_call(ident_expr("u"), "scheme", vec![])),
            let_stmt("host", instance_call(ident_expr("u"), "host", vec![])),
            let_stmt("p", instance_call(ident_expr("u"), "path", vec![])),
            let_stmt(
                "q",
                instance_call(ident_expr("u"), "query", vec![str_expr("q")]),
            ),
        ],
    );
    let mut codegen = RustCodegen::new();
    let file = codegen.generate(&[main]).expect("codegen must succeed");
    let src = buff_lang_codegen_rust::format_file(&file);
    insta::assert_snapshot!(src);
}

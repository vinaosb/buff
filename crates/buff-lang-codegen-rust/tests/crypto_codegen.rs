//! T124k integration tests - cryptographic prelude modules codegen.
//!
//! Verifies that the Rust codegen lowers the two T124k crypto modules:
//!
//! - **Hash** namespace (`Hash.sha256(d) -> String`,
//!   `Hash.sha512(d) -> String`, `Hash.md5(d) -> String`)
//!   - `sha256`/`sha512` wrap the `sha2` RustCrypto crate's `Sha256`/
//!     `Sha512` `digest()` one-shot API (block-scoped `use sha2::Digest;`
//!     brings the trait method into scope without polluting the
//!     caller's namespace).
//!   - `md5` wraps the `md5` RustCrypto crate's `compute().0` API
//!     (the `.0` accesses the inner `[u8; 16]` of the `md5::Digest`
//!     tuple struct). MD5 is cryptographically broken - checksum
//!     compatibility only.
//! - **HMAC** namespace (`HMAC.sha256(key, data) -> String`)
//!   - wraps the `hmac` RustCrypto crate's `Hmac::<sha2::Sha256>
//!     ::new_from_slice(...).map(|mut mac| { mac.update(...);
//!     hex::encode(mac.finalize().into_bytes()) }).unwrap_or_default()`
//!     API (block-scoped `use hmac::Mac;` for the trait methods).
//!
//! Acceptance snapshots for the canonical criteria (per the task
//! spec):
//!
//! ```text
//! Hash.sha256("hello")    -> 2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824
//! Hash.sha512(d)          -> { use sha2::Digest; hex::encode(sha2::Sha512::digest(d.as_bytes())) }
//! Hash.md5(d)             -> hex::encode(md5::compute(d.as_bytes()).0)
//! HMAC.sha256(k, d)       -> { use hmac::Mac; hmac::Hmac::<sha2::Sha256>::new_from_slice(...)
//!                               .map(|mut mac| { mac.update(...); hex::encode(mac.finalize()
//!                               .into_bytes()) }).unwrap_or_default() }
//! ```
//!
//! Run via:
//!
//! ```text
//! cargo test -p buff-lang-codegen-rust --test crypto_codegen
//! ```
//!
//! # Why AST-constructed tests (not source-parsed)
//!
//! Both modules are prelude namespaces (associated functions returning
//! String), so source parsing requires no new keyword / AST node -
//! the existing `MethodCall` shape handles them. We construct ASTs by
//! hand here for the same reasons `fs_codegen.rs` (T124j),
//! `format_codegen.rs` (T124i), `web_codegen.rs` (T124h),
//! `system_codegen.rs` (T124g), `regex_codegen.rs` (T124d),
//! `toml_codegen.rs` (T124e), and `utility_codegen.rs` (T124f) do:
//! direct AST construction decouples the codegen-pinning snapshots
//! from any future parser-restructuring work, and lets us test
//! specific edge cases (e.g. wrong arity, ident vs literal arg)
//! without writing Buff source that the parser may reject for
//! orthogonal reasons.

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
/// shape). The receiver is the bare namespace Ident (e.g. `Hash`,
/// `HMAC`).
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
// 1. Hash.sha256 - one-arg String, returns hex.
// ===========================================================================

#[test]
fn hash_codegen_sha256_with_literal_uses_sha2_digest() {
    // Hash.sha256("hello") -> { use sha2::Digest; hex::encode(
    // sha2::Sha256::digest(<arg>.as_bytes())) }. The canonical
    // SHA-256 hex digest of "hello" is
    // 2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824
    // (the codegen-only linking boundary means we can't evaluate this
    // at test time, but the codegen shape pins the canonical lowering).
    let src = codegen_one_expr_in(
        "f",
        ns_assoc_call("Hash", "sha256", vec![str_expr("hello")]),
    );
    assert!(
        src.contains("use sha2::Digest"),
        "expected `use sha2::Digest` (Digest trait import) in: {src}"
    );
    assert!(
        src.contains("sha2::Sha256::digest"),
        "expected `sha2::Sha256::digest(` (one-shot digest) in: {src}"
    );
    assert!(
        src.contains(".as_bytes()"),
        "expected `.as_bytes()` (bytes coercion) in: {src}"
    );
    assert!(
        src.contains("hex::encode"),
        "expected `hex::encode(` (hex encoder) in: {src}"
    );
    // Must NOT use bare .unwrap() (panicking-generated-code rule).
    assert!(
        !src.contains(".unwrap()"),
        "expected NO bare `.unwrap()` in Hash.sha256 output: {src}"
    );
    must_reparse(&src);
}

#[test]
fn hash_codegen_sha256_with_ident_arg() {
    // Hash.sha256(data) where `data` is a variable. The arg should
    // splice through as the bare ident.
    let src = codegen_one_expr_in(
        "f",
        ns_assoc_call("Hash", "sha256", vec![ident_expr("data")]),
    );
    assert!(
        src.contains("sha2::Sha256::digest"),
        "expected `sha2::Sha256::digest(` in: {src}"
    );
    assert!(
        src.contains("data.as_bytes()"),
        "expected `data.as_bytes()` (ident arg splice) in: {src}"
    );
    must_reparse(&src);
}

// ===========================================================================
// 2. Hash.sha512 - same shape as sha256 but Sha512.
// ===========================================================================

#[test]
fn hash_codegen_sha512_with_literal_uses_sha2_digest() {
    // Hash.sha512("hello") -> { use sha2::Digest; hex::encode(
    // sha2::Sha512::digest(<arg>.as_bytes())) }. SHA-512 hex digest
    // is 128 chars (vs 64 for SHA-256).
    let src = codegen_one_expr_in(
        "f",
        ns_assoc_call("Hash", "sha512", vec![str_expr("hello")]),
    );
    assert!(
        src.contains("use sha2::Digest"),
        "expected `use sha2::Digest` (Digest trait import) in: {src}"
    );
    assert!(
        src.contains("sha2::Sha512::digest"),
        "expected `sha2::Sha512::digest(` (one-shot digest) in: {src}"
    );
    assert!(
        src.contains("hex::encode"),
        "expected `hex::encode(` (hex encoder) in: {src}"
    );
    // Must NOT use bare .unwrap() (panicking-generated-code rule).
    assert!(
        !src.contains(".unwrap()"),
        "expected NO bare `.unwrap()` in Hash.sha512 output: {src}"
    );
    must_reparse(&src);
}

// ===========================================================================
// 3. Hash.md5 - md5::compute().0 + hex::encode (no Digest trait).
// ===========================================================================

#[test]
fn hash_codegen_md5_with_literal_uses_md5_compute() {
    // Hash.md5("hello") -> hex::encode(md5::compute(<arg>.as_bytes()).0).
    // The `.0` accesses the inner `[u8; 16]` of the `md5::Digest`
    // tuple struct; NO `use sha2::Digest` (md5::compute is a free
    // function, NOT a trait method).
    let src = codegen_one_expr_in("f", ns_assoc_call("Hash", "md5", vec![str_expr("hello")]));
    assert!(
        src.contains("md5::compute"),
        "expected `md5::compute(` in: {src}"
    );
    assert!(
        src.contains(".0"),
        "expected `.0` (tuple-struct field access) in: {src}"
    );
    assert!(
        src.contains("hex::encode"),
        "expected `hex::encode(` (hex encoder) in: {src}"
    );
    // Must NOT emit `use sha2::Digest` (md5 doesn't need it).
    assert!(
        !src.contains("use sha2::Digest"),
        "expected NO `use sha2::Digest` in Hash.md5 output: {src}"
    );
    // Must NOT use bare .unwrap() (panicking-generated-code rule).
    assert!(
        !src.contains(".unwrap()"),
        "expected NO bare `.unwrap()` in Hash.md5 output: {src}"
    );
    must_reparse(&src);
}

// ===========================================================================
// 4. HMAC.sha256 - two args (key, data), panic-free via .map().unwrap_or_default().
// ===========================================================================

#[test]
fn hmac_codegen_sha256_with_literals_uses_hmac_new_from_slice() {
    // HMAC.sha256("secret", "data") -> { use hmac::Mac;
    // hmac::Hmac::<sha2::Sha256>::new_from_slice(<key>.as_bytes())
    // .map(|mut mac| { mac.update(<data>.as_bytes()); hex::encode(
    // mac.finalize().into_bytes()) }).unwrap_or_default() }.
    let src = codegen_one_expr_in(
        "f",
        ns_assoc_call("HMAC", "sha256", vec![str_expr("secret"), str_expr("data")]),
    );
    assert!(
        src.contains("use hmac::Mac"),
        "expected `use hmac::Mac` (Mac trait import) in: {src}"
    );
    assert!(
        src.contains("hmac::Hmac::<sha2::Sha256>::new_from_slice"),
        "expected `hmac::Hmac::<sha2::Sha256>::new_from_slice(` in: {src}"
    );
    assert!(
        src.contains(".map(|mut mac|"),
        "expected `.map(|mut mac|` (closure on Result) in: {src}"
    );
    assert!(
        src.contains("mac.update"),
        "expected `mac.update(` (Mac trait method) in: {src}"
    );
    assert!(
        src.contains("mac.finalize"),
        "expected `mac.finalize(` (Mac trait method) in: {src}"
    );
    assert!(
        src.contains(".into_bytes"),
        "expected `.into_bytes()` (Output -> GenericArray) in: {src}"
    );
    assert!(
        src.contains("hex::encode"),
        "expected `hex::encode(` (hex encoder) in: {src}"
    );
    assert!(
        src.contains(".unwrap_or_default()"),
        "expected `.unwrap_or_default()` (panic-free Result collapse) in: {src}"
    );
    // Must NOT use bare .unwrap() (panicking-generated-code rule).
    assert!(
        !src.contains(".unwrap()"),
        "expected NO bare `.unwrap()` in HMAC.sha256 output: {src}"
    );
    // Sanity: the `.unwrap_or_default()` collapse must NOT be a bare
    // `.unwrap()` (the panic-free requirement). The substring check
    // above already covers this but let's be explicit.
    must_reparse(&src);
}

#[test]
fn hmac_codegen_sha256_with_ident_args_splices_both() {
    // HMAC.sha256(key, msg) where both are variables. Both args
    // should splice through as the bare idents in their respective
    // positions (key for new_from_slice, msg for update).
    let src = codegen_one_expr_in(
        "f",
        ns_assoc_call("HMAC", "sha256", vec![ident_expr("key"), ident_expr("msg")]),
    );
    assert!(
        src.contains("key.as_bytes()"),
        "expected `key.as_bytes()` (key ident splice) in: {src}"
    );
    assert!(
        src.contains("msg.as_bytes()"),
        "expected `msg.as_bytes()` (msg ident splice) in: {src}"
    );
    must_reparse(&src);
}

// ===========================================================================
// 5. extern_crates registration (narrow walkers).
// ===========================================================================

#[test]
fn hash_codegen_registers_sha2_for_sha256() {
    // A program with Hash.sha256(...) registers sha2 + hex.
    let main = func_decl(
        "main",
        &[],
        vec![expr_stmt(ns_assoc_call(
            "Hash",
            "sha256",
            vec![str_expr("hi")],
        ))],
    );
    let mut codegen = RustCodegen::new();
    let _ = codegen.generate(&[main]).expect("codegen must succeed");
    let extern_crates = codegen.extern_crates();
    assert!(
        extern_crates.contains("sha2"),
        "extern_crates should contain `sha2`, got: {:?}",
        extern_crates
    );
    assert!(
        extern_crates.contains("hex"),
        "extern_crates should contain `hex`, got: {:?}",
        extern_crates
    );
    // Must NOT register md5 or hmac for sha256.
    assert!(
        !extern_crates.contains("md5"),
        "extern_crates should NOT contain `md5` for Hash.sha256, got: {:?}",
        extern_crates
    );
    assert!(
        !extern_crates.contains("hmac"),
        "extern_crates should NOT contain `hmac` for Hash.sha256, got: {:?}",
        extern_crates
    );
}

#[test]
fn hash_codegen_registers_sha2_for_sha512() {
    // A program with Hash.sha512(...) registers sha2 + hex (but
    // NOT md5 or hmac).
    let main = func_decl(
        "main",
        &[],
        vec![expr_stmt(ns_assoc_call(
            "Hash",
            "sha512",
            vec![str_expr("hi")],
        ))],
    );
    let mut codegen = RustCodegen::new();
    let _ = codegen.generate(&[main]).expect("codegen must succeed");
    let extern_crates = codegen.extern_crates();
    assert!(
        extern_crates.contains("sha2"),
        "extern_crates should contain `sha2`, got: {:?}",
        extern_crates
    );
    assert!(
        extern_crates.contains("hex"),
        "extern_crates should contain `hex`, got: {:?}",
        extern_crates
    );
    assert!(
        !extern_crates.contains("md5"),
        "extern_crates should NOT contain `md5` for Hash.sha512, got: {:?}",
        extern_crates
    );
    assert!(
        !extern_crates.contains("hmac"),
        "extern_crates should NOT contain `hmac` for Hash.sha512, got: {:?}",
        extern_crates
    );
}

#[test]
fn hash_codegen_registers_md5_for_md5_only() {
    // A program with Hash.md5(...) registers md5 + hex (but NOT sha2
    // or hmac - the narrow md5 walker flags ONLY the md5 method).
    let main = func_decl(
        "main",
        &[],
        vec![expr_stmt(ns_assoc_call(
            "Hash",
            "md5",
            vec![str_expr("hi")],
        ))],
    );
    let mut codegen = RustCodegen::new();
    let _ = codegen.generate(&[main]).expect("codegen must succeed");
    let extern_crates = codegen.extern_crates();
    assert!(
        extern_crates.contains("md5"),
        "extern_crates should contain `md5`, got: {:?}",
        extern_crates
    );
    assert!(
        extern_crates.contains("hex"),
        "extern_crates should contain `hex`, got: {:?}",
        extern_crates
    );
    assert!(
        !extern_crates.contains("sha2"),
        "extern_crates should NOT contain `sha2` for Hash.md5-only, got: {:?}",
        extern_crates
    );
    assert!(
        !extern_crates.contains("hmac"),
        "extern_crates should NOT contain `hmac` for Hash.md5, got: {:?}",
        extern_crates
    );
}

#[test]
fn hmac_codegen_registers_hmac_and_sha2_for_sha256() {
    // A program with HMAC.sha256(...) registers hmac + sha2 + hex
    // (the lowering path `hmac::Hmac<sha2::Sha256>` needs BOTH
    // hmac + sha2).
    let main = func_decl(
        "main",
        &[],
        vec![expr_stmt(ns_assoc_call(
            "HMAC",
            "sha256",
            vec![str_expr("k"), str_expr("d")],
        ))],
    );
    let mut codegen = RustCodegen::new();
    let _ = codegen.generate(&[main]).expect("codegen must succeed");
    let extern_crates = codegen.extern_crates();
    assert!(
        extern_crates.contains("hmac"),
        "extern_crates should contain `hmac`, got: {:?}",
        extern_crates
    );
    assert!(
        extern_crates.contains("sha2"),
        "extern_crates should contain `sha2` (HMAC.sha256 lowers to Hmac<Sha256>), got: {:?}",
        extern_crates
    );
    assert!(
        extern_crates.contains("hex"),
        "extern_crates should contain `hex`, got: {:?}",
        extern_crates
    );
    // Must NOT register md5 for HMAC.sha256.
    assert!(
        !extern_crates.contains("md5"),
        "extern_crates should NOT contain `md5` for HMAC.sha256, got: {:?}",
        extern_crates
    );
}

#[test]
fn crypto_codegen_no_extern_crate_when_unused() {
    // A program with no Hash / HMAC calls should not register
    // sha2 / md5 / hmac.
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
        !extern_crates.contains("sha2"),
        "extern_crates should NOT contain `sha2` when Hash/HMAC are unused, got: {:?}",
        extern_crates
    );
    assert!(
        !extern_crates.contains("md5"),
        "extern_crates should NOT contain `md5` when Hash is unused, got: {:?}",
        extern_crates
    );
    assert!(
        !extern_crates.contains("hmac"),
        "extern_crates should NOT contain `hmac` when HMAC is unused, got: {:?}",
        extern_crates
    );
}

#[test]
fn crypto_codegen_combined_program_registers_all_three() {
    // A program using Hash.sha256 + Hash.md5 + HMAC.sha256 should
    // register sha2 + md5 + hmac + hex (the union of all three
    // narrow walkers).
    let main = func_decl(
        "main",
        &[],
        vec![
            let_stmt("a", ns_assoc_call("Hash", "sha256", vec![str_expr("x")])),
            let_stmt("b", ns_assoc_call("Hash", "md5", vec![str_expr("x")])),
            let_stmt(
                "c",
                ns_assoc_call("HMAC", "sha256", vec![str_expr("k"), str_expr("x")]),
            ),
        ],
    );
    let mut codegen = RustCodegen::new();
    let _ = codegen.generate(&[main]).expect("codegen must succeed");
    let extern_crates = codegen.extern_crates();
    assert!(
        extern_crates.contains("sha2"),
        "extern_crates should contain `sha2`, got: {:?}",
        extern_crates
    );
    assert!(
        extern_crates.contains("md5"),
        "extern_crates should contain `md5`, got: {:?}",
        extern_crates
    );
    assert!(
        extern_crates.contains("hmac"),
        "extern_crates should contain `hmac`, got: {:?}",
        extern_crates
    );
    assert!(
        extern_crates.contains("hex"),
        "extern_crates should contain `hex`, got: {:?}",
        extern_crates
    );
}

// ===========================================================================
// 6. Error cases - arity mismatch surfaces a clear CodegenError.
// ===========================================================================

#[test]
fn hash_codegen_rejects_sha256_with_zero_args() {
    // Hash.sha256() with no args - should error.
    let result = std::panic::catch_unwind(|| {
        let _ = codegen_one_expr_in("f", ns_assoc_call("Hash", "sha256", vec![]));
    });
    assert!(
        result.is_err(),
        "expected codegen to reject `Hash.sha256()` (no data arg)"
    );
}

#[test]
fn hash_codegen_rejects_sha256_with_two_args() {
    // Hash.sha256(a, b) with extra args - should error.
    let result = std::panic::catch_unwind(|| {
        let _ = codegen_one_expr_in(
            "f",
            ns_assoc_call("Hash", "sha256", vec![str_expr("a"), str_expr("b")]),
        );
    });
    assert!(
        result.is_err(),
        "expected codegen to reject `Hash.sha256(\"a\", \"b\")` (expected 1 arg)"
    );
}

#[test]
fn hash_codegen_rejects_md5_with_zero_args() {
    // Hash.md5() with no args - should error.
    let result = std::panic::catch_unwind(|| {
        let _ = codegen_one_expr_in("f", ns_assoc_call("Hash", "md5", vec![]));
    });
    assert!(
        result.is_err(),
        "expected codegen to reject `Hash.md5()` (no data arg)"
    );
}

#[test]
fn hmac_codegen_rejects_sha256_with_one_arg() {
    // HMAC.sha256(key) with missing data arg - should error.
    let result = std::panic::catch_unwind(|| {
        let _ = codegen_one_expr_in("f", ns_assoc_call("HMAC", "sha256", vec![str_expr("k")]));
    });
    assert!(
        result.is_err(),
        "expected codegen to reject `HMAC.sha256(\"k\")` (expected 2 args - key + data)"
    );
}

#[test]
fn hmac_codegen_rejects_sha256_with_three_args() {
    // HMAC.sha256(k, d, extra) with extra args - should error.
    let result = std::panic::catch_unwind(|| {
        let _ = codegen_one_expr_in(
            "f",
            ns_assoc_call(
                "HMAC",
                "sha256",
                vec![str_expr("k"), str_expr("d"), str_expr("x")],
            ),
        );
    });
    assert!(
        result.is_err(),
        "expected codegen to reject `HMAC.sha256(\"k\", \"d\", \"x\")` (expected 2 args)"
    );
}

// ===========================================================================
// 7. insta snapshots - byte-stable codegen pinning.
// ===========================================================================

#[test]
fn hash_codegen_sha256_snapshot() {
    let src = codegen_one_expr_in(
        "f",
        ns_assoc_call("Hash", "sha256", vec![str_expr("hello")]),
    );
    insta::assert_snapshot!(src);
}

#[test]
fn hash_codegen_sha512_snapshot() {
    let src = codegen_one_expr_in(
        "f",
        ns_assoc_call("Hash", "sha512", vec![str_expr("hello")]),
    );
    insta::assert_snapshot!(src);
}

#[test]
fn hash_codegen_md5_snapshot() {
    let src = codegen_one_expr_in("f", ns_assoc_call("Hash", "md5", vec![str_expr("hello")]));
    insta::assert_snapshot!(src);
}

#[test]
fn hmac_codegen_sha256_snapshot() {
    let src = codegen_one_expr_in(
        "f",
        ns_assoc_call(
            "HMAC",
            "sha256",
            vec![str_expr("secret"), str_expr("message")],
        ),
    );
    insta::assert_snapshot!(src);
}

#[test]
fn crypto_codegen_full_program_snapshot() {
    // End-to-end snapshot: a `main` that exercises one call from each
    // of the four crypto assoc fns. Pins the full shape of the
    // generated Rust for a typical crypto-using program (the
    // acceptance criterion from the task spec).
    let main = func_decl(
        "main",
        &[],
        vec![
            // Hash namespace - all 3 associated fns.
            let_stmt(
                "sha256hex",
                ns_assoc_call("Hash", "sha256", vec![str_expr("hello")]),
            ),
            let_stmt(
                "sha512hex",
                ns_assoc_call("Hash", "sha512", vec![str_expr("hello")]),
            ),
            let_stmt(
                "md5hex",
                ns_assoc_call("Hash", "md5", vec![str_expr("hello")]),
            ),
            // HMAC namespace - the one associated fn.
            let_stmt(
                "hmachex",
                ns_assoc_call(
                    "HMAC",
                    "sha256",
                    vec![str_expr("secret"), str_expr("message")],
                ),
            ),
        ],
    );
    let mut codegen = RustCodegen::new();
    let file = codegen.generate(&[main]).expect("codegen must succeed");
    let src = buff_lang_codegen_rust::format_file(&file);
    insta::assert_snapshot!(src);
}

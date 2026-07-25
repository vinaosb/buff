//! T49 integration tests - buff-crypto-extras prelude types codegen.
//!
//! Verifies that the Rust codegen lowers the T49 crypto-extras surface:
//!
//! - **AES** namespace (`AES.generate_key() -> Vector<Byte>`,
//!   `AES.generate_nonce() -> Vector<Byte>`,
//!   `AES.encrypt(key, nonce, plaintext) -> Vector<Byte>`,
//!   `AES.decrypt(key, nonce, ciphertext) -> Vector<Byte>`)
//! - **RSA** namespace (`RSA.generate_keypair(bits: 2048) -> RsaKeypair`,
//!   `RSA.sign(private_pem, data) -> Vector<Byte>`,
//!   `RSA.verify(public_pem, data, signature) -> Bool`)
//! - **ECDH** namespace (`ECDH.generate_private() -> Vector<Byte>`,
//!   `ECDH.public_from_private(private) -> Vector<Byte>`,
//!   `ECDH.derive_shared(private, public) -> Vector<Byte>`)
//! - **Argon2** namespace (`Argon2.generate_salt() -> Vector<Byte>`,
//!   `Argon2.derive_key(password, salt) -> Vector<Byte>`)
//! - **RsaKeypair** instance methods (`pair.public_pem() -> String`,
//!   `pair.private_pem() -> String`)
//!
//! Each namespace function wraps the `buff_crypto_extras::*` crate's
//! safe API. All fallible ops collapse to a sensible default via
//! `.unwrap_or_default()` (empty Vec / false Bool / empty String) per
//! Buff's "no panicking generated code" rule.
//!
//! Run via:
//!
//! ```text
//! cargo test -p buff-lang-codegen-rust --test crypto_extras_codegen
//! ```
//!
//! # Why AST-constructed tests (not source-parsed)
//!
//! All types here are prelude types (associated functions + instance
//! methods), so source parsing requires no new keyword / AST node —
//! the existing `MethodCall` shape handles them. We construct ASTs by
//! hand here for the same reasons `nlp_codegen.rs` (T46),
//! `geo_codegen.rs` (T45), `crypto_codegen.rs` (T124k),
//! `web3_codegen.rs` (T48), `chat_codegen.rs` (T47),
//! `protobuf_codegen.rs` (T52), and `scrape_codegen.rs` (T43) do:
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

fn int_expr(n: i64) -> Expr {
    Expr::Literal(Literal::Int(n), span())
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
/// shape). The receiver is the bare namespace Ident (e.g. `AES`).
fn ns_assoc_call(namespace: &str, method: &str, args: Vec<Expr>) -> Expr {
    Expr::MethodCall {
        receiver: Box::new(ident_expr(namespace)),
        method: ident(method),
        args,
        span: span(),
    }
}

/// `recv.<method>(args...)` AST node (instance-method call shape).
/// The receiver is a variable Ident (e.g. `pair`).
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
// 1. AES namespace — 4 associated functions.
// ===========================================================================

#[test]
fn aes_codegen_generate_key_lowers_correctly() {
    // AES.generate_key() -> buff_crypto_extras::aes_gcm_api::generate_key()
    let src = codegen_one_expr_in("f", ns_assoc_call("AES", "generate_key", vec![]));
    assert!(
        src.contains("buff_crypto_extras::aes_gcm_api::generate_key"),
        "expected `buff_crypto_extras::aes_gcm_api::generate_key` in: {src}"
    );
    must_reparse(&src);
}

#[test]
fn aes_codegen_generate_nonce_lowers_correctly() {
    // AES.generate_nonce() -> buff_crypto_extras::aes_gcm_api::generate_nonce()
    let src = codegen_one_expr_in("f", ns_assoc_call("AES", "generate_nonce", vec![]));
    assert!(
        src.contains("buff_crypto_extras::aes_gcm_api::generate_nonce"),
        "expected `buff_crypto_extras::aes_gcm_api::generate_nonce` in: {src}"
    );
    must_reparse(&src);
}

#[test]
fn aes_codegen_encrypt_lowers_correctly() {
    // AES.encrypt(key, nonce, plaintext) — three args, all spliced by
    // .as_slice() so the underlying &[u8] bounds are satisfied.
    let src = codegen_one_expr_in(
        "f",
        ns_assoc_call(
            "AES",
            "encrypt",
            vec![ident_expr("k"), ident_expr("n"), ident_expr("pt")],
        ),
    );
    assert!(
        src.contains("buff_crypto_extras::aes_gcm_api::encrypt"),
        "expected `buff_crypto_extras::aes_gcm_api::encrypt` in: {src}"
    );
    assert!(
        src.contains(".as_slice()"),
        "expected `.as_slice()` (borrow conversion) in: {src}"
    );
    assert!(
        src.contains(".unwrap_or_default()"),
        "expected `.unwrap_or_default()` (panic-free) in: {src}"
    );
    must_reparse(&src);
}

#[test]
fn aes_codegen_decrypt_lowers_correctly() {
    // AES.decrypt(key, nonce, ciphertext) — same shape as encrypt.
    let src = codegen_one_expr_in(
        "f",
        ns_assoc_call(
            "AES",
            "decrypt",
            vec![ident_expr("k"), ident_expr("n"), ident_expr("ct")],
        ),
    );
    assert!(
        src.contains("buff_crypto_extras::aes_gcm_api::decrypt"),
        "expected `buff_crypto_extras::aes_gcm_api::decrypt` in: {src}"
    );
    assert!(
        src.contains(".unwrap_or_default()"),
        "expected `.unwrap_or_default()` (panic-free) in: {src}"
    );
    must_reparse(&src);
}

// ===========================================================================
// 2. RSA namespace — 3 associated functions.
// ===========================================================================

#[test]
fn rsa_codegen_generate_keypair_lowers_correctly() {
    // RSA.generate_keypair(2048) -> buff_crypto_extras::rsa_api::
    // generate_keypair(2048 as usize).unwrap_or_default().
    let src = codegen_one_expr_in(
        "f",
        ns_assoc_call("RSA", "generate_keypair", vec![int_expr(2048)]),
    );
    assert!(
        src.contains("buff_crypto_extras::rsa_api::generate_keypair"),
        "expected `buff_crypto_extras::rsa_api::generate_keypair` in: {src}"
    );
    assert!(
        src.contains(" as usize"),
        "expected ` as usize` (Int<64> -> usize lift) in: {src}"
    );
    assert!(
        src.contains(".unwrap_or_default()"),
        "expected `.unwrap_or_default()` (panic-free) in: {src}"
    );
    must_reparse(&src);
}

#[test]
fn rsa_codegen_sign_lowers_correctly() {
    // RSA.sign(private_pem, data) — two args; private_pem by ref,
    // data by .as_slice().
    let src = codegen_one_expr_in(
        "f",
        ns_assoc_call("RSA", "sign", vec![ident_expr("priv"), ident_expr("data")]),
    );
    assert!(
        src.contains("buff_crypto_extras::rsa_api::sign"),
        "expected `buff_crypto_extras::rsa_api::sign` in: {src}"
    );
    assert!(
        src.contains(".unwrap_or_default()"),
        "expected `.unwrap_or_default()` (panic-free) in: {src}"
    );
    must_reparse(&src);
}

#[test]
fn rsa_codegen_verify_lowers_correctly() {
    // RSA.verify(public_pem, data, signature) — three args. The
    // wrapper returns bool directly (NO unwrap_or_default needed —
    // it collapses all failures to false itself).
    let src = codegen_one_expr_in(
        "f",
        ns_assoc_call(
            "RSA",
            "verify",
            vec![ident_expr("pub"), ident_expr("data"), ident_expr("sig")],
        ),
    );
    assert!(
        src.contains("buff_crypto_extras::rsa_api::verify"),
        "expected `buff_crypto_extras::rsa_api::verify` in: {src}"
    );
    // verify returns bool directly — no unwrap_or_default.
    assert!(
        !src.contains(".unwrap_or_default()"),
        "verify should NOT have `.unwrap_or_default()` (wrapper already returns bool), got: {src}"
    );
    must_reparse(&src);
}

// ===========================================================================
// 3. ECDH namespace — 3 associated functions.
// ===========================================================================

#[test]
fn ecdh_codegen_generate_private_lowers_correctly() {
    // ECDH.generate_private() -> buff_crypto_extras::ecdh_api::
    // p256_generate_private().
    let src = codegen_one_expr_in("f", ns_assoc_call("ECDH", "generate_private", vec![]));
    assert!(
        src.contains("buff_crypto_extras::ecdh_api::p256_generate_private"),
        "expected `buff_crypto_extras::ecdh_api::p256_generate_private` in: {src}"
    );
    must_reparse(&src);
}

#[test]
fn ecdh_codegen_public_from_private_lowers_correctly() {
    // ECDH.public_from_private(private) -> ...p256_public_from_private(
    // private.as_slice()).unwrap_or_default().
    let src = codegen_one_expr_in(
        "f",
        ns_assoc_call("ECDH", "public_from_private", vec![ident_expr("sk")]),
    );
    assert!(
        src.contains("buff_crypto_extras::ecdh_api::p256_public_from_private"),
        "expected `buff_crypto_extras::ecdh_api::p256_public_from_private` in: {src}"
    );
    assert!(
        src.contains(".unwrap_or_default()"),
        "expected `.unwrap_or_default()` (panic-free) in: {src}"
    );
    must_reparse(&src);
}

#[test]
fn ecdh_codegen_derive_shared_lowers_correctly() {
    // ECDH.derive_shared(private, public) -> ...p256_derive_shared(
    // private.as_slice(), public.as_slice()).unwrap_or_default().
    let src = codegen_one_expr_in(
        "f",
        ns_assoc_call(
            "ECDH",
            "derive_shared",
            vec![ident_expr("sk"), ident_expr("pk")],
        ),
    );
    assert!(
        src.contains("buff_crypto_extras::ecdh_api::p256_derive_shared"),
        "expected `buff_crypto_extras::ecdh_api::p256_derive_shared` in: {src}"
    );
    assert!(
        src.contains(".unwrap_or_default()"),
        "expected `.unwrap_or_default()` (panic-free) in: {src}"
    );
    must_reparse(&src);
}

// ===========================================================================
// 4. Argon2 namespace — 2 associated functions.
// ===========================================================================

#[test]
fn argon2_codegen_generate_salt_lowers_correctly() {
    // Argon2.generate_salt() -> buff_crypto_extras::argon2_api::
    // generate_salt().
    let src = codegen_one_expr_in("f", ns_assoc_call("Argon2", "generate_salt", vec![]));
    assert!(
        src.contains("buff_crypto_extras::argon2_api::generate_salt"),
        "expected `buff_crypto_extras::argon2_api::generate_salt` in: {src}"
    );
    must_reparse(&src);
}

#[test]
fn argon2_codegen_derive_key_lowers_correctly() {
    // Argon2.derive_key(password, salt) -> ...derive_key(&password,
    // salt.as_slice()).unwrap_or_default().
    let src = codegen_one_expr_in(
        "f",
        ns_assoc_call(
            "Argon2",
            "derive_key",
            vec![string_expr("hunter2"), ident_expr("salt")],
        ),
    );
    assert!(
        src.contains("buff_crypto_extras::argon2_api::derive_key"),
        "expected `buff_crypto_extras::argon2_api::derive_key` in: {src}"
    );
    assert!(
        src.contains(".unwrap_or_default()"),
        "expected `.unwrap_or_default()` (panic-free) in: {src}"
    );
    must_reparse(&src);
}

// ===========================================================================
// 5. RsaKeypair instance methods — public_pem / private_pem.
// ===========================================================================

#[test]
fn rsa_keypair_codegen_public_pem_lowers_correctly() {
    // pair.public_pem() -> recv.public_pem.clone(). The codegen
    // infers the receiver type from the let-binding (RsaKeypair
    // since RSA.generate_keypair returns it). The instance-method
    // arm fires on the (RsaKeypair, PublicPem) pair.
    let src = codegen_stmts_in(
        "f",
        vec![
            let_stmt(
                "pair",
                ns_assoc_call("RSA", "generate_keypair", vec![int_expr(2048)]),
            ),
            expr_stmt(instance_call("pair", "public_pem", vec![])),
        ],
    );
    assert!(
        src.contains("buff_crypto_extras::rsa_api::generate_keypair"),
        "expected `buff_crypto_extras::rsa_api::generate_keypair` (ctor) in: {src}"
    );
    assert!(
        src.contains(".public_pem"),
        "expected `.public_pem` field access in: {src}"
    );
    assert!(
        src.contains(".clone()"),
        "expected `.clone()` (lift &String to owned String) in: {src}"
    );
    must_reparse(&src);
}

#[test]
fn rsa_keypair_codegen_private_pem_lowers_correctly() {
    // pair.private_pem() -> recv.private_pem.clone(). Same shape
    // as public_pem above.
    let src = codegen_stmts_in(
        "f",
        vec![
            let_stmt(
                "pair",
                ns_assoc_call("RSA", "generate_keypair", vec![int_expr(2048)]),
            ),
            expr_stmt(instance_call("pair", "private_pem", vec![])),
        ],
    );
    assert!(
        src.contains("buff_crypto_extras::rsa_api::generate_keypair"),
        "expected `buff_crypto_extras::rsa_api::generate_keypair` (ctor) in: {src}"
    );
    assert!(
        src.contains(".private_pem"),
        "expected `.private_pem` field access in: {src}"
    );
    assert!(
        src.contains(".clone()"),
        "expected `.clone()` (lift &String to owned String) in: {src}"
    );
    must_reparse(&src);
}

// ===========================================================================
// 6. extern_crates registration (narrow walker).
// ===========================================================================

#[test]
fn crypto_extras_codegen_registers_buff_crypto_extras_for_aes() {
    let main = func_decl(
        "main",
        &[],
        vec![let_stmt(
            "key",
            ns_assoc_call("AES", "generate_key", vec![]),
        )],
    );
    let mut codegen = RustCodegen::new();
    let _ = codegen.generate(&[main]).expect("codegen must succeed");
    let extern_crates = codegen.extern_crates();
    assert!(
        extern_crates.contains("buff-crypto-extras"),
        "extern_crates should contain `buff-crypto-extras`, got: {:?}",
        extern_crates
    );
    assert!(
        extern_crates.contains("aes-gcm"),
        "extern_crates should contain `aes-gcm`, got: {:?}",
        extern_crates
    );
}

#[test]
fn crypto_extras_codegen_registers_buff_crypto_extras_for_rsa() {
    let main = func_decl(
        "main",
        &[],
        vec![let_stmt(
            "pair",
            ns_assoc_call("RSA", "generate_keypair", vec![int_expr(2048)]),
        )],
    );
    let mut codegen = RustCodegen::new();
    let _ = codegen.generate(&[main]).expect("codegen must succeed");
    let extern_crates = codegen.extern_crates();
    assert!(
        extern_crates.contains("buff-crypto-extras"),
        "extern_crates should contain `buff-crypto-extras`, got: {:?}",
        extern_crates
    );
    assert!(
        extern_crates.contains("rsa"),
        "extern_crates should contain `rsa`, got: {:?}",
        extern_crates
    );
    assert!(
        extern_crates.contains("sha2"),
        "extern_crates should contain `sha2`, got: {:?}",
        extern_crates
    );
    assert!(
        extern_crates.contains("signature"),
        "extern_crates should contain `signature`, got: {:?}",
        extern_crates
    );
}

#[test]
fn crypto_extras_codegen_registers_buff_crypto_extras_for_ecdh() {
    let main = func_decl(
        "main",
        &[],
        vec![let_stmt(
            "sk",
            ns_assoc_call("ECDH", "generate_private", vec![]),
        )],
    );
    let mut codegen = RustCodegen::new();
    let _ = codegen.generate(&[main]).expect("codegen must succeed");
    let extern_crates = codegen.extern_crates();
    assert!(
        extern_crates.contains("buff-crypto-extras"),
        "extern_crates should contain `buff-crypto-extras`, got: {:?}",
        extern_crates
    );
    assert!(
        extern_crates.contains("p256"),
        "extern_crates should contain `p256`, got: {:?}",
        extern_crates
    );
    assert!(
        extern_crates.contains("p384"),
        "extern_crates should contain `p384`, got: {:?}",
        extern_crates
    );
}

#[test]
fn crypto_extras_codegen_registers_buff_crypto_extras_for_argon2() {
    let main = func_decl(
        "main",
        &[],
        vec![let_stmt(
            "salt",
            ns_assoc_call("Argon2", "generate_salt", vec![]),
        )],
    );
    let mut codegen = RustCodegen::new();
    let _ = codegen.generate(&[main]).expect("codegen must succeed");
    let extern_crates = codegen.extern_crates();
    assert!(
        extern_crates.contains("buff-crypto-extras"),
        "extern_crates should contain `buff-crypto-extras`, got: {:?}",
        extern_crates
    );
    assert!(
        extern_crates.contains("argon2"),
        "extern_crates should contain `argon2`, got: {:?}",
        extern_crates
    );
}

#[test]
fn crypto_extras_codegen_registers_buff_crypto_extras_for_rsa_keypair_instance() {
    // pair.public_pem() should register buff-crypto-extras via the
    // `program_uses_namespace("RsaKeypair")` walker.
    let main = func_decl(
        "main",
        &[],
        vec![
            let_stmt(
                "pair",
                ns_assoc_call("RSA", "generate_keypair", vec![int_expr(2048)]),
            ),
            expr_stmt(instance_call("pair", "public_pem", vec![])),
        ],
    );
    let mut codegen = RustCodegen::new();
    let _ = codegen.generate(&[main]).expect("codegen must succeed");
    let extern_crates = codegen.extern_crates();
    assert!(
        extern_crates.contains("buff-crypto-extras"),
        "extern_crates should contain `buff-crypto-extras`, got: {:?}",
        extern_crates
    );
}

#[test]
fn crypto_extras_codegen_no_extern_crate_when_unused() {
    // A program with no AES.* / RSA.* / ECDH.* / Argon2.* / RsaKeypair.*
    // calls should not register buff-crypto-extras + the 8 RustCrypto
    // crates.
    let main = func_decl(
        "main",
        &[],
        vec![expr_stmt(Expr::FuncCall {
            callee: Box::new(ident_expr("print")),
            args: vec![ident_expr("hi")],
            span: span(),
        })],
    );
    let mut codegen = RustCodegen::new();
    let _ = codegen.generate(&[main]).expect("codegen must succeed");
    let extern_crates = codegen.extern_crates();
    assert!(
        !extern_crates.contains("buff-crypto-extras"),
        "extern_crates should NOT contain `buff-crypto-extras` when crypto-extras types are unused, got: {:?}",
        extern_crates
    );
    assert!(
        !extern_crates.contains("aes-gcm"),
        "extern_crates should NOT contain `aes-gcm` when crypto-extras types are unused, got: {:?}",
        extern_crates
    );
    assert!(
        !extern_crates.contains("rsa"),
        "extern_crates should NOT contain `rsa` when crypto-extras types are unused, got: {:?}",
        extern_crates
    );
}

// ===========================================================================
// 7. Full program snapshot — pins the end-to-end codegen shape.
// ===========================================================================

#[test]
fn crypto_extras_codegen_full_program_snapshot() {
    // End-to-end snapshot: a `main` that exercises the full
    // crypto-extras surface from the task spec's acceptance criteria.
    let main = func_decl(
        "main",
        &[],
        vec![
            // AES roundtrip.
            let_stmt("key", ns_assoc_call("AES", "generate_key", vec![])),
            let_stmt("nonce", ns_assoc_call("AES", "generate_nonce", vec![])),
            let_stmt(
                "ct",
                ns_assoc_call(
                    "AES",
                    "encrypt",
                    vec![ident_expr("key"), ident_expr("nonce"), ident_expr("key")],
                ),
            ),
            let_stmt(
                "pt",
                ns_assoc_call(
                    "AES",
                    "decrypt",
                    vec![ident_expr("key"), ident_expr("nonce"), ident_expr("ct")],
                ),
            ),
            // RSA sign / verify roundtrip.
            let_stmt(
                "pair",
                ns_assoc_call("RSA", "generate_keypair", vec![int_expr(2048)]),
            ),
            let_stmt(
                "sig",
                ns_assoc_call(
                    "RSA",
                    "sign",
                    vec![
                        instance_call("pair", "private_pem", vec![]),
                        ident_expr("ct"),
                    ],
                ),
            ),
            let_stmt(
                "ok",
                ns_assoc_call(
                    "RSA",
                    "verify",
                    vec![
                        instance_call("pair", "public_pem", vec![]),
                        ident_expr("ct"),
                        ident_expr("sig"),
                    ],
                ),
            ),
            // ECDH key agreement.
            let_stmt(
                "priv_key",
                ns_assoc_call("ECDH", "generate_private", vec![]),
            ),
            let_stmt(
                "pub_key",
                ns_assoc_call("ECDH", "public_from_private", vec![ident_expr("priv_key")]),
            ),
            let_stmt(
                "shared",
                ns_assoc_call(
                    "ECDH",
                    "derive_shared",
                    vec![ident_expr("priv_key"), ident_expr("pub_key")],
                ),
            ),
            // Argon2 KDF.
            let_stmt("salt", ns_assoc_call("Argon2", "generate_salt", vec![])),
            let_stmt(
                "dk",
                ns_assoc_call(
                    "Argon2",
                    "derive_key",
                    vec![string_expr("hunter2"), ident_expr("salt")],
                ),
            ),
        ],
    );
    let mut codegen = RustCodegen::new();
    let file = codegen.generate(&[main]).expect("codegen must succeed");
    let src = buff_lang_codegen_rust::format_file(&file);
    insta::assert_snapshot!(src);
}

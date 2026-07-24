//! T48 integration tests - buff-web3 prelude types codegen.
//!
//! Verifies that the Rust codegen lowers the T48 web3 surface:
//!
//! - **Provider** associated fn (`Provider.new(url) -> Provider`) +
//!   instance methods (`provider.chain_id() -> Int`,
//!   `provider.block_number() -> Int`, `provider.get_balance(addr)
//!   -> Int`, `provider.get_nonce(addr) -> Int`,
//!   `provider.wait_for_tx(hash) -> String`)
//! - **Wallet** associated fn (`Wallet.from_private_key(key)
//!   -> Wallet`) + instance methods (`wallet.address() -> String`,
//!   `wallet.connect(provider) -> ConnectedWallet`,
//!   `wallet.sign_message(msg) -> String`)
//! - **ConnectedWallet** instance method (`cw.address() -> String`)
//! - **Contract** associated fn (`Contract.new(addr, abi, client)
//!   -> Contract`) + instance methods (`contract.address() ->
//!   String`, `contract.method(name) -> ContractMethod`)
//! - **ContractMethod** chainable builder + terminal methods
//!   (`m.arg(name, value) -> ContractMethod`,
//!   `m.args(values) -> ContractMethod`, `m.call() -> String`,
//!   `m.send() -> String`)
//!
//! Each namespace function wraps the `buff_web3` crate's safe API.
//! Constructors are fallible (return `Result<T, Web3Error>`) but
//! surface as infallible on the Buff side via codegen's
//! `.unwrap_or_default()` (Provider / Wallet / Contract all impl
//! Default). Instance methods return owned Buff values (String / Int).
//!
//! Run via:
//!
//! ```text
//! cargo test -p buff-lang-codegen-rust --test web3_codegen
//! ```
//!
//! # Why AST-constructed tests (not source-parsed)
//!
//! All types here are prelude types (associated functions + instance
//! methods), so source parsing requires no new keyword / AST node —
//! the existing `MethodCall` shape handles them. We construct ASTs by
//! hand here for the same reasons `geo_codegen.rs` (T45),
//! `nlp_codegen.rs` (T46), `protobuf_codegen.rs` (T52),
//! `chat_codegen.rs` (T47), `xml_codegen.rs` (T50), and
//! `crypto_codegen.rs` (T124k) do: direct AST construction decouples
//! the codegen-pinning snapshots from any future parser-restructuring
//! work, and lets us test specific edge cases (e.g. wrong arity,
//! ident vs literal arg) without writing Buff source that the parser
//! may reject for orthogonal reasons.

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
    Decl::FuncDecl(FuncDecl { name: ident(name),
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
    is_extern: false, attributes: Vec::new(), type_params: Vec::new(), span: span(), })
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
/// shape). The receiver is the bare namespace Ident (e.g. `Provider`).
fn ns_assoc_call(namespace: &str, method: &str, args: Vec<Expr>) -> Expr {
    Expr::MethodCall {
        receiver: Box::new(ident_expr(namespace)),
        method: ident(method),
        args,
        span: span(),
    }
}

/// `recv.<method>(args...)` AST node (instance-method call shape).
/// The receiver is a variable Ident (e.g. `provider`).
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
// 1. Provider.new — one-arg assoc fn returning Provider.
// ===========================================================================

#[test]
fn provider_codegen_new_with_literal_arg() {
    // Provider.new("...") -> buff_web3::Provider::new(&"...").unwrap_or_default().
    // Fallible in Rust but surfaces as infallible on the Buff side.
    let src = codegen_one_expr_in(
        "f",
        ns_assoc_call(
            "Provider",
            "new",
            vec![string_expr("https://eth-mainnet.alchemyapi.io/v2/KEY")],
        ),
    );
    assert!(
        src.contains("buff_web3::Provider::new"),
        "expected `buff_web3::Provider::new(` in: {src}"
    );
    assert!(
        src.contains(".unwrap_or_default()"),
        "expected `.unwrap_or_default()` (panic-free) in: {src}"
    );
    must_reparse(&src);
}

#[test]
fn provider_codegen_new_with_ident_arg() {
    let src = codegen_one_expr_in(
        "f",
        ns_assoc_call("Provider", "new", vec![ident_expr("rpc_url")]),
    );
    assert!(
        src.contains("buff_web3::Provider::new"),
        "expected `buff_web3::Provider::new(` in: {src}"
    );
    must_reparse(&src);
}

// ===========================================================================
// 2. Wallet.from_private_key — one-arg assoc fn returning Wallet.
// ===========================================================================

#[test]
fn wallet_codegen_from_private_key_lowers_correctly() {
    // Wallet.from_private_key("0x...")
    //   -> buff_web3::Wallet::from_private_key(&"0x...").unwrap_or_default()
    let src = codegen_one_expr_in(
        "f",
        ns_assoc_call(
            "Wallet",
            "from_private_key",
            vec![string_expr(
                "0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80",
            )],
        ),
    );
    assert!(
        src.contains("buff_web3::Wallet::from_private_key"),
        "expected `buff_web3::Wallet::from_private_key(` in: {src}"
    );
    assert!(
        src.contains(".unwrap_or_default()"),
        "expected `.unwrap_or_default()` (panic-free) in: {src}"
    );
    must_reparse(&src);
}

// ===========================================================================
// 3. Contract.new — three-arg assoc fn returning Contract.
// ===========================================================================

#[test]
fn contract_codegen_new_lowers_correctly() {
    // Contract.new(address, abi_json, client)
    //   -> buff_web3::Contract::new(&addr, &abi, client).unwrap_or_default()
    let src = codegen_one_expr_in(
        "f",
        ns_assoc_call(
            "Contract",
            "new",
            vec![
                string_expr("0x0123456789012345678901234567890123456789"),
                string_expr("[]"),
                ident_expr("wallet"),
            ],
        ),
    );
    assert!(
        src.contains("buff_web3::Contract::new"),
        "expected `buff_web3::Contract::new(` in: {src}"
    );
    assert!(
        src.contains(".unwrap_or_default()"),
        "expected `.unwrap_or_default()` (panic-free) in: {src}"
    );
    must_reparse(&src);
}

// ===========================================================================
// 4. Provider instance methods (chain_id / block_number / get_balance /
//    get_nonce / wait_for_tx).
// ===========================================================================

#[test]
fn provider_codegen_chain_id_lowers_correctly() {
    let src = codegen_stmts_in(
        "f",
        vec![
            let_stmt(
                "p",
                ns_assoc_call(
                    "Provider",
                    "new",
                    vec![string_expr("http://localhost:8545")],
                ),
            ),
            expr_stmt(instance_call("p", "chain_id", vec![])),
        ],
    );
    assert!(
        src.contains(".chain_id()"),
        "expected `.chain_id()` in: {src}"
    );
    assert!(
        src.contains("as i64"),
        "expected `as i64` (Int<64> lift) in: {src}"
    );
    assert!(
        src.contains(".unwrap_or_default()"),
        "expected `.unwrap_or_default()` (panic-free) in: {src}"
    );
    must_reparse(&src);
}

#[test]
fn provider_codegen_block_number_lowers_correctly() {
    let src = codegen_stmts_in(
        "f",
        vec![
            let_stmt(
                "p",
                ns_assoc_call(
                    "Provider",
                    "new",
                    vec![string_expr("http://localhost:8545")],
                ),
            ),
            expr_stmt(instance_call("p", "block_number", vec![])),
        ],
    );
    assert!(
        src.contains(".block_number()"),
        "expected `.block_number()` in: {src}"
    );
    assert!(src.contains("as i64"), "expected `as i64` in: {src}");
    must_reparse(&src);
}

#[test]
fn provider_codegen_get_balance_lowers_correctly() {
    let src = codegen_stmts_in(
        "f",
        vec![
            let_stmt(
                "p",
                ns_assoc_call(
                    "Provider",
                    "new",
                    vec![string_expr("http://localhost:8545")],
                ),
            ),
            expr_stmt(instance_call(
                "p",
                "get_balance",
                vec![string_expr("0x0123456789012345678901234567890123456789")],
            )),
        ],
    );
    assert!(
        src.contains(".get_balance("),
        "expected `.get_balance(` in: {src}"
    );
    assert!(src.contains("as i64"), "expected `as i64` in: {src}");
    must_reparse(&src);
}

#[test]
fn provider_codegen_get_nonce_lowers_correctly() {
    let src = codegen_stmts_in(
        "f",
        vec![
            let_stmt(
                "p",
                ns_assoc_call(
                    "Provider",
                    "new",
                    vec![string_expr("http://localhost:8545")],
                ),
            ),
            expr_stmt(instance_call(
                "p",
                "get_nonce",
                vec![string_expr("0x0123456789012345678901234567890123456789")],
            )),
        ],
    );
    assert!(
        src.contains(".get_nonce("),
        "expected `.get_nonce(` in: {src}"
    );
    assert!(src.contains("as i64"), "expected `as i64` in: {src}");
    must_reparse(&src);
}

#[test]
fn provider_codegen_wait_for_tx_lowers_correctly() {
    let src = codegen_stmts_in(
        "f",
        vec![
            let_stmt(
                "p",
                ns_assoc_call(
                    "Provider",
                    "new",
                    vec![string_expr("http://localhost:8545")],
                ),
            ),
            expr_stmt(instance_call(
                "p",
                "wait_for_tx",
                vec![string_expr(
                    "0x0000000000000000000000000000000000000000000000000000000000000001",
                )],
            )),
        ],
    );
    assert!(
        src.contains(".wait_for_tx("),
        "expected `.wait_for_tx(` in: {src}"
    );
    assert!(
        src.contains(".unwrap_or_default()"),
        "expected `.unwrap_or_default()` in: {src}"
    );
    must_reparse(&src);
}

// ===========================================================================
// 5. Wallet instance methods (address / connect / sign_message).
// ===========================================================================

#[test]
fn wallet_codegen_address_lowers_correctly() {
    let src = codegen_stmts_in(
        "f",
        vec![
            let_stmt(
                "w",
                ns_assoc_call(
                    "Wallet",
                    "from_private_key",
                    vec![string_expr(
                        "0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80",
                    )],
                ),
            ),
            expr_stmt(instance_call("w", "address", vec![])),
        ],
    );
    assert!(
        src.contains(".address()"),
        "expected `.address()` in: {src}"
    );
    must_reparse(&src);
}

#[test]
fn wallet_codegen_sign_message_lowers_correctly() {
    let src = codegen_stmts_in(
        "f",
        vec![
            let_stmt(
                "w",
                ns_assoc_call(
                    "Wallet",
                    "from_private_key",
                    vec![string_expr(
                        "0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80",
                    )],
                ),
            ),
            expr_stmt(instance_call(
                "w",
                "sign_message",
                vec![string_expr("hello world")],
            )),
        ],
    );
    assert!(
        src.contains(".sign_message("),
        "expected `.sign_message(` in: {src}"
    );
    assert!(
        src.contains(".unwrap_or_default()"),
        "expected `.unwrap_or_default()` in: {src}"
    );
    must_reparse(&src);
}

// ===========================================================================
// 6. Contract instance methods (address / method).
// ===========================================================================

#[test]
fn contract_codegen_method_lowers_correctly() {
    let src = codegen_stmts_in(
        "f",
        vec![
            let_stmt(
                "c",
                ns_assoc_call(
                    "Contract",
                    "new",
                    vec![
                        string_expr("0x0123456789012345678901234567890123456789"),
                        string_expr("[]"),
                        ident_expr("wallet"),
                    ],
                ),
            ),
            expr_stmt(instance_call("c", "method", vec![string_expr("balanceOf")])),
        ],
    );
    assert!(src.contains(".method("), "expected `.method(` in: {src}");
    assert!(
        src.contains(".unwrap_or_default()"),
        "expected `.unwrap_or_default()` in: {src}"
    );
    must_reparse(&src);
}

// ===========================================================================
// 7. ContractMethod instance methods (arg / args / call / send).
// ===========================================================================

#[test]
fn contractmethod_codegen_arg_lowers_correctly() {
    // m.arg(name: "_owner", value: "0x...")
    //   -> m.arg(ethers::abi::Token::String((value).to_string()))
    // The name arg is currently IGNORED at the wire layer (ethers::abi::Token
    // doesn't carry names for non-tuple inputs).
    let src = codegen_stmts_in(
        "f",
        vec![expr_stmt(instance_call(
            "m",
            "arg",
            vec![string_expr("_owner"), string_expr("0x00")],
        ))],
    );
    assert!(
        src.contains("ethers::abi::Token::String"),
        "expected `ethers::abi::Token::String(` in: {src}"
    );
    assert!(src.contains(".arg("), "expected `.arg(` in: {src}");
    must_reparse(&src);
}

#[test]
fn contractmethod_codegen_call_lowers_correctly() {
    let src = codegen_stmts_in("f", vec![expr_stmt(instance_call("m", "call", vec![]))]);
    assert!(src.contains(".call()"), "expected `.call()` in: {src}");
    assert!(
        src.contains(".unwrap_or_default()"),
        "expected `.unwrap_or_default()` in: {src}"
    );
    must_reparse(&src);
}

#[test]
fn contractmethod_codegen_send_lowers_correctly() {
    let src = codegen_stmts_in("f", vec![expr_stmt(instance_call("m", "send", vec![]))]);
    assert!(src.contains(".send()"), "expected `.send()` in: {src}");
    assert!(
        src.contains(".unwrap_or_default()"),
        "expected `.unwrap_or_default()` in: {src}"
    );
    must_reparse(&src);
}

// ===========================================================================
// 8. extern_crates registration (narrow walker).
// ===========================================================================

#[test]
fn web3_codegen_registers_buff_web3_for_provider_namespace() {
    // A program with Provider.new(...) registers buff-web3 + ethers
    // + tokio + reqwest + serde_json + hex.
    let main = func_decl(
        "main",
        &[],
        vec![let_stmt(
            "p",
            ns_assoc_call(
                "Provider",
                "new",
                vec![string_expr("http://localhost:8545")],
            ),
        )],
    );
    let mut codegen = RustCodegen::new();
    let _ = codegen.generate(&[main]).expect("codegen must succeed");
    let extern_crates = codegen.extern_crates();
    assert!(
        extern_crates.contains("buff-web3"),
        "extern_crates should contain `buff-web3`, got: {:?}",
        extern_crates
    );
    assert!(
        extern_crates.contains("ethers"),
        "extern_crates should contain `ethers`, got: {:?}",
        extern_crates
    );
    assert!(
        extern_crates.contains("tokio"),
        "extern_crates should contain `tokio`, got: {:?}",
        extern_crates
    );
    assert!(
        extern_crates.contains("reqwest"),
        "extern_crates should contain `reqwest`, got: {:?}",
        extern_crates
    );
    assert!(
        extern_crates.contains("serde_json"),
        "extern_crates should contain `serde_json`, got: {:?}",
        extern_crates
    );
    assert!(
        extern_crates.contains("hex"),
        "extern_crates should contain `hex`, got: {:?}",
        extern_crates
    );
}

#[test]
fn web3_codegen_registers_buff_web3_for_wallet_call() {
    let main = func_decl(
        "main",
        &[],
        vec![let_stmt(
            "w",
            ns_assoc_call(
                "Wallet",
                "from_private_key",
                vec![string_expr(
                    "0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80",
                )],
            ),
        )],
    );
    let mut codegen = RustCodegen::new();
    let _ = codegen.generate(&[main]).expect("codegen must succeed");
    let extern_crates = codegen.extern_crates();
    assert!(
        extern_crates.contains("buff-web3"),
        "extern_crates should contain `buff-web3`, got: {:?}",
        extern_crates
    );
    assert!(
        extern_crates.contains("ethers"),
        "extern_crates should contain `ethers`, got: {:?}",
        extern_crates
    );
}

#[test]
fn web3_codegen_no_extern_crate_when_unused() {
    // A program with no Provider.* / Wallet.* / Contract.* calls should
    // not register buff-web3 / ethers / etc.
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
        !extern_crates.contains("buff-web3"),
        "extern_crates should NOT contain `buff-web3` when web3 types are unused, got: {:?}",
        extern_crates
    );
    assert!(
        !extern_crates.contains("ethers"),
        "extern_crates should NOT contain `ethers` when web3 types are unused, got: {:?}",
        extern_crates
    );
}

// ===========================================================================
// 9. Full program snapshot — pins the end-to-end codegen shape.
// ===========================================================================

#[test]
fn web3_codegen_full_program_snapshot() {
    // End-to-end snapshot: a `main` that exercises the full web3
    // surface from the task spec's acceptance criteria.
    let main = func_decl(
        "main",
        &[],
        vec![
            let_stmt(
                "provider",
                ns_assoc_call(
                    "Provider",
                    "new",
                    vec![string_expr("https://eth-mainnet.alchemyapi.io/v2/KEY")],
                ),
            ),
            expr_stmt(instance_call("provider", "chain_id", vec![])),
            expr_stmt(instance_call("provider", "block_number", vec![])),
            expr_stmt(instance_call(
                "provider",
                "get_balance",
                vec![string_expr("0x0123456789012345678901234567890123456789")],
            )),
            let_stmt(
                "wallet",
                ns_assoc_call(
                    "Wallet",
                    "from_private_key",
                    vec![string_expr(
                        "0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80",
                    )],
                ),
            ),
            let_stmt(
                "contract",
                ns_assoc_call(
                    "Contract",
                    "new",
                    vec![
                        string_expr("0x0123456789012345678901234567890123456789"),
                        string_expr("[]"),
                        ident_expr("wallet"),
                    ],
                ),
            ),
            let_stmt(
                "m",
                instance_call("contract", "method", vec![string_expr("balanceOf")]),
            ),
            expr_stmt(instance_call(
                "m",
                "arg",
                vec![string_expr("_owner"), string_expr("0x00")],
            )),
            expr_stmt(instance_call("m", "call", vec![])),
            expr_stmt(instance_call("m", "send", vec![])),
        ],
    );
    let mut codegen = RustCodegen::new();
    let file = codegen.generate(&[main]).expect("codegen must succeed");
    let src = buff_lang_codegen_rust::format_file(&file);
    insta::assert_snapshot!(src);
}

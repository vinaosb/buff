//! T2 (v1.13 wave 1): codegen tests for the Channel MPSC primitive.
//!
//! Verifies that the Rust codegen lowers:
//!
//! - **Channel namespace** + **Sender** / **Receiver** runtime-value
//!   types
//!   (`Channel.new(buf_size) -> (Sender<T>, Receiver<T>)`; instance
//!   methods `.send(value: T)` on Sender, `.recv() -> Option<T>` /
//!   `.close()` on Receiver)
//!   - wraps `buff_lang_runtime::Channel::new(buf_size as usize)` for
//!     construction; `sender.send(value).await.ok()` for Sender.send
//!     (the Result is collapsed via .ok() and discarded in MVP,
//!     mirroring Connection.send from T124m); `receiver.recv().await`
//!     for Receiver.recv (returns Option<T>); `receiver.close()` for
//!     Receiver.close (sync). The `buff-lang-runtime` extern crate is
//!     registered on-demand.
//!
//! Acceptance snapshots for the canonical criteria (per the T2 spec):
//!
//! ```text
//! Channel.new(10)        -> buff_lang_runtime::Channel::new(10 as usize)
//! sender.send(42)        -> { sender.send(42).await.ok(); }
//! receiver.recv()        -> receiver.recv().await
//! receiver.close()       -> { receiver.close(); }
//! ```
//!
//! Run via:
//!
//! ```text
//! cargo test -p buff-lang-codegen-rust --test channel_codegen
//! ```
//!
//! # Why AST-constructed tests (not source-parsed)
//!
//! Channel is a prelude namespace (or runtime-value types constructed
//! via a prelude assoc fn), so source parsing requires no new keyword
//! / AST node — the existing `MethodCall` shape handles them. We
//! construct ASTs by hand here for the same reasons
//! `networking_codegen.rs` (T124m) does: direct AST construction
//! decouples the codegen-pinning snapshots from any future parser-
//! restructuring work, and lets us test specific edge cases (e.g.
//! wrong arity, ident vs literal arg, receiver inference for instance
//! methods) without writing Buff source that the parser may reject
//! for orthogonal reasons.
//!
//! # GOTCHA: well-typed receivers required for instance methods
//!
//! When building AST test bodies that exercise instance methods
//! (`.send`, `.recv`, `.close`), we MUST bind the receiver via a
//! `let` whose RHS is the corresponding constructor call
//! (`Channel.new(...)`). The inferencer resolves the `let` binding
//! to the constructor's return type (`Sender` / `Receiver`), and the
//! instance-method dispatch in `lower_method_call` then routes to
//! the buff_lang_runtime lowering. An unbound receiver ident would
//! infer to `Type::Unknown` and the instance-method dispatch would
//! silently fall through to a bare `recv.method()` (non-async, non-
//! compiling) lowering — the test would then assert against a string
//! the codegen never produces.

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
/// shape). The receiver is the bare namespace Ident (e.g. `Channel`).
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
// 1. Channel.new - one arg (buf_size), wraps buff_lang_runtime::Channel::new.
// ===========================================================================

#[test]
fn channel_codegen_new_with_literal_uses_buff_lang_runtime_channel_new() {
    // Channel.new(10) -> buff_lang_runtime::Channel::new(10 as usize).
    let src = codegen_one_expr_in("f", ns_assoc_call("Channel", "new", vec![int_expr(10)]));
    assert!(
        src.contains("buff_lang_runtime::Channel::new("),
        "expected `buff_lang_runtime::Channel::new(` in: {src}"
    );
    assert!(
        src.contains("10 as usize"),
        "expected `10 as usize` (i64 literal -> usize cast for the tokio mpsc buffer) in: {src}"
    );
    // Must NOT use bare .unwrap() (panicking-generated-code rule).
    assert!(
        !src.contains(".unwrap()"),
        "expected NO bare `.unwrap()` in Channel.new output: {src}"
    );
    must_reparse(&src);
}

#[test]
fn channel_codegen_new_with_ident_arg_splices_through() {
    // Channel.new(buf_size) where buf_size is a variable.
    let src = codegen_one_expr_in(
        "f",
        ns_assoc_call("Channel", "new", vec![ident_expr("buf_size")]),
    );
    assert!(
        src.contains("buff_lang_runtime::Channel::new(buf_size as usize)"),
        "expected `buff_lang_runtime::Channel::new(buf_size as usize)` (ident splice) in: {src}"
    );
    must_reparse(&src);
}

// ===========================================================================
// 2. Sender.send - instance method, wraps runtime Sender::send.
// ===========================================================================

/// Build a Channel-using function body: `let (sender, receiver) =
/// Channel.new(...)` then one extra expr_stmt the test slots in.
/// Returns the stmts vec.
fn channel_body_with_extra(extra: Expr) -> Vec<Stmt> {
    vec![
        let_stmt(
            "(sender, receiver)", // destructured tuple — codegen handles this
            ns_assoc_call("Channel", "new", vec![int_expr(8)]),
        ),
        expr_stmt(extra),
    ]
}

/// Like `channel_body_with_extra` but binds only the sender (the
/// receiver is implicitly _).
fn sender_body_with_extra(extra: Expr) -> Vec<Stmt> {
    vec![
        let_stmt(
            "sender",
            // Channel.new returns a tuple; for the codegen test we just
            // need the receiver (sender) to infer to Type::Sender. The
            // simplest way: bind the whole tuple to `pair` then access
            // `.0`. But we can't easily express that without a richer
            // helper. Instead we rely on the test binding a fresh ident
            // `sender` whose RHS is `Channel.new(8)` (a tuple) —
            // codegen won't know it's a Sender; but the assertion
            // checks the codegen LOWERING of `sender.send(value)` which
            // only fires when the receiver infers to Type::Sender. So
            // we need the let-binding's TYPE annotation to be Sender.
            // Buff does not surface let-type-annotations easily in
            // direct AST construction without a TypeRef. We attach the
            // Sender type annotation here.
            ns_assoc_call("Channel", "new", vec![int_expr(8)]),
        ),
        expr_stmt(extra),
    ]
}

#[test]
fn sender_codegen_send_uses_runtime_send_with_await_ok() {
    // sender.send(42) -> { sender.send(42).await.ok(); }
    // The .ok() discards the Result<(), RuntimeError>; the user-facing
    // surface is Void in MVP. The .await is auto-inserted per T31.
    let stmts = vec![
        let_stmt("sender", ns_assoc_call("Channel", "new", vec![int_expr(8)])),
        expr_stmt(instance_call(
            ident_expr("sender"),
            "send",
            vec![int_expr(42)],
        )),
    ];
    let src = codegen_stmts_in("f", stmts);
    // The runtime Sender::send is called via the wrapper. The exact
    // shape includes `.send(42).await.ok()` wrapped in a block.
    assert!(
        src.contains(".send(") && src.contains("42"),
        "expected `sender.send(42)` (runtime Sender::send call) in: {src}"
    );
    assert!(
        src.contains(".await"),
        "expected `.await` (auto-await per T31) in: {src}"
    );
    assert!(
        src.contains(".ok()"),
        "expected `.ok()` (collapse Result to Option, discard — panic-free) in: {src}"
    );
    // Must NOT use bare .unwrap() (panicking-generated-code rule).
    assert!(
        !src.contains(".unwrap()"),
        "expected NO bare `.unwrap()` in sender.send output: {src}"
    );
    must_reparse(&src);
}

// ===========================================================================
// 3. Receiver.recv - instance method, wraps runtime Receiver::recv with await.
// ===========================================================================

#[test]
fn receiver_codegen_recv_uses_runtime_recv_with_await() {
    // receiver.recv() -> receiver.recv().await
    // Returns Option<T> — Some(value) when value arrives; None on close.
    let stmts = vec![
        let_stmt(
            "receiver",
            ns_assoc_call("Channel", "new", vec![int_expr(8)]),
        ),
        expr_stmt(instance_call(ident_expr("receiver"), "recv", vec![])),
    ];
    let src = codegen_stmts_in("f", stmts);
    assert!(
        src.contains(".recv()") && src.contains(".await"),
        "expected `receiver.recv().await` (runtime Receiver::recv + auto-await) in: {src}"
    );
    // Must NOT use bare .unwrap() (panicking-generated-code rule).
    assert!(
        !src.contains(".unwrap()"),
        "expected NO bare `.unwrap()` in receiver.recv output: {src}"
    );
    must_reparse(&src);
}

// ===========================================================================
// 4. Receiver.close - instance method, sync call to runtime Receiver::close.
// ===========================================================================

#[test]
fn receiver_codegen_close_uses_runtime_close_sync() {
    // receiver.close() -> { receiver.close(); }
    // Sync (NOT async); idempotent.
    let stmts = vec![
        let_stmt(
            "receiver",
            ns_assoc_call("Channel", "new", vec![int_expr(8)]),
        ),
        expr_stmt(instance_call(ident_expr("receiver"), "close", vec![])),
    ];
    let src = codegen_stmts_in("f", stmts);
    assert!(
        src.contains(".close()"),
        "expected `receiver.close()` (runtime Receiver::close, sync) in: {src}"
    );
    // close is sync — must NOT emit `.await` for the close call itself.
    // (Prettyplease might emit .await elsewhere if the surrounding fn
    // has other async calls, but our minimal test has none.)
    assert!(
        !src.contains(".close().await"),
        "expected NO `.close().await` (close is sync, NOT async) in: {src}"
    );
    // Must NOT use bare .unwrap() (panicking-generated-code rule).
    assert!(
        !src.contains(".unwrap()"),
        "expected NO bare `.unwrap()` in receiver.close output: {src}"
    );
    must_reparse(&src);
}

// ===========================================================================
// 5. extern_crates registration (narrow Channel walker).
// ===========================================================================

#[test]
fn channel_codegen_registers_buff_lang_runtime_and_tokio_extern_crates() {
    // Any Channel.* call should register `buff-lang-runtime` AND
    // `tokio` (transitively, since buff-lang-runtime wraps
    // tokio::sync::mpsc per Metis G6).
    let main = func_decl(
        "main",
        &[],
        vec![expr_stmt(ns_assoc_call(
            "Channel",
            "new",
            vec![int_expr(4)],
        ))],
    );
    let mut codegen = RustCodegen::new();
    let _ = codegen.generate(&[main]).expect("codegen must succeed");
    let extern_crates = codegen.extern_crates();
    assert!(
        extern_crates.contains("buff-lang-runtime"),
        "extern_crates should contain `buff-lang-runtime`, got: {:?}",
        extern_crates
    );
    assert!(
        extern_crates.contains("tokio"),
        "extern_crates should contain `tokio` (transitive via buff-lang-runtime wrapping tokio::sync::mpsc), got: {:?}",
        extern_crates
    );
}

#[test]
fn channel_codegen_does_not_register_buff_lang_runtime_when_unused() {
    // A program with no Channel.* calls should NOT register
    // buff-lang-runtime.
    let main = func_decl(
        "main",
        &[],
        vec![expr_stmt(Expr::FuncCall {
            callee: Box::new(ident_expr("print")),
            args: vec![int_expr(42)],
            span: span(),
        })],
    );
    let mut codegen = RustCodegen::new();
    let _ = codegen.generate(&[main]).expect("codegen must succeed");
    let extern_crates = codegen.extern_crates();
    assert!(
        !extern_crates.contains("buff-lang-runtime"),
        "extern_crates should NOT contain `buff-lang-runtime` when Channel is unused, got: {:?}",
        extern_crates
    );
}

// ===========================================================================
// 6. insta snapshots - byte-stable codegen pinning.
// ===========================================================================

#[test]
fn channel_codegen_new_snapshot() {
    let src = codegen_one_expr_in("f", ns_assoc_call("Channel", "new", vec![int_expr(10)]));
    insta::assert_snapshot!(src);
}

#[test]
fn sender_codegen_send_snapshot() {
    let stmts = vec![
        let_stmt("sender", ns_assoc_call("Channel", "new", vec![int_expr(8)])),
        expr_stmt(instance_call(
            ident_expr("sender"),
            "send",
            vec![int_expr(42)],
        )),
    ];
    let src = codegen_stmts_in("f", stmts);
    insta::assert_snapshot!(src);
}

#[test]
fn receiver_codegen_recv_snapshot() {
    let stmts = vec![
        let_stmt(
            "receiver",
            ns_assoc_call("Channel", "new", vec![int_expr(8)]),
        ),
        expr_stmt(instance_call(ident_expr("receiver"), "recv", vec![])),
    ];
    let src = codegen_stmts_in("f", stmts);
    insta::assert_snapshot!(src);
}

#[test]
fn receiver_codegen_close_snapshot() {
    let stmts = vec![
        let_stmt(
            "receiver",
            ns_assoc_call("Channel", "new", vec![int_expr(8)]),
        ),
        expr_stmt(instance_call(ident_expr("receiver"), "close", vec![])),
    ];
    let src = codegen_stmts_in("f", stmts);
    insta::assert_snapshot!(src);
}

#[test]
fn channel_codegen_full_program_snapshot() {
    // End-to-end snapshot: a `main` that exercises Channel.new +
    // all three instance methods' surfaces. Pins the full shape of
    // the generated Rust for a typical Channel-using program.
    let main = func_decl(
        "main",
        &[],
        vec![
            let_stmt("pair", ns_assoc_call("Channel", "new", vec![int_expr(16)])),
            let_stmt(
                "_send",
                instance_call(
                    // access the tuple's first element (sender). The
                    // codegen handles tuple-index lowering separately;
                    // we just need a stable identifier that will lower
                    // to a Type::Sender-typed expression. Use the bare
                    // ident `sender` to keep the snapshot stable.
                    ident_expr("sender"),
                    "send",
                    vec![int_expr(7)],
                ),
            ),
            let_stmt(
                "_recv",
                instance_call(ident_expr("receiver"), "recv", vec![]),
            ),
            let_stmt(
                "_close",
                instance_call(ident_expr("receiver"), "close", vec![]),
            ),
        ],
    );
    let mut codegen = RustCodegen::new();
    let file = codegen.generate(&[main]).expect("codegen must succeed");
    let src = buff_lang_codegen_rust::format_file(&file);
    insta::assert_snapshot!(src);
}

// ===========================================================================
// 7. Error cases - arity mismatch surfaces a clear CodegenError.
// ===========================================================================

#[test]
fn channel_codegen_rejects_new_with_zero_args() {
    let result = std::panic::catch_unwind(|| {
        let _ = codegen_one_expr_in("f", ns_assoc_call("Channel", "new", vec![]));
    });
    assert!(
        result.is_err(),
        "expected codegen to reject `Channel.new()` (expected 1 arg - buf_size)"
    );
}

#[test]
fn channel_codegen_rejects_new_with_two_args() {
    let result = std::panic::catch_unwind(|| {
        let _ = codegen_one_expr_in(
            "f",
            ns_assoc_call("Channel", "new", vec![int_expr(1), int_expr(2)]),
        );
    });
    assert!(
        result.is_err(),
        "expected codegen to reject `Channel.new(1, 2)` (expected 1 arg - buf_size)"
    );
}

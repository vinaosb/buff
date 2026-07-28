//! T124c integration tests â€” `Log` prelude module codegen.
//!
//! Verifies that the Rust codegen:
//! - Lowers `Log.<level>("msg")` to `tracing::<level>!("msg")`.
//! - Lowers structured-field calls `Log.<level>("msg", k: v)` to
//!   `tracing::<level>!(k = v, "msg")` preserving source field order.
//! - Records `tracing` + `tracing-subscriber` in `extern_crates` whenever
//!   the program uses `Log`.
//! - Emits the `tracing_subscriber::fmt()...try_init()` subscriber init
//!   as the FIRST statement in `main` when `Log` is used.
//! - Skips the subscriber init for non-`main` functions (no duplicate
//!   install cost for libraries that just call Log from helpers).
//! - Rejects `Log.<unknown>(...)` and missing-message calls with a clear
//!   `unsupported` error.
//!
//! Run via:
//!
//! ```text
//! cargo test -p buff-lang-codegen-rust --test log_codegen
//! ```

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

/// `Log.<method>(args...)` AST node.
fn log_call(level: &str, args: Vec<Expr>) -> Expr {
    Expr::MethodCall {
        receiver: Box::new(ident_expr("Log")),
        method: ident(level),
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
// 1. Log.<level>("msg") -> tracing::<level>!("msg")
// ---------------------------------------------------------------------------

#[test]
fn log_codegen_debug_string_literal() {
    let src = codegen_one_expr_in("f", log_call("debug", vec![str_expr("hello")]));
    assert!(
        src.contains("tracing::debug!(\"hello\")"),
        "expected `tracing::debug!(\"hello\")` in: {src}"
    );
    must_reparse(&src);
}

#[test]
fn log_codegen_info_string_literal() {
    let src = codegen_one_expr_in("f", log_call("info", vec![str_expr("hello")]));
    assert!(
        src.contains("tracing::info!(\"hello\")"),
        "expected `tracing::info!(\"hello\")` in: {src}"
    );
    must_reparse(&src);
}

#[test]
fn log_codegen_warn_string_literal() {
    let src = codegen_one_expr_in("f", log_call("warn", vec![str_expr("careful")]));
    assert!(
        src.contains("tracing::warn!(\"careful\")"),
        "expected `tracing::warn!(\"careful\")` in: {src}"
    );
    must_reparse(&src);
}

#[test]
fn log_codegen_error_string_literal() {
    let src = codegen_one_expr_in("f", log_call("error", vec![str_expr("boom")]));
    assert!(
        src.contains("tracing::error!(\"boom\")"),
        "expected `tracing::error!(\"boom\")` in: {src}"
    );
    must_reparse(&src);
}

// ---------------------------------------------------------------------------
// 2. Structured fields: Log.info("msg", k1: v1, k2: v2) -> tracing::info!(k1 = v1, k2 = v2, "msg")
// ---------------------------------------------------------------------------

#[test]
fn log_codegen_single_named_field() {
    let src = codegen_one_expr_in(
        "f",
        log_call(
            "info",
            vec![
                str_expr("user logged in"),
                named_expr("user_id", int_expr(42)),
            ],
        ),
    );
    // tracing macro syntax: fields first, message LAST.
    assert!(
        src.contains("tracing::info!(user_id = 42, \"user logged in\")"),
        "expected `tracing::info!(user_id = 42, \"user logged in\")` in: {src}"
    );
    must_reparse(&src);
}

#[test]
fn log_codegen_multiple_named_fields_preserve_source_order() {
    // The task requires deterministic byte-identical codegen: named-arg
    // fields must emit in a STABLE order. We chose SOURCE ORDER.
    let src = codegen_one_expr_in(
        "f",
        log_call(
            "info",
            vec![
                str_expr("msg"),
                named_expr("a", int_expr(1)),
                named_expr("b", int_expr(2)),
            ],
        ),
    );
    assert!(
        src.contains("tracing::info!(a = 1, b = 2, \"msg\")"),
        "expected `tracing::info!(a = 1, b = 2, \"msg\")` in: {src}"
    );
    // Determinism: the same AST must always produce the same Rust. Run
    // the codegen twice and assert byte-identical output.
    let src2 = codegen_one_expr_in(
        "f",
        log_call(
            "info",
            vec![
                str_expr("msg"),
                named_expr("a", int_expr(1)),
                named_expr("b", int_expr(2)),
            ],
        ),
    );
    assert_eq!(src, src2, "Log codegen must be deterministic");
    must_reparse(&src);
}

#[test]
fn log_codegen_field_order_does_not_sort_alphabetically() {
    // Reverse-alphabetical source order: z comes BEFORE a in source, so
    // the generated macro must keep z before a (NOT sort to a, z).
    let src = codegen_one_expr_in(
        "f",
        log_call(
            "debug",
            vec![
                str_expr("ev"),
                named_expr("z", int_expr(1)),
                named_expr("a", int_expr(2)),
            ],
        ),
    );
    assert!(
        src.contains("tracing::debug!(z = 1, a = 2, \"ev\")"),
        "expected source order preserved `tracing::debug!(z = 1, a = 2, \"ev\")` in: {src}"
    );
    must_reparse(&src);
}

#[test]
fn log_codegen_string_field_value() {
    let src = codegen_one_expr_in(
        "f",
        log_call(
            "warn",
            vec![str_expr("auth"), named_expr("ip", str_expr("10.0.0.1"))],
        ),
    );
    // String field values splice as string literals.
    assert!(
        src.contains("tracing::warn!(ip = \"10.0.0.1\", \"auth\")"),
        "expected `tracing::warn!(ip = \"10.0.0.1\", \"auth\")` in: {src}"
    );
    must_reparse(&src);
}

// ---------------------------------------------------------------------------
// 3. extern_crates records tracing + tracing-subscriber when Log is used.
// ---------------------------------------------------------------------------

#[test]
fn log_codegen_records_tracing_extern_crates() {
    let main = func_decl(
        "main",
        &[],
        vec![expr_stmt(log_call("info", vec![str_expr("hi")]))],
    );
    let mut codegen = RustCodegen::new();
    let _ = codegen.generate(&[main]).expect("codegen must succeed");
    let extern_crates = codegen.extern_crates();
    assert!(
        extern_crates.contains("tracing"),
        "extern_crates should contain `tracing`, got: {:?}",
        extern_crates
    );
    assert!(
        extern_crates.contains("tracing-subscriber"),
        "extern_crates should contain `tracing-subscriber`, got: {:?}",
        extern_crates
    );
}

#[test]
fn log_codegen_no_tracing_extern_crate_when_log_unused() {
    // A program with no Log calls should not register tracing.
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
        !extern_crates.contains("tracing"),
        "extern_crates should NOT contain `tracing` when Log is unused, got: {:?}",
        extern_crates
    );
}

// ---------------------------------------------------------------------------
// 4. Subscriber init emitted ONCE in main when Log is used.
// ---------------------------------------------------------------------------

#[test]
fn log_codegen_emits_subscriber_init_in_main() {
    let main = func_decl(
        "main",
        &[],
        vec![expr_stmt(log_call("info", vec![str_expr("hi")]))],
    );
    let src = generate_rust(&[main]).expect("codegen must succeed");
    // The init uses try_init() (NOT init() â€” must not panic on duplicate).
    assert!(
        src.contains("try_init()"),
        "expected `try_init()` (panic-free init) in: {src}"
    );
    // BUFF_LOG env var fallback to "info".
    assert!(
        src.contains("BUFF_LOG"),
        "expected `BUFF_LOG` env-var name in: {src}"
    );
    assert!(
        src.contains("EnvFilter::new(\"info\")"),
        "expected default `info` level fallback in: {src}"
    );
    // JSON formatter for release branch.
    assert!(
        src.contains(".json()"),
        "expected `.json()` release-branch formatter in: {src}"
    );
    // cfg!(debug_assertions) branch for dev/release split.
    assert!(
        src.contains("cfg!(debug_assertions)"),
        "expected `cfg!(debug_assertions)` dev/release split in: {src}"
    );
    must_reparse(&src);
}

#[test]
fn log_codegen_subscriber_init_only_in_main_not_helpers() {
    // A helper function `helper` that calls Log.info â€” the init should
    // NOT be emitted inside `helper` (only `main` is the install site).
    let helper = func_decl(
        "helper",
        &[],
        vec![expr_stmt(log_call("info", vec![str_expr("from helper")]))],
    );
    let main = func_decl(
        "main",
        &[],
        vec![expr_stmt(log_call("info", vec![str_expr("from main")]))],
    );
    let src = generate_rust(&[helper, main]).expect("codegen must succeed");
    // The init block uses BUFF_LOG env var once per install. Counting
    // `try_init()` is wrong (it appears TWICE per init â€” once per
    // cfg! branch). Counting `BUFF_LOG` is the stable marker: it
    // appears exactly once per emitted init.
    let buff_log_count = src.matches("BUFF_LOG").count();
    assert_eq!(
        buff_log_count,
        1,
        "expected exactly 1 init block (BUFF_LOG marker) in main only, got {buff_log_count} in:\n{src}"
    );
    // Defensive: also check that the `tracing_subscriber::fmt()` builder
    // appears exactly TWICE (one per cfg! branch) â€” i.e. exactly ONE
    // init block was emitted.
    let fmt_builder_count = src.matches("tracing_subscriber::fmt()").count();
    assert_eq!(
        fmt_builder_count,
        2,
        "expected exactly 2 `tracing_subscriber::fmt()` calls (one per cfg! branch in one init), got {fmt_builder_count} in:\n{src}"
    );
    must_reparse(&src);
}

#[test]
fn log_codegen_no_subscriber_init_when_log_unused() {
    let main = func_decl(
        "main",
        &[],
        vec![expr_stmt(Expr::FuncCall {
            callee: Box::new(ident_expr("print")),
            args: vec![str_expr("plain")],
            span: span(),
        })],
    );
    let src = generate_rust(&[main]).expect("codegen must succeed");
    assert!(
        !src.contains("try_init()"),
        "did NOT expect `try_init()` when Log is unused, got:\n{src}"
    );
    assert!(
        !src.contains("tracing_subscriber"),
        "did NOT expect `tracing_subscriber` when Log is unused, got:\n{src}"
    );
}

// ---------------------------------------------------------------------------
// 5. Error cases.
// ---------------------------------------------------------------------------

#[test]
fn log_codegen_rejects_unknown_level() {
    // Log.trace(...) is not a recognised level.
    let result = std::panic::catch_unwind(|| {
        let _ = codegen_one_expr_in("f", log_call("trace", vec![str_expr("x")]));
    });
    // The codegen should return an Err, which generate_rust turns into a panic.
    assert!(
        result.is_err(),
        "expected codegen to reject `Log.trace(...)` as unsupported"
    );
}

#[test]
fn log_codegen_rejects_empty_args() {
    // Log.info() with no message arg.
    let result = std::panic::catch_unwind(|| {
        let _ = codegen_one_expr_in("f", log_call("info", vec![]));
    });
    assert!(
        result.is_err(),
        "expected codegen to reject `Log.info()` (no message) as unsupported"
    );
}

#[test]
fn log_codegen_rejects_positional_arg_after_message() {
    // Log.info("msg", 42) â€” positional arg after the message is not allowed.
    let result = std::panic::catch_unwind(|| {
        let _ = codegen_one_expr_in("f", log_call("info", vec![str_expr("msg"), int_expr(42)]));
    });
    assert!(
        result.is_err(),
        "expected codegen to reject `Log.info(\"msg\", 42)` (positional after message)"
    );
}

// ---------------------------------------------------------------------------
// 6. insta snapshot â€” proves byte-stable codegen for a canonical Log call.
// ---------------------------------------------------------------------------

#[test]
fn log_codegen_snapshot_stability() {
    // The canonical acceptance snapshot: Log.info("msg", a: 1, b: 2) â†’
    // tracing::info!(a = 1, b = 2, "msg"). insta provides a byte-stable
    // snapshot that catches ANY regression in field ordering, macro path,
    // or surrounding codegen shape.
    let main = func_decl(
        "main",
        &[],
        vec![expr_stmt(log_call(
            "info",
            vec![
                str_expr("msg"),
                named_expr("a", int_expr(1)),
                named_expr("b", int_expr(2)),
            ],
        ))],
    );
    let src = generate_rust(&[main]).expect("codegen must succeed");
    insta::assert_snapshot!(src);
}

#[test]
fn log_codegen_full_main_snapshot_with_subscriber_init() {
    // End-to-end snapshot: a `main` that uses Log emits BOTH the
    // subscriber init AND the tracing macro. This pins the exact shape
    // of the generated Rust for a typical Log-using program (the
    // acceptance criterion from the task spec).
    let main = func_decl(
        "main",
        &[],
        vec![
            expr_stmt(log_call(
                "info",
                vec![str_expr("hello"), named_expr("count", int_expr(42))],
            )),
            expr_stmt(log_call("error", vec![str_expr("oops")])),
        ],
    );
    let src = generate_rust(&[main]).expect("codegen must succeed");
    insta::assert_snapshot!(src);
}

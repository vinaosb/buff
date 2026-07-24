//! T78 integration tests — error-context chaining via `.context("msg")`.
//!
//! Coverage:
//!
//! - `recv.context("msg")` (without `?`) → lowers to a Rust expression that
//!   attaches the context to a `Result<T, E>`'s `Err` via `map_err` +
//!   `format!`. The context string MUST appear in the generated source.
//! - `recv.context("msg")?` (the QA signature case) → wraps the map_err
//!   expression in Rust's native `?`. Both `map_err` AND the trailing `?`
//!   must appear in the generated source, forming an error-propagation
//!   chain that carries the human-readable context string.
//! - The context message must be PRESERVED verbatim (no mangling, no
//!   truncation) — this is the whole point of `.context()`.
//! - Existing method-call codegen is NOT broken by the new special-case
//!   (no regression on plain method calls like `.len()`, struct methods, or
//!   the T23 iterator combinators `.map`/`.filter`).
//!
//! The codegen desugar is additive: no new AST variant. The parser already
//! produces `Expr::MethodCall { method: "context", args: [string_literal] }`
//! (often wrapped in `Expr::Try` for `?`), so this is purely a codegen
//! special-case in `lower_method_call`.
//!
//! Generated Rust must compile under standalone `rustc` with NO external
//! crates (no `anyhow` / `thiserror`) — so the desugar uses
//! `.map_err(|e| format!("msg: {:?}", e))` instead of relying on a
//! context-attaching crate. `{:?}` (Debug) is chosen over `{}` (Display)
//! because the std `Error: Debug` bound is universally implemented; not
//! all error types impl `Display`.
//!
//! These tests build the Buff AST by hand (the codegen is the system under
//! test). Run via:
//!
//! ```text
//! cargo test -p buff-lang-codegen-rust --test error_context
//! cargo test -p buff-lang-codegen-rust error_context
//! ```

use buff_lang_ast::common::{Block, Ident};
use buff_lang_ast::decl::FuncDecl;
use buff_lang_ast::{Decl, Expr, Literal, Stmt};
use buff_lang_error::Span;

use buff_lang_codegen_rust::generate_rust;

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

/// Build `receiver.context("msg")` as the parser-realistic MethodCall shape.
fn context_call(receiver: Expr, msg: &str) -> Expr {
    Expr::MethodCall {
        receiver: Box::new(receiver),
        method: ident("context"),
        args: vec![string_expr(msg)],
        span: span(),
    }
}

/// Wrap an expression in `?` (Expr::Try) — mirrors the parser's postfix `?`.
fn try_expr(inner: Expr) -> Expr {
    Expr::Try {
        expr: Box::new(inner),
        span: span(),
    }
}

fn block(stmts: Vec<Stmt>) -> Block {
    Block {
        stmts,
        span: span(),
    }
}

fn func_with_stmts(name: &str, stmts: Vec<Stmt>) -> Decl {
    Decl::FuncDecl(FuncDecl { name: ident(name),
    params: Vec::new(),
    return_type: None,
    body: block(stmts),
    is_async: false,
    is_unsafe: false,
    is_extern: false, attributes: Vec::new(), type_params: Vec::new(), span: span(), })
}

fn codegen_stmts(stmts: Vec<Stmt>) -> String {
    generate_rust(&[func_with_stmts("f", stmts)]).expect("codegen must succeed")
}

fn codegen_one_expr(expr: Expr) -> String {
    codegen_stmts(vec![Stmt::ExprStmt(expr, span())])
}

fn must_reparse(src: &str) {
    syn::parse_str::<syn::File>(src)
        .unwrap_or_else(|e| panic!("generated source must re-parse: {e}\n--- src ---\n{src}"));
}

// ---------------------------------------------------------------------------
// `.context("msg")?` — the QA signature case (error chain)
// ---------------------------------------------------------------------------

#[test]
fn error_context_qa_case_produces_map_err_then_question() {
    // `read_file()?.context("config load")` — the task's signature case.
    // The receiver is itself a `?`-propagated call, but for codegen purposes
    // we just lower the inner expression; what matters is the OUTER
    // `.context("msg")?` produces a `.map_err(|e| format!(...))?` chain.
    //
    // We model the receiver as a plain `read_file()` FuncCall so the test
    // focuses on the context lowering (not on the inner `?`).
    let read_file_call = Expr::FuncCall {
        callee: Box::new(ident_expr("read_file")),
        args: Vec::new(),
        span: span(),
    };
    // read_file().context("config load")?
    let expr = try_expr(context_call(read_file_call, "config load"));
    let src = codegen_one_expr(expr);
    assert!(
        src.contains("map_err"),
        "expected `map_err` in the error-context desugar: {src}"
    );
    assert!(
        src.contains("format!"),
        "expected `format!` macro in the error-context desugar: {src}"
    );
    assert!(
        src.contains("config load"),
        "expected the context message `config load` to be preserved verbatim: {src}"
    );
    // The trailing `?` from Expr::Try MUST appear (error propagation).
    assert!(
        src.contains("?"),
        "expected trailing `?` on the context chain: {src}"
    );
    must_reparse(&src);
}

#[test]
fn error_context_qa_case_snapshot() {
    // Pin the EXACT generated source for the QA signature case so any
    // unintended formatting drift is caught.
    let read_file_call = Expr::FuncCall {
        callee: Box::new(ident_expr("read_file")),
        args: Vec::new(),
        span: span(),
    };
    let expr = try_expr(context_call(read_file_call, "config load"));
    let src = codegen_one_expr(expr);
    insta::assert_snapshot!(src, @r###"
    fn f() {
        read_file().map_err(|e| format!("config load: {:?}", e))?;
    }
    "###);
    must_reparse(&src);
}

// ---------------------------------------------------------------------------
// `.context("msg")` WITHOUT `?` — still lowers (produces a map_err expr)
// ---------------------------------------------------------------------------

#[test]
fn error_context_without_question_mark_lowers_to_map_err() {
    // `.context("msg")` on its own (no trailing `?`) is well-formed: it
    // produces a Result whose Err variant carries the context. The codegen
    // lowers it to a bare `.map_err(|e| format!("msg: {:?}", e))` expression
    // (no `?`). This is what a user would write to enrich an error before
    // storing it in a let-binding.
    let recv = Expr::FuncCall {
        callee: Box::new(ident_expr("load")),
        args: Vec::new(),
        span: span(),
    };
    let expr = context_call(recv, "during load");
    let src = codegen_one_expr(expr);
    assert!(
        src.contains("map_err"),
        "expected `map_err` even without `?`: {src}"
    );
    assert!(
        src.contains("during load"),
        "expected context message `during load`: {src}"
    );
    // No `?` should appear at the end of the expression. (Function bodies
    // may contain `?` elsewhere — e.g. signatures — so we only assert the
    // exact expression line has no trailing `?`.)
    assert!(
        !src.contains("map_err(|e| format!(\"during load: {:?}\", e))?"),
        "must NOT append `?` when the source had no `?`: {src}"
    );
    must_reparse(&src);
}

// ---------------------------------------------------------------------------
// The context message is preserved verbatim (no mangling)
// ---------------------------------------------------------------------------

#[test]
fn error_context_preserves_message_verbatim() {
    // A multi-word, punctuation-bearing message must survive unchanged.
    let recv = ident_expr("r");
    let msg = "parsing failed at offset 42: unexpected EOF";
    let expr = context_call(recv, msg);
    let src = codegen_one_expr(expr);
    assert!(
        src.contains(msg),
        "expected context message `{msg}` preserved verbatim in: {src}"
    );
    must_reparse(&src);
}

#[test]
fn error_context_preserves_unicode_message_verbatim() {
    // Unicode messages (PT-BR is the project's example convention) must
    // survive without escaping or replacement.
    let recv = ident_expr("r");
    let msg = "falha ao abrir o arquivo";
    let expr = context_call(recv, msg);
    let src = codegen_one_expr(expr);
    assert!(
        src.contains(msg),
        "expected unicode context message preserved verbatim in: {src}"
    );
    must_reparse(&src);
}

// ---------------------------------------------------------------------------
// `.context("msg")?` in a let-binding (the realistic use site)
// ---------------------------------------------------------------------------

#[test]
fn error_context_in_let_binding_emits_map_err_then_question() {
    // `let cfg = load().context("config load")?` — the realistic ergonomic
    // form. The whole RHS is a `Try { MethodCall { context } }` node.
    let load_call = Expr::FuncCall {
        callee: Box::new(ident_expr("load")),
        args: Vec::new(),
        span: span(),
    };
    let rhs = try_expr(context_call(load_call, "config load"));
    let src = codegen_stmts(vec![Stmt::LetDecl {
        name: ident("cfg"),
        value: rhs,
        mutable: false,
        ty: None,
        span: span(),
    }]);
    assert!(
        src.contains("let cfg = "),
        "expected `let cfg = ...` binding in: {src}"
    );
    assert!(
        src.contains("map_err"),
        "expected `map_err` in let-binding context chain: {src}"
    );
    assert!(
        src.contains("config load"),
        "expected preserved context message in: {src}"
    );
    assert!(
        src.contains("?;"),
        "expected trailing `?;` on the let-binding RHS: {src}"
    );
    must_reparse(&src);
}

// ---------------------------------------------------------------------------
// No regression on existing method-call codegen
// ---------------------------------------------------------------------------

#[test]
fn error_context_does_not_break_plain_method_calls() {
    // A plain method call (e.g. `s.len()`) must still lower via the default
    // arm now that `context` is special-cased. This confirms the new arm
    // is gated on `method.name == "context"` ONLY.
    let recv = ident_expr("s");
    let expr = Expr::MethodCall {
        receiver: Box::new(recv),
        method: ident("len"),
        args: Vec::new(),
        span: span(),
    };
    let src = codegen_one_expr(expr);
    assert!(
        src.contains("s.len()"),
        "plain method call `s.len()` must still lower correctly: {src}"
    );
    assert!(
        !src.contains("map_err"),
        "plain method call must NOT be rewritten as map_err: {src}"
    );
    must_reparse(&src);
}

#[test]
fn error_context_does_not_break_struct_field_access() {
    // The T26 field-access-vs-method-call heuristic must still produce a
    // field access for `p.name` (zero-arg, not in KNOWN_ZERO_ARG_METHODS).
    // Adding the `context` arm must not change this.
    let recv = ident_expr("p");
    let expr = Expr::MethodCall {
        receiver: Box::new(recv),
        method: ident("name"),
        args: Vec::new(),
        span: span(),
    };
    let src = codegen_one_expr(expr);
    assert!(
        src.contains("p.name"),
        "field access `p.name` must still lower correctly: {src}"
    );
    assert!(
        !src.contains("map_err"),
        "field access must NOT be rewritten as map_err: {src}"
    );
    must_reparse(&src);
}

// ---------------------------------------------------------------------------
// Error case: `.context()` with non-string-literal argument
// ---------------------------------------------------------------------------

#[test]
fn error_context_with_non_string_arg_returns_unsupported_error() {
    // `.context(42)` — the argument MUST be a string literal. Any other
    // shape is a type error in Buff, but since codegen doesn't do type
    // checking, we surface a clear `unsupported` error here. This guards
    // against silent mis-compilation (a non-string context message would
    // produce nonsensical Rust).
    let recv = ident_expr("r");
    let bad_arg = Expr::Literal(Literal::Int(42), span());
    let expr = Expr::MethodCall {
        receiver: Box::new(recv),
        method: ident("context"),
        args: vec![bad_arg],
        span: span(),
    };
    let result = generate_rust(&[func_with_stmts("f", vec![Stmt::ExprStmt(expr, span())])]);
    assert!(
        result.is_err(),
        "expected codegen to reject non-string `.context()` argument"
    );
    let err = result.unwrap_err().to_string();
    assert!(
        err.contains("context") && err.contains("string"),
        "expected a helpful error mentioning `context` + `string`, got: {err}"
    );
}

#[test]
fn error_context_with_wrong_arity_returns_unsupported_error() {
    // `.context("a", "b")` — exactly 1 arg is required.
    let recv = ident_expr("r");
    let expr = Expr::MethodCall {
        receiver: Box::new(recv),
        method: ident("context"),
        args: vec![string_expr("a"), string_expr("b")],
        span: span(),
    };
    let result = generate_rust(&[func_with_stmts("f", vec![Stmt::ExprStmt(expr, span())])]);
    assert!(
        result.is_err(),
        "expected codegen to reject multi-arg `.context()`"
    );
}

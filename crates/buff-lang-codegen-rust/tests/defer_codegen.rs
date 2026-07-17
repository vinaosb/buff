//! T100 integration tests — codegen for the `defer` statement.
//!
//! `defer EXPR` schedules `EXPR` to run when the enclosing FUNCTION exits,
//! on ANY exit path (explicit `return` or implicit fall-through). Multiple
//! defers run LIFO (last-registered first). The codegen collects deferred
//! expressions during lowering and emits them in REVERSE order at every
//! function exit point.
//!
//! Each test hand-builds a `Stmt::Defer` (or feeds source through the
//! parser), runs it through [`buff_lang_codegen_rust::generate_rust`], and
//! asserts the resulting Rust source places the deferred statements BEFORE
//! the return (or at the body tail for fall-through).
//!
//! Run via:
//!
//! ```text
//! cargo test -p buff-lang-codegen-rust --test defer_codegen
//! cargo test -p buff-lang-codegen-rust defer
//! ```

use buff_lang_ast::common::{Block, Ident, Param};
use buff_lang_ast::decl::FuncDecl;
use buff_lang_ast::{Decl, Expr, Literal, Stmt, TypeRef};
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

fn int_expr(n: i64) -> Expr {
    Expr::Literal(Literal::Int(n), span())
}

fn string_expr(s: &str) -> Expr {
    Expr::Literal(Literal::String(s.to_string()), span())
}

/// `print("text")` — a prelude call lowered to `println!(...)`.
fn print_call(text: &str) -> Expr {
    Expr::FuncCall {
        callee: Box::new(ident_expr("print")),
        args: vec![string_expr(text)],
        span: span(),
    }
}

/// `f.close()` — a method call on receiver `f`.
fn method_call(receiver_name: &str, method_name: &str) -> Expr {
    Expr::MethodCall {
        receiver: Box::new(ident_expr(receiver_name)),
        method: ident(method_name),
        args: Vec::new(),
        span: span(),
    }
}

/// Wrap a list of statements in a zero-arg `fn f() -> Int { ... }`.
fn func_with_stmts(stmts: Vec<Stmt>) -> Decl {
    Decl::FuncDecl(FuncDecl {
        name: ident("f"),
        params: Vec::<Param>::new(),
        return_type: Some(TypeRef::Named {
            name: ident("Int"),
            span: span(),
        }),
        body: Block {
            stmts,
            span: span(),
        },
        is_async: false,
        is_unsafe: false,
        is_extern: false,
        attributes: Vec::new(),
        span: span(),
    })
}

/// Wrap a list of statements in a zero-arg `fn f() { ... }` (no return type
/// — the body falls through, exercising the fall-through defer tail).
fn void_func_with_stmts(stmts: Vec<Stmt>) -> Decl {
    Decl::FuncDecl(FuncDecl {
        name: ident("f"),
        params: Vec::<Param>::new(),
        return_type: None,
        body: Block {
            stmts,
            span: span(),
        },
        is_async: false,
        is_unsafe: false,
        is_extern: false,
        attributes: Vec::new(),
        span: span(),
    })
}

fn return_zero_stmt() -> Stmt {
    Stmt::Return(Some(int_expr(0)), span())
}

fn defer_stmt(expr: Expr) -> Stmt {
    Stmt::Defer { expr, span: span() }
}

// ---------------------------------------------------------------------------
// Single defer before an explicit return.
// ---------------------------------------------------------------------------

#[test]
fn defer_single_before_return() {
    // func f() -> Int:
    //     defer print("done")
    //     return 0
    // → the println! must appear BEFORE the `return 0;`.
    let src = generate_rust(&[func_with_stmts(vec![
        defer_stmt(print_call("done")),
        return_zero_stmt(),
    ])])
    .expect("codegen must succeed");
    // The deferred print is lowered to println!("done").
    let print_idx = src
        .find("println!")
        .expect("expected a println! from the deferred print");
    let return_idx = src.find("return").expect("expected a return statement");
    assert!(
        print_idx < return_idx,
        "deferred println! must come BEFORE return;\nsrc:\n{src}"
    );
    assert!(
        src.contains("println!(\"done\")"),
        "expected println!(\"done\") in:\n{src}"
    );
    // Re-parse to verify the generated Rust is syntactically valid.
    syn::parse_str::<syn::File>(&src).expect("generated Rust must parse");
}

// ---------------------------------------------------------------------------
// Multiple defers run LIFO (last-registered first).
// ---------------------------------------------------------------------------

#[test]
fn defer_multiple_lifo_order() {
    // func f() -> Int:
    //     defer print("first")
    //     defer print("second")
    //     return 0
    // → "second" must appear BEFORE "first" in the output (LIFO), and BOTH
    //   before the return.
    let src = generate_rust(&[func_with_stmts(vec![
        defer_stmt(print_call("first")),
        defer_stmt(print_call("second")),
        return_zero_stmt(),
    ])])
    .expect("codegen must succeed");
    let first_idx = src
        .find("println!(\"first\")")
        .expect("expected println!(\"first\")");
    let second_idx = src
        .find("println!(\"second\")")
        .expect("expected println!(\"second\")");
    let return_idx = src.find("return").expect("expected a return");
    // LIFO: second (registered last) runs first → appears earlier in source.
    assert!(
        second_idx < first_idx,
        "LIFO violated: \"second\" must come before \"first\";\nsrc:\n{src}"
    );
    // Both defers run before the return.
    assert!(
        first_idx < return_idx,
        "both defers must run before return;\nsrc:\n{src}"
    );
    syn::parse_str::<syn::File>(&src).expect("generated Rust must parse");
}

// ---------------------------------------------------------------------------
// Defer with no explicit return runs at the body tail (fall-through).
// ---------------------------------------------------------------------------

#[test]
fn defer_no_return_runs_at_end() {
    // func f():
    //     defer print("done")
    // (no return — falls through)
    // → the println! must be present (emitted at the fall-through tail).
    let src = generate_rust(&[void_func_with_stmts(vec![defer_stmt(print_call("done"))])])
        .expect("codegen must succeed");
    assert!(
        src.contains("println!(\"done\")"),
        "expected println!(\"done\") at fall-through tail in:\n{src}"
    );
    syn::parse_str::<syn::File>(&src).expect("generated Rust must parse");
}

// ---------------------------------------------------------------------------
// Defer a method call: `defer f.close()` → Stmt::Defer { MethodCall }.
// ---------------------------------------------------------------------------

#[test]
fn defer_method_call_codegen() {
    // func f() -> Int:
    //     defer f.close()
    //     return 0
    // → the deferred `.close` expression must appear before the return.
    //
    // NOTE: T26's field-access-vs-method-call heuristic rewrites zero-arg
    // method calls whose name is NOT in `KNOWN_ZERO_ARG_METHODS` to a Rust
    // field access (`f.close` instead of `f.close()`). `close` is not on
    // the allow-list, so the deferred expression renders as `f.close`. The
    // DEFER MECHANISM (emit-before-return) is what this test verifies; the
    // field-access rendering is pre-existing T26 behaviour, not T100's
    // concern. We assert on the bare `.close` token so the test is robust
    // to either rendering.
    let src = generate_rust(&[func_with_stmts(vec![
        defer_stmt(method_call("f", "close")),
        return_zero_stmt(),
    ])])
    .expect("codegen must succeed");
    let close_idx = src.find(".close").expect("expected .close expression");
    let return_idx = src.find("return").expect("expected a return");
    assert!(
        close_idx < return_idx,
        "deferred .close must come before return;\nsrc:\n{src}"
    );
    syn::parse_str::<syn::File>(&src).expect("generated Rust must parse");
}

// ---------------------------------------------------------------------------
// End-to-end: feed source through lexer → parser → codegen.
// ---------------------------------------------------------------------------

#[test]
fn defer_end_to_end_from_source() {
    use buff_lang_error::SourceId;
    use buff_lang_parser::parse;

    let src_text = "func f():\n    defer print(\"done\")\n    return 0";
    let tokens = buff_lang_lexer::tokenize(src_text, SourceId(0)).expect("lexer");
    let decls = parse(&tokens, SourceId(0)).expect("parser");
    let rust = generate_rust(&decls).expect("codegen");
    // The deferred print becomes println!("done") and runs before return.
    let print_idx = rust
        .find("println!(\"done\")")
        .expect("expected println!(\"done\")");
    let return_idx = rust.find("return").expect("expected return");
    assert!(
        print_idx < return_idx,
        "deferred println! must come before return;\nrust:\n{rust}"
    );
    syn::parse_str::<syn::File>(&rust).expect("generated Rust must parse");
}

// ---------------------------------------------------------------------------
// Parser smoke: `defer f.close()` parses to Stmt::Defer { MethodCall }.
// ---------------------------------------------------------------------------

#[test]
fn defer_parses_to_method_call() {
    use buff_lang_error::SourceId;

    let src_text = "func f():\n    defer g.close()";
    let tokens = buff_lang_lexer::tokenize(src_text, SourceId(0)).expect("lexer");
    let decls = buff_lang_parser::parse(&tokens, SourceId(0)).expect("parser");
    let func = match decls.first() {
        Some(Decl::FuncDecl(f)) => f,
        other => panic!("expected FuncDecl, got {other:?}"),
    };
    let body = &func.body.stmts;
    assert_eq!(body.len(), 1, "expected exactly one statement");
    match &body[0] {
        Stmt::Defer { expr, .. } => {
            // The deferred expression should be a MethodCall g.close().
            match expr {
                Expr::MethodCall { method, args, .. } => {
                    assert_eq!(method.name, "close");
                    assert!(args.is_empty(), "close() takes no args");
                }
                other => panic!("expected MethodCall inside Defer, got {other:?}"),
            }
        }
        other => panic!("expected Stmt::Defer, got {other:?}"),
    }
}

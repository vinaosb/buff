//! T99 — Process environment access: Rust **codegen** integration tests.
//!
//! These tests verify that the prelude functions `args()`, `env("NAME")`, and
//! `exit(code)` lower to the correct Rust idioms.
//!
//! ## Coverage
//!
//! - `args()` → `std::env::args().collect::<Vec<String>>()`
//! - `env("PATH")` → `std::env::var("PATH").ok()`
//! - `exit(0)` → `std::process::exit(0)`
//!
//! ## Deferral
//!
//! The end-to-end scenario `func main(): let a = args(); print(a[0])` uses
//! Vector indexing (`a[0]`), which requires the array/index expression AST
//! node (T23 — not yet done). These tests verify the codegen SHAPE of each
//! prelude call individually; the indexing integration is deferred to T23.

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

fn int_expr(n: i64) -> Expr {
    Expr::Literal(Literal::Int(n), span())
}

fn string_expr(s: &str) -> Expr {
    Expr::Literal(Literal::String(s.to_string()), span())
}

fn ident_expr(s: &str) -> Expr {
    Expr::Ident(ident(s), span())
}

fn call_expr(name: &str, args: Vec<Expr>) -> Expr {
    Expr::FuncCall {
        callee: Box::new(ident_expr(name)),
        args,
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
    Decl::FuncDecl(FuncDecl {
        name: ident(name),
        params: Vec::new(),
        return_type: None,
        body: block(stmts),
        is_async: false,
        is_unsafe: false,
        is_extern: false,
        attributes: Vec::new(),
        type_params: Vec::new(),
        span: span(),
    })
}

// ---------------------------------------------------------------------------
// 1. args() → std::env::args().collect::<Vec<String>>()
// ---------------------------------------------------------------------------

#[test]
fn env_access_args_codegen_shape() {
    // func main() { let a = args() }
    let f = func_with_stmts(
        "main",
        vec![Stmt::LetDecl {
            name: ident("a"),
            value: call_expr("args", vec![]),
            mutable: false,
            ty: None,
            span: span(),
        }],
    );
    let src = generate_rust(&[f]).unwrap();
    assert!(
        src.contains("std::env::args()"),
        "args() should use std::env::args(), src = {src}"
    );
    assert!(
        src.contains("collect::< Vec < String > >") || src.contains("collect::<Vec<String>>"),
        "args() should collect into Vec<String>, src = {src}"
    );
    syn::parse_str::<syn::File>(&src).expect("must re-parse as Rust");
}

#[test]
fn env_access_args_no_args_required() {
    // args() with arguments should fail.
    let f = func_with_stmts(
        "main",
        vec![Stmt::ExprStmt(call_expr("args", vec![int_expr(1)]), span())],
    );
    let result = generate_rust(&[f]);
    assert!(result.is_err(), "args() with args should error");
}

// ---------------------------------------------------------------------------
// 2. env("NAME") → std::env::var("NAME").ok()
// ---------------------------------------------------------------------------

#[test]
fn env_access_env_codegen_shape() {
    // func main() { let p = env("PATH") }
    let f = func_with_stmts(
        "main",
        vec![Stmt::LetDecl {
            name: ident("p"),
            value: call_expr("env", vec![string_expr("PATH")]),
            mutable: false,
            ty: None,
            span: span(),
        }],
    );
    let src = generate_rust(&[f]).unwrap();
    assert!(
        src.contains("std::env::var"),
        "env() should use std::env::var, src = {src}"
    );
    assert!(
        src.contains(".ok()"),
        "env() should call .ok(), src = {src}"
    );
    assert!(
        src.contains(r#""PATH""#),
        "env() should pass the variable name, src = {src}"
    );
    syn::parse_str::<syn::File>(&src).expect("must re-parse as Rust");
}

#[test]
fn env_access_env_wrong_arg_count_errors() {
    // env() with no args should fail.
    let f = func_with_stmts(
        "main",
        vec![Stmt::ExprStmt(call_expr("env", vec![]), span())],
    );
    let result = generate_rust(&[f]);
    assert!(result.is_err(), "env() with no args should error");

    // env() with two args should fail.
    let f = func_with_stmts(
        "main",
        vec![Stmt::ExprStmt(
            call_expr("env", vec![string_expr("A"), string_expr("B")]),
            span(),
        )],
    );
    let result = generate_rust(&[f]);
    assert!(result.is_err(), "env() with two args should error");
}

// ---------------------------------------------------------------------------
// 3. exit(code) → std::process::exit(code)
// ---------------------------------------------------------------------------

#[test]
fn env_access_exit_codegen_shape() {
    // func main() { exit(0) }
    let f = func_with_stmts(
        "main",
        vec![Stmt::ExprStmt(call_expr("exit", vec![int_expr(0)]), span())],
    );
    let src = generate_rust(&[f]).unwrap();
    assert!(
        src.contains("std::process::exit"),
        "exit() should use std::process::exit, src = {src}"
    );
    assert!(
        src.contains("0"),
        "exit() should pass the exit code, src = {src}"
    );
    syn::parse_str::<syn::File>(&src).expect("must re-parse as Rust");
}

#[test]
fn env_access_exit_wrong_arg_count_errors() {
    // exit() with no args should fail.
    let f = func_with_stmts(
        "main",
        vec![Stmt::ExprStmt(call_expr("exit", vec![]), span())],
    );
    let result = generate_rust(&[f]);
    assert!(result.is_err(), "exit() with no args should error");

    // exit() with two args should fail.
    let f = func_with_stmts(
        "main",
        vec![Stmt::ExprStmt(
            call_expr("exit", vec![int_expr(0), int_expr(1)]),
            span(),
        )],
    );
    let result = generate_rust(&[f]);
    assert!(result.is_err(), "exit() with two args should error");
}

// ---------------------------------------------------------------------------
// 4. Combined snapshot — args + env + exit together
// ---------------------------------------------------------------------------

#[test]
fn env_access_combined_snapshot() {
    // func main() {
    //     let a = args();
    //     let p = env("PATH");
    //     exit(0);
    // }
    let f = func_with_stmts(
        "main",
        vec![
            Stmt::LetDecl {
                name: ident("a"),
                value: call_expr("args", vec![]),
                mutable: false,
                ty: None,
                span: span(),
            },
            Stmt::LetDecl {
                name: ident("p"),
                value: call_expr("env", vec![string_expr("PATH")]),
                mutable: false,
                ty: None,
                span: span(),
            },
            Stmt::ExprStmt(call_expr("exit", vec![int_expr(0)]), span()),
        ],
    );
    let src = generate_rust(&[f]).unwrap();
    insta::assert_snapshot!(src, @r###"
    fn main() {
        let a: Vec<String> = std::env::args().collect::<Vec<String>>();
        let p: Option<String> = std::env::var("PATH".to_string()).ok();
        std::process::exit(0);
        {
            if let Ok(__buff_contents) = std::fs::read_to_string(".env") {
                for __buff_line in __buff_contents.lines() {
                    let __buff_line = __buff_line.trim();
                    if __buff_line.is_empty() || __buff_line.starts_with('#') {
                        continue;
                    }
                    if let Some((__buff_key, __buff_val)) = __buff_line.split_once('=') {
                        let __buff_k = __buff_key.trim().to_string();
                        let __buff_v = __buff_val.trim().to_string();
                        if !__buff_k.is_empty() && std::env::var(&__buff_k).is_err() {
                            unsafe {
                                std::env::set_var(&__buff_k, &__buff_v);
                            }
                        }
                    }
                }
            }
        }
    }
    "###);
}

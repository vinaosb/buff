//! T124g integration tests - system prelude modules codegen.
//!
//! Verifies that the Rust codegen lowers the four T124g system modules:
//!
//! - **Args** namespace (`Args.list()`, `Args.get(i)`) - wraps Rust's
//!   `std::env::args` iterator. Uses only Rust `std` (NO extern crate).
//! - **Env** namespace (`Env.get("KEY")`, `Env.set("KEY", "v")`,
//!   `Env.has("KEY")`) - wraps Rust's `std::env::var` / `set_var`.
//!   Uses only Rust `std` (NO extern crate).
//! - **input()** free fn (`input()`, `input(prompt)`) - reads stdin
//!   line (trimmed), optionally printing a prompt first. Wraps
//!   `std::io::stdin` + `std::io::Write::flush`. Uses only Rust `std`.
//! - **sleep()** free fn (`sleep(Duration.seconds(N))`, `sleep(N)`,
//!   `sleep(expr)`) - async-transparent sleep. Lowers to
//!   `tokio::time::sleep(...).await`. Records `tokio` in extern_crates.
//!
//! Acceptance snapshots for the canonical criteria (per the task spec):
//!
//! ```text
//! Args.list()                 ->  std::env::args().collect::<Vec<String>>()
//! Args.get(1)                 ->  std::env::args().nth(1).unwrap_or_default()
//! Env.get("HOME")             ->  std::env::var("HOME").ok()
//! Env.set("KEY", "v")         ->  std::env::set_var("KEY", "v")
//! Env.has("HOME")             ->  std::env::var("HOME").is_ok()
//! input()                     ->  { let mut __buff_prelude_line = ...
//!                                    __buff_prelude_line.trim_end().to_string() }
//! input("Name: ")             ->  { print!("Name: "); flush; read; trim }
//! sleep(Duration.seconds(2))  ->  tokio::time::sleep(
//!                                    std::time::Duration::from_secs(2)
//!                                  ).await
//! sleep(2)                    ->  tokio::time::sleep(
//!                                    std::time::Duration::from_secs(2)
//!                                  ).await
//! ```
//!
//! Run via:
//!
//! ```text
//! cargo test -p buff-lang-codegen-rust --test system_codegen
//! ```
//!
//! # Why AST-constructed tests (not source-parsed)
//!
//! All four modules are prelude namespaces (or free prelude fns), so
//! source parsing requires no new keyword / AST node - the existing
//! `MethodCall` (for Args/Env) and `FuncCall` (for input/sleep) shapes
//! handle them. We construct ASTs by hand here for the same reasons
//! `regex_codegen.rs` (T124d), `toml_codegen.rs` (T124e), and
//! `utility_codegen.rs` (T124f) do: direct AST construction decouples
//! the codegen-pinning snapshots from any future parser-restructuring
//! work, and lets us test specific edge cases (e.g. wrong arity,
//! Duration.seconds AST shape detection, ident vs literal arg) without
//! writing Buff source that the parser may reject for orthogonal
//! reasons.

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

/// `<callee>(args...)` AST node (free-function call shape). Used for
/// the `input()` and `sleep()` free prelude fns.
fn free_call(callee: &str, args: Vec<Expr>) -> Expr {
    Expr::FuncCall {
        callee: Box::new(ident_expr(callee)),
        args,
        span: span(),
    }
}

/// `Duration.<unit>(N)` AST node (the canonical sleep arg shape).
fn duration_call(unit: &str, n: i64) -> Expr {
    Expr::MethodCall {
        receiver: Box::new(ident_expr("Duration")),
        method: ident(unit),
        args: vec![int_expr(n)],
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
// 1. Args module - associated functions (list / get).
// ===========================================================================

#[test]
fn args_codegen_list_collects_to_vec_string() {
    // Args.list() -> std::env::args().collect::<Vec<String>>()
    let src = codegen_one_expr_in("f", ns_assoc_call("Args", "list", vec![]));
    assert!(
        src.contains("std::env::args()"),
        "expected `std::env::args()` in: {src}"
    );
    assert!(
        src.contains(".collect::<Vec<String>>()"),
        "expected `.collect::<Vec<String>>()` turbofish in: {src}"
    );
    must_reparse(&src);
}

#[test]
fn args_codegen_get_uses_unwrap_or_default() {
    // Args.get(1) -> std::env::args().nth(1).unwrap_or_default()
    // Acceptance criterion: NEVER panics on out-of-bounds (empty String
    // fallback). Mirrors Toml.parse / Regex.compile's panic-free stance.
    let src = codegen_one_expr_in("f", ns_assoc_call("Args", "get", vec![int_expr(1)]));
    assert!(
        src.contains("std::env::args().nth(1)"),
        "expected `std::env::args().nth(1)` in: {src}"
    );
    assert!(
        src.contains(".unwrap_or_default()"),
        "expected `.unwrap_or_default()` (panic-free fallback) in: {src}"
    );
    // Must NOT use bare .unwrap() (panicking-generated-code rule).
    assert!(
        !src.contains(".unwrap()"),
        "expected NO bare `.unwrap()` in Args.get output: {src}"
    );
    must_reparse(&src);
}

#[test]
fn args_codegen_get_ident_arg() {
    // Args.get(my_index_var) - non-literal arg passes through unchanged.
    let src = codegen_one_expr_in(
        "f",
        ns_assoc_call("Args", "get", vec![ident_expr("my_index_var")]),
    );
    assert!(
        src.contains(".nth(my_index_var)"),
        "expected `.nth(my_index_var)` (ident arg passthrough) in: {src}"
    );
    must_reparse(&src);
}

#[test]
fn args_codegen_no_extern_crate_registered() {
    // Args uses only Rust `std` - NO extern crate should be registered.
    let main = func_decl(
        "main",
        &[],
        vec![expr_stmt(ns_assoc_call("Args", "list", vec![]))],
    );
    let mut codegen = RustCodegen::new();
    let _ = codegen.generate(&[main]).expect("codegen must succeed");
    let extern_crates = codegen.extern_crates();
    assert!(
        !extern_crates.contains("tokio"),
        "Args should NOT register `tokio` extern crate, got: {:?}",
        extern_crates
    );
    assert!(
        !extern_crates.contains("rand"),
        "Args should NOT register `rand` extern crate, got: {:?}",
        extern_crates
    );
}

// ===========================================================================
// 2. Env module - associated functions (get / set / has).
// ===========================================================================

#[test]
fn env_codegen_get_var_ok() {
    // Env.get("HOME") -> std::env::var("HOME".to_string()).ok()
    // The literal is lifted via `.to_string()` (Buff hides `&str` from the
    // user; codegen emits `String` and `std::env::var` borrows via `AsRef`).
    let src = codegen_one_expr_in("f", ns_assoc_call("Env", "get", vec![str_expr("HOME")]));
    assert!(
        src.contains("std::env::var(\"HOME\".to_string())"),
        "expected `std::env::var(\"HOME\".to_string())` in: {src}"
    );
    assert!(
        src.contains(".ok()"),
        "expected `.ok()` (None on unset/invalid UTF-8) in: {src}"
    );
    must_reparse(&src);
}

#[test]
fn env_codegen_get_ident_arg_borrows() {
    // Env.get(my_string_var) - non-literal arg borrows via &.
    let src = codegen_one_expr_in(
        "f",
        ns_assoc_call("Env", "get", vec![ident_expr("my_string_var")]),
    );
    assert!(
        src.contains("&my_string_var"),
        "expected `&my_string_var` (borrow coercion for String -> &str) in: {src}"
    );
    must_reparse(&src);
}

#[test]
fn env_codegen_set_var_two_args() {
    // Env.set("KEY", "v") -> std::env::set_var("KEY".to_string(), "v".to_string())
    let src = codegen_one_expr_in(
        "f",
        ns_assoc_call("Env", "set", vec![str_expr("KEY"), str_expr("v")]),
    );
    assert!(
        src.contains("std::env::set_var(\"KEY\".to_string(), \"v\".to_string())"),
        "expected `std::env::set_var(\"KEY\".to_string(), \"v\".to_string())` in: {src}"
    );
    must_reparse(&src);
}

#[test]
fn env_codegen_has_var_is_ok() {
    // Env.has("HOME") -> std::env::var("HOME".to_string()).is_ok()
    let src = codegen_one_expr_in("f", ns_assoc_call("Env", "has", vec![str_expr("HOME")]));
    assert!(
        src.contains("std::env::var(\"HOME\".to_string())"),
        "expected `std::env::var(\"HOME\".to_string())` in: {src}"
    );
    assert!(
        src.contains(".is_ok()"),
        "expected `.is_ok()` (Bool return) in: {src}"
    );
    must_reparse(&src);
}

#[test]
fn env_codegen_no_extern_crate_registered() {
    // Env uses only Rust `std` - NO extern crate should be registered.
    let main = func_decl(
        "main",
        &[],
        vec![expr_stmt(ns_assoc_call(
            "Env",
            "get",
            vec![str_expr("HOME")],
        ))],
    );
    let mut codegen = RustCodegen::new();
    let _ = codegen.generate(&[main]).expect("codegen must succeed");
    let extern_crates = codegen.extern_crates();
    assert!(
        !extern_crates.contains("tokio"),
        "Env should NOT register `tokio` extern crate, got: {:?}",
        extern_crates
    );
}

// ===========================================================================
// 3. input() free fn - stdin reader with optional prompt.
// ===========================================================================

#[test]
fn input_codegen_no_args_trims_trailing_newline() {
    // input() -> { let mut __buff_prelude_line = String::new();
    //              std::io::stdin().read_line(&mut __buff_prelude_line).ok();
    //              __buff_prelude_line.trim_end().to_string() }
    //
    // The trim_end strips the trailing newline (the difference between
    // T124g input() and T99 read_line()). The .to_string() lifts &str
    // to String (Buff hides references from the user).
    let src = codegen_one_expr_in("f", free_call("input", vec![]));
    assert!(src.contains("read_line"), "expected `read_line` in: {src}");
    assert!(
        src.contains(".trim_end()"),
        "expected `.trim_end()` (trailing-newline trim) in: {src}"
    );
    assert!(
        src.contains(".to_string()"),
        "expected `.to_string()` (lift &str -> String) in: {src}"
    );
    // Must NOT include a prompt print (no prompt arg).
    assert!(
        !src.contains("print!("),
        "expected NO `print!` macro (no prompt arg) in: {src}"
    );
    assert!(
        !src.contains("flush"),
        "expected NO `flush` (no prompt arg) in: {src}"
    );
    must_reparse(&src);
}

#[test]
fn input_codegen_prompt_arg_prints_and_flushes() {
    // input("Name: ") -> { print!("Name: "); use std::io::Write;
    //                      std::io::stdout().flush().ok();
    //                      <read + trim> }
    //
    // The flush is critical: without it, the prompt may stay buffered
    // in stdout's pipe until after the read_line returns (interactive
    // pipelines deadlock). The `use std::io::Write;` brings the flush
    // method into scope (block-local so it doesn't pollute the user's
    // module).
    let src = codegen_one_expr_in("f", free_call("input", vec![str_expr("Name: ")]));
    assert!(
        src.contains("print!(\"Name: \".to_string())"),
        "expected `print!(\"Name: \".to_string())` in: {src}"
    );
    assert!(
        src.contains("use std::io::Write"),
        "expected `use std::io::Write` (block-local trait import) in: {src}"
    );
    assert!(
        src.contains("std::io::stdout().flush()"),
        "expected `std::io::stdout().flush()` in: {src}"
    );
    assert!(
        src.contains(".trim_end()"),
        "expected `.trim_end()` (trim on the prompt form too) in: {src}"
    );
    must_reparse(&src);
}

#[test]
fn input_codegen_ident_prompt_arg() {
    // input(my_prompt_var) - non-literal prompt splices via the same
    // print! macro shape.
    let src = codegen_one_expr_in("f", free_call("input", vec![ident_expr("my_prompt_var")]));
    assert!(
        src.contains("print!(my_prompt_var)"),
        "expected `print!(my_prompt_var)` (ident prompt passthrough) in: {src}"
    );
    must_reparse(&src);
}

#[test]
fn input_codegen_rejects_two_args() {
    // input(prompt, extra) with two args - should error.
    let result = std::panic::catch_unwind(|| {
        let _ = codegen_one_expr_in(
            "f",
            free_call("input", vec![str_expr("p"), str_expr("extra")]),
        );
    });
    assert!(
        result.is_err(),
        "expected codegen to reject `input(p, extra)` (>1 arg)"
    );
}

#[test]
fn input_codegen_no_extern_crate_registered() {
    // input uses only Rust `std` - NO extern crate should be registered.
    let main = func_decl("main", &[], vec![expr_stmt(free_call("input", vec![]))]);
    let mut codegen = RustCodegen::new();
    let _ = codegen.generate(&[main]).expect("codegen must succeed");
    let extern_crates = codegen.extern_crates();
    assert!(
        !extern_crates.contains("tokio"),
        "input should NOT register `tokio` extern crate, got: {:?}",
        extern_crates
    );
}

// ===========================================================================
// 4. sleep() free fn - async-transparent tokio::time::sleep lowering.
// ===========================================================================

#[test]
fn sleep_codegen_duration_seconds_arg() {
    // sleep(Duration.seconds(2)) ->
    //   tokio::time::sleep(std::time::Duration::from_secs(2)).await
    //
    // The Duration.seconds(N) AST shape is detected and rewritten to
    // std::time::Duration::from_secs(N) so the generated Rust uses
    // std::time::Duration (the type tokio::time::sleep takes) rather
    // than chrono::TimeDelta (which T124b's Duration.seconds would
    // normally produce). Keeps the sleep path chrono-independent.
    let src = codegen_one_expr_in("f", free_call("sleep", vec![duration_call("seconds", 2)]));
    assert!(
        src.contains("tokio::time::sleep"),
        "expected `tokio::time::sleep` in: {src}"
    );
    assert!(
        src.contains("std::time::Duration::from_secs(2)"),
        "expected `std::time::Duration::from_secs(2)` (Duration.seconds rewritten) in: {src}"
    );
    assert!(
        src.contains(".await"),
        "expected `.await` (async-transparent) in: {src}"
    );
    // Must NOT splice chrono::TimeDelta (the Duration.seconds shape is
    // detected BEFORE the generic Duration lowering kicks in).
    assert!(
        !src.contains("chrono::TimeDelta"),
        "expected NO `chrono::TimeDelta` (sleep path is chrono-independent) in: {src}"
    );
    must_reparse(&src);
}

#[test]
fn sleep_codegen_duration_millis_arg() {
    // sleep(Duration.millis(500)) ->
    //   tokio::time::sleep(std::time::Duration::from_millis(500)).await
    let src = codegen_one_expr_in("f", free_call("sleep", vec![duration_call("millis", 500)]));
    assert!(
        src.contains("std::time::Duration::from_millis(500)"),
        "expected `std::time::Duration::from_millis(500)` in: {src}"
    );
    assert!(src.contains(".await"), "expected `.await` in: {src}");
    must_reparse(&src);
}

#[test]
fn sleep_codegen_plain_int_arg_treated_as_seconds() {
    // sleep(2) -> tokio::time::sleep(std::time::Duration::from_secs(2)).await
    //
    // Per the task spec: "accept Duration.seconds(n) (or a plain
    // int-seconds form if Duration type absent)". The plain-int form
    // is the fallback when the user doesn't want to spell out
    // Duration.seconds.
    let src = codegen_one_expr_in("f", free_call("sleep", vec![int_expr(2)]));
    assert!(
        src.contains("tokio::time::sleep(std::time::Duration::from_secs(2))"),
        "expected `tokio::time::sleep(std::time::Duration::from_secs(2))` in: {src}"
    );
    assert!(src.contains(".await"), "expected `.await` in: {src}");
    must_reparse(&src);
}

#[test]
fn sleep_codegen_other_expr_passthrough() {
    // sleep(my_duration_var) - passthrough: tokio::time::sleep(<expr>).await.
    // The user is responsible for the arg being a std::time::Duration.
    let src = codegen_one_expr_in("f", free_call("sleep", vec![ident_expr("my_duration_var")]));
    assert!(
        src.contains("tokio::time::sleep(my_duration_var)"),
        "expected `tokio::time::sleep(my_duration_var)` (passthrough) in: {src}"
    );
    assert!(src.contains(".await"), "expected `.await` in: {src}");
    must_reparse(&src);
}

#[test]
fn sleep_codegen_registers_tokio_extern_crate() {
    // Any sleep(...) call should register the `tokio` crate.
    let main = func_decl(
        "main",
        &[],
        vec![expr_stmt(free_call("sleep", vec![int_expr(1)]))],
    );
    let mut codegen = RustCodegen::new();
    let _ = codegen.generate(&[main]).expect("codegen must succeed");
    let extern_crates = codegen.extern_crates();
    assert!(
        extern_crates.contains("tokio"),
        "extern_crates should contain `tokio`, got: {:?}",
        extern_crates
    );
}

#[test]
fn sleep_codegen_registers_tokio_via_duration_form() {
    // A program with sleep(Duration.seconds(N)) but no other sleep call
    // should still register tokio.
    let main = func_decl(
        "main",
        &[],
        vec![expr_stmt(free_call(
            "sleep",
            vec![duration_call("seconds", 1)],
        ))],
    );
    let mut codegen = RustCodegen::new();
    let _ = codegen.generate(&[main]).expect("codegen must succeed");
    let extern_crates = codegen.extern_crates();
    assert!(
        extern_crates.contains("tokio"),
        "extern_crates should contain `tokio` (Duration.seconds walker), got: {:?}",
        extern_crates
    );
}

#[test]
fn sleep_codegen_no_tokio_extern_crate_when_unused() {
    // A program with no sleep calls should not register tokio.
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
        !extern_crates.contains("tokio"),
        "extern_crates should NOT contain `tokio` when sleep is unused, got: {:?}",
        extern_crates
    );
}

#[test]
fn sleep_codegen_rejects_zero_arity() {
    // sleep() with no args - should error.
    let result = std::panic::catch_unwind(|| {
        let _ = codegen_one_expr_in("f", free_call("sleep", vec![]));
    });
    assert!(
        result.is_err(),
        "expected codegen to reject `sleep()` (no duration arg)"
    );
}

#[test]
fn sleep_codegen_rejects_three_args() {
    // sleep(a, b, c) with three args - should error.
    let result = std::panic::catch_unwind(|| {
        let _ = codegen_one_expr_in(
            "f",
            free_call("sleep", vec![int_expr(1), int_expr(2), int_expr(3)]),
        );
    });
    assert!(
        result.is_err(),
        "expected codegen to reject `sleep(1, 2, 3)` (>1 arg)"
    );
}

// ===========================================================================
// 5. Error cases - arity mismatch surfaces a clear CodegenError.
// ===========================================================================

#[test]
fn args_codegen_rejects_list_with_args() {
    // Args.list(extra) with args - should error.
    let result = std::panic::catch_unwind(|| {
        let _ = codegen_one_expr_in("f", ns_assoc_call("Args", "list", vec![int_expr(1)]));
    });
    assert!(
        result.is_err(),
        "expected codegen to reject `Args.list(1)` (expected 0 args)"
    );
}

#[test]
fn args_codegen_rejects_get_with_wrong_arity() {
    // Args.get() with no args - should error.
    let result = std::panic::catch_unwind(|| {
        let _ = codegen_one_expr_in("f", ns_assoc_call("Args", "get", vec![]));
    });
    assert!(
        result.is_err(),
        "expected codegen to reject `Args.get()` (no index arg)"
    );
}

#[test]
fn env_codegen_rejects_get_with_wrong_arity() {
    // Env.get() with no args - should error.
    let result = std::panic::catch_unwind(|| {
        let _ = codegen_one_expr_in("f", ns_assoc_call("Env", "get", vec![]));
    });
    assert!(
        result.is_err(),
        "expected codegen to reject `Env.get()` (no key arg)"
    );
}

#[test]
fn env_codegen_rejects_set_with_wrong_arity() {
    // Env.set("KEY") with one arg - should error.
    let result = std::panic::catch_unwind(|| {
        let _ = codegen_one_expr_in("f", ns_assoc_call("Env", "set", vec![str_expr("KEY")]));
    });
    assert!(
        result.is_err(),
        "expected codegen to reject `Env.set(\"KEY\")` (expected 2 args)"
    );
}

#[test]
fn env_codegen_rejects_has_with_wrong_arity() {
    // Env.has() with no args - should error.
    let result = std::panic::catch_unwind(|| {
        let _ = codegen_one_expr_in("f", ns_assoc_call("Env", "has", vec![]));
    });
    assert!(
        result.is_err(),
        "expected codegen to reject `Env.has()` (no key arg)"
    );
}

// ===========================================================================
// 6. insta snapshots - byte-stable codegen pinning.
// ===========================================================================

#[test]
fn args_codegen_list_snapshot() {
    let src = codegen_one_expr_in("f", ns_assoc_call("Args", "list", vec![]));
    insta::assert_snapshot!(src);
}

#[test]
fn args_codegen_get_snapshot() {
    let src = codegen_one_expr_in("f", ns_assoc_call("Args", "get", vec![int_expr(1)]));
    insta::assert_snapshot!(src);
}

#[test]
fn env_codegen_get_snapshot() {
    let src = codegen_one_expr_in("f", ns_assoc_call("Env", "get", vec![str_expr("HOME")]));
    insta::assert_snapshot!(src);
}

#[test]
fn env_codegen_set_snapshot() {
    let src = codegen_one_expr_in(
        "f",
        ns_assoc_call("Env", "set", vec![str_expr("KEY"), str_expr("v")]),
    );
    insta::assert_snapshot!(src);
}

#[test]
fn env_codegen_has_snapshot() {
    let src = codegen_one_expr_in("f", ns_assoc_call("Env", "has", vec![str_expr("HOME")]));
    insta::assert_snapshot!(src);
}

#[test]
fn input_codegen_no_args_snapshot() {
    let src = codegen_one_expr_in("f", free_call("input", vec![]));
    insta::assert_snapshot!(src);
}

#[test]
fn input_codegen_prompt_snapshot() {
    let src = codegen_one_expr_in("f", free_call("input", vec![str_expr("Name: ")]));
    insta::assert_snapshot!(src);
}

#[test]
fn sleep_codegen_duration_seconds_snapshot() {
    let src = codegen_one_expr_in("f", free_call("sleep", vec![duration_call("seconds", 2)]));
    insta::assert_snapshot!(src);
}

#[test]
fn sleep_codegen_plain_int_snapshot() {
    let src = codegen_one_expr_in("f", free_call("sleep", vec![int_expr(2)]));
    insta::assert_snapshot!(src);
}

#[test]
fn sleep_codegen_duration_millis_snapshot() {
    let src = codegen_one_expr_in("f", free_call("sleep", vec![duration_call("millis", 500)]));
    insta::assert_snapshot!(src);
}

#[test]
fn system_codegen_full_program_snapshot() {
    // End-to-end snapshot: a `main` that exercises one call from each
    // of the four modules. Pins the full shape of the generated Rust
    // for a typical system-using program (the acceptance criterion
    // from the task spec). The sleep call here uses the plain-int form
    // (canonical Duration.seconds is exercised in its own snapshot).
    let main = func_decl(
        "main",
        &[],
        vec![
            let_stmt("args", ns_assoc_call("Args", "list", vec![])),
            let_stmt("first", ns_assoc_call("Args", "get", vec![int_expr(1)])),
            let_stmt("home", ns_assoc_call("Env", "get", vec![str_expr("HOME")])),
            expr_stmt(ns_assoc_call(
                "Env",
                "set",
                vec![str_expr("DEBUG"), str_expr("1")],
            )),
            let_stmt(
                "has_path",
                ns_assoc_call("Env", "has", vec![str_expr("PATH")]),
            ),
            let_stmt("name", free_call("input", vec![str_expr("Name: ")])),
            let_stmt("line", free_call("input", vec![])),
            expr_stmt(free_call("sleep", vec![int_expr(1)])),
        ],
    );
    let mut codegen = RustCodegen::new();
    let file = codegen.generate(&[main]).expect("codegen must succeed");
    let src = buff_lang_codegen_rust::format_file(&file);
    insta::assert_snapshot!(src);
}

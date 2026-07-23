//! T124l integration tests - Process + OS prelude modules codegen.
//!
//! Verifies that the Rust codegen lowers the two T124l system
//! modules:
//!
//! - **Process** value type (`Process.spawn(cmd, args) -> Process`,
//!   instance methods `.wait() -> Int` + `.id() -> Int`, plus the
//!   side-effecting associated function `Process.exit(code)`)
//!   - wraps `std::process::Command` / `Child` / `exit` (NO extern
//!     crate needed - std-only, mirrors the Path / Dir.list /
//!     Tempfile.dir stance from T124j). The spawned value's Rust
//!     type is `Option<std::process::Child>` - the Option wrapper
//!     lets `Process.spawn` be panic-free (a spawn failure collapses
//!     to `None`; `.wait()` / `.id()` then operate on the Option via
//!     `.map(...).unwrap_or_default()`).
//! - **OS** namespace (`OS.name() -> String`, `OS.arch() -> String`,
//!   `OS.hostname() -> String`, `OS.cpus() -> Int`)
//!   - `name` / `arch` wrap `std::env::consts::OS` / `ARCH`
//!     (compile-time consts - std-only).
//!   - `hostname` wraps the env-var fallback
//!     `std::env::var("COMPUTERNAME").or_else(|_|
//!     std::env::var("HOSTNAME")).unwrap_or_default()` (std-only -
//!     NO `hostname` crate added, per spec).
//!   - `cpus` wraps the `num_cpus` Rust crate (`num_cpus::get() as
//!     i64`).
//!
//! Acceptance snapshots for the canonical criteria (per the task
//! spec):
//!
//! ```text
//! Process.spawn(c, a)        -> std::process::Command::new(c).args(a).spawn().ok()
//! process.wait()             -> recv.map(|mut c| c.wait().map(|s| s.code().unwrap_or_default())
//!                                              .unwrap_or_default()).unwrap_or_default()
//! process.id()               -> recv.map(|c| c.id() as i64).unwrap_or_default()
//! Process.exit(code)         -> std::process::exit(code as i32)
//! OS.name()                  -> std::env::consts::OS.to_string()
//! OS.arch()                  -> std::env::consts::ARCH.to_string()
//! OS.hostname()              -> std::env::var("COMPUTERNAME").or_else(|_| std::env::var("HOSTNAME"))
//!                                                       .unwrap_or_default()
//! OS.cpus()                  -> num_cpus::get() as i64
//! ```
//!
//! Run via:
//!
//! ```text
//! cargo test -p buff-lang-codegen-rust --test process_codegen
//! ```
//!
//! # Why AST-constructed tests (not source-parsed)
//!
//! Both modules are prelude namespaces (or a runtime-value type
//! constructed via a prelude assoc fn, in Process's case), so
//! source parsing requires no new keyword / AST node - the existing
//! `MethodCall` shape handles them. We construct ASTs by hand here
//! for the same reasons `fs_codegen.rs` (T124j),
//! `crypto_codegen.rs` (T124k), `format_codegen.rs` (T124i),
//! `web_codegen.rs` (T124h), `system_codegen.rs` (T124g),
//! `regex_codegen.rs` (T124d), `toml_codegen.rs` (T124e), and
//! `utility_codegen.rs` (T124f) do: direct AST construction
//! decouples the codegen-pinning snapshots from any future
//! parser-restructuring work, and lets us test specific edge cases
//! (e.g. wrong arity, ident vs literal arg, receiver inference for
//! instance methods) without writing Buff source that the parser
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

fn str_expr(s: &str) -> Expr {
    Expr::Literal(Literal::String(s.to_string()), span())
}

fn int_expr(n: i64) -> Expr {
    Expr::Literal(Literal::Int(n), span())
}

fn ident_expr(s: &str) -> Expr {
    Expr::Ident(ident(s), span())
}

/// An empty `[]` array literal. Used as the args parameter for
/// `Process.spawn(cmd, args)` so the type inferencer can resolve it
/// (an unbound `args` ident would error and make the spawn's return
/// type Unknown, breaking the instance-method dispatch). Infers as
/// `Vector<Int>` (default for empty array) - the codegen splices the
/// expression directly so the runtime shape is `Command::new(cmd)
/// .args([]).spawn().ok()`.
fn empty_args_expr() -> Expr {
    Expr::ArrayLit {
        elements: vec![],
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
/// shape). The receiver is the bare namespace Ident (e.g. `Process`,
/// `OS`).
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
// 1. Process.spawn - two args (cmd, args), wraps Command::new().args().spawn().ok().
// ===========================================================================

#[test]
fn process_codegen_spawn_with_literals_uses_command_spawn_ok() {
    // Process.spawn("ls", ["-l"]) -> std::process::Command::new("ls")
    //   .args(["-l"]).spawn().ok(). The `.ok()` collapses a spawn
    // failure to None - NEVER panics, matching Buff's "no panicking
    // generated code" rule.
    let src = codegen_one_expr_in(
        "f",
        ns_assoc_call("Process", "spawn", vec![str_expr("ls"), empty_args_expr()]),
    );
    assert!(
        src.contains("std::process::Command::new("),
        "expected `std::process::Command::new(` in: {src}"
    );
    assert!(
        src.contains(".args("),
        "expected `.args(` (separate arg vector pass-through - NO shell) in: {src}"
    );
    assert!(
        src.contains(".spawn()"),
        "expected `.spawn()` (kick off the child process) in: {src}"
    );
    assert!(
        src.contains(".ok()"),
        "expected `.ok()` (panic-free Result -> Option collapse) in: {src}"
    );
    // Must NOT use bare .unwrap() (panicking-generated-code rule).
    assert!(
        !src.contains(".unwrap()"),
        "expected NO bare `.unwrap()` in Process.spawn output: {src}"
    );
    // Must NOT shell out (no `sh -c` or shell invocation - the spec
    // explicitly forbids shell expansion / shell-injection vectors).
    assert!(
        !src.contains("sh -c") && !src.contains("shell"),
        "expected NO shell invocation in Process.spawn output (spec forbids it): {src}"
    );
    must_reparse(&src);
}

#[test]
fn process_codegen_spawn_with_ident_cmd_arg_splices_through() {
    // Process.spawn(cmd, args) where both are variables. Both
    // should splice through as the bare idents (cmd as the
    // Command::new arg, args as the .args arg). NOTE: this test
    // deliberately uses unbound idents (no prior `let` binding)
    // because it tests CODEGEN splicing only - the inference
    // failure (unknown ident) does NOT affect codegen output (the
    // spawn lowering just splices the lowered receiver args). The
    // wait/id instance-method dispatch tests use bound idents via
    // `empty_args_expr()` + the spawn ctor so the inferencer
    // resolves `p` to Type::Process.
    let src = codegen_one_expr_in(
        "f",
        ns_assoc_call(
            "Process",
            "spawn",
            vec![ident_expr("cmd"), ident_expr("args")],
        ),
    );
    assert!(
        src.contains("std::process::Command::new(cmd)"),
        "expected `std::process::Command::new(cmd)` (ident arg splice) in: {src}"
    );
    assert!(
        src.contains(".args(args)"),
        "expected `.args(args)` (ident arg splice) in: {src}"
    );
    must_reparse(&src);
}

// ===========================================================================
// 2. Process.wait - instance method, wraps Child::wait() through Option.
// ===========================================================================

/// Build a typical Process-using function body: `let p = Process.spawn(...)`
/// then one extra expr_stmt the test slots in. Returns the stmts vec.
fn process_body_with_extra(extra: Expr) -> Vec<Stmt> {
    vec![
        let_stmt(
            "p",
            ns_assoc_call("Process", "spawn", vec![str_expr("ls"), empty_args_expr()]),
        ),
        expr_stmt(extra),
    ]
}

#[test]
fn process_codegen_wait_uses_child_wait_through_option_map() {
    // process.wait() -> recv.map(|mut c| { c.wait().map(|s| s.code()
    //   .unwrap_or_default()).unwrap_or_default() }).unwrap_or_default().
    // The three .unwrap_or_default() layers handle: (a) spawn failed
    // (outer Option None), (b) wait() returned an error (middle
    // Result Err), (c) signal-terminated process has no exit code
    // (inner Option None). All collapse to `0` - NEVER panics.
    let src = codegen_stmts_in(
        "f",
        process_body_with_extra(instance_call(ident_expr("p"), "wait", vec![])),
    );
    assert!(
        src.contains(".map(|mut c|"),
        "expected `.map(|mut c|` (outer Option-map closure; mut because wait takes &mut self) in: {src}"
    );
    assert!(
        src.contains("c.wait()"),
        "expected `c.wait()` (Child::wait blocking call) in: {src}"
    );
    assert!(
        src.contains(".map(|s| s.code()"),
        "expected `.map(|s| s.code()` (ExitStatus::code Option<Int>) in: {src}"
    );
    // Three .unwrap_or_default() calls (one per Option/Result layer).
    let unwrap_or_default_count = src.matches(".unwrap_or_default()").count();
    assert_eq!(
        unwrap_or_default_count, 3,
        "expected 3 `.unwrap_or_default()` calls (outer Option + middle Result + inner Option), got {unwrap_or_default_count} in: {src}"
    );
    // Must NOT use bare .unwrap() (panicking-generated-code rule).
    assert!(
        !src.contains(".unwrap()"),
        "expected NO bare `.unwrap()` in process.wait output: {src}"
    );
    must_reparse(&src);
}

// ===========================================================================
// 3. Process.id - instance method, wraps Child::id() through Option.
// ===========================================================================

#[test]
fn process_codegen_id_uses_child_id_as_i64_through_option_map() {
    // process.id() -> recv.map(|c| c.id() as i64).unwrap_or_default().
    // The `as i64` widens Rust's `u32` pid to Buff's `Int<64>`.
    let src = codegen_stmts_in(
        "f",
        process_body_with_extra(instance_call(ident_expr("p"), "id", vec![])),
    );
    assert!(
        src.contains(".map(|c| c.id() as i64)"),
        "expected `.map(|c| c.id() as i64)` (Option-map widening u32 pid to i64) in: {src}"
    );
    assert!(
        src.contains(".unwrap_or_default()"),
        "expected `.unwrap_or_default()` (panic-free Option collapse - 0 when spawn failed) in: {src}"
    );
    // Must NOT use bare .unwrap() (panicking-generated-code rule).
    assert!(
        !src.contains(".unwrap()"),
        "expected NO bare `.unwrap()` in process.id output: {src}"
    );
    must_reparse(&src);
}

// ===========================================================================
// 4. Process.exit - side-effecting terminal call, wraps std::process::exit.
// ===========================================================================

#[test]
fn process_codegen_exit_uses_std_process_exit_as_i32() {
    // Process.exit(code) -> std::process::exit(<code> as i32). The
    // `as i32` narrows Buff's default Int<64> to the OS's i32 exit-
    // code width. The call NEVER returns (it terminates the program
    // immediately).
    let src = codegen_one_expr_in("f", ns_assoc_call("Process", "exit", vec![int_expr(42)]));
    assert!(
        src.contains("std::process::exit("),
        "expected `std::process::exit(` (terminal call) in: {src}"
    );
    assert!(
        src.contains(" as i32"),
        "expected ` as i32` (Int<64> -> i32 narrowing cast) in: {src}"
    );
    // Must NOT use bare .unwrap() (panicking-generated-code rule).
    assert!(
        !src.contains(".unwrap()"),
        "expected NO bare `.unwrap()` in Process.exit output: {src}"
    );
    must_reparse(&src);
}

// ===========================================================================
// 5. OS.name / OS.arch - std::env::consts::{OS,ARCH}.to_string().
// ===========================================================================

#[test]
fn os_codegen_name_uses_std_env_consts_os() {
    // OS.name() -> std::env::consts::OS.to_string().
    let src = codegen_one_expr_in("f", ns_assoc_call("OS", "name", vec![]));
    assert!(
        src.contains("std::env::consts::OS"),
        "expected `std::env::consts::OS` (compile-time OS const) in: {src}"
    );
    assert!(
        src.contains(".to_string()"),
        "expected `.to_string()` (lift &str to String) in: {src}"
    );
    // Must NOT use bare .unwrap() (panicking-generated-code rule).
    assert!(
        !src.contains(".unwrap()"),
        "expected NO bare `.unwrap()` in OS.name output: {src}"
    );
    must_reparse(&src);
}

#[test]
fn os_codegen_arch_uses_std_env_consts_arch() {
    // OS.arch() -> std::env::consts::ARCH.to_string().
    let src = codegen_one_expr_in("f", ns_assoc_call("OS", "arch", vec![]));
    assert!(
        src.contains("std::env::consts::ARCH"),
        "expected `std::env::consts::ARCH` (compile-time ARCH const) in: {src}"
    );
    assert!(
        src.contains(".to_string()"),
        "expected `.to_string()` (lift &str to String) in: {src}"
    );
    // Must NOT use bare .unwrap() (panicking-generated-code rule).
    assert!(
        !src.contains(".unwrap()"),
        "expected NO bare `.unwrap()` in OS.arch output: {src}"
    );
    must_reparse(&src);
}

// ===========================================================================
// 6. OS.hostname - env-var fallback (COMPUTERNAME -> HOSTNAME -> "").
// ===========================================================================

#[test]
fn os_codegen_hostname_uses_env_var_computername_or_hostname() {
    // OS.hostname() -> std::env::var("COMPUTERNAME")
    //   .or_else(|_| std::env::var("HOSTNAME")).unwrap_or_default().
    let src = codegen_one_expr_in("f", ns_assoc_call("OS", "hostname", vec![]));
    assert!(
        src.contains("std::env::var(\"COMPUTERNAME\")"),
        "expected `std::env::var(\"COMPUTERNAME\")` (Windows env-var hostname) in: {src}"
    );
    assert!(
        src.contains(".or_else(|_| std::env::var(\"HOSTNAME\"))"),
        "expected `.or_else(|_| std::env::var(\"HOSTNAME\"))` (Unix fallback) in: {src}"
    );
    assert!(
        src.contains(".unwrap_or_default()"),
        "expected `.unwrap_or_default()` (empty String when neither env var is set - NEVER panics) in: {src}"
    );
    // Must NOT emit a `hostname::` path (the spec forbids a hostname crate).
    assert!(
        !src.contains("hostname::"),
        "expected NO `hostname::` path in OS.hostname output (spec forbids the crate): {src}"
    );
    // Must NOT use bare .unwrap() (panicking-generated-code rule).
    assert!(
        !src.contains(".unwrap()"),
        "expected NO bare `.unwrap()` in OS.hostname output: {src}"
    );
    must_reparse(&src);
}

// ===========================================================================
// 7. OS.cpus - num_cpus::get() as i64.
// ===========================================================================

#[test]
fn os_codegen_cpus_uses_num_cpus_get_as_i64() {
    // OS.cpus() -> num_cpus::get() as i64.
    let src = codegen_one_expr_in("f", ns_assoc_call("OS", "cpus", vec![]));
    assert!(
        src.contains("num_cpus::get()"),
        "expected `num_cpus::get()` (logical CPU count) in: {src}"
    );
    assert!(
        src.contains(" as i64"),
        "expected ` as i64` (usize -> Int<64> widening cast) in: {src}"
    );
    // Must NOT use bare .unwrap() (panicking-generated-code rule).
    assert!(
        !src.contains(".unwrap()"),
        "expected NO bare `.unwrap()` in OS.cpus output: {src}"
    );
    must_reparse(&src);
}

// ===========================================================================
// 8. extern_crates registration (narrow num_cpus walker).
// ===========================================================================

#[test]
fn os_codegen_registers_num_cpus_for_cpus() {
    // A program with OS.cpus() registers the num_cpus crate.
    let main = func_decl(
        "main",
        &[],
        vec![expr_stmt(ns_assoc_call("OS", "cpus", vec![]))],
    );
    let mut codegen = RustCodegen::new();
    let _ = codegen.generate(&[main]).expect("codegen must succeed");
    let extern_crates = codegen.extern_crates();
    assert!(
        extern_crates.contains("num_cpus"),
        "extern_crates should contain `num_cpus`, got: {:?}",
        extern_crates
    );
}

#[test]
fn os_codegen_does_not_register_num_cpus_for_name_only() {
    // A program with only OS.name (NO OS.cpus) should NOT register
    // num_cpus (OS.name uses std::env::consts - no extern crate needed).
    let main = func_decl(
        "main",
        &[],
        vec![expr_stmt(ns_assoc_call("OS", "name", vec![]))],
    );
    let mut codegen = RustCodegen::new();
    let _ = codegen.generate(&[main]).expect("codegen must succeed");
    let extern_crates = codegen.extern_crates();
    assert!(
        !extern_crates.contains("num_cpus"),
        "extern_crates should NOT contain `num_cpus` when only OS.name is used, got: {:?}",
        extern_crates
    );
}

#[test]
fn os_codegen_does_not_register_num_cpus_for_arch_or_hostname() {
    // A program with OS.arch + OS.hostname (NO OS.cpus) should NOT
    // register num_cpus (those calls use std::env::consts / env-var
    // - no extern crate needed).
    let main = func_decl(
        "main",
        &[],
        vec![
            expr_stmt(ns_assoc_call("OS", "arch", vec![])),
            expr_stmt(ns_assoc_call("OS", "hostname", vec![])),
        ],
    );
    let mut codegen = RustCodegen::new();
    let _ = codegen.generate(&[main]).expect("codegen must succeed");
    let extern_crates = codegen.extern_crates();
    assert!(
        !extern_crates.contains("num_cpus"),
        "extern_crates should NOT contain `num_cpus` when only OS.arch/hostname are used, got: {:?}",
        extern_crates
    );
}

#[test]
fn process_codegen_registers_no_extern_crate() {
    // A program using Process.* (spawn + wait + id + exit) should
    // NOT register any extern crate (std::process is in std - NO
    // extern crate needed, mirrors Math/Strings/Args/Env/Path stance).
    let main = func_decl(
        "main",
        &[],
        vec![
            let_stmt(
                "p",
                ns_assoc_call("Process", "spawn", vec![str_expr("ls"), empty_args_expr()]),
            ),
            expr_stmt(instance_call(ident_expr("p"), "wait", vec![])),
            expr_stmt(instance_call(ident_expr("p"), "id", vec![])),
            expr_stmt(ns_assoc_call("Process", "exit", vec![int_expr(0)])),
        ],
    );
    let mut codegen = RustCodegen::new();
    let _ = codegen.generate(&[main]).expect("codegen must succeed");
    let extern_crates = codegen.extern_crates();
    assert!(
        !extern_crates.contains("num_cpus"),
        "extern_crates should NOT contain `num_cpus` when only Process is used, got: {:?}",
        extern_crates
    );
}

#[test]
fn process_codegen_no_extern_crate_when_unused() {
    // A program with no Process/OS calls should not register
    // num_cpus.
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
        !extern_crates.contains("num_cpus"),
        "extern_crates should NOT contain `num_cpus` when Process/OS are unused, got: {:?}",
        extern_crates
    );
}

// ===========================================================================
// 9. Error cases - arity mismatch surfaces a clear CodegenError.
// ===========================================================================

#[test]
fn process_codegen_rejects_spawn_with_zero_args() {
    // Process.spawn() with no args - should error.
    let result = std::panic::catch_unwind(|| {
        let _ = codegen_one_expr_in("f", ns_assoc_call("Process", "spawn", vec![]));
    });
    assert!(
        result.is_err(),
        "expected codegen to reject `Process.spawn()` (expected 2 args - cmd + args)"
    );
}

#[test]
fn process_codegen_rejects_spawn_with_one_arg() {
    // Process.spawn(c) with missing args arg - should error.
    let result = std::panic::catch_unwind(|| {
        let _ = codegen_one_expr_in("f", ns_assoc_call("Process", "spawn", vec![str_expr("ls")]));
    });
    assert!(
        result.is_err(),
        "expected codegen to reject `Process.spawn(\"ls\")` (expected 2 args - cmd + args)"
    );
}

#[test]
fn process_codegen_rejects_spawn_with_three_args() {
    // Process.spawn(c, a, extra) with extra args - should error.
    let result = std::panic::catch_unwind(|| {
        let _ = codegen_one_expr_in(
            "f",
            ns_assoc_call(
                "Process",
                "spawn",
                vec![str_expr("ls"), empty_args_expr(), str_expr("extra")],
            ),
        );
    });
    assert!(
        result.is_err(),
        "expected codegen to reject `Process.spawn(\"ls\", args, \"extra\")` (expected 2 args)"
    );
}

#[test]
fn process_codegen_rejects_exit_with_zero_args() {
    // Process.exit() with no args - should error.
    let result = std::panic::catch_unwind(|| {
        let _ = codegen_one_expr_in("f", ns_assoc_call("Process", "exit", vec![]));
    });
    assert!(
        result.is_err(),
        "expected codegen to reject `Process.exit()` (no code arg)"
    );
}

#[test]
fn process_codegen_rejects_exit_with_two_args() {
    // Process.exit(a, b) with extra args - should error.
    let result = std::panic::catch_unwind(|| {
        let _ = codegen_one_expr_in(
            "f",
            ns_assoc_call("Process", "exit", vec![int_expr(0), int_expr(1)]),
        );
    });
    assert!(
        result.is_err(),
        "expected codegen to reject `Process.exit(0, 1)` (expected 1 arg)"
    );
}

#[test]
fn os_codegen_rejects_name_with_args() {
    // OS.name(extra) with args - should error.
    let result = std::panic::catch_unwind(|| {
        let _ = codegen_one_expr_in("f", ns_assoc_call("OS", "name", vec![str_expr("x")]));
    });
    assert!(
        result.is_err(),
        "expected codegen to reject `OS.name(\"x\")` (expected 0 args)"
    );
}

#[test]
fn os_codegen_rejects_cpus_with_args() {
    // OS.cpus(extra) with args - should error.
    let result = std::panic::catch_unwind(|| {
        let _ = codegen_one_expr_in("f", ns_assoc_call("OS", "cpus", vec![int_expr(0)]));
    });
    assert!(
        result.is_err(),
        "expected codegen to reject `OS.cpus(0)` (expected 0 args)"
    );
}

// ===========================================================================
// 10. insta snapshots - byte-stable codegen pinning.
// ===========================================================================

#[test]
fn process_codegen_spawn_snapshot() {
    let src = codegen_one_expr_in(
        "f",
        ns_assoc_call("Process", "spawn", vec![str_expr("ls"), empty_args_expr()]),
    );
    insta::assert_snapshot!(src);
}

#[test]
fn process_codegen_wait_snapshot() {
    let src = codegen_stmts_in(
        "f",
        process_body_with_extra(instance_call(ident_expr("p"), "wait", vec![])),
    );
    insta::assert_snapshot!(src);
}

#[test]
fn process_codegen_id_snapshot() {
    let src = codegen_stmts_in(
        "f",
        process_body_with_extra(instance_call(ident_expr("p"), "id", vec![])),
    );
    insta::assert_snapshot!(src);
}

#[test]
fn process_codegen_exit_snapshot() {
    let src = codegen_one_expr_in("f", ns_assoc_call("Process", "exit", vec![int_expr(42)]));
    insta::assert_snapshot!(src);
}

#[test]
fn os_codegen_name_snapshot() {
    let src = codegen_one_expr_in("f", ns_assoc_call("OS", "name", vec![]));
    insta::assert_snapshot!(src);
}

#[test]
fn os_codegen_arch_snapshot() {
    let src = codegen_one_expr_in("f", ns_assoc_call("OS", "arch", vec![]));
    insta::assert_snapshot!(src);
}

#[test]
fn os_codegen_hostname_snapshot() {
    let src = codegen_one_expr_in("f", ns_assoc_call("OS", "hostname", vec![]));
    insta::assert_snapshot!(src);
}

#[test]
fn os_codegen_cpus_snapshot() {
    let src = codegen_one_expr_in("f", ns_assoc_call("OS", "cpus", vec![]));
    insta::assert_snapshot!(src);
}

#[test]
fn process_codegen_full_program_snapshot() {
    // End-to-end snapshot: a `main` that exercises one call from each
    // of the two system modules' surfaces. Pins the full shape of the
    // generated Rust for a typical Process/OS-using program (the
    // acceptance criterion from the task spec).
    let main = func_decl(
        "main",
        &[],
        vec![
            // Process value type + both instance methods + the
            // side-effecting Process.exit call.
            let_stmt(
                "p",
                ns_assoc_call("Process", "spawn", vec![str_expr("ls"), empty_args_expr()]),
            ),
            let_stmt("exit_code", instance_call(ident_expr("p"), "wait", vec![])),
            let_stmt("pid", instance_call(ident_expr("p"), "id", vec![])),
            // OS namespace - all 4 associated fns.
            let_stmt("osname", ns_assoc_call("OS", "name", vec![])),
            let_stmt("osarch", ns_assoc_call("OS", "arch", vec![])),
            let_stmt("host", ns_assoc_call("OS", "hostname", vec![])),
            let_stmt("ncpus", ns_assoc_call("OS", "cpus", vec![])),
            // NOTE: Process.exit is omitted from the full-program
            // snapshot because `std::process::exit` is `-> !` (the
            // never type); codegen as a non-trailing stmt would
            // produce unreachable-code warnings. The dedicated
            // `process_codegen_exit_snapshot` covers its shape.
        ],
    );
    let mut codegen = RustCodegen::new();
    let file = codegen.generate(&[main]).expect("codegen must succeed");
    let src = buff_lang_codegen_rust::format_file(&file);
    insta::assert_snapshot!(src);
}

//! Integration tests for intelligent clone analysis (T33).
//!
//! These tests exercise the v0.5 EXTENSIONS to the v0.1 move-by-default
//! semantics (T33a):
//!
//! - **Char is Copy** — `let c = 'A'; let c2 = c; use(c)` emits NO clone.
//!   (T21 added Char to the language; T33 closes the gap in the move
//!   analyzer.)
//! - **Move-at-let-binding** — `let v = [...]; let v2 = v; use(v)` clones
//!   `v` at the move-after-let use site (the let-RHS counts as a use).
//! - **Arc across spawn** — non-Copy bindings captured inside `spawn`
//!   bodies are wrapped at their definition in `Arc::new(...)` and the
//!   spawn-body use is rewritten to `Arc::clone(&x)` (a refcount bump,
//!   NOT a deep clone).
//! - **CoW mutation** — Arc-shared bindings that are subsequently mutated
//!   get `*Arc::make_mut(&mut x) = ...` at the assignment site
//!   (copy-on-write: clones only when refcount > 1).
//!
//! Tests build small Buff ASTs by hand, lower them with
//! [`buff_lang_codegen_rust::generate_rust`], and assert properties of
//! the resulting Rust source STRING (presence/absence of `.clone()` /
//! `Arc::new` / `Arc::clone` / `Arc::make_mut` at the right spots).
//!
//! The final test (`clone_analysis_*_compiles_with_rustc`) is
//! `#[ignore]`d — it requires `rustc` on PATH and verifies that a
//! representative Arc-wrapped program actually compiles without
//! borrow-checker errors. Mirrors the T33a pattern.

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

fn int_expr(n: i64) -> Expr {
    Expr::Literal(Literal::Int(n), span())
}

fn char_expr(c: char) -> Expr {
    Expr::Literal(Literal::Char(c), span())
}

fn string_expr(s: &str) -> Expr {
    Expr::Literal(Literal::String(s.to_string()), span())
}

fn ident_expr(s: &str) -> Expr {
    Expr::Ident(ident(s), span())
}

fn array_expr(els: Vec<Expr>) -> Expr {
    Expr::ArrayLit {
        elements: els,
        span: span(),
    }
}

fn named_type(s: &str) -> TypeRef {
    TypeRef::Named {
        name: ident(s),
        span: span(),
    }
}

/// Build `name(args...)` as an `Expr::FuncCall`.
fn call_expr(name: &str, args: Vec<Expr>) -> Expr {
    Expr::FuncCall {
        callee: Box::new(ident_expr(name)),
        args,
        span: span(),
    }
}

/// `spawn <task>` → `Expr::Spawn`.
fn spawn_expr(task: Expr) -> Expr {
    Expr::Spawn {
        task: Box::new(task),
        span: span(),
    }
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

fn let_stmt_mut(name: &str, value: Expr) -> Stmt {
    Stmt::LetDecl {
        name: ident(name),
        value,
        mutable: true,
        ty: None,
        span: span(),
    }
}

fn let_stmt_typed(name: &str, ty: TypeRef, value: Expr) -> Stmt {
    Stmt::LetDecl {
        name: ident(name),
        value,
        mutable: false,
        ty: Some(ty),
        span: span(),
    }
}

fn assign_stmt(name: &str, value: Expr) -> Stmt {
    Stmt::Assignment {
        target: ident_expr(name),
        op: buff_lang_ast::op::BinaryOp::Assign,
        value,
        span: span(),
    }
}

fn take_stmt(arg: Expr) -> Stmt {
    Stmt::ExprStmt(call_expr("take", vec![arg]), span())
}

fn spawn_take_stmt(arg: Expr) -> Stmt {
    Stmt::ExprStmt(spawn_expr(call_expr("take", vec![arg])), span())
}

fn func_with(name: &str, params: Vec<Param>, stmts: Vec<Stmt>) -> Decl {
    Decl::FuncDecl(FuncDecl {
        name: ident(name),
        params,
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

fn param(name: &str, ty: &str) -> Param {
    Param {
        name: ident(name),
        ty: named_type(ty),
        default_value: None,
        span: span(),
    }
}

/// Assert the generated Rust source re-parses as a valid `syn::File`.
fn must_reparse(src: &str) {
    syn::parse_str::<syn::File>(src).expect("generated Rust must re-parse");
}

// ===========================================================================
// 1. Char is Copy (T33): used-after-move does NOT clone.
// ===========================================================================

#[test]
fn clone_analysis_char_used_after_move_no_clone() {
    // let c = 'A'; let c2 = c; take(c)
    // Char is Copy — re-use after the "move" into c2 should NOT clone.
    let f = func_with(
        "f",
        Vec::new(),
        vec![
            let_stmt("c", char_expr('A')),
            let_stmt("c2", ident_expr("c")),
            take_stmt(ident_expr("c")),
        ],
    );
    let src = generate_rust(&[f]).unwrap();
    assert!(
        !src.contains(".clone()"),
        "Char is Copy, no clone: src = {src}"
    );
    assert!(src.contains("'A'"), "char literal preserved: src = {src}");
    must_reparse(&src);
}

#[test]
fn clone_analysis_char_param_used_many_times_no_clone() {
    // func f(c: Char) { take(c); take(c); take(c) } — no clones.
    let f = func_with(
        "f",
        vec![param("c", "Char")],
        vec![
            take_stmt(ident_expr("c")),
            take_stmt(ident_expr("c")),
            take_stmt(ident_expr("c")),
        ],
    );
    let src = generate_rust(&[f]).unwrap();
    assert!(!src.contains(".clone()"), "src = {src}");
    must_reparse(&src);
}

// ===========================================================================
// 2. Move-at-let-binding: `let v2 = v` (non-Copy v) moves v; later use(v)
//    gets `.clone()`.
// ===========================================================================

#[test]
fn clone_analysis_string_move_at_let_binding_then_use_clones() {
    // let s = "hi"; let s2 = s; take(s)
    // The let-RHS counts as a use (move into s2). The later take(s) is the
    // SECOND use, so it must clone.
    let f = func_with(
        "f",
        Vec::new(),
        vec![
            let_stmt("s", string_expr("hi")),
            let_stmt("s2", ident_expr("s")),
            take_stmt(ident_expr("s")),
        ],
    );
    let src = generate_rust(&[f]).unwrap();
    // The let-RHS `let s2 = s;` is the first (move) — no clone.
    assert!(
        src.contains("let s2: String = s;"),
        "first use is a move: src = {src}"
    );
    // The take(s) is the second use — clone.
    assert!(
        src.contains("take(s.clone());"),
        "second use must clone: src = {src}"
    );
    must_reparse(&src);
}

#[test]
fn clone_analysis_vector_move_at_let_binding_then_use_clones() {
    // let v = [1, 2]; let v2 = v; take(v) — v used after move into v2.
    let f = func_with(
        "f",
        Vec::new(),
        vec![
            let_stmt("v", array_expr(vec![int_expr(1), int_expr(2)])),
            let_stmt("v2", ident_expr("v")),
            take_stmt(ident_expr("v")),
        ],
    );
    let src = generate_rust(&[f]).unwrap();
    // The later take(v) must clone (v was already moved into v2).
    assert!(
        src.contains("take(v.clone());"),
        "second use after move-at-let must clone: src = {src}"
    );
    must_reparse(&src);
}

// ===========================================================================
// 3. Int used-after-move: NO clone (Copy type).
// ===========================================================================

#[test]
fn clone_analysis_int_move_at_let_binding_no_clone() {
    // let x = 42; let y = x; take(x) — Int is Copy, no clone anywhere.
    let f = func_with(
        "f",
        Vec::new(),
        vec![
            let_stmt("x", int_expr(42)),
            let_stmt("y", ident_expr("x")),
            take_stmt(ident_expr("x")),
        ],
    );
    let src = generate_rust(&[f]).unwrap();
    assert!(!src.contains(".clone()"), "Int is Copy: src = {src}");
    must_reparse(&src);
}

// ===========================================================================
// 4. First use = move (no clone); multi-use all clone after the first.
// ===========================================================================

#[test]
fn clone_analysis_string_first_use_no_clone() {
    // let s = "hi"; take(s) — single use, no clone.
    let f = func_with(
        "f",
        Vec::new(),
        vec![let_stmt("s", string_expr("hi")), take_stmt(ident_expr("s"))],
    );
    let src = generate_rust(&[f]).unwrap();
    assert!(src.contains("take(s);"), "first use no clone: src = {src}");
    assert!(!src.contains("take(s.clone())"), "src = {src}");
    must_reparse(&src);
}

#[test]
fn clone_analysis_string_multi_use_clones_after_first() {
    // let s = "hi"; take(s); take(s); take(s)
    // 1st use = move (no clone); 2nd+ = clone.
    let f = func_with(
        "f",
        Vec::new(),
        vec![
            let_stmt("s", string_expr("hi")),
            take_stmt(ident_expr("s")),
            take_stmt(ident_expr("s")),
            take_stmt(ident_expr("s")),
        ],
    );
    let src = generate_rust(&[f]).unwrap();
    // Exactly one plain `take(s);` (the first use).
    let plain = src.matches("take(s);").count();
    assert_eq!(plain, 1, "first use no clone: src = {src}");
    // Exactly two clones (2nd and 3rd uses).
    let clones = src.matches("take(s.clone());").count();
    assert_eq!(clones, 2, "2nd+ use must clone: src = {src}");
    must_reparse(&src);
}

// ===========================================================================
// 5. Spawn → Arc::new at the binding's definition.
// ===========================================================================

#[test]
fn clone_analysis_spawn_captured_var_gets_arc_new() {
    // let s = "hi"; spawn take(s)
    // s is non-Copy and captured across spawn → wrap in Arc::new at the let.
    let f = func_with(
        "f",
        Vec::new(),
        vec![
            let_stmt("s", string_expr("hi")),
            spawn_take_stmt(ident_expr("s")),
        ],
    );
    let src = generate_rust(&[f]).unwrap();
    assert!(
        src.contains("std::sync::Arc::new("),
        "expected Arc::new at binding: src = {src}"
    );
    must_reparse(&src);
}

#[test]
fn clone_analysis_spawn_captured_var_gets_arc_clone_inside_spawn() {
    // let s = "hi"; spawn take(s)
    // Inside the spawn body, the use of s becomes Arc::clone(&s).
    let f = func_with(
        "f",
        Vec::new(),
        vec![
            let_stmt("s", string_expr("hi")),
            spawn_take_stmt(ident_expr("s")),
        ],
    );
    let src = generate_rust(&[f]).unwrap();
    assert!(
        src.contains("std::sync::Arc::clone(&s)"),
        "expected Arc::clone(&s) inside spawn: src = {src}"
    );
    must_reparse(&src);
}

#[test]
fn clone_analysis_spawn_does_not_arc_wrap_int() {
    // let n = 42; spawn take(n) — Int is Copy; no Arc::new, no Arc::clone.
    let f = func_with(
        "f",
        Vec::new(),
        vec![
            let_stmt("n", int_expr(42)),
            spawn_take_stmt(ident_expr("n")),
        ],
    );
    let src = generate_rust(&[f]).unwrap();
    assert!(
        !src.contains("Arc"),
        "Int should not be Arc-wrapped: src = {src}"
    );
    must_reparse(&src);
}

#[test]
fn clone_analysis_spawn_does_not_arc_wrap_char() {
    // let c = 'A'; spawn take(c) — Char is Copy; no Arc::new, no Arc::clone.
    let f = func_with(
        "f",
        Vec::new(),
        vec![
            let_stmt("c", char_expr('X')),
            spawn_take_stmt(ident_expr("c")),
        ],
    );
    let src = generate_rust(&[f]).unwrap();
    assert!(
        !src.contains("Arc"),
        "Char should not be Arc-wrapped: src = {src}"
    );
    must_reparse(&src);
}

#[test]
fn clone_analysis_no_spawn_no_arc_anywhere() {
    // let s = "hi"; take(s) — no spawn; no Arc::new / Arc::clone.
    let f = func_with(
        "f",
        Vec::new(),
        vec![let_stmt("s", string_expr("hi")), take_stmt(ident_expr("s"))],
    );
    let src = generate_rust(&[f]).unwrap();
    assert!(!src.contains("Arc"), "no spawn → no Arc: src = {src}");
    must_reparse(&src);
}

// ===========================================================================
// 6. CoW mutation: Arc-shared binding that is subsequently mutated gets
//    Arc::make_mut at the assignment site.
// ===========================================================================

#[test]
fn clone_analysis_arc_mut_var_gets_make_mut() {
    // let mut v = [1, 2]; spawn take(v); v = [3, 4]
    // v is captured across spawn (Arc) AND subsequently mutated (CoW).
    let f = func_with(
        "f",
        Vec::new(),
        vec![
            let_stmt_mut("v", array_expr(vec![int_expr(1), int_expr(2)])),
            spawn_take_stmt(ident_expr("v")),
            assign_stmt("v", array_expr(vec![int_expr(3), int_expr(4)])),
        ],
    );
    let src = generate_rust(&[f]).unwrap();
    // Arc::new at the let.
    assert!(
        src.contains("std::sync::Arc::new("),
        "expected Arc::new at let: src = {src}"
    );
    // Arc::make_mut at the assignment site.
    assert!(
        src.contains("*std::sync::Arc::make_mut(&mut v)"),
        "expected *Arc::make_mut(&mut v) at assignment: src = {src}"
    );
    must_reparse(&src);
}

#[test]
fn clone_analysis_arc_var_not_mutated_no_make_mut() {
    // let s = "hi"; spawn take(s) — captured but NOT mutated; no make_mut.
    let f = func_with(
        "f",
        Vec::new(),
        vec![
            let_stmt("s", string_expr("hi")),
            spawn_take_stmt(ident_expr("s")),
        ],
    );
    let src = generate_rust(&[f]).unwrap();
    assert!(
        !src.contains("make_mut"),
        "no mutation → no make_mut: src = {src}"
    );
    must_reparse(&src);
}

// ===========================================================================
// 7. Determinism: two runs of the same program produce byte-identical Rust.
//    (T29 flaky-test lesson — never rely on HashMap iteration order.)
// ===========================================================================

#[test]
fn clone_analysis_deterministic_two_runs_match() {
    let mk = || {
        func_with(
            "f",
            Vec::new(),
            vec![
                let_stmt("s", string_expr("hi")),
                let_stmt("n", int_expr(42)),
                let_stmt_mut("v", array_expr(vec![int_expr(1), int_expr(2)])),
                spawn_take_stmt(ident_expr("s")),
                spawn_take_stmt(ident_expr("v")),
                assign_stmt("v", array_expr(vec![int_expr(9)])),
                take_stmt(ident_expr("n")),
            ],
        )
    };
    let a = generate_rust(&[mk()]).unwrap();
    let b = generate_rust(&[mk()]).unwrap();
    assert_eq!(
        a, b,
        "deterministic codegen — two runs must match byte-for-byte"
    );
}

// ===========================================================================
// 8. Arc-wrapped `let` SKIPS the type annotation (inference handles
//    `Arc<T>`). Emitting `let s: String = Arc::new(...)` would be
//    incoherent Rust; we drop the annotation and let rustc infer `Arc<T>`.
// ===========================================================================

#[test]
fn clone_analysis_arc_wrapped_let_skips_type_annotation() {
    // let s = "hi"; spawn take(s)
    // The binding is Arc-wrapped, so the type annotation is dropped
    // (the actual Rust type is `Arc<String>`, not `String` — emitting
    // `let s: String = Arc::new(...)` would be a type error).
    let f = func_with(
        "f",
        Vec::new(),
        vec![
            let_stmt("s", string_expr("hi")),
            spawn_take_stmt(ident_expr("s")),
        ],
    );
    let src = generate_rust(&[f]).unwrap();
    // The Arc wrap is there.
    assert!(
        src.contains("std::sync::Arc::new("),
        "expected Arc::new at binding: src = {src}"
    );
    // No `: String` annotation — Arc wrap drops it.
    assert!(
        !src.contains("let s: String"),
        "Arc-wrapped let must skip the annotation: src = {src}"
    );
    assert!(
        src.contains("let s = std::sync::Arc::new("),
        "expected unannotated Arc-wrapped let: src = {src}"
    );
    must_reparse(&src);
}

// ===========================================================================
// 9. Multiple spawns of the same Arc var: each gets its own Arc::clone.
// ===========================================================================

#[test]
fn clone_analysis_multiple_spawns_each_get_arc_clone() {
    // let s = "hi"; spawn take(s); spawn take(s)
    let f = func_with(
        "f",
        Vec::new(),
        vec![
            let_stmt("s", string_expr("hi")),
            spawn_take_stmt(ident_expr("s")),
            spawn_take_stmt(ident_expr("s")),
        ],
    );
    let src = generate_rust(&[f]).unwrap();
    // Two Arc::clone(&s) — one per spawn body.
    let count = src.matches("std::sync::Arc::clone(&s)").count();
    assert_eq!(
        count, 2,
        "expected 2 Arc::clone(&s), got {count}: src = {src}"
    );
    // Only ONE Arc::new (at the single let).
    let news = src.matches("std::sync::Arc::new(").count();
    assert_eq!(news, 1, "expected 1 Arc::new at the let: src = {src}");
    must_reparse(&src);
}

// ===========================================================================
// 10. Existing v0.1 behavior still works: typed let with mut + clone.
// ===========================================================================

#[test]
fn clone_analysis_typed_mut_let_string_used_twice_clones() {
    // let mut s: String = "hi"; take(s); take(s)
    let f = func_with(
        "f",
        Vec::new(),
        vec![
            let_stmt_typed("s", named_type("String"), string_expr("hi")),
            take_stmt(ident_expr("s")),
            take_stmt(ident_expr("s")),
        ],
    );
    let src = generate_rust(&[f]).unwrap();
    assert!(src.contains("take(s);"), "first use no clone: src = {src}");
    assert!(
        src.contains("take(s.clone());"),
        "second use clones: src = {src}"
    );
    must_reparse(&src);
}

// ===========================================================================
// 11. Compile-test (ignored by default — requires rustc on PATH).
//     Verifies the Arc-wrapped Rust pattern (Arc::new + Arc::clone +
//     Arc::make_mut) actually compiles without borrow-checker errors.
//
// The codegen-emitted form (`tokio::spawn(async move { ... })`) requires
// the tokio runtime to compile, which isn't available in single-file
// `rustc` mode (no Cargo manifest). To keep the test self-contained we
// compile a hand-crafted Rust snippet that MIRRORS exactly what the
// codegen emits for `let s = "hi"; spawn take(s); s = "bye"` minus the
// tokio wrapper — i.e. the Arc::new wrap at the let, an Arc::clone
// inside the (would-be) spawn body, and the *Arc::make_mut deref at the
// mutation site. The string-presence tests above already verify the
// codegen output contains these constructs; this test verifies they
// actually COMPILE.
// ===========================================================================

#[test]
#[ignore = "requires rustc on PATH; run with: cargo test -- --ignored \
            clone_analysis_arc_pattern_compiles_with_rustc"]
fn clone_analysis_arc_pattern_compiles_with_rustc() {
    use std::process::Command;

    // Mirrors the codegen output for:
    //   let mut s = "hi"; spawn take(s); s = "bye"
    // (with the tokio::spawn wrapper stripped — single-file rustc has no
    // tokio crate available, so we exercise the Arc/CoW mechanics
    // directly).
    let src = r#"
fn take<T>(_x: T) {}

fn main() {
    // Arc-wrap at the let (non-Copy binding captured across spawn).
    let mut s = std::sync::Arc::new(String::from("hi"));
    // Arc::clone inside the spawn body (cheap refcount bump).
    let _task_local_copy = std::sync::Arc::clone(&s);
    take(_task_local_copy);
    // CoW mutation via *Arc::make_mut(&mut s) = ...
    *std::sync::Arc::make_mut(&mut s) = String::from("bye");
}
"#;

    // Write to a temp file and compile.
    let temp_dir = std::env::temp_dir();
    let rs_path = temp_dir.join("buff_t33_arc_compile_test.rs");
    let out_path = temp_dir.join("buff_t33_arc_compile_test.out");
    std::fs::write(&rs_path, src).expect("write temp .rs");

    let result = Command::new("rustc")
        .args(["--edition", "2021"])
        .arg(&rs_path)
        .arg("-o")
        .arg(&out_path)
        .output();

    // Cleanup regardless of outcome.
    let _ = std::fs::remove_file(&rs_path);
    let _ = std::fs::remove_file(&out_path);

    let output = result.expect("rustc is not on PATH — install rustc to run this test");
    assert!(
        output.status.success(),
        "rustc failed to compile the Arc/CoW pattern:\n--- src ---\n{src}\n\
         --- stdout ---\n{}\n--- stderr ---\n{}\n",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

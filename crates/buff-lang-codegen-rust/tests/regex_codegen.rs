//! T124d integration tests — `Regex` prelude module codegen.
//!
//! Verifies that the Rust codegen:
//! - Lowers `Regex.compile(p)` to `regex::Regex::new(p).unwrap_or_else(...)`.
//! - Lowers `regex.match(text)` to an `if recv.is_match(text) { Some(...) } else { None }`.
//! - Lowers `regex.find(text)` to `recv.find(text).map(|m| m.as_str().to_string())`.
//! - Lowers `regex.replace(text, repl)` to `recv.replace_all(text, repl).to_string()`.
//! - Lowers `regex.captures(text)` to a deterministic block that populates
//!   a `HashMap<String, String>` with numbered groups (in index order)
//!   and named groups (in source-declaration order).
//! - Records `regex` in `extern_crates` whenever the program uses `Regex`.
//! - Emits the `regex::Regex::...` fully-qualified paths so NO `use` import
//!   is required in the generated source.
//!
//! Acceptance snapshot for the task's canonical criterion:
//!
//! ```text
//! regex.replace("a1b2", "\\d", "X")  ->  "aXbX"
//! ```
//!
//! Run via:
//!
//! ```text
//! cargo test -p buff-lang-codegen-rust --test regex_codegen
//! ```
//!
//! # Note on `match` as a method name
//!
//! `match` is a Buff keyword (one of the 25 reserved). The parser today
//! only accepts `TokenKind::Ident(_)` in method-call position, so
//! `regex.match(text)` will NOT parse from Buff source. The codegen +
//! registry still wire up the `Match` variant so:
//!   (a) these AST-constructed tests exercise the lowering end-to-end,
//!   (b) a future parser relaxation lights it up with no further work.
//! The other three instance methods (find/replace/captures) parse fine
//! since they're not keywords.

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

/// `Regex.<method>(args...)` AST node (associated-function call shape).
fn regex_assoc_call(method: &str, args: Vec<Expr>) -> Expr {
    Expr::MethodCall {
        receiver: Box::new(ident_expr("Regex")),
        method: ident(method),
        args,
        span: span(),
    }
}

/// `recv.<method>(args...)` AST node (instance-method call shape).
fn method_call(recv: Expr, method: &str, args: Vec<Expr>) -> Expr {
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

// ---------------------------------------------------------------------------
// 1. Regex.compile(pattern) -> regex::Regex::new(pattern).unwrap_or_else(...)
// ---------------------------------------------------------------------------

#[test]
fn regex_codegen_compile_string_literal() {
    let src = codegen_one_expr_in("f", regex_assoc_call("compile", vec![str_expr(r"\d+")]));
    assert!(
        src.contains("regex::Regex::new"),
        "expected `regex::Regex::new` in: {src}"
    );
    // The fallback is the provably-valid never-match regex `a^`.
    assert!(
        src.contains(r#""a^""#),
        "expected fallback regex `\"a^\"` in: {src}"
    );
    // unwrap_or_else (NOT bare unwrap) — panicking-generated-code rule.
    assert!(
        src.contains("unwrap_or_else"),
        "expected `unwrap_or_else` (panic-free fallback) in: {src}"
    );
    // No bare `.unwrap()` on the user's pattern (only on the fallback).
    // Counting: there should be exactly ONE `.unwrap()` call, on the
    // fallback regex `Regex::new(r"a^")`.
    let unwrap_count = src.matches(".unwrap()").count();
    assert_eq!(
        unwrap_count, 1,
        "expected exactly 1 `.unwrap()` (on the fallback), got {unwrap_count} in:\n{src}"
    );
    must_reparse(&src);
}

#[test]
fn regex_codegen_compile_via_ident_arg() {
    // Regex.compile(my_pattern_var) — non-literal arg borrows via &.
    let src = codegen_one_expr_in(
        "f",
        regex_assoc_call("compile", vec![ident_expr("my_pattern_var")]),
    );
    // The ident should be borrowed (&my_pattern_var) so Rust's Deref
    // coercion turns String into &str.
    assert!(
        src.contains("&my_pattern_var"),
        "expected `&my_pattern_var` (borrow coercion for String -> &str) in: {src}"
    );
    must_reparse(&src);
}

// ---------------------------------------------------------------------------
// 2. regex.match(text) -> Option<String>
// ---------------------------------------------------------------------------

#[test]
fn regex_codegen_match_string_literal() {
    // `r.match("text")` — note `match` is a Buff keyword; the parser
    // doesn't accept it from source, but AST-constructed tests can.
    let r_pat = let_stmt("r", regex_assoc_call("compile", vec![str_expr(r"\d+")]));
    let body = vec![
        r_pat,
        expr_stmt(method_call(
            ident_expr("r"),
            "match",
            vec![str_expr("abc123")],
        )),
    ];
    let src = codegen_stmts_in("f", body);
    // Wraps is_match in an if/else producing Some(text)/None.
    assert!(
        src.contains(".is_match("),
        "expected `.is_match(` in: {src}"
    );
    assert!(
        src.contains("Some(") && src.contains("None"),
        "expected `Some(...)`/`None` Option wrapping in: {src}"
    );
    assert!(
        src.contains(".to_string()"),
        "expected `.to_string()` on the wrapped text in: {src}"
    );
    must_reparse(&src);
}

// ---------------------------------------------------------------------------
// 3. regex.find(text) -> Option<String>
// ---------------------------------------------------------------------------

#[test]
fn regex_codegen_find_string_literal() {
    let r_pat = let_stmt("r", regex_assoc_call("compile", vec![str_expr(r"\d+")]));
    let body = vec![
        r_pat,
        expr_stmt(method_call(
            ident_expr("r"),
            "find",
            vec![str_expr("abc123")],
        )),
    ];
    let src = codegen_stmts_in("f", body);
    // `.find(text).map(|m| m.as_str().to_string())`
    assert!(src.contains(".find("), "expected `.find(` in: {src}");
    assert!(
        src.contains(".map(|m| m.as_str().to_string())"),
        "expected `.map(|m| m.as_str().to_string())` in: {src}"
    );
    must_reparse(&src);
}

// ---------------------------------------------------------------------------
// 4. regex.replace(text, replacement) -> String (replace ALL matches)
// ---------------------------------------------------------------------------

#[test]
fn regex_codegen_replace_all_semantics() {
    // The task's canonical acceptance: regex.replace("a1b2","\d","X") == "aXbX".
    // replace_all (NOT replace) — verifies ALL matches are replaced.
    let r_pat = let_stmt("r", regex_assoc_call("compile", vec![str_expr(r"\d")]));
    let body = vec![
        r_pat,
        expr_stmt(method_call(
            ident_expr("r"),
            "replace",
            vec![str_expr("a1b2"), str_expr("X")],
        )),
    ];
    let src = codegen_stmts_in("f", body);
    // `.replace_all(text, repl).to_string()`
    assert!(
        src.contains(".replace_all("),
        "expected `.replace_all(` (NOT `.replace(` which would do one match) in: {src}"
    );
    assert!(
        src.contains(".to_string()"),
        "expected `.to_string()` (Cow<str> -> String) in: {src}"
    );
    must_reparse(&src);
}

#[test]
fn regex_codegen_replace_via_ident_args() {
    // replace(my_text, my_repl) — non-literal args borrow via &.
    let r_pat = let_stmt("r", regex_assoc_call("compile", vec![str_expr(r"\d")]));
    let body = vec![
        r_pat,
        expr_stmt(method_call(
            ident_expr("r"),
            "replace",
            vec![ident_expr("my_text"), ident_expr("my_repl")],
        )),
    ];
    let src = codegen_stmts_in("f", body);
    assert!(
        src.contains("&my_text") && src.contains("&my_repl"),
        "expected borrows for non-literal args in: {src}"
    );
    must_reparse(&src);
}

// ---------------------------------------------------------------------------
// 5. regex.captures(text) -> Map<String, String>
// ---------------------------------------------------------------------------

#[test]
fn regex_codegen_captures_builds_hashmap() {
    let r_pat = let_stmt(
        "r",
        regex_assoc_call("compile", vec![str_expr(r"(\w)(\d)")]),
    );
    let body = vec![
        r_pat,
        expr_stmt(method_call(
            ident_expr("r"),
            "captures",
            vec![str_expr("a1")],
        )),
    ];
    let src = codegen_stmts_in("f", body);
    // The block uses std::collections::HashMap (fully-qualified, no `use`).
    assert!(
        src.contains("std::collections::HashMap"),
        "expected `std::collections::HashMap` (fully-qualified) in: {src}"
    );
    // Numbered-group iteration via `caps.iter().enumerate()`.
    assert!(
        src.contains(".iter().enumerate()"),
        "expected `.iter().enumerate()` for numbered groups in: {src}"
    );
    // Named-group iteration via `recv.capture_names().flatten()`.
    assert!(
        src.contains(".capture_names()"),
        "expected `.capture_names()` for named-group iteration in: {src}"
    );
    assert!(
        src.contains(".flatten()"),
        "expected `.flatten()` to skip unnamed positions in: {src}"
    );
    // Group lookup via `.name(__buff_name)`.
    assert!(
        src.contains(".name(__buff_name)"),
        "expected `.name(__buff_name)` named-group lookup in: {src}"
    );
    // The full match is keyed as "0" via `__buff_i.to_string()`.
    assert!(
        src.contains("__buff_i.to_string()"),
        "expected `__buff_i.to_string()` (numbered-group key) in: {src}"
    );
    must_reparse(&src);
}

#[test]
fn regex_codegen_captures_deterministic_source() {
    // Same AST → byte-identical Rust. Run codegen twice; assert equality.
    let r_pat = let_stmt("r", regex_assoc_call("compile", vec![str_expr(r"(\w+)")]));
    let body = vec![
        r_pat,
        expr_stmt(method_call(
            ident_expr("r"),
            "captures",
            vec![str_expr("hello")],
        )),
    ];
    let src1 = codegen_stmts_in("f", body.clone());
    let src2 = codegen_stmts_in("f", body);
    assert_eq!(src1, src2, "Regex.captures codegen must be deterministic");
}

// ---------------------------------------------------------------------------
// 6. extern_crates records regex when Regex is used.
// ---------------------------------------------------------------------------

#[test]
fn regex_codegen_records_regex_extern_crate() {
    let main = func_decl(
        "main",
        &[],
        vec![expr_stmt(regex_assoc_call(
            "compile",
            vec![str_expr(r"\d+")],
        ))],
    );
    let mut codegen = RustCodegen::new();
    let _ = codegen.generate(&[main]).expect("codegen must succeed");
    let extern_crates = codegen.extern_crates();
    assert!(
        extern_crates.contains("regex"),
        "extern_crates should contain `regex`, got: {:?}",
        extern_crates
    );
}

#[test]
fn regex_codegen_records_regex_extern_crate_on_instance_method() {
    // A program that uses regex.find but never explicitly calls
    // Regex.compile should still register `regex` (the walker flags
    // any recv.find(...) call conservatively).
    let main = func_decl(
        "main",
        &[],
        vec![expr_stmt(method_call(
            ident_expr("some_regex"),
            "find",
            vec![str_expr("text")],
        ))],
    );
    let mut codegen = RustCodegen::new();
    let _ = codegen.generate(&[main]).expect("codegen must succeed");
    let extern_crates = codegen.extern_crates();
    assert!(
        extern_crates.contains("regex"),
        "extern_crates should contain `regex` (conservative find walker), got: {:?}",
        extern_crates
    );
}

#[test]
fn regex_codegen_no_regex_extern_crate_when_unused() {
    // A program with no Regex calls should not register regex.
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
        !extern_crates.contains("regex"),
        "extern_crates should NOT contain `regex` when Regex is unused, got: {:?}",
        extern_crates
    );
}

#[test]
fn regex_codegen_records_regex_via_type_annotation() {
    // `let r: Regex = ...` should register regex even if the value
    // expression doesn't itself mention regex.
    let main = func_decl(
        "main",
        &[],
        vec![Stmt::LetDecl {
            name: ident("r"),
            value: str_expr("placeholder"),
            mutable: false,
            ty: Some(named_type("Regex")),
            span: span(),
        }],
    );
    let mut codegen = RustCodegen::new();
    let _ = codegen.generate(&[main]).expect("codegen must succeed");
    let extern_crates = codegen.extern_crates();
    assert!(
        extern_crates.contains("regex"),
        "extern_crates should contain `regex` via type annotation, got: {:?}",
        extern_crates
    );
}

// ---------------------------------------------------------------------------
// 7. Error cases.
// ---------------------------------------------------------------------------

#[test]
fn regex_codegen_rejects_compile_with_wrong_arity() {
    // Regex.compile() with no args — should error.
    let result = std::panic::catch_unwind(|| {
        let _ = codegen_one_expr_in("f", regex_assoc_call("compile", vec![]));
    });
    assert!(
        result.is_err(),
        "expected codegen to reject `Regex.compile()` (no pattern arg)"
    );
}

#[test]
fn regex_codegen_rejects_match_with_wrong_arity() {
    // regex.match(a, b) — too many args. The receiver must be a known
    // Regex value (via `let r = Regex.compile(...)`) so the codegen
    // dispatches to the prelude-instance-fn path that enforces arity.
    let result = std::panic::catch_unwind(|| {
        let r_pat = let_stmt("r", regex_assoc_call("compile", vec![str_expr(r"\d")]));
        let body = vec![
            r_pat,
            expr_stmt(method_call(
                ident_expr("r"),
                "match",
                vec![str_expr("a"), str_expr("b")],
            )),
        ];
        let _ = codegen_stmts_in("f", body);
    });
    assert!(
        result.is_err(),
        "expected codegen to reject `regex.match(a, b)` (too many args)"
    );
}

#[test]
fn regex_codegen_rejects_replace_with_wrong_arity() {
    // regex.replace(a) — too few args. Same setup as above.
    let result = std::panic::catch_unwind(|| {
        let r_pat = let_stmt("r", regex_assoc_call("compile", vec![str_expr(r"\d")]));
        let body = vec![
            r_pat,
            expr_stmt(method_call(ident_expr("r"), "replace", vec![str_expr("a")])),
        ];
        let _ = codegen_stmts_in("f", body);
    });
    assert!(
        result.is_err(),
        "expected codegen to reject `regex.replace(a)` (too few args)"
    );
}

// ---------------------------------------------------------------------------
// 8. insta snapshots — byte-stable codegen pinning.
// ---------------------------------------------------------------------------

#[test]
fn regex_codegen_compile_snapshot() {
    // Snapshot the canonical compile lowering.
    let src = codegen_one_expr_in("f", regex_assoc_call("compile", vec![str_expr(r"\d+")]));
    insta::assert_snapshot!(src);
}

#[test]
fn regex_codegen_match_snapshot() {
    // Snapshot the canonical match lowering.
    let r_pat = let_stmt("r", regex_assoc_call("compile", vec![str_expr(r"\d+")]));
    let body = vec![
        r_pat,
        expr_stmt(method_call(
            ident_expr("r"),
            "match",
            vec![str_expr("abc123")],
        )),
    ];
    let src = codegen_stmts_in("f", body);
    insta::assert_snapshot!(src);
}

#[test]
fn regex_codegen_find_snapshot() {
    // Snapshot the canonical find lowering.
    let r_pat = let_stmt("r", regex_assoc_call("compile", vec![str_expr(r"\d+")]));
    let body = vec![
        r_pat,
        expr_stmt(method_call(
            ident_expr("r"),
            "find",
            vec![str_expr("abc123")],
        )),
    ];
    let src = codegen_stmts_in("f", body);
    insta::assert_snapshot!(src);
}

#[test]
fn regex_codegen_replace_snapshot() {
    // Snapshot the canonical replace lowering (the task's acceptance:
    // regex.replace("a1b2", "\\d", "X") -> "aXbX").
    let r_pat = let_stmt("r", regex_assoc_call("compile", vec![str_expr(r"\d")]));
    let body = vec![
        r_pat,
        expr_stmt(method_call(
            ident_expr("r"),
            "replace",
            vec![str_expr("a1b2"), str_expr("X")],
        )),
    ];
    let src = codegen_stmts_in("f", body);
    insta::assert_snapshot!(src);
}

#[test]
fn regex_codegen_captures_snapshot() {
    // Snapshot the canonical captures lowering with a named + numbered
    // group pattern.
    let r_pat = let_stmt(
        "r",
        regex_assoc_call("compile", vec![str_expr(r"(?P<word>\w+)(\d)")]),
    );
    let body = vec![
        r_pat,
        expr_stmt(method_call(
            ident_expr("r"),
            "captures",
            vec![str_expr("hello1")],
        )),
    ];
    let src = codegen_stmts_in("f", body);
    insta::assert_snapshot!(src);
}

#[test]
fn regex_codegen_full_program_snapshot() {
    // End-to-end snapshot: a `main` that exercises every Regex surface
    // (compile + match + find + replace + captures). Pins the full
    // shape of the generated Rust for a typical Regex-using program
    // (the acceptance criterion from the task spec).
    let main = func_decl(
        "main",
        &[],
        vec![
            let_stmt("r", regex_assoc_call("compile", vec![str_expr(r"\d")])),
            expr_stmt(method_call(ident_expr("r"), "match", vec![str_expr("abc")])),
            expr_stmt(method_call(ident_expr("r"), "find", vec![str_expr("a1b2")])),
            expr_stmt(method_call(
                ident_expr("r"),
                "replace",
                vec![str_expr("a1b2"), str_expr("X")],
            )),
            expr_stmt(method_call(
                ident_expr("r"),
                "captures",
                vec![str_expr("a1")],
            )),
        ],
    );
    let src = generate_rust(&[main]).expect("codegen must succeed");
    insta::assert_snapshot!(src);
}

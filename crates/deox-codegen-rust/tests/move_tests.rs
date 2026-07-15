//! Integration tests for move-by-default semantics (T33a).
//!
//! Every binding in Deox is MOVED by default (Rust move semantics). The
//! codegen inserts `.clone()` automatically when a non-Copy variable is
//! used after being moved once. Copy primitives (Int/Float/Double/Bool/
//! Byte/Bits) never get cloned. Generated Rust never contains `&`, `&mut`,
//! or lifetime annotations.
//!
//! These tests build small Deox ASTs by hand, lower them with
//! [`deox_codegen_rust::generate_rust`], and assert properties of the
//! resulting Rust source.

use deox_ast::common::{Block, Ident, Param};
use deox_ast::decl::FuncDecl;
use deox_ast::{Decl, Expr, Literal, Stmt, TypeRef};
use deox_error::Span;

use deox_codegen_rust::generate_rust;

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

fn let_stmt(name: &str, value: Expr) -> Stmt {
    Stmt::LetDecl {
        name: ident(name),
        value,
        mutable: false,
        ty: None,
        span: span(),
    }
}

fn let_stmt_typed_mut(name: &str, ty: TypeRef, value: Expr) -> Stmt {
    Stmt::LetDecl {
        name: ident(name),
        value,
        mutable: true,
        ty: Some(ty),
        span: span(),
    }
}

fn take_stmt(arg: Expr) -> Stmt {
    Stmt::ExprStmt(call_expr("take", vec![arg]), span())
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
        span: span(),
    })
}

fn func_with_return(
    name: &str,
    params: Vec<Param>,
    ret: Option<TypeRef>,
    stmts: Vec<Stmt>,
) -> Decl {
    Decl::FuncDecl(FuncDecl {
        name: ident(name),
        params,
        return_type: ret,
        body: Block {
            stmts,
            span: span(),
        },
        is_async: false,
        is_unsafe: false,
        is_extern: false,
        span: span(),
    })
}

// ---------------------------------------------------------------------------
// 1. Int is Copy — no clone ever.
// ---------------------------------------------------------------------------

#[test]
fn test_move_simple_int() {
    // let x = 42; let y = x;
    let f = func_with(
        "f",
        Vec::new(),
        vec![let_stmt("x", int_expr(42)), let_stmt("y", ident_expr("x"))],
    );
    let src = generate_rust(&[f]).unwrap();
    assert!(!src.contains(".clone()"), "src = {src}");
    // T12: type annotations inferred — `let x: i64 = 42`, `let y: i64 = x`.
    assert!(src.contains("let x: i64 = 42"), "src = {src}");
    assert!(src.contains("let y: i64 = x"), "src = {src}");
    syn::parse_str::<syn::File>(&src).expect("must re-parse");
}

// ---------------------------------------------------------------------------
// 2. String — first use is a move, no clone.
// ---------------------------------------------------------------------------

#[test]
fn test_move_string_no_clone_on_first_use() {
    // let s = "hi"; let s2 = s;
    let f = func_with(
        "f",
        Vec::new(),
        vec![
            let_stmt("s", string_expr("hi")),
            let_stmt("s2", ident_expr("s")),
        ],
    );
    let src = generate_rust(&[f]).unwrap();
    assert!(!src.contains(".clone()"), "src = {src}");
    // T12: type annotations inferred — `let s: String = "hi"`.
    assert!(src.contains(r#"let s: String = "hi""#), "src = {src}");
    // First use of s — no clone.
    assert!(src.contains("let s2: String = s;"), "src = {src}");
    syn::parse_str::<syn::File>(&src).expect("must re-parse");
}

// ---------------------------------------------------------------------------
// 3. String used twice — second use gets `.clone()`.
// ---------------------------------------------------------------------------

#[test]
fn test_string_used_after_move_gets_clone() {
    // let s = "hi"; use(s); use(s);
    let f = func_with(
        "f",
        Vec::new(),
        vec![
            let_stmt("s", string_expr("hi")),
            take_stmt(ident_expr("s")),
            take_stmt(ident_expr("s")),
        ],
    );
    let src = generate_rust(&[f]).unwrap();
    // First use: no clone. Second use: clone.
    assert!(src.contains("take(s);"), "src = {src}");
    assert!(src.contains("take(s.clone());"), "src = {src}");
    syn::parse_str::<syn::File>(&src).expect("must re-parse");
}

// ---------------------------------------------------------------------------
// 4. Int used many times — never any clone (Copy type).
// ---------------------------------------------------------------------------

#[test]
fn test_int_used_multiple_times_no_clone() {
    // let x = 42; emit(x); emit(x);
    // (Use `emit` instead of `print` so the print→println! mapping doesn't
    // transform the call site — we want to count bare ident uses here.)
    let f = func_with(
        "f",
        Vec::new(),
        vec![
            let_stmt("x", int_expr(42)),
            Stmt::ExprStmt(call_expr("emit", vec![ident_expr("x")]), span()),
            Stmt::ExprStmt(call_expr("emit", vec![ident_expr("x")]), span()),
        ],
    );
    let src = generate_rust(&[f]).unwrap();
    assert!(!src.contains(".clone()"), "src = {src}");
    // Both uses of x without clone.
    let occurrences = src.matches("emit(x)").count();
    assert_eq!(occurrences, 2, "src = {src}");
    syn::parse_str::<syn::File>(&src).expect("must re-parse");
}

// ---------------------------------------------------------------------------
// 5. Function param: no `&` or `&mut` in signature.
// ---------------------------------------------------------------------------

#[test]
fn test_func_param_no_ref() {
    // func process(data: String) { }
    let f = func_with(
        "process",
        vec![Param {
            name: ident("data"),
            ty: named_type("String"),
            span: span(),
        }],
        Vec::new(),
    );
    let src = generate_rust(&[f]).unwrap();
    assert!(
        src.contains("data: String"),
        "expected `data: String` in signature, src = {src}"
    );
    assert!(
        !src.contains("&String"),
        "no borrow in signature, src = {src}"
    );
    assert!(!src.contains("&mut"), "no &mut in signature, src = {src}");
    syn::parse_str::<syn::File>(&src).expect("must re-parse");
}

// ---------------------------------------------------------------------------
// 6. Calling a function moves the argument (no borrow at call site).
// ---------------------------------------------------------------------------

#[test]
fn test_func_param_move() {
    // func process(data: String) { }
    // func caller() { let my_str = "x"; process(my_str); }
    let process = func_with(
        "process",
        vec![Param {
            name: ident("data"),
            ty: named_type("String"),
            span: span(),
        }],
        Vec::new(),
    );
    let caller = func_with(
        "caller",
        Vec::new(),
        vec![
            let_stmt("my_str", string_expr("x")),
            Stmt::ExprStmt(call_expr("process", vec![ident_expr("my_str")]), span()),
        ],
    );
    let src = generate_rust(&[process, caller]).unwrap();
    // Calling `process(my_str)` — first use of my_str, no clone, no borrow.
    assert!(
        src.contains("process(my_str)"),
        "expected `process(my_str)` (move, no borrow), src = {src}"
    );
    assert!(
        !src.contains("&my_str"),
        "no borrow at call site, src = {src}"
    );
    syn::parse_str::<syn::File>(&src).expect("must re-parse");
}

// ---------------------------------------------------------------------------
// 7. Var used twice across function calls — second call gets clone.
// ---------------------------------------------------------------------------

#[test]
fn test_var_used_after_func_call_gets_clone() {
    // let v = "x"; consume(v); consume(v);
    let f = func_with(
        "f",
        Vec::new(),
        vec![
            let_stmt("v", string_expr("x")),
            Stmt::ExprStmt(call_expr("consume", vec![ident_expr("v")]), span()),
            Stmt::ExprStmt(call_expr("consume", vec![ident_expr("v")]), span()),
        ],
    );
    let src = generate_rust(&[f]).unwrap();
    // First call moves, second call clones.
    assert!(src.contains("consume(v);"), "src = {src}");
    assert!(src.contains("consume(v.clone());"), "src = {src}");
    syn::parse_str::<syn::File>(&src).expect("must re-parse");
}

// ---------------------------------------------------------------------------
// 8. No lifetime annotations in function signatures.
// ---------------------------------------------------------------------------

#[test]
fn test_no_lifetimes_in_signature() {
    // func foo(a: String, b: String) -> String { return a; }
    let f = func_with_return(
        "foo",
        vec![
            Param {
                name: ident("a"),
                ty: named_type("String"),
                span: span(),
            },
            Param {
                name: ident("b"),
                ty: named_type("String"),
                span: span(),
            },
        ],
        Some(named_type("String")),
        vec![Stmt::Return(Some(ident_expr("a")), span())],
    );
    let src = generate_rust(&[f]).unwrap();
    // No lifetime annotations anywhere in the output.
    assert!(!src.contains('\''), "no lifetime annotations, src = {src}");
    assert!(
        src.contains("fn foo(a: String, b: String) -> String"),
        "expected plain signature, src = {src}"
    );
    syn::parse_str::<syn::File>(&src).expect("must re-parse");
}

// ---------------------------------------------------------------------------
// 9. Two independent strings — each gets its own clone on 2nd use.
// ---------------------------------------------------------------------------

#[test]
fn test_multiple_strings_independent() {
    // let a = "x"; let b = "y"; use(a); use(a); use(b); use(b);
    let f = func_with(
        "f",
        Vec::new(),
        vec![
            let_stmt("a", string_expr("x")),
            let_stmt("b", string_expr("y")),
            take_stmt(ident_expr("a")),
            take_stmt(ident_expr("a")),
            take_stmt(ident_expr("b")),
            take_stmt(ident_expr("b")),
        ],
    );
    let src = generate_rust(&[f]).unwrap();
    // Each variable's 2nd use gets a clone; first uses don't.
    assert!(
        src.contains("take(a);"),
        "first use of a should not clone, src = {src}"
    );
    assert!(
        src.contains("take(a.clone());"),
        "second use of a should clone, src = {src}"
    );
    assert!(
        src.contains("take(b);"),
        "first use of b should not clone, src = {src}"
    );
    assert!(
        src.contains("take(b.clone());"),
        "second use of b should clone, src = {src}"
    );
    syn::parse_str::<syn::File>(&src).expect("must re-parse");
}

// ---------------------------------------------------------------------------
// 10. String parameter used twice — second use inside body gets clone.
// ---------------------------------------------------------------------------

#[test]
fn test_string_param_used_twice_gets_clone() {
    // func greet(name: String) { use(name); use(name); }
    let f = func_with(
        "greet",
        vec![Param {
            name: ident("name"),
            ty: named_type("String"),
            span: span(),
        }],
        vec![take_stmt(ident_expr("name")), take_stmt(ident_expr("name"))],
    );
    let src = generate_rust(&[f]).unwrap();
    assert!(src.contains("take(name);"), "src = {src}");
    assert!(src.contains("take(name.clone());"), "src = {src}");
    syn::parse_str::<syn::File>(&src).expect("must re-parse");
}

// ---------------------------------------------------------------------------
// 11. Int parameter used many times — no clone (param is Copy).
// ---------------------------------------------------------------------------

#[test]
fn test_int_param_used_many_times_no_clone() {
    // func add1(n: Int) { emit(n); emit(n); emit(n); }
    // (Use `emit` instead of `print` so the print→println! mapping doesn't
    // transform the call site — we want to count bare ident uses here.)
    let f = func_with(
        "add1",
        vec![Param {
            name: ident("n"),
            ty: named_type("Int"),
            span: span(),
        }],
        vec![
            Stmt::ExprStmt(call_expr("emit", vec![ident_expr("n")]), span()),
            Stmt::ExprStmt(call_expr("emit", vec![ident_expr("n")]), span()),
            Stmt::ExprStmt(call_expr("emit", vec![ident_expr("n")]), span()),
        ],
    );
    let src = generate_rust(&[f]).unwrap();
    assert!(!src.contains(".clone()"), "src = {src}");
    let n_uses = src.matches("emit(n)").count();
    assert_eq!(n_uses, 3, "src = {src}");
    syn::parse_str::<syn::File>(&src).expect("must re-parse");
}

// ---------------------------------------------------------------------------
// 12. Bool literal — Copy type, no clone.
// ---------------------------------------------------------------------------

#[test]
fn test_bool_var_no_clone() {
    // let b = true; let b2 = b; let b3 = b;
    let f = func_with(
        "f",
        Vec::new(),
        vec![
            let_stmt("b", Expr::Literal(Literal::Bool(true), span())),
            let_stmt("b2", ident_expr("b")),
            let_stmt("b3", ident_expr("b")),
        ],
    );
    let src = generate_rust(&[f]).unwrap();
    assert!(!src.contains(".clone()"), "src = {src}");
    syn::parse_str::<syn::File>(&src).expect("must re-parse");
}

// ---------------------------------------------------------------------------
// 13. Copy propagation: a String assigned from a String var still non-Copy.
// ---------------------------------------------------------------------------

#[test]
fn test_string_assigned_from_string_var_still_non_copy() {
    // let s = "hi"; let t = s; use(t); use(t);
    // t inherits non-Copy-ness from s; second use of t should clone.
    let f = func_with(
        "f",
        Vec::new(),
        vec![
            let_stmt("s", string_expr("hi")),
            let_stmt("t", ident_expr("s")),
            take_stmt(ident_expr("t")),
            take_stmt(ident_expr("t")),
        ],
    );
    let src = generate_rust(&[f]).unwrap();
    // First use of t (which already consumed s as a move): no clone.
    assert!(src.contains("take(t);"), "src = {src}");
    // Second use of t: clone.
    assert!(src.contains("take(t.clone());"), "src = {src}");
    syn::parse_str::<syn::File>(&src).expect("must re-parse");
}

// ---------------------------------------------------------------------------
// 14. Reset between functions: same name reused in two functions is
//     independent.
// ---------------------------------------------------------------------------

#[test]
fn test_move_analyzer_resets_between_functions() {
    // fn f() { let s = "a"; use(s); use(s); }
    // fn g() { let s = "b"; use(s); use(s); }
    // Both functions should have the same pattern: first use no clone,
    // second use clone. State from f must not leak into g.
    let mk = |fn_name: &str, lit: &str| {
        func_with(
            fn_name,
            Vec::new(),
            vec![
                let_stmt("s", string_expr(lit)),
                take_stmt(ident_expr("s")),
                take_stmt(ident_expr("s")),
            ],
        )
    };
    let src = generate_rust(&[mk("f", "a"), mk("g", "b")]).unwrap();
    // Each function should have both a non-clone use and a clone use.
    let clone_count = src.matches("take(s.clone())").count();
    let plain_count = src.matches("take(s);").count();
    assert_eq!(
        clone_count, 2,
        "expected 2 clones (one per fn), src = {src}"
    );
    assert_eq!(plain_count, 2, "expected 2 plain uses, src = {src}");
    syn::parse_str::<syn::File>(&src).expect("must re-parse");
}

// ---------------------------------------------------------------------------
// 15. `let mut` and typed annotations are unaffected by move analysis.
// ---------------------------------------------------------------------------

#[test]
fn test_let_mut_typed_with_move() {
    // let mut s: String = "hi"; use(s); use(s);
    let f = func_with(
        "f",
        Vec::new(),
        vec![
            let_stmt_typed_mut("s", named_type("String"), string_expr("hi")),
            take_stmt(ident_expr("s")),
            take_stmt(ident_expr("s")),
        ],
    );
    let src = generate_rust(&[f]).unwrap();
    assert!(src.contains("take(s);"), "src = {src}");
    assert!(src.contains("take(s.clone());"), "src = {src}");
    syn::parse_str::<syn::File>(&src).expect("must re-parse");
}

// ---------------------------------------------------------------------------
// 16. KNOWN LIMITATION: reassignment does not reset the use counter.
//     Documented via #[ignore] — run with --ignored to see current behavior.
// ---------------------------------------------------------------------------

#[test]
#[ignore = "v0.1 limitation: reassignment does not reset the move counter. \
            T33b (v1.0) will address this. Run with: cargo test -- --ignored"]
fn test_reassignment_resets_counter_limitation() {
    // let s = "a"; use(s); s = "b"; use(s);
    // The second `use(s)` after reassignment SHOULD be a fresh move (no
    // clone), but the current analyzer does not reset on assignment, so
    // it will emit a spurious `.clone()`. This test documents that.
    let f = func_with(
        "f",
        Vec::new(),
        vec![
            let_stmt("s", string_expr("a")),
            take_stmt(ident_expr("s")),
            Stmt::Assignment {
                target: ident_expr("s"),
                op: deox_ast::op::BinaryOp::Assign,
                value: string_expr("b"),
                span: span(),
            },
            take_stmt(ident_expr("s")),
        ],
    );
    let src = generate_rust(&[f]).unwrap();
    // Document the current (incorrect) behavior:
    assert!(
        src.contains("take(s.clone());"),
        "v0.1 limitation: reassignment does not reset counter; src = {src}"
    );
}

// ---------------------------------------------------------------------------
// 17. Compile-test (ignored by default — requires rustc on PATH).
//     Verifies that the generated Rust actually compiles with rustc.
// ---------------------------------------------------------------------------

#[test]
#[ignore = "requires rustc on PATH; run with: cargo test -- --ignored \
            test_generated_rust_compiles_with_rustc"]
fn test_generated_rust_compiles_with_rustc() {
    use std::process::Command;

    // Generate a self-contained program that exercises move semantics.
    // Output:
    //   fn main() {
    //       let s = "hi";
    //       let s2 = s;
    //       let s3 = s.clone();
    //   }
    // This compiles because `"hi"` is `&'static str` (Copy AND Clone),
    // so inserting `.clone()` on the second use yields a valid `&'static str`.
    let f = func_with(
        "main",
        Vec::new(),
        vec![
            let_stmt("s", string_expr("hi")),
            let_stmt("s2", ident_expr("s")),
            let_stmt("s3", ident_expr("s")),
        ],
    );
    let src = generate_rust(&[f]).expect("codegen must succeed");
    assert!(
        src.contains("s.clone()"),
        "expected clone to be inserted; src = {src}"
    );

    // Write to a temp file and compile.
    let temp_dir = std::env::temp_dir();
    let rs_path = temp_dir.join("deox_t33a_compile_test.rs");
    let out_path = temp_dir.join("deox_t33a_compile_test.out");
    std::fs::write(&rs_path, &src).expect("write temp .rs");

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
        "rustc failed to compile generated Rust:\n--- stdout ---\n{}\n--- stderr ---\n{}\n",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

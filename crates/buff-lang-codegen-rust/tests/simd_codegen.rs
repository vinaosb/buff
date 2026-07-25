//! T54 integration tests - buff-simd prelude type codegen.
//!
//! Verifies that the Rust codegen lowers the T54 SIMD type:
//!
//! - **Simd** (`Simd.splat(x) -> Simd`, `Simd.from_slice(s) -> Simd`,
//!   `Simd.from_array(arr) -> Simd`, `simd.add(other)`, `simd.sub(other)`,
//!   `simd.mul(other)`, `simd.div(other)`, `simd.sum()`, `simd.min()`,
//!   `simd.max()`, `simd.to_vec()`)
//!
//! Each constructor + instance method wraps the `buff_simd` crate's
//! safe API. The fallible constructor (`Simd.from_slice`) is panic-free
//! via `unwrap_or_default()` (the wrapper type impls Default as
//! `splat(0.0)`). `Simd.splat` / `Simd.from_array` are infallible.
//! Instance methods (`add` / `sub` / `mul` / `div` / `sum` / `min` /
//! `max` / `to_vec`) return `Simd` / `f32` / `Vec<f32>` directly.
//!
//! Run via:
//!
//! ```text
//! cargo test -p buff-lang-codegen-rust --test simd_codegen
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

fn ident_expr(s: &str) -> Expr {
    Expr::Ident(ident(s), span())
}

fn float_expr(v: f64) -> Expr {
    Expr::Literal(Literal::Double(v), span())
}

fn named_type(name: &str) -> TypeRef {
    TypeRef::Named {
        name: ident(name),
        span: span(),
    }
}

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

fn ns_assoc_call(namespace: &str, method: &str, args: Vec<Expr>) -> Expr {
    Expr::MethodCall {
        receiver: Box::new(ident_expr(namespace)),
        method: ident(method),
        args,
        span: span(),
    }
}

fn instance_call(recv: &str, method: &str, args: Vec<Expr>) -> Expr {
    Expr::MethodCall {
        receiver: Box::new(ident_expr(recv)),
        method: ident(method),
        args,
        span: span(),
    }
}

fn codegen_stmts_in(name: &str, stmts: Vec<Stmt>) -> String {
    let func = func_decl(name, &[], stmts);
    generate_rust(&[func]).expect("codegen must succeed")
}

fn codegen_one_expr_in(name: &str, expr: Expr) -> String {
    codegen_stmts_in(name, vec![expr_stmt(expr)])
}

fn must_reparse(src: &str) {
    syn::parse_str::<syn::File>(src)
        .unwrap_or_else(|e| panic!("generated source must re-parse: {e}\n--- src ---\n{src}"));
}

// ===========================================================================
// 1. Simd.splat — one-arg constructor (broadcast scalar to all lanes).
// ===========================================================================

#[test]
fn simd_codegen_splat_with_literal_arg() {
    let src = codegen_one_expr_in("f", ns_assoc_call("Simd", "splat", vec![float_expr(5.0)]));
    assert!(
        src.contains("buff_simd::Simd::splat"),
        "expected `buff_simd::Simd::splat(` in: {src}"
    );
    must_reparse(&src);
}

#[test]
fn simd_codegen_splat_with_ident_arg() {
    let src = codegen_one_expr_in("f", ns_assoc_call("Simd", "splat", vec![ident_expr("x")]));
    assert!(
        src.contains("buff_simd::Simd::splat"),
        "expected `buff_simd::Simd::splat(` in: {src}"
    );
    must_reparse(&src);
}

// ===========================================================================
// 2. Simd.from_slice / Simd.from_array — one-arg fallible+infallible ctors.
// ===========================================================================

#[test]
fn simd_codegen_from_slice_lowers_with_unwrap_or_default() {
    let src = codegen_one_expr_in(
        "f",
        ns_assoc_call("Simd", "from_slice", vec![ident_expr("data")]),
    );
    assert!(
        src.contains("buff_simd::Simd::from_slice"),
        "expected `buff_simd::Simd::from_slice(` in: {src}"
    );
    assert!(
        src.contains(".unwrap_or_default()"),
        "expected `.unwrap_or_default()` (panic-free Simd.from_slice) in: {src}"
    );
    must_reparse(&src);
}

#[test]
fn simd_codegen_from_array_lowers_correctly() {
    let src = codegen_one_expr_in(
        "f",
        ns_assoc_call("Simd", "from_array", vec![ident_expr("arr")]),
    );
    assert!(
        src.contains("buff_simd::Simd::from_slice"),
        "expected `buff_simd::Simd::from_slice(` (from_array lowers via slice path) in: {src}"
    );
    assert!(
        src.contains(".unwrap_or_default()"),
        "expected `.unwrap_or_default()` in: {src}"
    );
    must_reparse(&src);
}

// ===========================================================================
// 3. simd.add / simd.mul — one-arg lane-wise binary instance methods.
// ===========================================================================

#[test]
fn simd_codegen_add_and_mul_lowers_correctly() {
    let src = codegen_stmts_in(
        "f",
        vec![
            let_stmt("a", ns_assoc_call("Simd", "splat", vec![float_expr(1.0)])),
            let_stmt("b", ns_assoc_call("Simd", "splat", vec![float_expr(2.0)])),
            expr_stmt(instance_call("a", "add", vec![ident_expr("b")])),
            expr_stmt(instance_call("a", "mul", vec![ident_expr("b")])),
        ],
    );
    assert!(
        src.contains("buff_simd::Simd::splat"),
        "splat ctor in: {src}"
    );
    assert!(src.contains(".add("), "expected `.add(` in: {src}");
    assert!(src.contains(".mul("), "expected `.mul(` in: {src}");
    must_reparse(&src);
}

// ===========================================================================
// 4. simd.sub / simd.div — remaining lane-wise binary ops.
// ===========================================================================

#[test]
fn simd_codegen_sub_and_div_lowers_correctly() {
    let src = codegen_stmts_in(
        "f",
        vec![
            let_stmt("a", ns_assoc_call("Simd", "splat", vec![float_expr(1.0)])),
            let_stmt("b", ns_assoc_call("Simd", "splat", vec![float_expr(2.0)])),
            expr_stmt(instance_call("a", "sub", vec![ident_expr("b")])),
            expr_stmt(instance_call("a", "div", vec![ident_expr("b")])),
        ],
    );
    assert!(src.contains(".sub("), "expected `.sub(` in: {src}");
    assert!(src.contains(".div("), "expected `.div(` in: {src}");
    must_reparse(&src);
}

// ===========================================================================
// 5. simd.sum / simd.min / simd.max — zero-arg horizontal reductions.
// ===========================================================================

#[test]
fn simd_codegen_sum_min_max_lowers_correctly() {
    let src = codegen_stmts_in(
        "f",
        vec![
            let_stmt("a", ns_assoc_call("Simd", "splat", vec![float_expr(1.0)])),
            expr_stmt(instance_call("a", "sum", vec![])),
            expr_stmt(instance_call("a", "min", vec![])),
            expr_stmt(instance_call("a", "max", vec![])),
        ],
    );
    assert!(src.contains(".sum()"), "expected `.sum()` in: {src}");
    assert!(src.contains(".min()"), "expected `.min()` in: {src}");
    assert!(src.contains(".max()"), "expected `.max()` in: {src}");
    must_reparse(&src);
}

// ===========================================================================
// 6. simd.to_vec — zero-arg extract returning Vector<Float>.
// ===========================================================================

#[test]
fn simd_codegen_to_vec_lowers_correctly() {
    let src = codegen_stmts_in(
        "f",
        vec![
            let_stmt("a", ns_assoc_call("Simd", "splat", vec![float_expr(1.0)])),
            expr_stmt(instance_call("a", "to_vec", vec![])),
        ],
    );
    assert!(src.contains(".to_vec()"), "expected `.to_vec()` in: {src}");
    must_reparse(&src);
}

// ===========================================================================
// 7. extern_crates registration — buff-simd + wide when Simd is used.
// ===========================================================================

#[test]
fn simd_codegen_registers_extern_crates_when_used() {
    let main = func_decl(
        "main",
        &[],
        vec![let_stmt(
            "a",
            ns_assoc_call("Simd", "splat", vec![float_expr(1.0)]),
        )],
    );
    let mut codegen = RustCodegen::new();
    let _ = codegen.generate(&[main]).expect("codegen must succeed");
    let extern_crates = codegen.extern_crates();
    assert!(
        extern_crates.contains("buff-simd"),
        "extern_crates should contain `buff-simd`, got: {:?}",
        extern_crates
    );
    assert!(
        extern_crates.contains("wide"),
        "extern_crates should contain `wide`, got: {:?}",
        extern_crates
    );
}

#[test]
fn simd_codegen_no_extern_crate_when_unused() {
    let main = func_decl(
        "main",
        &[],
        vec![expr_stmt(Expr::FuncCall {
            callee: Box::new(ident_expr("print")),
            args: vec![ident_expr("hi")],
            span: span(),
        })],
    );
    let mut codegen = RustCodegen::new();
    let _ = codegen.generate(&[main]).expect("codegen must succeed");
    let extern_crates = codegen.extern_crates();
    assert!(
        !extern_crates.contains("buff-simd"),
        "extern_crates should NOT contain `buff-simd` when Simd is unused, got: {:?}",
        extern_crates
    );
    assert!(
        !extern_crates.contains("wide"),
        "extern_crates should NOT contain `wide` when Simd is unused, got: {:?}",
        extern_crates
    );
}

// ===========================================================================
// 8. Full program snapshot — pins the end-to-end codegen shape.
// ===========================================================================

#[test]
fn simd_codegen_full_program_snapshot() {
    let main = func_decl(
        "main",
        &[],
        vec![
            let_stmt(
                "a",
                ns_assoc_call("Simd", "from_array", vec![ident_expr("arr")]),
            ),
            let_stmt("b", ns_assoc_call("Simd", "splat", vec![float_expr(2.0)])),
            let_stmt("prod", instance_call("a", "mul", vec![ident_expr("b")])),
            expr_stmt(instance_call("prod", "sum", vec![])),
            expr_stmt(instance_call("prod", "min", vec![])),
            expr_stmt(instance_call("prod", "max", vec![])),
            expr_stmt(instance_call("prod", "to_vec", vec![])),
        ],
    );
    let mut codegen = RustCodegen::new();
    let file = codegen.generate(&[main]).expect("codegen must succeed");
    let src = buff_lang_codegen_rust::format_file(&file);
    insta::assert_snapshot!(src);
}

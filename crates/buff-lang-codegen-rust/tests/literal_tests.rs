//! T12 integration tests — type-annotated `let` bindings and literal codegen.
//!
//! Every `let` binding emitted by `buff-lang-codegen-rust` now carries an
//! explicit Rust type annotation. When the Buff source provides one
//! (`let x: Int = …`), it is used directly; otherwise the integrated
//! [`TypeInferencer`] infers the type from the initializer. These tests
//! exercise each primitive literal kind, decimal-type annotation,
//! arithmetic precedence, and compound assignment.

use buff_lang_ast::common::{Block, Ident, Param};
use buff_lang_ast::decl::FuncDecl;
use buff_lang_ast::op::BinaryOp;
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

fn float_expr(f: f32) -> Expr {
    Expr::Literal(Literal::Float(f), span())
}

fn double_expr(d: f64) -> Expr {
    Expr::Literal(Literal::Double(d), span())
}

fn bool_expr(b: bool) -> Expr {
    Expr::Literal(Literal::Bool(b), span())
}

fn string_expr(s: &str) -> Expr {
    Expr::Literal(Literal::String(s.to_string()), span())
}

fn byte_expr(b: u8) -> Expr {
    Expr::Literal(Literal::Byte(b), span())
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

fn let_stmt(name: &str, value: Expr) -> Stmt {
    Stmt::LetDecl {
        name: ident(name),
        value,
        mutable: false,
        ty: None,
        span: span(),
    }
}

fn let_stmt_mut_typed(name: &str, ty: TypeRef, value: Expr) -> Stmt {
    Stmt::LetDecl {
        name: ident(name),
        value,
        mutable: true,
        ty: Some(ty),
        span: span(),
    }
}

fn func_with_stmts(name: &str, stmts: Vec<Stmt>) -> Decl {
    Decl::FuncDecl(FuncDecl {
        name: ident(name),
        params: Vec::new(),
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

fn binary(op: BinaryOp, lhs: Expr, rhs: Expr) -> Expr {
    Expr::BinaryOp {
        op,
        lhs: Box::new(lhs),
        rhs: Box::new(rhs),
        span: span(),
    }
}

// ---------------------------------------------------------------------------
// 1. `let x = 42` — inferred Int → `let x: i64 = 42;`
// ---------------------------------------------------------------------------

#[test]
fn test_codegen_let_int() {
    let f = func_with_stmts("f", vec![let_stmt("x", int_expr(42))]);
    let src = generate_rust(&[f]).unwrap();
    assert!(src.contains("let x: i64 = 42"), "src = {src}");
    syn::parse_str::<syn::File>(&src).expect("must re-parse");
}

// ---------------------------------------------------------------------------
// 2. `let pi = 3.14` — inferred Float → `let pi: f32 = …`
// ---------------------------------------------------------------------------

#[test]
fn test_codegen_let_float() {
    let f = func_with_stmts("f", vec![let_stmt("pi", float_expr(2.5))]);
    let src = generate_rust(&[f]).unwrap();
    assert!(src.contains("let pi: f32 = "), "src = {src}");
    assert!(src.contains("f32"), "src = {src}");
    syn::parse_str::<syn::File>(&src).expect("must re-parse");
}

// ---------------------------------------------------------------------------
// 3. `let z = 99.9d` — inferred Double → `let z: f64 = …`
// ---------------------------------------------------------------------------

#[test]
fn test_codegen_let_double() {
    let f = func_with_stmts("f", vec![let_stmt("z", double_expr(99.9))]);
    let src = generate_rust(&[f]).unwrap();
    assert!(src.contains("let z: f64 = "), "src = {src}");
    assert!(src.contains("f64"), "src = {src}");
    syn::parse_str::<syn::File>(&src).expect("must re-parse");
}

// ---------------------------------------------------------------------------
// 4. `let b = true` — inferred Bool → `let b: bool = true;`
// ---------------------------------------------------------------------------

#[test]
fn test_codegen_let_bool() {
    let f = func_with_stmts("f", vec![let_stmt("b", bool_expr(true))]);
    let src = generate_rust(&[f]).unwrap();
    assert!(src.contains("let b: bool = true;"), "src = {src}");
    syn::parse_str::<syn::File>(&src).expect("must re-parse");
}

// ---------------------------------------------------------------------------
// 5. `let s = "hi"` — inferred String → `let s: String = "hi";`
// ---------------------------------------------------------------------------

#[test]
fn test_codegen_let_string() {
    let f = func_with_stmts("f", vec![let_stmt("s", string_expr("hi"))]);
    let src = generate_rust(&[f]).unwrap();
    assert!(src.contains(r#"let s: String = "hi";"#), "src = {src}");
    syn::parse_str::<syn::File>(&src).expect("must re-parse");
}

// ---------------------------------------------------------------------------
// 6. `let b = 0xFF` — inferred Byte (Bits<8>) → `let b: u8 = 255;`
// ---------------------------------------------------------------------------

#[test]
fn test_codegen_let_byte() {
    let f = func_with_stmts("f", vec![let_stmt("b", byte_expr(0xFF))]);
    let src = generate_rust(&[f]).unwrap();
    assert!(src.contains("let b: u8 = "), "src = {src}");
    syn::parse_str::<syn::File>(&src).expect("must re-parse");
}

// ---------------------------------------------------------------------------
// 7. Arithmetic precedence: `2 + 3 * 4` should preserve grouping.
// ---------------------------------------------------------------------------

#[test]
fn test_codegen_arithmetic_precedence() {
    // 2 + (3 * 4) — Rust prints as `2 + 3 * 4` (precedence respected).
    let expr = binary(
        BinaryOp::Add,
        int_expr(2),
        binary(BinaryOp::Mul, int_expr(3), int_expr(4)),
    );
    let f = func_with_stmts("f", vec![Stmt::ExprStmt(expr, span())]);
    let src = generate_rust(&[f]).unwrap();
    // prettyplease preserves precedence without redundant parens for `*`
    // over `+`. We accept either `2 + 3 * 4` or `2 + (3 * 4)` — both are
    // valid Rust and both reflect the original AST shape.
    assert!(
        src.contains("2 + 3 * 4") || src.contains("2 + (3 * 4)"),
        "expected arithmetic precedence preserved, src = {src}"
    );
    syn::parse_str::<syn::File>(&src).expect("must re-parse");
}

// ---------------------------------------------------------------------------
// 8. Compound assignment: `x += 1` → `x += 1;`
// ---------------------------------------------------------------------------

#[test]
fn test_codegen_compound_assignment() {
    let f = func_with_stmts(
        "f",
        vec![
            let_stmt_mut_typed("x", named_type("Int"), int_expr(0)),
            Stmt::Assignment {
                target: ident_expr("x"),
                op: BinaryOp::AddAssign,
                value: int_expr(1),
                span: span(),
            },
        ],
    );
    let src = generate_rust(&[f]).unwrap();
    assert!(src.contains("x += 1"), "src = {src}");
    syn::parse_str::<syn::File>(&src).expect("must re-parse");
}

// ---------------------------------------------------------------------------
// 9. Explicit Decimal annotation: `let z: Decimal = ...` →
//    `let z: rust_decimal::Decimal = ...`. (Buff v0.1 has no Decimal
//    literal syntax, so we use an Int initializer — type-check is
//    deferred. The point of this test is the ANNOTATION mapping.)
// ---------------------------------------------------------------------------

#[test]
fn test_codegen_decimal_annotation() {
    let f = func_with_stmts(
        "f",
        vec![let_stmt_mut_typed(
            "z",
            named_type("Decimal"),
            // v0.1 has no decimal literal; use Int as a placeholder initializer
            // so the AST is well-formed. We only check the annotation.
            int_expr(0),
        )],
    );
    let src = generate_rust(&[f]).unwrap();
    assert!(
        src.contains("let mut z: rust_decimal::Decimal = "),
        "src = {src}"
    );
    syn::parse_str::<syn::File>(&src).expect("must re-parse");
}

// ---------------------------------------------------------------------------
// 10. `let mut x: Int = 0` — explicit annotation + mutability preserved.
// ---------------------------------------------------------------------------

#[test]
fn test_codegen_let_mut_explicit_type() {
    let f = func_with_stmts(
        "f",
        vec![let_stmt_mut_typed("x", named_type("Int"), int_expr(0))],
    );
    let src = generate_rust(&[f]).unwrap();
    assert!(src.contains("let mut x: i64 = 0"), "src = {src}");
    syn::parse_str::<syn::File>(&src).expect("must re-parse");
}

// ---------------------------------------------------------------------------
// 11. All primitive literal kinds in one function — snapshot.
// ---------------------------------------------------------------------------

#[test]
fn test_codegen_all_literals_snapshot() {
    let f = func_with_stmts(
        "literals",
        vec![
            let_stmt("i", int_expr(42)),
            let_stmt("f", float_expr(2.5)),
            let_stmt("d", double_expr(9.9)),
            let_stmt("b", bool_expr(true)),
            let_stmt("s", string_expr("hi")),
            let_stmt("byte", byte_expr(0xFF)),
        ],
    );
    let src = generate_rust(&[f]).unwrap();
    insta::assert_snapshot!(src, @"
fn literals() {
    let i: i64 = 42;
    let f: f32 = 2.5f32;
    let d: f64 = 9.9f64;
    let b: bool = true;
    let s: String = \"hi\";
    let byte: u8 = 255;
}
");
}

// ---------------------------------------------------------------------------
// 12. Type propagation through let-chain: `let x = 42; let y = x;`
//     — y should also be annotated `i64`.
// ---------------------------------------------------------------------------

#[test]
fn test_codegen_type_propagation_through_let_chain() {
    let f = func_with_stmts(
        "f",
        vec![let_stmt("x", int_expr(42)), let_stmt("y", ident_expr("x"))],
    );
    let src = generate_rust(&[f]).unwrap();
    assert!(src.contains("let x: i64 = 42"), "src = {src}");
    assert!(src.contains("let y: i64 = x"), "src = {src}");
    syn::parse_str::<syn::File>(&src).expect("must re-parse");
}

// ---------------------------------------------------------------------------
// 13. Param-typed let: `func f(n: Int) { let y = n; }` — y is i64
// ---------------------------------------------------------------------------

#[test]
fn test_codegen_param_type_propagates_to_let() {
    let f = Decl::FuncDecl(FuncDecl {
        name: ident("f"),
        params: vec![Param {
            name: ident("n"),
            ty: named_type("Int"),
            span: span(),
        }],
        return_type: None,
        body: Block {
            stmts: vec![let_stmt("y", ident_expr("n"))],
            span: span(),
        },
        is_async: false,
        is_unsafe: false,
        is_extern: false,
        span: span(),
    });
    let src = generate_rust(&[f]).unwrap();
    // y should get the param's type (i64) as inferred annotation.
    assert!(src.contains("let y: i64 = n"), "src = {src}");
    syn::parse_str::<syn::File>(&src).expect("must re-parse");
}

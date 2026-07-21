//! Integration tests for `buff-lang-codegen-wgsl`.
//!
//! These tests prove the contract documented in T44's QA spec:
//!
//! 1. **QA case** — `{ x => x * 2.0 }` lowers to a shader containing
//!    `@compute @workgroup_size(64)` and the body `output[i] = x * 2.0;`.
//! 2. **f64 / Double rejection** — any `Literal::Double(...)` in the body
//!    produces `Err(WgslError::UnsupportedType { ... })` mentioning
//!    `Float<64>` / `f64`.
//! 3. **Type filtering** — `Double`/`Float<64>` param annotation is rejected.
//! 4. **Arithmetic ops** — `+ - * / %` all lower correctly.
//! 5. **Nested arithmetic precedence** — parens inserted for correctness.
//! 6. **Storage buffer bindings** — `@group(0) @binding(0/1)` present.
//! 7. **Global-invocation-id guard** — `if (i >= arrayLength(&input))` present.
//! 8. **Full-shader insta snapshot** — byte-stable across invocations.
//!
//! All test names contain `wgsl_codegen` so `cargo test wgsl_codegen` matches
//! the full suite.

#![cfg(test)]

use buff_lang_ast::common::{Block, Ident, Param};
use buff_lang_ast::expr::Expr;
use buff_lang_ast::op::{BinaryOp, UnaryOp};
use buff_lang_ast::stmt::Stmt;
use buff_lang_ast::ty::TypeRef;
use buff_lang_ast::Literal;
use buff_lang_codegen_wgsl::{
    filter_buff_type_name, filter_literal, generate_wgsl, generate_wgsl_with_options,
    lower::lower_expr, render_shader, resolve_param_type, WgslCodegen, WgslError, WgslOptions,
    WgslScalarType,
};
use buff_lang_error::Span;

// ---------------------------------------------------------------------------
// Test helpers
// ---------------------------------------------------------------------------

fn span() -> Span {
    Span::dummy()
}

fn ident(name: &str) -> Ident {
    Ident::new(name, span())
}

fn ident_expr(name: &str) -> Expr {
    Expr::Ident(ident(name), span())
}

fn float_lit(v: f32) -> Expr {
    Expr::Literal(Literal::Float(v), span())
}

fn int_lit(v: i64) -> Expr {
    Expr::Literal(Literal::Int(v), span())
}

fn double_lit(v: f64) -> Expr {
    Expr::Literal(Literal::Double(v), span())
}

fn bool_lit(v: bool) -> Expr {
    Expr::Literal(Literal::Bool(v), span())
}

fn binop(op: BinaryOp, lhs: Expr, rhs: Expr) -> Expr {
    Expr::BinaryOp {
        op,
        lhs: Box::new(lhs),
        rhs: Box::new(rhs),
        span: span(),
    }
}

fn unary(op: UnaryOp, operand: Expr) -> Expr {
    Expr::UnaryOp {
        op,
        operand: Box::new(operand),
        span: span(),
    }
}

fn x_param(ty: Option<&str>) -> Param {
    let ty_name = ty.unwrap_or("Float");
    Param {
        name: ident("x"),
        ty: TypeRef::Named {
            name: ident(ty_name),
            span: span(),
        },
        default_value: None,
        is_comptime: false,
        span: span(),
    }
}

fn lambda(param: Param, body_expr: Expr) -> Expr {
    Expr::Lambda {
        params: vec![param],
        body: Block {
            stmts: vec![Stmt::ExprStmt(body_expr, span())],
            span: span(),
        },
        return_type: None,
        span: span(),
    }
}

// ---------------------------------------------------------------------------
// 1. QA case — `{x => x * 2.0}` → contains `@compute @workgroup_size(64)`
// ---------------------------------------------------------------------------

#[test]
fn wgsl_codegen_qa_case_x_times_two() {
    // The single most important test — the T44 QA gate.
    let lambda = lambda(
        x_param(None),
        binop(BinaryOp::Mul, ident_expr("x"), float_lit(2.0)),
    );
    let src = generate_wgsl(&lambda).expect("QA case must succeed");

    // Workgroup + entry point.
    assert!(
        src.contains("@compute @workgroup_size(64)"),
        "missing @compute @workgroup_size(64) in:\n{src}"
    );

    // Storage buffer bindings.
    assert!(
        src.contains("@group(0) @binding(0) var<storage, read> input: array<f32>;"),
        "missing input binding in:\n{src}"
    );
    assert!(
        src.contains("@group(0) @binding(1) var<storage, read_write> output: array<f32>;"),
        "missing output binding in:\n{src}"
    );

    // Bounds-check guard.
    assert!(
        src.contains("if (i >= arrayLength(&input))"),
        "missing bounds-check guard in:\n{src}"
    );

    // Param binding.
    assert!(
        src.contains("let x = input[i];"),
        "missing `let x = input[i];` param binding in:\n{src}"
    );

    // Body assignment.
    assert!(
        src.contains("output[i] = x * 2.0;"),
        "missing `output[i] = x * 2.0;` body assignment in:\n{src}"
    );
}

// ---------------------------------------------------------------------------
// 2. Full-shader insta snapshot — locks the byte-stable output.
// ---------------------------------------------------------------------------

#[test]
fn wgsl_codegen_full_shader_snapshot() {
    let lambda = lambda(
        x_param(None),
        binop(BinaryOp::Mul, ident_expr("x"), float_lit(2.0)),
    );
    let src = generate_wgsl(&lambda).expect("snapshot must succeed");
    insta::assert_snapshot!("wgsl_full_shader_x_times_two", src);
}

#[test]
fn wgsl_codegen_nested_arithmetic_snapshot() {
    // {x => (x + 1) * (x - 2)}
    let body = binop(
        BinaryOp::Mul,
        binop(BinaryOp::Add, ident_expr("x"), float_lit(1.0)),
        binop(BinaryOp::Sub, ident_expr("x"), float_lit(2.0)),
    );
    let lambda = lambda(x_param(None), body);
    let src = generate_wgsl(&lambda).expect("nested arithmetic snapshot must succeed");
    insta::assert_snapshot!("wgsl_nested_arithmetic", src);
}

// ---------------------------------------------------------------------------
// 3. f64 / Double rejection (RED spec).
// ---------------------------------------------------------------------------

#[test]
fn wgsl_codegen_rejects_f64_literal_in_body() {
    // {x => x * 2.0d}  (2.0d is a Buff Double literal)
    let lambda = lambda(
        x_param(None),
        binop(BinaryOp::Mul, ident_expr("x"), double_lit(2.0)),
    );
    let err = generate_wgsl(&lambda).expect_err("f64 must be rejected");
    assert!(
        matches!(err, WgslError::UnsupportedType { .. }),
        "expected UnsupportedType, got {err:?}"
    );
    let msg = err.to_string();
    assert!(
        msg.contains("Float<64>"),
        "error must name Float<64>: {msg}"
    );
    assert!(msg.contains("f64"), "error must mention f64: {msg}");
}

#[test]
fn wgsl_codegen_rejects_double_param_annotation() {
    // {x: Double => x * 2.0}  — the type itself is non-WGSL.
    let lambda = lambda(
        x_param(Some("Double")),
        binop(BinaryOp::Mul, ident_expr("x"), float_lit(2.0)),
    );
    let err = generate_wgsl(&lambda).expect_err("Double param must be rejected");
    assert!(matches!(err, WgslError::UnsupportedType { .. }));
    assert!(err.to_string().contains("Float<64>"));
}

#[test]
fn wgsl_codegen_rejects_decimal_param_annotation() {
    let lambda = lambda(x_param(Some("Decimal")), ident_expr("x"));
    let err = generate_wgsl(&lambda).expect_err("Decimal param must be rejected");
    assert!(matches!(err, WgslError::UnsupportedType { .. }));
    assert!(err.to_string().contains("Decimal"));
}

#[test]
fn wgsl_codegen_filter_literal_rejects_f64() {
    let err = filter_literal(&Literal::Double(1.0)).expect_err("Double literal rejected");
    assert!(matches!(err, WgslError::UnsupportedType { .. }));
}

#[test]
fn wgsl_codegen_filter_buff_type_name_rejects_int64() {
    let err = filter_buff_type_name("Int<64>").expect_err("Int<64> rejected");
    assert!(matches!(err, WgslError::UnsupportedType { .. }));
    assert!(err.to_string().contains("Int<64>"));
}

// ---------------------------------------------------------------------------
// 4. Arithmetic operators — `+ - * / %`.
// ---------------------------------------------------------------------------

#[test]
fn wgsl_codegen_addition() {
    let lambda = lambda(
        x_param(None),
        binop(BinaryOp::Add, ident_expr("x"), float_lit(1.0)),
    );
    let src = generate_wgsl(&lambda).unwrap();
    assert!(src.contains("output[i] = x + 1.0;"));
}

#[test]
fn wgsl_codegen_subtraction() {
    let lambda = lambda(
        x_param(None),
        binop(BinaryOp::Sub, ident_expr("x"), float_lit(1.0)),
    );
    let src = generate_wgsl(&lambda).unwrap();
    assert!(src.contains("output[i] = x - 1.0;"));
}

#[test]
fn wgsl_codegen_multiplication() {
    let lambda = lambda(
        x_param(None),
        binop(BinaryOp::Mul, ident_expr("x"), float_lit(3.0)),
    );
    let src = generate_wgsl(&lambda).unwrap();
    assert!(src.contains("output[i] = x * 3.0;"));
}

#[test]
fn wgsl_codegen_division() {
    let lambda = lambda(
        x_param(None),
        binop(BinaryOp::Div, ident_expr("x"), float_lit(2.0)),
    );
    let src = generate_wgsl(&lambda).unwrap();
    assert!(src.contains("output[i] = x / 2.0;"));
}

#[test]
fn wgsl_codegen_modulo() {
    let lambda = lambda(
        x_param(None),
        binop(BinaryOp::Mod, ident_expr("x"), float_lit(2.0)),
    );
    let src = generate_wgsl(&lambda).unwrap();
    assert!(src.contains("output[i] = x % 2.0;"));
}

// ---------------------------------------------------------------------------
// 5. Nested arithmetic precedence — parens inserted for correctness.
// ---------------------------------------------------------------------------

#[test]
fn wgsl_codegen_nested_arithmetic_parens() {
    // {x => (x + 1) * 2} — nested BinaryOp on LHS must be parenthesized.
    let body = binop(
        BinaryOp::Mul,
        binop(BinaryOp::Add, ident_expr("x"), float_lit(1.0)),
        float_lit(2.0),
    );
    let lambda = lambda(x_param(None), body);
    let src = generate_wgsl(&lambda).unwrap();
    assert!(
        src.contains("output[i] = (x + 1.0) * 2.0;"),
        "nested arithmetic must be parenthesized in:\n{src}"
    );
}

#[test]
fn wgsl_codegen_deeply_nested_arithmetic() {
    // {x => ((x + 1) * 2) - 3}
    let inner = binop(
        BinaryOp::Mul,
        binop(BinaryOp::Add, ident_expr("x"), float_lit(1.0)),
        float_lit(2.0),
    );
    let body = binop(BinaryOp::Sub, inner, float_lit(3.0));
    let lambda = lambda(x_param(None), body);
    let src = generate_wgsl(&lambda).unwrap();
    assert!(
        src.contains("output[i] = ((x + 1.0) * 2.0) - 3.0;"),
        "deeply nested arithmetic must be parenthesized in:\n{src}"
    );
}

// ---------------------------------------------------------------------------
// 6. Storage buffer bindings present.
// ---------------------------------------------------------------------------

#[test]
fn wgsl_codegen_default_bindings_present() {
    let lambda = lambda(x_param(None), ident_expr("x"));
    let src = generate_wgsl(&lambda).unwrap();
    assert!(src.contains("@group(0) @binding(0) var<storage, read> input: array<f32>;"));
    assert!(src.contains("@group(0) @binding(1) var<storage, read_write> output: array<f32>;"));
}

#[test]
fn wgsl_codegen_custom_bindings_respected() {
    let opts = WgslOptions {
        group: 2,
        binding_input: 5,
        binding_output: 7,
        ..WgslOptions::default()
    };
    let lambda = lambda(x_param(None), ident_expr("x"));
    let src = generate_wgsl_with_options(&lambda, &opts).unwrap();
    assert!(src.contains("@group(2) @binding(5) var<storage, read> input: array<f32>;"));
    assert!(src.contains("@group(2) @binding(7) var<storage, read_write> output: array<f32>;"));
}

#[test]
fn wgsl_codegen_int32_element_type() {
    let opts = WgslOptions {
        element_type: WgslScalarType::I32,
        ..WgslOptions::default()
    };
    let lambda = lambda(
        x_param(None),
        binop(BinaryOp::Add, ident_expr("x"), int_lit(1)),
    );
    let src = generate_wgsl_with_options(&lambda, &opts).unwrap();
    assert!(src.contains("var<storage, read> input: array<i32>;"));
    assert!(src.contains("var<storage, read_write> output: array<i32>;"));
    assert!(src.contains("output[i] = x + 1;"));
}

// ---------------------------------------------------------------------------
// 7. Global-invocation-id guard present.
// ---------------------------------------------------------------------------

#[test]
fn wgsl_codegen_global_invocation_id_guard_present() {
    let lambda = lambda(x_param(None), ident_expr("x"));
    let src = generate_wgsl(&lambda).unwrap();
    assert!(src.contains("@builtin(global_invocation_id) gid: vec3<u32>"));
    assert!(src.contains("let i = gid.x;"));
    assert!(src.contains("if (i >= arrayLength(&input))"));
    assert!(src.contains("return;"));
}

// ---------------------------------------------------------------------------
// 8. Workgroup size variations.
// ---------------------------------------------------------------------------

#[test]
fn wgsl_codegen_custom_workgroup_size() {
    let opts = WgslOptions {
        workgroup_size: 256,
        ..WgslOptions::default()
    };
    let lambda = lambda(x_param(None), ident_expr("x"));
    let src = generate_wgsl_with_options(&lambda, &opts).unwrap();
    assert!(src.contains("@compute @workgroup_size(256)"));
}

// ---------------------------------------------------------------------------
// 9. Determinism — byte-identical output for the same input.
// ---------------------------------------------------------------------------

#[test]
fn wgsl_codegen_deterministic_byte_identical() {
    let lambda = lambda(
        x_param(None),
        binop(BinaryOp::Mul, ident_expr("x"), float_lit(2.0)),
    );
    let a = generate_wgsl(&lambda).unwrap();
    let b = generate_wgsl(&lambda).unwrap();
    let c = generate_wgsl(&lambda).unwrap();
    assert_eq!(a, b, "first != second");
    assert_eq!(b, c, "second != third");
}

// ---------------------------------------------------------------------------
// 10. Entry API: WgslCodegen struct round-trips.
// ---------------------------------------------------------------------------

#[test]
fn wgsl_codegen_struct_equivalent_to_function() {
    let lambda = lambda(
        x_param(None),
        binop(BinaryOp::Mul, ident_expr("x"), float_lit(2.0)),
    );
    let from_fn = generate_wgsl(&lambda).unwrap();
    let from_struct = WgslCodegen::default().generate(&lambda).unwrap();
    assert_eq!(from_fn, from_struct);
}

// ---------------------------------------------------------------------------
// 11. Unary operators.
// ---------------------------------------------------------------------------

#[test]
fn wgsl_codegen_unary_negation() {
    let lambda = lambda(x_param(None), unary(UnaryOp::Neg, ident_expr("x")));
    let src = generate_wgsl(&lambda).unwrap();
    assert!(src.contains("output[i] = -x;"));
}

#[test]
fn wgsl_codegen_unary_neg_of_binop() {
    let lambda = lambda(
        x_param(None),
        unary(
            UnaryOp::Neg,
            binop(BinaryOp::Add, ident_expr("x"), float_lit(1.0)),
        ),
    );
    let src = generate_wgsl(&lambda).unwrap();
    assert!(src.contains("output[i] = -(x + 1.0);"));
}

// ---------------------------------------------------------------------------
// 12. Rejection paths — non-lambda, wrong arity, multi-statement body.
// ---------------------------------------------------------------------------

#[test]
fn wgsl_codegen_rejects_non_lambda() {
    let not_a_lambda = binop(BinaryOp::Add, int_lit(1), int_lit(2));
    let err = generate_wgsl(&not_a_lambda).expect_err("non-lambda must be rejected");
    assert!(matches!(err, WgslError::NotMapLambda { .. }));
    assert!(err.to_string().contains("binary op"));
}

#[test]
fn wgsl_codegen_rejects_zero_params() {
    let lambda = Expr::Lambda {
        params: vec![],
        body: Block {
            stmts: vec![Stmt::ExprStmt(ident_expr("x"), span())],
            span: span(),
        },
        return_type: None,
        span: span(),
    };
    let err = generate_wgsl(&lambda).expect_err("zero-param lambda must be rejected");
    assert!(matches!(err, WgslError::NotMapLambda { .. }));
}

#[test]
fn wgsl_codegen_rejects_two_params() {
    let lambda = Expr::Lambda {
        params: vec![x_param(None), x_param(None)],
        body: Block {
            stmts: vec![Stmt::ExprStmt(ident_expr("x"), span())],
            span: span(),
        },
        return_type: None,
        span: span(),
    };
    let err = generate_wgsl(&lambda).expect_err("two-param lambda must be rejected");
    assert!(matches!(err, WgslError::NotMapLambda { .. }));
}

#[test]
fn wgsl_codegen_rejects_empty_body() {
    let lambda = Expr::Lambda {
        params: vec![x_param(None)],
        body: Block {
            stmts: vec![],
            span: span(),
        },
        return_type: None,
        span: span(),
    };
    let err = generate_wgsl(&lambda).expect_err("empty body must be rejected");
    assert!(matches!(err, WgslError::InvalidLambdaBody { count: 0, .. }));
}

#[test]
fn wgsl_codegen_rejects_multi_statement_body() {
    let body = Block {
        stmts: vec![
            Stmt::ExprStmt(float_lit(1.0), span()),
            Stmt::ExprStmt(float_lit(2.0), span()),
            Stmt::ExprStmt(float_lit(3.0), span()),
        ],
        span: span(),
    };
    let lambda = Expr::Lambda {
        params: vec![x_param(None)],
        body,
        return_type: None,
        span: span(),
    };
    let err = generate_wgsl(&lambda).expect_err("multi-statement body must be rejected");
    assert!(matches!(err, WgslError::InvalidLambdaBody { count: 3, .. }));
}

#[test]
fn wgsl_codegen_rejects_let_decl_body() {
    let body = Block {
        stmts: vec![Stmt::LetDecl {
            name: ident("y"),
            value: float_lit(1.0),
            mutable: false,
            ty: None,
            span: span(),
        }],
        span: span(),
    };
    let lambda = Expr::Lambda {
        params: vec![x_param(None)],
        body,
        return_type: None,
        span: span(),
    };
    let err = generate_wgsl(&lambda).expect_err("let-decl body must be rejected");
    assert!(matches!(err, WgslError::InvalidLambdaBody { .. }));
}

// ---------------------------------------------------------------------------
// 13. Free-variable rejection.
// ---------------------------------------------------------------------------

#[test]
fn wgsl_codegen_rejects_free_variable_in_body() {
    // {x => y}  — y is not the param.
    let lambda = lambda(x_param(None), ident_expr("y"));
    let err = generate_wgsl(&lambda).expect_err("free variable must be rejected");
    assert!(matches!(err, WgslError::UnsupportedExpr { .. }));
    assert!(err.to_string().contains("free variable"));
}

// ---------------------------------------------------------------------------
// 14. Function-call rejection.
// ---------------------------------------------------------------------------

#[test]
fn wgsl_codegen_rejects_function_call_in_body() {
    // {x => foo(x)}  — function calls are not pure shader body expressions.
    let call = Expr::FuncCall {
        callee: Box::new(ident_expr("foo")),
        args: vec![ident_expr("x")],
        span: span(),
    };
    let lambda = lambda(x_param(None), call);
    let err = generate_wgsl(&lambda).expect_err("function call must be rejected");
    assert!(matches!(err, WgslError::UnsupportedExpr { .. }));
    assert!(err.to_string().contains("function call"));
}

// ---------------------------------------------------------------------------
// 15. Bool literal lowering (sanity — bool is a valid WGSL scalar).
// ---------------------------------------------------------------------------

#[test]
fn wgsl_codegen_bool_literal_lowered() {
    // {x => true}  — bool lowers to `true`/`false` (u32 in storage).
    let lambda = lambda(x_param(None), bool_lit(true));
    let src = generate_wgsl(&lambda).unwrap();
    assert!(src.contains("output[i] = true;"));
}

// ---------------------------------------------------------------------------
// 16. WgslOptions validation.
// ---------------------------------------------------------------------------

#[test]
fn wgsl_codegen_options_validate_rejects_zero_workgroup_size() {
    let opts = WgslOptions {
        workgroup_size: 0,
        ..WgslOptions::default()
    };
    assert!(opts.validate().is_err());
}

#[test]
fn wgsl_codegen_options_validate_rejects_aliased_bindings() {
    let opts = WgslOptions {
        binding_input: 3,
        binding_output: 3,
        ..WgslOptions::default()
    };
    assert!(opts.validate().is_err());
}

// ---------------------------------------------------------------------------
// 17. Direct access — render_shader + lower_expr smoke.
// ---------------------------------------------------------------------------

#[test]
fn wgsl_codegen_render_shader_smoke() {
    let src = render_shader(&WgslOptions::default(), "x", "x * 2.0").unwrap();
    assert!(src.contains("@compute @workgroup_size(64)"));
    assert!(src.contains("output[i] = x * 2.0;"));
}

#[test]
fn wgsl_codegen_lower_expr_smoke() {
    let body = binop(BinaryOp::Mul, ident_expr("x"), float_lit(2.0));
    assert_eq!(lower_expr(&body, "x").unwrap(), "x * 2.0");
}

// ---------------------------------------------------------------------------
// 18. resolve_param_type — type filtering on AST TypeRef.
// ---------------------------------------------------------------------------

#[test]
fn wgsl_codegen_resolve_param_type_defaults_to_f32() {
    assert_eq!(
        resolve_param_type(None).unwrap(),
        WgslScalarType::F32,
        "unannotated param defaults to f32"
    );
}

#[test]
fn wgsl_codegen_resolve_param_type_float_annotation_is_f32() {
    let ty = TypeRef::Named {
        name: ident("Float"),
        span: span(),
    };
    assert_eq!(resolve_param_type(Some(&ty)).unwrap(), WgslScalarType::F32);
}

#[test]
fn wgsl_codegen_resolve_param_type_int32_annotation_is_i32() {
    let ty = TypeRef::Named {
        name: ident("Int<32>"),
        span: span(),
    };
    assert_eq!(resolve_param_type(Some(&ty)).unwrap(), WgslScalarType::I32);
}

#[test]
fn wgsl_codegen_resolve_param_type_double_annotation_rejected() {
    let ty = TypeRef::Named {
        name: ident("Double"),
        span: span(),
    };
    let err = resolve_param_type(Some(&ty)).expect_err("Double must be rejected");
    assert!(matches!(err, WgslError::UnsupportedType { .. }));
    assert!(err.to_string().contains("Float<64>"));
}

// ---------------------------------------------------------------------------
// 19. WgslScalarType helpers.
// ---------------------------------------------------------------------------

#[test]
fn wgsl_codegen_scalar_type_array_string() {
    assert_eq!(WgslScalarType::F32.as_wgsl_array(), "array<f32>");
    assert_eq!(WgslScalarType::I32.as_wgsl_array(), "array<i32>");
    assert_eq!(WgslScalarType::U32.as_wgsl_array(), "array<u32>");
    assert_eq!(WgslScalarType::F16.as_wgsl_array(), "array<f16>");
}

// ---------------------------------------------------------------------------
// 20. Error display includes both `Float<64>` and `f64` per RED spec.
// ---------------------------------------------------------------------------

#[test]
fn wgsl_codegen_f64_error_message_complete() {
    let err = WgslError::f64_rejected();
    let msg = err.to_string();
    assert!(
        msg.contains("Float<64>"),
        "msg must contain Float<64>: {msg}"
    );
    assert!(msg.contains("f64"), "msg must mention f64: {msg}");
    assert!(msg.contains("WGSL"), "msg must mention WGSL: {msg}");
}

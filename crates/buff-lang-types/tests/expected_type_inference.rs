//! T77 — Expected-type driven inference.
//!
//! When a `.map()` / `.filter()` call's receiver is a `Vector<T>`, the
//! element type `T` is propagated as the EXPECTED type of the lambda's
//! single parameter. This lets `{ x => x * 2 }` infer `x` correctly
//! from the receiver's element type WITHOUT an explicit annotation.
//!
//! Acceptance (from the T77 contract):
//! - `Vector<Float>.map({ x => x * 2 })` -> x: Float, result: Vector<Float>.
//! - `Vector<Int>.map({ x => x + 1 })`   -> x: Int,   result: Vector<Int>.

use buff_lang_ast::{op::BinaryOp, Block, Expr, Ident, Literal, Param, Stmt, TypeRef};
use buff_lang_error::Span;
use buff_lang_types::{Type, TypeInferencer};

// ---------------------------------------------------------------------------
// Test helpers (mirror the real AST construction used elsewhere in this crate)
// ---------------------------------------------------------------------------

fn sp() -> Span {
    Span::dummy()
}

fn ident_expr(name: &str) -> Expr {
    Expr::Ident(Ident::new(name, sp()), sp())
}

fn int_lit(n: i64) -> Expr {
    Expr::Literal(Literal::Int(n), sp())
}

/// The placeholder TypeRef the parser fills for an un-annotated lambda param.
/// Inference is supposed to replace it with a real type (T77 closes that gap
/// for `.map()` / `.filter()` receivers).
fn placeholder_tyref() -> TypeRef {
    TypeRef::Named {
        name: Ident::new("_", sp()),
        span: sp(),
    }
}

/// Build a single-param lambda `{ param => body_expr }` with a placeholder
/// param type (the realistic parser-produced shape for an un-annotated
/// closure).
fn lambda(param: &str, body_expr: Expr) -> Expr {
    Expr::Lambda {
        params: vec![Param {
            name: Ident::new(param, sp()),
            ty: placeholder_tyref(),
            default_value: None,
            span: sp(),
        }],
        body: Block {
            stmts: vec![Stmt::ExprStmt(body_expr, sp())],
            span: sp(),
        },
        return_type: None,
        span: sp(),
    }
}

/// Build `receiver.method(args...)`.
fn method_call(receiver: Expr, method: &str, args: Vec<Expr>) -> Expr {
    Expr::MethodCall {
        receiver: Box::new(receiver),
        method: Ident::new(method, sp()),
        args,
        span: sp(),
    }
}

fn binary(op: BinaryOp, lhs: Expr, rhs: Expr) -> Expr {
    Expr::BinaryOp {
        op,
        lhs: Box::new(lhs),
        rhs: Box::new(rhs),
        span: sp(),
    }
}

// ---------------------------------------------------------------------------
// T77 acceptance: `.map()` propagates the receiver element type to the
// lambda's single parameter.
// ---------------------------------------------------------------------------

#[test]
fn expected_type_inference_map_float_element() {
    // `items: Vector<Float>` (Float<32>).
    // `items.map({ x => x * 2 })` -> x: Float, result: Vector<Float>.
    let mut inf = TypeInferencer::new();
    inf.bind("items", Type::vector(Type::float_default()));

    let body = binary(BinaryOp::Mul, ident_expr("x"), int_lit(2));
    let call = method_call(ident_expr("items"), "map", vec![lambda("x", body)]);

    let result = inf.infer_expr(&call).unwrap();
    // The map result preserves the body's type (Float * Int -> Float).
    assert_eq!(
        result,
        Type::vector(Type::float_default()),
        "Vector<Float>.map({{ x => x * 2 }}) should yield Vector<Float>"
    );
    // T77 contract: the lambda param type is inferred from the receiver's
    // element type.
    assert_eq!(
        inf.lookup("x"),
        Some(&Type::float_default()),
        "lambda param x should be inferred as Float from Vector<Float>.map()"
    );
}

#[test]
fn expected_type_inference_map_int_element() {
    // `items: Vector<Int>` (Int<64>).
    // `items.map({ x => x + 1 })` -> x: Int, result: Vector<Int>.
    let mut inf = TypeInferencer::new();
    inf.bind("items", Type::vector(Type::int_default()));

    let body = binary(BinaryOp::Add, ident_expr("x"), int_lit(1));
    let call = method_call(ident_expr("items"), "map", vec![lambda("x", body)]);

    let result = inf.infer_expr(&call).unwrap();
    assert_eq!(
        result,
        Type::vector(Type::int_default()),
        "Vector<Int>.map({{ x => x + 1 }}) should yield Vector<Int>"
    );
    assert_eq!(
        inf.lookup("x"),
        Some(&Type::int_default()),
        "lambda param x should be inferred as Int from Vector<Int>.map()"
    );
}

// ---------------------------------------------------------------------------
// Supplementary: `.filter()` preserves the element type; map over an
// ArrayLit receiver (the parser-realistic `[1,2,3].map(...)` shape); map
// result feeds a second `.map()` (chaining).
// ---------------------------------------------------------------------------

#[test]
fn expected_type_inference_filter_preserves_element_type() {
    // `items: Vector<Float>`. `.filter({ x => x > 0 })` -> Vector<Float>.
    let mut inf = TypeInferencer::new();
    inf.bind("items", Type::vector(Type::float_default()));

    let body = binary(BinaryOp::Gt, ident_expr("x"), int_lit(0));
    let call = method_call(ident_expr("items"), "filter", vec![lambda("x", body)]);

    let result = inf.infer_expr(&call).unwrap();
    assert_eq!(
        result,
        Type::vector(Type::float_default()),
        "Vector<Float>.filter(...) should preserve the element type"
    );
    assert_eq!(
        inf.lookup("x"),
        Some(&Type::float_default()),
        "filter lambda param should inherit the element type"
    );
}

#[test]
fn expected_type_inference_map_over_array_literal_receiver() {
    // `[1.0, 2.0].map({ x => x * 2 })` — receiver is an ArrayLit of floats,
    // which infers to Vector<Float>. The element type flows into the lambda.
    let mut inf = TypeInferencer::new();

    let receiver = Expr::ArrayLit {
        elements: vec![
            Expr::Literal(Literal::Float(1.0), sp()),
            Expr::Literal(Literal::Float(2.0), sp()),
        ],
        span: sp(),
    };
    let body = binary(BinaryOp::Mul, ident_expr("x"), int_lit(2));
    let call = method_call(receiver, "map", vec![lambda("x", body)]);

    let result = inf.infer_expr(&call).unwrap();
    assert_eq!(
        result,
        Type::vector(Type::float_default()),
        "[Float].map({{ x => x * 2 }}) should yield Vector<Float>"
    );
}

#[test]
fn expected_type_inference_lambda_without_context_stays_unknown() {
    // A bare lambda with NO expected type stays Unknown (no regression of the
    // v0.5 fallback — the closures/codegen path is unaffected).
    let mut inf = TypeInferencer::new();
    let lam = lambda("x", binary(BinaryOp::Mul, ident_expr("x"), int_lit(2)));
    let result = inf.infer_expr(&lam).unwrap();
    assert_eq!(
        result,
        Type::Unknown,
        "bare lambda without context stays Unknown"
    );
}

#[test]
fn expected_type_inference_map_on_non_vector_falls_back_to_unknown() {
    // `.map()` on a non-Vector receiver stays Unknown (no false positive).
    let mut inf = TypeInferencer::new();
    inf.bind("s", Type::string());

    let body = binary(BinaryOp::Mul, ident_expr("x"), int_lit(2));
    let call = method_call(ident_expr("s"), "map", vec![lambda("x", body)]);

    let result = inf.infer_expr(&call).unwrap();
    assert_eq!(
        result,
        Type::Unknown,
        "String.map(...) should stay Unknown (no Vector element type)"
    );
}

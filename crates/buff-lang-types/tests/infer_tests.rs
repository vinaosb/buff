//! Integration tests for `buff-lang-types` — literal inference, promotion,
//! operator typing, identifier lookup, control-flow typing, and error cases.

use buff_lang_ast::{BinaryOp, Block, Expr, Ident, Literal, Stmt, TypeRef, UnaryOp};
use buff_lang_error::{Diagnostic, Severity, Span};
use buff_lang_types::{promote_binary, FloatWidth, IntWidth, Type, TypeInferencer};

// ---------------------------------------------------------------------------
// Test helpers
// ---------------------------------------------------------------------------

fn sp() -> Span {
    Span::dummy()
}

fn int_lit(n: i64) -> Expr {
    Expr::Literal(Literal::Int(n), sp())
}

fn float_lit(n: f32) -> Expr {
    Expr::Literal(Literal::Float(n), sp())
}

fn double_lit(n: f64) -> Expr {
    Expr::Literal(Literal::Double(n), sp())
}

fn bool_lit(b: bool) -> Expr {
    Expr::Literal(Literal::Bool(b), sp())
}

fn str_lit(s: &str) -> Expr {
    Expr::Literal(Literal::String(s.to_string()), sp())
}

fn byte_lit(b: u8) -> Expr {
    Expr::Literal(Literal::Byte(b), sp())
}

fn ident(name: &str) -> Expr {
    Expr::Ident(Ident::new(name, sp()), sp())
}

fn binary(op: BinaryOp, lhs: Expr, rhs: Expr) -> Expr {
    Expr::BinaryOp {
        op,
        lhs: Box::new(lhs),
        rhs: Box::new(rhs),
        span: sp(),
    }
}

fn unary(op: UnaryOp, operand: Expr) -> Expr {
    Expr::UnaryOp {
        op,
        operand: Box::new(operand),
        span: sp(),
    }
}

fn block(stmts: Vec<Stmt>) -> Block {
    Block { stmts, span: sp() }
}

fn expr_stmt(e: Expr) -> Stmt {
    Stmt::ExprStmt(e, sp())
}

fn if_expr(cond: Expr, then_b: Block, else_b: Option<Block>) -> Expr {
    Expr::IfExpr {
        cond: Box::new(cond),
        then_block: then_b,
        else_block: else_b,
        span: sp(),
    }
}

// ---------------------------------------------------------------------------
// Literal inference (tests 1–6)
// ---------------------------------------------------------------------------

#[test]
fn test_infer_int_literal() {
    let mut inf = TypeInferencer::new();
    assert_eq!(
        inf.infer_expr(&int_lit(42)).unwrap(),
        Type::Int {
            width: IntWidth::W64
        }
    );
}

#[test]
fn test_infer_float_literal() {
    let mut inf = TypeInferencer::new();
    assert_eq!(
        inf.infer_expr(&float_lit(2.5)).unwrap(),
        Type::Float {
            width: FloatWidth::W32
        }
    );
}

#[test]
fn test_infer_double_literal() {
    let mut inf = TypeInferencer::new();
    assert_eq!(inf.infer_expr(&double_lit(99.9)).unwrap(), Type::Double);
}

#[test]
fn test_infer_bool_literal() {
    let mut inf = TypeInferencer::new();
    assert_eq!(inf.infer_expr(&bool_lit(true)).unwrap(), Type::Bool);
    assert_eq!(inf.infer_expr(&bool_lit(false)).unwrap(), Type::Bool);
}

#[test]
fn test_infer_string_literal() {
    let mut inf = TypeInferencer::new();
    assert_eq!(inf.infer_expr(&str_lit("hi")).unwrap(), Type::String);
}

#[test]
fn test_infer_byte_literal() {
    let mut inf = TypeInferencer::new();
    assert_eq!(
        inf.infer_expr(&byte_lit(0xFF)).unwrap(),
        Type::Bits {
            width: IntWidth::W8
        }
    );
}

// ---------------------------------------------------------------------------
// Identifier lookup (tests 7–8)
// ---------------------------------------------------------------------------

#[test]
fn test_infer_ident_lookup() {
    let mut inf = TypeInferencer::new();
    inf.bind("x", Type::int_default());
    assert_eq!(inf.infer_expr(&ident("x")).unwrap(), Type::int_default());
}

#[test]
fn test_infer_ident_undefined() {
    let mut inf = TypeInferencer::new();
    let err = inf.infer_expr(&ident("y")).unwrap_err();
    assert_eq!(err.diagnostic.severity, Severity::Error);
    assert!(err.diagnostic.message.contains("undefined variable"));
    assert!(err.diagnostic.message.contains("y"));
}

// ---------------------------------------------------------------------------
// Binary operator inference (tests 9–14)
// ---------------------------------------------------------------------------

#[test]
fn test_infer_binary_add_int_int() {
    let mut inf = TypeInferencer::new();
    assert_eq!(
        inf.infer_expr(&binary(BinaryOp::Add, int_lit(1), int_lit(2)))
            .unwrap(),
        Type::int_default()
    );
}

#[test]
fn test_infer_binary_add_int_float() {
    let mut inf = TypeInferencer::new();
    assert_eq!(
        inf.infer_expr(&binary(BinaryOp::Add, int_lit(1), float_lit(2.0)))
            .unwrap(),
        Type::float_default()
    );
}

#[test]
fn test_infer_binary_add_int_double() {
    let mut inf = TypeInferencer::new();
    assert_eq!(
        inf.infer_expr(&binary(BinaryOp::Add, int_lit(1), double_lit(2.0)))
            .unwrap(),
        Type::Double
    );
}

#[test]
fn test_infer_binary_compare() {
    let mut inf = TypeInferencer::new();
    assert_eq!(
        inf.infer_expr(&binary(BinaryOp::Lt, int_lit(1), int_lit(2)))
            .unwrap(),
        Type::Bool
    );
    assert_eq!(
        inf.infer_expr(&binary(BinaryOp::Eq, bool_lit(true), bool_lit(false)))
            .unwrap(),
        Type::Bool
    );
}

#[test]
fn test_infer_binary_logical_and() {
    let mut inf = TypeInferencer::new();
    assert_eq!(
        inf.infer_expr(&binary(BinaryOp::And, bool_lit(true), bool_lit(false)))
            .unwrap(),
        Type::Bool
    );
    assert_eq!(
        inf.infer_expr(&binary(BinaryOp::Or, bool_lit(true), bool_lit(false)))
            .unwrap(),
        Type::Bool
    );
}

#[test]
fn test_infer_binary_logical_error() {
    let mut inf = TypeInferencer::new();
    let err = inf
        .infer_expr(&binary(BinaryOp::And, int_lit(1), int_lit(2)))
        .unwrap_err();
    assert!(err.diagnostic.message.contains("Bool"));
}

// ---------------------------------------------------------------------------
// Unary operator inference (tests 15–17)
// ---------------------------------------------------------------------------

#[test]
fn test_infer_unary_neg() {
    let mut inf = TypeInferencer::new();
    assert_eq!(
        inf.infer_expr(&unary(UnaryOp::Neg, int_lit(5))).unwrap(),
        Type::int_default()
    );
    assert_eq!(
        inf.infer_expr(&unary(UnaryOp::Neg, float_lit(2.5)))
            .unwrap(),
        Type::float_default()
    );
}

#[test]
fn test_infer_unary_not_bool() {
    let mut inf = TypeInferencer::new();
    assert_eq!(
        inf.infer_expr(&unary(UnaryOp::Not, bool_lit(true)))
            .unwrap(),
        Type::Bool
    );
}

#[test]
fn test_infer_unary_not_int_error() {
    let mut inf = TypeInferencer::new();
    assert!(inf.infer_expr(&unary(UnaryOp::Not, int_lit(5))).is_err());
}

// ---------------------------------------------------------------------------
// let declarations (tests 18, 25–26)
// ---------------------------------------------------------------------------

#[test]
fn test_infer_let_decl() {
    let mut inf = TypeInferencer::new();
    let stmt = Stmt::LetDecl {
        name: Ident::new("x", sp()),
        value: int_lit(42),
        mutable: false,
        ty: None,
        span: sp(),
    };
    assert_eq!(inf.infer_stmt(&stmt).unwrap(), Type::int_default());
    assert_eq!(inf.lookup("x"), Some(&Type::int_default()));
}

#[test]
fn test_infer_let_decl_annotation_mismatch() {
    // `let x: Int = "hello"` → TypeError
    let mut inf = TypeInferencer::new();
    let stmt = Stmt::LetDecl {
        name: Ident::new("x", sp()),
        value: str_lit("hello"),
        mutable: false,
        ty: Some(TypeRef::Named {
            name: Ident::new("Int", sp()),
            span: sp(),
        }),
        span: sp(),
    };
    let err = inf.infer_stmt(&stmt).unwrap_err();
    assert!(err.diagnostic.message.contains("expected"));
    assert!(err.diagnostic.message.contains("found"));
}

#[test]
fn test_infer_let_decl_annotation_widening_ok() {
    // `let x: Float = 1` → Float (Int widens to Float)
    let mut inf = TypeInferencer::new();
    let stmt = Stmt::LetDecl {
        name: Ident::new("x", sp()),
        value: int_lit(1),
        mutable: false,
        ty: Some(TypeRef::Named {
            name: Ident::new("Float", sp()),
            span: sp(),
        }),
        span: sp(),
    };
    assert_eq!(inf.infer_stmt(&stmt).unwrap(), Type::float_default());
    assert_eq!(inf.lookup("x"), Some(&Type::float_default()));
}

#[test]
fn test_infer_let_decl_annotation_narrowing_rejected() {
    // `let x: Int = 3.14` → TypeError (Float cannot narrow to Int)
    let mut inf = TypeInferencer::new();
    let stmt = Stmt::LetDecl {
        name: Ident::new("x", sp()),
        value: float_lit(2.5),
        mutable: false,
        ty: Some(TypeRef::Named {
            name: Ident::new("Int", sp()),
            span: sp(),
        }),
        span: sp(),
    };
    assert!(inf.infer_stmt(&stmt).is_err());
}

// ---------------------------------------------------------------------------
// if expressions (tests 19–21)
// ---------------------------------------------------------------------------

#[test]
fn test_infer_if_else_same_types() {
    let mut inf = TypeInferencer::new();
    let e = if_expr(
        bool_lit(true),
        block(vec![expr_stmt(int_lit(1))]),
        Some(block(vec![expr_stmt(int_lit(2))])),
    );
    assert_eq!(inf.infer_expr(&e).unwrap(), Type::int_default());
}

#[test]
fn test_infer_if_else_diff_types_error() {
    let mut inf = TypeInferencer::new();
    let e = if_expr(
        bool_lit(true),
        block(vec![expr_stmt(int_lit(1))]),
        Some(block(vec![expr_stmt(str_lit("a"))])),
    );
    let err = inf.infer_expr(&e).unwrap_err();
    assert!(err.diagnostic.message.contains("different types"));
}

#[test]
fn test_infer_if_cond_not_bool_error() {
    let mut inf = TypeInferencer::new();
    let e = if_expr(
        int_lit(5),
        block(vec![expr_stmt(int_lit(1))]),
        Some(block(vec![expr_stmt(int_lit(2))])),
    );
    let err = inf.infer_expr(&e).unwrap_err();
    assert!(err.diagnostic.message.contains("condition"));
}

#[test]
fn test_infer_if_without_else_is_void() {
    let mut inf = TypeInferencer::new();
    let e = if_expr(bool_lit(true), block(vec![expr_stmt(int_lit(1))]), None);
    assert_eq!(inf.infer_expr(&e).unwrap(), Type::Void);
}

// ---------------------------------------------------------------------------
// Promotion rules (tests 22–23)
// ---------------------------------------------------------------------------

#[test]
fn test_promote_decimal_dominates() {
    assert_eq!(
        promote_binary(&Type::Decimal, &Type::int_default()),
        Some(Type::Decimal)
    );
    assert_eq!(
        promote_binary(&Type::float_default(), &Type::Decimal),
        Some(Type::Decimal)
    );
}

#[test]
fn test_promote_double_dominates_float() {
    assert_eq!(
        promote_binary(&Type::Double, &Type::float_default()),
        Some(Type::Double)
    );
}

#[test]
fn test_promote_bits_max_width() {
    let small = Type::Bits {
        width: IntWidth::W8,
    };
    let big = Type::Bits {
        width: IntWidth::W64,
    };
    assert_eq!(promote_binary(&small, &big), Some(big.clone()));
    assert_eq!(promote_binary(&big, &small), Some(big));
}

#[test]
fn test_promote_int_max_width() {
    let w32 = Type::Int {
        width: IntWidth::W32,
    };
    let w64 = Type::Int {
        width: IntWidth::W64,
    };
    assert_eq!(promote_binary(&w32, &w64), Some(w64.clone()));
}

#[test]
fn test_promote_incompatible_returns_none() {
    assert_eq!(promote_binary(&Type::Bool, &Type::int_default()), None);
    assert_eq!(promote_binary(&Type::String, &Type::int_default()), None);
}

// ---------------------------------------------------------------------------
// Display (test 24)
// ---------------------------------------------------------------------------

#[test]
fn test_type_display() {
    assert_eq!(format!("{}", Type::int_default()), "Int<64>");
    assert_eq!(format!("{}", Type::float_default()), "Float<32>");
    assert_eq!(format!("{}", Type::Double), "Double");
    assert_eq!(format!("{}", Type::bool()), "Bool");
    assert_eq!(format!("{}", Type::string()), "String");
    assert_eq!(format!("{}", Type::byte()), "Bits<8>");
    assert_eq!(format!("{}", Type::Decimal), "Decimal");
    assert_eq!(format!("{}", Type::Unknown), "Unknown");
    assert_eq!(format!("{}", Type::Void), "Void");
}

// ---------------------------------------------------------------------------
// Extra coverage: bitwise, span propagation, compound assignment
// ---------------------------------------------------------------------------

#[test]
fn test_infer_bitwise_int_int() {
    let mut inf = TypeInferencer::new();
    assert_eq!(
        inf.infer_expr(&binary(BinaryOp::BitAnd, int_lit(1), int_lit(2)))
            .unwrap(),
        Type::int_default()
    );
    assert_eq!(
        inf.infer_expr(&binary(BinaryOp::Shl, int_lit(1), int_lit(2)))
            .unwrap(),
        Type::int_default()
    );
}

#[test]
fn test_infer_bitwise_on_float_errors() {
    let mut inf = TypeInferencer::new();
    assert!(inf
        .infer_expr(&binary(BinaryOp::BitAnd, float_lit(1.0), int_lit(2)))
        .is_err());
}

#[test]
fn test_infer_unary_bitnot_int() {
    let mut inf = TypeInferencer::new();
    assert_eq!(
        inf.infer_expr(&unary(UnaryOp::BitNot, int_lit(5))).unwrap(),
        Type::int_default()
    );
}

#[test]
fn test_infer_unary_bitnot_float_errors() {
    let mut inf = TypeInferencer::new();
    assert!(inf
        .infer_expr(&unary(UnaryOp::BitNot, float_lit(2.5)))
        .is_err());
}

#[test]
fn test_infer_comparison_incompatible_errors() {
    let mut inf = TypeInferencer::new();
    assert!(inf
        .infer_expr(&binary(BinaryOp::Lt, int_lit(1), str_lit("a")))
        .is_err());
}

#[test]
fn test_type_error_carries_span() {
    let mut inf = TypeInferencer::new();
    let span = Span::new(10, 20, buff_lang_error::SourceId(7));
    let e = Expr::Ident(Ident::new("missing", span), span);
    let err = inf.infer_expr(&e).unwrap_err();
    assert_eq!(err.diagnostic.span, span);
    assert_eq!(err.diagnostic.severity, Severity::Error);
}

#[test]
fn test_diagnostic_is_error_construct() {
    let d = Diagnostic::error("boom", Span::dummy());
    assert_eq!(d.severity, Severity::Error);
    assert_eq!(d.message, "boom");
}

#[test]
fn test_infer_nested_arithmetic() {
    // (1 + 2.0) + 3.0d  →  Double
    let mut inf = TypeInferencer::new();
    let inner = binary(BinaryOp::Add, int_lit(1), float_lit(2.0));
    let outer = binary(BinaryOp::Add, inner, double_lit(3.0));
    assert_eq!(inf.infer_expr(&outer).unwrap(), Type::Double);
}

#[test]
fn test_infer_compound_assign_ok() {
    let mut inf = TypeInferencer::new();
    inf.bind("x", Type::int_default());
    let lhs = ident("x");
    let e = binary(BinaryOp::AddAssign, lhs, int_lit(3));
    assert_eq!(inf.infer_expr(&e).unwrap(), Type::int_default());
}

#[test]
fn test_infer_func_call_returns_unknown() {
    let mut inf = TypeInferencer::new();
    let callee = ident("f");
    let e = Expr::FuncCall {
        callee: Box::new(callee),
        args: vec![int_lit(1)],
        span: sp(),
    };
    assert_eq!(inf.infer_expr(&e).unwrap(), Type::Unknown);
}

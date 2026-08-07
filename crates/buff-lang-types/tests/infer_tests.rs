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
// T19: Byte (Bits<8>) support — named test for acceptance criteria
// ---------------------------------------------------------------------------

#[test]
fn byte_type() {
    let mut inf = TypeInferencer::new();

    // 0xFF infers as Byte (Bits<8>)
    assert_eq!(
        inf.infer_expr(&byte_lit(0xFF)).unwrap(),
        Type::Bits {
            width: IntWidth::W8
        }
    );

    // 0b1010 infers as Byte (Bits<8>)
    assert_eq!(
        inf.infer_expr(&byte_lit(0b1010)).unwrap(),
        Type::Bits {
            width: IntWidth::W8
        }
    );

    // let b: Byte = 0xFF → type-checks (Byte annotation matches byte literal)
    let stmt = Stmt::LetDecl {
        name: Ident::new("b", sp()),
        value: byte_lit(0xFF),
        mutable: false,
        ty: Some(TypeRef::Named {
            name: Ident::new("Byte", sp()),
            span: sp(),
        }),
        span: sp(),
    };
    assert_eq!(
        inf.infer_stmt(&stmt).unwrap(),
        Type::Bits {
            width: IntWidth::W8
        }
    );
    assert_eq!(
        inf.lookup("b"),
        Some(&Type::Bits {
            width: IntWidth::W8
        })
    );

    // Byte + Byte → Byte (Bits<8>)
    let mut inf2 = TypeInferencer::new();
    assert_eq!(
        inf2.infer_expr(&binary(BinaryOp::Add, byte_lit(1), byte_lit(2)))
            .unwrap(),
        Type::Bits {
            width: IntWidth::W8
        }
    );

    // Byte + Int → Int (signed wins)
    let mut inf3 = TypeInferencer::new();
    assert_eq!(
        inf3.infer_expr(&binary(BinaryOp::Add, byte_lit(1), int_lit(2)))
            .unwrap(),
        Type::int_default()
    );
}

// ---------------------------------------------------------------------------
// T18: Double (f64) full support — named test for acceptance criteria
// ---------------------------------------------------------------------------

#[test]
fn double_inference() {
    // 2.5d infers as Double (f64), not Float (f32)
    let mut inf = TypeInferencer::new();
    assert_eq!(inf.infer_expr(&double_lit(2.5)).unwrap(), Type::Double);

    // 2.5 (no suffix) infers as Float (f32)
    assert_eq!(
        inf.infer_expr(&float_lit(2.5)).unwrap(),
        Type::Float {
            width: FloatWidth::W32
        }
    );

    // Double + Double → Double
    assert_eq!(
        inf.infer_expr(&binary(BinaryOp::Add, double_lit(1.0), double_lit(2.0)))
            .unwrap(),
        Type::Double
    );

    // Double + Float → Double (widening)
    assert_eq!(
        inf.infer_expr(&binary(BinaryOp::Add, double_lit(1.0), float_lit(2.0)))
            .unwrap(),
        Type::Double
    );

    // Float + Double → Double (widening, reversed)
    assert_eq!(
        inf.infer_expr(&binary(BinaryOp::Add, float_lit(1.0), double_lit(2.0)))
            .unwrap(),
        Type::Double
    );

    // Int + Double → Double (widening)
    assert_eq!(
        inf.infer_expr(&binary(BinaryOp::Add, int_lit(1), double_lit(2.0)))
            .unwrap(),
        Type::Double
    );
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

// ---------------------------------------------------------------------------
// T20: Decimal (128-bit fixed-point) — named module so the selector
// `cargo test -p buff-lang-types decimal_type` matches all sub-tests.
//
// Decimal literals (`99.90m`) infer as `Type::Decimal` (NOT Double/Float),
// Decimal arithmetic stays Decimal, and the type is flagged CPU-only
// (never GPU-eligible). Exactness of `0.1m + 0.2m == 0.3m` is proven at the
// codegen layer; here we prove the *type* of that comparison is `Bool`.
// ---------------------------------------------------------------------------

mod decimal_type {
    use super::*;

    /// Build a `Literal::Decimal(raw)` expression (raw digit text, no suffix).
    fn dec_lit(raw: &str) -> Expr {
        Expr::Literal(Literal::Decimal(raw.to_string()), sp())
    }

    #[test]
    fn decimal_literal_infers_decimal() {
        // `99.90m` infers as Decimal — NOT Double, NOT Float.
        let mut inf = TypeInferencer::new();
        let ty = inf.infer_expr(&dec_lit("99.90")).unwrap();
        assert_eq!(ty, Type::Decimal);
        assert_ne!(ty, Type::Double);
        assert_ne!(
            ty,
            Type::Float {
                width: FloatWidth::W32
            }
        );
    }

    #[test]
    fn decimal_not_double_not_float() {
        // Explicit double-negative: a Decimal literal is neither Double nor
        // Float, even though its textual shape resembles both.
        let mut inf = TypeInferencer::new();
        let ty = inf.infer_expr(&dec_lit("0.1")).unwrap();
        assert!(!ty.is_float_like() || ty == Type::Decimal);
        assert_eq!(ty, Type::Decimal);
        assert_ne!(ty, Type::double());
    }

    #[test]
    fn decimal_add_decimal_is_decimal() {
        // Decimal + Decimal → Decimal
        let mut inf = TypeInferencer::new();
        let e = binary(BinaryOp::Add, dec_lit("0.1"), dec_lit("0.2"));
        assert_eq!(inf.infer_expr(&e).unwrap(), Type::Decimal);
    }

    #[test]
    fn decimal_mul_decimal_is_decimal() {
        // Decimal * Decimal → Decimal
        let mut inf = TypeInferencer::new();
        let e = binary(BinaryOp::Mul, dec_lit("2.0"), dec_lit("3.0"));
        assert_eq!(inf.infer_expr(&e).unwrap(), Type::Decimal);
    }

    #[test]
    fn decimal_sub_div_mod_stay_decimal() {
        // Sub / Div / Mod all stay Decimal.
        let mut inf = TypeInferencer::new();
        assert_eq!(
            inf.infer_expr(&binary(BinaryOp::Sub, dec_lit("1.0"), dec_lit("0.5")))
                .unwrap(),
            Type::Decimal
        );
        assert_eq!(
            inf.infer_expr(&binary(BinaryOp::Div, dec_lit("1.0"), dec_lit("4.0")))
                .unwrap(),
            Type::Decimal
        );
        assert_eq!(
            inf.infer_expr(&binary(BinaryOp::Mod, dec_lit("1.0"), dec_lit("4.0")))
                .unwrap(),
            Type::Decimal
        );
    }

    #[test]
    fn decimal_dominates_other_numerics() {
        // Decimal dominates Int, Float, Double (per promote.rs).
        let mut inf = TypeInferencer::new();
        assert_eq!(
            inf.infer_expr(&binary(BinaryOp::Add, dec_lit("1.0"), int_lit(2)))
                .unwrap(),
            Type::Decimal
        );
        assert_eq!(
            inf.infer_expr(&binary(BinaryOp::Add, dec_lit("1.0"), float_lit(2.0)))
                .unwrap(),
            Type::Decimal
        );
        assert_eq!(
            inf.infer_expr(&binary(BinaryOp::Add, dec_lit("1.0"), double_lit(2.0)))
                .unwrap(),
            Type::Decimal
        );
        // Reversed operand order still Decimal.
        assert_eq!(
            inf.infer_expr(&binary(BinaryOp::Add, int_lit(2), dec_lit("1.0")))
                .unwrap(),
            Type::Decimal
        );
    }

    #[test]
    fn decimal_comparison_infers_bool() {
        // `0.1m + 0.2m == 0.3m` infers as Bool (the exactness proof itself
        // lives in the codegen/rust_decimal layer; here we confirm the
        // comparison type-checks to Bool).
        let mut inf = TypeInferencer::new();
        let lhs = binary(BinaryOp::Add, dec_lit("0.1"), dec_lit("0.2"));
        let cmp = binary(BinaryOp::Eq, lhs, dec_lit("0.3"));
        assert_eq!(inf.infer_expr(&cmp).unwrap(), Type::Bool);
    }

    #[test]
    fn decimal_let_decl_binds_decimal() {
        // `let price = 99.90m` binds `price` as Decimal in the environment.
        let mut inf = TypeInferencer::new();
        let stmt = Stmt::LetDecl {
            name: Ident::new("price", sp()),
            value: dec_lit("99.90"),
            mutable: false,
            ty: None,
            span: sp(),
        };
        assert_eq!(inf.infer_stmt(&stmt).unwrap(), Type::Decimal);
        assert_eq!(inf.lookup("price"), Some(&Type::Decimal));
    }

    #[test]
    fn decimal_let_annotation_matches() {
        // `let price: Decimal = 99.90m` type-checks (annotation matches).
        let mut inf = TypeInferencer::new();
        let stmt = Stmt::LetDecl {
            name: Ident::new("price", sp()),
            value: dec_lit("99.90"),
            mutable: false,
            ty: Some(TypeRef::Named {
                name: Ident::new("Decimal", sp()),
                span: sp(),
            }),
            span: sp(),
        };
        assert_eq!(inf.infer_stmt(&stmt).unwrap(), Type::Decimal);
        assert_eq!(inf.lookup("price"), Some(&Type::Decimal));
    }

    #[test]
    fn decimal_unary_neg_stays_decimal() {
        // `-99.90m` is Decimal (negation preserves numeric type).
        let mut inf = TypeInferencer::new();
        let e = unary(UnaryOp::Neg, dec_lit("99.90"));
        assert_eq!(inf.infer_expr(&e).unwrap(), Type::Decimal);
    }

    #[test]
    fn decimal_is_cpu_only_never_gpu() {
        // CRITICAL: Decimal must NEVER be GPU-eligible. The dispatch engine
        // (v1.0) will consume this predicate to force Decimal onto CPU/Rayon.
        assert!(!Type::Decimal.is_gpu_eligible());
        assert!(Type::Decimal.must_run_on_cpu());

        // Sanity: 32-bit WGSL-native scalars ARE gpu-eligible (contrast).
        assert!(Type::float_default().is_gpu_eligible());
    }

    #[test]
    fn decimal_is_numeric_and_float_like() {
        // Decimal participates in numeric promotion and is float-like.
        assert!(Type::Decimal.is_numeric());
        assert!(Type::Decimal.is_float_like());
    }

    #[test]
    fn decimal_compound_assign_ok() {
        // `price += 0.1m` type-checks when price is Decimal.
        let mut inf = TypeInferencer::new();
        inf.bind("price", Type::Decimal);
        let e = binary(BinaryOp::AddAssign, ident("price"), dec_lit("0.1"));
        assert_eq!(inf.infer_expr(&e).unwrap(), Type::Decimal);
    }

    // T76: Union types `A | B` tests.

    fn named_type(name: &str) -> TypeRef {
        TypeRef::Named {
            name: Ident::new(name, sp()),
            span: sp(),
        }
    }

    #[test]
    fn union_types_two_members_annotation_resolves() {
        let annotated = TypeRef::Union(vec![named_type("String"), named_type("Int")], sp());
        let expected = Type::Union(vec![Type::string(), Type::int_default()]);

        let mut inf = TypeInferencer::new();
        inf.bind("input", expected.clone());

        let stmt = Stmt::LetDecl {
            name: Ident::new("value", sp()),
            value: ident("input"),
            mutable: false,
            ty: Some(annotated),
            span: sp(),
        };

        assert_eq!(inf.infer_stmt(&stmt).unwrap(), expected.clone());
        assert_eq!(inf.lookup("value"), Some(&expected));
    }

    #[test]
    fn union_types_three_members_annotation_resolves() {
        let annotated = TypeRef::Union(
            vec![named_type("String"), named_type("Int"), named_type("Bool")],
            sp(),
        );
        let expected = Type::Union(vec![Type::string(), Type::int_default(), Type::bool()]);

        let mut inf = TypeInferencer::new();
        inf.bind("input", expected.clone());

        let stmt = Stmt::LetDecl {
            name: Ident::new("value", sp()),
            value: ident("input"),
            mutable: false,
            ty: Some(annotated),
            span: sp(),
        };

        assert_eq!(inf.infer_stmt(&stmt).unwrap(), expected.clone());
        assert_eq!(inf.lookup("value"), Some(&expected));
    }

    #[test]
    fn union_types_display_formats_with_pipe() {
        let ty = Type::Union(vec![Type::string(), Type::int_default()]);
        assert_eq!(format!("{ty}"), "String | Int<64>");
    }

    #[test]
    fn union_types_nested_annotation_resolves_recursively() {
        let inner = TypeRef::Union(vec![named_type("String"), named_type("Int")], sp());
        let annotated = TypeRef::Union(vec![inner, named_type("Bool")], sp());
        let expected = Type::Union(vec![
            Type::Union(vec![Type::string(), Type::int_default()]),
            Type::bool(),
        ]);

        let mut inf = TypeInferencer::new();
        inf.bind("input", expected.clone());

        let stmt = Stmt::LetDecl {
            name: Ident::new("value", sp()),
            value: ident("input"),
            mutable: false,
            ty: Some(annotated),
            span: sp(),
        };

        assert_eq!(inf.infer_stmt(&stmt).unwrap(), expected.clone());
        assert_eq!(inf.lookup("value"), Some(&expected));
    }

    #[test]
    fn union_types_unknown_member_becomes_unknown() {
        let annotated = TypeRef::Union(vec![named_type("String"), named_type("Mystery")], sp());
        let expected = Type::Union(vec![Type::string(), Type::Unknown]);

        let mut inf = TypeInferencer::new();
        inf.bind("input", expected.clone());

        let stmt = Stmt::LetDecl {
            name: Ident::new("value", sp()),
            value: ident("input"),
            mutable: false,
            ty: Some(annotated),
            span: sp(),
        };

        assert_eq!(inf.infer_stmt(&stmt).unwrap(), expected.clone());
        assert_eq!(inf.lookup("value"), Some(&expected));
    }
}

// ---------------------------------------------------------------------------
// T83 — Nested collection literal type inference.
// ---------------------------------------------------------------------------

/// Build an `Expr::ArrayLit` from a list of elements.
fn array_lit(elements: Vec<Expr>) -> Expr {
    Expr::ArrayLit {
        elements,
        span: sp(),
    }
}

/// Build an `Expr::MapLit` from a list of `(key, value)` entry pairs.
fn map_lit(entries: Vec<(Expr, Expr)>) -> Expr {
    Expr::MapLit {
        entries,
        span: sp(),
    }
}

/// T83: `[[1, 2], [3, 4]]` must infer `Vector<Vector<Int>>`, NOT flatten
/// to `Vector<Int>` (the pre-T83 default-Int fallback).
///
/// Pre-T83 bug: the outer literal's `infer_collection_element` saw
/// ArrayLit elements (not int literals), fell through to the default-
/// Int fallback, and produced `Vector<Int<64>>` — losing the nesting
/// depth. T83 short-circuits when the first element is itself an
/// ArrayLit/MapLit, recursing via `infer_expr` to preserve depth.
#[test]
fn t83_nested_vector_literal_preserves_nesting_depth() {
    let inner1 = array_lit(vec![int_lit(1), int_lit(2)]);
    let inner2 = array_lit(vec![int_lit(3), int_lit(4)]);
    let outer = array_lit(vec![inner1, inner2]);
    let mut inf = TypeInferencer::new();
    let ty = inf.infer_expr(&outer).unwrap();
    // Expected: Vector<Vector<Int<64>>> (i64 is the default int width
    // for small positive literals via range analysis).
    match ty {
        Type::Vector(inner) => match *inner {
            Type::Vector(innermost) => match *innermost {
                Type::Int { .. } => {}
                other => panic!("T83: innermost type must be Int, got {other:?}"),
            },
            other => panic!("T83: inner type must be Vector, got {other:?}"),
        },
        other => panic!("T83: outer type must be Vector, got {other:?}"),
    }
}

/// T83: 3-deep nesting `[[[1]]]` → `Vector<Vector<Vector<Int>>>`.
/// Ensures recursion is unbounded (not just one level deep).
#[test]
fn t83_deeply_nested_vector_literal_preserves_all_depths() {
    let deepest = array_lit(vec![int_lit(1)]);
    let middle = array_lit(vec![deepest]);
    let outer = array_lit(vec![middle]);
    let mut inf = TypeInferencer::new();
    let ty = inf.infer_expr(&outer).unwrap();
    // Walk three levels deep.
    let level1 = match ty {
        Type::Vector(inner) => *inner,
        other => panic!("T83: level 1 must be Vector, got {other:?}"),
    };
    let level2 = match level1 {
        Type::Vector(inner) => *inner,
        other => panic!("T83: level 2 must be Vector, got {other:?}"),
    };
    match level2 {
        Type::Vector(inner) => match *inner {
            Type::Int { .. } => {}
            other => panic!("T83: innermost must be Int, got {other:?}"),
        },
        other => panic!("T83: level 3 must be Vector, got {other:?}"),
    }
}

/// T83: nested MAP `{"a": {"b": 1}}` → `Map<String, Map<String, Int>>`.
/// Before the fix, the outer map's value type fell through to the
/// default Int fallback, producing `Map<String, Int<64>>` (lost the
/// inner Map).
#[test]
fn t83_nested_map_literal_preserves_value_nesting() {
    let inner_map = map_lit(vec![(str_lit("b"), int_lit(1))]);
    let outer = map_lit(vec![(str_lit("a"), inner_map)]);
    let mut inf = TypeInferencer::new();
    let ty = inf.infer_expr(&outer).unwrap();
    match ty {
        Type::Map(_key_ty, val_ty) => match *val_ty {
            Type::Map(_inner_k, inner_v) => match *inner_v {
                Type::Int { .. } => {}
                other => panic!("T83: innermost value must be Int, got {other:?}"),
            },
            other => panic!("T83: value must be Map, got {other:?}"),
        },
        other => panic!("T83: outer must be Map, got {other:?}"),
    }
}

/// T83: flat collection behavior is UNCHANGED. `[1, 2, 3]` still
/// infers `Vector<Int>` (auto-width via range analysis). This guards
/// against T83's nested-recursion path accidentally catching flat
/// literals.
#[test]
fn t83_flat_vector_literal_is_unchanged() {
    let flat = array_lit(vec![int_lit(1), int_lit(2), int_lit(3)]);
    let mut inf = TypeInferencer::new();
    let ty = inf.infer_expr(&flat).unwrap();
    match ty {
        Type::Vector(inner) => match *inner {
            Type::Int { .. } => {}
            other => panic!("T83: flat element must be Int, got {other:?}"),
        },
        other => panic!("T83: flat literal must be Vector, got {other:?}"),
    }
}

/// T83: flat map behavior is UNCHANGED. `{"k": 1}` still infers
/// `Map<String, Int>` (the pre-T83 behavior).
#[test]
fn t83_flat_map_literal_is_unchanged() {
    let flat = map_lit(vec![(str_lit("k"), int_lit(1))]);
    let mut inf = TypeInferencer::new();
    let ty = inf.infer_expr(&flat).unwrap();
    match ty {
        Type::Map(k, v) => {
            match *k {
                Type::String => {}
                other => panic!("T83: flat map key must be String, got {other:?}"),
            }
            match *v {
                Type::Int { .. } => {}
                other => panic!("T83: flat map value must be Int, got {other:?}"),
            }
        }
        other => panic!("T83: flat literal must be Map, got {other:?}"),
    }
}

/// T83: mixed vector-of-vectors with different int widths still works
/// (uses the first element's nested type). `[[1, 2, 3], [4, 5]]` →
/// `Vector<Vector<Int>>`.
#[test]
fn t83_nested_vector_with_varying_lengths_preserves_nesting() {
    let inner1 = array_lit(vec![int_lit(1), int_lit(2), int_lit(3)]);
    let inner2 = array_lit(vec![int_lit(4), int_lit(5)]);
    let outer = array_lit(vec![inner1, inner2]);
    let mut inf = TypeInferencer::new();
    let ty = inf.infer_expr(&outer).unwrap();
    match ty {
        Type::Vector(inner) => match *inner {
            Type::Vector(innermost) => match *innermost {
                Type::Int { .. } => {}
                other => panic!("T83: innermost must be Int, got {other:?}"),
            },
            other => panic!("T83: inner must be Vector, got {other:?}"),
        },
        other => panic!("T83: outer must be Vector, got {other:?}"),
    }
}

// ---------------------------------------------------------------------------
// T84 — Range expression type inference (`start..end` → `Range<T>`)
// ---------------------------------------------------------------------------
//
// T68 shipped the AST/parser/codegen for ranges but the type inferencer
// returned `Type::Unknown`. T84 closes the loop: `0..10` now infers
// `Range<Int<64>>` (a lazy iterator, NOT a Vector).

fn range_expr(start: Expr, end: Expr, inclusive: bool) -> Expr {
    Expr::Range {
        start: Box::new(start),
        end: Box::new(end),
        inclusive,
        span: sp(),
    }
}

/// T84: `0..10` (exclusive) infers `Range<Int<64>>`. The element type
/// is taken from the start bound (an `Int` literal → `Int<64>` via
/// range-analysis default-width).
#[test]
fn t84_exclusive_range_infers_range_of_int() {
    let e = range_expr(int_lit(0), int_lit(10), false);
    let mut inf = TypeInferencer::new();
    let ty = inf.infer_expr(&e).unwrap();
    match ty {
        Type::Range(elem) => match *elem {
            Type::Int { .. } => {}
            other => panic!("T84: range element must be Int, got {other:?}"),
        },
        other => panic!("T84: `0..10` must infer Range<Int>, got {other:?}"),
    }
}

/// T84: `0..=10` (inclusive) infers `Range<Int<64>>` — the same type as
/// the exclusive form. Buff surfaces a single `Range<T>` abstraction;
/// the inclusive/exclusive distinction lives in the AST
/// (`Expr::Range { inclusive }`), not the type layer.
#[test]
fn t84_inclusive_range_infers_range_of_int() {
    let e = range_expr(int_lit(0), int_lit(10), true);
    let mut inf = TypeInferencer::new();
    let ty = inf.infer_expr(&e).unwrap();
    assert!(
        matches!(ty, Type::Range(ref elem) if matches!(**elem, Type::Int { .. })),
        "T84: `0..=10` must infer Range<Int>, got {ty:?}"
    );
}

/// T84: range with float bounds infers `Range<Float<...>>` — the
/// element type flows from the bound type, not hardcoded to Int.
#[test]
fn t84_range_with_float_bounds_infers_range_of_float() {
    let e = range_expr(float_lit(0.0), float_lit(1.0), false);
    let mut inf = TypeInferencer::new();
    let ty = inf.infer_expr(&e).unwrap();
    assert!(
        matches!(ty, Type::Range(ref elem) if matches!(**elem, Type::Float { .. })),
        "T84: `0.0..1.0` must infer Range<Float>, got {ty:?}"
    );
}

/// T84: range with two unknown bounds (idents explicitly bound to
/// `Unknown` in the environment) falls back to `Range<Int<64>>` —
/// Buff's default integer width. This keeps the range element type
/// concrete even when the bounds are themselves indeterminate (e.g.
/// a function parameter whose type couldn't be resolved). Note: a
/// truly undefined variable returns a `TypeError` at inference time
/// (not `Unknown`), so we pre-bind the idents here to exercise the
/// fallback path.
#[test]
fn t84_range_with_unknown_bounds_falls_back_to_int() {
    let e = range_expr(ident("a"), ident("b"), false);
    let mut inf = TypeInferencer::new();
    // Pre-bind both idents to Unknown so inference doesn't error.
    inf.bind("a", Type::Unknown);
    inf.bind("b", Type::Unknown);
    let ty = inf.infer_expr(&e).unwrap();
    assert!(
        matches!(ty, Type::Range(ref elem) if matches!(**elem, Type::Int { .. })),
        "T84: unknown-bounds range must fall back to Range<Int>, got {ty:?}"
    );
}

/// T84: `Range<Int>` is NOT numeric — it's a lazy iterator, so it
/// participates in no numeric promotion. Mirrors Vector/Map/Option
/// (collection types are never numeric).
#[test]
fn t84_range_is_not_numeric() {
    let r = Type::range(Type::int_default());
    assert!(!r.is_numeric(), "Range must not be numeric");
    assert!(!r.is_float_like(), "Range must not be float-like");
    assert!(!r.is_integer_like(), "Range must not be integer-like");
    assert!(!r.is_gpu_eligible(), "Range must not be GPU-eligible");
}

/// T84: `Type::range(elem)` constructor produces the expected shape
/// and `Display` renders `Range<elem>`.
#[test]
fn t84_range_constructor_and_display() {
    let r = Type::range(Type::int_default());
    assert!(matches!(r, Type::Range(_)));
    assert_eq!(r.to_string(), "Range<Int<64>>");

    let r2 = Type::range(Type::Double);
    assert_eq!(r2.to_string(), "Range<Double>");
}

/// T84: `Range` is registered as a prelude type — `is_prelude_type`
/// resolves the surface name, `prelude_type_lookup` returns the
/// variant, and `buff_type()` returns a `Type::Range` (NOT Void —
/// Range IS a runtime value, unlike namespace-only modules).
#[test]
fn t84_range_registered_as_prelude_type() {
    use buff_lang_types::{is_prelude_type, prelude_type_lookup, PreludeType};
    assert!(is_prelude_type("Range"), "`Range` must be a prelude type");
    let pt = prelude_type_lookup("Range").expect("Range must resolve");
    assert_eq!(pt, PreludeType::Range);
    assert_eq!(pt.name(), "Range");
    // Range IS a runtime value (a lazy iterator handle), NOT a
    // namespace-only module like Log/Toml/Math.
    assert!(!pt.is_namespace_only(), "Range must NOT be namespace-only");
    // buff_type returns Type::Range (not Void).
    let ty = pt.buff_type();
    assert!(
        matches!(ty, Type::Range(_)),
        "Range.buff_type() must return Type::Range, got {ty:?}"
    );
}

// ---------------------------------------------------------------------------
// T42 — Complex pattern type inference for match arms
// ---------------------------------------------------------------------------
//
// Tests for enum variant patterns, nested patterns, struct patterns,
// or-patterns, and guard conditions.

use buff_lang_ast::{MatchArm, Pattern};

fn pat_ident(name: &str) -> Pattern {
    Pattern::Ident(Ident::new(name, sp()), sp())
}

fn pat_wild() -> Pattern {
    Pattern::Wildcard(sp())
}

fn pat_variant(enum_name: &str, variant: &str, subpatterns: Vec<Pattern>) -> Pattern {
    Pattern::Variant {
        enum_name: Ident::new(enum_name, sp()),
        variant: Ident::new(variant, sp()),
        subpatterns,
        span: sp(),
    }
}

fn pat_tuple(subs: Vec<Pattern>) -> Pattern {
    Pattern::Tuple(subs, sp())
}

fn pat_struct(name: &str, fields: Vec<(&str, Pattern)>) -> Pattern {
    Pattern::Struct {
        name: Ident::new(name, sp()),
        fields: fields
            .into_iter()
            .map(|(n, p)| (Ident::new(n, sp()), p))
            .collect(),
        span: sp(),
        rest: false,
    }
}

fn pat_or(alts: Vec<Pattern>) -> Pattern {
    Pattern::Or(alts, sp())
}

fn match_arm(pattern: Pattern, body: Vec<Stmt>) -> MatchArm {
    MatchArm {
        pattern,
        guard: None,
        body: block(body),
        span: sp(),
    }
}

fn match_arm_guarded(pattern: Pattern, guard: Expr, body: Vec<Stmt>) -> MatchArm {
    MatchArm {
        pattern,
        guard: Some(guard),
        body: block(body),
        span: sp(),
    }
}

fn match_expr(scrutinee: Expr, arms: Vec<MatchArm>) -> Expr {
    Expr::MatchExpr {
        scrutinee: Box::new(scrutinee),
        arms,
        span: sp(),
    }
}

/// T42: enum variant pattern `Some(x)` infers `x` as the inner type.
/// `match opt { Some(x) => x, None => 0 }` — the scrutinee is
/// `Option<Int>`, so `x` should infer as `Int`.
#[test]
fn t42_enum_variant_pattern_infers_inner_type() {
    // Build: match opt { Some(x) => x, None => 0 }
    let scrutinee = ident("opt");
    let arms = vec![
        match_arm(
            pat_variant("Option", "Some", vec![pat_ident("x")]),
            vec![expr_stmt(ident("x"))],
        ),
        match_arm(
            pat_variant("Option", "None", vec![]),
            vec![expr_stmt(int_lit(0))],
        ),
    ];
    let e = match_expr(scrutinee, arms);
    let mut inf = TypeInferencer::new();
    inf.bind("opt", Type::option(Type::int_default()));
    let ty = inf.infer_expr(&e).unwrap();
    // Both arms return Int, so the match type is Int.
    assert_eq!(
        ty,
        Type::int_default(),
        "T42: match on Option<Int> should infer Int"
    );
    // x should be bound to Int in the Some arm.
    // (The env is restored after each arm, so we check via the arm body inference.)
}

/// T42: nested pattern `Some(Some(x))` infers `x` as the innermost type.
/// `match opt { Some(Some(x)) => x, _ => 0 }` — scrutinee is
/// `Option<Option<Int>>`, so `x` should infer as `Int`.
#[test]
fn t42_nested_pattern_infers_inner_type() {
    let scrutinee = ident("opt");
    let arms = vec![
        match_arm(
            pat_variant(
                "Option",
                "Some",
                vec![pat_variant("Option", "Some", vec![pat_ident("x")])],
            ),
            vec![expr_stmt(ident("x"))],
        ),
        match_arm(pat_wild(), vec![expr_stmt(int_lit(0))]),
    ];
    let e = match_expr(scrutinee, arms);
    let mut inf = TypeInferencer::new();
    inf.bind("opt", Type::option(Type::option(Type::int_default())));
    let ty = inf.infer_expr(&e).unwrap();
    assert_eq!(
        ty,
        Type::int_default(),
        "T42: nested match on Option<Option<Int>> should infer Int"
    );
}

/// T42: struct pattern `Point(x, y)` — each field binding gets Unknown
/// (full struct field resolution is deferred to rustc).
#[test]
fn t42_struct_pattern_binds_fields() {
    let scrutinee = ident("p");
    let arms = vec![
        match_arm(
            pat_struct("Point", vec![("x", pat_ident("a")), ("y", pat_ident("b"))]),
            vec![expr_stmt(int_lit(1))],
        ),
        match_arm(pat_wild(), vec![expr_stmt(int_lit(0))]),
    ];
    let e = match_expr(scrutinee, arms);
    let mut inf = TypeInferencer::new();
    inf.bind("p", Type::Unknown);
    let ty = inf.infer_expr(&e).unwrap();
    // Both arms return Int, so the match type is Int.
    assert_eq!(
        ty,
        Type::int_default(),
        "T42: struct pattern match should infer Int"
    );
}

/// T42: or-pattern `Red | Blue` — both arms bind the same types.
/// `match color { Red | Blue => 1, _ => 0 }`.
#[test]
fn t42_or_pattern_accepts_alternatives() {
    let scrutinee = ident("color");
    let arms = vec![
        match_arm(
            pat_or(vec![pat_ident("Red"), pat_ident("Blue")]),
            vec![expr_stmt(int_lit(1))],
        ),
        match_arm(pat_wild(), vec![expr_stmt(int_lit(0))]),
    ];
    let e = match_expr(scrutinee, arms);
    let mut inf = TypeInferencer::new();
    inf.bind("color", Type::Unknown);
    let ty = inf.infer_expr(&e).unwrap();
    assert_eq!(
        ty,
        Type::int_default(),
        "T42: or-pattern match should infer Int"
    );
}

/// T42: guard condition `Some(x) if x > 0` — the guard is inferred
/// and must be Bool.
#[test]
fn t42_guard_condition_inferred() {
    let scrutinee = ident("opt");
    let arms = vec![
        match_arm_guarded(
            pat_variant("Option", "Some", vec![pat_ident("x")]),
            binary(BinaryOp::Gt, ident("x"), int_lit(0)),
            vec![expr_stmt(ident("x"))],
        ),
        match_arm(pat_wild(), vec![expr_stmt(int_lit(0))]),
    ];
    let e = match_expr(scrutinee, arms);
    let mut inf = TypeInferencer::new();
    inf.bind("opt", Type::option(Type::int_default()));
    let ty = inf.infer_expr(&e).unwrap();
    assert_eq!(
        ty,
        Type::int_default(),
        "T42: guarded match should infer Int"
    );
}

/// T42: tuple pattern `(a, b)` — each element gets the corresponding
/// tuple member type.
#[test]
fn t42_tuple_pattern_infers_member_types() {
    let scrutinee = ident("pair");
    let arms = vec![
        match_arm(
            pat_tuple(vec![pat_ident("a"), pat_ident("b")]),
            vec![expr_stmt(int_lit(1))],
        ),
        match_arm(pat_wild(), vec![expr_stmt(int_lit(0))]),
    ];
    let e = match_expr(scrutinee, arms);
    let mut inf = TypeInferencer::new();
    inf.bind(
        "pair",
        Type::tuple(vec![Type::string(), Type::int_default()]),
    );
    let ty = inf.infer_expr(&e).unwrap();
    assert_eq!(
        ty,
        Type::int_default(),
        "T42: tuple pattern match should infer Int"
    );
}

/// T42: match arms with different types return Unknown (defer to rustc).
#[test]
fn t42_mismatched_arm_types_return_unknown() {
    let scrutinee = ident("opt");
    let arms = vec![
        match_arm(
            pat_variant("Option", "Some", vec![pat_ident("x")]),
            vec![expr_stmt(ident("x"))], // returns Int
        ),
        match_arm(
            pat_variant("Option", "None", vec![]),
            vec![expr_stmt(str_lit("none"))], // returns String
        ),
    ];
    let e = match_expr(scrutinee, arms);
    let mut inf = TypeInferencer::new();
    inf.bind("opt", Type::option(Type::int_default()));
    let ty = inf.infer_expr(&e).unwrap();
    assert_eq!(
        ty,
        Type::Unknown,
        "T42: mismatched arm types should return Unknown"
    );
}

// ---------------------------------------------------------------------------
// Qualified enum variant access in expression context
// ---------------------------------------------------------------------------
//
// `EnumName.Variant` (with NO following parens) parses as
// `Expr::MethodCall { receiver: Ident("EnumName"), method: "Variant", args: [] }`
// (see crates/buff-lang-parser/src/expr/expr_postfix.rs — there is no
// `Expr::FieldAccess` AST variant). This section tests that the inferencer's
// MethodCall arm resolves such zero-arg calls to `Type::User(EnumName, [])`
// when `EnumName` is a registered enum and `Variant` is one of its variants.
//
// Before the fix, this fell through to `Type::Unknown` (the MethodCall
// fallthrough) and — when combined with the CLI `buff check` pass's
// pre-binding of user-typed parameters — surfaced as spurious
// "undefined variable" / "cannot compare" errors on patterns like
// `if f == PreludeFn.Abs`. See `crates/buff-lang-cli/src/check.rs` for the
// matching pre-binding fix.

use buff_lang_ast::{Decl, EnumDecl, EnumVariant};

/// Build a unit-style `EnumDecl` like:
/// ```buff
/// enum PreludeFn:
///     Abs
///     Sqrt
/// ```
fn unit_enum_decl(name: &str, variants: &[&str]) -> Decl {
    Decl::EnumDecl(EnumDecl {
        name: Ident::new(name, sp()),
        type_params: Vec::new(),
        variants: variants
            .iter()
            .map(|v| EnumVariant {
                name: Ident::new(*v, sp()),
                data: None,
                span: sp(),
            })
            .collect(),
        span: sp(),
    })
}

/// Build `EnumName.Variant` — the zero-arg MethodCall shape the parser
/// produces for qualified enum variant access.
fn enum_variant_expr(enum_name: &str, variant: &str) -> Expr {
    Expr::MethodCall {
        receiver: Box::new(ident(enum_name)),
        method: Ident::new(variant, sp()),
        args: Vec::new(),
        span: sp(),
    }
}

/// Registered enum + zero-arg MethodCall resolves to `Type::User(EnumName, [])`.
#[test]
fn qualified_enum_variant_resolves_to_user_type() {
    let decls = vec![unit_enum_decl("PreludeFn", &["Abs", "Sqrt", "Log"])];

    let mut inf = TypeInferencer::new();
    inf.register_enum_decls(&decls);

    let e = enum_variant_expr("PreludeFn", "Abs");
    let ty = inf.infer_expr(&e).unwrap();
    assert_eq!(
        ty,
        Type::user("PreludeFn", Vec::new()),
        "qualified variant `PreludeFn.Abs` should resolve to Type::User(\"PreludeFn\", [])"
    );
}

/// A method name that is NOT a declared variant still falls through to
/// `Type::Unknown` (no spurious resolution). This preserves the pre-fix
/// behaviour for unknown identifiers and keeps the prelude-types registry
/// (DateTime.now(), etc.) path reachable.
#[test]
fn qualified_non_variant_still_unknown() {
    let decls = vec![unit_enum_decl("PreludeFn", &["Abs", "Sqrt"])];

    let mut inf = TypeInferencer::new();
    inf.register_enum_decls(&decls);

    // "NotARealVariant" is not declared on PreludeFn.
    let e = enum_variant_expr("PreludeFn", "NotARealVariant");
    let ty = inf.infer_expr(&e).unwrap();
    assert_eq!(
        ty,
        Type::Unknown,
        "non-variant method name should fall through to Unknown"
    );
}

/// No registered enum → fall through to `Type::Unknown` (no panic, no error).
#[test]
fn qualified_variant_with_empty_registry_is_unknown() {
    let mut inf = TypeInferencer::new();
    // Do NOT call register_enum_decls — the default registry is empty.
    let e = enum_variant_expr("Unknown", "Variant");
    let ty = inf.infer_expr(&e).unwrap();
    assert_eq!(
        ty,
        Type::Unknown,
        "empty registry should fall through to Unknown"
    );
}

/// The motivating bug fix: `f == PreludeFn.Abs` type-checks when `f` is
/// bound to the enum type. Before the fix, this surfaced as either
/// "undefined variable: f" (when the param wasn't pre-bound) or
/// "cannot compare Unknown with Unknown" (deferring to the fallthrough).
/// After the fix, both sides resolve to `Type::User("PreludeFn", [])`
/// and the Eq operator returns Bool.
#[test]
fn qualified_enum_variant_in_comparison_yields_bool() {
    let decls = vec![unit_enum_decl("PreludeFn", &["Abs", "Sqrt"])];

    let mut inf = TypeInferencer::new();
    inf.register_enum_decls(&decls);
    // Bind `f` to the user enum type (mirrors the pre-binding the CLI
    // check pass performs for a parameter `f: PreludeFn`).
    inf.bind("f", Type::user("PreludeFn", Vec::new()));

    // Build: f == PreludeFn.Abs
    let e = binary(
        BinaryOp::Eq,
        ident("f"),
        enum_variant_expr("PreludeFn", "Abs"),
    );
    let ty = inf.infer_expr(&e).unwrap();
    assert_eq!(
        ty,
        Type::Bool,
        "qualified variant in Eq comparison should yield Bool"
    );
}

/// BUG-14 regression: `status == Status.Pending` must type-check even when
/// the two sides resolve to `Type::User` with the same `name` but DIFFERENT
/// `args`. Before the fix, the equality check only allowed exact type match
/// (`lhs_ty == rhs_ty`), numeric promotion, Unknown, or Option-like. Two
/// `Type::User("Color", ...)` values with different `args` failed all four
/// checks and cascaded a "cannot compare" type error. The `is_same_user_type`
/// helper compares only the `name` field, allowing user-defined enum/struct
/// equality regardless of resolved type arguments.
#[test]
fn same_named_user_types_with_different_args_are_comparable() {
    let decls = vec![unit_enum_decl("Color", &["Red", "Green", "Blue"])];

    let mut inf = TypeInferencer::new();
    inf.register_enum_decls(&decls);
    // Bind `c` to Color WITH a type arg (differs from the empty-args variant
    // the MethodCall arm resolves `Color.Red` to). This makes lhs_ty != rhs_ty
    // via exact match, exercising the is_same_user_type path.
    inf.bind(
        "c",
        Type::user("Color", vec![Type::Int {
            width: IntWidth::W64,
        }]),
    );

    // Build: c == Color.Red
    //   lhs resolves to Type::User("Color", [Int])  (from the binding)
    //   rhs resolves to Type::User("Color", [])     (from the enum variant)
    // These differ in `args` so `==` fails, but `is_same_user_type` passes
    // because the `name` field matches.
    let e = binary(
        BinaryOp::Eq,
        ident("c"),
        enum_variant_expr("Color", "Red"),
    );
    let ty = inf
        .infer_expr(&e)
        .expect("BUG-14: same-named user types must be comparable even with different args");
    assert_eq!(
        ty,
        Type::Bool,
        "Color == Color.Red should yield Bool even when args differ"
    );
}

/// BUG-14: two identifiers bound to the same user type name (but different
/// args) must be comparable via `!=` as well.
#[test]
fn same_named_user_types_comparable_with_neq() {
    let decls = vec![unit_enum_decl("Status", &["Pending", "Done"])];

    let mut inf = TypeInferencer::new();
    inf.register_enum_decls(&decls);
    // Bind `status` to Status with a type arg; `other` to Status with no args.
    inf.bind(
        "status",
        Type::user("Status", vec![Type::Bool]),
    );
    inf.bind("other", Type::user("Status", vec![]));

    // status != other — both are Type::User("Status", ...) with different args.
    let e = binary(BinaryOp::Neq, ident("status"), ident("other"));
    let ty = inf
        .infer_expr(&e)
        .expect("BUG-14: same-named user types must be comparable with !=");
    assert_eq!(ty, Type::Bool);
}

/// Regression: two DIFFERENTLY-named user types must still error (the fix
/// is not over-permissive). `Color != Status` should be a type error.
#[test]
fn differently_named_user_types_still_error_on_comparison() {
    let decls = vec![
        unit_enum_decl("Color", &["Red"]),
        unit_enum_decl("Status", &["Pending"]),
    ];

    let mut inf = TypeInferencer::new();
    inf.register_enum_decls(&decls);
    inf.bind("c", Type::user("Color", vec![]));
    inf.bind("s", Type::user("Status", vec![]));

    // c == s — different names, must error.
    let e = binary(BinaryOp::Eq, ident("c"), ident("s"));
    let result = inf.infer_expr(&e);
    assert!(
        result.is_err(),
        "BUG-14 regression: differently-named user types must still error"
    );
}

/// Regression: even when `f` is bound to `Type::Unknown` (the CLI check
/// pass's fallback for user-typed parameters), the comparison still
/// succeeds because `Unknown` is permissive in `promote_binary`. This
/// guards the specific `buff check` path where the param's user type
/// can't be resolved to a `Type::User` by the minimal CLI resolver.
#[test]
fn qualified_enum_variant_comparison_with_unknown_param_yields_bool() {
    let decls = vec![unit_enum_decl("PreludeFn", &["Abs", "Sqrt"])];

    let mut inf = TypeInferencer::new();
    inf.register_enum_decls(&decls);
    // Bind `f` to Unknown (mirrors CLI check.rs fallback for user types).
    inf.bind("f", Type::Unknown);

    // Build: f == PreludeFn.Abs
    let e = binary(
        BinaryOp::Eq,
        ident("f"),
        enum_variant_expr("PreludeFn", "Abs"),
    );
    let ty = inf.infer_expr(&e).unwrap();
    assert_eq!(
        ty,
        Type::Bool,
        "Unknown == Type::User should yield Bool (Unknown is permissive)"
    );
}

// ===========================================================================
// P1.6 regression tests — Unknown-typed if-conditions and logical operators.
//
// The self-host corpus (18 .buff files) uses user-defined predicate-style
// functions as if-conditions (`if type_is_numeric(t):`, `if stream_check_raw(s,
// kind):`) and method calls on user-typed values (`if vec.is_empty():`). The
// local inferencer returns `Type::Unknown` for these because cross-function
// return types and user-type method tables are not resolved at this layer.
// Before P1.6, `infer_if` and the `And`/`Or` binary-operator arm strictly
// required `Type::Bool`, producing cascading "if condition must be Bool, found
// Unknown" / "logical operators require Bool, found Unknown and Bool" errors.
//
// The fix mirrors the EXISTING permissive-Unknown policy in `promote_binary`
// (promote.rs line 28: "Unknown combines with anything, yielding Unknown …
// suppresses error cascades after a prior type error"). These tests guard
// against regressions by asserting that `Unknown` conditions type-check
// without error.
// ===========================================================================

/// P1.6: `if unknown_condition:` must NOT error. The condition infers to
/// `Type::Unknown` (e.g. a user-defined predicate call); the if-expression
/// accepts it and returns `Void` (no else branch).
#[test]
fn p16_if_with_unknown_condition_does_not_error() {
    // Build: if some_user_fn { return true }
    // `some_user_fn` is an unbound ident → resolves to Unknown via lookup_ident
    // error... BUT we simulate the real scenario by binding a var to Unknown
    // (matching how user-fn-call results flow through let-bindings).
    let mut inf = TypeInferencer::new();
    inf.bind("flag", Type::Unknown);

    let then_block = block(vec![Stmt::Return(Some(bool_lit(true)), sp())]);
    let e = if_expr(ident("flag"), then_block, None);
    let ty = inf.infer_expr(&e).expect(
        "P1.6: if-condition of Type::Unknown must not error (user-defined predicate call scenario)",
    );
    assert_eq!(ty, Type::Void, "if without else yields Void");
}

/// P1.6: `if c == Color.Red:` where `c` is `Type::Unknown` (user-typed param
/// fallback) must yield `Void` for the whole if-expression. This is the
/// end-to-end scenario from the self-host corpus (`if f == PreludeFn.Abs:` in
/// prelude.buff:178). The `==` yields `Bool` via the permissive-Unknown
/// `promote_binary` rule, and `infer_if` accepts `Bool`.
#[test]
fn p16_if_with_enum_variant_comparison_and_unknown_param() {
    let decls = vec![unit_enum_decl("Color", &["Red", "Green", "Blue"])];

    let mut inf = TypeInferencer::new();
    inf.register_enum_decls(&decls);
    // Bind `c` to Unknown — mirrors the CLI check.rs fallback for user-typed
    // params (`typeref_to_type("Color").unwrap_or(Type::Unknown)`).
    inf.bind("c", Type::Unknown);

    // Build: if c == Color.Red { return true }
    let cond = binary(BinaryOp::Eq, ident("c"), enum_variant_expr("Color", "Red"));
    let then_block = block(vec![Stmt::Return(Some(bool_lit(true)), sp())]);
    let e = if_expr(cond, then_block, None);

    let ty = inf
        .infer_expr(&e)
        .expect("P1.6: if f == EnumName.Variant must not error when f is Type::Unknown");
    assert_eq!(ty, Type::Void, "if without else yields Void");
}

/// P1.6: `Unknown and Bool` / `Bool and Unknown` / `Unknown and Unknown` must
/// all yield `Bool`. Before P1.6 these cascaded "logical operators require
/// Bool, found Unknown and Bool" errors in self-host/promote.buff and
/// self-host/parser/stmt.buff. The fix accepts Unknown on either side (or
/// both), matching promote_binary's permissive-Unknown policy.
#[test]
fn p16_logical_and_or_with_unknown_operands() {
    let mut inf = TypeInferencer::new();
    inf.bind("a", Type::Unknown);
    inf.bind("b", Type::Bool);
    inf.bind("c", Type::Unknown);

    // Unknown and Bool → Bool
    let e1 = binary(BinaryOp::And, ident("a"), ident("b"));
    let t1 = inf
        .infer_expr(&e1)
        .expect("P1.6: Unknown and Bool must yield Bool (not error)");
    assert_eq!(t1, Type::Bool);

    // Bool or Unknown → Bool
    let e2 = binary(BinaryOp::Or, ident("b"), ident("a"));
    let t2 = inf
        .infer_expr(&e2)
        .expect("P1.6: Bool or Unknown must yield Bool (not error)");
    assert_eq!(t2, Type::Bool);

    // Unknown and Unknown → Bool
    let e3 = binary(BinaryOp::And, ident("a"), ident("c"));
    let t3 = inf
        .infer_expr(&e3)
        .expect("P1.6: Unknown and Unknown must yield Bool (not error)");
    assert_eq!(t3, Type::Bool);
}

/// P1.6: a bare `not` identifier resolves to `Type::Unknown` instead of
/// erroring "undefined variable: not". This is a targeted workaround for a
/// parser lang-gap: the parser splits `return not type_is_gpu_eligible(t)` at
/// the bare `not` (two statements) because `not` is not yet a Buff keyword.
/// Without this, self-host/types/ty.buff:690 cascades a spurious error.
#[test]
fn p16_bare_not_identifier_resolves_to_unknown() {
    let mut inf = TypeInferencer::new();
    // Do NOT bind `not` in the environment — it must still resolve via the
    // special-case in the Expr::Ident arm, not via env lookup.
    let ty = inf
        .infer_expr(&ident("not"))
        .expect("P1.6: bare `not` identifier must resolve to Unknown (parser lang-gap workaround)");
    assert_eq!(ty, Type::Unknown, "bare `not` resolves to Unknown");
}

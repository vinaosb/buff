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
                other => panic!(
                    "T83: innermost type must be Int, got {other:?}"
                ),
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
                other => panic!(
                    "T83: innermost value must be Int, got {other:?}"
                ),
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

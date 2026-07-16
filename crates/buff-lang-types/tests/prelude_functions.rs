//! T96 — Standard library prelude: type-inference integration tests.
//!
//! These tests exercise the public API of [`buff_lang_types::prelude`] AND
//! the end-to-end inference flow through [`TypeInferencer::infer_expr`] for
//! every prelude function. They are the T96 "RED → GREEN" evidence: each
//! test was written first (RED), then the prelude registry + inferencer
//! wiring (GREEN) made it pass.
//!
//! ## Running
//!
//! Both selectors find the same tests:
//!
//! ```text
//! cargo test -p buff-lang-types --test prelude_functions
//! cargo test -p buff-lang-types prelude_functions
//! ```
//!
//! (The latter works because every test in this file is named
//! `prelude_functions_*`, so the substring filter matches all of them.)
//!
//! ## Coverage
//!
//! - Math category: `abs` / `min` / `max` / `sqrt` / `floor` / `ceil` /
//!   `round` / `pow` (return-type rules).
//! - Conversion category: `Int` / `Float` / `String` / `Bool` constructors.
//! - I/O category: `print` / `println` (Void) and `read_line` (String).
//! - Implicit availability: prelude functions resolve WITHOUT any `import`
//!   (they are recognised built-in names, not user funcs).
//! - Registry surface: `is_prelude`, `lookup`, `category_of` smoke checks.

use buff_lang_ast::{Expr, Ident, Literal, UnaryOp};
use buff_lang_error::Span;
use buff_lang_types::{
    category_of, is_prelude, lookup, prelude::PreludeCategory, prelude::PreludeFn, Type,
    TypeInferencer,
};

// ---------------------------------------------------------------------------
// Small AST/test helpers.
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

fn string_lit(s: &str) -> Expr {
    Expr::Literal(Literal::String(s.to_string()), sp())
}

fn ident_expr(name: &str) -> Expr {
    Expr::Ident(Ident::new(name, sp()), sp())
}

fn neg(expr: Expr) -> Expr {
    Expr::UnaryOp {
        op: UnaryOp::Neg,
        operand: Box::new(expr),
        span: sp(),
    }
}

/// Build a `name(args...)` FuncCall expression with a bare-Ident callee.
fn call(name: &str, args: Vec<Expr>) -> Expr {
    Expr::FuncCall {
        callee: Box::new(ident_expr(name)),
        args,
        span: sp(),
    }
}

// ---------------------------------------------------------------------------
// 1. abs / min / max — polymorphic math (RED: abs(-5)==5, min(3,7)==3,
//    max(3,7)==7; GREEN: prelude registry resolves their return types.)
// ---------------------------------------------------------------------------

#[test]
fn prelude_functions_abs_of_neg_five_is_int() {
    // RED criterion: `abs(-5)` returns 5 without import. We check the
    // *type* (Int) — the runtime value (5) is Rust's job once codegen
    // lowers `abs(-5)` to `(-5i64).abs()`.
    let mut inf = TypeInferencer::new();
    let expr = call("abs", vec![neg(int_lit(5))]);
    assert_eq!(inf.infer_expr(&expr).unwrap(), Type::int_default());
}

#[test]
fn prelude_functions_abs_is_polymorphic() {
    // abs(Float) -> Float, abs(Double) -> Double, abs(Byte) -> Bits<8>.
    let cases = [
        (float_lit(2.5), Type::float_default()),
        (double_lit(9.0), Type::Double),
        (Expr::Literal(Literal::Byte(0xFF), sp()), Type::byte()),
    ];
    for (arg, expected) in cases {
        let mut inf = TypeInferencer::new();
        let expr = call("abs", vec![arg]);
        assert_eq!(inf.infer_expr(&expr).unwrap(), expected);
    }
}

#[test]
fn prelude_functions_min_three_seven_is_int() {
    // RED criterion: `min(3, 7)` → 3 (Int).
    let mut inf = TypeInferencer::new();
    let expr = call("min", vec![int_lit(3), int_lit(7)]);
    assert_eq!(inf.infer_expr(&expr).unwrap(), Type::int_default());
}

#[test]
fn prelude_functions_max_three_seven_is_int() {
    // RED criterion: `max(3, 7)` → 7 (Int).
    let mut inf = TypeInferencer::new();
    let expr = call("max", vec![int_lit(3), int_lit(7)]);
    assert_eq!(inf.infer_expr(&expr).unwrap(), Type::int_default());
}

#[test]
fn prelude_functions_min_max_widen_mixed_types() {
    // min(Int, Float) -> Float (promote_binary widens).
    let mut inf = TypeInferencer::new();
    let expr = call("min", vec![int_lit(1), float_lit(2.5)]);
    assert_eq!(inf.infer_expr(&expr).unwrap(), Type::float_default());

    // max(Int, Double) -> Double.
    let mut inf = TypeInferencer::new();
    let expr = call("max", vec![int_lit(1), double_lit(2.0)]);
    assert_eq!(inf.infer_expr(&expr).unwrap(), Type::Double);
}

// ---------------------------------------------------------------------------
// 2. Conversions — Int("42"), String(42), Float(..), Bool(..)
//    (RED: Int("42")→42, String(42)→"42".)
// ---------------------------------------------------------------------------

#[test]
fn prelude_functions_int_of_string_returns_int() {
    // RED criterion: `Int("42")` converts String → Int (Type::Int<64>).
    let mut inf = TypeInferencer::new();
    let expr = call("Int", vec![string_lit("42")]);
    assert_eq!(inf.infer_expr(&expr).unwrap(), Type::int_default());
}

#[test]
fn prelude_functions_int_of_float_returns_int() {
    // `Int(2.5)` → Int (regardless of arg type). Avoid the float `3.14`
    // here so clippy's `approx_constant` lint doesn't fire (π ≈ 3.14159).
    let mut inf = TypeInferencer::new();
    let expr = call("Int", vec![float_lit(2.5)]);
    assert_eq!(inf.infer_expr(&expr).unwrap(), Type::int_default());
}

#[test]
fn prelude_functions_string_of_int_returns_string() {
    // RED criterion: `String(42)` converts Int → String.
    let mut inf = TypeInferencer::new();
    let expr = call("String", vec![int_lit(42)]);
    assert_eq!(inf.infer_expr(&expr).unwrap(), Type::string());
}

#[test]
fn prelude_functions_string_of_anything_returns_string() {
    // String(Double), String(Bool) — all become String.
    let mut inf = TypeInferencer::new();
    let expr = call("String", vec![double_lit(1.0)]);
    assert_eq!(inf.infer_expr(&expr).unwrap(), Type::string());

    let mut inf = TypeInferencer::new();
    let expr = call("String", vec![Expr::Literal(Literal::Bool(true), sp())]);
    assert_eq!(inf.infer_expr(&expr).unwrap(), Type::string());
}

#[test]
fn prelude_functions_float_of_int_returns_float() {
    let mut inf = TypeInferencer::new();
    let expr = call("Float", vec![int_lit(42)]);
    assert_eq!(inf.infer_expr(&expr).unwrap(), Type::float_default());
}

#[test]
fn prelude_functions_bool_of_int_returns_bool() {
    let mut inf = TypeInferencer::new();
    let expr = call("Bool", vec![int_lit(1)]);
    assert_eq!(inf.infer_expr(&expr).unwrap(), Type::bool());
}

// ---------------------------------------------------------------------------
// 3. I/O — print/println (Void), read_line (String)
//    (RED: print("hello") generates println!("hello").)
// ---------------------------------------------------------------------------

#[test]
fn prelude_functions_print_returns_void() {
    // print("hello") is a statement-expression; its type is Void. The actual
    // `println!("hello")` codegen is verified in the codegen tests.
    let mut inf = TypeInferencer::new();
    let expr = call("print", vec![string_lit("hello")]);
    assert_eq!(inf.infer_expr(&expr).unwrap(), Type::Void);
}

#[test]
fn prelude_functions_println_returns_void() {
    let mut inf = TypeInferencer::new();
    let expr = call("println", vec![int_lit(42)]);
    assert_eq!(inf.infer_expr(&expr).unwrap(), Type::Void);
}

#[test]
fn prelude_functions_read_line_returns_string() {
    // read_line() takes no args, returns String.
    let mut inf = TypeInferencer::new();
    let expr = call("read_line", vec![]);
    assert_eq!(inf.infer_expr(&expr).unwrap(), Type::string());
}

// ---------------------------------------------------------------------------
// 4. Float-returning math — sqrt / floor / ceil / round
// ---------------------------------------------------------------------------

#[test]
fn prelude_functions_sqrt_returns_float() {
    let mut inf = TypeInferencer::new();
    let expr = call("sqrt", vec![int_lit(16)]);
    assert_eq!(inf.infer_expr(&expr).unwrap(), Type::float_default());

    let mut inf = TypeInferencer::new();
    let expr = call("sqrt", vec![float_lit(2.0)]);
    assert_eq!(inf.infer_expr(&expr).unwrap(), Type::float_default());
}

#[test]
fn prelude_functions_floor_ceil_round_return_float_or_double() {
    // floor(Float) -> Float.
    let mut inf = TypeInferencer::new();
    let expr = call("floor", vec![float_lit(1.5)]);
    assert_eq!(inf.infer_expr(&expr).unwrap(), Type::float_default());

    // ceil(Double) -> Double (width preserved).
    let mut inf = TypeInferencer::new();
    let expr = call("ceil", vec![double_lit(2.5)]);
    assert_eq!(inf.infer_expr(&expr).unwrap(), Type::Double);

    // round(Int) -> Float (int coerced up because Rust's .round() is a
    // float method).
    let mut inf = TypeInferencer::new();
    let expr = call("round", vec![int_lit(5)]);
    assert_eq!(inf.infer_expr(&expr).unwrap(), Type::float_default());
}

// ---------------------------------------------------------------------------
// 5. pow — polymorphic in base type
// ---------------------------------------------------------------------------

#[test]
fn prelude_functions_pow_int_base_returns_int() {
    // pow(Int, Int) -> Int.
    let mut inf = TypeInferencer::new();
    let expr = call("pow", vec![int_lit(2), int_lit(10)]);
    assert_eq!(inf.infer_expr(&expr).unwrap(), Type::int_default());
}

#[test]
fn prelude_functions_pow_float_base_returns_float() {
    // pow(Float, Int) -> Float.
    let mut inf = TypeInferencer::new();
    let expr = call("pow", vec![float_lit(2.0), int_lit(10)]);
    assert_eq!(inf.infer_expr(&expr).unwrap(), Type::float_default());
}

// ---------------------------------------------------------------------------
// 6. Implicit availability — prelude functions resolve WITHOUT an import
//    (RED: all prelude functions available without `import`.)
// ---------------------------------------------------------------------------

#[test]
fn prelude_functions_resolve_without_import() {
    // Every prelude name must resolve through a fresh TypeInferencer
    // (empty environment — no `import` was processed). Each call returns a
    // *concrete* type, NOT Type::Unknown.
    let cases: &[(&str, Vec<Expr>, Type)] = &[
        ("abs", vec![int_lit(-5)], Type::int_default()),
        ("min", vec![int_lit(1), int_lit(2)], Type::int_default()),
        ("max", vec![int_lit(1), int_lit(2)], Type::int_default()),
        ("sqrt", vec![int_lit(4)], Type::float_default()),
        ("floor", vec![float_lit(1.5)], Type::float_default()),
        ("ceil", vec![float_lit(1.5)], Type::float_default()),
        ("round", vec![float_lit(1.5)], Type::float_default()),
        ("pow", vec![int_lit(2), int_lit(3)], Type::int_default()),
        ("Int", vec![string_lit("1")], Type::int_default()),
        ("Float", vec![int_lit(1)], Type::float_default()),
        ("String", vec![int_lit(1)], Type::string()),
        ("Bool", vec![int_lit(1)], Type::bool()),
        ("print", vec![string_lit("x")], Type::Void),
        ("println", vec![string_lit("x")], Type::Void),
        ("read_line", vec![], Type::string()),
    ];
    for (name, args, expected) in cases {
        let mut inf = TypeInferencer::new();
        let expr = call(name, args.clone());
        let got = inf
            .infer_expr(&expr)
            .unwrap_or_else(|e| panic!("infer {name} failed: {e:?}"));
        assert_ne!(got, Type::Unknown, "{name} should not be Unknown");
        assert_eq!(got, *expected, "prelude fn {name}");
    }
}

// ---------------------------------------------------------------------------
// 7. Registry surface — is_prelude / lookup / category_of
// ---------------------------------------------------------------------------

#[test]
fn prelude_functions_registry_lookup_smoke() {
    // Every PreludeFn variant is findable by name.
    for &variant in PreludeFn::ALL {
        let name = variant.name();
        assert!(is_prelude(name), "is_prelude({name:?})");
        assert_eq!(lookup(name), Some(variant), "lookup({name:?})");
        assert!(category_of(name).is_some(), "category_of({name:?})");
    }
    // Unknown names are rejected.
    assert!(!is_prelude("not_in_prelude"));
    assert_eq!(lookup(""), None);
    assert_eq!(category_of("args"), None); // reserved for T99
}

#[test]
fn prelude_functions_categories_partition_correctly() {
    // Spot-check one representative per category.
    assert_eq!(
        lookup("abs").map(PreludeFn::category),
        Some(PreludeCategory::Math)
    );
    assert_eq!(
        lookup("Int").map(PreludeFn::category),
        Some(PreludeCategory::Convert)
    );
    assert_eq!(
        lookup("print").map(PreludeFn::category),
        Some(PreludeCategory::Io)
    );
    assert_eq!(
        lookup("read_line").map(PreludeFn::category),
        Some(PreludeCategory::Io)
    );
}

// ---------------------------------------------------------------------------
// 8. Non-prelude calls still return Unknown (no false positives)
// ---------------------------------------------------------------------------

#[test]
fn prelude_functions_user_func_still_unknown() {
    // A user-defined function name (not in the prelude) is NOT recognised —
    // its call returns Type::Unknown, mirroring the pre-T96 behaviour so
    // existing user-code semantics are unchanged.
    let mut inf = TypeInferencer::new();
    let expr = call("user_defined_fn", vec![int_lit(1)]);
    assert_eq!(inf.infer_expr(&expr).unwrap(), Type::Unknown);
}

#[test]
fn prelude_functions_lowercase_int_is_distinct_from_type_int() {
    // The prelude name "Int" is case-sensitive: a call to "int" (lowercase)
    // is NOT a prelude conversion — it stays Unknown.
    let mut inf = TypeInferencer::new();
    let expr = call("int", vec![string_lit("1")]);
    assert_eq!(inf.infer_expr(&expr).unwrap(), Type::Unknown);
}

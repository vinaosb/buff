//! T22 — Numeric coercion rules (flexible vs fixed Int modes).
//!
//! This is the **integration** test module selected by
//! `cargo test -p buff-lang-types numeric_coercion`. It exercises the
//! public crate API end-to-end across the three T22 concerns:
//!
//! 1. **Widening rules** between primitive numeric categories
//!    (`Int + Float -> Float`, `Float + Double -> Double`). These already
//!    worked via `promote_binary` from T10/T20 — the tests here pin them
//!    as part of T22 acceptance.
//! 2. **Flexible `Int` auto-width** — value `5 -> Int<8>`, value `300 ->
//!    Int<16>` — via the new `range_analysis` module. Includes the
//!    "x = 127; y = x + 1 -> Int<16>" widening walk.
//! 3. **Fixed `Int<W>` preservation** — `Int<32> + Int<32> -> Int<32>` and
//!    `Int<8>` stays `Int<8>` on every operator (the predicate the codegen
//!    layer relies on to map `i8`/`i32` and to inherit Rust's debug-panic /
//!    release-wrap overflow behaviour for free).
//!
//! Collection auto-width (`[20,25,18] -> Vector<Int<8>>`) is tested as a
//! **pure helper** here (`collection_int_width(&[20,25,18]) == W8`). End-to
//! end collection-literal inference is deferred to T23/T67 — see
//! `decisions.md` §T22 — because the AST has no array-literal expression
//! until that task lands.

use buff_lang_ast::{BinaryOp, Expr, Ident, Literal, Stmt, TypeRef, UnaryOp};
use buff_lang_error::Span;
use buff_lang_types::{
    collection_int_width, promote_binary, smallest_int_width, IntRange, IntWidth, Type,
    TypeInferencer,
};

// ---------------------------------------------------------------------------
// Small AST/test helpers (mirror the shape used in infer_tests.rs).
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

fn ident_expr(name: &str) -> Expr {
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

fn int_w(w: IntWidth) -> Type {
    Type::Int { width: w }
}

// ---------------------------------------------------------------------------
// 1. Cross-category widening rules (Int<->Float<->Double).
// ---------------------------------------------------------------------------

#[test]
fn int_plus_float_widens_to_float() {
    // T22 RED: `Int + Float -> Float`.
    let lhs = Type::int_default(); // Int<64>
    let rhs = Type::float_default(); // Float<32>
    assert_eq!(promote_binary(&lhs, &rhs), Some(Type::float_default()));
}

#[test]
fn float_plus_int_widens_to_float() {
    // Operand order is irrelevant — Float wins either side.
    let lhs = Type::float_default();
    let rhs = Type::int_default();
    assert_eq!(promote_binary(&lhs, &rhs), Some(Type::float_default()));
}

#[test]
fn float_plus_double_widens_to_double() {
    // T22 RED: `Float + Double -> Double`.
    let lhs = Type::float_default();
    let rhs = Type::double();
    assert_eq!(promote_binary(&lhs, &rhs), Some(Type::double()));
}

#[test]
fn int_plus_double_widens_to_double() {
    // The full chain: Int jumps over Float straight to Double.
    let lhs = Type::int_default();
    let rhs = Type::double();
    assert_eq!(promote_binary(&lhs, &rhs), Some(Type::double()));
}

#[test]
fn double_dominates_wide_int() {
    // Even Int<128> is dominated by Double (not the other way around).
    let lhs = int_w(IntWidth::W128);
    let rhs = Type::double();
    assert_eq!(promote_binary(&lhs, &rhs), Some(Type::double()));
}

#[test]
fn nested_arithmetic_widens_through_categories() {
    // `(1 + 2.0) + 3.0d` -> Double (re-derivation of T18 result in T22 terms).
    let mut inf = TypeInferencer::new();
    let inner = binary(BinaryOp::Add, int_lit(1), float_lit(2.0));
    let outer = binary(BinaryOp::Add, inner, double_lit(3.0));
    assert_eq!(inf.infer_expr(&outer).unwrap(), Type::double());
}

// ---------------------------------------------------------------------------
// 2. Flexible `Int` auto-width via range analysis.
// ---------------------------------------------------------------------------

#[test]
fn flexible_value_5_is_int8() {
    // T22 RED: flexible Int value 5 -> Int<8> (smallest fitting width).
    assert_eq!(smallest_int_width(5, 5), IntWidth::W8);
    assert_eq!(IntRange::exact(5).width(), IntWidth::W8);
}

#[test]
fn flexible_value_300_is_int16() {
    // T22 RED: value 300 -> Int<16>.
    assert_eq!(smallest_int_width(300, 300), IntWidth::W16);
}

#[test]
fn flexible_value_100000_is_int32() {
    assert_eq!(smallest_int_width(100_000, 100_000), IntWidth::W32);
}

#[test]
fn flexible_i8_boundary_127_vs_128() {
    // The exact boundary the T22 plan calls out: i8 max is 127.
    assert_eq!(smallest_int_width(127, 127), IntWidth::W8);
    assert_eq!(smallest_int_width(128, 128), IntWidth::W16);
    // And the negative side: i8 min is -128.
    assert_eq!(smallest_int_width(-128, -128), IntWidth::W8);
    assert_eq!(smallest_int_width(-129, -129), IntWidth::W16);
}

#[test]
fn flexible_widening_on_add_127_plus_1_is_int16() {
    // T22 RED (flexible widening): `x = 127; y = x + 1` widens y's tracked
    // range to [128, 128] which no longer fits i8 — y must become Int<16>.
    let x = IntRange::exact(127);
    let y = x + IntRange::exact(1);
    assert_eq!(y, IntRange::exact(128));
    assert_eq!(y.width(), IntWidth::W16);
}

#[test]
fn flexible_widening_on_sub_minus_128_minus_1_is_int16() {
    // Symmetric negative-side widening: x = -128; y = x - 1 -> -129 -> Int<16>.
    let x = IntRange::exact(-128);
    let y = x - IntRange::exact(1);
    assert_eq!(y, IntRange::exact(-129));
    assert_eq!(y.width(), IntWidth::W16);
}

#[test]
fn flexible_negation_swaps_range_bounds() {
    // `x = 10..20` (e.g. from an if/else join); `-x` is `-20..-10`.
    let r = IntRange::new(10, 20);
    assert_eq!(-r, IntRange::new(-20, -10));
    // The negated range still fits i8 (|20| <= 127).
    assert_eq!((-r).width(), IntWidth::W8);
}

#[test]
fn flexible_union_picks_outer_bounds() {
    // if/else join: `let x = if c { 1 } else { 1000 }` -> union [1,5] U [900,1000]
    // = [1, 1000] which needs i16.
    let a = IntRange::new(1, 5);
    let b = IntRange::new(900, 1000);
    let joined = a.union(b);
    assert_eq!(joined, IntRange::new(1, 1000));
    assert_eq!(joined.width(), IntWidth::W16);
}

// ---------------------------------------------------------------------------
// 3. Collection auto-width (pure helper, end-to-end deferred to T23/T67).
// ---------------------------------------------------------------------------

#[test]
fn collection_helper_20_25_18_is_int8() {
    // T22 RED: collection auto-width — [20, 25, 18] -> Int<8>.
    // End-to-end `[...] -> Vector<Int<8>>` requires the T23/T67
    // array-literal AST node, which does not exist yet; the helper is the
    // pure-function foundation T23/T67 will call.
    assert_eq!(collection_int_width(&[20, 25, 18]), IntWidth::W8);
}

#[test]
fn collection_helper_large_pair_is_int32() {
    assert_eq!(collection_int_width(&[100_000, 200_000]), IntWidth::W32);
}

#[test]
fn collection_helper_negative_min_drives_width() {
    // [-200, 5]: min -200 needs i16, max 5 fits i8 -> i16 wins.
    assert_eq!(collection_int_width(&[-200, 5]), IntWidth::W16);
}

#[test]
fn collection_helper_empty_is_int64_default() {
    // An empty collection falls back to Buff's default Int width so it still
    // type-checks against a plain `Int` element type.
    assert_eq!(collection_int_width(&[]), IntWidth::W64);
}

// ---------------------------------------------------------------------------
// 4. Fixed-mode `Int<W>` preservation on every operator.
// ---------------------------------------------------------------------------

#[test]
fn fixed_int32_plus_int32_is_int32() {
    // T22 RED: `Int<32> + Int<32> -> Int<32>` (fixed mode preserves type).
    let lhs = int_w(IntWidth::W32);
    let rhs = int_w(IntWidth::W32);
    assert_eq!(promote_binary(&lhs, &rhs), Some(int_w(IntWidth::W32)));
}

#[test]
fn fixed_int8_plus_int8_is_int8() {
    // Narrowest fixed width is preserved on Add.
    let lhs = int_w(IntWidth::W8);
    let rhs = int_w(IntWidth::W8);
    assert_eq!(promote_binary(&lhs, &rhs), Some(int_w(IntWidth::W8)));
}

#[test]
fn fixed_int_width_preserved_on_every_arith_op() {
    // Add / Sub / Mul / Div / Mod all preserve Int<32>.
    let a = int_w(IntWidth::W32);
    for op in [
        BinaryOp::Add,
        BinaryOp::Sub,
        BinaryOp::Mul,
        BinaryOp::Div,
        BinaryOp::Mod,
    ] {
        let mut inf = TypeInferencer::new();
        inf.bind("a", a.clone());
        inf.bind("b", a.clone());
        let e = binary(op, ident_expr("a"), ident_expr("b"));
        assert_eq!(
            inf.infer_expr(&e).unwrap(),
            a,
            "operator {op:?} should preserve Int<32>"
        );
    }
}

#[test]
fn fixed_int_width_preserved_on_bitwise_ops() {
    // BitAnd / BitOr / BitXor / Shl / Shr all preserve Int<32>.
    let a = int_w(IntWidth::W32);
    for op in [
        BinaryOp::BitAnd,
        BinaryOp::BitOr,
        BinaryOp::BitXor,
        BinaryOp::Shl,
        BinaryOp::Shr,
    ] {
        let mut inf = TypeInferencer::new();
        inf.bind("a", a.clone());
        inf.bind("b", a.clone());
        let e = binary(op, ident_expr("a"), ident_expr("b"));
        assert_eq!(
            inf.infer_expr(&e).unwrap(),
            a,
            "bitwise op {op:?} should preserve Int<32>"
        );
    }
}

#[test]
fn fixed_int8_negation_preserves_width() {
    // Unary `-` on an `Int<8>` value yields `Int<8>` (signed negation is
    // in-width; for the i8 corner `-128` Rust's debug-build panic is the
    // T22 overflow contract, inherited for free).
    let mut inf = TypeInferencer::new();
    inf.bind("x", int_w(IntWidth::W8));
    let e = Expr::UnaryOp {
        op: UnaryOp::Neg,
        operand: Box::new(ident_expr("x")),
        span: sp(),
    };
    assert_eq!(inf.infer_expr(&e).unwrap(), int_w(IntWidth::W8));
}

#[test]
fn fixed_mixed_widths_pick_max() {
    // Int<8> + Int<32> -> Int<32> (max-width rule; fixed mode just means
    // "explicit width is honoured", not "no promotion").
    let lhs = int_w(IntWidth::W8);
    let rhs = int_w(IntWidth::W32);
    assert_eq!(promote_binary(&lhs, &rhs), Some(int_w(IntWidth::W32)));
    // And reversed.
    assert_eq!(promote_binary(&rhs, &lhs), Some(int_w(IntWidth::W32)));
}

// ---------------------------------------------------------------------------
// 5. Overflow contract reminder (documented, not runtime-tested here).
// ---------------------------------------------------------------------------

#[test]
fn fixed_overflow_mode_documentation() {
    // T22 spec: "overflow in fixed mode -> panic in debug, wrap in release".
    //
    // Buff inherits this BEHAVIOUR FOR FREE from Rust: codegen maps Int<8>
    // to i8 (verified in `rust_codegen.rs::buff_type_to_syn`), and Rust's
    // native `+`/`-`/`*` operators already panic on overflow in debug and
    // wrap in release. No explicit `checked_add` is emitted — the simplest
    // correct path. The codegen test in `buff-lang-codegen-rust` (see
    // T22 evidence `task-22-overflow-modes.txt`) asserts the i8 mapping
    // end-to-end so the contract is mechanically pinned.
    //
    // This test exists to name the contract in the numeric_coercion module
    // and to assert the WIDTH INVARIANT the contract depends on: fixed
    // Int<8> must STAY Int<8> through arithmetic (no silent widening that
    // would change the overflow boundary).
    let i8_ty = int_w(IntWidth::W8);
    assert_eq!(promote_binary(&i8_ty, &i8_ty), Some(i8_ty.clone()));
    assert_eq!(
        promote_binary(&i8_ty, &int_w(IntWidth::W16)),
        Some(int_w(IntWidth::W16))
    );
}

// ---------------------------------------------------------------------------
// 6. Let-decl integration: fixed annotation overrides inference.
// ---------------------------------------------------------------------------

#[test]
fn let_with_int8_annotation_pins_int8() {
    // `let x: Int<8> = 5` — when the user writes a fixed annotation, that
    // width wins (the value 5 would infer i8 anyway, but the *type* is the
    // annotation, not the inference). We assert the bound type is Int<8>,
    // which is what codegen maps to Rust i8.
    //
    // NOTE: typeref_to_type("Int") currently returns int_default() (Int<64>);
    // the parser does not yet produce `Int<8>` TypeRefs (T11 limitation).
    // So this test seeds the binding directly via `bind`, which is what the
    // future `Int<8>` annotation lowering will produce.
    let mut inf = TypeInferencer::new();
    let i8_ty = int_w(IntWidth::W8);
    inf.bind("x", i8_ty.clone());
    assert_eq!(inf.lookup("x"), Some(&i8_ty));

    // Arithmetic on the pinned binding preserves Int<8>.
    let e = binary(BinaryOp::Add, ident_expr("x"), ident_expr("x"));
    assert_eq!(inf.infer_expr(&e).unwrap(), i8_ty);
}

#[test]
fn let_decl_int_annotation_uses_int64_default() {
    // `let x: Int = 5` — the plain `Int` annotation resolves to the default
    // width Int<64>, regardless of how small the value is. This is the
    // *fixed-mode* (explicit annotation) behaviour; *flexible* mode (no
    // annotation) is where range analysis narrows to Int<8>.
    let stmt = Stmt::LetDecl {
        name: Ident::new("x", sp()),
        value: int_lit(5),
        mutable: false,
        ty: Some(TypeRef::Named {
            name: Ident::new("Int", sp()),
            span: sp(),
        }),
        span: sp(),
    };
    let mut inf = TypeInferencer::new();
    let ty = inf.infer_stmt(&stmt).unwrap();
    assert_eq!(ty, Type::int_default()); // Int<64>, NOT Int<8>.
    assert_eq!(inf.lookup("x"), Some(&Type::int_default()));
}

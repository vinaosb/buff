//! Range analysis for flexible-mode Buff `Int` variables (T22).
//!
//! In **flexible mode** a plain `let x: Int = …` (no width annotation) does
//! not lock the variable to `Int<64>`; instead the compiler tracks the
//! possible `(min, max)` range of the variable as arithmetic widens it, and
//! picks the smallest Rust integer width that can hold the whole range. So
//! `let x = 5` infers `Int<8>` (i8), but `let x = 300` infers `Int<16>`
//! (i16), and `let y = x + 1` widens the tracked range so `y` may land in a
//! larger width than `x`.
//!
//! In **fixed mode** — an explicit `let x: Int<8>` — the width is preserved
//! across all operations and the value uses Rust's native fixed-width
//! arithmetic, which already panics on overflow in debug builds and wraps in
//! release builds (no explicit `checked_*` calls needed).
//!
//! This module is **pure** — it knows nothing about the AST. The inference
//! pass (and T23/T67 collection-literal lowering) will call into it.
//!
//! # API
//!
//! - [`IntRange`] — `(min, max)` interval tracker with widening arithmetic.
//! - [`smallest_int_width`] — pick the smallest signed [`IntWidth`] that fits
//!   a closed `[min, max]` range.
//! - [`collection_int_width`] — convenience: smallest width fitting every
//!   value in a slice (the helper T23/T67 will call when lowering
//!   `[20, 25, 18]` -> `Vector<Int<8>>`).

use crate::ty::IntWidth;
use std::ops::{Add, Mul, Neg, Sub};

/// A closed signed-integer interval `[min, max]` used by flexible-mode
/// range analysis.
///
/// Invariant: `min <= max`. The interval is stored as `i128` so it can
/// losslessly represent the bounds of every signed Rust integer width up to
/// `i128` itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IntRange {
    /// Inclusive lower bound.
    pub min: i128,
    /// Inclusive upper bound.
    pub max: i128,
}

impl IntRange {
    /// Creates a new range. `min` may be greater than `max`; the values are
    /// swapped internally to preserve the `min <= max` invariant.
    pub fn new(min: i128, max: i128) -> Self {
        if min <= max {
            Self { min, max }
        } else {
            Self { min: max, max: min }
        }
    }

    /// A singleton range `[v, v]` — used for `let x = 5` (literal init).
    pub fn exact(v: i128) -> Self {
        Self { min: v, max: v }
    }

    /// Returns the smallest signed [`IntWidth`] that can hold this entire
    /// range. See [`smallest_int_width`].
    pub fn width(self) -> IntWidth {
        smallest_int_width(self.min, self.max)
    }

    /// Widen (union) this range with `other`: the result covers both
    /// intervals. Used for `if/else` join points and array literals.
    pub fn union(self, other: Self) -> Self {
        Self::new(self.min.min(other.min), self.max.max(other.max))
    }

    /// Interval subtraction: `[a.min - b.max, a.max - b.min]`.
    pub fn sub_interval(self, other: Self) -> Self {
        let lo = self.min.saturating_sub(other.max);
        let hi = self.max.saturating_sub(other.min);
        Self::new(lo, hi)
    }

    /// Interval multiplication: covers the min and max of the four corner
    /// products. This is the standard sound interval-multiplication rule.
    pub fn mul_interval(self, other: Self) -> Self {
        let c1 = self.min.saturating_mul(other.min);
        let c2 = self.min.saturating_mul(other.max);
        let c3 = self.max.saturating_mul(other.min);
        let c4 = self.max.saturating_mul(other.max);
        Self::new(c1.min(c2).min(c3).min(c4), c1.max(c2).max(c3).max(c4))
    }
}

// ---------------------------------------------------------------------------
// Operator-trait impls (idiomatic Rust: `range1 + range2`, `-range`).
// Clippy's `should_implement_trait` prefers these over inherent methods
// whose names collide with the std ops.
// ---------------------------------------------------------------------------

impl Add for IntRange {
    type Output = Self;
    /// Interval addition: `[a.min + b.min, a.max + b.max]`.
    ///
    /// Saturates to `i128` bounds — practical ranges never reach there for
    /// any real Buff program (a `let y = x + 1` widening step moves the
    /// endpoints by tiny amounts).
    fn add(self, other: Self) -> Self {
        Self::new(
            self.min.saturating_add(other.min),
            self.max.saturating_add(other.max),
        )
    }
}

impl Sub for IntRange {
    type Output = Self;
    /// Interval subtraction via [`IntRange::sub_interval`].
    fn sub(self, other: Self) -> Self {
        self.sub_interval(other)
    }
}

impl Mul for IntRange {
    type Output = Self;
    /// Interval multiplication via [`IntRange::mul_interval`].
    fn mul(self, other: Self) -> Self {
        self.mul_interval(other)
    }
}

impl Neg for IntRange {
    type Output = Self;
    /// Interval negation: `[-max, -min]`.
    fn neg(self) -> Self {
        Self::new(self.max.saturating_neg(), self.min.saturating_neg())
    }
}

/// Smallest signed Rust integer width that can hold every value in the
/// closed interval `[min, max]`.
///
/// Returns [`IntWidth::W128`] if the range exceeds `i64`, and `W128` again
/// if it exceeds `i128` (the largest signed width Buff tracks; values beyond
/// `i128` are unreachable from Buff source).
///
/// # Examples (T22 acceptance)
///
/// ```
/// use buff_lang_types::IntWidth;
/// use buff_lang_types::range_analysis::smallest_int_width;
/// assert_eq!(smallest_int_width(5, 5), IntWidth::W8);    // value 5  -> i8
/// assert_eq!(smallest_int_width(0, 300), IntWidth::W16); // value 300 -> i16
/// assert_eq!(smallest_int_width(0, 100_000), IntWidth::W32);
/// assert_eq!(smallest_int_width(0, i128::MAX), IntWidth::W128);
/// ```
pub fn smallest_int_width(min: i128, max: i128) -> IntWidth {
    // Order the bounds defensively — public callers may pass them swapped.
    let (lo, hi) = if min <= max { (min, max) } else { (max, min) };

    // Signed Rust widths and their inclusive ranges:
    //   i8   : [-128, 127]
    //   i16  : [-32_768, 32_767]
    //   i32  : [-2_147_483_648, 2_147_483_647]
    //   i64  : [-9_223_372_036_854_775_808, 9_223_372_036_854_775_807]
    //   i128 : covers every remaining i128 value.
    if (i8::MIN as i128..=i8::MAX as i128).contains(&lo)
        && (i8::MIN as i128..=i8::MAX as i128).contains(&hi)
    {
        IntWidth::W8
    } else if (i16::MIN as i128..=i16::MAX as i128).contains(&lo)
        && (i16::MIN as i128..=i16::MAX as i128).contains(&hi)
    {
        IntWidth::W16
    } else if (i32::MIN as i128..=i32::MAX as i128).contains(&lo)
        && (i32::MIN as i128..=i32::MAX as i128).contains(&hi)
    {
        IntWidth::W32
    } else if (i64::MIN as i128..=i64::MAX as i128).contains(&lo)
        && (i64::MIN as i128..=i64::MAX as i128).contains(&hi)
    {
        IntWidth::W64
    } else {
        // Anything else (the bounds fit in i128 by construction since the
        // inputs are i128) lands in the widest Buff width.
        IntWidth::W128
    }
}

/// Smallest signed Rust integer width that can hold **every** value in the
/// slice. The helper T23/T67 (collection literals) will call when lowering
/// e.g. `[20, 25, 18]` -> `Vector<Int<8>>` — it computes the min/max across
/// the literal element values and forwards to [`smallest_int_width`].
///
/// Returns [`IntWidth::W64`] (Buff's default Int width) for an empty slice,
/// matching `Int::int_default()` so an empty collection still type-checks
/// against a plain `Int` element.
pub fn collection_int_width(values: &[i128]) -> IntWidth {
    if values.is_empty() {
        return IntWidth::W64;
    }
    let lo = values.iter().copied().min().unwrap_or(i128::MIN);
    let hi = values.iter().copied().max().unwrap_or(i128::MAX);
    smallest_int_width(lo, hi)
}

// ---------------------------------------------------------------------------
// Unit tests — T22 RED/GREEN for the pure range-analysis primitives.
// The cross-cutting `numeric_coercion` integration tests live in
// `tests/numeric_coercion.rs` and exercise the public crate API end-to-end.
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn singleton_value_5_is_w8() {
        assert_eq!(IntRange::exact(5).width(), IntWidth::W8);
    }

    #[test]
    fn singleton_value_300_is_w16() {
        assert_eq!(IntRange::exact(300).width(), IntWidth::W16);
    }

    #[test]
    fn value_127_is_w8_value_128_is_w16() {
        // i8 max is 127 — the classic boundary the T22 plan calls out.
        assert_eq!(smallest_int_width(127, 127), IntWidth::W8);
        assert_eq!(smallest_int_width(128, 128), IntWidth::W16);
    }

    #[test]
    fn value_negative_129_is_w16() {
        // i8 min is -128, so -129 must widen to i16.
        assert_eq!(smallest_int_width(-129, -129), IntWidth::W16);
        assert_eq!(smallest_int_width(-128, -128), IntWidth::W8);
    }

    #[test]
    fn value_100000_is_w32() {
        assert_eq!(smallest_int_width(100_000, 100_000), IntWidth::W32);
    }

    #[test]
    fn collection_20_25_18_is_w8() {
        // The T22 acceptance example: [20, 25, 18] -> Int<8>.
        assert_eq!(collection_int_width(&[20, 25, 18]), IntWidth::W8);
    }

    #[test]
    fn collection_100000_200000_is_w32() {
        assert_eq!(collection_int_width(&[100_000, 200_000]), IntWidth::W32);
    }

    #[test]
    fn collection_empty_is_w64_default() {
        // Empty slice falls back to Buff's default Int width.
        assert_eq!(collection_int_width(&[]), IntWidth::W64);
    }

    #[test]
    fn collection_with_negative_uses_min_corner() {
        // [-200, 5] -> min is -200 (needs i16), max is 5 (fits i8); i16 wins.
        assert_eq!(collection_int_width(&[-200, 5]), IntWidth::W16);
    }

    #[test]
    fn range_add_widens_correctly() {
        // x = 127 (i8), y = x + 1 -> [128, 128] -> i16.  This is the T22
        // "flexible widening" example: `let y = x + 1` widens the range.
        let x = IntRange::exact(127);
        let y = x + IntRange::exact(1);
        assert_eq!(y, IntRange::exact(128));
        assert_eq!(y.width(), IntWidth::W16);
    }

    #[test]
    fn range_sub_widens_into_negative() {
        // x = -128 (i8), y = x - 1 -> [-129, -129] -> i16.
        let x = IntRange::exact(-128);
        let y = x - IntRange::exact(1);
        assert_eq!(y, IntRange::exact(-129));
        assert_eq!(y.width(), IntWidth::W16);
    }

    #[test]
    fn range_neg_swaps_bounds() {
        let r = IntRange::new(10, 20);
        assert_eq!(-r, IntRange::new(-20, -10));
    }

    #[test]
    fn range_union_of_intervals() {
        let a = IntRange::new(1, 5);
        let b = IntRange::new(-3, 2);
        assert_eq!(a.union(b), IntRange::new(-3, 5));
    }

    #[test]
    fn range_mul_corners() {
        // [-2, 3] * [-4, 5]:
        //   corners: (-2*-4=8), (-2*5=-10), (3*-4=-12), (3*5=15)
        //   -> [-12, 15] -> i8 fits
        let r = IntRange::new(-2, 3) * IntRange::new(-4, 5);
        assert_eq!(r, IntRange::new(-12, 15));
        assert_eq!(r.width(), IntWidth::W8);
    }
}

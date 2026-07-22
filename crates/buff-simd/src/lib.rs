//! `buff-simd` — first-class SIMD types for the Buff language
//! (Mojo-inspired).
//!
//! Pure-Rust MVP wrapping the [`wide`](https://crates.io/crates/wide)
//! crate's portable stable-SIMD types. NO nightly `std::simd`, NO
//! runtime `is_x86_feature_detected!` detection (compile-time target
//! features only — per T54 spec), NO GPU dispatch (that's WGSL's job).
//!
//! # Why explicit SIMD?
//!
//! Buff's auto-vectorizer remains the default for non-explicit code.
//! `Simd<T, N>` is for hand-optimized hot loops where the
//! auto-vectorizer misses (4-8x speedup ceiling for compute frameworks:
//! buff-tensor T8, buff-science T13, buff-ml T15, buff-image T9,
//! buff-dsp T11).
//!
//! # Pipeline
//!
//! ```text
//!   Simd.splat(x) ─────────────────────────────────┐
//!   Simd.from_slice([a, b, c, d]) ─────────────────┤
//!   Simd.from_array([a, b, c, d]) ─────────────────┤
//!                                                  ▼
//!                                       Simd { wide::f32x4 }
//!                                                  │
//!                                                  ├─ simd.add(other)
//!                                                  ├─ simd.sub(other)
//!                                                  ├─ simd.mul(other)
//!                                                  ├─ simd.div(other)
//!                                                  ├─ simd.sum()
//!                                                  ├─ simd.min()
//!                                                  ├─ simd.max()
//!                                                  └─ simd.to_vec()
//!                                                  ▼
//!                                          f32 / Vec<f32>
//! ```
//!
//! # Lane width
//!
//! The MVP is **fixed at 4 lanes** (`f32x4` — a single 128-bit SSE /
//! NEON register). The conceptual `Simd<Float, 4>` maps 1:1 to this
//! type. Wider registers (`f32x8` = 256-bit AVX, `f32x16` = 512-bit
//! AVX-512) are a v1.20+ enhancement; the `wide` crate already supports
//! them but the Buff surface + codegen would need the generic `N`
//! parameter plumbed through `Type::Simd` + the prelude registry.
//!
//! # Panic-free contract
//!
//! No `unwrap` / `expect` / `panic!` / `todo!` / `unimplemented!` in
//! non-test code. `Simd::from_slice` returns `Result`; every other
//! operation is infallible (4-wide register arithmetic never traps).

pub mod error;

pub use error::SimdError;

use wide::f32x4;

/// The fixed lane count of the MVP `Simd` type (`f32x4` = 4 lanes).
pub const LANES: usize = 4;

/// A SIMD register holding 4 `f32` lanes — the concrete realisation of
/// the conceptual `Simd<Float, 4>`.
///
/// Wraps [`wide::f32x4`] (a 128-bit SSE / NEON register). Constructed
/// via [`Simd::splat`] (broadcast), [`Simd::from_slice`] (fallible —
/// length-checked), or [`Simd::from_array`] (infallible). Instance
/// methods: `add` / `sub` / `mul` / `div` (lane-wise binary),
/// `sum` / `min` / `max` (horizontal reductions), `to_vec` (extract).
///
/// `Copy` + `Clone` + `Debug` + `PartialEq` — the underlying `f32x4`
/// is a value type with no interior mutability. `Default` splats `0.0`.
///
/// # Auto-vectorizer vs explicit SIMD
///
/// Buff's auto-vectorizer handles plain `Vector<Float>` loops where it
/// can. `Simd` is the escape hatch for hot loops the vectorizer misses
/// (gather / scatter / non-contiguous access patterns / explicit
/// horizontal reductions). See `benches/dot_product.rs` for the
/// ≥3x speedup demonstration.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Simd(pub(crate) f32x4);

impl Simd {
    /// Broadcast a scalar `f32` to all 4 lanes.
    ///
    /// `Simd.splat(5.0)` → `[5.0, 5.0, 5.0, 5.0]`. Infallible.
    #[inline]
    #[must_use]
    pub fn splat(x: f32) -> Self {
        Simd(f32x4::splat(x))
    }

    /// Construct from a flat slice of at least 4 `f32` values.
    ///
    /// Reads the first 4 elements; returns
    /// [`SimdError::LengthMismatch`] if the slice is too short and
    /// [`SimdError::NonFinite`] if any of the first 4 elements is
    /// NaN / infinite. The codegen layer wraps this in
    /// `.unwrap_or_default()` so the Buff surface stays panic-free
    /// (a too-short slice yields `Simd::splat(0.0)`).
    #[inline]
    pub fn from_slice(slice: &[f32]) -> Result<Self, SimdError> {
        if slice.len() < LANES {
            return Err(SimdError::LengthMismatch {
                got: slice.len(),
                need: LANES,
            });
        }
        let arr = [slice[0], slice[1], slice[2], slice[3]];
        for (idx, &v) in arr.iter().enumerate() {
            if !v.is_finite() {
                return Err(SimdError::NonFinite { idx });
            }
        }
        Ok(Simd::from_array(arr))
    }

    /// Construct from a fixed-size 4-element array. Infallible.
    #[inline]
    #[must_use]
    pub fn from_array(arr: [f32; LANES]) -> Self {
        Simd(f32x4::new(arr))
    }

    /// Lane-wise addition: `[a0+b0, a1+b1, a2+b2, a3+b3]`.
    #[inline]
    #[must_use]
    pub fn add(self, other: Simd) -> Simd {
        Simd(self.0 + other.0)
    }

    /// Lane-wise subtraction: `[a0-b0, a1-b1, a2-b2, a3-b3]`.
    #[inline]
    #[must_use]
    pub fn sub(self, other: Simd) -> Simd {
        Simd(self.0 - other.0)
    }

    /// Lane-wise multiplication: `[a0*b0, a1*b1, a2*b2, a3*b3]`.
    #[inline]
    #[must_use]
    pub fn mul(self, other: Simd) -> Simd {
        Simd(self.0 * other.0)
    }

    /// Lane-wise division: `[a0/b0, a1/b1, a2/b2, a3/b3]`.
    #[inline]
    #[must_use]
    pub fn div(self, other: Simd) -> Simd {
        Simd(self.0 / other.0)
    }

    /// Horizontal sum: `a0 + a1 + a2 + a3`.
    ///
    /// This is the canonical dot-product reduction. Implemented via
    /// `to_array()` + explicit add chain so the result is
    /// deterministic across architectures (the `wide` crate's
    /// `reduce_sum` uses architecture-specific reduction orderings
    /// that can differ by 1 ULP).
    #[inline]
    #[must_use]
    pub fn sum(self) -> f32 {
        let a = self.0.to_array();
        a[0] + a[1] + a[2] + a[3]
    }

    /// Horizontal minimum: `min(a0, a1, a2, a3)`.
    ///
    /// Returns the smallest lane value. NaN-aware (a NaN lane
    /// propagates per `f32::min` semantics).
    #[inline]
    #[must_use]
    pub fn min(self) -> f32 {
        let a = self.0.to_array();
        a.iter().copied().fold(f32::INFINITY, f32::min)
    }

    /// Horizontal maximum: `max(a0, a1, a2, a3)`.
    ///
    /// Returns the largest lane value. NaN-aware (a NaN lane
    /// propagates per `f32::max` semantics).
    #[inline]
    #[must_use]
    pub fn max(self) -> f32 {
        let a = self.0.to_array();
        a.iter().copied().fold(f32::NEG_INFINITY, f32::max)
    }

    /// Lane-wise minimum against another `Simd`:
    /// `[min(a0,b0), min(a1,b1), min(a2,b2), min(a3,b3)]`.
    #[inline]
    #[must_use]
    pub fn lane_min(self, other: Simd) -> Simd {
        Simd(self.0.min(other.0))
    }

    /// Lane-wise maximum against another `Simd`:
    /// `[max(a0,b0), max(a1,b1), max(a2,b2), max(a3,b3)]`.
    #[inline]
    #[must_use]
    pub fn lane_max(self, other: Simd) -> Simd {
        Simd(self.0.max(other.0))
    }

    /// Extract the 4 lanes to a `Vec<f32>` (length 4).
    #[inline]
    #[must_use]
    pub fn to_vec(self) -> Vec<f32> {
        self.0.to_array().to_vec()
    }

    /// Extract the 4 lanes to a fixed-size array.
    #[inline]
    #[must_use]
    pub fn to_array(self) -> [f32; LANES] {
        self.0.to_array()
    }
}

impl Default for Simd {
    fn default() -> Self {
        Simd::splat(0.0)
    }
}

impl std::fmt::Display for Simd {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let a = self.0.to_array();
        write!(f, "Simd({}, {}, {}, {})", a[0], a[1], a[2], a[3])
    }
}

impl From<[f32; LANES]> for Simd {
    fn from(arr: [f32; LANES]) -> Self {
        Simd::from_array(arr)
    }
}

/// 4-lane dot product: `(a0*b0) + (a1*b1) + (a2*b2) + (a3*b3)`.
///
/// The canonical SIMD reduction — `sum` of `mul`. Exposed as a free
/// function (NOT a method) so the benchmark harness can compare it
/// against a scalar loop without constructing two `Simd` values inline.
/// Used by `benches/dot_product.rs` for the ≥3x speedup demonstration.
#[inline]
#[must_use]
pub fn dot(a: Simd, b: Simd) -> f32 {
    a.mul(b).sum()
}

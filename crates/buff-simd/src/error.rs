//! Error type for the `buff-simd` crate.
//!
//! All fallible operations surface as [`SimdError`]. The crate's only
//! fallible constructor (`Simd::from_slice`) maps a length mismatch into
//! this enum so the public surface depends only on `buff-simd`'s own
//! types (Buff code never sees a raw `wide::*` type).
//!
//! # Panic-free contract
//!
//! No `unwrap` / `expect` / `panic!` / `todo!` / `unimplemented!` in
//! this module or any non-test code path. The `Simd` type wraps `wide`
//! operations that are themselves infallible (arithmetic on 4-wide
//! registers never traps); the only fallible entry point is the
//! slice-length-checked constructor.

use thiserror::Error;

#[derive(Debug, Error)]
pub enum SimdError {
    /// `Simd.from_slice(slice)` called with a slice shorter than the
    /// register lane count (4 for `f32x4`). The MVP is fixed-width —
    /// a future `Simd<T, N>` generic would relax this to `N`.
    #[error("simd slice too short: got {got} elements, need at least {need}")]
    LengthMismatch { got: usize, need: usize },

    /// `Simd.from_slice(slice)` called with a slice containing a
    /// non-finite value (NaN / +inf / -inf). SIMD reductions
    /// (`sum` / `min` / `max`) propagate non-finite values; we surface
    /// the explicit error so the Buff user sees their bug at construction
    /// time rather than as a silent NaN cascade.
    #[error("simd slice contains non-finite value at index {idx}")]
    NonFinite { idx: usize },
}

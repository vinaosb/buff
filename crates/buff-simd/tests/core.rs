//! Integration tests for the `buff-simd` crate (T54).
//!
//! Covers all 11 public functions per the T54 spec:
//! - Simd: splat, from_slice, from_array, add, sub, mul, div, sum,
//!   min, max, to_vec (+ lane_min / lane_max / to_array helpers).
//! - dot (free fn).
//!
//! Per T54 acceptance: "Simd<Float, 4> operations produce correct
//! results vs scalar equivalents. Benchmark shows >=3x speedup on dot
//! product vs scalar loop. 4 examples + 15 tests." — we ship 17 unit
//! tests + 4 insta snapshots (above the floor).

use buff_simd::{dot, Simd, SimdError, LANES};

fn approx_eq(a: f32, b: f32, tol: f32) -> bool {
    (a - b).abs() < tol
}

fn approx_eq_slice(a: &[f32], b: &[f32], tol: f32) -> bool {
    a.len() == b.len() && a.iter().zip(b).all(|(x, y)| approx_eq(*x, *y, tol))
}

#[test]
fn splat_broadcasts_scalar_to_all_lanes() {
    let s = Simd::splat(5.0);
    assert!(approx_eq_slice(&s.to_vec(), &[5.0, 5.0, 5.0, 5.0], 1e-6));
}

#[test]
fn from_array_round_trips_four_lanes() {
    let s = Simd::from_array([1.0, 2.0, 3.0, 4.0]);
    assert!(approx_eq_slice(&s.to_vec(), &[1.0, 2.0, 3.0, 4.0], 1e-6));
}

#[test]
fn from_slice_exact_length_succeeds() {
    let s = Simd::from_slice(&[10.0, 20.0, 30.0, 40.0]).expect("4-elem slice");
    assert!(approx_eq_slice(
        &s.to_vec(),
        &[10.0, 20.0, 30.0, 40.0],
        1e-6
    ));
}

#[test]
fn from_slice_longer_takes_first_four() {
    let s = Simd::from_slice(&[1.0, 2.0, 3.0, 4.0, 5.0, 6.0]).expect("6-elem slice");
    assert!(approx_eq_slice(&s.to_vec(), &[1.0, 2.0, 3.0, 4.0], 1e-6));
}

#[test]
fn from_slice_too_short_returns_length_mismatch() {
    let err = Simd::from_slice(&[1.0, 2.0, 3.0]).unwrap_err();
    assert!(matches!(err, SimdError::LengthMismatch { got: 3, need: 4 }));
}

#[test]
fn from_slice_empty_returns_length_mismatch() {
    let err = Simd::from_slice(&[]).unwrap_err();
    assert!(matches!(
        err,
        SimdError::LengthMismatch {
            got: 0,
            need: LANES
        }
    ));
}

#[test]
fn from_slice_non_finite_returns_error() {
    let err = Simd::from_slice(&[1.0, f32::NAN, 3.0, 4.0]).unwrap_err();
    assert!(matches!(err, SimdError::NonFinite { idx: 1 }));
    let err2 = Simd::from_slice(&[1.0, 2.0, f32::INFINITY, 4.0]).unwrap_err();
    assert!(matches!(err2, SimdError::NonFinite { idx: 2 }));
}

#[test]
fn add_is_lane_wise() {
    let a = Simd::from_array([1.0, 2.0, 3.0, 4.0]);
    let b = Simd::from_array([10.0, 20.0, 30.0, 40.0]);
    let r = a.add(b);
    assert!(approx_eq_slice(
        &r.to_vec(),
        &[11.0, 22.0, 33.0, 44.0],
        1e-6
    ));
}

#[test]
fn sub_is_lane_wise() {
    let a = Simd::from_array([10.0, 20.0, 30.0, 40.0]);
    let b = Simd::from_array([1.0, 2.0, 3.0, 4.0]);
    let r = a.sub(b);
    assert!(approx_eq_slice(&r.to_vec(), &[9.0, 18.0, 27.0, 36.0], 1e-6));
}

#[test]
fn mul_is_lane_wise() {
    let a = Simd::from_array([1.0, 2.0, 3.0, 4.0]);
    let b = Simd::from_array([2.0, 3.0, 4.0, 5.0]);
    let r = a.mul(b);
    assert!(approx_eq_slice(&r.to_vec(), &[2.0, 6.0, 12.0, 20.0], 1e-6));
}

#[test]
fn div_is_lane_wise() {
    let a = Simd::from_array([10.0, 20.0, 30.0, 40.0]);
    let b = Simd::from_array([2.0, 4.0, 5.0, 10.0]);
    let r = a.div(b);
    assert!(approx_eq_slice(&r.to_vec(), &[5.0, 5.0, 6.0, 4.0], 1e-6));
}

#[test]
fn sum_reduces_horizontally() {
    let s = Simd::from_array([1.0, 2.0, 3.0, 4.0]);
    assert!(approx_eq(s.sum(), 10.0, 1e-6));
}

#[test]
fn min_finds_smallest_lane() {
    let s = Simd::from_array([3.0, -1.0, 4.0, 1.0]);
    assert!(approx_eq(s.min(), -1.0, 1e-6));
}

#[test]
fn max_finds_largest_lane() {
    let s = Simd::from_array([3.0, -1.0, 4.0, 1.0]);
    assert!(approx_eq(s.max(), 4.0, 1e-6));
}

#[test]
fn lane_min_and_lane_max_are_element_wise() {
    let a = Simd::from_array([1.0, 5.0, 3.0, 2.0]);
    let b = Simd::from_array([2.0, 3.0, 2.0, 4.0]);
    assert!(approx_eq_slice(
        &a.lane_min(b).to_vec(),
        &[1.0, 3.0, 2.0, 2.0],
        1e-6
    ));
    assert!(approx_eq_slice(
        &a.lane_max(b).to_vec(),
        &[2.0, 5.0, 3.0, 4.0],
        1e-6
    ));
}

#[test]
fn dot_matches_scalar_computation() {
    let a = Simd::from_array([1.0, 2.0, 3.0, 4.0]);
    let b = Simd::from_array([5.0, 6.0, 7.0, 8.0]);
    let simd_dot = dot(a, b);
    let scalar_dot = 1.0 * 5.0 + 2.0 * 6.0 + 3.0 * 7.0 + 4.0 * 8.0;
    assert!(approx_eq(simd_dot, scalar_dot, 1e-5));
    assert!(approx_eq(simd_dot, 70.0, 1e-5));
}

#[test]
fn default_is_zero_splat() {
    let d = Simd::default();
    assert!(approx_eq_slice(&d.to_vec(), &[0.0, 0.0, 0.0, 0.0], 1e-6));
}

#[test]
fn from_array_trait_conversion_works() {
    let s: Simd = [1.5, -2.5, 3.5, -4.5].into();
    assert!(approx_eq_slice(&s.to_vec(), &[1.5, -2.5, 3.5, -4.5], 1e-6));
}

// ---- Insta snapshots (4+) ---------------------------------------------------

#[test]
fn snapshot_simd_display() {
    let s = Simd::from_array([1.5, -2.5, 3.0, 4.25]);
    insta::assert_snapshot!("simd_display", format!("{s}"));
}

#[test]
fn snapshot_simd_default_display() {
    let s = Simd::default();
    insta::assert_snapshot!("simd_default_display", format!("{s}"));
}

#[test]
fn snapshot_simd_error_all_variants() {
    let e1 = SimdError::LengthMismatch { got: 2, need: 4 };
    let e2 = SimdError::NonFinite { idx: 3 };
    insta::assert_snapshot!("simd_error_all", format!("{e1}\n{e2}"));
}

#[test]
fn snapshot_simd_dot_product_chain() {
    let a = Simd::from_array([1.0, 2.0, 3.0, 4.0]);
    let b = Simd::from_array([2.0, 2.0, 2.0, 2.0]);
    let c = Simd::from_array([1.0, 1.0, 1.0, 1.0]);
    let chain = dot(a, b) + dot(b, c);
    insta::assert_snapshot!("simd_dot_chain", format!("{chain}"));
}

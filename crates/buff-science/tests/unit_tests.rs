//! Unit tests for buff-science.

use buff_science::{interp, linalg, ode, optimize, stats};
use buff_tensor::Tensor;

// ============================================================
// linalg tests
// ============================================================

#[test]
fn test_matmul_2x2() {
    let a = Tensor::from_vec(vec![1.0, 2.0, 3.0, 4.0], vec![2, 2]).unwrap();
    let b = Tensor::from_vec(vec![5.0, 6.0, 7.0, 8.0], vec![2, 2]).unwrap();
    let c = linalg::matmul(&a, &b).unwrap();
    assert_eq!(c.as_slice(), &[19.0, 22.0, 43.0, 50.0]);
}

#[test]
fn test_matmul_identity() {
    let a = Tensor::from_vec(vec![1.0, 2.0, 3.0, 4.0], vec![2, 2]).unwrap();
    let identity = Tensor::from_vec(vec![1.0, 0.0, 0.0, 1.0], vec![2, 2]).unwrap();
    let c = linalg::matmul(&a, &identity).unwrap();
    assert_eq!(c.as_slice(), &[1.0, 2.0, 3.0, 4.0]);
}

#[test]
fn test_transpose_2x3() {
    let t = Tensor::from_vec(vec![1.0, 2.0, 3.0, 4.0, 5.0, 6.0], vec![2, 3]).unwrap();
    let tt = linalg::transpose(&t).unwrap();
    assert_eq!(tt.shape().as_slice(), &[3, 2]);
    assert_eq!(tt.as_slice(), &[1.0, 4.0, 2.0, 5.0, 3.0, 6.0]);
}

#[test]
fn test_inverse_2x2() {
    let m = Tensor::from_vec(vec![4.0, 7.0, 2.0, 6.0], vec![2, 2]).unwrap();
    let inv = linalg::inverse(&m).unwrap();
    let product = linalg::matmul(&m, &inv).unwrap();
    // Check product is approximately identity.
    // f32 has ~7 significant digits; 1e-5 tolerance is appropriate.
    let slice = product.as_slice();
    assert!((slice[0] - 1.0).abs() < 1e-5, "diag[0,0] = {}", slice[0]);
    assert!((slice[3] - 1.0).abs() < 1e-5, "diag[1,1] = {}", slice[3]);
    assert!(slice[1].abs() < 1e-5, "off-diag[0,1] = {}", slice[1]);
    assert!(slice[2].abs() < 1e-5, "off-diag[1,0] = {}", slice[2]);
}

#[test]
fn test_inverse_3x3() {
    let m = Tensor::from_vec(
        vec![4.0, 7.0, 3.0, 2.0, 6.0, 5.0, 1.0, 1.0, 1.0],
        vec![3, 3],
    )
    .unwrap();
    let inv = linalg::inverse(&m).unwrap();
    let product = linalg::matmul(&m, &inv).unwrap();
    let slice = product.as_slice();
    // f32 has ~7 significant digits; 1e-5 tolerance is appropriate.
    for i in 0..3 {
        for j in 0..3 {
            let expected = if i == j { 1.0 } else { 0.0 };
            assert!(
                (slice[i * 3 + j] - expected).abs() < 1e-5,
                "m * m_inv [{i},{j}] = {} expected {expected}",
                slice[i * 3 + j]
            );
        }
    }
}

#[test]
fn test_determinant_2x2() {
    let m = Tensor::from_vec(vec![4.0, 7.0, 2.0, 6.0], vec![2, 2]).unwrap();
    let det = linalg::determinant(&m).unwrap();
    assert!((det - 10.0).abs() < 1e-10);
}

#[test]
fn test_determinant_3x3() {
    let m = Tensor::from_vec(
        vec![6.0, 1.0, 1.0, 4.0, -2.0, 5.0, 2.0, 8.0, 7.0],
        vec![3, 3],
    )
    .unwrap();
    let det = linalg::determinant(&m).unwrap();
    // det = 6*(-2*7 - 5*8) - 1*(4*7 - 5*2) + 1*(4*8 - (-2)*2)
    //      = 6*(-14 - 40) - 1*(28 - 10) + 1*(32 + 4)
    //      = 6*(-54) - 18 + 36 = -324 - 18 + 36 = -306
    assert!((det - (-306.0)).abs() < 1e-10);
}

#[test]
fn test_solve_2x2() {
    // Solve [2 1; 1 3] * x = [5; 7]. Exact: x = [1.6; 1.8]
    let a = Tensor::from_vec(vec![2.0, 1.0, 1.0, 3.0], vec![2, 2]).unwrap();
    let b = Tensor::from_vec(vec![5.0, 7.0], vec![2, 1]).unwrap();
    let x = linalg::solve(&a, &b).unwrap();
    // f32 has ~7 significant digits; 1e-5 tolerance is appropriate.
    let slice = x.as_slice();
    assert!((slice[0] - 1.6).abs() < 1e-5, "x[0] = {}", slice[0]);
    assert!((slice[1] - 1.8).abs() < 1e-5, "x[1] = {}", slice[1]);
}

#[test]
fn test_singular_matrix() {
    let m = Tensor::from_vec(vec![1.0, 2.0, 2.0, 4.0], vec![2, 2]).unwrap();
    let result = linalg::inverse(&m);
    assert!(result.is_err());
}

// ============================================================
// ode tests
// ============================================================

#[test]
fn test_rk4_exponential() {
    // Solve dy/dt = y, y(0) = 1. Exact: y(1) = e.
    let result = ode::rk4(|_t, y| y, 1.0, 0.0, 1.0, 0.01);
    assert!((result - std::f64::consts::E).abs() < 1e-4);
}

#[test]
fn test_rk4_linear() {
    // Solve dy/dt = 1, y(0) = 0. Exact: y(1) = 1.
    let result = ode::rk4(|_t, _y| 1.0, 0.0, 0.0, 1.0, 0.01);
    assert!((result - 1.0).abs() < 1e-10);
}

#[test]
fn test_rk4_vec_system() {
    // Solve dx/dt = y, dy/dt = -x (harmonic oscillator).
    // x(0) = 1, y(0) = 0. At t = 2*pi, x should be ~1.
    let result = ode::rk4_vec(
        |_t, state| vec![state[1], -state[0]],
        vec![1.0, 0.0],
        0.0,
        2.0 * std::f64::consts::PI,
        0.001,
    );
    assert!((result[0] - 1.0).abs() < 0.01);
    assert!(result[1].abs() < 0.01);
}

// ============================================================
// interp tests
// ============================================================

#[test]
fn test_interp_linear_exact() {
    let xs = vec![0.0, 1.0, 2.0];
    let ys = vec![0.0, 10.0, 20.0];
    assert!((interp::linear(&xs, &ys, 0.5).unwrap() - 5.0).abs() < 1e-10);
}

#[test]
fn test_interp_linear_boundary() {
    let xs = vec![0.0, 1.0, 2.0];
    let ys = vec![0.0, 10.0, 20.0];
    assert!((interp::linear(&xs, &ys, 0.0).unwrap() - 0.0).abs() < 1e-10);
    assert!((interp::linear(&xs, &ys, 2.0).unwrap() - 20.0).abs() < 1e-10);
}

#[test]
fn test_interp_linear_out_of_range() {
    let xs = vec![0.0, 1.0];
    let ys = vec![0.0, 10.0];
    assert!(interp::linear(&xs, &ys, -1.0).is_err());
    assert!(interp::linear(&xs, &ys, 2.0).is_err());
}

#[test]
fn test_interp_empty() {
    assert!(interp::linear(&[], &[], 0.0).is_err());
}

// ============================================================
// optimize tests
// ============================================================

#[test]
fn test_gradient_descent_quadratic() {
    // Minimize f(x) = x^2. Gradient = 2x.
    let result = optimize::gradient_descent(
        |x| {
            let v = x[0];
            (v * v, vec![2.0 * v])
        },
        vec![5.0],
        0.1,
        100,
    )
    .unwrap();
    assert!(result[0].abs() < 0.01);
}

#[test]
fn test_gradient_descent_2d() {
    // Minimize f(x, y) = x^2 + y^2.
    let result = optimize::gradient_descent(
        |x| {
            let val = x[0] * x[0] + x[1] * x[1];
            (val, vec![2.0 * x[0], 2.0 * x[1]])
        },
        vec![3.0, -4.0],
        0.1,
        200,
    )
    .unwrap();
    assert!(result[0].abs() < 0.01);
    assert!(result[1].abs() < 0.01);
}

#[test]
fn test_gradient_descent_empty() {
    let result = optimize::gradient_descent(|_| (0.0, vec![]), vec![], 0.1, 10);
    assert!(result.is_err());
}

// ============================================================
// stats tests
// ============================================================

#[test]
fn test_mean() {
    let data = vec![1.0, 2.0, 3.0, 4.0, 5.0];
    assert!((stats::mean(&data).unwrap() - 3.0).abs() < 1e-10);
}

#[test]
fn test_mean_empty() {
    assert!(stats::mean(&[]).is_err());
}

#[test]
fn test_variance() {
    let data = vec![1.0, 2.0, 3.0, 4.0, 5.0];
    // Population variance = 2.0
    assert!((stats::variance(&data).unwrap() - 2.0).abs() < 1e-10);
}

#[test]
fn test_stddev() {
    let data = vec![1.0, 2.0, 3.0, 4.0, 5.0];
    assert!((stats::stddev(&data).unwrap() - 2.0_f64.sqrt()).abs() < 1e-10);
}

#[test]
fn test_correlation_perfect_positive() {
    let x = vec![1.0, 2.0, 3.0, 4.0, 5.0];
    let y = vec![2.0, 4.0, 6.0, 8.0, 10.0];
    assert!((stats::correlation(&x, &y).unwrap() - 1.0).abs() < 1e-10);
}

#[test]
fn test_correlation_perfect_negative() {
    let x = vec![1.0, 2.0, 3.0, 4.0, 5.0];
    let y = vec![10.0, 8.0, 6.0, 4.0, 2.0];
    assert!((stats::correlation(&x, &y).unwrap() - (-1.0)).abs() < 1e-10);
}

#[test]
fn test_histogram_basic() {
    let data = vec![1.0, 2.0, 3.0, 4.0, 5.0];
    let h = stats::histogram(&data, 5).unwrap();
    // Each bin should have exactly 1 entry.
    for count in h.values() {
        assert_eq!(*count, 1);
    }
}

#[test]
fn test_histogram_single_bin() {
    let data = vec![1.0, 2.0, 3.0];
    let h = stats::histogram(&data, 1).unwrap();
    assert_eq!(h.get(&0), Some(&3));
}

#[test]
fn test_histogram_empty() {
    assert!(stats::histogram(&[], 5).is_err());
}

// ============================================================
// proptest tests (numerical stability)
// ============================================================

use proptest::prelude::*;

proptest! {
    #[test]
    fn proptest_matmul_identity(a in (-10.0f64..10.0), b in (-10.0f64..10.0),
                                c in (-10.0f64..10.0), d in (-10.0f64..10.0)) {
        // matmul with identity should return same matrix.
        let vals = vec![a as f32, b as f32, c as f32, d as f32];
        let m = Tensor::from_vec(vals, vec![2, 2]).unwrap();
        let identity = Tensor::from_vec(vec![1.0, 0.0, 0.0, 1.0], vec![2, 2]).unwrap();
        let result = linalg::matmul(&m, &identity).unwrap();
        let orig = m.as_slice();
        let res = result.as_slice();
        for i in 0..4 {
            assert!((orig[i] - res[i]).abs() < 1e-5, "matmul identity failed at {i}");
        }
    }

    #[test]
    fn proptest_inverse_roundtrip(a in -10.0f64..10.0, b in -10.0f64..10.0,
                                  c in -10.0f64..10.0, d in -10.0f64..10.0) {
        // For non-singular 2x2 matrices, m * m^-1 ~= I.
        // f32 has ~7 significant digits; 1e-3 tolerance handles poorly-conditioned cases.
        let det = a * d - b * c;
        prop_assume!(det.abs() > 0.5);  // Filter to well-conditioned matrices only.
        let m = Tensor::from_vec(vec![a as f32, b as f32, c as f32, d as f32], vec![2, 2]).unwrap();
        let inv = linalg::inverse(&m).unwrap();
        let product = linalg::matmul(&m, &inv).unwrap();
        let s = product.as_slice();
        assert!((s[0] - 1.0).abs() < 1e-3, "inverse roundtrip diag[0,0] = {}", s[0]);
        assert!((s[3] - 1.0).abs() < 1e-3, "inverse roundtrip diag[1,1] = {}", s[3]);
        assert!(s[1].abs() < 1e-3, "inverse roundtrip off-diag[0,1] = {}", s[1]);
        assert!(s[2].abs() < 1e-3, "inverse roundtrip off-diag[1,0] = {}", s[2]);
    }

    #[test]
    fn proptest_rk4_accuracy(scale in 0.1f64..5.0) {
        // Solve dy/dt = scale * y, y(0) = 1. Exact: y(1) = e^scale.
        let result = ode::rk4(move |_t, y| scale * y, 1.0, 0.0, 1.0, 0.001);
        let expected = scale.exp();
        assert!((result - expected).abs() < 0.001,
            "rk4 accuracy: got {result}, expected {expected}");
    }
}

// ============================================================
// insta snapshot tests (5)
// ============================================================

use std::fmt::Write;

fn format_tensor(t: &Tensor) -> String {
    let mut s = String::new();
    let _ = write!(s, "shape: {:?}\ndata: [", t.shape().as_slice());
    for (i, &v) in t.as_slice().iter().enumerate() {
        if i > 0 {
            let _ = write!(s, ", ");
        }
        let _ = write!(s, "{v:.6}");
    }
    let _ = write!(s, "]");
    s
}

#[test]
fn snap_inverse_3x3() {
    let m = Tensor::from_vec(
        vec![4.0, 7.0, 3.0, 2.0, 6.0, 5.0, 1.0, 1.0, 1.0],
        vec![3, 3],
    )
    .unwrap();
    let inv = linalg::inverse(&m).unwrap();
    insta::assert_snapshot!("inverse_3x3", format_tensor(&inv));
}

#[test]
fn snap_determinant_3x3() {
    let m = Tensor::from_vec(
        vec![6.0, 1.0, 1.0, 4.0, -2.0, 5.0, 2.0, 8.0, 7.0],
        vec![3, 3],
    )
    .unwrap();
    let det = linalg::determinant(&m).unwrap();
    insta::assert_snapshot!("determinant_3x3", format!("{det:.6}"));
}

#[test]
fn snap_rk4_exponential() {
    let result = ode::rk4(|_t, y| y, 1.0, 0.0, 1.0, 0.01);
    insta::assert_snapshot!("rk4_exponential", format!("{result:.6}"));
}

#[test]
fn snap_stats_mean_variance() {
    let data = vec![1.0, 2.0, 3.0, 4.0, 5.0];
    let m = stats::mean(&data).unwrap();
    let v = stats::variance(&data).unwrap();
    insta::assert_snapshot!("stats_mean_variance", format!("mean={m:.6}, var={v:.6}"));
}

#[test]
fn snap_correlation_perfect() {
    let x = vec![1.0, 2.0, 3.0, 4.0, 5.0];
    let y = vec![2.0, 4.0, 6.0, 8.0, 10.0];
    let r = stats::correlation(&x, &y).unwrap();
    insta::assert_snapshot!("correlation_perfect", format!("{r:.6}"));
}

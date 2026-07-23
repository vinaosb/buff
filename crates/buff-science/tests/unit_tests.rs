//! Integration tests for buff-science.
//!
//! 15+ tests including proptest for numerical stability.

use buff_science::{interp, linalg, ode, optimize, stats};
use buff_tensor::Tensor;
use proptest::prelude::*;

// ===========================================================================
// Linalg tests
// ===========================================================================

#[test]
fn linalg_matmul_basic() {
    let a = Tensor::from_vec(vec![1.0, 2.0, 3.0, 4.0], vec![2, 2]).unwrap();
    let b = Tensor::from_vec(vec![5.0, 6.0, 7.0, 8.0], vec![2, 2]).unwrap();
    let c = linalg::matmul(&a, &b).unwrap();
    assert_eq!(c.as_slice(), &[19.0, 22.0, 43.0, 50.0]);
}

#[test]
fn linalg_transpose_basic() {
    let t = Tensor::from_vec(vec![1.0, 2.0, 3.0, 4.0, 5.0, 6.0], vec![2, 3]).unwrap();
    let tt = linalg::transpose(&t).unwrap();
    assert_eq!(tt.shape().as_slice(), &[3, 2]);
    assert_eq!(tt.as_slice(), &[1.0, 4.0, 2.0, 5.0, 3.0, 6.0]);
}

#[test]
fn linalg_inverse_3x3() {
    // 3x3 invertible matrix.
    let m = Tensor::from_vec(
        vec![2.0, 1.0, 1.0, 1.0, 3.0, 2.0, 1.0, 0.0, 0.0],
        vec![3, 3],
    )
    .unwrap();
    let inv = linalg::inverse(&m).unwrap();
    // Verify m * m^-1 ≈ I
    let product = linalg::matmul(&m, &inv).unwrap();
    let data = product.as_slice();
    for r in 0..3 {
        for c in 0..3 {
            let expected = if r == c { 1.0 } else { 0.0 };
            assert!(
                (data[r * 3 + c] - expected).abs() < 1e-4,
                "product[{},{}]={}, expected {}",
                r,
                c,
                data[r * 3 + c],
                expected
            );
        }
    }
}

#[test]
fn linalg_inverse_singular() {
    // Singular matrix (row 2 = row 1).
    let m = Tensor::from_vec(
        vec![1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0, 9.0],
        vec![3, 3],
    )
    .unwrap();
    let result = linalg::inverse(&m);
    assert!(result.is_err());
}

#[test]
fn linalg_determinant_2x2() {
    let m = Tensor::from_vec(vec![3.0, 8.0, 4.0, 6.0], vec![2, 2]).unwrap();
    let det = linalg::determinant(&m).unwrap();
    assert!((det - (-14.0)).abs() < 1e-6);
}

#[test]
fn linalg_solve_basic() {
    // Solve 2x + y = 5, x + 3y = 7
    let a = Tensor::from_vec(vec![2.0, 1.0, 1.0, 3.0], vec![2, 2]).unwrap();
    let b = Tensor::from_vec(vec![5.0, 7.0], vec![2, 1]).unwrap();
    let x = linalg::solve(&a, &b).unwrap();
    // x ≈ [1.6, 1.8]
    assert!((x.as_slice()[0] - 1.6).abs() < 1e-4);
    assert!((x.as_slice()[1] - 1.8).abs() < 1e-4);
}

// ===========================================================================
// ODE tests
// ===========================================================================

#[test]
fn ode_rk4_exponential() {
    // Solve dy/dt = y, y(0) = 1. Solution: y(t) = e^t.
    let result = ode::rk4(|_t, y| y, 1.0, 0.0, 1.0, 0.001);
    assert!(
        (result - std::f64::consts::E).abs() < 1e-4,
        "RK4 exponential: got {}, expected e ≈ {}",
        result,
        std::f64::consts::E
    );
}

#[test]
fn ode_rk4_linear() {
    // Solve dy/dt = 1, y(0) = 0. Solution: y(t) = t.
    let result = ode::rk4(|_t, _y| 1.0, 0.0, 0.0, 5.0, 0.1);
    assert!((result - 5.0).abs() < 1e-6);
}

#[test]
fn ode_rk4_vec_system() {
    // Simple harmonic oscillator: dy1/dt = y2, dy2/dt = -y1
    // y1(0) = 0, y2(0) = 1 => y1(t) = sin(t)
    let f = |_t: f64, y: &[f64]| vec![y[1], -y[0]];
    let result = ode::rk4_vec(f, vec![0.0, 1.0], 0.0, std::f64::consts::FRAC_PI_2, 0.001);
    assert!(
        (result[0] - 1.0).abs() < 1e-3,
        "sin(pi/2) ≈ 1, got {}",
        result[0]
    );
}

// ===========================================================================
// Interpolation tests
// ===========================================================================

#[test]
fn interp_linear_basic() {
    let xs = vec![0.0, 1.0, 2.0, 3.0];
    let ys = vec![0.0, 10.0, 20.0, 30.0];
    let y = interp::linear(&xs, &ys, 1.5).unwrap();
    assert!((y - 15.0).abs() < 1e-10);
}

#[test]
fn interp_linear_endpoints() {
    let xs = vec![0.0, 1.0];
    let ys = vec![0.0, 10.0];
    assert!((interp::linear(&xs, &ys, 0.0).unwrap() - 0.0).abs() < 1e-10);
    assert!((interp::linear(&xs, &ys, 1.0).unwrap() - 10.0).abs() < 1e-10);
}

#[test]
fn interp_linear_out_of_range() {
    let xs = vec![0.0, 1.0];
    let ys = vec![0.0, 10.0];
    assert!(interp::linear(&xs, &ys, -1.0).is_err());
    assert!(interp::linear(&xs, &ys, 2.0).is_err());
}

// ===========================================================================
// Optimization tests
// ===========================================================================

#[test]
fn optimize_gradient_descent_quadratic() {
    // Minimize f(x) = x^2. Minimum at x = 0.
    let result =
        optimize::gradient_descent(|x| x[0] * x[0], |x| vec![2.0 * x[0]], vec![5.0], 0.1, 100);
    assert!(
        result[0].abs() < 0.01,
        "gradient descent on x^2: got {}",
        result[0]
    );
}

// ===========================================================================
// Stats tests
// ===========================================================================

#[test]
fn stats_mean_basic() {
    let data = vec![1.0, 2.0, 3.0, 4.0, 5.0];
    assert!((stats::mean(&data).unwrap() - 3.0).abs() < 1e-10);
}

#[test]
fn stats_variance_basic() {
    let data = vec![1.0, 2.0, 3.0, 4.0, 5.0];
    assert!((stats::variance(&data).unwrap() - 2.0).abs() < 1e-10);
}

#[test]
fn stats_stddev_basic() {
    let data = vec![1.0, 2.0, 3.0, 4.0, 5.0];
    assert!((stats::stddev(&data).unwrap() - 2.0_f64.sqrt()).abs() < 1e-10);
}

#[test]
fn stats_correlation_perfect() {
    let x = vec![1.0, 2.0, 3.0, 4.0, 5.0];
    let y = vec![2.0, 4.0, 6.0, 8.0, 10.0];
    let corr = stats::correlation(&x, &y).unwrap();
    assert!((corr - 1.0).abs() < 1e-10);
}

#[test]
fn stats_correlation_negative() {
    let x = vec![1.0, 2.0, 3.0, 4.0, 5.0];
    let y = vec![10.0, 8.0, 6.0, 4.0, 2.0];
    let corr = stats::correlation(&x, &y).unwrap();
    assert!((corr - (-1.0)).abs() < 1e-10);
}

#[test]
fn stats_histogram_basic() {
    let data = vec![1.0, 2.0, 3.0, 4.0, 5.0];
    let hist = stats::histogram(&data, 5).unwrap();
    // Each value should be in its own bin (uniform distribution).
    assert_eq!(hist.len(), 5);
}

#[test]
fn stats_empty_error() {
    assert!(stats::mean(&[]).is_err());
    assert!(stats::variance(&[1.0]).is_err());
}

// ===========================================================================
// Proptest: numerical stability
// ===========================================================================

proptest! {
    #[test]
    fn proptest_matmul_identity(a in prop::collection::vec(-100.0f64..100.0, 4)) {
        // a * I = a (for 2x2 matrices)
        let mat_a = Tensor::from_vec(a.iter().map(|v| *v as f32).collect(), vec![2, 2]).unwrap();
        let identity = Tensor::from_vec(vec![1.0, 0.0, 0.0, 1.0], vec![2, 2]).unwrap();
        let result = linalg::matmul(&mat_a, &identity).unwrap();
        for (got, want) in result.as_slice().iter().zip(mat_a.as_slice().iter()) {
            prop_assert!((got - want).abs() < 1e-4, "matmul identity failed: got {got}, want {want}");
        }
    }

    #[test]
    fn proptest_inverse_roundtrip(m0 in -10.0f64..10.0, m1 in -10.0f64..10.0,
                                   m2 in -10.0f64..10.0, m3 in -10.0f64..10.0) {
        // For 2x2 diagonal-ish matrices, verify m * m^-1 ≈ I
        let det = m0 * m3 - m1 * m2;
        prop_assume!(det.abs() > 0.1, "skip near-singular");

        let mat = Tensor::from_vec(
            vec![m0 as f32, m1 as f32, m2 as f32, m3 as f32],
            vec![2, 2],
        ).unwrap();
        let inv = linalg::inverse(&mat).unwrap();
        let product = linalg::matmul(&mat, &inv).unwrap();
        let data = product.as_slice();
        prop_assert!((data[0] - 1.0).abs() < 1e-3, "I[0,0]={}", data[0]);
        prop_assert!((data[1]).abs() < 1e-3, "I[0,1]={}", data[1]);
        prop_assert!((data[2]).abs() < 1e-3, "I[1,0]={}", data[2]);
        prop_assert!((data[3] - 1.0).abs() < 1e-3, "I[1,1]={}", data[3]);
    }

    #[test]
    fn proptest_rk4_exponential(initial in 0.1f64..10.0, t_end in 0.01f64..2.0) {
        // dy/dt = y => y(t) = y0 * e^t
        let result = ode::rk4(|_t, y| y, initial, 0.0, t_end, 0.001);
        let expected = initial * (t_end).exp();
        prop_assert!(
            (result - expected).abs() / expected.abs().max(1.0) < 1e-3,
            "RK4 exponential: got {result}, expected {expected}"
        );
    }
}

// ===========================================================================
// Snapshot tests
// ===========================================================================

#[test]
fn snapshot_matmul_result() {
    let a = Tensor::from_vec(vec![1.0, 2.0, 3.0, 4.0, 5.0, 6.0], vec![2, 3]).unwrap();
    let b = Tensor::from_vec(vec![7.0, 8.0, 9.0, 10.0, 11.0, 12.0], vec![3, 2]).unwrap();
    let c = linalg::matmul(&a, &b).unwrap();
    insta::assert_snapshot!("matmul_2x3_x_3x2", format!("{:?}", c.as_slice()));
}

#[test]
fn snapshot_inverse_2x2() {
    let m = Tensor::from_vec(vec![4.0, 7.0, 2.0, 6.0], vec![2, 2]).unwrap();
    let inv = linalg::inverse(&m).unwrap();
    insta::assert_snapshot!("inverse_2x2", format!("{:?}", inv.as_slice()));
}

#[test]
fn snapshot_determinant_3x3() {
    let m = Tensor::from_vec(
        vec![6.0, 1.0, 1.0, 4.0, -2.0, 5.0, 2.0, 8.0, 7.0],
        vec![3, 3],
    )
    .unwrap();
    let det = linalg::determinant(&m).unwrap();
    insta::assert_snapshot!("determinant_3x3", format!("{det:.6}"));
}

#[test]
fn snapshot_rk4_exponential() {
    let result = ode::rk4(|_t, y| y, 1.0, 0.0, 1.0, 0.001);
    insta::assert_snapshot!("rk4_exponential", format!("{result:.6}"));
}

#[test]
fn snapshot_stats_dataset() {
    let data = vec![1.0, 2.0, 3.0, 4.0, 5.0];
    let m = stats::mean(&data).unwrap();
    let v = stats::variance(&data).unwrap();
    let s = stats::stddev(&data).unwrap();
    insta::assert_snapshot!(
        "stats_dataset",
        format!("mean={m:.4} var={v:.4} stddev={s:.4}")
    );
}

#[test]
fn snapshot_histogram_uniform() {
    let data: Vec<f64> = (0..100).map(|i| i as f64).collect();
    let hist = stats::histogram(&data, 10).unwrap();
    let formatted: Vec<String> = hist.iter().map(|(k, v)| format!("bin{k}:{v}")).collect();
    insta::assert_snapshot!("histogram_uniform", formatted.join(" "));
}

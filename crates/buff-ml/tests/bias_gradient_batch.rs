//! Regression test for the bias-gradient bug documented in
//! `crates/buff-ml/AGENTS.md` ("Bias gradient 3x factor").
//!
//! Root cause: `Tensor::sum_axis` in `buff-tensor` decomposed the flat index
//! with axes in the wrong order (treated axis 0 as fastest-varying instead of
//! the last axis, violating the row-major layout convention). The bias
//! backward in `Var::add_row_bias` reduces the upstream grad over the batch
//! axis via `sum_axis(0)`, so for `batch > 1` AND `output_dim > 1` the bias
//! gradient was scrambled. The bug was previously masked because the only
//! bias-gradient test (`proptest_linear_bias_gradient_matches_numerical` in
//! `unit_tests.rs`) used `Linear(1, 1)` — i.e. `output_dim == 1`, where the
//! wrong-axis reduction happens to land every contribution in the single
//! output cell and thus produces the correct scalar.
//!
//! These tests use `batch > 1` together with `output_dim > 1` so the bug is
//! exercised, and verify the autodiff bias gradient against:
//!   (a) a hand-derived analytical value, and
//!   (b) an independent central finite-difference estimate.

use buff_ml::*;
use buff_tensor::Tensor;

// ---------------------------------------------------------------------------
// Fixed, hand-computable configuration: a 2-layer MLP (Linear -> ReLU ->
// Linear) with identity weights so ReLU stays in its linear regime.
//
//   L1 = Linear(2,2): W1 = [[1,0],[0,1]], b1 = [1,1]
//   ReLU
//   L2 = Linear(2,2): W2 = [[1,0],[0,1]], b2 = [0,0]
//   x = [[1,2],[3,4],[5,6]]   (batch = 3, in = 2)
//   target = zeros([3,2])
//
// Forward (all pre-activations > 0, so ReLU is the identity):
//   h = x + [1,1] = [[2,3],[4,5],[6,7]]
//   y = h + [0,0] = [[2,3],[4,5],[6,7]]
//
// mse_loss = mean((y - target)^2) = sum(y^2) / N, N = 6
//   dL/dy = 2*y/N = y/3 = [[2/3, 1],[4/3, 5/3],[2, 7/3]]
//
// Bias gradient = sum over the batch axis (axis 0) of the upstream grad:
//   b2.grad = [ (2/3+4/3+2) , (1+5/3+7/3) ] = [12/3, 15/3] = [4.0, 5.0]
// Because W2 = I and ReLU is the identity here, b1.grad is the same: [4.0, 5.0].
// ---------------------------------------------------------------------------

const W1: [f32; 4] = [1.0, 0.0, 0.0, 1.0];
const B1: [f32; 2] = [1.0, 1.0];
const W2: [f32; 4] = [1.0, 0.0, 0.0, 1.0];
const B2: [f32; 2] = [0.0, 0.0];
const OUT_DIM: usize = 2;

fn input_x() -> Tensor {
    Tensor::from_vec(vec![1.0, 2.0, 3.0, 4.0, 5.0, 6.0], vec![3, 2]).expect("x shape valid")
}

fn target_zero() -> Tensor {
    Tensor::from_vec(vec![0.0; 6], vec![3, 2]).expect("target shape valid")
}

/// Build a fresh 2-layer model with the fixed base parameters above.
fn build_model() -> Model {
    let mut l1 = Linear::new(2, 2).expect("Linear(2,2)");
    l1.load_parameters(&[
        Tensor::from_vec(W1.to_vec(), vec![2, 2]).expect("W1"),
        Tensor::from_vec(B1.to_vec(), vec![2]).expect("B1"),
    ])
    .expect("load L1 params");
    let mut l2 = Linear::new(2, 2).expect("Linear(2,2)");
    l2.load_parameters(&[
        Tensor::from_vec(W2.to_vec(), vec![2, 2]).expect("W2"),
        Tensor::from_vec(B2.to_vec(), vec![2]).expect("B2"),
    ])
    .expect("load L2 params");
    Model::sequential(vec![Box::new(l1), Box::new(ReLU::new()), Box::new(l2)])
}

/// Build a model where bias component `comp` of `layer_index` (0 = L1, 2 = L2)
/// is shifted by `delta`. Used by the finite-difference check.
fn build_model_with_bias_shift(layer_index: usize, comp: usize, delta: f32) -> Model {
    let mut b1 = B1;
    let mut b2 = B2;
    match layer_index {
        0 => b1[comp] += delta,
        2 => b2[comp] += delta,
        _ => {}
    }
    let mut l1 = Linear::new(2, 2).expect("Linear(2,2)");
    l1.load_parameters(&[
        Tensor::from_vec(W1.to_vec(), vec![2, 2]).expect("W1"),
        Tensor::from_vec(b1.to_vec(), vec![2]).expect("B1"),
    ])
    .expect("load L1 params");
    let mut l2 = Linear::new(2, 2).expect("Linear(2,2)");
    l2.load_parameters(&[
        Tensor::from_vec(W2.to_vec(), vec![2, 2]).expect("W2"),
        Tensor::from_vec(b2.to_vec(), vec![2]).expect("B2"),
    ])
    .expect("load L2 params");
    Model::sequential(vec![Box::new(l1), Box::new(ReLU::new()), Box::new(l2)])
}

/// Forward-only loss for a freshly built model (used by the numerical
/// finite-difference check). Returns the scalar MSE loss value.
fn model_loss_value(model: &Model, x: &Tensor, target: &Tensor) -> f32 {
    let tape = Tape::new();
    let xv = tape.leaf(x.clone(), false).expect("leaf x");
    let pred = model.forward(xv, false).expect("forward");
    mse_loss(&pred, target)
        .expect("mse_loss")
        .value()
        .as_slice()[0]
}

/// Read a layer's bias gradient as an owned Vec. Linear::collect_grads returns
/// [("weight", ..), ("bias", ..)].
fn bias_grad(model: &Model, layer_index: usize) -> Vec<f32> {
    let layer = model.get_layer(layer_index).expect("layer exists");
    layer
        .collect_grads()
        .iter()
        .find(|(name, _)| name == "bias")
        .map(|(_, t)| t.as_slice().to_vec())
        .expect("bias grad present")
}

/// Central finite-difference estimate of the bias gradient for `layer_index`
/// (0 = L1, 2 = L2), by perturbing each bias component of a freshly rebuilt
/// model and recomputing the forward loss.
fn numerical_bias_grad(layer_index: usize, x: &Tensor, target: &Tensor) -> Vec<f32> {
    const EPS: f32 = 1e-3;
    let mut out = vec![0.0f32; OUT_DIM];
    for k in 0..OUT_DIM {
        let l_plus = model_loss_value(&build_model_with_bias_shift(layer_index, k, EPS), x, target);
        let l_minus = model_loss_value(
            &build_model_with_bias_shift(layer_index, k, -EPS),
            x,
            target,
        );
        out[k] = (l_plus - l_minus) / (2.0 * EPS);
    }
    out
}

/// Run a full forward + backward pass on a freshly built model and return it
/// (with accumulated grads available via `collect_grads`).
fn run_backward(x: &Tensor, target: &Tensor) -> Model {
    let model = build_model();
    let tape = Tape::new();
    let xv = tape.leaf(x.clone(), false).expect("leaf x");
    let pred = model.forward(xv, true).expect("forward");
    let loss = mse_loss(&pred, target).expect("mse_loss");
    model.backward(&loss).expect("backward");
    model
}

#[test]
fn last_layer_bias_grad_matches_analytical_batch_gt1() {
    // Regression for AGENTS.md "Bias gradient 3x factor": with the buggy
    // `sum_axis`, autodiff returned [3.0, 6.0] instead of the analytical [4,5].
    let model = run_backward(&input_x(), &target_zero());
    let g = bias_grad(&model, 2);
    assert_eq!(
        g.len(),
        OUT_DIM,
        "expected {OUT_DIM} bias components, got {g:?}"
    );
    let eps = 1e-5;
    assert!(
        (g[0] - 4.0).abs() < eps,
        "last-layer bias grad[0] wrong: got {}, expected 4.0",
        g[0]
    );
    assert!(
        (g[1] - 5.0).abs() < eps,
        "last-layer bias grad[1] wrong: got {}, expected 5.0",
        g[1]
    );
}

#[test]
fn first_layer_bias_grad_matches_analytical_batch_gt1() {
    // With W2 = I and ReLU in its linear regime, b1.grad == b2.grad == [4,5].
    let model = run_backward(&input_x(), &target_zero());
    let g = bias_grad(&model, 0);
    let eps = 1e-5;
    assert!(
        (g[0] - 4.0).abs() < eps,
        "first-layer bias grad[0] wrong: got {}, expected 4.0",
        g[0]
    );
    assert!(
        (g[1] - 5.0).abs() < eps,
        "first-layer bias grad[1] wrong: got {}, expected 5.0",
        g[1]
    );
}

#[test]
fn multilayer_bias_grad_matches_numerical_finite_difference() {
    // Gold-standard cross-check: autodiff bias gradient vs central finite
    // differences, for BOTH Linear layers in the batch>1 multi-layer net.
    let x = input_x();
    let target = target_zero();
    let model = run_backward(&x, &target);

    for layer_index in [0usize, 2] {
        let auto = bias_grad(&model, layer_index);
        let num = numerical_bias_grad(layer_index, &x, &target);
        assert_eq!(
            auto.len(),
            num.len(),
            "grad length mismatch at layer {layer_index}"
        );
        // f32 central difference is accurate to ~1e-4 here; allow 5e-3 slack.
        for (k, (a, n)) in auto.iter().zip(num.iter()).enumerate() {
            assert!(
                (a - n).abs() < 5e-3,
                "layer {layer_index} bias grad[{k}]: autodiff={a}, numerical={n}"
            );
        }
    }
}

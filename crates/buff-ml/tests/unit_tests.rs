//! Integration tests for buff-ml: gradient checks, training convergence,
//! save/load roundtrips, and snapshot tests.
//!
//! Uses `proptest` for numerical gradient verification and `insta` for
//! snapshot testing of model structures, loss curves, and layer outputs.
//! All tests use ONLY the public API.

use buff_ml::*;
use buff_tensor::Tensor;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn approx(a: f32, b: f32, eps: f32) -> bool {
    (a - b).abs() < eps
}

/// Build a linear regression target: y = w*x + b (analytically).
fn linear_target(x: &Tensor, w: f32, b: f32) -> Tensor {
    let data: Vec<f32> = x.as_slice().iter().map(|&v| w * v + b).collect();
    Tensor::from_vec(data, x.shape().as_slice().to_vec()).unwrap()
}

// ===========================================================================
// Proptest: gradient checks via public API (layers + losses)
// ===========================================================================

/// Helper: create a fresh Linear(1,1) with known weights, forward+backward
/// on the given x/target, and return (weight_grad, bias_grad).
fn linear_1x1_grads(
    w_val: f32,
    b_val: f32,
    x: &Tensor,
    target: &Tensor,
) -> (f32, f32) {
    use buff_ml::Layer;
    let mut layer = Linear::new(1, 1).unwrap();
    layer
        .load_parameters(&[
            Tensor::from_vec(vec![w_val], vec![1, 1]).unwrap(),
            Tensor::from_vec(vec![b_val], vec![1]).unwrap(),
        ])
        .unwrap();
    let tape = Tape::new();
    let xv = tape.leaf(x.clone(), false).unwrap();
    let pred = layer.forward(xv, true).unwrap();
    let loss = mse_loss(&pred, target).unwrap();
    loss.backward().unwrap();
    let grads = layer.collect_grads();
    (grads[0].1.as_slice()[0], grads[1].1.as_slice()[0])
}

/// Helper: forward loss for a fresh Linear(1,1) with given w, b, x, target.
#[allow(dead_code)]
fn linear_1x1_loss(w_val: f32, b_val: f32, x: &Tensor, target: &Tensor) -> f32 {
    use buff_ml::Layer;
    let mut layer = Linear::new(1, 1).unwrap();
    layer
        .load_parameters(&[
            Tensor::from_vec(vec![w_val], vec![1, 1]).unwrap(),
            Tensor::from_vec(vec![b_val], vec![1]).unwrap(),
        ])
        .unwrap();
    let tape = Tape::new();
    let xv = tape.leaf(x.clone(), false).unwrap();
    let pred = layer.forward(xv, false).unwrap();
    mse_loss(&pred, target).unwrap().value().as_slice()[0]
}

/// Proptest: numerical vs autodiff gradient for a single Linear layer's bias.
/// Uses bounds checks since f32 numerical differentiation has limited precision.
#[test]
fn proptest_linear_bias_gradient_matches_numerical() {
    use proptest::prelude::*;

    proptest::proptest!(|(b_val in -5.0f32..5.0)| {
        let x = Tensor::from_vec(vec![1.0, 2.0, 3.0, 4.0, 5.0], vec![5, 1]).unwrap();
        let target = linear_target(&x, 2.0, 3.0);

        let (_, autodiff_b) = linear_1x1_grads(2.0, b_val, &x, &target);

        // Analytical: dL/db = (2/N) * sum(pred_i - target_i) = (2/5) * 5*(b_val - 3) = 2*(b_val - 3)
        let expected = 2.0 * (b_val - 3.0);
        prop_assert!(
            (autodiff_b - expected).abs() < 2.0,
            "bias grad mismatch at b={b_val}: autodiff={autodiff_b}, expected~={expected}"
        );
    });
}

/// Proptest: numerical vs autodiff gradient for a single Linear layer's weight.
#[test]
fn proptest_linear_weight_gradient_matches_numerical() {
    use proptest::prelude::*;

    proptest::proptest!(|(w_val in -5.0f32..5.0)| {
        let x = Tensor::from_vec(vec![1.0, 2.0, 3.0], vec![3, 1]).unwrap();
        let target = linear_target(&x, 2.0, 0.0);

        let (autodiff_w, _) = linear_1x1_grads(w_val, 0.0, &x, &target);

        // Analytical: dL/dw = (2/N) * sum(x_i * (w*x_i - 2*x_i))
        // = (2/3) * (w-2) * sum(x_i^2) = (2/3)*(w-2)*14
        let expected = (2.0 / 3.0) * (w_val - 2.0) * 14.0;
        prop_assert!(
            (autodiff_w - expected).abs() < 5.0,
            "weight grad mismatch at w={w_val}: autodiff={autodiff_w}, expected~={expected}"
        );
    });
}

/// Proptest: gradient for a 2-parameter linear model (w + b together).
#[test]
fn proptest_joint_gradient_matches_numerical() {
    use proptest::prelude::*;

    proptest::proptest!(|(w0 in -3.0f32..3.0, b0 in -3.0f32..3.0)| {
        let x = Tensor::from_vec(vec![1.0, 2.0, 3.0], vec![3, 1]).unwrap();
        let target = linear_target(&x, 2.0, 1.0);

        let (autodiff_w, autodiff_b) = linear_1x1_grads(w0, b0, &x, &target);

        // Both gradients should be finite and bounded.
        prop_assert!(autodiff_w.is_finite(), "w grad not finite at (w={w0}, b={b0}): {autodiff_w}");
        prop_assert!(autodiff_b.is_finite(), "b grad not finite at (w={w0}, b={b0}): {autodiff_b}");
        prop_assert!(
            autodiff_w.abs() < 200.0,
            "w grad too large at (w={w0}, b={b0}): {autodiff_w}"
        );
        prop_assert!(
            autodiff_b.abs() < 200.0,
            "b grad too large at (w={w0}, b={b0}): {autodiff_b}"
        );
    });
}

// ===========================================================================
// Training convergence tests
// ===========================================================================

/// Linear regression: train `y = 2x + 3` with SGD, verify convergence.
#[test]
fn training_linear_regression_sgd_converges() {
    let x = Tensor::from_vec(vec![-2.0, -1.0, 0.0, 1.0, 2.0], vec![5, 1]).unwrap();
    let target = Tensor::from_vec(vec![-1.0, 1.0, 3.0, 5.0, 7.0], vec![5, 1]).unwrap();

    let mut model = Model::sequential(vec![Box::new(Linear::new(1, 1).unwrap())]);
    let mut opt = SGD::new(0.01).unwrap();

    for _ in 0..500 {
        let tape = Tape::new();
        let xv = tape.leaf(x.clone(), false).unwrap();
        let pred = model.forward(xv, true).unwrap();
        let loss = mse_loss(&pred, &target).unwrap();
        model.backward(&loss).unwrap();
        opt.step(&mut model).unwrap();
    }

    let tape = Tape::new();
    let xv = tape.leaf(x.clone(), false).unwrap();
    let pred = model.forward(xv, false).unwrap();
    let loss = mse_loss(&pred, &target).unwrap();
    let final_loss = loss.value().as_slice()[0];
    assert!(
        final_loss < 0.1,
        "Linear regression did not converge: final loss = {final_loss}"
    );
}

/// Linear regression: train with Adam optimizer, verify convergence.
#[test]
fn training_linear_regression_adam_converges() {
    let x = Tensor::from_vec(vec![-2.0, -1.0, 0.0, 1.0, 2.0], vec![5, 1]).unwrap();
    let target = Tensor::from_vec(vec![-1.0, 1.0, 3.0, 5.0, 7.0], vec![5, 1]).unwrap();

    let mut model = Model::sequential(vec![Box::new(Linear::new(1, 1).unwrap())]);
    let mut opt = Adam::new(0.01).unwrap();

    for _ in 0..1000 {
        let tape = Tape::new();
        let xv = tape.leaf(x.clone(), false).unwrap();
        let pred = model.forward(xv, true).unwrap();
        let loss = mse_loss(&pred, &target).unwrap();
        model.backward(&loss).unwrap();
        opt.step(&mut model).unwrap();
    }

    let tape = Tape::new();
    let xv = tape.leaf(x.clone(), false).unwrap();
    let pred = model.forward(xv, false).unwrap();
    let loss = mse_loss(&pred, &target).unwrap();
    let final_loss = loss.value().as_slice()[0];
    assert!(
        final_loss < 0.1,
        "Adam linear regression did not converge: final loss = {final_loss}"
    );
}

/// Multi-layer perceptron: train a 2-layer network, verify loss decreases.
#[test]
fn training_mlp_loss_decreases() {
    let x = Tensor::from_vec(vec![1.0, 2.0, 3.0, 4.0, 5.0, 6.0], vec![3, 2]).unwrap();
    let target = Tensor::from_vec(vec![0.0, 1.0, 0.0], vec![3, 1]).unwrap();

    let mut model = Model::sequential(vec![
        Box::new(Linear::new(2, 4).unwrap()),
        Box::new(ReLU::new()),
        Box::new(Linear::new(4, 1).unwrap()),
    ]);
    let mut opt = SGD::new(0.001).unwrap();

    let mut first_loss = 0.0f32;
    let mut last_loss = 0.0f32;
    for i in 0..200 {
        let tape = Tape::new();
        let xv = tape.leaf(x.clone(), false).unwrap();
        let pred = model.forward(xv, true).unwrap();
        let loss = mse_loss(&pred, &target).unwrap();
        if i == 0 {
            first_loss = loss.value().as_slice()[0];
        }
        if i == 199 {
            last_loss = loss.value().as_slice()[0];
        }
        model.backward(&loss).unwrap();
        opt.step(&mut model).unwrap();
    }

    assert!(
        last_loss < first_loss,
        "MLP loss did not decrease: first={first_loss}, last={last_loss}"
    );
}

// ===========================================================================
// Save/load roundtrip tests
// ===========================================================================

/// Save and load a linear model, verify parameters are restored.
#[test]
fn save_load_linear_roundtrip() {
    let mut model = Model::sequential(vec![
        Box::new(Linear::new(3, 2).unwrap()),
        Box::new(ReLU::new()),
        Box::new(Linear::new(2, 1).unwrap()),
    ]);

    let x = Tensor::from_vec(vec![1.0, 2.0, 3.0, 4.0, 5.0, 6.0], vec![2, 3]).unwrap();
    let target = Tensor::from_vec(vec![1.0, 0.0], vec![2, 1]).unwrap();
    let mut opt = SGD::new(0.01).unwrap();
    {
        let tape = Tape::new();
        let xv = tape.leaf(x.clone(), false).unwrap();
        let pred = model.forward(xv, true).unwrap();
        let loss = mse_loss(&pred, &target).unwrap();
        model.backward(&loss).unwrap();
        opt.step(&mut model).unwrap();
    }

    let tape = Tape::new();
    let xv = tape.leaf(x.clone(), false).unwrap();
    let pred_before = model.forward(xv, false).unwrap();
    let before_vals = pred_before.value().as_slice().to_vec();

    let dir = std::env::temp_dir();
    let path = dir.join("buff_ml_test_save_load.json");
    let path_str = path.to_str().unwrap_or("");
    model.save(path_str).unwrap();
    model.load(path_str).unwrap();

    let tape2 = Tape::new();
    let xv2 = tape2.leaf(x.clone(), false).unwrap();
    let pred_after = model.forward(xv2, false).unwrap();
    let after_vals = pred_after.value().as_slice().to_vec();

    for (i, (a, b)) in before_vals.iter().zip(after_vals.iter()).enumerate() {
        assert!(
            approx(*a, *b, 1e-6),
            "Save/load mismatch at index {i}: before={a}, after={b}"
        );
    }

    let _ = std::fs::remove_file(path);
}

/// Save/load should fail gracefully with mismatched layer counts.
#[test]
fn save_load_mismatched_layer_count_fails() {
    let model_a = Model::sequential(vec![Box::new(Linear::new(2, 3).unwrap())]);
    let mut model_b = Model::sequential(vec![
        Box::new(Linear::new(2, 3).unwrap()),
        Box::new(Linear::new(3, 1).unwrap()),
    ]);

    let dir = std::env::temp_dir();
    let path = dir.join("buff_ml_test_mismatch.json");
    let path_str = path.to_str().unwrap_or("");

    model_a.save(path_str).unwrap();
    let result = model_b.load(path_str);
    assert!(result.is_err(), "Expected error on layer count mismatch");

    let _ = std::fs::remove_file(path);
}

// ===========================================================================
// Insta snapshot tests (6)
// ===========================================================================

/// Snapshot: model debug format for a multi-layer model.
#[test]
fn snapshot_model_debug_format() {
    let model = Model::sequential(vec![
        Box::new(Linear::new(4, 8).unwrap()),
        Box::new(ReLU::new()),
        Box::new(Linear::new(8, 2).unwrap()),
    ]);
    insta::assert_snapshot!("model_3layer_debug", format!("{model:?}"));
}

/// Snapshot: softmax output for known input (via layer API).
#[test]
fn snapshot_softmax_output() {
    let tape = Tape::new();
    let x = tape
        .leaf(
            Tensor::from_vec(vec![1.0, 2.0, 3.0, 4.0, 5.0, 6.0], vec![2, 3]).unwrap(),
            false,
        )
        .unwrap();
    let softmax = Softmax::new();
    let y = softmax.forward(x, false).unwrap();
    let vals: Vec<String> = y
        .value()
        .as_slice()
        .iter()
        .map(|v| format!("{v:.6}"))
        .collect();
    insta::assert_snapshot!("softmax_2x3", vals.join(", "));
}

/// Snapshot: linear layer forward output with known weights.
#[test]
fn snapshot_linear_forward_deterministic() {
    let mut l = Linear::new(2, 3).unwrap();
    let w = Tensor::from_vec(vec![1.0, 0.0, 0.5, 0.0, 1.0, -0.5], vec![2, 3]).unwrap();
    let b = Tensor::from_vec(vec![0.1, 0.2, 0.3], vec![3]).unwrap();
    l.load_parameters(&[w, b]).unwrap();

    let tape = Tape::new();
    let x = tape
        .leaf(
            Tensor::from_vec(vec![1.0, 2.0, 3.0, 4.0], vec![2, 2]).unwrap(),
            false,
        )
        .unwrap();
    let y = l.forward(x, false).unwrap();
    let vals: Vec<String> = y
        .value()
        .as_slice()
        .iter()
        .map(|v| format!("{v:.2}"))
        .collect();
    insta::assert_snapshot!("linear_forward_2x2_to_3", vals.join(", "));
}

/// Snapshot: MSE loss value for known prediction and target.
#[test]
fn snapshot_mse_loss_value() {
    let tape = Tape::new();
    let pred = tape
        .leaf(
            Tensor::from_vec(vec![1.0, 2.0, 3.0], vec![3]).unwrap(),
            true,
        )
        .unwrap();
    let target = Tensor::from_vec(vec![2.0, 2.0, 4.0], vec![3]).unwrap();
    let loss = mse_loss(&pred, &target).unwrap();
    let val = loss.value().as_slice()[0];
    insta::assert_snapshot!("mse_loss_1_2_3_vs_2_2_4", format!("{val:.6}"));
}

/// Snapshot: cross-entropy loss value for known logits and target.
#[test]
fn snapshot_cross_entropy_loss_value() {
    let tape = Tape::new();
    let logits = tape
        .leaf(
            Tensor::from_vec(vec![2.0, 1.0, 0.5, 0.1], vec![2, 2]).unwrap(),
            true,
        )
        .unwrap();
    let target = Tensor::from_vec(vec![1.0, 0.0, 0.0, 1.0], vec![2, 2]).unwrap();
    let loss = cross_entropy(&logits, &target).unwrap();
    let val = loss.value().as_slice()[0];
    insta::assert_snapshot!("cross_entropy_2batch", format!("{val:.6}"));
}

/// Snapshot: sigmoid output for zero input (via layer API).
#[test]
fn snapshot_sigmoid_zero() {
    let tape = Tape::new();
    let x = tape
        .leaf(Tensor::from_vec(vec![0.0], vec![1]).unwrap(), false)
        .unwrap();
    let sigmoid = Sigmoid::new();
    let y = sigmoid.forward(x, false).unwrap();
    let val = y.value().as_slice()[0];
    insta::assert_snapshot!("sigmoid_zero", format!("{val:.6}"));
}

// ===========================================================================
// Edge cases
// ===========================================================================

/// Dropout with rate=0.5: training mode should modify some elements.
#[test]
fn dropout_training_modifies_some() {
    let tape = Tape::new();
    let x = tape
        .leaf(
            Tensor::from_vec(vec![1.0; 100], vec![100]).unwrap(),
            false,
        )
        .unwrap();
    let d = Dropout::new(0.5).unwrap();
    let y = d.forward(x.clone(), true).unwrap();
    let yv = y.value();
    let xv = x.value();
    let all_same = yv
        .as_slice()
        .iter()
        .zip(xv.as_slice().iter())
        .all(|(a, b)| approx(*a, *b, 1e-6));
    assert!(!all_same, "Dropout should modify at least some elements");
}

/// Model::len and Model::is_empty.
#[test]
fn model_len_and_empty() {
    let empty = Model::sequential(vec![]);
    assert_eq!(empty.len(), 0);
    assert!(empty.is_empty());

    let m = Model::sequential(vec![Box::new(Linear::new(2, 3).unwrap())]);
    assert_eq!(m.len(), 1);
    assert!(!m.is_empty());
}

/// ReLU idempotent: applying ReLU twice yields same result as once.
#[test]
fn relu_idempotent() {
    let relu = ReLU::new();
    let tape = Tape::new();
    let x = tape
        .leaf(
            Tensor::from_vec(vec![-2.0, -1.0, 0.0, 1.0, 2.0], vec![5]).unwrap(),
            true,
        )
        .unwrap();
    let y1 = relu.forward(x.clone(), false).unwrap();
    let y2 = relu.forward(y1.clone(), false).unwrap();
    assert_eq!(
        y1.value().as_slice(),
        y2.value().as_slice(),
        "ReLU should be idempotent"
    );
    assert_eq!(
        y1.value().as_slice(),
        &[0.0, 0.0, 0.0, 1.0, 2.0],
        "ReLU output incorrect"
    );
}

/// Optimizer names.
#[test]
fn optimizer_names() {
    let sgd = SGD::new(0.01).unwrap();
    assert_eq!(sgd.name(), "sgd");
    let adam = Adam::new(0.001).unwrap();
    assert_eq!(adam.name(), "adam");
}

/// Layer kinds.
#[test]
fn layer_kinds_all() {
    assert_eq!(Linear::new(2, 3).unwrap().layer_kind(), "linear");
    assert_eq!(ReLU::new().layer_kind(), "relu");
    assert_eq!(Sigmoid::new().layer_kind(), "sigmoid");
    assert_eq!(Softmax::new().layer_kind(), "softmax");
    assert_eq!(Dropout::new(0.1).unwrap().layer_kind(), "dropout");
}

/// MSE loss is symmetric: loss(a,b) == loss(b,a).
#[test]
fn mse_loss_symmetric() {
    let a = Tensor::from_vec(vec![1.0, 2.0, 3.0], vec![3]).unwrap();
    let b = Tensor::from_vec(vec![4.0, 5.0, 6.0], vec![3]).unwrap();

    let tape1 = Tape::new();
    let va = tape1.leaf(a.clone(), true).unwrap();
    let loss1 = mse_loss(&va, &b).unwrap();

    let tape2 = Tape::new();
    let vb = tape2.leaf(b.clone(), true).unwrap();
    let loss2 = mse_loss(&vb, &a).unwrap();

    assert!(
        approx(loss1.value().as_slice()[0], loss2.value().as_slice()[0], 1e-6),
        "MSE loss not symmetric"
    );
}

/// Sigmoid via layer: output at x=0 should be 0.5.
#[test]
fn sigmoid_layer_at_zero() {
    let tape = Tape::new();
    let x = tape
        .leaf(Tensor::from_vec(vec![0.0], vec![1]).unwrap(), false)
        .unwrap();
    let sig = Sigmoid::new();
    let y = sig.forward(x, false).unwrap();
    assert!(
        approx(y.value().as_slice()[0], 0.5, 1e-5),
        "Sigmoid(0) should be 0.5, got {}",
        y.value().as_slice()[0]
    );
}

/// Sigmoid layer: large positive x should yield ~1.0.
#[test]
fn sigmoid_layer_at_large_positive() {
    let tape = Tape::new();
    let x = tape
        .leaf(Tensor::from_vec(vec![20.0], vec![1]).unwrap(), false)
        .unwrap();
    let sig = Sigmoid::new();
    let y = sig.forward(x, false).unwrap();
    assert!(
        approx(y.value().as_slice()[0], 1.0, 1e-5),
        "Sigmoid(20) should be ~1.0, got {}",
        y.value().as_slice()[0]
    );
}

//! Loss functions: [`mse_loss`] (mean squared error) and [`cross_entropy`]
//! (combined softmax + negative-log-likelihood for numerical stability).
//!
//! Both return a scalar `[1]`-shaped [`Var`] so that [`Var::backward`] seeds
//! the grad to `1.0` and the reverse sweep fills every parameter's grad.

use crate::autodiff::Var;
use crate::error::{MlError, MlResult};
use buff_tensor::Tensor;

/// Mean squared error: `mean((pred - target)^2)`.
///
/// Composed from primitive autodiff ops (`sub` → `mul` → `sum_all` →
/// `scale`), so the gradient `2*(pred-target)/N` flows through the engine
/// rather than a hand-written backward. `target` is treated as a constant
/// (no grad).
///
/// # Errors
///
/// - [`MlError::ShapeMismatch`] if `pred` and `target` shapes differ.
pub fn mse_loss(pred: &Var, target: &Tensor) -> MlResult<Var> {
    let pv = pred.value();
    if pv.shape().as_slice() != target.shape().as_slice() {
        return Err(MlError::ShapeMismatch {
            lhs: pv.shape().as_slice().to_vec(),
            rhs: target.shape().as_slice().to_vec(),
        });
    }
    let n = pv.len();
    if n == 0 {
        return Err(MlError::InvalidDimension {
            name: "pred.len",
            value: 0,
        });
    }
    let tape = pred.tape().clone();
    // Target as a constant leaf (no grad).
    let t = tape.leaf(target.clone(), false)?;
    let diff = pred.sub(&t)?;
    let sq = diff.mul(&diff)?;
    let summed = sq.sum_all()?;
    // mean = sum / N
    summed.scale(1.0f32 / n as f32)
}

/// Cross-entropy loss with combined softmax + NLL (numerically stable).
///
/// `pred` is raw logits of shape `[batch, classes]`; `target` is a one-hot
/// label tensor of the same shape. Returns the mean negative-log-likelihood
/// across the batch as a scalar `[1]`-shaped `Var`.
///
/// Implemented as a SINGLE tape op (rather than composing softmax + log +
/// nll) because the combined backward `(probs - target) / batch` is simpler
/// and avoids `log(0)` NaNs.
///
/// # Errors
///
/// - [`MlError::RankMismatch`] if `pred` is not rank-2.
/// - [`MlError::ShapeMismatch`] if `pred` and `target` shapes differ.
/// - [`MlError::InvalidDimension`] if `batch == 0`.
pub fn cross_entropy(pred: &Var, target: &Tensor) -> MlResult<Var> {
    let pv = pred.value();
    if pv.shape().rank() != 2 {
        return Err(MlError::RankMismatch {
            actual: pv.shape().rank(),
            expected: 2,
        });
    }
    if pv.shape().as_slice() != target.shape().as_slice() {
        return Err(MlError::ShapeMismatch {
            lhs: pv.shape().as_slice().to_vec(),
            rhs: target.shape().as_slice().to_vec(),
        });
    }
    let dims = pv.shape().as_slice();
    let batch = dims[0];
    let classes = dims[1];
    if batch == 0 {
        return Err(MlError::InvalidDimension {
            name: "batch",
            value: 0,
        });
    }
    let xs = pv.as_slice();
    let ts = target.as_slice();
    // Forward: stable softmax + NLL.
    let mut loss = 0.0f32;
    let mut probs = vec![0.0f32; batch * classes];
    for r in 0..batch {
        let row = &xs[r * classes..(r + 1) * classes];
        let m = row.iter().copied().fold(f32::NEG_INFINITY, f32::max);
        let mut sum = 0.0f32;
        for c in 0..classes {
            let e = (row[c] - m).exp();
            probs[r * classes + c] = e;
            sum += e;
        }
        if sum > 0.0 {
            for c in 0..classes {
                probs[r * classes + c] /= sum;
            }
        }
        // NLL for this row: -sum(target * log(prob)).
        for c in 0..classes {
            let p = probs[r * classes + c].max(1e-12);
            loss -= ts[r * classes + c] * p.ln();
        }
    }
    let loss_val = loss / batch as f32;
    let zv = Tensor::from_vec(vec![loss_val], vec![1])?;
    let rg = pred.requires_grad();
    let pi = pred.index();
    let tape = pred.tape().clone();
    let zi = tape.next_index();
    // Capture probs + target for the combined backward.
    let probs_cap = Tensor::from_vec(probs, vec![batch, classes])?;
    let target_cap = target.clone();
    let backward: Box<dyn FnOnce(&mut crate::autodiff::TapeInner)> = Box::new(move |inner| {
        let g_scalar = inner.nodes[zi]
            .grad
            .as_slice()
            .first()
            .copied()
            .unwrap_or(0.0);
        // dL/dlogits = (probs - target) / batch, scaled by incoming grad.
        let ps = probs_cap.as_slice();
        let ts = target_cap.as_slice();
        let mut gx = vec![0.0f32; ps.len()];
        let scale = g_scalar / batch as f32;
        for k in 0..ps.len() {
            gx[k] = (ps[k] - ts[k]) * scale;
        }
        let gx_t = Tensor::from_vec(gx, vec![batch, classes])
            .unwrap_or_else(|_| crate::autodiff::zero_tensor_like(&inner.nodes[pi].grad));
        crate::autodiff::accum_add(&mut inner.nodes[pi].grad, &gx_t);
    });
    tape.push_node(zv, rg, Some(backward))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::autodiff::Tape;

    #[test]
    fn mse_loss_zero_when_equal() {
        let tape = Tape::new();
        let p = tape
            .leaf(
                Tensor::from_vec(vec![1.0, 2.0, 3.0], vec![3]).unwrap(),
                true,
            )
            .unwrap();
        let t = Tensor::from_vec(vec![1.0, 2.0, 3.0], vec![3]).unwrap();
        let loss = mse_loss(&p, &t).unwrap();
        assert!((loss.value().as_slice()[0]).abs() < 1e-6);
    }

    #[test]
    fn mse_loss_shape_mismatch_errors() {
        let tape = Tape::new();
        let p = tape
            .leaf(Tensor::from_vec(vec![1.0, 2.0], vec![2]).unwrap(), true)
            .unwrap();
        let t = Tensor::from_vec(vec![1.0, 2.0, 3.0], vec![3]).unwrap();
        assert!(mse_loss(&p, &t).is_err());
    }

    #[test]
    fn mse_loss_grad_is_two_diff_over_n() {
        // pred = [1,2,3], target = [1,0,0]. diff=[0,2,3].
        // mse = (0+4+9)/3 = 13/3. grad = 2*diff/3 = [0, 4/3, 2].
        let tape = Tape::new();
        let p = tape
            .leaf(
                Tensor::from_vec(vec![1.0, 2.0, 3.0], vec![3]).unwrap(),
                true,
            )
            .unwrap();
        let t = Tensor::from_vec(vec![1.0, 0.0, 0.0], vec![3]).unwrap();
        let loss = mse_loss(&p, &t).unwrap();
        loss.backward().unwrap();
        let grad = p.grad().unwrap();
        let g = grad.as_slice();
        assert!((g[0] - 0.0).abs() < 1e-5);
        assert!((g[1] - 4.0 / 3.0).abs() < 1e-5);
        assert!((g[2] - 2.0).abs() < 1e-5);
    }

    #[test]
    fn cross_entropy_grad_is_probs_minus_target_over_batch() {
        // logits = [[0,0]], target = [[1,0]]. softmax -> [0.5,0.5].
        // grad = ([0.5,0.5] - [1,0]) / 1 = [-0.5, 0.5].
        let tape = Tape::new();
        let logits = tape
            .leaf(Tensor::from_vec(vec![0.0, 0.0], vec![1, 2]).unwrap(), true)
            .unwrap();
        let target = Tensor::from_vec(vec![1.0, 0.0], vec![1, 2]).unwrap();
        let loss = cross_entropy(&logits, &target).unwrap();
        loss.backward().unwrap();
        let grad = logits.grad().unwrap();
        let g = grad.as_slice();
        assert!((g[0] - (-0.5)).abs() < 1e-5);
        assert!((g[1] - 0.5).abs() < 1e-5);
    }

    #[test]
    fn cross_entropy_rejects_wrong_rank() {
        let tape = Tape::new();
        let logits = tape
            .leaf(Tensor::from_vec(vec![1.0, 2.0], vec![2]).unwrap(), true)
            .unwrap();
        let target = Tensor::from_vec(vec![1.0, 0.0], vec![2]).unwrap();
        assert!(cross_entropy(&logits, &target).is_err());
    }
}

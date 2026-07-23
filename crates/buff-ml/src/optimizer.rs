//! Optimizers: [`SGD`] (stochastic gradient descent) and [`Adam`].
//!
//! Each optimizer implements the [`Optimizer`] trait: `step` reads parameter
//! values and accumulated gradients from a [`Model`](crate::Model) via
//! [`Layer::parameters`] and [`Layer::collect_grads`], computes the update,
//! and writes the updated values back via [`Layer::load_parameters`].

use crate::error::{MlError, MlResult};
use crate::model::Model;
use buff_tensor::Tensor;
use std::collections::BTreeMap;

/// Optimizer trait.
pub trait Optimizer {
    /// Perform one optimization step: read params + grads from `model`,
    /// compute updates, write back.
    ///
    /// # Errors
    ///
    /// Returns [`MlError`] if parameter shapes are inconsistent.
    fn step(&mut self, model: &mut Model) -> MlResult<()>;

    /// Human-readable name (for logging / serialization).
    fn name(&self) -> &'static str;
}

/// Stochastic gradient descent with optional momentum.
///
/// Update rule: `param = param - lr * grad`.
pub struct SGD {
    /// Learning rate.
    pub lr: f32,
}

impl SGD {
    /// Create a new SGD optimizer with the given learning rate.
    ///
    /// # Errors
    ///
    /// Returns [`MlError::InvalidHyperparameter`] if `lr` is non-positive.
    pub fn new(lr: f32) -> MlResult<Self> {
        if lr <= 0.0 || !lr.is_finite() {
            return Err(MlError::InvalidHyperparameter {
                name: "lr",
                value: lr,
            });
        }
        Ok(Self { lr })
    }
}

impl Optimizer for SGD {
    fn step(&mut self, model: &mut Model) -> MlResult<()> {
        for layer in model.layers_mut() {
            let params = layer.parameters();
            let grads = layer.collect_grads();
            let mut updated = Vec::with_capacity(params.len());
            for (p, (_name, g)) in params.iter().zip(grads.iter()) {
                let new_data: Vec<f32> = p
                    .as_slice()
                    .iter()
                    .zip(g.as_slice().iter())
                    .map(|(&pv, &gv)| pv - self.lr * gv)
                    .collect();
                updated.push(Tensor::from_vec(new_data, p.shape().as_slice().to_vec())?);
            }
            layer.load_parameters(&updated)?;
        }
        Ok(())
    }

    fn name(&self) -> &'static str {
        "sgd"
    }
}

/// Adam optimizer (Adaptive Moment Estimation).
///
/// Maintains per-parameter first-moment (`m`) and second-moment (`v`) estimates.
///
/// Update rules:
/// ```text
/// m = beta1 * m + (1 - beta1) * grad
/// v = beta2 * v + (1 - beta2) * grad^2
/// m_hat = m / (1 - beta1^t)
/// v_hat = v / (1 - beta2^t)
/// param = param - lr * m_hat / (sqrt(v_hat) + eps)
/// ```
pub struct Adam {
    /// Learning rate.
    pub lr: f32,
    /// Exponential decay rate for the first moment (default 0.9).
    pub beta1: f32,
    /// Exponential decay rate for the second moment (default 0.999).
    pub beta2: f32,
    /// Small constant for numerical stability (default 1e-8).
    pub eps: f32,
    /// Current timestep (starts at 1).
    t: u32,
    /// Per-parameter moment estimates: param_index -> (m, v).
    moments: BTreeMap<usize, (Tensor, Tensor)>,
}

impl Adam {
    /// Create a new Adam optimizer.
    ///
    /// # Errors
    ///
    /// Returns [`MlError::InvalidHyperparameter`] if any hyperparameter is
    /// non-positive or non-finite.
    pub fn new(lr: f32) -> MlResult<Self> {
        if lr <= 0.0 || !lr.is_finite() {
            return Err(MlError::InvalidHyperparameter {
                name: "lr",
                value: lr,
            });
        }
        Ok(Self {
            lr,
            beta1: 0.9,
            beta2: 0.999,
            eps: 1e-8,
            t: 0,
            moments: BTreeMap::new(),
        })
    }
}

impl Optimizer for Adam {
    fn step(&mut self, model: &mut Model) -> MlResult<()> {
        self.t += 1;
        let t = self.t as f32;
        let bc1 = 1.0 - self.beta1.powf(t);
        let bc2 = 1.0 - self.beta2.powf(t);

        let mut param_idx = 0;
        for layer in model.layers_mut() {
            let params = layer.parameters();
            let grads = layer.collect_grads();
            let mut updated = Vec::with_capacity(params.len());
            for (p, (_name, g)) in params.iter().zip(grads.iter()) {
                let shape = p.shape().as_slice().to_vec();
                let (m, v) = self.moments.entry(param_idx).or_insert_with(|| {
                    (
                        Tensor::zeros(shape.clone()).unwrap_or_else(|_| p.clone()),
                        Tensor::zeros(shape.clone()).unwrap_or_else(|_| p.clone()),
                    )
                });

                // Update moments.
                let m_data = m.as_mut_slice();
                let v_data = v.as_mut_slice();
                let new_data: Vec<f32> = p
                    .as_slice()
                    .iter()
                    .zip(g.as_slice().iter())
                    .enumerate()
                    .map(|(i, (&pv, &gv))| {
                        m_data[i] = self.beta1 * m_data[i] + (1.0 - self.beta1) * gv;
                        v_data[i] = self.beta2 * v_data[i] + (1.0 - self.beta2) * gv * gv;
                        let m_hat = m_data[i] / bc1;
                        let v_hat = v_data[i] / bc2;
                        pv - self.lr * m_hat / (v_hat.sqrt() + self.eps)
                    })
                    .collect();
                updated.push(Tensor::from_vec(new_data, shape)?);
                param_idx += 1;
            }
            layer.load_parameters(&updated)?;
        }
        Ok(())
    }

    fn name(&self) -> &'static str {
        "adam"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sgd_rejects_zero_lr() {
        assert!(SGD::new(0.0).is_err());
        assert!(SGD::new(-0.1).is_err());
        assert!(SGD::new(0.01).is_ok());
    }

    #[test]
    fn adam_rejects_invalid_lr() {
        assert!(Adam::new(0.0).is_err());
        assert!(Adam::new(f32::NAN).is_err());
        assert!(Adam::new(0.001).is_ok());
    }
}

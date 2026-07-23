//! Neural network layers built on the [`crate::autodiff`] `Var` tape.
//!
//! Each layer implements the [`Layer`] trait: a stateful forward pass that
//! builds graph nodes on the input `Var`'s tape, plus accessors for the
//! optimizer to read current parameter values and accumulated gradients
//! (after [`crate::Var::backward`]) and write updated values back.
//!
//! # Layers
//!
//! - [`Linear`]: fully-connected `y = x @ W + b` (Xavier-initialized; weight
//!   stored as `[input_dim, output_dim]` so forward is a plain matmul — no
//!   transpose node needed, gradient flows directly through the matmul op).
//! - [`ReLU`], [`Sigmoid`], [`Softmax`]: elementwise / rowwise activations.
//! - [`Dropout`]: rowwise Bernoulli mask (train-only; no-op in eval mode).

use crate::autodiff::Var;
use crate::error::{MlError, MlResult};
use buff_tensor::Tensor;
use std::cell::RefCell;

// ---------------------------------------------------------------------------
// Small deterministic LCG (no `rand` dep — keeps the crate pure-Rust per the
// workspace "no C library" rule. Reproducible across runs/tests).
// ---------------------------------------------------------------------------

/// Advance a 64-bit LCG state (Numerical Recipes constants) and return the
/// next pseudo-random `u64`.
fn lcg_next(state: &mut u64) -> u64 {
    *state = state
        .wrapping_mul(6364136223846793005)
        .wrapping_add(1442695040888963407);
    *state
}

/// Map an LCG state to a uniform `f32` in `[0, 1)`.
fn lcg_unit(state: &mut u64) -> f32 {
    let v = lcg_next(state);
    // Use the top 24 bits for f32 precision (mantissa is 24 bits).
    (v >> 40) as f32 / ((1u64 << 24) as f32)
}

// ---------------------------------------------------------------------------
// Layer trait
// ---------------------------------------------------------------------------

/// A neural network layer.
///
/// Layers are stateful (they hold parameters and remember the [`Var`]s they
/// created in the most recent forward pass so the optimizer can read their
/// gradients). The trait surface is intentionally small: forward + four
/// helpers used by [`crate::Model`] / [`crate::Optimizer`].
///
/// Layers are single-threaded for the MVP (they hold `Var`s which are
/// `!Send` because the tape uses `Rc`).
pub trait Layer {
    /// Forward pass: build graph nodes on `x`'s tape and return the output
    /// [`Var`]. When `training` is true, layers like [`Dropout`] apply their
    /// stochastic behavior; in eval mode they are deterministic.
    fn forward(&self, x: Var, training: bool) -> MlResult<Var>;

    /// Current parameter values as owned tensors (flat copies). Used by
    /// [`crate::Optimizer::step`] (read before update) and [`crate::Model::save`].
    /// The order is layer-defined and stable (e.g. `[weight, bias]` for
    /// [`Linear`]).
    fn parameters(&self) -> Vec<Tensor>;

    /// Replace parameter values wholesale. Used by [`crate::Optimizer::step`]
    /// (write the updated values back) and [`crate::Model::load`]. The `vals`
    /// slice must match the order + shapes produced by [`parameters`](Self::parameters).
    ///
    /// # Errors
    ///
    /// Returns [`MlError::ShapeMismatch`] if `vals` has the wrong count or a
    /// shape mismatch with the layer's current parameters.
    fn load_parameters(&mut self, vals: &[Tensor]) -> MlResult<()>;

    /// Gradients accumulated at each parameter from the last
    /// [`Var::backward`] sweep, as `(name, grad)` pairs. Order matches
    /// [`parameters`](Self::parameters).
    fn collect_grads(&self) -> Vec<(String, Tensor)>;

    /// The layer kind tag used in the JSON serialization format
    /// (e.g. `"linear"`, `"relu"`, `"dropout"`). Stable across versions —
    /// used by [`crate::Model::load`] to dispatch reconstruction.
    fn layer_kind(&self) -> &'static str;
}

// ---------------------------------------------------------------------------
// Linear
// ---------------------------------------------------------------------------

/// Fully-connected layer: `y = x @ W + b`.
///
/// Stores `weight` as `[input_dim, output_dim]` and `bias` as `[output_dim]`,
/// so forward is a plain `x[batch, in] @ W[in, out] = [batch, out]` followed
/// by the row-broadcast bias add. This layout lets the gradient flow directly
/// through the matmul op (no transpose node): `dW = x^T @ grad`,
/// `dx = grad @ W^T`.
///
/// Initialization is Xavier/Glorot uniform, drawn from a deterministic LCG
/// (seeded from the dimensions) so that construction is reproducible and
/// dependency-free. Tests that need known weights use
/// [`Layer::load_parameters`].
pub struct Linear {
    input_dim: usize,
    output_dim: usize,
    weight: Tensor, // [input_dim, output_dim]
    bias: Tensor,   // [output_dim]
    // Param Vars created during the most recent forward pass (for grad
    // collection). RefCell because forward takes &self but must record state.
    last_weight: RefCell<Option<Var>>,
    last_bias: RefCell<Option<Var>>,
}

impl Linear {
    /// Create a new `Linear(input_dim, output_dim)` with Xavier-uniform
    /// weights (deterministic LCG seed derived from the dimensions).
    ///
    /// # Errors
    ///
    /// - [`MlError::InvalidDimension`] if either dim is zero.
    pub fn new(input_dim: usize, output_dim: usize) -> MlResult<Self> {
        if input_dim == 0 {
            return Err(MlError::InvalidDimension {
                name: "input_dim",
                value: input_dim,
            });
        }
        if output_dim == 0 {
            return Err(MlError::InvalidDimension {
                name: "output_dim",
                value: output_dim,
            });
        }
        // Deterministic seed: mix the dims so different shapes get different
        // inits, but the SAME shape always gets the SAME init (reproducible).
        let mut state: u64 = 0x00C0_FFEE_1234_5678
            ^ (input_dim as u64).wrapping_mul(0x9E3779B97F4A7C15)
            ^ (output_dim as u64).wrapping_mul(0xD1B54A32D192ED03);
        // Xavier-uniform limit: sqrt(6 / (in + out)).
        let limit = (6.0f32 / (input_dim + output_dim) as f32).sqrt();
        let mut w = vec![0.0f32; input_dim * output_dim];
        for v in w.iter_mut() {
            *v = (lcg_unit(&mut state) - 0.5) * 2.0 * limit;
        }
        let weight = Tensor::from_vec(w, vec![input_dim, output_dim])?;
        let bias = Tensor::zeros(vec![output_dim])?;
        Ok(Linear {
            input_dim,
            output_dim,
            weight,
            bias,
            last_weight: RefCell::new(None),
            last_bias: RefCell::new(None),
        })
    }

    /// `(input_dim, output_dim)` — the constructor arguments.
    pub fn dims(&self) -> (usize, usize) {
        (self.input_dim, self.output_dim)
    }
}

impl Layer for Linear {
    fn forward(&self, x: Var, _training: bool) -> MlResult<Var> {
        // Validate input rank-2 with last dim == input_dim.
        let xv = x.value();
        if xv.shape().rank() != 2 {
            return Err(MlError::RankMismatch {
                actual: xv.shape().rank(),
                expected: 2,
            });
        }
        let in_dim = xv.shape().as_slice()[1];
        if in_dim != self.input_dim {
            return Err(MlError::ShapeMismatch {
                lhs: xv.shape().as_slice().to_vec(),
                rhs: vec![xv.shape().as_slice()[0], self.input_dim],
            });
        }
        let tape = x.tape().clone();
        // Parameter leaf vars (requires_grad so backward fills their grads).
        let w = tape.leaf(self.weight.clone(), true)?;
        let b = tape.leaf(self.bias.clone(), true)?;
        // y = x @ W   (x:[batch,in] @ W:[in,out] = [batch,out])
        let z = x.matmul(&w)?;
        let out = z.add_row_bias(&b)?;
        *self.last_weight.borrow_mut() = Some(w);
        *self.last_bias.borrow_mut() = Some(b);
        Ok(out)
    }

    fn parameters(&self) -> Vec<Tensor> {
        vec![self.weight.clone(), self.bias.clone()]
    }

    fn load_parameters(&mut self, vals: &[Tensor]) -> MlResult<()> {
        if vals.len() != 2 {
            return Err(MlError::ShapeMismatch {
                lhs: vec![vals.len()],
                rhs: vec![2],
            });
        }
        if vals[0].shape().as_slice() != [self.input_dim, self.output_dim] {
            return Err(MlError::ShapeMismatch {
                lhs: vals[0].shape().as_slice().to_vec(),
                rhs: vec![self.input_dim, self.output_dim],
            });
        }
        if vals[1].shape().as_slice() != [self.output_dim] {
            return Err(MlError::ShapeMismatch {
                lhs: vals[1].shape().as_slice().to_vec(),
                rhs: vec![self.output_dim],
            });
        }
        self.weight = vals[0].clone();
        self.bias = vals[1].clone();
        Ok(())
    }

    fn collect_grads(&self) -> Vec<(String, Tensor)> {
        let w_grad = self
            .last_weight
            .borrow()
            .as_ref()
            .and_then(Var::grad)
            .unwrap_or_else(|| {
                Tensor::zeros(vec![self.input_dim, self.output_dim])
                    .unwrap_or_else(|_| self.weight.clone())
            });
        let b_grad = self
            .last_bias
            .borrow()
            .as_ref()
            .and_then(Var::grad)
            .unwrap_or_else(|| {
                Tensor::zeros(vec![self.output_dim]).unwrap_or_else(|_| self.bias.clone())
            });
        vec![("weight".to_string(), w_grad), ("bias".to_string(), b_grad)]
    }

    fn layer_kind(&self) -> &'static str {
        "linear"
    }
}

// ---------------------------------------------------------------------------
// ReLU / Sigmoid / Softmax (no parameters)
// ---------------------------------------------------------------------------

/// ReLU activation: `max(0, x)`. Stateless.
#[derive(Debug, Clone, Default)]
pub struct ReLU;

impl ReLU {
    /// Construct a `ReLU` layer.
    pub fn new() -> Self {
        ReLU
    }
}

impl Layer for ReLU {
    fn forward(&self, x: Var, _training: bool) -> MlResult<Var> {
        x.relu()
    }
    fn parameters(&self) -> Vec<Tensor> {
        Vec::new()
    }
    fn load_parameters(&mut self, _vals: &[Tensor]) -> MlResult<()> {
        Ok(())
    }
    fn collect_grads(&self) -> Vec<(String, Tensor)> {
        Vec::new()
    }
    fn layer_kind(&self) -> &'static str {
        "relu"
    }
}

/// Sigmoid activation: `1 / (1 + e^-x)`. Stateless.
#[derive(Debug, Clone, Default)]
pub struct Sigmoid;

impl Sigmoid {
    /// Construct a `Sigmoid` layer.
    pub fn new() -> Self {
        Sigmoid
    }
}

impl Layer for Sigmoid {
    fn forward(&self, x: Var, _training: bool) -> MlResult<Var> {
        x.sigmoid()
    }
    fn parameters(&self) -> Vec<Tensor> {
        Vec::new()
    }
    fn load_parameters(&mut self, _vals: &[Tensor]) -> MlResult<()> {
        Ok(())
    }
    fn collect_grads(&self) -> Vec<(String, Tensor)> {
        Vec::new()
    }
    fn layer_kind(&self) -> &'static str {
        "sigmoid"
    }
}

/// Softmax activation over the last axis (per-row for rank-2 input). Stateless.
#[derive(Debug, Clone, Default)]
pub struct Softmax;

impl Softmax {
    /// Construct a `Softmax` layer.
    pub fn new() -> Self {
        Softmax
    }
}

impl Layer for Softmax {
    fn forward(&self, x: Var, _training: bool) -> MlResult<Var> {
        x.softmax()
    }
    fn parameters(&self) -> Vec<Tensor> {
        Vec::new()
    }
    fn load_parameters(&mut self, _vals: &[Tensor]) -> MlResult<()> {
        Ok(())
    }
    fn collect_grads(&self) -> Vec<(String, Tensor)> {
        Vec::new()
    }
    fn layer_kind(&self) -> &'static str {
        "softmax"
    }
}

// ---------------------------------------------------------------------------
// Dropout (stateful: rate; behavior gated by `training`)
// ---------------------------------------------------------------------------

/// Inverted dropout: in training mode, zeroes each element with probability
/// `rate` and scales survivors by `1 / (1 - rate)` so the expected value is
/// preserved. In eval mode (`training = false`), it is a no-op identity.
///
/// The mask is drawn from the deterministic LCG (seeded from the input's
/// byte length + a per-layer counter) so runs are reproducible. A real
/// framework would thread a seeded RNG; that is deferred to v1.18+.
pub struct Dropout {
    rate: f32,
    inv_keep: f32, // 1 / (1 - rate)
    counter: RefCell<u64>,
}

impl Dropout {
    /// Construct a `Dropout(rate)`. `rate` must be in `[0, 1)`.
    ///
    /// # Errors
    ///
    /// - [`MlError::InvalidProbability`] if `rate` is outside `[0, 1)`.
    pub fn new(rate: f32) -> MlResult<Self> {
        if !(0.0..1.0).contains(&rate) {
            return Err(MlError::InvalidProbability { value: rate });
        }
        // rate < 1 enforced above; keep the guard for clarity + clippy.
        let inv_keep = if (1.0 - rate) > 0.0 {
            1.0 / (1.0 - rate)
        } else {
            0.0
        };
        Ok(Dropout {
            rate,
            inv_keep,
            counter: RefCell::new(0xA5A5_5A5A),
        })
    }

    /// The dropout rate (probability of zeroing an element in train mode).
    pub fn rate(&self) -> f32 {
        self.rate
    }
}

impl Layer for Dropout {
    fn forward(&self, x: Var, training: bool) -> MlResult<Var> {
        if !training || self.rate == 0.0 {
            return Ok(x);
        }
        // Build the mask from the deterministic LCG. The mask node multiplies
        // the input elementwise; its backward routes grad through the mask.
        let xv = x.value();
        let mut state = {
            let c = *self.counter.borrow();
            // Advance counter for next forward (per-layer determinism).
            *self.counter.borrow_mut() = c.wrapping_add(xv.len() as u64 + 1);
            c ^ (xv.len() as u64).wrapping_mul(0x100000001B3)
        };
        let mut mask_data = vec![0.0f32; xv.len()];
        for m in mask_data.iter_mut() {
            if lcg_unit(&mut state) >= self.rate {
                *m = self.inv_keep;
            }
        }
        let tape = x.tape().clone();
        let mask = tape.leaf(
            Tensor::from_vec(mask_data, xv.shape().as_slice().to_vec())?,
            false,
        )?;
        x.mul(&mask)
    }
    fn parameters(&self) -> Vec<Tensor> {
        Vec::new()
    }
    fn load_parameters(&mut self, _vals: &[Tensor]) -> MlResult<()> {
        Ok(())
    }
    fn collect_grads(&self) -> Vec<(String, Tensor)> {
        Vec::new()
    }
    fn layer_kind(&self) -> &'static str {
        "dropout"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::autodiff::Tape;

    #[test]
    fn linear_new_rejects_zero_dim() {
        assert!(Linear::new(0, 3).is_err());
        assert!(Linear::new(3, 0).is_err());
        assert!(Linear::new(2, 3).is_ok());
    }

    #[test]
    fn linear_forward_shape() {
        let tape = Tape::new();
        let x = tape
            .leaf(Tensor::from_vec(vec![1.0, 2.0, 3.0, 4.0], vec![2, 2]).unwrap(), false)
            .unwrap();
        let l = Linear::new(2, 3).unwrap();
        let y = l.forward(x, false).unwrap();
        assert_eq!(y.value().shape().as_slice(), &[2, 3]);
    }

    #[test]
    fn linear_forward_rejects_wrong_input_dim() {
        let tape = Tape::new();
        let x = tape
            .leaf(Tensor::from_vec(vec![1.0, 2.0, 3.0], vec![1, 3]).unwrap(), false)
            .unwrap();
        let l = Linear::new(2, 3).unwrap();
        assert!(l.forward(x, false).is_err());
    }

    #[test]
    fn linear_load_parameters_roundtrips() {
        let mut l = Linear::new(2, 3).unwrap();
        let w = Tensor::from_vec(vec![1.0; 6], vec![2, 3]).unwrap();
        let b = Tensor::from_vec(vec![0.5; 3], vec![3]).unwrap();
        l.load_parameters(&[w.clone(), b.clone()]).unwrap();
        let p = l.parameters();
        assert_eq!(p[0].as_slice(), w.as_slice());
        assert_eq!(p[1].as_slice(), b.as_slice());
    }

    #[test]
    fn linear_load_parameters_rejects_wrong_count() {
        let mut l = Linear::new(2, 3).unwrap();
        assert!(l.load_parameters(&[Tensor::zeros(vec![2, 3]).unwrap()]).is_err());
    }

    #[test]
    fn dropout_rejects_invalid_rate() {
        assert!(Dropout::new(-0.1).is_err());
        assert!(Dropout::new(1.0).is_err());
        assert!(Dropout::new(0.5).is_ok());
        assert!(Dropout::new(0.0).is_ok());
    }

    #[test]
    fn dropout_is_noop_in_eval_mode() {
        let tape = Tape::new();
        let x = tape
            .leaf(Tensor::from_vec(vec![1.0, 2.0, 3.0, 4.0], vec![2, 2]).unwrap(), false)
            .unwrap();
        let d = Dropout::new(0.5).unwrap();
        let y = d.forward(x.clone(), false).unwrap();
        // Eval mode -> identity.
        assert_eq!(y.value().as_slice(), x.value().as_slice());
    }

    #[test]
    fn dropout_zero_rate_is_identity() {
        let tape = Tape::new();
        let x = tape
            .leaf(Tensor::from_vec(vec![1.0, 2.0, 3.0, 4.0], vec![4]).unwrap(), false)
            .unwrap();
        let d = Dropout::new(0.0).unwrap();
        let y = d.forward(x.clone(), true).unwrap();
        assert_eq!(y.value().as_slice(), x.value().as_slice());
    }

    #[test]
    fn activation_layer_kinds() {
        assert_eq!(ReLU::new().layer_kind(), "relu");
        assert_eq!(Sigmoid::new().layer_kind(), "sigmoid");
        assert_eq!(Softmax::new().layer_kind(), "softmax");
        assert_eq!(Dropout::new(0.1).unwrap().layer_kind(), "dropout");
    }
}

//! Reverse-mode automatic differentiation on `buff_tensor::Tensor`.
//!
//! # Design (micrograd pattern)
//!
//! A [`Tape`] is a define-by-run computation graph: every [`Var`] holds an
//! index into a shared [`Tape`]. Operations between [`Var`]s push a new
//! [`Node`] onto the tape that records:
//!
//! 1. The forward `value` (a [`Tensor`]),
//! 2. An accumulated `grad` (zero-initialized, filled during the sweep),
//! 3. An optional `backward` closure that, given `&mut TapeInner`, propagates
//!    this node's accumulated grad to its inputs.
//!
//! [`Var::backward`] seeds the output node's grad to all-ones, then walks
//! nodes in reverse order (which is reverse-topological, since the tape is
//! built parent-before-child). Each op's closure reads its accumulated grad
//! and adds contributions into its inputs' grad slots. Leaf nodes (created
//! via [`Tape::leaf`] with `requires_grad = true`) keep their accumulated
//! grad for the caller to read via [`Var::grad`].
//!
//! This mirrors Karpathy's micrograd (<https://github.com/karpathy/micrograd>)
//! lifted from scalars to tensors. The closures capture only indices and
//! owned intermediate tensors (never `Rc<Tape>`), so there are no reference
//! cycles; the tape is freed when the last [`Var`] referencing it drops.
//!
//! # Single-threaded
//!
//! The tape uses `Rc<RefCell<...>>` for shared interior mutability, so [`Var`]
//! is `!Send` / `!Sync`. Multi-threaded data-parallel training is a v1.19+
//! concern; the MVP runs the whole forward + backward sweep on one thread
//! (the elementwise loss / activation ops inside still use rayon internally
//! via `buff-tensor::math`).

use crate::error::{MlError, MlResult};
use buff_tensor::Tensor;
use std::cell::RefCell;
use std::rc::Rc;

// ---------------------------------------------------------------------------
// Helpers (infallible — shape already validated by the originating Tensor)
// ---------------------------------------------------------------------------

/// Build a zero-filled tensor with the same shape as `t`. Infallible because
/// the shape came from a valid `Tensor` (rank already ≤ `MVP_RANK_CAP`); the
/// defensive fallbacks only trigger on a logic bug, never on user input.
pub(crate) fn zero_tensor_like(t: &Tensor) -> Tensor {
    let n = t.len();
    let shape = t.shape().as_slice().to_vec();
    match Tensor::from_vec(vec![0.0f32; n], shape) {
        Ok(z) => z,
        Err(_) => match Tensor::from_vec(vec![0.0f32; n], vec![n.max(1)]) {
            Ok(z) => z,
            Err(_) => t.clone(),
        },
    }
}

/// In-place elementwise accumulation: `dst[k] += src[k]` for the overlapping
/// prefix. Used by every backward closure to add gradient contributions into
/// a node's accumulated grad slot. Shapes match by construction (both equal
/// the node's value shape); the min-length guard is purely defensive.
pub(crate) fn accum_add(dst: &mut Tensor, src: &Tensor) {
    for (d, &s) in dst.as_mut_slice().iter_mut().zip(src.as_slice().iter()) {
        *d += s;
    }
}

/// Compute `x.transpose()` defensively; returns a zero-shaped tensor on the
/// (unreachable) failure path so callers never panic.
fn transpose_or_zero(x: &Tensor) -> Tensor {
    x.transpose().unwrap_or_else(|_| zero_tensor_like(x))
}

// ---------------------------------------------------------------------------
// Tape internals
// ---------------------------------------------------------------------------

/// Closure type for backward propagation: given mutable access to the tape,
/// adds gradient contributions to input nodes.
type BackwardFn = Box<dyn FnOnce(&mut TapeInner)>;

/// The shared computation graph backing one forward pass.
///
/// Held behind `Rc<RefCell<...>>` so every [`Var`] on the tape shares one
/// mutable interior. Construct one per training step via [`Tape::new`].
#[derive(Debug)]
pub struct Tape {
    inner: RefCell<TapeInner>,
}

#[derive(Debug)]
pub(crate) struct TapeInner {
    pub(crate) nodes: Vec<Node>,
}

pub(crate) struct Node {
    /// Forward value (immutable after the node is pushed).
    pub(crate) value: Tensor,
    /// Accumulated gradient (filled during the backward sweep).
    pub(crate) grad: Tensor,
    /// Reverse-mode propagation closure. `None` for leaf nodes; consumed
    /// (taken out of the `Option`) the first time the sweep visits it.
    backward: Option<BackwardFn>,
    /// Whether this node participates in gradient accumulation. Leaf nodes
    /// created via [`Tape::leaf`] with `requires_grad = true` report their
    /// accumulated grad via [`Var::grad`].
    requires_grad: bool,
}

impl std::fmt::Debug for Node {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Node")
            .field("value", &self.value)
            .field("grad", &self.grad)
            .field("has_backward", &self.backward.is_some())
            .field("requires_grad", &self.requires_grad)
            .finish()
    }
}

impl Tape {
    /// Create a fresh, empty tape. One per forward pass.
    pub fn new() -> Rc<Tape> {
        Rc::new(Tape {
            inner: RefCell::new(TapeInner { nodes: Vec::new() }),
        })
    }

    /// Create a leaf [`Var`] wrapping `value`. If `requires_grad` is true the
    /// node accumulates a gradient during [`Var::backward`] that the caller
    /// reads via [`Var::grad`]; otherwise the node is a constant.
    ///
    /// # Errors
    ///
    /// Propagates shape-validation errors from the zero-init grad buffer
    /// (only fails if `value`'s shape exceeds the rank cap — i.e. never for
    /// shapes that came from a valid `Tensor`).
    pub fn leaf(self: &Rc<Tape>, value: Tensor, requires_grad: bool) -> MlResult<Var> {
        self.push_node(value, requires_grad, None)
    }

    /// The current number of nodes on the tape (next free index).
    pub(crate) fn next_index(&self) -> usize {
        self.inner.borrow().nodes.len()
    }

    /// Push a fully-formed node and return a [`Var`] handle to it.
    pub(crate) fn push_node(
        self: &Rc<Tape>,
        value: Tensor,
        requires_grad: bool,
        backward: Option<BackwardFn>,
    ) -> MlResult<Var> {
        let grad = zero_tensor_like(&value);
        let mut inner = self.inner.borrow_mut();
        inner.nodes.push(Node {
            value,
            grad,
            backward,
            requires_grad,
        });
        let index = inner.nodes.len() - 1;
        Ok(Var {
            tape: Rc::clone(self),
            index,
            requires_grad,
        })
    }

    /// Run the reverse-mode sweep from `index`. Seeds the grad at `index` to
    /// all-ones (the vector-Jacobian product seed), then walks nodes in
    /// reverse, invoking each op's backward closure exactly once.
    ///
    /// # Errors
    ///
    /// Propagates shape-validation errors from the seed grad buffer.
    fn backward_from(&self, index: usize) -> MlResult<()> {
        let mut inner = self.inner.borrow_mut();
        // Seed: grad of the output w.r.t. itself is 1 everywhere.
        let seed_shape = inner.nodes[index].value.shape().as_slice().to_vec();
        let seed = Tensor::from_vec(
            vec![1.0f32; inner.nodes[index].value.len().max(1)],
            seed_shape,
        )
        .unwrap_or_else(|_| zero_tensor_like(&inner.nodes[index].value));
        inner.nodes[index].grad = seed;
        // Reverse sweep: reverse-index order is reverse-topological because
        // the tape is built parent-before-child.
        for i in (0..=index).rev() {
            let closure_opt = std::mem::take(&mut inner.nodes[i].backward);
            if let Some(closure) = closure_opt {
                closure(&mut inner);
            }
        }
        Ok(())
    }
}

impl Default for Tape {
    fn default() -> Self {
        // `Tape::new` returns `Rc<Tape>`; this default builds a bare owned
        // tape for `Default` conformance (rarely used — callers use `new`).
        let rc = Tape::new();
        // Reconstruct ownership: safe because `new` is the only ref so far.
        match Rc::try_unwrap(rc) {
            Ok(t) => t,
            Err(rc) => {
                // Should not happen (fresh tape), but fall back to a clone
                // of the inner state to honor the `Default` contract.
                let inner = rc.inner.borrow().nodes.iter().map(|n| Node {
                    value: n.value.clone(),
                    grad: n.grad.clone(),
                    backward: None,
                    requires_grad: n.requires_grad,
                }).collect();
                Tape {
                    inner: RefCell::new(TapeInner { nodes: inner }),
                }
            }
        }
    }
}

/// A node handle in the computation graph — a gradient-tracking wrapper
/// around a [`Tensor`].
///
/// Built by [`Tape::leaf`] (for inputs / parameters / constants) or returned
/// by the `pub(crate)` op methods ([`Var::add`], [`Var::matmul`], ...). The
/// three public methods mirror the micrograd / PyTorch surface:
/// [`Var::requires_grad`], [`Var::backward`], [`Var::grad`].
#[derive(Debug)]
pub struct Var {
    tape: Rc<Tape>,
    index: usize,
    requires_grad: bool,
}

impl Var {
    /// Whether this node accumulates a gradient during backward.
    pub fn requires_grad(&self) -> bool {
        self.requires_grad
    }

    /// The forward value (cloned — `Var` does not expose mutable access to
    /// its stored `Tensor`).
    pub fn value(&self) -> Tensor {
        self.tape.inner.borrow().nodes[self.index].value.clone()
    }

    /// Run reverse-mode autodiff from this node. Seeds this node's grad to
    /// all-ones (the VJP seed for a scalar loss) and walks the tape backward,
    /// accumulating grads into every ancestor that `requires_grad`.
    ///
    /// # Errors
    ///
    /// Propagates shape-validation errors from the seed grad buffer.
    pub fn backward(&self) -> MlResult<()> {
        self.tape.backward_from(self.index)
    }

    /// The accumulated gradient for this node, or `None` if the node does not
    /// `requires_grad`. Clones the stored grad buffer (cheap for small
    /// tensors; the MVP does not expose a reference view because of the
    /// shared `RefCell` borrow).
    pub fn grad(&self) -> Option<Tensor> {
        if !self.requires_grad {
            return None;
        }
        Some(self.tape.inner.borrow().nodes[self.index].grad.clone())
    }

    /// The backing tape (shared `Rc` handle). Used by layers to push new
    /// nodes derived from an input `Var`.
    pub(crate) fn tape(&self) -> &Rc<Tape> {
        &self.tape
    }

    /// This node's index on the tape (used by loss fns that build a single
    /// combined backward closure referencing the input node).
    pub(crate) fn index(&self) -> usize {
        self.index
    }

    // -----------------------------------------------------------------
    // Op helpers (pub(crate)) — used by layers / losses. Each records a
    // forward node + a backward closure. Closures capture only indices and
    // owned intermediate tensors (no `Rc<Tape>` -> no cycles).
    // -----------------------------------------------------------------

    /// Elementwise `self + rhs`. Backward: identity to both inputs.
    #[allow(dead_code)]
    pub(crate) fn add(&self, rhs: &Var) -> MlResult<Var> {
        let xv = self.value();
        let yv = rhs.value();
        let zv = xv.add(&yv)?;
        let rg = self.requires_grad || rhs.requires_grad;
        let xi = self.index;
        let yi = rhs.index;
        let zi = self.tape.next_index();
        let backward: Box<dyn FnOnce(&mut TapeInner)> = Box::new(move |inner| {
            let gz = inner.nodes[zi].grad.clone();
            accum_add(&mut inner.nodes[xi].grad, &gz);
            accum_add(&mut inner.nodes[yi].grad, &gz);
        });
        self.tape.push_node(zv, rg, Some(backward))
    }

    /// Elementwise `self - rhs`. Backward: `+grad` to self, `-grad` to rhs.
    pub(crate) fn sub(&self, rhs: &Var) -> MlResult<Var> {
        let xv = self.value();
        let yv = rhs.value();
        let zv = xv.sub(&yv)?;
        let rg = self.requires_grad || rhs.requires_grad;
        let xi = self.index;
        let yi = rhs.index;
        let zi = self.tape.next_index();
        let backward: Box<dyn FnOnce(&mut TapeInner)> = Box::new(move |inner| {
            let gz = inner.nodes[zi].grad.clone();
            accum_add(&mut inner.nodes[xi].grad, &gz);
            // rhs gets -grad.
            let neg = gz.neg().unwrap_or_else(|_| zero_tensor_like(&gz));
            accum_add(&mut inner.nodes[yi].grad, &neg);
        });
        self.tape.push_node(zv, rg, Some(backward))
    }

    /// Elementwise `self * rhs` (Hadamard). Backward: `grad*rhs` to self,
    /// `grad*self` to rhs.
    pub(crate) fn mul(&self, rhs: &Var) -> MlResult<Var> {
        let xv = self.value();
        let yv = rhs.value();
        let zv = xv.mul(&yv)?;
        let rg = self.requires_grad || rhs.requires_grad;
        let xi = self.index;
        let yi = rhs.index;
        let zi = self.tape.next_index();
        // Capture forward values for the cross-terms.
        let xv_cap = xv.clone();
        let yv_cap = yv.clone();
        let backward: Box<dyn FnOnce(&mut TapeInner)> = Box::new(move |inner| {
            let gz = inner.nodes[zi].grad.clone();
            let gx = gz.mul(&yv_cap).unwrap_or_else(|_| zero_tensor_like(&gz));
            let gy = gz.mul(&xv_cap).unwrap_or_else(|_| zero_tensor_like(&gz));
            accum_add(&mut inner.nodes[xi].grad, &gx);
            accum_add(&mut inner.nodes[yi].grad, &gy);
        });
        self.tape.push_node(zv, rg, Some(backward))
    }

    /// Scalar multiply `self * scalar`. Backward: `grad*scalar` to self.
    pub(crate) fn scale(&self, scalar: f32) -> MlResult<Var> {
        let xv = self.value();
        let zv = xv.scale(scalar)?;
        let rg = self.requires_grad;
        let xi = self.index;
        let zi = self.tape.next_index();
        let backward: Box<dyn FnOnce(&mut TapeInner)> = Box::new(move |inner| {
            let gz = inner.nodes[zi].grad.clone();
            let gx = gz.scale(scalar).unwrap_or_else(|_| zero_tensor_like(&gz));
            accum_add(&mut inner.nodes[xi].grad, &gx);
        });
        self.tape.push_node(zv, rg, Some(backward))
    }

    /// 2-D matrix multiply `self @ rhs`. Backward: `grad @ rhs^T` to self,
    /// `self^T @ grad` to rhs.
    pub(crate) fn matmul(&self, rhs: &Var) -> MlResult<Var> {
        let xv = self.value();
        let yv = rhs.value();
        let zv = xv.matmul(&yv)?;
        let rg = self.requires_grad || rhs.requires_grad;
        let xi = self.index;
        let yi = rhs.index;
        let zi = self.tape.next_index();
        // Capture transposes for the backward (shapes guaranteed compatible).
        let y_t = transpose_or_zero(&yv);
        let x_t = transpose_or_zero(&xv);
        let backward: Box<dyn FnOnce(&mut TapeInner)> = Box::new(move |inner| {
            let gz = inner.nodes[zi].grad.clone();
            // gx += gz @ y^T   (shape [m,n] @ [n,k] -> [m,k] == x shape)
            let gx = gz.matmul(&y_t).unwrap_or_else(|_| zero_tensor_like(&gz));
            // gy += x^T @ gz   (shape [k,m] @ [m,n] -> [k,n] == y shape)
            let gy = x_t.matmul(&gz).unwrap_or_else(|_| zero_tensor_like(&gz));
            accum_add(&mut inner.nodes[xi].grad, &gx);
            accum_add(&mut inner.nodes[yi].grad, &gy);
        });
        self.tape.push_node(zv, rg, Some(backward))
    }

    /// Row-broadcast bias add: `self` is `[batch, out]`, `bias` is `[out]`.
    /// Adds `bias` to every row. Backward: `grad` to self; `sum_axis(0)` of
    /// grad to bias (collapsing the batch dimension).
    pub(crate) fn add_row_bias(&self, bias: &Var) -> MlResult<Var> {
        let xv = self.value();
        let bv = bias.value();
        if xv.shape().rank() != 2 {
            return Err(MlError::RankMismatch {
                actual: xv.shape().rank(),
                expected: 2,
            });
        }
        let dims = xv.shape().as_slice();
        let batch = dims[0];
        let out = dims[1];
        if bv.shape().rank() != 1 || bv.len() != out {
            return Err(MlError::ShapeMismatch {
                lhs: bv.shape().as_slice().to_vec(),
                rhs: vec![out],
            });
        }
        // Forward: tile bias to [batch, out] and add.
        let mut tiled = xv.clone();
        {
            let buf = tiled.as_mut_slice();
            let bb = bv.as_slice();
            for r in 0..batch {
                for c in 0..out {
                    buf[r * out + c] += bb[c];
                }
            }
        }
        let rg = self.requires_grad || bias.requires_grad;
        let xi = self.index;
        let bi = bias.index;
        let zi = self.tape.next_index();
        let backward: Box<dyn FnOnce(&mut TapeInner)> = Box::new(move |inner| {
            let gz = inner.nodes[zi].grad.clone();
            // self.grad += gz (same shape).
            accum_add(&mut inner.nodes[xi].grad, &gz);
            // bias.grad += sum over batch of gz  -> shape [out].
            let gb = gz.sum_axis(0).unwrap_or_else(|_| zero_tensor_like(&gz));
            accum_add(&mut inner.nodes[bi].grad, &gb);
        });
        self.tape.push_node(tiled, rg, Some(backward))
    }

    /// ReLU activation. Backward: `grad * (x > 0)` (mask).
    pub(crate) fn relu(&self) -> MlResult<Var> {
        let xv = self.value();
        let zv: Tensor = {
            let mut z = xv.clone();
            for v in z.as_mut_slice().iter_mut() {
                if *v < 0.0 {
                    *v = 0.0;
                }
            }
            z
        };
        let rg = self.requires_grad;
        let xi = self.index;
        let zi = self.tape.next_index();
        // Capture the forward mask (1 where x>0, else 0).
        let xv_cap = xv.clone();
        let backward: Box<dyn FnOnce(&mut TapeInner)> = Box::new(move |inner| {
            let gz = inner.nodes[zi].grad.clone();
            let mut gx = gz.clone();
            let xs = xv_cap.as_slice();
            for (g, &x) in gx.as_mut_slice().iter_mut().zip(xs.iter()) {
                if x <= 0.0 {
                    *g = 0.0;
                }
            }
            accum_add(&mut inner.nodes[xi].grad, &gx);
        });
        self.tape.push_node(zv, rg, Some(backward))
    }

    /// Sigmoid activation `1 / (1 + e^-x)`. Backward: `grad * s * (1 - s)`.
    pub(crate) fn sigmoid(&self) -> MlResult<Var> {
        let xv = self.value();
        let zv: Tensor = {
            let mut z = xv.clone();
            for v in z.as_mut_slice().iter_mut() {
                *v = 1.0 / (1.0 + (-*v).exp());
            }
            z
        };
        let rg = self.requires_grad;
        let xi = self.index;
        let zi = self.tape.next_index();
        // Capture the sigmoid output s for the backward (s*(1-s)).
        let s_cap = zv.clone();
        let backward: Box<dyn FnOnce(&mut TapeInner)> = Box::new(move |inner| {
            let gz = inner.nodes[zi].grad.clone();
            let mut gx = gz.clone();
            let ss = s_cap.as_slice();
            for (g, &s) in gx.as_mut_slice().iter_mut().zip(ss.iter()) {
                *g *= s * (1.0 - s);
            }
            accum_add(&mut inner.nodes[xi].grad, &gx);
        });
        self.tape.push_node(zv, rg, Some(backward))
    }

    /// Per-row softmax over the last axis (numerically stable:
    /// subtract row-max before exp). Input shape `[batch, classes]`.
    /// Backward: per-row Jacobian-vector product
    /// `dx[i,j] = s[i,j] * (g[i,j] - sum_k(g[i,k] * s[i,k]))`.
    pub(crate) fn softmax(&self) -> MlResult<Var> {
        let xv = self.value();
        if xv.shape().rank() != 2 {
            return Err(MlError::RankMismatch {
                actual: xv.shape().rank(),
                expected: 2,
            });
        }
        let dims = xv.shape().as_slice();
        let batch = dims[0];
        let classes = dims[1];
        // Forward: stable softmax.
        let mut zdata = vec![0.0f32; batch * classes];
        let xs = xv.as_slice();
        for r in 0..batch {
            let row = &xs[r * classes..(r + 1) * classes];
            let m = row.iter().copied().fold(f32::NEG_INFINITY, f32::max);
            let mut sum = 0.0f32;
            for c in 0..classes {
                let e = (row[c] - m).exp();
                zdata[r * classes + c] = e;
                sum += e;
            }
            if sum > 0.0 {
                for c in 0..classes {
                    zdata[r * classes + c] /= sum;
                }
            }
        }
        let zv = Tensor::from_vec(zdata, vec![batch, classes])?;
        let rg = self.requires_grad;
        let xi = self.index;
        let zi = self.tape.next_index();
        let s_cap = zv.clone();
        let backward: Box<dyn FnOnce(&mut TapeInner)> = Box::new(move |inner| {
            let gz = inner.nodes[zi].grad.clone();
            let gs = gz.as_slice();
            let ss = s_cap.as_slice();
            let mut gx = vec![0.0f32; batch * classes];
            for r in 0..batch {
                let grow = &gs[r * classes..(r + 1) * classes];
                let srow = &ss[r * classes..(r + 1) * classes];
                let dot: f32 = grow.iter().zip(srow.iter()).map(|(&g, &s)| g * s).sum();
                for c in 0..classes {
                    // dx = s * (g - dot)  where dot = sum(g*s)
                    gx[r * classes + c] = srow[c] * (grow[c] - dot);
                }
            }
            let gx_t = Tensor::from_vec(gx, vec![batch, classes])
                .unwrap_or_else(|_| zero_tensor_like(&gz));
            accum_add(&mut inner.nodes[xi].grad, &gx_t);
        });
        self.tape.push_node(zv, rg, Some(backward))
    }

    /// Sum of all elements, returned as a scalar `[1]`-shaped `Var`.
    /// Backward: broadcast the scalar grad to the input shape (every input
    /// element gets the full scalar grad).
    pub(crate) fn sum_all(&self) -> MlResult<Var> {
        let xv = self.value();
        let s = xv.sum_all();
        let zv = Tensor::from_vec(vec![s], vec![1])?;
        let rg = self.requires_grad;
        let xi = self.index;
        let zi = self.tape.next_index();
        let x_shape = xv.shape().as_slice().to_vec();
        let x_len = xv.len();
        let backward: Box<dyn FnOnce(&mut TapeInner)> = Box::new(move |inner| {
            let g_scalar = inner.nodes[zi].grad.as_slice().first().copied().unwrap_or(0.0);
            let broadcast = Tensor::from_vec(vec![g_scalar; x_len], x_shape.clone())
                .unwrap_or_else(|_| zero_tensor_like(&inner.nodes[xi].grad));
            accum_add(&mut inner.nodes[xi].grad, &broadcast);
        });
        self.tape.push_node(zv, rg, Some(backward))
    }
}

impl Clone for Var {
    /// Clone produces another handle to the SAME tape node (shares the
    /// underlying `Rc<Tape>` + index). Gradients accumulated through either
    /// clone flow into the single shared node.
    fn clone(&self) -> Self {
        Var {
            tape: Rc::clone(&self.tape),
            index: self.index,
            requires_grad: self.requires_grad,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn approx(a: f32, b: f32) -> bool {
        (a - b).abs() < 1e-4
    }

    #[test]
    fn var_leaf_requires_grad_flag() {
        let tape = Tape::new();
        let v = tape.leaf(Tensor::from_vec(vec![1.0, 2.0], vec![2]).unwrap(), true).unwrap();
        assert!(v.requires_grad());
        let c = tape.leaf(Tensor::from_vec(vec![1.0], vec![1]).unwrap(), false).unwrap();
        assert!(!c.requires_grad());
    }

    #[test]
    fn var_grad_none_when_no_requires_grad() {
        let tape = Tape::new();
        let c = tape.leaf(Tensor::from_vec(vec![3.0], vec![1]).unwrap(), false).unwrap();
        let s = c.sum_all().unwrap();
        s.backward().unwrap();
        assert!(c.grad().is_none());
    }

    #[test]
    fn add_backward_is_identity() {
        let tape = Tape::new();
        let a = tape.leaf(Tensor::from_vec(vec![2.0, 3.0], vec![2]).unwrap(), true).unwrap();
        let b = tape.leaf(Tensor::from_vec(vec![5.0, 7.0], vec![2]).unwrap(), true).unwrap();
        let z = a.add(&b).unwrap();
        z.backward().unwrap();
        assert_eq!(z.value().as_slice(), &[7.0, 10.0]);
        assert_eq!(a.grad().unwrap().as_slice(), &[1.0, 1.0]);
        assert_eq!(b.grad().unwrap().as_slice(), &[1.0, 1.0]);
    }

    #[test]
    fn mul_backward_swaps_inputs() {
        let tape = Tape::new();
        let a = tape.leaf(Tensor::from_vec(vec![2.0, 3.0], vec![2]).unwrap(), true).unwrap();
        let b = tape.leaf(Tensor::from_vec(vec![4.0, 5.0], vec![2]).unwrap(), true).unwrap();
        let z = a.mul(&b).unwrap();
        z.backward().unwrap();
        // dz/da = b, dz/db = a
        assert_eq!(a.grad().unwrap().as_slice(), &[4.0, 5.0]);
        assert_eq!(b.grad().unwrap().as_slice(), &[2.0, 3.0]);
    }

    #[test]
    fn scale_backward_multiplies_grad() {
        let tape = Tape::new();
        let a = tape.leaf(Tensor::from_vec(vec![1.0, 2.0], vec![2]).unwrap(), true).unwrap();
        let z = a.scale(3.0).unwrap();
        z.backward().unwrap();
        assert_eq!(z.value().as_slice(), &[3.0, 6.0]);
        assert_eq!(a.grad().unwrap().as_slice(), &[3.0, 3.0]);
    }

    #[test]
    fn sum_all_backward_broadcasts_scalar() {
        let tape = Tape::new();
        let a = tape.leaf(Tensor::from_vec(vec![1.0, 2.0, 3.0, 4.0], vec![2, 2]).unwrap(), true).unwrap();
        let s = a.sum_all().unwrap();
        s.backward().unwrap();
        assert!(approx(s.value().as_slice()[0], 10.0));
        // grad of sum is ones everywhere.
        assert_eq!(a.grad().unwrap().as_slice(), &[1.0, 1.0, 1.0, 1.0]);
    }

    #[test]
    fn matmul_backward_uses_transposes() {
        // x = [[1,2],[3,4]] (2x2), y = [[5,6],[7,8]] (2x2)
        // z = x@y = [[19,22],[43,50]]
        let tape = Tape::new();
        let x = tape.leaf(
            Tensor::from_vec(vec![1.0, 2.0, 3.0, 4.0], vec![2, 2]).unwrap(),
            true,
        )
        .unwrap();
        let y = tape.leaf(
            Tensor::from_vec(vec![5.0, 6.0, 7.0, 8.0], vec![2, 2]).unwrap(),
            true,
        )
        .unwrap();
        let z = x.matmul(&y).unwrap();
        z.backward().unwrap();
        // dz/dx = z.grad @ y^T ; z.grad = ones(2,2) ; y^T=[[5,7],[6,8]]
        // => dz/dx = [[1*5+1*6, 1*7+1*8],[...]] row sums of y^T columns
        //   row0: 5+6=11, 7+8=15 ; row1 same => [[11,15],[11,15]]
        assert_eq!(x.grad().unwrap().as_slice(), &[11.0, 15.0, 11.0, 15.0]);
        // dz/dy = x^T @ z.grad ; x^T=[[1,3],[2,4]] ; z.grad=ones
        // => [[1*1+3*1, 1*1+3*1],[2*1+4*1, 2*1+4*1]] = [[4,4],[6,6]]
        assert_eq!(y.grad().unwrap().as_slice(), &[4.0, 4.0, 6.0, 6.0]);
    }

    #[test]
    fn add_row_bias_backward_sums_batch() {
        // x [[1,2],[3,4]] (2x2), bias [10,20]
        let tape = Tape::new();
        let x = tape.leaf(
            Tensor::from_vec(vec![1.0, 2.0, 3.0, 4.0], vec![2, 2]).unwrap(),
            true,
        )
        .unwrap();
        let b = tape.leaf(Tensor::from_vec(vec![10.0, 20.0], vec![2]).unwrap(), true).unwrap();
        let z = x.add_row_bias(&b).unwrap();
        assert_eq!(z.value().as_slice(), &[11.0, 22.0, 13.0, 24.0]);
        z.backward().unwrap();
        assert_eq!(x.grad().unwrap().as_slice(), &[1.0, 1.0, 1.0, 1.0]);
        // bias.grad = sum over batch of ones => [2,2]
        assert_eq!(b.grad().unwrap().as_slice(), &[2.0, 2.0]);
    }

    #[test]
    fn relu_backward_masks_nonpositive() {
        let tape = Tape::new();
        let a = tape.leaf(Tensor::from_vec(vec![-1.0, 2.0, 0.0, 3.0], vec![2, 2]).unwrap(), true).unwrap();
        let z = a.relu().unwrap();
        assert_eq!(z.value().as_slice(), &[0.0, 2.0, 0.0, 3.0]);
        z.backward().unwrap();
        // grad passes where a > 0, zero elsewhere.
        assert_eq!(a.grad().unwrap().as_slice(), &[0.0, 1.0, 0.0, 1.0]);
    }

    #[test]
    fn sigmoid_backward_correct_derivative() {
        let tape = Tape::new();
        let a = tape.leaf(Tensor::from_vec(vec![0.0], vec![1]).unwrap(), true).unwrap();
        let z = a.sigmoid().unwrap();
        z.backward().unwrap();
        let s = z.value().as_slice()[0]; // sigmoid(0) = 0.5
        assert!(approx(s, 0.5));
        let g = a.grad().unwrap().as_slice()[0];
        assert!(approx(g, s * (1.0 - s))); // 0.25
    }

    #[test]
    fn softmax_outputs_sum_to_one_per_row() {
        let tape = Tape::new();
        let a = tape.leaf(
            Tensor::from_vec(vec![1.0, 2.0, 3.0, 1.0, 0.0, -1.0], vec![2, 3]).unwrap(),
            false,
        )
        .unwrap();
        let z = a.softmax().unwrap();
        for r in 0..2 {
            let zv = z.value();
            let row = &zv.as_slice()[r * 3..(r + 1) * 3];
            let sum: f32 = row.iter().sum();
            assert!(approx(sum, 1.0));
        }
    }
}

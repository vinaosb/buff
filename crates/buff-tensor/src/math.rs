//! Numeric operations on `Tensor<f32>`: elementwise, matmul, reduce.
//!
//! # Parallelism (CPU via rayon)
//!
//! Per T6 decision (`.sisyphus/decisions/wgsl-extensibility-v1x.md` §3),
//! the MVP is CPU-only via rayon. The elementwise ops parallelize
//! trivially (zip two flat buffers, map per element); matmul parallelizes
//! over the outer row index; reduce parallelizes via rayon's
//! built-in `reduce` primitive after axis-grouping.
//!
//! # GPU dispatch (DEFERRED)
//!
//! Elementwise GPU dispatch is estimated at ~50 LOC + a dispatch wrapper
//! around the existing WGSL pipeline (`buff-lang-codegen-wgsl` +
//! `buff-lang-runtime::gpu_pipeline`); matmul + reduce GPU paths are
//! ~1500 LOC / ~15 days. Both are explicitly deferred to v1.18+.

use crate::error::{TensorError, TensorResult};
use crate::tensor::Tensor;
use rayon::prelude::*;

// ---------------------------------------------------------------------------
// Elementwise ops
// ---------------------------------------------------------------------------

/// Apply a binary elementwise op to two same-shape tensors, producing a
/// new tensor. Pure helper — the public ops delegate here.
fn elementwise_binary<F>(lhs: &Tensor, rhs: &Tensor, op: F) -> TensorResult<Tensor>
where
    F: Fn(f32, f32) -> f32 + Sync + Send,
{
    if !lhs.shape().elementwise_compatible(rhs.shape()) {
        return Err(TensorError::ShapeMismatch {
            lhs: lhs.shape().as_slice().to_vec(),
            rhs: rhs.shape().as_slice().to_vec(),
        });
    }
    let lhs_data = lhs.as_slice();
    let rhs_data = rhs.as_slice();
    let out: Vec<f32> = lhs_data
        .par_iter()
        .zip(rhs_data.par_iter())
        .map(|(&a, &b)| op(a, b))
        .collect();
    let shape = lhs.shape().clone();
    Tensor::from_vec(out, shape.as_slice().to_vec())
}

/// Apply a unary elementwise op, producing a new tensor.
///
/// # Errors
///
/// Propagates shape-validation errors (never actually fails — the shape
/// is cloned from `src` and the data length is preserved by `par_iter`).
fn elementwise_unary<F>(src: &Tensor, op: F) -> TensorResult<Tensor>
where
    F: Fn(f32) -> f32 + Sync + Send,
{
    let data: Vec<f32> = src.as_slice().par_iter().map(|&v| op(v)).collect();
    let shape = src.shape().clone();
    Tensor::from_vec(data, shape.as_slice().to_vec())
}

impl Tensor {
    /// Elementwise addition: `lhs + rhs`. Shapes must match exactly
    /// (broadcasting deferred to v1.18+).
    ///
    /// # Errors
    ///
    /// [`TensorError::ShapeMismatch`] if shapes differ.
    pub fn add(&self, rhs: &Tensor) -> TensorResult<Tensor> {
        elementwise_binary(self, rhs, |a, b| a + b)
    }

    /// Elementwise subtraction: `lhs - rhs`. Shapes must match exactly.
    ///
    /// # Errors
    ///
    /// [`TensorError::ShapeMismatch`] if shapes differ.
    pub fn sub(&self, rhs: &Tensor) -> TensorResult<Tensor> {
        elementwise_binary(self, rhs, |a, b| a - b)
    }

    /// Elementwise multiplication: `lhs * rhs` (Hadamard product).
    /// For matrix multiplication, see [`Self::matmul`]. Shapes must
    /// match exactly (broadcasting deferred to v1.18+).
    ///
    /// # Errors
    ///
    /// [`TensorError::ShapeMismatch`] if shapes differ.
    pub fn mul(&self, rhs: &Tensor) -> TensorResult<Tensor> {
        elementwise_binary(self, rhs, |a, b| a * b)
    }

    /// Elementwise division: `lhs / rhs`. Shapes must match exactly.
    /// Division by zero produces `inf` / `nan` per IEEE-754 (NOT an error).
    ///
    /// # Errors
    ///
    /// [`TensorError::ShapeMismatch`] if shapes differ.
    pub fn div(&self, rhs: &Tensor) -> TensorResult<Tensor> {
        elementwise_binary(self, rhs, |a, b| a / b)
    }

    /// Elementwise negation: `-x`.
    ///
    /// # Errors
    ///
    /// Propagates shape-validation errors (never actually fails).
    pub fn neg(&self) -> TensorResult<Tensor> {
        elementwise_unary(self, |v| -v)
    }

    /// Elementwise scalar multiplication: `self * scalar`.
    ///
    /// # Errors
    ///
    /// Propagates shape-validation errors (never actually fails).
    pub fn scale(&self, scalar: f32) -> TensorResult<Tensor> {
        elementwise_unary(self, |v| v * scalar)
    }
}

// ---------------------------------------------------------------------------
// Matmul
// ---------------------------------------------------------------------------

impl Tensor {
    /// Matrix multiplication of two 2-D tensors.
    ///
    /// `self` is `(m, k)`, `rhs` is `(k, n)`, result is `(m, n)`.
    /// For N-D batched matmul (deferred to v1.18+) see future
    /// `matmul_batched`.
    ///
    /// # Algorithm
    ///
    /// Naive triple-loop with row-parallelism via rayon. BLAS-optimized
    /// performance is a v1.18+ concern (would require linking
    /// `ndarray`/`blas-src` extern — the MVP keeps the dependency
    /// surface minimal per the project "pure-Rust preference" rule).
    /// Acceptable for MVP workloads (rank-2, ≤4M-element matrices).
    ///
    /// # Errors
    ///
    /// - [`TensorError::RankMismatch`] if either side is not rank 2.
    /// - [`TensorError::ShapeMismatch`] if `self.cols != rhs.rows`.
    pub fn matmul(&self, rhs: &Tensor) -> TensorResult<Tensor> {
        let out_shape = self.shape().matmul_compatible(rhs.shape())?;
        let (lhs_data, lhs_shape) = self.parts();
        let (rhs_data, _) = rhs.parts();
        let m = lhs_shape.as_slice()[0];
        let k = lhs_shape.as_slice()[1];
        let n = out_shape.as_slice()[1];
        let out: Vec<f32> = (0..m)
            .into_par_iter()
            .map(|r| {
                let mut row = vec![0.0f32; n];
                for c in 0..n {
                    let mut acc = 0.0f32;
                    for i in 0..k {
                        acc += lhs_data[r * k + i] * rhs_data[i * n + c];
                    }
                    row[c] = acc;
                }
                row
            })
            .flatten()
            .collect();
        Tensor::from_vec(out, out_shape.as_slice().to_vec())
    }
}

// ---------------------------------------------------------------------------
// Reductions
// ---------------------------------------------------------------------------

impl Tensor {
    /// Sum reduction along `axis`. Negative `axis` counts from the end
    /// (Python-style).
    ///
    /// # Errors
    ///
    /// - [`TensorError::AxisOutOfBounds`] if `axis` is out of bounds.
    pub fn sum_axis(&self, axis: isize) -> TensorResult<Tensor> {
        let out_shape = self.shape().reduce_axis(axis)?;
        let out_len = out_shape.num_elements();
        let mut out = vec![0.0f32; out_len];
        let in_data = self.as_slice();
        let in_shape = self.shape();
        let in_dims = in_shape.as_slice();
        let rank = in_dims.len() as isize;
        let abs_axis = (if axis < 0 { axis + rank } else { axis }) as usize;
        let out_strides = out_shape.strides();
        for (in_flat, &v) in in_data.iter().enumerate() {
            let mut remaining = in_flat;
            let mut out_flat = 0usize;
            // Decompose the flat index LAST-axis-first: row-major layout means
            // the LAST axis varies fastest, so `remaining % dim` must peel the
            // last axis first. Iterating forward (axis 0 first) would treat
            // axis 0 as fastest and scramble the reduction (the T7 root cause
            // of the buff-ml bias-gradient bug). The `out_ax` mapping below
            // depends only on `ax` vs `abs_axis`, so reversing iteration order
            // fixes the decomposition without changing the output mapping.
            for (ax, &dim) in in_dims.iter().enumerate().rev() {
                let idx = remaining % dim;
                remaining /= dim;
                if ax == abs_axis {
                    continue;
                }
                let out_ax = if ax < abs_axis { ax } else { ax - 1 };
                out_flat = out_flat.saturating_add(idx.saturating_mul(out_strides[out_ax]));
            }
            out[out_flat] += v;
        }
        Tensor::from_vec(out, out_shape.as_slice().to_vec())
    }

    /// Mean reduction along `axis`. Equivalent to `sum_axis / n` where
    /// `n` is the size of the reduced axis.
    ///
    /// # Errors
    ///
    /// - [`TensorError::AxisOutOfBounds`] if `axis` is out of bounds.
    /// - [`TensorError::Empty`] if the reduced axis has size 0.
    pub fn mean_axis(&self, axis: isize) -> TensorResult<Tensor> {
        let summed = self.sum_axis(axis)?;
        let n = self.shape().as_slice()[(if axis < 0 {
            axis + self.shape().rank() as isize
        } else {
            axis
        }) as usize];
        if n == 0 {
            return Err(TensorError::Empty);
        }
        summed.scale(1.0f32 / n as f32)
    }

    /// Max reduction along `axis`. Returns the maximum element per
    /// output cell. For minimum, swap the sign and re-call.
    ///
    /// # Errors
    ///
    /// - [`TensorError::AxisOutOfBounds`] if `axis` is out of bounds.
    /// - [`TensorError::Empty`] if the tensor has zero elements.
    pub fn max_axis(&self, axis: isize) -> TensorResult<Tensor> {
        if self.is_empty() {
            return Err(TensorError::Empty);
        }
        let out_shape = self.shape().reduce_axis(axis)?;
        let out_len = out_shape.num_elements();
        let mut out: Vec<f32> = vec![f32::NEG_INFINITY; out_len];
        let in_data = self.as_slice();
        let in_shape = self.shape();
        let in_dims = in_shape.as_slice();
        let rank = in_dims.len() as isize;
        let abs_axis = (if axis < 0 { axis + rank } else { axis }) as usize;
        let out_strides = out_shape.strides();
        for (in_flat, &v) in in_data.iter().enumerate() {
            let mut remaining = in_flat;
            let mut out_flat = 0usize;
            // Same last-axis-first decomposition as `sum_axis` (row-major).
            for (ax, &dim) in in_dims.iter().enumerate().rev() {
                let idx = remaining % dim;
                remaining /= dim;
                if ax == abs_axis {
                    continue;
                }
                let out_ax = if ax < abs_axis { ax } else { ax - 1 };
                out_flat = out_flat.saturating_add(idx.saturating_mul(out_strides[out_ax]));
            }
            if v > out[out_flat] {
                out[out_flat] = v;
            }
        }
        Tensor::from_vec(out, out_shape.as_slice().to_vec())
    }

    /// Sum of all elements (scalar reduction).
    pub fn sum_all(&self) -> f32 {
        self.as_slice().par_iter().sum()
    }

    /// Mean of all elements. Returns NaN for an empty tensor.
    pub fn mean_all(&self) -> f32 {
        let n = self.len();
        if n == 0 {
            return f32::NAN;
        }
        self.sum_all() / n as f32
    }

    /// Maximum element. Returns `None` for an empty tensor.
    pub fn max_all(&self) -> Option<f32> {
        self.as_slice()
            .par_iter()
            .copied()
            .max_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn approx_eq(a: f32, b: f32) -> bool {
        (a - b).abs() < 1e-5f32
    }

    fn tensor_approx_eq(a: &Tensor, b: &Tensor) -> bool {
        if a.shape() != b.shape() {
            return false;
        }
        a.as_slice()
            .iter()
            .zip(b.as_slice().iter())
            .all(|(&x, &y)| approx_eq(x, y))
    }

    #[test]
    fn elementwise_add_basic() {
        let a = Tensor::from_vec(vec![1.0, 2.0, 3.0, 4.0], vec![2, 2]).unwrap();
        let b = Tensor::from_vec(vec![10.0, 20.0, 30.0, 40.0], vec![2, 2]).unwrap();
        let c = a.add(&b).unwrap();
        assert_eq!(c.as_slice(), &[11.0, 22.0, 33.0, 44.0]);
    }

    #[test]
    fn elementwise_sub_basic() {
        let a = Tensor::from_vec(vec![10.0, 20.0, 30.0], vec![3]).unwrap();
        let b = Tensor::from_vec(vec![1.0, 2.0, 3.0], vec![3]).unwrap();
        let c = a.sub(&b).unwrap();
        assert_eq!(c.as_slice(), &[9.0, 18.0, 27.0]);
    }

    #[test]
    fn elementwise_mul_basic() {
        let a = Tensor::from_vec(vec![2.0, 3.0, 4.0], vec![3]).unwrap();
        let b = Tensor::from_vec(vec![5.0, 6.0, 7.0], vec![3]).unwrap();
        let c = a.mul(&b).unwrap();
        assert_eq!(c.as_slice(), &[10.0, 18.0, 28.0]);
    }

    #[test]
    fn elementwise_div_basic() {
        let a = Tensor::from_vec(vec![10.0, 20.0, 30.0], vec![3]).unwrap();
        let b = Tensor::from_vec(vec![2.0, 5.0, 10.0], vec![3]).unwrap();
        let c = a.div(&b).unwrap();
        assert_eq!(c.as_slice(), &[5.0, 4.0, 3.0]);
    }

    #[test]
    fn elementwise_shape_mismatch() {
        let a = Tensor::zeros(vec![2, 3]).unwrap();
        let b = Tensor::zeros(vec![3, 2]).unwrap();
        let err = a.add(&b).unwrap_err();
        assert_eq!(
            err,
            TensorError::ShapeMismatch {
                lhs: vec![2, 3],
                rhs: vec![3, 2],
            }
        );
    }

    #[test]
    fn elementwise_neg_and_scale() {
        let a = Tensor::from_vec(vec![1.0, -2.0, 3.0], vec![3]).unwrap();
        let n = a.neg().unwrap();
        assert_eq!(n.as_slice(), &[-1.0, 2.0, -3.0]);
        let s = a.scale(2.0).unwrap();
        assert_eq!(s.as_slice(), &[2.0, -4.0, 6.0]);
    }

    #[test]
    fn matmul_basic_2x2() {
        // a = [[1, 2], [3, 4]], b = [[5, 6], [7, 8]]
        // a*b = [[1*5+2*7, 1*6+2*8], [3*5+4*7, 3*6+4*8]]
        //     = [[19, 22], [43, 50]]
        let a = Tensor::from_vec(vec![1.0, 2.0, 3.0, 4.0], vec![2, 2]).unwrap();
        let b = Tensor::from_vec(vec![5.0, 6.0, 7.0, 8.0], vec![2, 2]).unwrap();
        let c = a.matmul(&b).unwrap();
        assert_eq!(c.shape().as_slice(), &[2, 2]);
        assert_eq!(c.as_slice(), &[19.0, 22.0, 43.0, 50.0]);
    }

    #[test]
    fn matmul_non_square() {
        // a (2,3) * b (3,2) = c (2,2)
        let a = Tensor::from_vec(vec![1.0, 2.0, 3.0, 4.0, 5.0, 6.0], vec![2, 3]).unwrap();
        let b = Tensor::from_vec(vec![7.0, 8.0, 9.0, 10.0, 11.0, 12.0], vec![3, 2]).unwrap();
        let c = a.matmul(&b).unwrap();
        assert_eq!(c.shape().as_slice(), &[2, 2]);
        // c[0,0] = 1*7 + 2*9 + 3*11 = 7 + 18 + 33 = 58
        // c[0,1] = 1*8 + 2*10 + 3*12 = 8 + 20 + 36 = 64
        // c[1,0] = 4*7 + 5*9 + 6*11 = 28 + 45 + 66 = 139
        // c[1,1] = 4*8 + 5*10 + 6*12 = 32 + 50 + 72 = 154
        assert_eq!(c.as_slice(), &[58.0, 64.0, 139.0, 154.0]);
    }

    #[test]
    fn matmul_shape_mismatch_inner_dim() {
        let a = Tensor::zeros(vec![2, 3]).unwrap();
        let b = Tensor::zeros(vec![4, 5]).unwrap();
        let err = a.matmul(&b).unwrap_err();
        assert!(matches!(err, TensorError::ShapeMismatch { .. }));
    }

    #[test]
    fn matmul_rejects_non_2d() {
        let a = Tensor::zeros(vec![2, 2, 2]).unwrap();
        let b = Tensor::zeros(vec![2, 2]).unwrap();
        let err = a.matmul(&b).unwrap_err();
        assert!(matches!(err, TensorError::RankMismatch { .. }));
    }

    #[test]
    fn matmul_identity() {
        // a * I = a
        let a = Tensor::from_vec(vec![1.0, 2.0, 3.0, 4.0, 5.0, 6.0], vec![2, 3]).unwrap();
        let i = Tensor::from_vec(
            vec![1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0],
            vec![3, 3],
        )
        .unwrap();
        let c = a.matmul(&i).unwrap();
        assert_eq!(c.shape().as_slice(), &[2, 3]);
        assert!(tensor_approx_eq(&c, &a));
    }

    #[test]
    fn reduce_sum_axis_0() {
        // [[1,2,3],[4,5,6]] -> sum axis 0 -> [5,7,9]
        let t = Tensor::from_vec(vec![1.0, 2.0, 3.0, 4.0, 5.0, 6.0], vec![2, 3]).unwrap();
        let r = t.sum_axis(0).unwrap();
        assert_eq!(r.shape().as_slice(), &[3]);
        assert_eq!(r.as_slice(), &[5.0, 7.0, 9.0]);
    }

    #[test]
    fn reduce_sum_axis_1() {
        // [[1,2,3],[4,5,6]] -> sum axis 1 -> [6, 15]
        let t = Tensor::from_vec(vec![1.0, 2.0, 3.0, 4.0, 5.0, 6.0], vec![2, 3]).unwrap();
        let r = t.sum_axis(1).unwrap();
        assert_eq!(r.shape().as_slice(), &[2]);
        assert_eq!(r.as_slice(), &[6.0, 15.0]);
    }

    #[test]
    fn reduce_sum_negative_axis() {
        let t = Tensor::from_vec(vec![1.0, 2.0, 3.0, 4.0, 5.0, 6.0], vec![2, 3]).unwrap();
        let r = t.sum_axis(-1).unwrap();
        assert_eq!(r.shape().as_slice(), &[2]);
        assert_eq!(r.as_slice(), &[6.0, 15.0]);
    }

    #[test]
    fn reduce_mean_axis() {
        // [[1,2,3],[4,5,6]] -> mean axis 1 -> [2, 5]
        let t = Tensor::from_vec(vec![1.0, 2.0, 3.0, 4.0, 5.0, 6.0], vec![2, 3]).unwrap();
        let r = t.mean_axis(1).unwrap();
        assert_eq!(r.as_slice(), &[2.0, 5.0]);
    }

    #[test]
    fn reduce_max_axis() {
        // [[1,2,3],[4,5,6]] -> max axis 1 -> [3, 6]
        let t = Tensor::from_vec(vec![1.0, 2.0, 3.0, 4.0, 5.0, 6.0], vec![2, 3]).unwrap();
        let r = t.max_axis(1).unwrap();
        assert_eq!(r.as_slice(), &[3.0, 6.0]);
    }

    #[test]
    fn reduce_sum_3d_axis() {
        // shape [2,2,2]: 8 elements. Sum along axis 1 -> shape [2,2].
        let t =
            Tensor::from_vec(vec![1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0], vec![2, 2, 2]).unwrap();
        let r = t.sum_axis(1).unwrap();
        assert_eq!(r.shape().as_slice(), &[2, 2]);
        // batch 0: [1,2] + [3,4] = [4,6]
        // batch 1: [5,6] + [7,8] = [12,14]
        assert_eq!(r.as_slice(), &[4.0, 6.0, 12.0, 14.0]);
    }

    #[test]
    fn reduce_axis_out_of_bounds() {
        let t = Tensor::zeros(vec![2, 3]).unwrap();
        let err = t.sum_axis(5).unwrap_err();
        assert_eq!(err, TensorError::AxisOutOfBounds { axis: 5, rank: 2 });
    }

    #[test]
    fn reduce_sum_all_basic() {
        let t = Tensor::from_vec(vec![1.0, 2.0, 3.0, 4.0], vec![2, 2]).unwrap();
        assert!(approx_eq(t.sum_all(), 10.0));
        assert!(approx_eq(t.mean_all(), 2.5));
        assert_eq!(t.max_all(), Some(4.0));
    }

    #[test]
    fn reduce_sum_all_empty() {
        let t = Tensor::zeros(vec![0]).unwrap();
        assert!(approx_eq(t.sum_all(), 0.0));
        assert!(t.max_all().is_none() || t.max_all() == Some(f32::NEG_INFINITY));
    }

    #[test]
    fn reduce_max_all_of_empty() {
        let t = Tensor::zeros(vec![0]).unwrap();
        assert_eq!(t.max_all(), None);
    }
}

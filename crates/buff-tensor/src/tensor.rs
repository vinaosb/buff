//! The `Tensor<T>` type — owned N-dimensional array (rank ≤ 4 for MVP).
//!
//! Storage is a flat `Vec<T>` in row-major (C-order) layout, paired with
//! a [`Shape`] describing the per-axis sizes. Indexing goes through
//! [`Shape::flat_offset`] which computes the row-major offset via
//! checked arithmetic; the `Vec` access is then a single bounds-checked
//! indexing.
//!
//! # MVP scope (per T8 spec)
//!
//! - **dtype**: `f32` ONLY. `f64` / `i64` deferred to v1.18+. The
//!   `Tensor<T>` type is generic over `T` so a future widening is a
//!   one-line change at the call sites, but the MVP exposes only
//!   `Tensor<f32>` via the [`Tensor`] alias.
//! - **rank cap**: 4 ([`crate::shape::MVP_RANK_CAP`]). Higher ranks
//!   deferred to v1.18+.
//! - **GPU dispatch**: NONE for MVP. Per T6 decision
//!   (`.sisyphus/decisions/wgsl-extensibility-v1x.md` §3), the MVP
//!   is CPU-only via rayon. Elementwise GPU dispatch is feasible as a
//!   v1.18+ enhancement (~50 LOC); matmul + reduce GPU paths are
//!   estimated at ~1500 LOC / ~15 days and explicitly deferred.
//! - **broadcasting**: NONE for MVP (shapes must match exactly in
//!   elementwise ops). Deferred to v1.18+.
//! - **autodiff**: NONE. That is T15 (buff-ml).

use crate::error::{TensorError, TensorResult};
use crate::shape::{check_rank_cap, Shape};
use std::marker::PhantomData;
use std::ops::{Deref, DerefMut};

/// The canonical MVP tensor alias — 32-bit float element type.
///
/// Per T8 spec: "dtype f32 ONLY (defer f64/i64 to v1.18+)". The
/// underlying [`TensorCore<T>`] is generic, but only `f32` is exposed
/// at the surface. Future widening is a one-line change.
pub type Tensor = TensorCore<f32>;

/// Owned N-dimensional array.
///
/// Stores elements as a flat `Vec<T>` in row-major (C-order) layout,
/// paired with a [`Shape`]. The type is parametric over `T` for
/// future extension to `f64` / `i64`; the MVP exposes only
/// [`Tensor`] (=`TensorCore<f32>`).
#[derive(Debug, Clone, PartialEq)]
pub struct TensorCore<T> {
    /// Row-major flat storage.
    data: Vec<T>,
    /// Per-axis dimensions.
    shape: Shape,
    /// Marker so the structural derive above does not flag T as
    /// "unused generic" when T is `()` or similar in tests.
    _marker: PhantomData<T>,
}

impl<T> TensorCore<T> {
    /// Construct from a flat data buffer + shape. The buffer length
    /// must equal `shape.num_elements()`.
    ///
    /// # Errors
    ///
    /// - [`TensorError::DataLengthMismatch`] if `data.len()` != `shape.num_elements()`.
    /// - Propagates shape-validation errors from [`Shape::new`].
    pub fn from_vec(data: Vec<T>, shape: impl Into<Vec<usize>>) -> TensorResult<Self> {
        let shape = Shape::new(shape)?;
        let expected = shape.num_elements();
        if data.len() != expected {
            return Err(TensorError::DataLengthMismatch {
                data_len: data.len(),
                shape_elements: expected,
            });
        }
        Ok(Self {
            data,
            shape,
            _marker: PhantomData,
        })
    }

    /// The shape (per-axis dimensions).
    pub fn shape(&self) -> &Shape {
        &self.shape
    }

    /// Rank (number of dimensions).
    pub fn rank(&self) -> usize {
        self.shape.rank()
    }

    /// Total element count (product of shape dimensions).
    pub fn len(&self) -> usize {
        self.data.len()
    }

    /// Whether the tensor contains zero elements (some dim is 0).
    pub fn is_empty(&self) -> bool {
        self.data.is_empty()
    }

    /// View of the flat row-major data buffer.
    pub fn as_slice(&self) -> &[T] {
        &self.data
    }

    /// Mutable view of the flat row-major data buffer.
    pub fn as_mut_slice(&mut self) -> &mut [T] {
        &mut self.data
    }

    /// Convert into the underlying flat `Vec<T>`.
    pub fn into_vec(self) -> Vec<T> {
        self.data
    }

    /// Get a reference to the element at `index`. Returns `None` if the
    /// index is out of bounds or the rank is wrong.
    pub fn get(&self, index: &[usize]) -> Option<&T> {
        let flat = self.shape.flat_offset(index).ok()?;
        self.data.get(flat)
    }

    /// Set the element at `index` to `value`.
    ///
    /// # Errors
    ///
    /// - [`TensorError::IndexOutOfBounds`] if `index` is out of bounds.
    /// - [`TensorError::RankMismatch`] if `index.len()` != `shape.rank()`.
    pub fn set(&mut self, index: &[usize], value: T) -> TensorResult<()> {
        let flat = self.shape.flat_offset(index)?;
        // flat_offset already validated bounds, so direct indexing is safe.
        self.data[flat] = value;
        Ok(())
    }

    /// Reshape to a new shape with the same element count.
    ///
    /// The data buffer is reused as-is (row-major layout is preserved;
    /// since this is a flat Vec, no copy is needed).
    ///
    /// # Errors
    ///
    /// - [`TensorError::ReshapeMismatch`] if `new_shape.num_elements() != self.len()`.
    /// - Propagates shape-validation errors from [`Shape::new`].
    pub fn reshape(mut self, new_shape: impl Into<Vec<usize>>) -> TensorResult<Self> {
        let new_shape = Shape::new(new_shape)?;
        if new_shape.num_elements() != self.data.len() {
            return Err(TensorError::ReshapeMismatch {
                got: self.data.len(),
                target: new_shape.num_elements(),
            });
        }
        self.shape = new_shape;
        Ok(self)
    }

    /// Validate an internal rank cap (used by ops that produce shapes inline).
    #[allow(dead_code)]
    pub(crate) fn ensure_rank_cap(rank: usize) -> TensorResult<()> {
        check_rank_cap(rank)
    }

    /// Borrow the data buffer + shape. Internal helper used by ops that
    /// need both views (matmul reads lhs.data + lhs.shape + rhs.data).
    pub(crate) fn parts(&self) -> (&Vec<T>, &Shape) {
        (&self.data, &self.shape)
    }
}

impl<T: Clone> TensorCore<T> {
    /// Construct a tensor filled with `fill` of the given shape.
    ///
    /// # Errors
    ///
    /// Propagates shape-validation errors from [`Shape::new`].
    pub fn full(shape: impl Into<Vec<usize>>, fill: T) -> TensorResult<Self> {
        let shape = Shape::new(shape)?;
        let n = shape.num_elements();
        Ok(Self {
            data: vec![fill; n],
            shape,
            _marker: PhantomData,
        })
    }
}

impl Tensor {
    /// Construct a tensor of `shape` filled with `0.0`.
    ///
    /// # Errors
    ///
    /// Propagates shape-validation errors from [`Shape::new`].
    pub fn zeros(shape: impl Into<Vec<usize>>) -> TensorResult<Self> {
        Self::full(shape, 0.0f32)
    }

    /// Construct a tensor of `shape` filled with `1.0`.
    ///
    /// # Errors
    ///
    /// Propagates shape-validation errors from [`Shape::new`].
    pub fn ones(shape: impl Into<Vec<usize>>) -> TensorResult<Self> {
        Self::full(shape, 1.0f32)
    }

    /// Construct a tensor filled with `value`. Mirrors `full` but with
    /// Buff-naming symmetry alongside `zeros` / `ones`.
    ///
    /// # Errors
    ///
    /// Propagates shape-validation errors from [`Shape::new`].
    pub fn filled(shape: impl Into<Vec<usize>>, value: f32) -> TensorResult<Self> {
        Self::full(shape, value)
    }

    /// Transpose a 2-D tensor (matrix): swap the two axes. Returns a NEW
    /// tensor with rows and columns swapped.
    ///
    /// For N-D transpose by permutation, see [`Self::transpose_perm`]
    /// (used by future T13 / T15 callers; exposed as a separate fn so
    /// the common 2-D case stays a one-call site).
    ///
    /// # Errors
    ///
    /// - [`TensorError::RankMismatch`] if `self.rank() != 2`.
    pub fn transpose(&self) -> TensorResult<Self> {
        if self.shape.rank() != 2 {
            return Err(TensorError::RankMismatch {
                actual: self.shape.rank(),
                expected: 2,
            });
        }
        let (data, shape) = self.parts();
        let dims = shape.as_slice();
        let rows = dims[0];
        let cols = dims[1];
        let mut out = vec![0.0f32; rows * cols];
        for r in 0..rows {
            for c in 0..cols {
                // out[c, r] = self[r, c]; out is (cols, rows) shape, row-major.
                out[c * rows + r] = data[r * cols + c];
            }
        }
        Ok(Self {
            data: out,
            shape: Shape::new_unchecked(vec![cols, rows]),
            _marker: PhantomData,
        })
    }

    /// Transpose an N-D tensor by an explicit axis permutation.
    ///
    /// `perm` must be a permutation of `0..self.rank()`. The output
    /// tensor's axis `i` corresponds to the input's axis `perm[i]`.
    ///
    /// # Errors
    ///
    /// - [`TensorError::RankMismatch`] if `perm.len()` != `self.rank()`.
    /// - [`TensorError::IndexOutOfBounds`] (re-used) if `perm` is not a
    ///   valid permutation of `0..rank`.
    pub fn transpose_perm(&self, perm: &[isize]) -> TensorResult<Self> {
        let rank = self.shape.rank();
        if perm.len() != rank {
            return Err(TensorError::RankMismatch {
                actual: perm.len(),
                expected: rank,
            });
        }
        // Resolve negative axes.
        let rank_isize = rank as isize;
        let mut abs_perm = Vec::with_capacity(rank);
        for &p in perm {
            let a = if p < 0 { p + rank_isize } else { p };
            if a < 0 || a >= rank_isize {
                return Err(TensorError::IndexOutOfBounds {
                    index: perm.iter().map(|&p| p as usize).collect(),
                    shape: self.shape.as_slice().to_vec(),
                });
            }
            abs_perm.push(a as usize);
        }
        // Check it's a real permutation.
        let mut seen = vec![false; rank];
        for &a in &abs_perm {
            if seen[a] {
                return Err(TensorError::IndexOutOfBounds {
                    index: perm.iter().map(|&p| p as usize).collect(),
                    shape: self.shape.as_slice().to_vec(),
                });
            }
            seen[a] = true;
        }
        // Compute output shape.
        let in_dims = self.shape.as_slice();
        let out_dims: Vec<usize> = abs_perm.iter().map(|&p| in_dims[p]).collect();
        let out_shape = Shape::new_unchecked(out_dims);
        // Compute output strides (in terms of INPUT layout).
        let in_strides = self.shape.strides();
        let mut out_strides_from_in = vec![0usize; rank];
        for (out_axis, &p) in abs_perm.iter().enumerate() {
            out_strides_from_in[out_axis] = in_strides[p];
        }
        // Walk output indices in row-major order, gather input element.
        let n = self.data.len();
        let mut out_data = vec![0.0f32; n];
        let out_dims_slice = out_shape.as_slice();
        for (out_flat, slot) in out_data.iter_mut().enumerate().take(n) {
            let mut remaining = out_flat;
            let mut in_flat = 0usize;
            for axis in 0..rank {
                let dim = out_dims_slice[axis];
                let idx = remaining % dim;
                remaining /= dim;
                in_flat = in_flat.saturating_add(idx.saturating_mul(out_strides_from_in[axis]));
            }
            *slot = self.data[in_flat];
        }
        Ok(Self {
            data: out_data,
            shape: out_shape,
            _marker: PhantomData,
        })
    }
}

impl<T> Deref for TensorCore<T> {
    type Target = [T];
    fn deref(&self) -> &[T] {
        &self.data
    }
}

impl<T> DerefMut for TensorCore<T> {
    fn deref_mut(&mut self) -> &mut [T] {
        &mut self.data
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tensor_zeros_basic() {
        let t = Tensor::zeros(vec![2, 3]).unwrap();
        assert_eq!(t.shape().as_slice(), &[2, 3]);
        assert_eq!(t.rank(), 2);
        assert_eq!(t.len(), 6);
        assert!(t.as_slice().iter().all(|&v| v == 0.0));
    }

    #[test]
    fn tensor_ones_basic() {
        let t = Tensor::ones(vec![3]).unwrap();
        assert_eq!(t.shape().as_slice(), &[3]);
        assert!(t.as_slice().iter().all(|&v| v == 1.0));
    }

    #[test]
    fn tensor_filled_basic() {
        let t = Tensor::filled(vec![2, 2], 7.5).unwrap();
        assert!(t.as_slice().iter().all(|&v| v == 7.5));
    }

    #[test]
    fn tensor_from_vec_basic() {
        let t = Tensor::from_vec(vec![1.0, 2.0, 3.0, 4.0, 5.0, 6.0], vec![2, 3]).unwrap();
        assert_eq!(t.shape().as_slice(), &[2, 3]);
        assert_eq!(t.as_slice(), &[1.0, 2.0, 3.0, 4.0, 5.0, 6.0]);
    }

    #[test]
    fn tensor_from_vec_data_length_mismatch() {
        let err = Tensor::from_vec(vec![1.0, 2.0, 3.0], vec![2, 3]).unwrap_err();
        assert_eq!(
            err,
            TensorError::DataLengthMismatch {
                data_len: 3,
                shape_elements: 6,
            }
        );
    }

    #[test]
    fn tensor_get_set_basic() {
        let mut t = Tensor::from_vec(vec![1.0, 2.0, 3.0, 4.0], vec![2, 2]).unwrap();
        assert_eq!(t.get(&[0, 0]), Some(&1.0));
        assert_eq!(t.get(&[1, 1]), Some(&4.0));
        assert_eq!(t.get(&[5, 5]), None);
        t.set(&[0, 0], 99.0).unwrap();
        assert_eq!(t.get(&[0, 0]), Some(&99.0));
    }

    #[test]
    fn tensor_set_index_out_of_bounds() {
        let mut t = Tensor::zeros(vec![2, 2]).unwrap();
        let err = t.set(&[5, 0], 1.0).unwrap_err();
        assert_eq!(
            err,
            TensorError::IndexOutOfBounds {
                index: vec![5, 0],
                shape: vec![2, 2],
            }
        );
    }

    #[test]
    fn tensor_reshape_basic() {
        let t = Tensor::from_vec(vec![1.0, 2.0, 3.0, 4.0, 5.0, 6.0], vec![2, 3]).unwrap();
        let r = t.reshape(vec![3, 2]).unwrap();
        assert_eq!(r.shape().as_slice(), &[3, 2]);
        assert_eq!(r.as_slice(), &[1.0, 2.0, 3.0, 4.0, 5.0, 6.0]);
    }

    #[test]
    fn tensor_reshape_mismatch() {
        let t = Tensor::from_vec(vec![1.0, 2.0, 3.0, 4.0, 5.0, 6.0], vec![2, 3]).unwrap();
        let err = t.reshape(vec![2, 2]).unwrap_err();
        assert_eq!(err, TensorError::ReshapeMismatch { got: 6, target: 4 });
    }

    #[test]
    fn tensor_reshape_1d_to_3d() {
        let t = Tensor::from_vec(vec![1.0, 2.0, 3.0, 4.0], vec![4]).unwrap();
        let r = t.reshape(vec![2, 1, 2]).unwrap();
        assert_eq!(r.shape().as_slice(), &[2, 1, 2]);
    }

    #[test]
    fn tensor_transpose_2d_basic() {
        let t = Tensor::from_vec(vec![1.0, 2.0, 3.0, 4.0, 5.0, 6.0], vec![2, 3]).unwrap();
        let tt = t.transpose().unwrap();
        assert_eq!(tt.shape().as_slice(), &[3, 2]);
        // Original: [[1,2,3],[4,5,6]]
        // Transposed: [[1,4],[2,5],[3,6]]
        assert_eq!(tt.as_slice(), &[1.0, 4.0, 2.0, 5.0, 3.0, 6.0]);
    }

    #[test]
    fn tensor_transpose_rejects_non_2d() {
        let t = Tensor::zeros(vec![2, 2, 2]).unwrap();
        let err = t.transpose().unwrap_err();
        assert_eq!(
            err,
            TensorError::RankMismatch {
                actual: 3,
                expected: 2,
            }
        );
    }

    #[test]
    fn tensor_transpose_perm_basic() {
        // Shape [2,3,4], permute axes (1,0,2) -> [3,2,4].
        let t = Tensor::from_vec((0..24).map(|i| i as f32).collect(), vec![2, 3, 4]).unwrap();
        let tt = t.transpose_perm(&[1, 0, 2]).unwrap();
        assert_eq!(tt.shape().as_slice(), &[3, 2, 4]);
        assert_eq!(tt.len(), 24);
    }

    #[test]
    fn tensor_transpose_perm_rejects_wrong_len() {
        let t = Tensor::zeros(vec![2, 3]).unwrap();
        let err = t.transpose_perm(&[0, 1, 2]).unwrap_err();
        assert_eq!(
            err,
            TensorError::RankMismatch {
                actual: 3,
                expected: 2,
            }
        );
    }

    #[test]
    fn tensor_transpose_perm_rejects_invalid_perm() {
        let t = Tensor::zeros(vec![2, 3]).unwrap();
        // 0 appears twice — not a permutation.
        let err = t.transpose_perm(&[0, 0]).unwrap_err();
        assert!(matches!(err, TensorError::IndexOutOfBounds { .. }));
    }

    #[test]
    fn tensor_deref_works() {
        let t = Tensor::from_vec(vec![1.0, 2.0, 3.0, 4.0], vec![2, 2]).unwrap();
        // Deref to [f32] lets us use slice methods.
        assert_eq!(t.first(), Some(&1.0));
        assert_eq!(t.len(), 4); // from Vec
        assert_eq!(t.iter().sum::<f32>(), 10.0);
    }

    #[test]
    fn tensor_into_vec() {
        let t = Tensor::from_vec(vec![1.0, 2.0, 3.0], vec![3]).unwrap();
        let v = t.into_vec();
        assert_eq!(v, vec![1.0, 2.0, 3.0]);
    }
}

//! Shape + stride + indexing helpers for `Tensor<T>`.
//!
//! Pure-data structures — no `Tensor<T>` reference, no allocations beyond
//! the shape `Vec` itself. Keeping these in a sibling module makes the
//! `Tensor<T>` impl block in [`crate::tensor`] read as a list of high-level
//! operations, with the byte-arithmetic factored out.
//!
//! # Conventions
//!
//! - **Row-major (C-order) layout**: the last axis is contiguous in memory.
//!   Matches `ndarray::Array::from_shape_vec`'s default and is the
//!   convention every numeric Rust crate uses.
//! - **MVP rank cap**: 4 (per T8 spec). Enforced at [`Shape::new`] and
//!   again at [`Shape::check_rank_cap`].
//! - **No `unsafe`**: indexing computes a flat offset via checked
//!   arithmetic; the `Tensor<T>` then does a single bounds-checked
//!   `Vec` access.

use crate::error::{TensorError, TensorResult};

/// Maximum rank (number of dimensions) supported by the MVP.
///
/// Per T8 spec: "Do NOT support rank > 4 (defer to v1.18+)". Surfaced as
/// a public constant so callers can validate before constructing a shape.
pub const MVP_RANK_CAP: usize = 4;

/// A tensor shape: the size along each axis.
///
/// Owned `Vec<usize>` for simplicity — every MVP operation produces a new
/// shape, and the LOC cost of a small-vec optimization is not justified
/// at this scale. Shapes are always non-empty (rank 0 is rejected; the
/// smallest valid shape is rank 1 `[1]`).
///
/// # Layout convention
///
/// Row-major (C-order): the last axis varies fastest. Strides are
/// derived via [`Shape::strides`].
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Shape {
    dims: Vec<usize>,
}

impl Shape {
    /// Construct a shape from a dimension slice.
    ///
    /// # Errors
    ///
    /// - [`TensorError::RankTooLarge`] if `dims.len() > MVP_RANK_CAP`.
    /// - [`TensorError::Empty`] if `dims` is empty (rank-0 tensors are
    ///   not supported in the MVP).
    pub fn new(dims: impl Into<Vec<usize>>) -> TensorResult<Self> {
        let dims = dims.into();
        if dims.is_empty() {
            return Err(TensorError::Empty);
        }
        check_rank_cap(dims.len())?;
        Ok(Self { dims })
    }

    /// Construct a shape WITHOUT validation. Caller asserts invariants
    /// (non-empty, rank <= MVP_RANK_CAP). Internal-use only — used by
    /// operations that produce a shape from already-validated components.
    pub(crate) fn new_unchecked(dims: Vec<usize>) -> Self {
        debug_assert!(
            !dims.is_empty(),
            "Shape::new_unchecked called with empty dims"
        );
        debug_assert!(
            dims.len() <= MVP_RANK_CAP,
            "Shape::new_unchecked called with rank > MVP_RANK_CAP"
        );
        Self { dims }
    }

    /// The dimensions as a slice (the canonical view of the shape).
    pub fn as_slice(&self) -> &[usize] {
        &self.dims
    }

    /// Number of dimensions (rank).
    pub fn rank(&self) -> usize {
        self.dims.len()
    }

    /// Total element count implied by the shape (product of dimensions).
    /// Returns 1 for a shape containing a zero dimension (consistent with
    /// `ndarray::Array::len`).
    pub fn num_elements(&self) -> usize {
        self.dims.iter().product()
    }

    /// Strides for row-major layout: stride[axis] is the number of
    /// elements to skip to advance one position along `axis`. The last
    /// axis has stride 1; each preceding axis's stride is the product
    /// of all following dimensions.
    ///
    /// # Example
    ///
    /// A `[2, 3, 4]` shape yields strides `[12, 4, 1]`.
    pub fn strides(&self) -> Vec<usize> {
        let mut strides = vec![0usize; self.dims.len()];
        if self.dims.is_empty() {
            return strides;
        }
        // Last stride is always 1.
        let mut acc = 1usize;
        // Walk right-to-left, accumulating.
        for i in (0..self.dims.len()).rev() {
            strides[i] = acc;
            acc = acc.saturating_mul(self.dims[i]);
        }
        strides
    }

    /// Validate that `index` is in bounds for this shape.
    ///
    /// Returns the flat row-major offset on success, or
    /// [`TensorError::IndexOutOfBounds`] / [`TensorError::RankMismatch`]
    /// on failure. Pure arithmetic — no allocation.
    pub fn flat_offset(&self, index: &[usize]) -> TensorResult<usize> {
        if index.len() != self.dims.len() {
            return Err(TensorError::RankMismatch {
                actual: index.len(),
                expected: self.dims.len(),
            });
        }
        let strides = self.strides();
        let mut flat = 0usize;
        for (axis, (&idx, &dim)) in index.iter().zip(self.dims.iter()).enumerate() {
            if idx >= dim {
                return Err(TensorError::IndexOutOfBounds {
                    index: index.to_vec(),
                    shape: self.dims.clone(),
                });
            }
            // Saturating arithmetic: at MVP sizes this never saturates,
            // but the guard is here for forward-safety.
            flat = flat.saturating_add(idx.saturating_mul(strides[axis]));
        }
        Ok(flat)
    }

    /// Iterate over the dimensions.
    pub fn iter(&self) -> impl Iterator<Item = usize> + '_ {
        self.dims.iter().copied()
    }

    /// Compare two shapes for compatibility in elementwise ops (must
    /// be exactly equal — broadcasting is a v1.18+ concern).
    pub fn elementwise_compatible(&self, other: &Shape) -> bool {
        self.dims == other.dims
    }

    /// Validate matmul compatibility: `self` is LHS (m×k), `other` is RHS
    /// (k×n), result is (m×n). Both must be rank-2.
    pub fn matmul_compatible(&self, other: &Shape) -> TensorResult<Shape> {
        if self.dims.len() != 2 || other.dims.len() != 2 {
            return Err(TensorError::RankMismatch {
                actual: self.dims.len(),
                expected: 2,
            });
        }
        if self.dims[1] != other.dims[0] {
            return Err(TensorError::ShapeMismatch {
                lhs: self.dims.clone(),
                rhs: other.dims.clone(),
            });
        }
        Ok(Shape::new_unchecked(vec![self.dims[0], other.dims[1]]))
    }

    /// Validate reduction-along-axis compatibility and return the
    /// resulting shape with that axis removed.
    ///
    /// Negative `axis` counts from the end (Python-style).
    pub fn reduce_axis(&self, axis: isize) -> TensorResult<Shape> {
        let rank = self.dims.len() as isize;
        let signed_axis = if axis < 0 { axis + rank } else { axis };
        if signed_axis < 0 || signed_axis >= rank {
            return Err(TensorError::AxisOutOfBounds {
                axis,
                rank: rank as usize,
            });
        }
        let axis_usize = signed_axis as usize;
        let mut out = Vec::with_capacity(self.dims.len() - 1);
        out.extend_from_slice(&self.dims[..axis_usize]);
        out.extend_from_slice(&self.dims[axis_usize + 1..]);
        if out.is_empty() {
            // Scalar result of reducing a rank-1 tensor along its only axis.
            // We return a rank-1 shape [1] so the result stays a Tensor
            // (T15 ML autodiff will need this).
            Ok(Shape::new_unchecked(vec![1]))
        } else {
            Ok(Shape::new_unchecked(out))
        }
    }
}

/// Check that `rank` is within the MVP cap. Pure helper, exposed for
/// reuse from `Tensor<T>` constructors that take shapes inline.
pub(crate) fn check_rank_cap(rank: usize) -> TensorResult<()> {
    if rank > MVP_RANK_CAP {
        Err(TensorError::RankTooLarge(rank))
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shape_construct_and_query() {
        let s = Shape::new(vec![2, 3, 4]).unwrap();
        assert_eq!(s.as_slice(), &[2, 3, 4]);
        assert_eq!(s.rank(), 3);
        assert_eq!(s.num_elements(), 24);
    }

    #[test]
    fn shape_rejects_empty() {
        let err = Shape::new(vec![]).unwrap_err();
        assert_eq!(err, TensorError::Empty);
    }

    #[test]
    fn shape_rejects_rank_too_large() {
        let err = Shape::new(vec![1, 2, 3, 4, 5]).unwrap_err();
        assert_eq!(err, TensorError::RankTooLarge(5));
    }

    #[test]
    fn shape_strides_row_major() {
        let s = Shape::new(vec![2, 3, 4]).unwrap();
        assert_eq!(s.strides(), vec![12, 4, 1]);
    }

    #[test]
    fn shape_strides_1d() {
        let s = Shape::new(vec![7]).unwrap();
        assert_eq!(s.strides(), vec![1]);
    }

    #[test]
    fn shape_strides_2d() {
        let s = Shape::new(vec![3, 5]).unwrap();
        assert_eq!(s.strides(), vec![5, 1]);
    }

    #[test]
    fn shape_flat_offset_in_bounds() {
        let s = Shape::new(vec![2, 3, 4]).unwrap();
        assert_eq!(s.flat_offset(&[0, 0, 0]).unwrap(), 0);
        assert_eq!(s.flat_offset(&[0, 0, 1]).unwrap(), 1);
        assert_eq!(s.flat_offset(&[0, 1, 0]).unwrap(), 4);
        assert_eq!(s.flat_offset(&[1, 0, 0]).unwrap(), 12);
        assert_eq!(s.flat_offset(&[1, 2, 3]).unwrap(), 23);
    }

    #[test]
    fn shape_flat_offset_out_of_bounds() {
        let s = Shape::new(vec![2, 3]).unwrap();
        let err = s.flat_offset(&[2, 0]).unwrap_err();
        assert_eq!(
            err,
            TensorError::IndexOutOfBounds {
                index: vec![2, 0],
                shape: vec![2, 3],
            }
        );
    }

    #[test]
    fn shape_flat_offset_rank_mismatch() {
        let s = Shape::new(vec![2, 3]).unwrap();
        let err = s.flat_offset(&[0]).unwrap_err();
        assert_eq!(
            err,
            TensorError::RankMismatch {
                actual: 1,
                expected: 2,
            }
        );
    }

    #[test]
    fn shape_matmul_compatible() {
        let a = Shape::new(vec![2, 3]).unwrap();
        let b = Shape::new(vec![3, 4]).unwrap();
        let out = a.matmul_compatible(&b).unwrap();
        assert_eq!(out.as_slice(), &[2, 4]);
    }

    #[test]
    fn shape_matmul_incompatible_inner() {
        let a = Shape::new(vec![2, 3]).unwrap();
        let b = Shape::new(vec![4, 5]).unwrap();
        let err = a.matmul_compatible(&b).unwrap_err();
        assert_eq!(
            err,
            TensorError::ShapeMismatch {
                lhs: vec![2, 3],
                rhs: vec![4, 5],
            }
        );
    }

    #[test]
    fn shape_matmul_rejects_non_2d() {
        let a = Shape::new(vec![2, 3, 4]).unwrap();
        let b = Shape::new(vec![3, 4]).unwrap();
        let err = a.matmul_compatible(&b).unwrap_err();
        assert_eq!(
            err,
            TensorError::RankMismatch {
                actual: 3,
                expected: 2,
            }
        );
    }

    #[test]
    fn shape_reduce_axis_positive() {
        let s = Shape::new(vec![2, 3, 4]).unwrap();
        let r0 = s.reduce_axis(0).unwrap();
        assert_eq!(r0.as_slice(), &[3, 4]);
        let r1 = s.reduce_axis(1).unwrap();
        assert_eq!(r1.as_slice(), &[2, 4]);
        let r2 = s.reduce_axis(2).unwrap();
        assert_eq!(r2.as_slice(), &[2, 3]);
    }

    #[test]
    fn shape_reduce_axis_negative() {
        let s = Shape::new(vec![2, 3, 4]).unwrap();
        let r = s.reduce_axis(-1).unwrap();
        assert_eq!(r.as_slice(), &[2, 3]);
    }

    #[test]
    fn shape_reduce_axis_out_of_bounds() {
        let s = Shape::new(vec![2, 3]).unwrap();
        let err = s.reduce_axis(5).unwrap_err();
        assert_eq!(
            err,
            TensorError::AxisOutOfBounds {
                axis: 5,
                rank: 2,
            }
        );
    }

    #[test]
    fn shape_elementwise_compatible() {
        let a = Shape::new(vec![2, 3]).unwrap();
        let b = Shape::new(vec![2, 3]).unwrap();
        let c = Shape::new(vec![3, 2]).unwrap();
        assert!(a.elementwise_compatible(&b));
        assert!(!a.elementwise_compatible(&c));
    }

    #[test]
    fn shape_iter() {
        let s = Shape::new(vec![2, 3, 4]).unwrap();
        assert_eq!(s.iter().collect::<Vec<_>>(), vec![2, 3, 4]);
    }
}

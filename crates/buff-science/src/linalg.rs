//! Linear algebra operations on `buff_tensor::Tensor`.
//!
//! Pure-Rust implementations of matrix inverse (Gauss-Jordan),
//! determinant (LU decomposition), and solve (Gauss elimination
//! with partial pivoting). Matmul and transpose delegate to
//! buff-tensor's existing implementations.

use crate::error::{ScienceError, ScienceResult};
use buff_tensor::Tensor;

/// Extract a 2-D tensor as a flat row-major `Vec<f64>`.
///
/// # Errors
///
/// Returns [`ScienceError::NotMatrix`] if the tensor is not rank 2.
fn tensor_to_vec(t: &Tensor) -> ScienceResult<(usize, usize, Vec<f64>)> {
    if t.rank() != 2 {
        return Err(ScienceError::NotMatrix(t.rank()));
    }
    let dims = t.shape().as_slice();
    let rows = dims[0];
    let cols = dims[1];
    let data: Vec<f64> = t.as_slice().iter().map(|&v| v as f64).collect();
    Ok((rows, cols, data))
}

/// Create a `Tensor` from a flat row-major `Vec<f64>` with given shape.
fn vec_to_tensor(rows: usize, cols: usize, data: Vec<f64>) -> ScienceResult<Tensor> {
    let f32_data: Vec<f32> = data.into_iter().map(|v| v as f32).collect();
    Tensor::from_vec(f32_data, vec![rows, cols]).map_err(|_| ScienceError::ShapeMismatch {
        lhs: vec![rows, cols],
        rhs: vec![],
    })
}

/// Matrix multiplication: `a * b`.
///
/// Delegates to [`buff_tensor::Tensor::matmul`].
///
/// # Errors
///
/// Returns shape/rank errors if matrices are incompatible.
pub fn matmul(a: &Tensor, b: &Tensor) -> ScienceResult<Tensor> {
    a.matmul(b).map_err(|_| ScienceError::ShapeMismatch {
        lhs: a.shape().as_slice().to_vec(),
        rhs: b.shape().as_slice().to_vec(),
    })
}

/// Transpose a 2-D matrix.
///
/// Delegates to [`buff_tensor::Tensor::transpose`].
///
/// # Errors
///
/// Returns [`ScienceError::NotMatrix`] if the tensor is not rank 2.
pub fn transpose(t: &Tensor) -> ScienceResult<Tensor> {
    t.transpose().map_err(|_| ScienceError::NotMatrix(t.rank()))
}

/// Compute the inverse of a square matrix via Gauss-Jordan elimination.
///
/// # Errors
///
/// - [`ScienceError::NotMatrix`] if not rank 2.
/// - [`ScienceError::NotSquare`] if not square.
/// - [`ScienceError::SingularMatrix`] if the matrix is singular.
pub fn inverse(m: &Tensor) -> ScienceResult<Tensor> {
    let (rows, cols, data) = tensor_to_vec(m)?;
    if rows != cols {
        return Err(ScienceError::NotSquare { rows, cols });
    }
    let n = rows;
    // Build augmented matrix [A | I].
    let mut aug = vec![0.0f64; n * 2 * n];
    for i in 0..n {
        for j in 0..n {
            aug[i * 2 * n + j] = data[i * n + j];
        }
        aug[i * 2 * n + n + i] = 1.0;
    }
    // Gauss-Jordan elimination with partial pivoting.
    for col in 0..n {
        // Find pivot.
        let mut max_val = aug[col * 2 * n + col].abs();
        let mut max_row = col;
        for row in (col + 1)..n {
            let val = aug[row * 2 * n + col].abs();
            if val > max_val {
                max_val = val;
                max_row = row;
            }
        }
        if max_val < 1e-12 {
            return Err(ScienceError::SingularMatrix);
        }
        // Swap rows.
        if max_row != col {
            for j in 0..2 * n {
                aug.swap(col * 2 * n + j, max_row * 2 * n + j);
            }
        }
        // Scale pivot row.
        let pivot = aug[col * 2 * n + col];
        for j in 0..2 * n {
            aug[col * 2 * n + j] /= pivot;
        }
        // Eliminate column.
        for row in 0..n {
            if row == col {
                continue;
            }
            let factor = aug[row * 2 * n + col];
            for j in 0..2 * n {
                aug[row * 2 * n + j] -= factor * aug[col * 2 * n + j];
            }
        }
    }
    // Extract inverse from right half.
    let mut inv = vec![0.0f64; n * n];
    for i in 0..n {
        for j in 0..n {
            inv[i * n + j] = aug[i * 2 * n + n + j];
        }
    }
    vec_to_tensor(n, n, inv)
}

/// Compute the determinant of a square matrix via LU decomposition.
///
/// # Errors
///
/// - [`ScienceError::NotMatrix`] if not rank 2.
/// - [`ScienceError::NotSquare`] if not square.
pub fn determinant(m: &Tensor) -> ScienceResult<f64> {
    let (rows, cols, data) = tensor_to_vec(m)?;
    if rows != cols {
        return Err(ScienceError::NotSquare { rows, cols });
    }
    let n = rows;
    let mut lu = data;
    let mut det = 1.0f64;
    let mut perm_sign = 1i32;

    for col in 0..n {
        // Partial pivoting.
        let mut max_val = lu[col * n + col].abs();
        let mut max_row = col;
        for row in (col + 1)..n {
            let val = lu[row * n + col].abs();
            if val > max_val {
                max_val = val;
                max_row = row;
            }
        }
        if max_row != col {
            for j in 0..n {
                lu.swap(col * n + j, max_row * n + j);
            }
            perm_sign *= -1;
        }
        let pivot = lu[col * n + col];
        if pivot.abs() < 1e-15 {
            return Ok(0.0);
        }
        det *= pivot;
        // Eliminate below.
        for row in (col + 1)..n {
            let factor = lu[row * n + col] / pivot;
            for j in (col + 1)..n {
                lu[row * n + j] -= factor * lu[col * n + j];
            }
        }
    }
    Ok(det * perm_sign as f64)
}

/// Solve the linear system `a * x = b` for `x` via Gauss elimination
/// with partial pivoting.
///
/// Returns `x` such that `a * x` is approximately `b`.
///
/// # Errors
///
/// - [`ScienceError::NotMatrix`] if inputs are not rank 2.
/// - [`ScienceError::NotSquare`] if `a` is not square.
/// - [`ScienceError::SingularMatrix`] if `a` is singular.
/// - [`ScienceError::ShapeMismatch`] if row counts differ.
pub fn solve(a: &Tensor, b: &Tensor) -> ScienceResult<Tensor> {
    let (a_rows, a_cols, a_data) = tensor_to_vec(a)?;
    let (b_rows, b_cols, b_data) = tensor_to_vec(b)?;
    if a_rows != a_cols {
        return Err(ScienceError::NotSquare {
            rows: a_rows,
            cols: a_cols,
        });
    }
    if a_rows != b_rows {
        return Err(ScienceError::ShapeMismatch {
            lhs: vec![a_rows, a_cols],
            rhs: vec![b_rows, b_cols],
        });
    }
    let n = a_rows;
    let n_rhs = b_cols;
    // Augmented matrix [A | B].
    let mut aug = vec![0.0f64; n * (n + n_rhs)];
    for i in 0..n {
        for j in 0..n {
            aug[i * (n + n_rhs) + j] = a_data[i * n + j];
        }
        for j in 0..n_rhs {
            aug[i * (n + n_rhs) + n + j] = b_data[i * n_rhs + j];
        }
    }
    // Forward elimination with partial pivoting.
    for col in 0..n {
        let mut max_val = aug[col * (n + n_rhs) + col].abs();
        let mut max_row = col;
        for row in (col + 1)..n {
            let val = aug[row * (n + n_rhs) + col].abs();
            if val > max_val {
                max_val = val;
                max_row = row;
            }
        }
        if max_val < 1e-12 {
            return Err(ScienceError::SingularMatrix);
        }
        if max_row != col {
            for j in 0..(n + n_rhs) {
                aug.swap(col * (n + n_rhs) + j, max_row * (n + n_rhs) + j);
            }
        }
        // Eliminate below.
        for row in (col + 1)..n {
            let factor = aug[row * (n + n_rhs) + col] / aug[col * (n + n_rhs) + col];
            for j in col..(n + n_rhs) {
                aug[row * (n + n_rhs) + j] -= factor * aug[col * (n + n_rhs) + j];
            }
        }
    }
    // Back substitution.
    let mut x = vec![0.0f64; n * n_rhs];
    for i in (0..n).rev() {
        for j in 0..n_rhs {
            let mut sum = aug[i * (n + n_rhs) + n + j];
            for k in (i + 1)..n {
                sum -= aug[i * (n + n_rhs) + k] * x[k * n_rhs + j];
            }
            x[i * n_rhs + j] = sum / aug[i * (n + n_rhs) + i];
        }
    }
    vec_to_tensor(n, n_rhs, x)
}

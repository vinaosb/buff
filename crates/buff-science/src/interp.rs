//! Interpolation methods.

use crate::error::{ScienceError, ScienceResult};

/// Linear interpolation between tabulated (x, y) pairs.
///
/// `xs` must be sorted in ascending order. Returns the interpolated
/// `y` value at position `x`.
///
/// # Errors
///
/// - [`ScienceError::Empty`] if `xs` or `ys` is empty.
/// - [`ScienceError::LengthMismatch`] if `xs.len() != ys.len()`.
/// - [`ScienceError::OutOfRange`] if `x` is outside the range of `xs`.
pub fn linear(xs: &[f64], ys: &[f64], x: f64) -> ScienceResult<f64> {
    if xs.is_empty() || ys.is_empty() {
        return Err(ScienceError::Empty);
    }
    if xs.len() != ys.len() {
        return Err(ScienceError::LengthMismatch {
            expected: xs.len(),
            got: ys.len(),
        });
    }
    if x < xs[0] || x > *xs.last().ok_or(ScienceError::Empty)? {
        return Err(ScienceError::OutOfRange {
            x,
            min: xs[0],
            max: *xs.last().ok_or(ScienceError::Empty)?,
        });
    }
    // Binary search for the interval.
    let n = xs.len();
    if n == 1 {
        return Ok(ys[0]);
    }
    // Find the right endpoint of the interval containing x.
    let idx =
        match xs.binary_search_by(|xi| xi.partial_cmp(&x).unwrap_or(std::cmp::Ordering::Equal)) {
            Ok(i) => {
                if i + 1 < n {
                    i
                } else {
                    i - 1
                }
            }
            Err(i) => {
                if i == 0 {
                    0
                } else if i >= n {
                    n - 2
                } else {
                    i - 1
                }
            }
        };
    let x0 = xs[idx];
    let x1 = xs[idx + 1];
    let y0 = ys[idx];
    let y1 = ys[idx + 1];
    let t = (x - x0) / (x1 - x0);
    Ok(y0 + t * (y1 - y0))
}

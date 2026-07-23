//! Numerical optimization methods.

use crate::error::{ScienceError, ScienceResult};

/// Gradient descent minimizer.
///
/// Minimizes a function `f: R^n -> R` by iteratively moving in the
/// negative gradient direction. Returns the final parameter vector.
///
/// `f` takes a parameter slice and returns `(value, gradient)`.
/// `initial` is the starting parameter vector.
/// `lr` is the learning rate.
/// `steps` is the number of iterations.
///
/// # Errors
///
/// - [`ScienceError::Empty`] if `initial` is empty.
/// - [`ScienceError::ConvergenceFailed`] if the gradient norm is NaN.
///
/// # Example
///
/// ```ignore
/// // Minimize f(x) = x^2. Optimal x = 0.
/// let result = gradient_descent(
///     |x| { let v = x[0]; (v * v, vec![2.0 * v]) },
///     vec![5.0],
///     0.1,
///     100,
/// ).unwrap();
/// assert!((result[0]).abs() < 1e-3);
/// ```
pub fn gradient_descent(
    f: impl Fn(&[f64]) -> (f64, Vec<f64>),
    initial: Vec<f64>,
    lr: f64,
    steps: usize,
) -> ScienceResult<Vec<f64>> {
    if initial.is_empty() {
        return Err(ScienceError::Empty);
    }
    let n = initial.len();
    let mut params = initial;

    for _ in 0..steps {
        let (_val, grad) = f(&params);
        if grad.iter().any(|g| g.is_nan()) {
            return Err(ScienceError::ConvergenceFailed { steps });
        }
        for i in 0..n {
            params[i] -= lr * grad[i];
        }
    }
    Ok(params)
}

//! Optimization methods.

/// Gradient descent optimizer.
///
/// Minimizes `f` starting from `initial`, using a fixed learning rate
/// `lr` for `steps` iterations. `gradient` computes the gradient of `f`
/// at a given point.
///
/// Returns the final parameter vector.
///
/// # Example
///
/// ```ignore
/// // Minimize f(x) = x^2. Minimum at x = 0.
/// let result = gradient_descent(
///     |x| x * x,
///     |x| 2.0 * x,
///     vec![5.0],
///     0.1,
///     100,
/// );
/// assert!((result[0]).abs() < 0.01);
/// ```
pub fn gradient_descent(
    _f: impl Fn(&[f64]) -> f64,
    gradient: impl Fn(&[f64]) -> Vec<f64>,
    initial: Vec<f64>,
    lr: f64,
    steps: usize,
) -> Vec<f64> {
    let mut params = initial;
    for _ in 0..steps {
        let g = gradient(&params);
        for (p, gi) in params.iter_mut().zip(g.iter()) {
            *p -= lr * gi;
        }
    }
    params
}

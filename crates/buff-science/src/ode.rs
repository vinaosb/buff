//! Ordinary differential equation solvers.
//!
//! MVP ships the classical 4th-order Runge-Kutta method (RK4).

/// Classical 4th-order Runge-Kutta integrator.
///
/// Solves `dy/dt = f(t, y)` from `t_start` to `t_end` with step size
/// `step`. Returns the final `y` value.
///
/// `f` is the ODE right-hand side: `f(t, y) -> dy/dt`.
/// `initial` is `y(t_start)`.
///
/// # Example
///
/// ```ignore
/// // Solve dy/dt = y, y(0) = 1. Solution: y(t) = e^t.
/// let result = rk4(|_t, y| y, 1.0, 0.0, 1.0, 0.01);
/// assert!((result - std::f64::consts::E).abs() < 1e-4);
/// ```
pub fn rk4(f: impl Fn(f64, f64) -> f64, initial: f64, t_start: f64, t_end: f64, step: f64) -> f64 {
    if step <= 0.0 || t_end <= t_start {
        return initial;
    }
    let n_steps = ((t_end - t_start) / step).ceil() as usize;
    let h = (t_end - t_start) / n_steps as f64;
    let mut t = t_start;
    let mut y = initial;
    for _ in 0..n_steps {
        let k1 = h * f(t, y);
        let k2 = h * f(t + h / 2.0, y + k1 / 2.0);
        let k3 = h * f(t + h / 2.0, y + k2 / 2.0);
        let k4 = h * f(t + h, y + k3);
        y += (k1 + 2.0 * k2 + 2.0 * k3 + k4) / 6.0;
        t += h;
    }
    y
}

/// Vector RK4 for systems of ODEs.
///
/// Solves `dy/dt = f(t, y)` where `y` is a `Vec<f64>`.
/// Returns the final state vector.
pub fn rk4_vec(
    f: impl Fn(f64, &[f64]) -> Vec<f64>,
    initial: Vec<f64>,
    t_start: f64,
    t_end: f64,
    step: f64,
) -> Vec<f64> {
    if step <= 0.0 || t_end <= t_start {
        return initial;
    }
    let n_steps = ((t_end - t_start) / step).ceil() as usize;
    let h = (t_end - t_start) / n_steps as f64;
    let n = initial.len();
    let mut t = t_start;
    let mut y = initial;

    for _ in 0..n_steps {
        let k1 = f(t, &y);
        let y2: Vec<f64> = y
            .iter()
            .zip(k1.iter())
            .map(|(yi, ki)| yi + h / 2.0 * ki)
            .collect();
        let k2 = f(t + h / 2.0, &y2);
        let y3: Vec<f64> = y
            .iter()
            .zip(k2.iter())
            .map(|(yi, ki)| yi + h / 2.0 * ki)
            .collect();
        let k3 = f(t + h / 2.0, &y3);
        let y4: Vec<f64> = y
            .iter()
            .zip(k3.iter())
            .map(|(yi, ki)| yi + h * ki)
            .collect();
        let k4 = f(t + h, &y4);

        for i in 0..n {
            y[i] += h / 6.0 * (k1[i] + 2.0 * k2[i] + 2.0 * k3[i] + k4[i]);
        }
        t += h;
    }
    y
}

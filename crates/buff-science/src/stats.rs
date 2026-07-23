//! Descriptive statistics for Buff.
//!
//! Provides fundamental statistical functions operating on `f64` slices.
//! All functions are pure-Rust implementations.

use crate::error::{ScienceError, ScienceResult};
use std::collections::BTreeMap;

/// Compute the arithmetic mean of a dataset.
///
/// # Errors
///
/// - [`ScienceError::Empty`] if the slice is empty.
pub fn mean(data: &[f64]) -> ScienceResult<f64> {
    if data.is_empty() {
        return Err(ScienceError::Empty);
    }
    let sum: f64 = data.iter().sum();
    Ok(sum / data.len() as f64)
}

/// Compute the population variance of a dataset.
///
/// Uses the population formula (divides by N, not N-1).
///
/// # Errors
///
/// - [`ScienceError::Empty`] if the slice has fewer than 2 elements.
pub fn variance(data: &[f64]) -> ScienceResult<f64> {
    if data.len() < 2 {
        return Err(ScienceError::Empty);
    }
    let m = mean(data)?;
    let sum_sq: f64 = data.iter().map(|&x| (x - m).powi(2)).sum();
    Ok(sum_sq / data.len() as f64)
}

/// Compute the population standard deviation of a dataset.
///
/// # Errors
///
/// - [`ScienceError::Empty`] if the slice has fewer than 2 elements.
pub fn stddev(data: &[f64]) -> ScienceResult<f64> {
    let v = variance(data)?;
    Ok(v.sqrt())
}

/// Compute the Pearson correlation coefficient between two datasets.
///
/// Returns a value in `[-1, 1]` where 1 is perfect positive correlation,
/// -1 is perfect negative correlation, and 0 is no correlation.
///
/// # Errors
///
/// - [`ScienceError::LengthMismatch`] if the slices have different lengths.
/// - [`ScienceError::Empty`] if slices have fewer than 2 elements.
pub fn correlation(x: &[f64], y: &[f64]) -> ScienceResult<f64> {
    if x.len() != y.len() {
        return Err(ScienceError::LengthMismatch {
            expected: x.len(),
            got: y.len(),
        });
    }
    if x.len() < 2 {
        return Err(ScienceError::Empty);
    }
    let mx = mean(x)?;
    let my = mean(y)?;
    let mut sum_xy = 0.0;
    let mut sum_x2 = 0.0;
    let mut sum_y2 = 0.0;
    for (&xi, &yi) in x.iter().zip(y.iter()) {
        let dx = xi - mx;
        let dy = yi - my;
        sum_xy += dx * dy;
        sum_x2 += dx * dx;
        sum_y2 += dy * dy;
    }
    let denom = (sum_x2 * sum_y2).sqrt();
    if denom < 1e-15 {
        return Ok(0.0);
    }
    Ok(sum_xy / denom)
}

/// Compute a histogram of a dataset.
///
/// Returns `bins` equally-spaced bins from min to max, with counts.
/// Returns a `BTreeMap<usize, usize>` mapping bin index to count.
///
/// # Errors
///
/// - [`ScienceError::Empty`] if the data is empty or `bins` is 0.
pub fn histogram(data: &[f64], bins: usize) -> ScienceResult<BTreeMap<usize, usize>> {
    if data.is_empty() {
        return Err(ScienceError::Empty);
    }
    if bins == 0 {
        return Err(ScienceError::Empty);
    }
    let min = data.iter().cloned().fold(f64::INFINITY, f64::min);
    let max = data.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
    let range = max - min;
    let mut counts = BTreeMap::new();
    for &v in data {
        let bin = if range < 1e-15 {
            0
        } else {
            let b = ((v - min) / range * bins as f64).floor() as usize;
            b.min(bins - 1)
        };
        *counts.entry(bin).or_insert(0) += 1;
    }
    Ok(counts)
}

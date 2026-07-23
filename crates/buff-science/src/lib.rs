//! `buff-science` — Linear algebra, numerical methods, and statistics for Buff.
//!
//! Builds on [`buff_tensor::Tensor`] for matrix operations with pure-Rust
//! implementations of inverse (Gauss-Jordan), determinant (LU), and solve
//! (Gauss elimination). Provides ODE integration (RK4), interpolation,
//! optimization, and statistical functions.
//!
//! # Quick start
//!
//! ```ignore
//! use buff_science::linalg;
//! use buff_tensor::Tensor;
//!
//! let a = Tensor::from_vec(vec![1.0, 2.0, 3.0, 4.0], vec![2, 2]).unwrap();
//! let b = Tensor::from_vec(vec![5.0, 6.0, 7.0, 8.0], vec![2, 2]).unwrap();
//! let c = linalg::matmul(&a, &b).unwrap();
//! assert_eq!(c.as_slice(), &[19.0, 22.0, 43.0, 50.0]);
//! ```
//!
//! # Modules
//!
//! - [`linalg`] — matrix operations (matmul, transpose, inverse, determinant, solve)
//! - [`ode`] — ODE solvers (RK4)
//! - [`interp`] — interpolation (linear)
//! - [`optimize`] — optimization (gradient descent)
//! - [`stats`] — statistics (mean, variance, stddev, correlation, histogram)

// Project hard rule: no unwrap/expect/panic in non-test code.
#![cfg_attr(not(test), forbid(clippy::unwrap_used))]
#![cfg_attr(not(test), forbid(clippy::expect_used))]
#![cfg_attr(not(test), forbid(clippy::panic))]

pub mod error;
pub mod interp;
pub mod linalg;
pub mod ode;
pub mod optimize;
pub mod stats;

pub use error::{ScienceError, ScienceResult};

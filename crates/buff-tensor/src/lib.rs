//! `buff-tensor` — N-dimensional arrays (rank ≤ 4) for Buff.
//!
//! MVP per T8 (`.sisyphus/plans/buff-v1x-frameworks.md#L1464`):
//! - dtype f32 ONLY (defer f64/i64 to v1.18+).
//! - CPU-only via rayon. Per T6 decision
//!   (`.sisyphus/decisions/wgsl-extensibility-v1x.md` §3): elementwise
//!   GPU dispatch is feasible as a v1.18+ enhancement (~50 LOC);
//!   matmul + reduce GPU paths are ~1500 LOC / ~15 days, deferred.
//! - No autodiff (T15 buff-ml), no broadcasting, no sparse tensors.
//!
//! # Quick start
//!
//! ```
//! use buff_tensor::Tensor;
//!
//! let a = Tensor::from_vec(vec![1.0, 2.0, 3.0, 4.0], vec![2, 2]).unwrap();
//! let b = Tensor::from_vec(vec![5.0, 6.0, 7.0, 8.0], vec![2, 2]).unwrap();
//! let c = a.matmul(&b).unwrap();
//! assert_eq!(c.as_slice(), &[19.0, 22.0, 43.0, 50.0]);
//! ```
//!
//! # Convention summary
//!
//! - **Layout**: row-major (C-order). Last axis varies fastest.
//! - **Rank cap**: 4 ([`shape::MVP_RANK_CAP`]).
//! - **Negative axis**: counts from the end (Python-style: `-1` is the
//!   last axis).
//! - **Errors**: all fallible ops return `Result<_, TensorError>`.
//!   No `unwrap`/`expect`/`panic!` in non-test code.
//! - **Determinism**: rayon-parallel ops preserve input order via
//!   rayon's ordered `collect` (matches the contract in
//!   `buff_lang_runtime::cpu::CpuDispatcher::par_map`).

// Project hard rule: no `unwrap`/`expect`/`panic!` in NON-TEST code.
// Apply the clippy forbid at the crate root for non-test paths only
// (cfg(test) modules are exempt — the rule allows them in tests).
#![cfg_attr(not(test), forbid(clippy::unwrap_used))]
#![cfg_attr(not(test), forbid(clippy::expect_used))]
#![cfg_attr(not(test), forbid(clippy::panic))]

pub mod error;
pub mod math;
pub mod shape;
pub mod tensor;

pub use error::{TensorError, TensorResult};
pub use shape::{Shape, MVP_RANK_CAP};
pub use tensor::{Tensor, TensorCore};

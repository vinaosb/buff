//! `buff-ml` — Neural network layers + reverse-mode autodiff on `buff-tensor`.
//!
//! MVP per T15 (`.sisyphus/plans/buff-v1x-frameworks.md#L2068`):
//! - **dtype**: `f32` ONLY (defer f64 to v1.18+, same cap as `buff-tensor`).
//! - **CPU-only** via rayon. GPU dispatch deferred to v1.18+ (per T6 decision,
//!   same family of cc-rs avoidance that pushed `buff-tensor` to pure-Rust).
//! - **Reverse-mode autodiff** via a define-by-run computation tape
//!   (micrograd pattern — <https://github.com/karpathy/micrograd>). Each
//!   forward pass records ops on a [`Tape`]; [`Var::backward`] walks the
//!   tape in reverse, accumulating gradients into leaf nodes.
//! - **Layers**: [`Linear`], [`ReLU`], [`Sigmoid`], [`Softmax`], [`Dropout`].
//! - **Losses**: [`mse_loss`], [`cross_entropy`].
//! - **Optimizers**: [`SGD`], [`Adam`].
//! - **Serialization**: JSON only (ONNX/safetensors deferred to v1.18+).
//! - **NO CNNs/RNNs/Transformers** (defer v1.18+).
//! - **NO distributed training** (defer v1.19+).
//!
//! # Quick start
//!
//! ```
//! use buff_ml::{Tape, mse_loss, Model, Optimizer, SGD, Linear};
//! use buff_tensor::Tensor;
//!
//! // Train y = 2x + 3 on synthetic data.
//! let x = Tensor::from_vec(
//!     vec![-1.0, -0.5, 0.0, 0.5, 1.0],
//!     vec![5, 1],
//! ).unwrap();
//! let y = Tensor::from_vec(
//!     vec![1.0, 2.0, 3.0, 4.0, 5.0],
//!     vec![5, 1],
//! ).unwrap();
//!
//! let mut model = Model::sequential(vec![
//!     Box::new(Linear::new(1, 1).unwrap()),
//! ]);
//! let mut opt = SGD::new(0.1).unwrap();
//!
//! for _ in 0..200 {
//!     let tape = Tape::new();
//!     let xv = tape.leaf(x.clone(), false).unwrap();
//!     let pred = model.forward(xv.clone(), true).unwrap();
//!     let loss = mse_loss(&pred, &y).unwrap();
//!     model.backward(&loss).unwrap();
//!     opt.step(&mut model).unwrap();
//! }
//! // model now approximates y = 2x + 3.
//! ```
//!
//! # Convention summary
//!
//! - **Autodiff is define-by-run**: a fresh [`Tape`] per forward pass; leaf
//!   nodes that `requires_grad` accumulate gradients during [`Var::backward`].
//! - **f32 ONLY** for MVP (per T15 spec — no f64 autodiff).
//! - **All fallible ops return `Result<_, MlError>`**. No `unwrap`/`expect`/
//!   `panic!` in non-test code (project hard rule).
//! - **`BTreeMap`/`BTreeSet` only** where collections are used (project rule).
//! - **JSON serialization** for `Model::save` / `Model::load` (layer kind +
//!   flat weight/bias buffers + shapes).
//! - **Single-threaded training** for MVP (the tape uses `Rc<RefCell<...>>`;
//!   `Var` is therefore `!Send`). Multi-threaded data-parallel training is a
//!   v1.19+ concern.

// Project hard rule: no `unwrap`/`expect`/`panic!` in NON-TEST code.
// Apply the clippy forbid at the crate root for non-test paths only
// (cfg(test) modules are exempt — the rule allows them in tests).
#![cfg_attr(not(test), forbid(clippy::unwrap_used))]
#![cfg_attr(not(test), forbid(clippy::expect_used))]
#![cfg_attr(not(test), forbid(clippy::panic))]

pub mod autodiff;
pub mod error;
pub mod io;
pub mod layer;
pub mod loss;
pub mod model;
pub mod optimizer;

pub use autodiff::{Tape, Var};
pub use error::{MlError, MlResult};
pub use layer::{Dropout, Layer, Linear, ReLU, Sigmoid, Softmax};
pub use loss::{cross_entropy, mse_loss};
pub use model::Model;
pub use optimizer::{Adam, Optimizer, SGD};

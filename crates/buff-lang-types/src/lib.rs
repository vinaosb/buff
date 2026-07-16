//! Buff Types crate — type representation and local type inference for Buff.
//!
//! This crate is the semantic-analysis stage that sits between parsing
//! (`buff-lang-parser`) and code generation (`buff-codegen-*`). It defines:
//!
//! - the resolved [`Type`] representation ([`ty`]),
//! - numeric promotion rules ([`promote`]),
//! - a flat symbol table ([`env`]),
//! - a local [`TypeInferencer`] that walks the AST ([`infer`]).
//!
//! v0.1 supports only primitive types; v0.5 will add collections and
//! user-defined types.

pub mod env;
pub mod infer;
pub mod promote;
pub mod range_analysis;
pub mod ty;

pub use env::TypeEnv;
pub use infer::TypeInferencer;
pub use promote::{assignable_to, promote_binary};
// T22: pure range-analysis primitives (flexible-mode Int width inference,
// auto-width collection helper). Re-exported at crate root for convenience;
// the module path `range_analysis::` is the canonical location.
pub use range_analysis::{collection_int_width, smallest_int_width, IntRange};
pub use ty::{FloatWidth, IntWidth, Type};

// Re-export `Span` from `buff-lang-error` for downstream convenience.
pub use buff_lang_error::Span;

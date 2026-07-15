//! Deox Types crate — type representation and local type inference for Deox.
//!
//! This crate is the semantic-analysis stage that sits between parsing
//! (`deox-parser`) and code generation (`deox-codegen-*`). It defines:
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
pub mod ty;

pub use env::TypeEnv;
pub use infer::TypeInferencer;
pub use promote::{assignable_to, promote_binary};
pub use ty::{FloatWidth, IntWidth, Type};

// Re-export `Span` from `deox-error` for downstream convenience.
pub use deox_error::Span;

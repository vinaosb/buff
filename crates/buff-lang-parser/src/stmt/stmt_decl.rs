//! Declaration parsers - module root of the ITER-52 family split.
//!
//! The former 2,742-line `stmt_decl.rs` (itself the T106 extraction from
//! `stmt.rs`) is split by declaration family into the sibling modules under
//! `stmt_decl/`. All public parsers re-export flat, so the parent `stmt`
//! module's `pub use stmt_decl::*` and every existing call site are
//! unchanged - a pure move with zero snapshot churn.

mod aggregates;
mod extern_abi;
mod func;
mod import_export;
mod shared;
mod trait_impl;

pub use aggregates::*;
pub use extern_abi::*;
pub use func::*;
pub use import_export::*;
pub use shared::{extract_ident, parse_attributes, type_end};
pub use trait_impl::*;

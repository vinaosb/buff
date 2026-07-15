//! Deox Error crate — Error types and diagnostics for the Deox language.

pub mod diagnostic;
pub mod source_map;
pub mod span;

pub use diagnostic::*;
pub use source_map::*;
pub use span::*;

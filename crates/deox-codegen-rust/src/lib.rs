//! Deox Rust codegen crate — converts Deox AST to Rust source via syn/quote/prettyplease.
//!
//! ## Pipeline
//!
//! ```text
//!   &[deox_ast::Decl]
//!        │
//!        ▼  RustCodegen::generate
//!   syn::File
//!        │
//!        ▼  format::format  (prettyplease::unparse)
//!   String  (valid Rust source)
//! ```
//!
//! Every Rust construct is built through `syn` types — we never hand-format
//! Rust strings. The single string producer is `prettyplease`, whose output
//! is equivalent to a `rustfmt` pass.
//!
//! # Example
//!
//! ```
//! use deox_ast::{Decl, common::{Block, Ident}, decl::FuncDecl};
//! use deox_error::Span;
//!
//! let func = FuncDecl {
//!     name: Ident::new("empty", Span::dummy()),
//!     params: Vec::new(),
//!     return_type: None,
//!     body: Block::empty(Span::dummy()),
//!     is_async: false,
//!     is_unsafe: false,
//!     is_extern: false,
//!     span: Span::dummy(),
//! };
//! let src = deox_codegen_rust::generate_rust(&[Decl::FuncDecl(func)]).unwrap();
//! assert!(src.contains("fn empty()"));
//! ```

pub mod context;
pub mod format;
pub mod move_analysis;
pub mod rust_codegen;

pub use context::CodegenContext;
pub use format::format;
pub use move_analysis::MoveAnalyzer;
pub use rust_codegen::RustCodegen;

/// Convenience: lower a slice of Deox declarations to formatted Rust source.
///
/// Equivalent to building a [`RustCodegen`], calling [`RustCodegen::generate`],
/// then [`format`] on the result.
pub fn generate_rust(decls: &[deox_ast::Decl]) -> Result<String, deox_error::CodegenError> {
    let mut codegen = RustCodegen::new();
    let file = codegen.generate(decls)?;
    Ok(format(&file))
}

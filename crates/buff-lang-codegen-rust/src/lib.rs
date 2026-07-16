//! Buff Rust codegen crate — converts Buff AST to Rust source via syn/quote/prettyplease.
//!
//! ## Pipeline
//!
//! ```text
//!   &[buff_lang_ast::Decl]
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
//! use buff_lang_ast::{Decl, common::{Block, Ident}, decl::FuncDecl};
//! use buff_lang_error::Span;
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
//! let src = buff_lang_codegen_rust::generate_rust(&[Decl::FuncDecl(func)]).unwrap();
//! assert!(src.contains("fn empty()"));
//! ```

pub mod context;
pub mod format;
pub mod move_analysis;
pub mod rust_codegen;

pub use context::CodegenContext;
pub use format::format;
pub use move_analysis::MoveAnalyzer;
pub use rust_codegen::{buff_primitive_to_rust_name, RustCodegen};

/// Convenience alias for [`format`] so external callers (tests, the CLI)
/// can refer to it without importing the module. T26 introduced the alias
/// so the `struct_codegen` integration tests can format a `syn::File` they
/// obtained from the lower-level [`RustCodegen::generate`] entry point
/// (needed for the `#[repr(C)]` hook test which bypasses the convenience
/// [`generate_rust`] wrapper).
pub fn format_file(file: &syn::File) -> String {
    format(file)
}

/// Convenience: lower a slice of Buff declarations to formatted Rust source.
///
/// Equivalent to building a [`RustCodegen`], calling [`RustCodegen::generate`],
/// then [`format`] on the result.
pub fn generate_rust(
    decls: &[buff_lang_ast::Decl],
) -> Result<String, buff_lang_error::CodegenError> {
    let mut codegen = RustCodegen::new();
    let file = codegen.generate(decls)?;
    Ok(format(&file))
}

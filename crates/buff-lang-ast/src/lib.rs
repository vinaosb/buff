//! Buff AST crate — abstract syntax tree node definitions.
//!
//! This crate is **pure data**: it defines the node types produced by parsing
//! and consumed by later stages (type checking, codegen). It contains no
//! parsing logic and no type checking.
//!
//! # Module layout
//!
//! - `op`: binary/unary operator enums ([`BinaryOp`], [`UnaryOp`]).
//! - `common`: [`Ident`], [`Block`], [`Param`].
//! - `ty`: [`TypeRef`] type references.
//! - `expr`: [`Literal`], [`Expr`], [`MatchArm`], [`Pattern`].
//! - `stmt`: [`Stmt`].
//! - `decl`: [`Decl`] and the specific declaration structs.
//! - `ir`: the dataflow-graph intermediate representation
//!   ([`IrGraph`], [`IrNode`], [`AstLowerer`]).
//! - `lossless`: trivia-preserving source representation for LSP and
//!   comment-preserving `buff fmt` (T57). See [`lossless::LosslessTree`].
//!
//! [`Span`] is re-exported from `buff-lang-error` for convenience.

pub mod common;
pub mod decl;
pub mod expr;
pub mod ir;
pub mod lossless;
pub mod op;
pub mod stmt;
pub mod ty;

pub use common::*;
pub use decl::*;
pub use expr::*;
pub use ir::*;
pub use op::*;
pub use stmt::*;
pub use ty::*;

// Re-export `Span` from `buff-lang-error` for convenience — do not redefine.
pub use buff_lang_error::Span;

// P0.1.2b: Re-export the deterministic `Span` JSON serializer so consumers
// (e.g. `buff check --dump-ast`) can call `buff_lang_ast::span_to_json(span)`
// without reaching into the `common` submodule. Defined in `common.rs`
// because `Span` is a foreign type and `common` is the natural home for
// span-related helpers.
pub use common::span_to_json;

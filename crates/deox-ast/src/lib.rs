//! Deox AST crate — abstract syntax tree node definitions.
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
//!
//! [`Span`] is re-exported from `deox-error` for convenience.

pub mod common;
pub mod decl;
pub mod expr;
pub mod ir;
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

// Re-export `Span` from `deox-error` for convenience — do not redefine.
pub use deox_error::Span;

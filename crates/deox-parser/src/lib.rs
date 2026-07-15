//! Deox Parser crate — hand-rolled recursive-descent + Pratt parser.
//!
//! Converts a slice of [`deox_lexer::Token`]s (produced by the T6 lexer)
//! into AST nodes ([`deox_ast::Decl`] / [`deox_ast::Expr`]).
//!
//! ## Why hand-rolled?
//!
//! The codebase originally intended to use `chumsky` 1.0.0-alpha.8, but
//! that crate transitively depends on `stacker` which uses `cc-rs` to
//! compile a C shim. On this Windows host the Windows SDK is missing
//! `excpt.h`, so the C compile fails. Hand-rolling was previously chosen
//! for the lexer (T6) for the same family of reasons and worked well.
//!
//! ## Layout
//!
//! - [`stream`]: the [`TokenStream`] cursor (peek / next / expect).
//! - [`expr`]: expression parser entry point ([`expr::parse_expression`]).
//! - [`parser`]: top-level [`parser::parse`] returning `Vec<Decl>`.

pub mod expr;
pub mod parser;
pub mod stream;

pub use parser::{parse, parse_expression};
pub use stream::TokenStream;

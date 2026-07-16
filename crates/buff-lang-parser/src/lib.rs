//! Buff Parser crate — hand-rolled recursive-descent + Pratt parser.
//!
//! Converts a slice of [`buff_lang_lexer::Token`]s (produced by the T6 lexer)
//! into AST nodes ([`buff_lang_ast::Decl`] / [`buff_lang_ast::Expr`]).
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
//! - [`stmt`]: statement parser entry point ([`stmt::parse_statement`]).
//! - [`parser`]: top-level [`parser::parse`] returning `Vec<Decl>`.

pub mod expr;
pub mod parser;
pub mod stmt;
pub mod stream;

pub use parser::{parse, parse_expression};
pub use stmt::{
    parse_block, parse_block_braces, parse_enum_decl, parse_func_decl, parse_if_expr, parse_params,
    parse_statement, parse_type_ref,
};
pub use stream::TokenStream;

// T27: `match` parsing lives in `expr` (match is an expression, like `if`).
pub use expr::{parse_match, parse_pattern};

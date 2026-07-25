//! Buff lexer crate — token definitions, lexer implementation, indentation
//! algorithm, and string interpolation scanner.
//!
//! This crate defines the token types ([`TokenKind`], [`Token`]), the error
//! type ([`LexerError`]), and the main entry point [`tokenize`].
//!
//! ## Layout
//!
//! - [`token`]: token kind enum and spanned-token struct.
//! - [`error`]: lexer error wrapper around `buff_lang_error::LexError`.
//! - [`lexer`]: hand-rolled byte-scanner producing `Vec<Token>`.
//! - [`indent`]: offside-rule indentation tracker.
//! - [`string_interp`]: string-literal scanner with `{expr}` interpolation.

// Boxing the lexer error wrapper would change the public `tokenize` Result
// surface for every consumer. Out of scope; allowed at the crate level.
#![allow(clippy::result_large_err)]

pub mod error;
pub mod indent;
pub mod lexer;
pub mod string_interp;
pub mod token;

pub use error::*;
pub use lexer::tokenize;
pub use token::*;

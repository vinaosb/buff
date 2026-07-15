//! Deox lexer crate — token definitions and lexer implementation.
//!
//! This crate defines the token types ([`TokenKind`], [`Token`]) and error
//! types ([`LexerError`]) used by the Deox compiler. The actual lexing
//! implementation lives in the `lexer` module (T6).

pub mod error;
pub mod token;

pub use error::*;
pub use token::*;

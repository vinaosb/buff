//! Top-level parser entry point.
//!
//! [`parse`] consumes a slice of lexer-produced tokens and emits a list of
//! top-level declarations. Statement-level parsing (`let`, `if`, `func`)
//! arrives in T8 — for now only expression-level parsing is implemented,
//! via [`crate::expr::parse_expression`].
//!
//! [`parse`] is the public boundary that downstream code calls. It hides
//! [`TokenStream`] construction and any pre-filtering of layout tokens.

use deox_ast::Decl;
use deox_error::{ParseError, SourceId};

use crate::stream::TokenStream;

/// Parse a slice of tokens into zero or more top-level [`Decl`]s.
///
/// # T7 status
///
/// Only the expression sub-parser is wired up. Top-level declaration parsing
/// (`func`/`struct`/`enum`/`import`/`module`/`trait`) and statement parsing
/// (`let`/`return`/`if`-as-stmt/...) arrive in T8 and T9. For T7 this
/// function returns `Ok(vec![])` for any input that does not look like a
/// declaration keyword, so callers can integrate the parser end-to-end
/// without false errors.
///
/// # Errors
///
/// Returns [`ParseError`] only on unrecoverable internal failure. Unknown
/// top-level tokens are silently dropped in T7.
pub fn parse(_tokens: &[deox_lexer::Token], _source_id: SourceId) -> Result<Vec<Decl>, ParseError> {
    // T7 stub: statement/decl parsing is T8's responsibility.
    Ok(Vec::new())
}

/// Parse a single top-level expression from a token slice. Convenience
/// wrapper for T7 tests and embedding tools that want to evaluate an
/// expression without setting up a [`TokenStream`] themselves.
///
/// # Errors
///
/// Returns [`ParseError`] if the tokens do not form exactly one expression,
/// or if there are leftover tokens after the expression.
pub fn parse_expression(
    tokens: &[deox_lexer::Token],
    source_id: SourceId,
) -> Result<deox_ast::Expr, ParseError> {
    let mut stream = TokenStream::new(tokens, source_id);
    let expr = crate::expr::parse_expression(&mut stream)?;
    if !stream.is_at_end() {
        return Err(stream.unexpected("extra tokens after expression"));
    }
    Ok(expr)
}

//! String interpolation scanner.
//!
//! Implements a hand-rolled state machine that scans Buff string literals with
//! interpolation. A typical input looks like:
//!
//! ```text
//! "valor {x + 1} fim"
//! ```
//!
//! The output token sequence is:
//!
//! ```text
//! StringStart, StringPart("valor "), InterpStart, Ident("x"), Plus,
//! IntLit(1), InterpEnd, StringPart(" fim"), StringEnd
//! ```
//!
//! The scanner operates on the *entire* source string using absolute byte
//! positions so that spans refer back to the original input. When an
//! interpolation expression is encountered, the inner expression is tokenized
//! via a caller-supplied closure (`LexCallback`) so that nested string
//! literals (and recursively nested interpolations) are handled by the main
//! lexer.

use buff_lang_error::{SourceId, Span};

use crate::error::LexerError;
use crate::token::{Token, TokenKind};

/// A callback used by [`scan_string`] to tokenize an interpolation expression.
///
/// Implementations receive:
/// - the full source string
/// - the absolute byte position just after the opening `{`
/// - the absolute byte position of the matching `}`
///
/// They must push their tokens into `out` (with spans corrected for the
/// absolute positions) and return `Ok(())` on success.
pub trait LexCallback {
    fn lex_range(
        &mut self,
        source: &str,
        range_start: usize,
        range_end: usize,
        _source_id: SourceId,
        out: &mut Vec<Token>,
    ) -> Result<(), LexerError>;
}

/// Scan a string literal starting at `quote_start` (the position of the
/// opening `"`).
///
/// Returns the absolute byte position immediately *after* the closing `"`.
/// Pushes `StringStart`, `StringPart`, `InterpStart`, ... `StringEnd` tokens
/// into `out` along the way.
///
/// # Errors
///
/// - [`LexerError::unterminated_string`] if EOF is reached before the closing
///   quote.
/// - A generic [`LexerError`] for an unbalanced `}` or a stray escape at EOF.
pub fn scan_string(
    source: &str,
    quote_start: usize,
    source_id: SourceId,
    out: &mut Vec<Token>,
    interp_cb: &mut dyn LexCallback,
) -> Result<usize, LexerError> {
    out.push(Token::new(
        TokenKind::StringStart,
        Span::new(quote_start, quote_start + 1, source_id),
    ));

    let bytes = source.as_bytes();
    let mut i = quote_start + 1;
    let mut part_start = i;

    while i < bytes.len() {
        match bytes[i] {
            b'"' => {
                flush_part(source, part_start, i, source_id, out);
                out.push(Token::new(
                    TokenKind::StringEnd,
                    Span::new(i, i + 1, source_id),
                ));
                return Ok(i + 1);
            }
            b'\\' => {
                // We keep the raw escape bytes in the StringPart; escape
                // interpretation is the codegen/parser's job. Skip one extra
                // byte (the escaped char) if present.
                if i + 1 >= bytes.len() {
                    return Err(LexerError::unterminated_string(Span::new(
                        quote_start,
                        bytes.len(),
                        source_id,
                    )));
                }
                i += 2;
            }
            b'{' => {
                flush_part(source, part_start, i, source_id, out);
                out.push(Token::new(
                    TokenKind::InterpStart,
                    Span::new(i, i + 1, source_id),
                ));
                let after_brace = i + 1;
                let closing = find_matching_brace(source, after_brace, source_id)?;
                // T81: scan for `:` at brace-depth 0 to split expr from spec.
                let (expr_end, spec_text) =
                    split_interp_spec(source, after_brace, closing, source_id)?;
                interp_cb.lex_range(source, after_brace, expr_end, source_id, out)?;
                if let Some(spec) = spec_text {
                    out.push(Token::new(
                        TokenKind::InterpSpec(spec),
                        Span::new(expr_end, closing, source_id),
                    ));
                }
                out.push(Token::new(
                    TokenKind::InterpEnd,
                    Span::new(closing, closing + 1, source_id),
                ));
                i = closing + 1;
                part_start = i;
            }
            b'}' => {
                return Err(LexerError::new(
                    "unexpected '}' inside string literal",
                    Span::new(i, i + 1, source_id),
                ));
            }
            // Multi-byte UTF-8 sequences are handled implicitly: we walk one
            // byte at a time but never split a UTF-8 boundary because we only
            // react to ASCII bytes above.
            _ => {
                i += 1;
            }
        }
    }

    Err(LexerError::unterminated_string(Span::new(
        quote_start,
        bytes.len(),
        source_id,
    )))
}

fn flush_part(source: &str, start: usize, end: usize, source_id: SourceId, out: &mut Vec<Token>) {
    if end > start {
        let text = source[start..end].to_string();
        out.push(Token::new(
            TokenKind::StringPart(text),
            Span::new(start, end, source_id),
        ));
    }
}

/// Find the matching `}` for the `{` that precedes `start`.
///
/// `start` is the position immediately after the opening `{`. The returned
/// position is the index of the matching `}`.
///
/// Brace depth is tracked so nested `{ ... { ... } ... }` works. String
/// literals inside the interpolation are skipped so their `{`/`}` characters
/// do not affect brace depth.
fn find_matching_brace(
    source: &str,
    start: usize,
    source_id: SourceId,
) -> Result<usize, LexerError> {
    let bytes = source.as_bytes();
    let mut depth = 1usize;
    let mut i = start;
    while i < bytes.len() {
        match bytes[i] {
            b'{' => {
                depth += 1;
                i += 1;
            }
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    return Ok(i);
                }
                i += 1;
            }
            b'"' => {
                // Skip a nested string literal: walk to the next unescaped `"`.
                i += 1;
                while i < bytes.len() && bytes[i] != b'"' {
                    if bytes[i] == b'\\' && i + 1 < bytes.len() {
                        i += 2;
                    } else {
                        i += 1;
                    }
                }
                if i < bytes.len() {
                    i += 1; // consume closing `"`
                }
            }
            _ => {
                i += 1;
            }
        }
    }
    Err(LexerError::new(
        "unterminated interpolation in string literal",
        Span::new(start.saturating_sub(1), bytes.len(), source_id),
    ))
}

/// Split the inner text of `${...}` at the first `:` at brace-depth 0.
///
/// Returns `(expr_end, Some(spec_text))` when a `:` is found, or
/// `(closing, None)` when there is no specifier.
///
/// The `:` is NOT included in the spec text — `${x:.2}` yields
/// `(pos_of_colon, Some(".2"))`.
///
/// Nested braces `{ ... { ... } ... }` are tracked so `:` inside
/// nested braces does NOT count as a spec separator. String literals
/// inside the interpolation are also skipped so their `:` characters
/// are ignored.
fn split_interp_spec(
    source: &str,
    start: usize,
    closing: usize,
    _source_id: SourceId,
) -> Result<(usize, Option<String>), LexerError> {
    let bytes = source.as_bytes();
    let mut depth = 0usize;
    let mut i = start;
    while i < closing {
        match bytes[i] {
            b'{' => {
                depth += 1;
                i += 1;
            }
            b'}' => {
                if depth == 0 {
                    // Reached the closing `}` — no spec found.
                    return Ok((closing, None));
                }
                depth -= 1;
                i += 1;
            }
            b':' if depth == 0 => {
                // Found the spec separator at brace-depth 0.
                let spec = source[i + 1..closing].to_string();
                return Ok((i, Some(spec)));
            }
            b'"' => {
                // Skip a nested string literal so its `:` doesn't confuse us.
                i += 1;
                while i < closing && bytes[i] != b'"' {
                    if bytes[i] == b'\\' && i + 1 < closing {
                        i += 2;
                    } else {
                        i += 1;
                    }
                }
                if i < closing {
                    i += 1;
                }
            }
            _ => {
                i += 1;
            }
        }
    }
    // Reached `closing` without finding `:` at depth 0.
    Ok((closing, None))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A no-op callback that records the inner text of each interpolation.
    struct RecordInterp {
        captured: Vec<String>,
    }

    impl LexCallback for RecordInterp {
        fn lex_range(
            &mut self,
            source: &str,
            range_start: usize,
            range_end: usize,
            _source_id: SourceId,
            _out: &mut Vec<Token>,
        ) -> Result<(), LexerError> {
            self.captured
                .push(source[range_start..range_end].to_string());
            Ok(())
        }
    }

    fn lex_str(src: &str) -> (Vec<TokenKind>, Vec<String>) {
        let mut out = Vec::new();
        let mut cb = RecordInterp {
            captured: Vec::new(),
        };
        let _ = scan_string(src, 0, SourceId(0), &mut out, &mut cb);
        let kinds: Vec<TokenKind> = out.into_iter().map(|t| t.kind).collect();
        (kinds, cb.captured)
    }

    #[test]
    fn simple_string() {
        let (kinds, _) = lex_str("\"hello\"");
        assert_eq!(
            kinds,
            vec![
                TokenKind::StringStart,
                TokenKind::StringPart("hello".into()),
                TokenKind::StringEnd,
            ]
        );
    }

    #[test]
    fn empty_string() {
        let (kinds, _) = lex_str("\"\"");
        assert_eq!(kinds, vec![TokenKind::StringStart, TokenKind::StringEnd]);
    }

    #[test]
    fn interpolation_passes_inner_text() {
        let (_, interps) = lex_str("\"a {b + c} d\"");
        assert_eq!(interps, vec!["b + c".to_string()]);
    }

    #[test]
    fn unterminated_string_errors() {
        let mut out = Vec::new();
        let mut cb = RecordInterp {
            captured: Vec::new(),
        };
        let result = scan_string("\"abc", 0, SourceId(0), &mut out, &mut cb);
        assert!(result.is_err());
    }

    // T81: format specifier tests
    //
    // Extracts `(expr_text, spec)` pairs by scanning the token stream that
    // `scan_string` emits. Each interpolation produces zero or one
    // `InterpSpec` tokens between `InterpStart` and `InterpEnd`. The
    // expression text is captured by a simple callback that records the raw
    // range; the spec is read from the `InterpSpec` token.
    struct RecordExprOnly {
        captured: Vec<String>,
    }

    impl LexCallback for RecordExprOnly {
        fn lex_range(
            &mut self,
            source: &str,
            range_start: usize,
            range_end: usize,
            _source_id: SourceId,
            _out: &mut Vec<Token>,
        ) -> Result<(), LexerError> {
            self.captured
                .push(source[range_start..range_end].to_string());
            Ok(())
        }
    }

    fn lex_str_with_specs(src: &str) -> Vec<(String, Option<String>)> {
        let mut out = Vec::new();
        let mut cb = RecordExprOnly {
            captured: Vec::new(),
        };
        let _ = scan_string(src, 0, SourceId(0), &mut out, &mut cb);

        // Walk the token stream to find specs. Each interpolation block is
        // `InterpStart, <expr tokens>, [InterpSpec(spec)], InterpEnd`.
        // We track whether the CURRENT block has a spec.
        let mut specs: Vec<Option<String>> = Vec::new();
        let mut current_spec: Option<String> = None;
        let mut in_interp = false;
        for tok in &out {
            match &tok.kind {
                TokenKind::InterpStart => {
                    in_interp = true;
                    current_spec = None;
                }
                TokenKind::InterpSpec(s) => {
                    if in_interp {
                        current_spec = Some(s.clone());
                    }
                }
                TokenKind::InterpEnd => {
                    if in_interp {
                        specs.push(current_spec.take());
                        in_interp = false;
                    }
                }
                _ => {}
            }
        }

        cb.captured
            .into_iter()
            .zip(specs.into_iter())
            .map(|(expr, spec)| (expr, spec))
            .collect()
    }

    #[test]
    fn interp_without_spec_still_works() {
        let interps = lex_str_with_specs("\"{x}\"");
        assert_eq!(interps, vec![("x".to_string(), None)]);
    }

    #[test]
    fn interp_with_decimal_spec() {
        let interps = lex_str_with_specs("\"pi = {pi:.2}\"");
        assert_eq!(interps, vec![("pi".to_string(), Some(".2".to_string()))]);
    }

    #[test]
    fn interp_with_debug_spec() {
        let interps = lex_str_with_specs("\"{obj:?}\"");
        assert_eq!(interps, vec![("obj".to_string(), Some("?".to_string()))]);
    }

    #[test]
    fn interp_with_pad_spec() {
        let interps = lex_str_with_specs("\"{n:>10}\"");
        assert_eq!(interps, vec![("n".to_string(), Some(">10".to_string()))]);
    }

    #[test]
    fn interp_with_hex_spec() {
        let interps = lex_str_with_specs("\"{val:x}\"");
        assert_eq!(interps, vec![("val".to_string(), Some("x".to_string()))]);
    }

    #[test]
    fn interp_with_binary_spec() {
        let interps = lex_str_with_specs("\"{val:b}\"");
        assert_eq!(interps, vec![("val".to_string(), Some("b".to_string()))]);
    }

    #[test]
    fn interp_with_zero_pad_spec() {
        let interps = lex_str_with_specs("\"{val:05}\"");
        assert_eq!(interps, vec![("val".to_string(), Some("05".to_string()))]);
    }

    #[test]
    fn interp_spec_ignores_colon_in_nested_braces() {
        let interps = lex_str_with_specs("\"{f({a: 1})}\"");
        assert_eq!(interps, vec![("f({a: 1})".to_string(), None)]);
    }

    #[test]
    fn interp_spec_with_expression() {
        let interps = lex_str_with_specs("\"{x + y:.2}\"");
        assert_eq!(interps, vec![("x + y".to_string(), Some(".2".to_string()))]);
    }

    #[test]
    fn interp_spec_with_scientific() {
        let interps = lex_str_with_specs("\"{val:e}\"");
        assert_eq!(interps, vec![("val".to_string(), Some("e".to_string()))]);
    }

    #[test]
    fn interp_spec_with_octal() {
        let interps = lex_str_with_specs("\"{val:o}\"");
        assert_eq!(interps, vec![("val".to_string(), Some("o".to_string()))]);
    }

    #[test]
    fn interp_spec_with_left_pad() {
        let interps = lex_str_with_specs("\"{val:<10}\"");
        assert_eq!(interps, vec![("val".to_string(), Some("<10".to_string()))]);
    }

    #[test]
    fn interp_spec_with_center_pad() {
        let interps = lex_str_with_specs("\"{val:^10}\"");
        assert_eq!(interps, vec![("val".to_string(), Some("^10".to_string()))]);
    }

    #[test]
    fn interp_spec_multiple_interpolations() {
        let interps = lex_str_with_specs("\"{a:.2} and {b:?}\"");
        assert_eq!(
            interps,
            vec![
                ("a".to_string(), Some(".2".to_string())),
                ("b".to_string(), Some("?".to_string())),
            ]
        );
    }
}

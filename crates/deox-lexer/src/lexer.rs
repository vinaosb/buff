//! Main lexer entry point.
//!
//! [`tokenize`] converts a Deox source string into a `Vec<Token>` using a
//! hand-rolled byte-scanner. Single-pass design with full source access for
//! accurate span and indentation tracking.
//!
//! Features:
//! - 25 keywords + identifiers
//! - Integer, float (`3.14`), double (`3.14d`), byte (`0xFF`, `0b1010`) literals
//! - Single- and multi-char operators
//! - `//` line comments and `/* /* nested */ */` block comments
//! - Indentation tracking via [`IndentationTracker`](crate::indent::IndentationTracker)
//! - String interpolation via [`scan_string`](crate::string_interp::scan_string)
//! - `\r\n` / `\r` newline normalization (single [`TokenKind::Newline`])
//! - UTF-8 inside identifiers and string bodies
//! - All fallible paths return [`LexerError`]; no panics.

use deox_error::{SourceId, Span};

use crate::error::LexerError;
use crate::indent::IndentationTracker;
use crate::string_interp::{scan_string, LexCallback};
use crate::token::{Token, TokenKind};

/// Tokenize a Deox source string into a vector of tokens.
///
/// The output always ends with [`TokenKind::Eof`]. Synthetic
/// [`TokenKind::Indent`] / [`TokenKind::Dedent`] tokens are emitted based on
/// leading-whitespace changes between non-blank lines.
///
/// # Errors
///
/// Returns [`LexerError`] for any of:
/// - mixed tabs/spaces in indentation
/// - unterminated string, interpolation, or block comment
/// - invalid numeric literal (overflow / parse failure)
/// - unexpected character
pub fn tokenize(source: &str, source_id: SourceId) -> Result<Vec<Token>, LexerError> {
    let mut tokens: Vec<Token> = Vec::new();
    let mut indent_tracker = IndentationTracker::new();

    lex_range(
        source,
        0,
        source.len(),
        source_id,
        &mut tokens,
        &mut indent_tracker,
        /* track_indent = */ true,
    )?;

    let eof_pos = source.len();
    for kind in indent_tracker.finalize() {
        tokens.push(Token::new(kind, Span::new(eof_pos, eof_pos, source_id)));
    }
    tokens.push(Token::new(
        TokenKind::Eof,
        Span::new(eof_pos, eof_pos, source_id),
    ));
    Ok(tokens)
}

/// Lex `source[start..end]`, appending tokens to `out`.
///
/// When called for an interpolation expression (`track_indent = false`),
/// newlines are skipped and no Indent/Dedent tokens are emitted.
fn lex_range(
    source: &str,
    start: usize,
    end: usize,
    source_id: SourceId,
    out: &mut Vec<Token>,
    indent_tracker: &mut IndentationTracker,
    track_indent: bool,
) -> Result<(), LexerError> {
    let bytes = source.as_bytes();
    let mut pos = start;

    // Per-line state. We need THREE distinct flags because comments and
    // tokens affect them differently:
    // - `seen_token_on_line`: has any real token been emitted on this line?
    //   Controls whether a terminating newline is emitted.
    // - `line_lead_ended`: has the leading-whitespace phase ended (via either
    //   a comment or a token)? Controls whether subsequent whitespace is
    //   captured as indent or dropped as intra-line whitespace.
    // - `indent_checked_this_line`: has the indent check already fired for
    //   this line? Prevents double-emission.
    let mut seen_token_on_line = false;
    let mut line_lead_ended = false;
    let mut indent_checked_this_line = false;
    let mut pending_indent: Option<(usize, usize)> = None;

    while pos < end {
        let tok_start = pos;
        let c = bytes[pos];

        // Newline: \r\n | \n | \r → single Newline token (CRLF normalized).
        if c == b'\n' || c == b'\r' {
            pos += 1;
            if c == b'\r' && pos < end && bytes[pos] == b'\n' {
                pos += 1;
            }
            if track_indent && seen_token_on_line {
                out.push(Token::new(
                    TokenKind::Newline,
                    Span::new(tok_start, pos, source_id),
                ));
            }
            seen_token_on_line = false;
            line_lead_ended = false;
            indent_checked_this_line = false;
            pending_indent = None;
            continue;
        }

        // Whitespace. Captured as indent only if we're still in the leading
        // phase (no comment or token yet on this line); otherwise dropped.
        if c == b' ' || c == b'\t' {
            let ws_start = pos;
            while pos < end && (bytes[pos] == b' ' || bytes[pos] == b'\t') {
                pos += 1;
            }
            if track_indent && !line_lead_ended {
                pending_indent = Some((ws_start, pos));
            }
            continue;
        }

        // Line comment `//...` runs to end-of-line.
        if c == b'/' && pos + 1 < end && bytes[pos + 1] == b'/' {
            pos += 2;
            while pos < end && bytes[pos] != b'\n' && bytes[pos] != b'\r' {
                pos += 1;
            }
            // A comment ends the leading-whitespace phase for this line, but
            // does NOT count as a token (so no Newline is emitted for a
            // comment-only line).
            line_lead_ended = true;
            continue;
        }

        // Block comment `/* ... */` with nesting.
        if c == b'/' && pos + 1 < end && bytes[pos + 1] == b'*' {
            let comment_start = pos;
            pos += 2;
            let mut depth = 1usize;
            while pos < end && depth > 0 {
                if bytes[pos] == b'/' && pos + 1 < end && bytes[pos + 1] == b'*' {
                    depth += 1;
                    pos += 2;
                } else if bytes[pos] == b'*' && pos + 1 < end && bytes[pos + 1] == b'/' {
                    depth -= 1;
                    pos += 2;
                } else {
                    pos += 1;
                }
            }
            if depth > 0 {
                return Err(LexerError::new(
                    "unterminated block comment",
                    Span::new(comment_start, pos, source_id),
                ));
            }
            line_lead_ended = true;
            continue;
        }

        // About to emit a real token. If this is the first token on the
        // line, fire the indent check using the captured leading whitespace
        // (or empty string if none).
        if track_indent && !indent_checked_this_line {
            let (ws_start, ws_end) = pending_indent.take().unwrap_or((tok_start, tok_start));
            let indent_str = &source[ws_start..ws_end];
            let kinds = indent_tracker.check_line(indent_str, source_id, ws_start)?;
            for k in kinds {
                out.push(Token::new(k, Span::new(ws_start, ws_end, source_id)));
            }
            indent_checked_this_line = true;
        }
        seen_token_on_line = true;
        line_lead_ended = true;

        // String literal (with interpolation).
        if c == b'"' {
            let quote_start = pos;
            let mut interp_cb = InterpLexer { source, source_id };
            pos = scan_string(source, quote_start, source_id, out, &mut interp_cb)?;
            continue;
        }

        // Identifiers / keywords.
        if c.is_ascii_alphabetic() || c == b'_' {
            let id_start = pos;
            pos += 1;
            while pos < end && (bytes[pos].is_ascii_alphanumeric() || bytes[pos] == b'_') {
                pos += 1;
            }
            let s = &source[id_start..pos];
            let kind = match TokenKind::from_keyword(s) {
                Some(k) => k,
                None => TokenKind::Ident(s.to_string()),
            };
            out.push(Token::new(kind, Span::new(id_start, pos, source_id)));
            continue;
        }

        // Numbers.
        if c.is_ascii_digit() {
            let num_start = pos;
            let kind = scan_number(source, bytes, &mut pos, num_start, end, source_id)?;
            out.push(Token::new(kind, Span::new(num_start, pos, source_id)));
            continue;
        }

        // Multi-char operators.
        if let Some(kind) = scan_operator(source, &mut pos, tok_start, end) {
            out.push(Token::new(kind, Span::new(tok_start, pos, source_id)));
            continue;
        }

        // Single-char delimiters / operators.
        if let Some(kind) = single_char_kind(c) {
            pos += 1;
            out.push(Token::new(kind, Span::new(tok_start, pos, source_id)));
            continue;
        }

        return Err(LexerError::unexpected_char(
            c as char,
            Span::new(tok_start, tok_start + 1, source_id),
        ));
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Number scanning.
// ---------------------------------------------------------------------------

fn scan_number(
    source: &str,
    bytes: &[u8],
    pos: &mut usize,
    start: usize,
    end: usize,
    source_id: SourceId,
) -> Result<TokenKind, LexerError> {
    let span = |a, b| Span::new(a, b, source_id);

    // Hex / binary prefixes.
    if bytes[start] == b'0' && start + 1 < end {
        match bytes[start + 1] {
            b'x' | b'X' => {
                *pos = start + 2;
                while *pos < end && bytes[*pos].is_ascii_hexdigit() {
                    *pos += 1;
                }
                let s = &source[start + 2..*pos];
                let v = u32::from_str_radix(s, 16)
                    .map_err(|_| LexerError::invalid_number(span(start, *pos)))?;
                return byte_lit(v, span(start, *pos));
            }
            b'b' | b'B' => {
                *pos = start + 2;
                while *pos < end && (bytes[*pos] == b'0' || bytes[*pos] == b'1') {
                    *pos += 1;
                }
                let s = &source[start + 2..*pos];
                let v = u32::from_str_radix(s, 2)
                    .map_err(|_| LexerError::invalid_number(span(start, *pos)))?;
                return byte_lit(v, span(start, *pos));
            }
            _ => {}
        }
    }

    // Integer part.
    while *pos < end && bytes[*pos].is_ascii_digit() {
        *pos += 1;
    }

    // Decimal point + fractional part?
    if *pos + 1 < end && bytes[*pos] == b'.' && bytes[*pos + 1].is_ascii_digit() {
        *pos += 1; // dot
        while *pos < end && bytes[*pos].is_ascii_digit() {
            *pos += 1;
        }
        if *pos < end && (bytes[*pos] == b'd' || bytes[*pos] == b'D') {
            *pos += 1;
            let s = &source[start..*pos];
            let trimmed = &s[..s.len() - 1];
            let v: f64 = trimmed
                .parse()
                .map_err(|_| LexerError::invalid_number(span(start, *pos)))?;
            return Ok(TokenKind::DoubleLit(v));
        }
        if *pos < end && (bytes[*pos] == b'm' || bytes[*pos] == b'M') {
            *pos += 1;
            return Err(LexerError::new(
                "decimal 'm' suffix is not supported",
                span(start, *pos),
            ));
        }
        let s = &source[start..*pos];
        let v: f32 = s
            .parse()
            .map_err(|_| LexerError::invalid_number(span(start, *pos)))?;
        return Ok(TokenKind::FloatLit(v));
    }

    let s = &source[start..*pos];
    let v: i64 = s
        .parse()
        .map_err(|_| LexerError::invalid_number(span(start, *pos)))?;
    Ok(TokenKind::IntLit(v))
}

fn byte_lit(value: u32, span: Span) -> Result<TokenKind, LexerError> {
    if value > u32::from(u8::MAX) {
        return Err(LexerError::invalid_number(span));
    }
    Ok(TokenKind::ByteLit(value as u8))
}

// ---------------------------------------------------------------------------
// Operator scanning.
// ---------------------------------------------------------------------------

fn scan_operator(source: &str, pos: &mut usize, start: usize, end: usize) -> Option<TokenKind> {
    if start + 2 <= end {
        let two = &source[start..start + 2];
        let kind = match two {
            "==" => Some(TokenKind::EqEq),
            "!=" => Some(TokenKind::NotEq),
            "<=" => Some(TokenKind::LtEq),
            ">=" => Some(TokenKind::GtEq),
            "&&" => Some(TokenKind::AndAnd),
            "||" => Some(TokenKind::OrOr),
            "<<" => Some(TokenKind::Shl),
            ">>" => Some(TokenKind::Shr),
            "->" => Some(TokenKind::Arrow),
            "=>" => Some(TokenKind::FatArrow),
            "+=" => Some(TokenKind::PlusEq),
            "-=" => Some(TokenKind::MinusEq),
            "*=" => Some(TokenKind::StarEq),
            "/=" => Some(TokenKind::SlashEq),
            "%=" => Some(TokenKind::PercentEq),
            _ => None,
        };
        if let Some(k) = kind {
            *pos = start + 2;
            return Some(k);
        }
    }
    None
}

fn single_char_kind(c: u8) -> Option<TokenKind> {
    Some(match c {
        b'+' => TokenKind::Plus,
        b'-' => TokenKind::Minus,
        b'*' => TokenKind::Star,
        b'/' => TokenKind::Slash,
        b'%' => TokenKind::Percent,
        b'<' => TokenKind::Lt,
        b'>' => TokenKind::Gt,
        b'!' => TokenKind::Not,
        b'?' => TokenKind::Question,
        b'^' => TokenKind::Caret,
        b'|' => TokenKind::Pipe,
        b'&' => TokenKind::Amp,
        b'~' => TokenKind::Tilde,
        b'=' => TokenKind::Assign,
        b'(' => TokenKind::LParen,
        b')' => TokenKind::RParen,
        b'{' => TokenKind::LBrace,
        b'}' => TokenKind::RBrace,
        b'[' => TokenKind::LBracket,
        b']' => TokenKind::RBracket,
        b':' => TokenKind::Colon,
        b',' => TokenKind::Comma,
        b'.' => TokenKind::Dot,
        b';' => TokenKind::Semicolon,
        b'@' => TokenKind::At,
        _ => return None,
    })
}

// ---------------------------------------------------------------------------
// Callback used by scan_string to lex interpolation expressions.
//
// At the time `lex_range` is invoked, scan_string has already pushed
// InterpStart into `out` and will push InterpEnd after we return. So tokens
// pushed to `out` here land in the correct slot automatically.
// ---------------------------------------------------------------------------

struct InterpLexer<'a> {
    source: &'a str,
    source_id: SourceId,
}

impl<'a> LexCallback for InterpLexer<'a> {
    fn lex_range(
        &mut self,
        _source: &str,
        range_start: usize,
        range_end: usize,
        _source_id: SourceId,
        out: &mut Vec<Token>,
    ) -> Result<(), LexerError> {
        let mut dummy = IndentationTracker::new();
        lex_range(
            self.source,
            range_start,
            range_end,
            self.source_id,
            out,
            &mut dummy,
            /* track_indent = */ false,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kinds(src: &str) -> Result<Vec<TokenKind>, LexerError> {
        Ok(tokenize(src, SourceId(0))?
            .into_iter()
            .map(|t| t.kind)
            .collect())
    }

    #[test]
    fn empty_input_is_eof_only() {
        assert_eq!(kinds("").unwrap(), vec![TokenKind::Eof]);
    }

    #[test]
    fn simple_identifier() {
        assert_eq!(
            kinds("foo").unwrap(),
            vec![TokenKind::Ident("foo".into()), TokenKind::Eof]
        );
    }

    #[test]
    fn integer_literal() {
        assert_eq!(
            kinds("42").unwrap(),
            vec![TokenKind::IntLit(42), TokenKind::Eof]
        );
    }

    #[test]
    fn byte_hex_and_binary() {
        assert_eq!(
            kinds("0xFF 0b1010").unwrap(),
            vec![
                TokenKind::ByteLit(255),
                TokenKind::ByteLit(10),
                TokenKind::Eof,
            ]
        );
    }

    #[test]
    fn operators_two_char() {
        let tokens = kinds("== != -> => +=").unwrap();
        assert_eq!(
            tokens,
            vec![
                TokenKind::EqEq,
                TokenKind::NotEq,
                TokenKind::Arrow,
                TokenKind::FatArrow,
                TokenKind::PlusEq,
                TokenKind::Eof,
            ]
        );
    }

    #[test]
    fn unterminated_string_errors() {
        assert!(kinds("\"hello").is_err());
    }

    #[test]
    fn nested_block_comment() {
        let tokens = kinds("/* a /* b */ c */ x").unwrap();
        assert_eq!(tokens, vec![TokenKind::Ident("x".into()), TokenKind::Eof]);
    }

    #[test]
    fn string_with_interp() {
        let tokens = kinds("\"a {x + 1} b\"").unwrap();
        assert_eq!(
            tokens,
            vec![
                TokenKind::StringStart,
                TokenKind::StringPart("a ".into()),
                TokenKind::InterpStart,
                TokenKind::Ident("x".into()),
                TokenKind::Plus,
                TokenKind::IntLit(1),
                TokenKind::InterpEnd,
                TokenKind::StringPart(" b".into()),
                TokenKind::StringEnd,
                TokenKind::Eof,
            ]
        );
    }
}

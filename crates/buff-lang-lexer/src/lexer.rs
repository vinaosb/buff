//! Main lexer entry point.
//!
//! [`tokenize`] converts a Buff source string into a `Vec<Token>` using a
//! hand-rolled byte-scanner. Single-pass design with full source access for
//! accurate span and indentation tracking.
//!
//! Features:
//! - 25 keywords + identifiers
//! - Integer, float (`3.14`), double (`3.14d`), byte (`0xFF`, `0b1010`),
//!   decimal (`99.90m`) literals
//! - Single- and multi-char operators
//! - `//` line comments and `/* /* nested */ */` block comments
//! - Indentation tracking via [`IndentationTracker`](crate::indent::IndentationTracker)
//! - String interpolation via [`scan_string`](crate::string_interp::scan_string)
//! - `\r\n` / `\r` newline normalization (single [`TokenKind::Newline`])
//! - UTF-8 inside identifiers and string bodies
//! - All fallible paths return [`LexerError`]; no panics.

use buff_lang_error::{SourceId, Span};

use crate::error::LexerError;
use crate::indent::IndentationTracker;
use crate::string_interp::{scan_string, LexCallback};
use crate::token::{Token, TokenKind};

/// Tokenize a Buff source string into a vector of tokens.
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

        // String literal (with interpolation) and triple-quoted raw string.
        // Triple-quote `"""..."""` MUST be checked before single `"` so the
        // first quote isn't swallowed by scan_string.
        if c == b'"' {
            if pos + 2 < end && bytes[pos + 1] == b'"' && bytes[pos + 2] == b'"' {
                pos = scan_triple_string(source, pos, source_id, out)?;
                continue;
            }
            let quote_start = pos;
            let mut interp_cb = InterpLexer { source, source_id };
            pos = scan_string(source, quote_start, source_id, out, &mut interp_cb)?;
            continue;
        }

        // Char literal `'x'` — a single Unicode scalar value. Disambiguates
        // from the ASCII apostrophe by requiring a closing `'` after one
        // scalar value (with optional escape).
        if c == b'\'' {
            pos = scan_char(source, pos, source_id, out)?;
            continue;
        }

        // T104: Raw string literal `r"..."` — no escape processing, no
        // interpolation. Check BEFORE the identifier branch so `r"` is
        // consumed as a raw string, not as identifier `r` followed by `"`.
        if c == b'r' && pos + 1 < end && bytes[pos + 1] == b'"' {
            pos = scan_raw_string(source, pos, source_id, out)?;
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
// T104: Raw string literal `r"..."` scanning.
// ---------------------------------------------------------------------------

/// Scan a raw string literal `r"..."` (T104).
///
/// Raw strings have NO escape processing and NO interpolation — every byte
/// between the opening `"` and the next `"` is captured verbatim. This lets
/// backslashes, braces, and other special characters appear literally.
///
/// Returns the absolute byte position immediately after the closing `"`.
/// Pushes `StringStart`, `StringPart(text)`, `StringEnd` into `out` — the
/// same token sequence as a plain (non-interpolated) string, so the parser
/// and codegen handle it without changes.
///
/// # Limitations (v0.5)
///
/// - A raw string cannot contain a literal `"` character (no `r#"..."#`
///   hash-delimited form). This is deferred to a future version.
/// - Multi-line raw strings use the existing `"""..."""` triple-quote form
///   (T21), not `r"..."`.
fn scan_raw_string(
    source: &str,
    r_pos: usize,
    source_id: SourceId,
    out: &mut Vec<Token>,
) -> Result<usize, LexerError> {
    let bytes = source.as_bytes();
    let span = |a, b| Span::new(a, b, source_id);

    // `r_pos` is at `r`, the opening `"` is at `r_pos + 1`.
    let quote_start = r_pos + 1;

    out.push(Token::new(
        TokenKind::StringStart,
        span(r_pos, quote_start + 1),
    ));

    // Body starts right after the opening `"`. Scan verbatim until the next
    // `"` — no escape processing, no interpolation.
    let body_start = quote_start + 1;
    let mut i = body_start;
    while i < bytes.len() {
        if bytes[i] == b'"' {
            let text = source[body_start..i].to_string();
            if !text.is_empty() {
                out.push(Token::new(TokenKind::StringPart(text), span(body_start, i)));
            }
            out.push(Token::new(TokenKind::StringEnd, span(i, i + 1)));
            return Ok(i + 1);
        }
        i += 1;
    }

    Err(LexerError::unterminated_string(span(r_pos, bytes.len())))
}

// ---------------------------------------------------------------------------
// Char + triple-quote string scanning (T21).
// ---------------------------------------------------------------------------

/// Scan a Char literal starting at `quote_start` (the position of `'`).
///
/// Supports the forms:
/// - `'A'` — a single ASCII byte
/// - `'é'`, `'🚀'` — a single multi-byte Unicode scalar value
/// - `'\n'`, `'\t'`, `'\\'`, `'\''`, `'\"'` — common escapes
/// - `'\u{1F680}'` — Unicode escape (any scalar value)
///
/// Returns the absolute byte position immediately *after* the closing `'`.
/// Pushes a single [`TokenKind::CharLit`] token into `out`.
///
/// # Errors
///
/// Returns [`LexerError`] if: the literal is empty (`''`), is missing the
/// closing `'`, contains more than one scalar value (e.g. `'ab'`), or holds
/// an invalid escape sequence.
fn scan_char(
    source: &str,
    quote_start: usize,
    source_id: SourceId,
    out: &mut Vec<Token>,
) -> Result<usize, LexerError> {
    let bytes = source.as_bytes();
    let span = |a, b| Span::new(a, b, source_id);

    // We are sitting just past the opening `'`.
    let i = quote_start + 1;
    if i >= bytes.len() {
        return Err(LexerError::new(
            "unterminated char literal",
            span(quote_start, bytes.len()),
        ));
    }

    // Empty literal `''` is invalid.
    if bytes[i] == b'\'' {
        return Err(LexerError::new(
            "empty char literal",
            span(quote_start, i + 1),
        ));
    }

    // Compute (scalar, end) where `end` is the position just AFTER the
    // closing `'`. The two paths differ in how they locate the closing quote.
    let (scalar, end) = if bytes[i] == b'\\' {
        // Escape: interpret the escape (1-char or `\u{...}`) then require a
        // closing `'`. We can't naively walk-to-next-`'` because the escape
        // body itself may contain a `'` (e.g. `'\''`).
        let esc_start = i;
        let after_backslash = i + 1;
        if after_backslash >= bytes.len() {
            return Err(LexerError::new(
                "unterminated char literal escape",
                span(esc_start, bytes.len()),
            ));
        }
        // Determine how many bytes the escape body occupies (excluding `\`).
        let body_end = match bytes[after_backslash] {
            b'u' => {
                // Expect `u{XXXX...}` — scan until matching `}`.
                let mut j = after_backslash + 1;
                if j >= bytes.len() || bytes[j] != b'{' {
                    return Err(LexerError::new(
                        "Unicode escape must use the form `\\u{XXXX}`",
                        span(esc_start, j.min(bytes.len())),
                    ));
                }
                j += 1; // past `{`
                while j < bytes.len() && bytes[j] != b'}' {
                    j += 1;
                }
                if j >= bytes.len() {
                    return Err(LexerError::new(
                        "unterminated Unicode escape `\\u{...}`",
                        span(esc_start, bytes.len()),
                    ));
                }
                j + 1 // one past `}`
            }
            // Single-char escape (`n`, `r`, `t`, `\\`, `'`, `"`, `0`).
            _ => after_backslash + 1,
        };
        let body = &source[esc_start + 1..body_end];
        let c = parse_char_escape(body, span(esc_start, body_end))?;
        // Now expect the closing `'` at body_end.
        if body_end >= bytes.len() || bytes[body_end] != b'\'' {
            return Err(LexerError::new(
                "char literal must contain exactly one Unicode scalar value",
                span(quote_start, body_end.min(bytes.len())),
            ));
        }
        (c, body_end + 1)
    } else {
        // Single scalar value (1..=4 bytes UTF-8).
        let rest = &source[i..];
        let mut chars = rest.chars();
        let c = chars.next().ok_or_else(|| {
            LexerError::new("unterminated char literal", span(quote_start, bytes.len()))
        })?;
        let consumed = c.len_utf8();
        let after_scalar = i + consumed;
        // Must be immediately followed by the closing `'`.
        if after_scalar >= bytes.len() || bytes[after_scalar] != b'\'' {
            return Err(LexerError::new(
                "char literal must contain exactly one Unicode scalar value",
                span(quote_start, after_scalar.min(bytes.len())),
            ));
        }
        (c, after_scalar + 1)
    };

    out.push(Token::new(
        TokenKind::CharLit(scalar),
        span(quote_start, end),
    ));
    Ok(end)
}

/// Parse a Char-escape body (the text between `\` and the closing `'`).
///
/// Recognises: `n`, `r`, `t`, `\\`, `'`, `"`, `0`, and `u{XXXX..}`.
fn parse_char_escape(body: &str, span: Span) -> Result<char, LexerError> {
    let mut chars = body.chars();
    let first = chars
        .next()
        .ok_or_else(|| LexerError::new("empty escape sequence in char literal", span))?;
    let c = match first {
        'n' => '\n',
        'r' => '\r',
        't' => '\t',
        '\\' => '\\',
        '\'' => '\'',
        '"' => '"',
        '0' => '\0',
        'u' => {
            // `\u{1F680}` form. The remainder must be `{...}`.
            let rest: String = chars.collect();
            let trimmed = rest.trim_start_matches('{').trim_end_matches('}');
            let code = u32::from_str_radix(trimmed, 16).map_err(|_| {
                LexerError::new(format!("invalid Unicode escape: \\u{{{}}}", trimmed), span)
            })?;
            return char::from_u32(code).ok_or_else(|| {
                LexerError::new(
                    format!("\\u{{{}}} is not a valid Unicode scalar value", trimmed),
                    span,
                )
            });
        }
        other => {
            return Err(LexerError::new(
                format!("unknown char escape: \\{}", other),
                span,
            ));
        }
    };
    // The recognised single-char escapes must be alone in the body (no
    // trailing chars like `\nb`).
    if chars.next().is_some() {
        return Err(LexerError::new(
            "char literal escape has extra characters",
            span,
        ));
    }
    Ok(c)
}

/// Scan a triple-quoted raw string `"""..."""` (T21).
///
/// Triple-quoted strings are RAW: no escape processing and no interpolation
/// — every byte between the opening `"""` and the next `"""` is captured
/// verbatim. This lets multi-line strings (newlines, leading whitespace,
/// embedded quotes) be expressed without escaping.
///
/// Returns the absolute byte position immediately after the closing `"""`.
/// Pushes `StringStart`, `StringPart(text)`, `StringEnd` into `out` so the
/// downstream parser treats it like a simple (non-interpolated) string.
fn scan_triple_string(
    source: &str,
    quote_start: usize,
    source_id: SourceId,
    out: &mut Vec<Token>,
) -> Result<usize, LexerError> {
    let bytes = source.as_bytes();
    let span = |a, b| Span::new(a, b, source_id);

    out.push(Token::new(
        TokenKind::StringStart,
        span(quote_start, quote_start + 3),
    ));

    // Body starts right after `"""`. Find the next `"""` (allowing `""` inside
    // — we only close on three consecutive quotes).
    let body_start = quote_start + 3;
    let mut i = body_start;
    while i + 2 < bytes.len() {
        if bytes[i] == b'"' && bytes[i + 1] == b'"' && bytes[i + 2] == b'"' {
            // Body is body_start..i.
            let text = source[body_start..i].to_string();
            if !text.is_empty() {
                out.push(Token::new(TokenKind::StringPart(text), span(body_start, i)));
            }
            out.push(Token::new(TokenKind::StringEnd, span(i, i + 3)));
            return Ok(i + 3);
        }
        i += 1;
    }
    Err(LexerError::unterminated_string(span(
        quote_start,
        bytes.len(),
    )))
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
            // T20: 128-bit decimal literal (e.g. `99.90m`). Carry the RAW
            // digit text (no suffix) so the value never rounds through f64 —
            // exactness is preserved all the way to `dec!()` codegen.
            *pos += 1;
            let raw = &source[start..*pos - 1];
            return Ok(TokenKind::DecimalLit(raw.to_string()));
        }
        let s = &source[start..*pos];
        let v: f32 = s
            .parse()
            .map_err(|_| LexerError::invalid_number(span(start, *pos)))?;
        return Ok(TokenKind::FloatLit(v));
    }

    // Integer-only with optional `m`/`M` suffix → Decimal (T20).
    // e.g. `100m` → DecimalLit("100"). Mirrors how the `m` suffix works in
    // the fractional branch; keeps integers-with-suffix exact too.
    if *pos < end && (bytes[*pos] == b'm' || bytes[*pos] == b'M') {
        *pos += 1;
        let raw = &source[start..*pos - 1];
        return Ok(TokenKind::DecimalLit(raw.to_string()));
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
    // 3-char operators first (so `..=` is not split into `..` + `=`).
    if start + 3 <= end {
        let three = &source[start..start + 3];
        let kind = match three {
            "..=" => Some(TokenKind::DotDotEq),
            _ => None,
        };
        if let Some(k) = kind {
            *pos = start + 3;
            return Some(k);
        }
    }
    if start + 2 <= end {
        let two = &source[start..start + 2];
        let kind = match two {
            ".." => Some(TokenKind::DotDot),
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
            "??" => Some(TokenKind::QuestionQuestion),
            // T70: null-conditional operator `?.`. MUST appear in the 2-char
            // section (which runs BEFORE single_char_kind) so `?.` is matched
            // greedily instead of splitting into `?` (Question) + `.` (Dot).
            // A lone `?` (NOT followed by `.`) still falls through to
            // `single_char_kind(b'?') => TokenKind::Question` (the T30 Try
            // postfix). `??` (QuestionQuestion, T101) is matched earlier in
            // this same 2-char section, so `??` is unaffected.
            "?." => Some(TokenKind::QuestionDot),
            // T69: pipeline operator `|>`. MUST appear in the 2-char section
            // (which runs BEFORE single_char_kind) so `|>` is matched
            // greedily instead of splitting into `|` (Pipe) + `>` (Gt).
            "|>" => Some(TokenKind::PipeGt),
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

    // T21: Char literal lexing.
    mod char_literals {
        use super::*;

        #[test]
        fn ascii_char() {
            // `'A'` → CharLit('A')
            let tokens = kinds("'A'").unwrap();
            assert_eq!(tokens, vec![TokenKind::CharLit('A'), TokenKind::Eof]);
        }

        #[test]
        fn multibyte_latin_char() {
            // `'é'` is one Unicode scalar value even though it's 2 UTF-8 bytes.
            let tokens = kinds("'é'").unwrap();
            assert_eq!(tokens, vec![TokenKind::CharLit('é'), TokenKind::Eof]);
        }

        #[test]
        fn emoji_four_byte_char() {
            // `'🚀'` is a single scalar (U+1F680), 4 UTF-8 bytes.
            let tokens = kinds("'🚀'").unwrap();
            assert_eq!(tokens, vec![TokenKind::CharLit('🚀'), TokenKind::Eof]);
        }

        #[test]
        fn escape_newline() {
            let tokens = kinds("'\\n'").unwrap();
            assert_eq!(tokens, vec![TokenKind::CharLit('\n'), TokenKind::Eof]);
        }

        #[test]
        fn escape_quote() {
            // Escaped single quote inside a char literal.
            let tokens = kinds("'\\''").unwrap();
            assert_eq!(tokens, vec![TokenKind::CharLit('\''), TokenKind::Eof]);
        }

        #[test]
        fn escape_unicode_braces() {
            let tokens = kinds("'\\u{1F680}'").unwrap();
            assert_eq!(tokens, vec![TokenKind::CharLit('🚀'), TokenKind::Eof]);
        }

        #[test]
        fn empty_char_errors() {
            assert!(kinds("''").is_err());
        }

        #[test]
        fn two_scalar_values_errors() {
            // `'ab'` is NOT a valid char literal.
            assert!(kinds("'ab'").is_err());
        }

        #[test]
        fn unterminated_char_errors() {
            assert!(kinds("'a").is_err());
        }

        #[test]
        fn char_display_format() {
            assert_eq!(TokenKind::CharLit('A').to_string(), "char('A')");
        }

        #[test]
        fn char_in_expression_parses() {
            // `let c = 'x'` lexes cleanly — the `'` between `=` and `x` is
            // the start of a char literal, not an apostrophe.
            let tokens = kinds("let c = 'x'").unwrap();
            assert_eq!(
                tokens,
                vec![
                    TokenKind::KwLet,
                    TokenKind::Ident("c".into()),
                    TokenKind::Assign,
                    TokenKind::CharLit('x'),
                    TokenKind::Eof,
                ]
            );
        }
    }

    // T21: Triple-quoted raw strings.
    mod triple_strings {
        use super::*;

        #[test]
        fn simple_triple() {
            let tokens = kinds("\"\"\"hello\"\"\"").unwrap();
            assert_eq!(
                tokens,
                vec![
                    TokenKind::StringStart,
                    TokenKind::StringPart("hello".into()),
                    TokenKind::StringEnd,
                    TokenKind::Eof,
                ]
            );
        }

        #[test]
        fn empty_triple() {
            // `""""""` is an empty raw string (six quotes total).
            let tokens = kinds("\"\"\"\"\"\"").unwrap();
            assert_eq!(
                tokens,
                vec![TokenKind::StringStart, TokenKind::StringEnd, TokenKind::Eof]
            );
        }

        #[test]
        fn multiline_triple_preserves_newlines() {
            let src = "\"\"\"line1\nline2\"\"\"";
            let tokens = kinds(src).unwrap();
            // The StringPart should contain the literal newline.
            match &tokens[..] {
                [TokenKind::StringStart, TokenKind::StringPart(s), TokenKind::StringEnd, TokenKind::Eof] =>
                {
                    assert_eq!(s, "line1\nline2");
                }
                other => {
                    panic!("expected [StringStart, StringPart, StringEnd, Eof], got {other:?}")
                }
            }
        }

        #[test]
        fn triple_no_escape_processing() {
            // Backslashes are literal — no escape interpretation.
            let tokens = kinds("\"\"\"a\\nb\"\"\"").unwrap();
            assert_eq!(
                tokens,
                vec![
                    TokenKind::StringStart,
                    TokenKind::StringPart("a\\nb".into()),
                    TokenKind::StringEnd,
                    TokenKind::Eof,
                ]
            );
        }

        #[test]
        fn triple_no_interpolation() {
            // `{expr}` is literal text inside a raw string.
            let tokens = kinds("\"\"\"x {y} z\"\"\"").unwrap();
            assert_eq!(
                tokens,
                vec![
                    TokenKind::StringStart,
                    TokenKind::StringPart("x {y} z".into()),
                    TokenKind::StringEnd,
                    TokenKind::Eof,
                ]
            );
        }

        #[test]
        fn triple_unterminated_errors() {
            assert!(kinds("\"\"\"abc").is_err());
        }
    }

    // T20: Decimal (`m` suffix) literal lexing. The raw digit text is
    // carried verbatim (never rounded through f64) so exactness survives to
    // `rust_decimal_macros::dec!()` codegen.
    mod decimal_literals {
        use super::*;

        #[test]
        fn decimal_m_suffix_fractional() {
            // `99.90m` → DecimalLit("99.90") — exactness preserved.
            let tokens = kinds("99.90m").unwrap();
            assert_eq!(
                tokens,
                vec![TokenKind::DecimalLit("99.90".into()), TokenKind::Eof]
            );
        }

        #[test]
        fn decimal_capital_m_suffix() {
            // `0.1M` is equivalent to `0.1m`.
            let tokens = kinds("0.1M").unwrap();
            assert_eq!(
                tokens,
                vec![TokenKind::DecimalLit("0.1".into()), TokenKind::Eof]
            );
        }

        #[test]
        fn decimal_integer_with_suffix() {
            // `100m` → DecimalLit("100") — integer decimals are valid too.
            let tokens = kinds("100m").unwrap();
            assert_eq!(
                tokens,
                vec![TokenKind::DecimalLit("100".into()), TokenKind::Eof]
            );
        }

        #[test]
        fn decimal_in_arithmetic() {
            // `0.1m + 0.2m == 0.3m` lexes into the expected token stream.
            let tokens = kinds("0.1m + 0.2m == 0.3m").unwrap();
            assert_eq!(
                tokens,
                vec![
                    TokenKind::DecimalLit("0.1".into()),
                    TokenKind::Plus,
                    TokenKind::DecimalLit("0.2".into()),
                    TokenKind::EqEq,
                    TokenKind::DecimalLit("0.3".into()),
                    TokenKind::Eof,
                ]
            );
        }

        #[test]
        fn decimal_preserves_trailing_zero() {
            // `99.90m` keeps the trailing zero (NOT folded to "99.9") — this
            // is what distinguishes carrying raw text from rounding to f64.
            let tokens = kinds("99.90m").unwrap();
            match &tokens[..] {
                [TokenKind::DecimalLit(s), TokenKind::Eof] => {
                    assert_eq!(s, "99.90", "trailing zero must be preserved");
                }
                other => panic!("expected [DecimalLit, Eof], got {other:?}"),
            }
        }

        #[test]
        fn decimal_suffix_does_not_collide_with_double() {
            // `3.14d` is still Double, `3.14m` is Decimal, `3.14` is Float.
            assert!(matches!(
                kinds("3.14d").unwrap()[0],
                TokenKind::DoubleLit(_)
            ));
            assert_eq!(
                kinds("3.14m").unwrap()[0],
                TokenKind::DecimalLit("3.14".into())
            );
            assert!(matches!(kinds("3.14").unwrap()[0], TokenKind::FloatLit(_)));
        }

        #[test]
        fn decimal_m_suffix_display() {
            // Display renders as `decimal("99.90")` for diagnostics.
            assert_eq!(
                TokenKind::DecimalLit("99.90".into()).to_string(),
                "decimal(\"99.90\")"
            );
        }
    }
}

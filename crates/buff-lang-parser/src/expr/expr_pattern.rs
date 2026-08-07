//! Pattern + match + closure parsers - extracted from `expr.rs` (T106 mechanical split).
//!
//! Contains parse_match (T27 match-expression), parse_pattern (the recursive
//! pattern matcher), and parse_closure (lambda `{ |args| body }`).

use buff_lang_ast::{Block, Expr, Ident, Literal, MatchArm, Pattern, Stmt};
use buff_lang_error::{Diagnostic, ErrorCode, ParseError, Span};
use buff_lang_lexer::TokenKind;

use super::parse_expression;
use super::parse_primary;
use crate::stream::TokenStream;
// ---------------------------------------------------------------------------
// T27 — `match` expressions and patterns.
//
// Buff `match` syntax (brace form, consistent with map/struct-init "braces
// are data" rule from the README):
//
//   match scrutinee {
//       Pattern => body,
//       Pattern => body,
//       _       => fallback,
//   }
//
// Each arm is `Pattern => expr`. The body is a single expression (the parser
// wraps it in a one-statement block so the AST shape matches every other
// block-bearing node). Trailing comma is allowed. Pattern shapes:
//
//   _                — wildcard (catch-all)
//   Red              — unit variant or bare binding (resolved later by the
//                      type system; for now the parser emits Pattern::Ident)
//   Ok(v)            — data variant with a binding subpattern
//   Err(_)           — data variant with a wildcard subpattern
//   Ok(Ok(v))        — nested (recursively parsed)
//
// Bare identifiers in match arms are AMBIGUOUS at parse time: `Red` could
// be a unit variant reference OR a fresh binding. Rust resolves this with
// type information; Buff defers the same way. The exhaustiveness checker
// (buff-lang-types) treats any arm whose pattern name matches a known enum
// variant as covering that variant; non-matching names act as bindings
// (which are exhaustive by virtue of capturing any value).
// ---------------------------------------------------------------------------

/// Parse a `match scrutinee { arm, arm, ... }` expression whose `match`
/// keyword is the next significant token (T27, extended BUG-11).
///
/// # Shapes
///
/// **Brace form** (T27, original): `match EXPR { PAT => EXPR (, PAT => EXPR)* ,? }`.
/// Arms are comma-separated inside `{ }`. Each arm body is `PAT => body` where
/// `body` is a single expression (wrapped in a one-statement block), a
/// multi-statement `{ }` block (BUG-11c), or a `return` (self-host pattern).
///
/// **Layout form** (BUG-11a): `match EXPR:\n  PAT => body\n  PAT => body\n`.
/// Arms are newline-separated inside an indented block. Each arm body is
/// either `PAT => body` (single expression, multi-statement `{ }` block, or
/// `return`) or `PAT:` followed by an indented multi-statement block
/// (BUG-11b).
///
/// The scrutinee is a full expression (so `match foo.bar(x) { ... }` works).
/// Builds an [`Expr::MatchExpr`].
///
/// # Errors
///
/// Returns [`ParseError`] if:
/// - the scrutinee fails to parse,
/// - the opening `{` (brace form) or `:` + newline + indent (layout form) is
///   missing,
/// - an arm pattern fails to parse,
/// - the `=>` between pattern and body is missing (when no `:` block follows),
/// - the closing `}` (brace form) is missing.
pub fn parse_match(stream: &mut TokenStream<'_>) -> Result<Expr, ParseError> {
    let kw = stream.expect(TokenKind::KwMatch)?;
    let start = kw.span.start;
    let source_id = stream.source_id();
    // Scrutinee: a full expression.
    let scrutinee = parse_expression(stream)?;

    // BUG-11a: dispatch on the NEXT RAW token. `:` introduces the layout form
    // (`match x:\n    ...`); anything else (typically `{`) is the brace form.
    // Using `check_raw` is essential here: `peek_kind` would skip layout
    // tokens and could misidentify the boundary.
    if stream.check_raw(&TokenKind::Colon) {
        return parse_match_layout(stream, scrutinee, start, source_id);
    }

    // ---- Brace form (T27, original) -------------------------------------
    // Opening `{`.
    stream.expect(TokenKind::LBrace)?;
    let mut arms: Vec<MatchArm> = Vec::new();
    // Empty body `match e { }` is allowed (degenerate; will be flagged by
    // exhaustiveness if the scrutinee is non-empty).
    if matches!(stream.peek_kind(), Some(TokenKind::RBrace)) {
        let rb = stream.expect(TokenKind::RBrace)?;
        let span = Span::new(start, rb.span.end, source_id);
        return Ok(Expr::MatchExpr {
            scrutinee: Box::new(scrutinee),
            arms,
            span,
        });
    }
    let mut arm_end;
    loop {
        let pat = parse_pattern(stream)?;
        // T40: optional `if <cond>` pattern guard.
        let guard = parse_optional_guard(stream)?;
        let arm_pat_span = pat.span();
        stream.expect(TokenKind::FatArrow)?;
        // BUG-11c: the body may be a multi-statement `{ }` block. The arm
        // body helper dispatches on the next raw token.
        let (body, body_end) = parse_match_arm_body(stream, arm_pat_span, source_id, false)?;
        arm_end = body_end;
        arms.push(MatchArm {
            pattern: pat,
            guard,
            body,
            span: Span::new(arm_pat_span.start, arm_end, source_id),
        });
        match stream.peek_kind() {
            Some(TokenKind::Comma) => {
                stream.advance();
                // Allow trailing comma.
                if matches!(stream.peek_kind(), Some(TokenKind::RBrace)) {
                    break;
                }
            }
            Some(TokenKind::RBrace) => break,
            Some(other) => {
                return Err(ParseError::new(Diagnostic::error(
                    format!("expected `,` or `}}` after match arm, found `{other}`"),
                    stream
                        .peek()
                        .map(|t| t.span)
                        .unwrap_or_else(|| stream.eof_span()),
                )));
            }
            None => {
                return Err(ParseError::new(Diagnostic::error(
                    "unterminated match expression (missing `}`)",
                    stream.eof_span(),
                )));
            }
        }
    }
    let rb = stream.expect(TokenKind::RBrace)?;
    let span = Span::new(start, rb.span.end, source_id);
    Ok(Expr::MatchExpr {
        scrutinee: Box::new(scrutinee),
        arms,
        span,
    })
}

/// BUG-11a: parse the layout-sensitive form of a match expression.
///
/// Called after `match EXPR` has been parsed and `:` is the next raw token.
/// Consumes the `:`, the mandatory `Newline`, the `Indent`, then loops parsing
/// arms (newline-separated) until a `Dedent` or end-of-input. Arms may use
/// either `PAT => body` or `PAT:` + indented block (BUG-11b).
fn parse_match_layout(
    stream: &mut TokenStream<'_>,
    scrutinee: Expr,
    start: usize,
    source_id: buff_lang_error::SourceId,
) -> Result<Expr, ParseError> {
    // Consume the `:` (already verified by the caller via check_raw, but expect
    // gives a clean error message on any surprise).
    stream.expect(TokenKind::Colon)?;

    // Expect a Newline immediately after the colon.
    if !stream.check_raw(&TokenKind::Newline) {
        return Err(ParseError::new(
            Diagnostic::error(
                "expected newline after `:` for layout match block",
                stream.span_here(),
            )
            .with_code(ErrorCode::ExpectedLayoutNewline),
        ));
    }
    stream.advance_raw(); // consume Newline

    // Skip any stray blank lines between the `:` line and the first arm.
    while stream.consume_newline() {}

    // Expect an Indent to open the arm list.
    if !stream.consume_indent() {
        return Err(ParseError::new(
            Diagnostic::error(
                "expected indented match arms after `match ...:`",
                stream.span_here(),
            )
            .with_code(ErrorCode::ExpectedIndentedBlock),
        ));
    }

    let mut arms: Vec<MatchArm> = Vec::new();
    loop {
        // Skip blank lines / inter-arm separators.
        while stream.consume_newline() {}

        // End of the match block: a Dedent returns to the outer scope.
        if stream.check_raw(&TokenKind::Dedent) {
            break;
        }
        // Defensive: end-of-input without a Dedent (the lexer may collapse
        // trailing dedents at EOF in some edge cases).
        if stream.is_at_end() {
            break;
        }
        // Defensive: skip stray Indent tokens that should not appear here but
        // might from nested layout constructs.
        if stream.check_raw(&TokenKind::Indent) {
            stream.advance_raw();
            continue;
        }

        // Parse one arm: pattern + optional guard + body.
        let pat = parse_pattern(stream)?;
        let guard = parse_optional_guard(stream)?;
        let arm_pat_span = pat.span();

        let (body, body_end) = if stream.check_raw(&TokenKind::Colon) {
            // BUG-11b: `PAT:` introduces a colon-block arm with an indented
            // multi-statement body. parse_block consumes the `:`, Newline,
            // Indent, statements, and Dedent.
            let blk = crate::stmt::parse_block(stream)?;
            let end = blk.span.end;
            (blk, end)
        } else {
            // `PAT => body` form.
            stream.expect(TokenKind::FatArrow)?;
            parse_match_arm_body(stream, arm_pat_span, source_id, true)?
        };

        arms.push(MatchArm {
            pattern: pat,
            guard,
            body,
            span: Span::new(arm_pat_span.start, body_end, source_id),
        });
    }

    // Consume the closing Dedent if present (may be absent at EOF).
    let _ = stream.consume_dedent();

    let end = arms.last().map(|a| a.span.end).unwrap_or(start);
    let span = Span::new(start, end, source_id);
    Ok(Expr::MatchExpr {
        scrutinee: Box::new(scrutinee),
        arms,
        span,
    })
}

/// Parse an optional `if <cond>` pattern guard (T40). Called after the arm
/// pattern has been parsed. Returns `Some(expr)` when a guard is present,
/// `None` otherwise. The `if` keyword is consumed on the `Some` path.
fn parse_optional_guard(stream: &mut TokenStream<'_>) -> Result<Option<Expr>, ParseError> {
    if matches!(stream.peek_kind(), Some(TokenKind::KwIf)) {
        stream.advance(); // consume `if`
        let g = parse_expression(stream)?;
        Ok(Some(g))
    } else {
        Ok(None)
    }
}

/// Parse a match arm body — the part after `PAT =>` (or dispatched from the
/// layout form). Returns `(body_block, end_offset)`.
///
/// Dispatch rules (BUG-11b + BUG-11c):
///
/// - **`return`**: bare `return` or `return expr` (self-host pattern). In the
///   brace form the value is absent when the next significant token is `,` or
///   `}`; in the layout form it is absent when the next *raw* token is a
///   `Newline`/`Dedent` (end of line / end of block).
/// - **`{`** (raw): a multi-statement block via `parse_block_braces`
///   (BUG-11c). This is NOT a closure — `{` after `=>` in a match arm is a
///   statement block.
/// - **`:`** (raw, layout form only): an indented multi-statement block via
///   `parse_block` (BUG-11b).
/// - **otherwise**: a single expression wrapped in a one-statement block
///   (backward-compatible with the original T27 behaviour).
fn parse_match_arm_body(
    stream: &mut TokenStream<'_>,
    arm_pat_span: Span,
    source_id: buff_lang_error::SourceId,
    layout_form: bool,
) -> Result<(Block, usize), ParseError> {
    // Allow `return` as a match arm body (self-host pattern:
    // `TokenKind.KwFunc => return true,`). `return` is a statement, not an
    // expression, so we handle it specially before falling through to the
    // expression parser.
    if matches!(stream.peek_kind(), Some(TokenKind::KwReturn)) {
        let ret_tok = stream.advance_after_peek(); // peek_kind confirmed KwReturn
        let ret_expr = if layout_form {
            // Layout form: bare `return` at end of line → no value. After
            // consuming `return`, the next RAW token is a Newline (end of the
            // arm line) or a Dedent (end of the match block) when there is no
            // return value.
            match stream.peek_raw_kind() {
                Some(TokenKind::Newline) | Some(TokenKind::Dedent) | None => None,
                _ => Some(parse_expression(stream)?),
            }
        } else {
            // Brace form: bare `return` at end of arm → no value.
            if matches!(
                stream.peek_kind(),
                Some(TokenKind::Comma | TokenKind::RBrace)
            ) {
                None
            } else {
                Some(parse_expression(stream)?)
            }
        };
        let arm_end = ret_expr
            .as_ref()
            .map(|e| e.span().end)
            .unwrap_or(ret_tok.span.end);
        let block = Block {
            stmts: vec![Stmt::Return(ret_expr, ret_tok.span)],
            span: Span::new(arm_pat_span.start, arm_end, source_id),
        };
        return Ok((block, arm_end));
    }

    // BUG-11c: `{ ... }` after `=>` → multi-statement block (NOT a closure).
    // `check_raw` is used (not `check`) so we see the `{` before any layout
    // skipping; `{` is never a layout token so both checks agree, but raw is
    // the canonical form for body-shape dispatch.
    if stream.check_raw(&TokenKind::LBrace) {
        let block = crate::stmt::parse_block_braces(stream)?;
        let end = block.span.end;
        return Ok((block, end));
    }

    // BUG-11b: `:` after `=>` in the layout form → indented block body.
    if layout_form && stream.check_raw(&TokenKind::Colon) {
        let block = crate::stmt::parse_block(stream)?;
        let end = block.span.end;
        return Ok((block, end));
    }

    // Default: single expression wrapped in a one-statement block (T27
    // backward-compatible behaviour).
    let body_expr = parse_expression(stream)?;
    let arm_end = body_expr.span().end;
    let block = Block {
        stmts: vec![Stmt::ExprStmt(body_expr, arm_pat_span)],
        span: Span::new(arm_pat_span.start, arm_end, source_id),
    };
    Ok((block, arm_end))
}

/// Parse a single pattern (T27, extended in T71 and T39).
///
/// This is the PUBLIC entry point shared by `match` arms and `let`-
/// destructuring bindings. It parses ONE atomic pattern via
/// [`parse_pattern_atom`], then — when a `|` follows (T39 or-patterns) —
/// greedily consumes `| atom | atom | ...` and wraps the result in a
/// [`Pattern::Or`]. The `|` handling lives HERE (the wrapper), not in
/// [`parse_pattern_atom`], so every recursive subpattern position
/// (variant tuples, tuple patterns, struct fields) inherits or-pattern
/// support automatically: a subpattern call goes through this wrapper,
/// so `Ok(Red | Green)` and `Some(1 | 2)` parse correctly.
///
/// The `|` token is unambiguous in pattern position: Buff closures use
/// `{ params => body }` (FatArrow, no pipes), and `||`/`|>` are expression-
/// level operators that never appear inside a pattern. So a lone `Pipe`
/// after a pattern unambiguously introduces an or-pattern alternative.
///
/// # Errors
///
/// Returns [`ParseError`] if a subpattern fails to parse or a closing
/// delimiter (`)` or `}`) is missing.
pub fn parse_pattern(stream: &mut TokenStream<'_>) -> Result<Pattern, ParseError> {
    let source_id = stream.source_id();
    let first = parse_pattern_atom(stream)?;
    // T39: or-pattern `A | B | C`. Only enter the loop when a `|` follows;
    // the common single-pattern case returns immediately (zero peek cost
    // when there is no `|`, and byte-identical AST to pre-T39).
    if !matches!(stream.peek_kind(), Some(TokenKind::Pipe)) {
        return Ok(first);
    }
    let start = first.span().start;
    let mut alts = vec![first];
    while matches!(stream.peek_kind(), Some(TokenKind::Pipe)) {
        stream.advance(); // consume `|`
        alts.push(parse_pattern_atom(stream)?);
    }
    let end = alts
        .last()
        .map(|p| p.span().end)
        .unwrap_or_else(|| stream.eof_span().end);
    Ok(Pattern::Or(alts, Span::new(start, end, source_id)))
}

/// Parse a single ATOMIC pattern — the inner helper that does NOT consume a
/// trailing `|` chain (T39). See [`parse_pattern`] for the public wrapper
/// that adds or-pattern support.
///
/// Supported shapes:
/// - `_` — wildcard. Emits [`Pattern::Wildcard`].
/// - `Ident` — bare identifier. Emits [`Pattern::Ident`] (the
///   variant-vs-binding disambiguation is deferred to the type system).
/// - `Ident(pat, pat, ...)` — data variant with subpatterns. Emits
///   [`Pattern::Variant`] with an empty `enum_name` placeholder (the parser
///   does not know which enum the variant belongs to; exhaustiveness and
///   codegen resolve it by name).
/// - `Ident { field: pat, ... }` — struct destructuring. Emits
///   [`Pattern::Struct`] (T71). Shorthand `Ident { field }` binds the field
///   to a pattern of the same name.
/// - `(pat, pat, ...)` — tuple destructuring. Emits [`Pattern::Tuple`] (T71).
/// - `-N`, `42`, `"hi"`, `true`, `'a'` — literal patterns. Emits
///   [`Pattern::Literal`] (negative literals are encoded as a unary-minus
///   AST expr that we collapse into the literal value; the parser handles
///   the sign here so downstream codegen sees a plain `Literal::Int(-N)`).
///
/// Subpatterns inside a variant tuple / tuple pattern / struct field
/// recursively call `parse_pattern` (the wrapper), so nesting
/// (`Ok(Err(_))`, `(a, (b, c))`, `Outer { inner: Inner { x } }`,
/// `Some(1 | 2)`) works. Trailing comma is allowed in all delimited forms.
///
/// # Errors
///
/// Returns [`ParseError`] if a subpattern fails to parse or a closing
/// delimiter (`)` or `}`) is missing.
fn parse_pattern_atom(stream: &mut TokenStream<'_>) -> Result<Pattern, ParseError> {
    let source_id = stream.source_id();
    // Wildcard `_`. The lexer produces this as `Ident("_")` (underscore is a
    // valid identifier character), so we detect the wildcard by matching the
    // ident's NAME rather than a dedicated token kind.
    if let Some(TokenKind::Ident(name)) = stream.peek_kind() {
        if name == "_" {
            let tok = stream.advance_after_peek();
            return Ok(Pattern::Wildcard(tok.span));
        }
    }
    // Literal patterns: integer / float / double / bool / string / char / byte
    // (and decimal). We reuse the same token-kind set as `parse_primary`.
    if let Some(tok) = stream.peek().cloned() {
        let is_literal_kind = matches!(
            tok.kind,
            TokenKind::IntLit(_)
                | TokenKind::FloatLit(_)
                | TokenKind::DoubleLit(_)
                | TokenKind::ByteLit(_)
                | TokenKind::CharLit(_)
                | TokenKind::DecimalLit(_)
                | TokenKind::KwTrue
                | TokenKind::KwFalse
                | TokenKind::StringStart
        );
        if is_literal_kind {
            let span_start = tok.span.start;
            // A literal pattern is parsed by routing through the primary
            // expression parser and then collapsing the resulting
            // `Expr::Literal` back into a `Pattern::Literal`. This keeps the
            // literal-literal handling in one place (no DRY violation) and
            // gives us string interpolation handling for free (though interp
            // in patterns is unusual).
            let expr = parse_primary(stream)?;
            if let Expr::Literal(lit, span) = expr {
                let _ = span_start;
                return Ok(Pattern::Literal(lit, span));
            }
            // Any other primary shape at pattern position is an error
            // (e.g. `[1, 2]` is not a valid pattern in v0.5).
            return Err(ParseError::new(Diagnostic::error(
                "expected a literal pattern, found an expression",
                expr.span(),
            )));
        }
        // Negative literal: `-N`. The lexer tokenises `-42` as `Minus` `42`;
        // we collapse the two tokens into one `Literal::Int(-N)` pattern.
        if matches!(tok.kind, TokenKind::Minus) {
            let saved = stream.save();
            stream.advance(); // consume `-`
            if let Some(next) = stream.peek().cloned() {
                if let TokenKind::IntLit(v) = next.kind {
                    stream.advance();
                    return Ok(Pattern::Literal(
                        Literal::Int(-v),
                        Span::new(tok.span.start, next.span.end, source_id),
                    ));
                }
            }
            // Not a negative literal — roll back and fall through to ident
            // handling (which will error on `-` as a non-ident).
            stream.restore(saved);
        }
        // T71: tuple destructuring pattern `(subpat, subpat, ...)`. Recurses
        // into `parse_pattern` so nesting (`(a, (b, c))`) works. Trailing
        // comma inside the tuple is allowed. Empty `()` is allowed (zero
        // sub-patterns).
        if matches!(tok.kind, TokenKind::LParen) {
            let lp = stream.expect(TokenKind::LParen)?;
            let mut subs: Vec<Pattern> = Vec::new();
            if !matches!(stream.peek_kind(), Some(TokenKind::RParen)) {
                loop {
                    subs.push(parse_pattern(stream)?);
                    match stream.peek_kind() {
                        Some(TokenKind::Comma) => {
                            stream.advance();
                            if matches!(stream.peek_kind(), Some(TokenKind::RParen)) {
                                break;
                            }
                        }
                        Some(TokenKind::RParen) => break,
                        Some(other) => {
                            return Err(ParseError::new(Diagnostic::error(
                                format!("expected `,` or `)` in tuple pattern, found `{other}`"),
                                stream
                                    .peek()
                                    .map(|t| t.span)
                                    .unwrap_or_else(|| stream.eof_span()),
                            )));
                        }
                        None => {
                            return Err(ParseError::new(Diagnostic::error(
                                "unterminated tuple pattern (missing `)`)",
                                stream.eof_span(),
                            )));
                        }
                    }
                }
            }
            let rp = stream.expect(TokenKind::RParen)?;
            return Ok(Pattern::Tuple(
                subs,
                Span::new(lp.span.start, rp.span.end, source_id),
            ));
        }
        // Identifier-starting patterns: bare ident OR `Ident(subpatterns)`.
        if matches!(tok.kind, TokenKind::Ident(_)) {
            stream.advance();
            let TokenKind::Ident(name) = tok.kind.clone() else {
                // Defensive: the `matches!` above guarantees this is an Ident,
                // but we return a structured error instead of panicking so a
                // future TokenKind change can't crash the parser.
                return Err(ParseError::new(Diagnostic::error(
                    format!("expected identifier in pattern, found `{}`", tok.kind),
                    tok.span,
                )));
            };
            let ident = Ident::new(name, tok.span);
            // Dot-qualified variant pattern: `EnumName.VariantName` or
            // `EnumName.VariantName(args)`. Buff convention uses dot
            // notation for enum variant access (Type.method()).
            if matches!(stream.peek_kind(), Some(TokenKind::Dot)) {
                stream.advance(); // consume `.`
                let variant_tok = stream.advance().ok_or_else(|| {
                    ParseError::new(Diagnostic::error(
                        "expected variant name after `.` in pattern",
                        stream.eof_span(),
                    ))
                })?;
                let TokenKind::Ident(variant_ident) = variant_tok.kind.clone() else {
                    return Err(ParseError::new(Diagnostic::error(
                        "expected variant name after `.` in pattern",
                        variant_tok.span,
                    )));
                };
                let variant_ident = Ident::new(variant_ident, variant_tok.span);
                // Check for tuple variant: `EnumName.VariantName(args)`.
                if matches!(stream.peek_kind(), Some(TokenKind::LParen)) {
                    stream.advance(); // consume `(`
                    let mut subpats: Vec<Pattern> = Vec::new();
                    if !matches!(stream.peek_kind(), Some(TokenKind::RParen)) {
                        loop {
                            subpats.push(parse_pattern(stream)?);
                            match stream.peek_kind() {
                                Some(TokenKind::Comma) => {
                                    stream.advance();
                                    if matches!(stream.peek_kind(), Some(TokenKind::RParen)) {
                                        break;
                                    }
                                }
                                Some(TokenKind::RParen) => break,
                                _ => break,
                            }
                        }
                    }
                    let rp = stream.expect(TokenKind::RParen)?;
                    return Ok(Pattern::Variant {
                        enum_name: ident,
                        variant: variant_ident,
                        subpatterns: subpats,
                        span: Span::new(tok.span.start, rp.span.end, source_id),
                    });
                }
                // Unit variant: `EnumName.VariantName` (no args).
                return Ok(Pattern::Variant {
                    enum_name: ident,
                    variant: variant_ident,
                    subpatterns: Vec::new(),
                    span: Span::new(tok.span.start, variant_tok.span.end, source_id),
                });
            }
            // Variant tuple: `Ident ( subpat, subpat, ... )`.
            if matches!(stream.peek_kind(), Some(TokenKind::LParen)) {
                stream.advance(); // consume `(`
                let mut subpats: Vec<Pattern> = Vec::new();
                // Empty `()` is allowed — treat as zero subpatterns.
                if !matches!(stream.peek_kind(), Some(TokenKind::RParen)) {
                    loop {
                        subpats.push(parse_pattern(stream)?);
                        match stream.peek_kind() {
                            Some(TokenKind::Comma) => {
                                stream.advance();
                                if matches!(stream.peek_kind(), Some(TokenKind::RParen)) {
                                    break;
                                }
                            }
                            Some(TokenKind::RParen) => break,
                            Some(other) => {
                                return Err(ParseError::new(Diagnostic::error(
                                    format!(
                                        "expected `,` or `)` in variant pattern, found `{other}`"
                                    ),
                                    stream
                                        .peek()
                                        .map(|t| t.span)
                                        .unwrap_or_else(|| stream.eof_span()),
                                )));
                            }
                            None => {
                                return Err(ParseError::new(Diagnostic::error(
                                    "unterminated variant pattern (missing `)`)",
                                    stream.eof_span(),
                                )));
                            }
                        }
                    }
                }
                let rparen = stream.expect(TokenKind::RParen)?;
                return Ok(Pattern::Variant {
                    // Parser does not know which enum this variant belongs to;
                    // codegen emits just `Variant(...)` (Rust resolves it when
                    // the enum is in scope) and exhaustiveness matches by name.
                    enum_name: Ident::new("", tok.span),
                    variant: ident,
                    subpatterns: subpats,
                    span: Span::new(tok.span.start, rparen.span.end, source_id),
                });
            }
            // T71: struct destructuring pattern `Name { field: subpat, ... }`
            // (shorthand `Name { field }` == `Name { field: field }`). Field
            // order is preserved as written (Vec, never a HashMap — determinism).
            // Trailing comma inside the braces is allowed. Empty `Name { }` is
            // allowed. T41: a `..` rest token (`Name { x, .. }`) ignores all
            // unmentioned fields (sets the `rest` flag).
            if matches!(stream.peek_kind(), Some(TokenKind::LBrace)) {
                stream.expect(TokenKind::LBrace)?; // consume `{`
                let mut fields: Vec<(Ident, Pattern)> = Vec::new();
                // T41: `..` rest flag. Set when a `..` token is encountered
                // in the field list; the codegen emits Rust `..rest`.
                let mut rest = false;
                if !matches!(stream.peek_kind(), Some(TokenKind::RBrace)) {
                    loop {
                        // T41: `..` rest pattern. Consume the DotDot token,
                        // set the rest flag, then fall through to the
                        // comma/RBrace separator handling (a trailing comma
                        // after `..` is allowed: `{ x, .., }`).
                        if matches!(stream.peek_kind(), Some(TokenKind::DotDot)) {
                            stream.advance(); // consume `..`
                            rest = true;
                            match stream.peek_kind() {
                                Some(TokenKind::Comma) => {
                                    stream.advance();
                                    if matches!(stream.peek_kind(), Some(TokenKind::RBrace)) {
                                        break;
                                    }
                                    continue;
                                }
                                Some(TokenKind::RBrace) => break,
                                Some(other) => {
                                    return Err(ParseError::new(Diagnostic::error(
                                        format!(
                                            "expected `,` or `}}` after `..` in struct pattern, found `{other}`"
                                        ),
                                        stream
                                            .peek()
                                            .map(|t| t.span)
                                            .unwrap_or_else(|| stream.eof_span()),
                                    )));
                                }
                                None => {
                                    return Err(ParseError::new(Diagnostic::error(
                                        "unterminated struct pattern after `..` (missing `}`)",
                                        stream.eof_span(),
                                    )));
                                }
                            }
                        }
                        // Field name MUST be a bare identifier.
                        let Some(ftok) = stream.advance() else {
                            return Err(ParseError::new(Diagnostic::error(
                                "expected struct field name, found end of input",
                                stream.eof_span(),
                            )));
                        };
                        let TokenKind::Ident(fname) = ftok.kind.clone() else {
                            return Err(ParseError::new(Diagnostic::error(
                                format!(
                                    "expected struct field name (identifier), found `{}`",
                                    ftok.kind
                                ),
                                ftok.span,
                            )));
                        };
                        let field_ident = Ident::new(fname, ftok.span);
                        // Explicit `field: subpattern` OR shorthand `field`
                        // (which binds the field to a pattern of the same name).
                        let subpat = if matches!(stream.peek_kind(), Some(TokenKind::Colon)) {
                            stream.advance(); // consume `:`
                            parse_pattern(stream)?
                        } else {
                            Pattern::Ident(field_ident.clone(), ftok.span)
                        };
                        fields.push((field_ident, subpat));
                        match stream.peek_kind() {
                            Some(TokenKind::Comma) => {
                                stream.advance();
                                if matches!(stream.peek_kind(), Some(TokenKind::RBrace)) {
                                    break;
                                }
                            }
                            Some(TokenKind::RBrace) => break,
                            Some(other) => {
                                return Err(ParseError::new(Diagnostic::error(
                                    format!(
                                        "expected `,` or `}}` in struct pattern, found `{other}`"
                                    ),
                                    stream
                                        .peek()
                                        .map(|t| t.span)
                                        .unwrap_or_else(|| stream.eof_span()),
                                )));
                            }
                            None => {
                                return Err(ParseError::new(Diagnostic::error(
                                    "unterminated struct pattern (missing `}`)",
                                    stream.eof_span(),
                                )));
                            }
                        }
                    }
                }
                let rb = stream.expect(TokenKind::RBrace)?;
                return Ok(Pattern::Struct {
                    name: ident,
                    fields,
                    span: Span::new(tok.span.start, rb.span.end, source_id),
                    rest,
                });
            }
            // Bare unit variant / binding.
            return Ok(Pattern::Ident(ident, tok.span));
        }
    }
    // Fall-through: unexpected token at pattern position.
    let span = stream
        .peek()
        .map(|t| t.span)
        .unwrap_or_else(|| stream.eof_span());
    Err(ParseError::new(Diagnostic::error(
        format!(
            "expected a match pattern, found {}",
            stream
                .peek_kind()
                .map(|k| k.to_string())
                .unwrap_or_else(|| "end of input".to_string())
        ),
        span,
    )))
}

/// Parse a closure whose opening `{` is the next significant token (T23,
/// extended by BUG-13 for multi-statement bodies and zero-param form).
///
/// # Shapes
///
/// - **Single-expression body** (T23, backward compatible):
///   `{ ident (, ident)* => expr }`
/// - **Multi-statement body** (BUG-13): `{ params => stmt1; stmt2; final_expr }`
///   — statements separated by `;` or newlines; the FINAL expression is the
///   implicit return value (mirrors Rust closures). Preceding statements are
///   side effects (`let`, expression statements, etc.).
/// - **Zero-param form** (BUG-13): `{ => body }` — empty parameter list.
///
/// Parameter types are inferred (a placeholder `TypeRef` is stored; codegen
/// ignores it for closures). Typed params + capture analysis are T34 — this
/// form covers `.map` / `.filter` / `.reduce` and richer inline closures.
///
/// # Multi-statement body dispatch
///
/// After `=>`, the first body element is parsed with [`parse_expression`]
/// (greedy Pratt — stops when the next significant token is not an infix/postfix
/// continuation). If the next significant token is then `}`, the body is a
/// single expression (fast path, byte-identical to the original T23 form).
/// Otherwise the remaining statements are parsed in a loop via
/// [`crate::stmt::parse_statement`], consuming optional `;` separators;
/// [`TokenStream::peek_kind`] transparently skips `Newline`/`Indent`/`Dedent`
/// layout tokens, so newline-separated statements work without special handling.
pub fn parse_closure(stream: &mut TokenStream<'_>) -> Result<Expr, ParseError> {
    use buff_lang_ast::common::{Block, Param};
    let lb = stream.expect(TokenKind::LBrace)?;
    let source_id = stream.source_id();
    // Parse zero or more comma-separated identifier parameters. BUG-13: when
    // the next significant token is already `=>`, the param list is empty
    // (zero-param closure `{ => body }`); skip the loop entirely.
    let mut params: Vec<Param> = Vec::new();
    if !stream.check(&TokenKind::FatArrow) {
        loop {
            let ptok = match stream.advance() {
                Some(t) if matches!(t.kind, TokenKind::Ident(_)) => t,
                Some(other) => {
                    return Err(ParseError::new(Diagnostic::error(
                        format!("expected closure parameter name, found `{}`", other.kind),
                        other.span,
                    )));
                }
                None => {
                    return Err(ParseError::new(Diagnostic::error(
                        "expected closure parameter name, found end of input",
                        stream.eof_span(),
                    )));
                }
            };
            let TokenKind::Ident(pname) = ptok.kind.clone() else {
                // Defensive: the `matches!` above guarantees this is an Ident,
                // but we return a structured error instead of panicking so a
                // future TokenKind change can't crash the parser.
                return Err(ParseError::new(Diagnostic::error(
                    format!("expected closure parameter name, found `{}`", ptok.kind),
                    ptok.span,
                )));
            };
            params.push(Param {
                name: Ident::new(pname, ptok.span),
                // Placeholder type — closures infer their param types; codegen
                // emits `|name|` (no annotation). T34 will add typed params.
                ty: buff_lang_ast::TypeRef::Named {
                    name: Ident::new("_", ptok.span),
                    span: ptok.span,
                },
                default_value: None,
                is_comptime: false,
                span: ptok.span,
            });
            if matches!(stream.peek_kind(), Some(TokenKind::Comma)) {
                stream.advance(); // consume ','
                continue;
            }
            break;
        }
    }
    let arrow = stream.expect(TokenKind::FatArrow)?;
    // Parse the closure body. Two dispatch paths converge on the same Block:
    //
    // 1. **Statement-led body** (BUG-13): when the first body token is a
    //    statement keyword (`let`, `return`, `for`, …) that
    //    [`parse_expression`] cannot consume, every body element — including
    //    the first — is parsed via [`crate::stmt::parse_statement`]. This lets
    //    a `let` binding open the body: `{ req => let id = req; fetch(id) }`.
    // 2. **Expression-led body** (T23 + BUG-13): the first body element is an
    //    expression, parsed via [`parse_expression`] (the backward-compatible
    //    single-expression form `{ x => expr }` when no further statement
    //    follows). Subsequent statements, if any, use `parse_statement`.
    //
    // Both paths then loop to consume additional `;`/newline-separated
    // statements until `}`. The single-expression form takes the fast path —
    // the loop condition is false immediately and `stmts` stays at one element
    // (byte-identical to the pre-BUG-13 T23 behaviour).
    let body_starts_with_statement = matches!(
        stream.peek_kind(),
        Some(TokenKind::KwLet)
            | Some(TokenKind::KwReturn)
            | Some(TokenKind::KwBreak)
            | Some(TokenKind::KwContinue)
            | Some(TokenKind::KwFor)
            | Some(TokenKind::KwGuard)
            | Some(TokenKind::KwDefer)
            | Some(TokenKind::At)
    );
    let mut stmts: Vec<buff_lang_ast::Stmt> = if body_starts_with_statement {
        vec![crate::stmt::parse_statement(stream)?]
    } else {
        let body_expr = parse_expression(stream)?;
        vec![buff_lang_ast::Stmt::ExprStmt(body_expr, arrow.span)]
    };
    // BUG-13: consume additional statements separated by `;` or newlines.
    // [`TokenStream::peek_kind`] transparently skips `Newline`/`Indent`/
    // `Dedent`, so layout-separated statements work without special handling.
    while !matches!(stream.peek_kind(), Some(TokenKind::RBrace) | None) {
        // Consume an optional `;` separator (newlines already skipped above).
        if matches!(stream.peek_kind(), Some(TokenKind::Semicolon)) {
            stream.advance();
        }
        if matches!(stream.peek_kind(), Some(TokenKind::RBrace) | None) {
            // Trailing separator before `}` (e.g. `{ x => x; }`) — stop without
            // adding another statement.
            break;
        }
        stmts.push(crate::stmt::parse_statement(stream)?);
    }
    let rb = stream.expect(TokenKind::RBrace)?;
    let body = Block {
        stmts,
        span: Span::new(lb.span.start, rb.span.end, source_id),
    };
    let span = Span::new(lb.span.start, rb.span.end, source_id);
    Ok(Expr::Lambda {
        params,
        body,
        return_type: None,
        span,
    })
}

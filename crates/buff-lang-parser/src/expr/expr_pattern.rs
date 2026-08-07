//! Pattern + match + closure parsers - extracted from `expr.rs` (T106 mechanical split).
//!
//! Contains parse_match (T27 match-expression), parse_pattern (the recursive
//! pattern matcher), and parse_closure (lambda `{ |args| body }`).

use buff_lang_ast::{Block, Expr, Ident, Literal, MatchArm, Pattern, Stmt};
use buff_lang_error::{Diagnostic, ParseError, Span};
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
/// keyword is the next significant token (T27).
///
/// Shape: `match EXPR { PAT => EXPR (, PAT => EXPR)* ,? }`. The scrutinee is
/// a full expression (so `match foo.bar(x) { ... }` works). Each arm body is
/// a single expression wrapped in a one-statement block. Trailing comma is
/// allowed. Builds an [`Expr::MatchExpr`].
///
/// # Errors
///
/// Returns [`ParseError`] if:
/// - the scrutinee fails to parse,
/// - the opening `{` is missing,
/// - an arm pattern fails to parse,
/// - the `=>` between pattern and body is missing,
/// - the closing `}` is missing.
pub fn parse_match(stream: &mut TokenStream<'_>) -> Result<Expr, ParseError> {
    let kw = stream.expect(TokenKind::KwMatch)?;
    let start = kw.span.start;
    let source_id = stream.source_id();
    // Scrutinee: a full expression.
    let scrutinee = parse_expression(stream)?;
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
        // T40: optional `if <cond>` pattern guard. After the pattern (and
        // before the `=>`), a `if` keyword introduces a guard expression.
        // The arm matches only when BOTH the pattern matches AND the guard
        // evaluates to `true`. Buff's `if` is the reserved `KwIf` keyword, so
        // there is no ambiguity with an identifier named `if`.
        let guard = if matches!(stream.peek_kind(), Some(TokenKind::KwIf)) {
            stream.advance(); // consume `if`
            let g = parse_expression(stream)?;
            // Extend the arm's token-span start to include the guard so the
            // span covers `Pattern if guard`.
            Some(g)
        } else {
            None
        };
        stream.expect(TokenKind::FatArrow)?;
        // Allow `return` as a match arm body (self-host pattern:
        // `TokenKind.KwFunc => return true,`). `return` is a statement,
        // not an expression, so we handle it specially before falling
        // through to the expression parser.
        let arm_tok_span = pat.span();
        let body = if matches!(stream.peek_kind(), Some(TokenKind::KwReturn)) {
            let ret_tok = stream.advance().unwrap(); // consume `return`
            let ret_expr = if matches!(
                stream.peek_kind(),
                Some(TokenKind::Comma | TokenKind::RBrace)
            ) {
                None
            } else {
                Some(parse_expression(stream)?)
            };
            arm_end = ret_expr
                .as_ref()
                .map(|e| e.span().end)
                .unwrap_or(ret_tok.span.end);
            Block {
                stmts: vec![Stmt::Return(ret_expr, ret_tok.span)],
                span: Span::new(arm_tok_span.start, arm_end, source_id),
            }
        } else {
            let body_expr = parse_expression(stream)?;
            arm_end = body_expr.span().end;
            Block {
                stmts: vec![Stmt::ExprStmt(body_expr, arm_tok_span)],
                span: Span::new(arm_tok_span.start, arm_end, source_id),
            }
        };
        arms.push(MatchArm {
            pattern: pat,
            guard,
            body,
            span: Span::new(arm_tok_span.start, arm_end, source_id),
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

/// Parse a minimal closure `{ params => expr }` whose opening `{` is the next
/// significant token (T23).
///
/// Shape: `{ ident (, ident)* => expr }`. The body is a single expression
/// (wrapped in an `ExprStmt` to form a one-statement block). Parameter types
/// are inferred (a placeholder `TypeRef` is stored; codegen ignores it for
/// closures). Full closures (typed params, multi-statement bodies, capture
/// analysis) are T34 — this minimal form covers `.map` / `.filter` / `.reduce`.
pub fn parse_closure(stream: &mut TokenStream<'_>) -> Result<Expr, ParseError> {
    use buff_lang_ast::common::{Block, Param};
    let lb = stream.expect(TokenKind::LBrace)?;
    // Parse one or more comma-separated identifier parameters.
    let mut params: Vec<Param> = Vec::new();
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
    let arrow = stream.expect(TokenKind::FatArrow)?;
    let body_expr = parse_expression(stream)?;
    let rb = stream.expect(TokenKind::RBrace)?;
    let body = Block {
        stmts: vec![buff_lang_ast::Stmt::ExprStmt(body_expr, arrow.span)],
        span: Span::new(lb.span.start, rb.span.end, stream.source_id()),
    };
    let span = Span::new(lb.span.start, rb.span.end, stream.source_id());
    Ok(Expr::Lambda {
        params,
        body,
        return_type: None,
        span,
    })
}

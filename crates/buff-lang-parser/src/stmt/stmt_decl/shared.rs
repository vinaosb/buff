//! Shared helpers: attribute parsing, path-string scanning, identifier
//! extraction, type-end spans. ITER-52 pure move from stmt_decl.rs.

use buff_lang_ast::{Attribute, Ident, TypeRef};
use buff_lang_error::{Diagnostic, ParseError, Span};
use buff_lang_lexer::{Token, TokenKind};

use crate::stream::TokenStream;

/// Parse zero-or-more leading `@name` attribute forms (T35).
///
/// Each attribute is one of:
/// - `@ident` — argument-less form (e.g. `@test`).
/// - `@ident(args, ...)` — parenthesised form (e.g. `@prefer(gpu)`). The
///   args are stored as raw identifier/string text; no type-checking is
///   done at parse time (deferred to the semantic pass).
///
/// The function consumes attributes greedily and stops as soon as the next
/// significant token is not `@`. Returns the collected attributes in
/// declaration order (leftmost first). An empty `Vec` means no attributes
/// were present (the common case).
///
/// # Errors
///
/// Returns [`ParseError`] if:
/// - `@` is not followed by an identifier (the attribute name),
/// - a parenthesised form is missing its closing `)`.
pub fn parse_attributes(stream: &mut TokenStream<'_>) -> Result<Vec<Attribute>, ParseError> {
    let source_id = stream.source_id();
    let mut attrs = Vec::new();
    while matches!(stream.peek_kind(), Some(TokenKind::At)) {
        let at_tok = stream.advance_after_peek();
        let start = at_tok.span.start;
        let name_tok = stream.advance().ok_or_else(|| {
            ParseError::new(Diagnostic::error(
                "expected attribute name after `@`, found end of input",
                stream.eof_span(),
            ))
        })?;
        let name = extract_ident(name_tok.clone())?;
        // Optional `( arg, arg, ... )` — args are bare identifiers or
        // string-literal text. Stored as raw strings for forward-compat.
        let mut args: Vec<String> = Vec::new();
        // T0-G3: named `key = "value"` args go in a separate map so
        // codegen can look up by name (e.g. `@deprecated(since = "2.0",
        // replacement = "new_fn")`). Both forms can coexist on the same
        // attribute.
        let mut named_args: std::collections::BTreeMap<String, String> =
            std::collections::BTreeMap::new();
        let end = if matches!(stream.peek_kind(), Some(TokenKind::LParen)) {
            stream.advance(); // consume `(`
            if !matches!(stream.peek_kind(), Some(TokenKind::RParen)) {
                loop {
                    let arg_tok = stream.advance().ok_or_else(|| {
                        ParseError::new(Diagnostic::error(
                            "expected attribute argument, found end of input",
                            stream.eof_span(),
                        ))
                    })?;
                    // T0-G3: detect named-arg form `ident = "string"`.
                    // The identifier becomes the key; the following `=`
                    // and string literal become the value.
                    if let TokenKind::Ident(key) = &arg_tok.kind {
                        if matches!(stream.peek_kind(), Some(TokenKind::Assign)) {
                            stream.advance(); // consume `=`
                                              // Value must be a string literal (the parser
                                              // already rejects interpolation; we re-use
                                              // the same StringStart/StringPart/StringEnd
                                              // triple walk below for the value).
                            let val_tok = stream.advance().ok_or_else(|| {
                                ParseError::new(Diagnostic::error(
                                    "expected string after `=` in named attribute argument",
                                    stream.eof_span(),
                                ))
                            })?;
                            let value = match val_tok.kind {
                                TokenKind::StringStart => {
                                    let part = stream.advance().ok_or_else(|| {
                                        ParseError::new(Diagnostic::error(
                                            "expected string content in named attribute argument",
                                            stream.eof_span(),
                                        ))
                                    })?;
                                    let s = match part.kind {
                                        TokenKind::StringPart(s) => s,
                                        other => {
                                            return Err(ParseError::new(Diagnostic::error(
                                                format!(
                                                    "expected string content in named arg, found `{other}`"
                                                ),
                                                part.span,
                                            )));
                                        }
                                    };
                                    let end_tok = stream.advance().ok_or_else(|| {
                                        ParseError::new(Diagnostic::error(
                                            "unterminated string in named attribute argument",
                                            stream.eof_span(),
                                        ))
                                    })?;
                                    if !matches!(end_tok.kind, TokenKind::StringEnd) {
                                        return Err(ParseError::new(Diagnostic::error(
                                            "string interpolation not allowed in named attribute argument",
                                            end_tok.span,
                                        )));
                                    }
                                    s
                                }
                                other => {
                                    return Err(ParseError::new(Diagnostic::error(
                                        format!(
                                            "expected string literal after `=` in named attribute argument, found `{other}`"
                                        ),
                                        val_tok.span,
                                    )));
                                }
                            };
                            named_args.insert(key.clone(), value);
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
                                            "expected `,` or `)` after named attribute argument, found `{other}`"
                                        ),
                                        stream
                                            .peek()
                                            .map(|t| t.span)
                                            .unwrap_or_else(|| stream.eof_span()),
                                    )));
                                }
                                None => {
                                    return Err(ParseError::new(Diagnostic::error(
                                        "unterminated attribute argument list (missing `)`)",
                                        stream.eof_span(),
                                    )));
                                }
                            }
                            continue;
                        }
                    }
                    let arg = match &arg_tok.kind {
                        TokenKind::Ident(s) => s.clone(),
                        // T66: accept integer literals as attribute args so
                        // `@workgroup(64)` parses (the workgroup size is a
                        // numeric value). The integer is stored as its string
                        // representation in `Attribute::args` alongside the
                        // existing identifier/string forms. This is purely
                        // additive — existing attribute forms (`@test`,
                        // `@prefer(gpu)`, `@deprecated(since = "2.0")`) are
                        // unaffected.
                        TokenKind::IntLit(n) => n.to_string(),
                        TokenKind::StringStart => {
                            // Consume the full string-token triple.
                            let part = stream.advance().ok_or_else(|| {
                                ParseError::new(Diagnostic::error(
                                    "expected string content in attribute argument",
                                    stream.eof_span(),
                                ))
                            })?;
                            let s = match part.kind {
                                TokenKind::StringPart(s) => s,
                                other => {
                                    return Err(ParseError::new(Diagnostic::error(
                                        format!(
                                            "expected string content in attribute, found `{other}`"
                                        ),
                                        part.span,
                                    )));
                                }
                            };
                            let end_tok = stream.advance().ok_or_else(|| {
                                ParseError::new(Diagnostic::error(
                                    "unterminated string in attribute argument",
                                    stream.eof_span(),
                                ))
                            })?;
                            if !matches!(end_tok.kind, TokenKind::StringEnd) {
                                return Err(ParseError::new(Diagnostic::error(
                                    "string interpolation not allowed in attribute argument",
                                    end_tok.span,
                                )));
                            }
                            s
                        }
                        other => {
                            return Err(ParseError::new(Diagnostic::error(
                                format!("expected identifier or string in attribute argument, found `{other}`"),
                                arg_tok.span,
                            )));
                        }
                    };
                    args.push(arg);
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
                                    "expected `,` or `)` in attribute arguments, found `{other}`"
                                ),
                                stream
                                    .peek()
                                    .map(|t| t.span)
                                    .unwrap_or_else(|| stream.eof_span()),
                            )));
                        }
                        None => {
                            return Err(ParseError::new(Diagnostic::error(
                                "unterminated attribute argument list (missing `)`)",
                                stream.eof_span(),
                            )));
                        }
                    }
                }
            }
            let rp = stream.expect(TokenKind::RParen)?;
            rp.span.end
        } else {
            name_tok.span.end
        };
        attrs.push(Attribute {
            name,
            args,
            named_args,
            span: Span::new(start, end, source_id),
        });
    }
    Ok(attrs)
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Expect a string-literal path token (the `"./foo.buff"` part of an
/// import/export `from` clause). Returns the path string + the end offset
/// of the closing `"`.
///
/// The Buff lexer tokenizes every `"..."` as `StringStart, StringPart,
/// StringEnd` (the interpolation machinery), even for plain non-interpolated
/// strings. We consume that three-token sequence here and reject any
/// interpolated form (`InterpStart`) inside a path — paths must be plain
/// string literals.
///
/// # Errors
///
/// Returns [`ParseError`] if:
/// - the next significant token is not `StringStart`,
/// - the `StringPart` is missing,
/// - an interpolation appears inside the path string,
/// - the closing `StringEnd` is missing.
pub(super) fn expect_path_string(
    stream: &mut TokenStream<'_>,
) -> Result<(String, usize), ParseError> {
    // Consume `StringStart`.
    let start_tok = stream.advance().ok_or_else(|| {
        ParseError::new(Diagnostic::error(
            "expected path string after `from`, found end of input",
            stream.eof_span(),
        ))
    })?;
    if !matches!(start_tok.kind, TokenKind::StringStart) {
        return Err(ParseError::new(Diagnostic::error(
            format!(
                "expected path string after `from`, found `{}`",
                start_tok.kind
            ),
            start_tok.span,
        )));
    }

    // Expect exactly one StringPart (no interpolation allowed).
    let part_tok = stream.advance().ok_or_else(|| {
        ParseError::new(Diagnostic::error(
            "expected path string content, found end of input",
            stream.eof_span(),
        ))
    })?;
    let path = match part_tok.kind {
        TokenKind::StringPart(s) => s,
        TokenKind::InterpStart => {
            return Err(ParseError::new(Diagnostic::error(
                "path string cannot contain interpolation",
                part_tok.span,
            )));
        }
        other => {
            return Err(ParseError::new(Diagnostic::error(
                format!("expected path string content, found `{other}`"),
                part_tok.span,
            )));
        }
    };

    // Consume `StringEnd`.
    let end_tok = stream.advance().ok_or_else(|| {
        ParseError::new(Diagnostic::error(
            "unterminated path string (missing closing quote)",
            stream.eof_span(),
        ))
    })?;
    if !matches!(end_tok.kind, TokenKind::StringEnd) {
        // If interpolation slipped in (InterpStart between parts), the
        // token after the part would be InterpStart, not StringEnd.
        return Err(ParseError::new(Diagnostic::error(
            format!(
                "path string cannot contain interpolation; expected end of string, found `{}`",
                end_tok.kind
            ),
            end_tok.span,
        )));
    }

    Ok((path, end_tok.span.end))
}

// ---------------------------------------------------------------------------
// T35 — Attribute parsing (`@name`).
//
// Buff attributes are `@`-prefixed identifiers preceding a declaration:
//
//   @test
//   func test_addition():
//       assert_eq(add(2, 3), 5)
//
// For v0.5 only the argument-less form `@test` is meaningful; the parser
// also accepts `@name(arg, arg)` for forward-compat with the `@prefer(gpu)`
// shape the README anticipates, storing the args as raw strings on the
// [`Attribute`] node. Attributes attach to [`FuncDecl`]s today; attaching
// them to structs/enums is a future task.
// ---------------------------------------------------------------------------

/// Pull an [`Ident`] out of a token whose kind is [`TokenKind::Ident`].
/// Errors on any other kind.
pub fn extract_ident(tok: Token) -> Result<Ident, ParseError> {
    match tok.kind {
        TokenKind::Ident(s) => Ok(Ident::new(s, tok.span)),
        other => Err(ParseError::new(Diagnostic::error(
            format!("expected identifier, found `{other}`"),
            tok.span,
        ))),
    }
}

/// End byte offset of a [`TypeRef`]'s span. Small helper so call sites don't
/// need to repeat the variant match.
pub fn type_end(ty: &TypeRef) -> usize {
    match ty {
        TypeRef::Named { span, .. }
        | TypeRef::Generic { span, .. }
        | TypeRef::Option(_, span)
        | TypeRef::Function { span, .. }
        | TypeRef::Union(_, span)
        | TypeRef::Tuple(_, span)
        | TypeRef::TraitObject { span, .. } => span.end,
    }
}

/// `const NAME[: Ty] = value` - a top-level constant declaration
/// (ITER-53C). Dispatched from `parse_one_decl` on `KwConst` (keyword
/// #31). The value is any expression; codegen requires it to lower to a
/// Rust const expression (literals today).
pub fn parse_const_decl(
    stream: &mut TokenStream<'_>,
) -> Result<buff_lang_ast::ConstDecl, ParseError> {
    use super::func::parse_type_ref;
    use crate::expr::parse_expression;
    use buff_lang_ast::ConstDecl;
    use buff_lang_error::Span;

    let source_id = stream.source_id();
    let start = stream.expect(TokenKind::KwConst)?.span.start;
    let tok = stream.advance().ok_or_else(|| {
        ParseError::new(Diagnostic::error(
            "expected constant name after `const`, found end of input",
            stream.eof_span(),
        ))
    })?;
    let name = extract_ident(tok)?;
    let ty = if matches!(stream.peek_kind(), Some(TokenKind::Colon)) {
        stream.advance();
        Some(parse_type_ref(stream)?)
    } else {
        None
    };
    stream.expect(TokenKind::Assign)?;
    let value = parse_expression(stream)?;
    let span = Span::new(start, value.span().end, source_id);
    Ok(ConstDecl {
        name,
        ty,
        value,
        span,
    })
}

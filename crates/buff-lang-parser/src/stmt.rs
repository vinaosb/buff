//! Statement parser — hand-rolled recursive-descent for Buff statements.
//!
//! Builds on T7's [`crate::expr::parse_expression`] for sub-expressions and
//! shares the same [`crate::stream::TokenStream`] cursor.
//!
//! # Supported statements (T8)
//!
//! - `let name[: Type] = expr` ([`Stmt::LetDecl`])
//! - `let mut name = expr`
//! - assignment: `target = expr`, `target += expr`, … ([`Stmt::Assignment`])
//! - bare expression as statement: `print(x)` ([`Stmt::ExprStmt`])
//! - `if cond { ... } else { ... }` — wrapped in [`Stmt::ExprStmt`] using
//!   [`Expr::IfExpr`] (if is an expression)
//! - `return expr` / bare `return` ([`Stmt::Return`])
//! - `break` / `continue`
//! - `for var in iter { ... }` iterator loop ([`Stmt::ForIn`])
//! - `for cond { ... }` conditional loop, while-style ([`Stmt::ForWhile`])
//!
//! # Function declarations
//!
//! [`parse_func_decl`] parses a top-level `func name(params) -> Ret { body }`.
//! Inside `parse_statement`, encountering `func` is an error — function
//! declarations are not statements.
//!
//! # Layout (T9)
//!
//! Two block forms coexist:
//!
//! - **Braces**: `{ stmt; stmt; ... }` — explicit, indentation-agnostic.
//! - **Layout (offside rule)**: `: NEWLINE INDENT stmt stmt ... DEDENT` —
//!   Python/F#-style indentation-sensitive blocks.
//!
//! [`parse_block`] dispatches on the upcoming raw token: a `{` delegates to
//! [`parse_block_braces`]; a `:` triggers the layout path. Layout tokens
//! (`Newline`/`Indent`/`Dedent`) are observed via the `_raw` family of
//! [`TokenStream`] methods, while statement bodies still use the regular
//! skipping peek/advance so existing parsers compose unchanged.

use buff_lang_ast::{
    BinaryOp, Block, EnumDecl, EnumVariant, Expr, FuncDecl, Ident, Param, Stmt, TypeRef,
};
use buff_lang_error::{Diagnostic, ParseError, Span};
use buff_lang_lexer::{Token, TokenKind};

use crate::expr::parse_expression;
use crate::stream::TokenStream;

// ---------------------------------------------------------------------------
// Public entry points
// ---------------------------------------------------------------------------

/// Parse a single statement.
///
/// Dispatches on the upcoming significant token:
///
/// | Token      | Result                                  |
/// |------------|------------------------------------------|
/// | `let`      | [`Stmt::LetDecl`]                        |
/// | `if`       | [`Stmt::ExprStmt`] wrapping [`Expr::IfExpr`] |
/// | `return`   | [`Stmt::Return`]                         |
/// | `break`    | [`Stmt::Break`]                          |
/// | `continue` | [`Stmt::Continue`]                       |
/// | `for`      | [`Stmt::ForIn`] or [`Stmt::ForWhile`]    |
/// | `func`     | **error** — func decls are top-level     |
/// | other      | assignment-or-expression statement       |
///
/// # Errors
///
/// Returns [`ParseError`] on any syntax error. The cursor position after an
/// error is unspecified (no error recovery yet).
pub fn parse_statement(stream: &mut TokenStream<'_>) -> Result<Stmt, ParseError> {
    match stream.peek_kind() {
        Some(TokenKind::KwLet) => parse_let(stream),
        Some(TokenKind::KwIf) => {
            // if-as-expression: parse as Expr, wrap in ExprStmt.
            let expr = parse_if_expr(stream)?;
            let span = expr.span();
            Ok(Stmt::ExprStmt(expr, span))
        }
        Some(TokenKind::KwFunc) => Err(ParseError::new(Diagnostic::error(
            "function declarations must be top-level",
            stream
                .peek()
                .map(|t| t.span)
                .unwrap_or_else(|| stream.eof_span()),
        ))),
        Some(TokenKind::KwReturn) => parse_return(stream),
        Some(TokenKind::KwBreak) => {
            let tok = stream.advance().expect("peek guaranteed a token");
            Ok(Stmt::Break(tok.span))
        }
        Some(TokenKind::KwContinue) => {
            let tok = stream.advance().expect("peek guaranteed a token");
            Ok(Stmt::Continue(tok.span))
        }
        Some(TokenKind::KwFor) => parse_for(stream),
        _ => parse_assignment_or_expr_stmt(stream),
    }
}

/// Parse a brace-delimited block of statements: `{ stmt stmt ... }`.
///
/// The opening `{` and closing `}` are consumed. Statements are separated
/// by layout (newlines) which [`TokenStream`] transparently skips, so no
/// explicit separator handling is required. An empty block `{ }` is valid
/// and produces an empty [`Block`].
///
/// T9 will add an indent/dedent-based alternative; for T8 only braces are
/// supported.
///
/// # Errors
///
/// Returns [`ParseError`] if the opening or closing brace is missing or if
/// any inner statement fails to parse.
pub fn parse_block_braces(stream: &mut TokenStream<'_>) -> Result<Block, ParseError> {
    let lbrace = stream.expect(TokenKind::LBrace)?;
    let start = lbrace.span.start;
    let source_id = stream.source_id();
    let mut stmts = Vec::new();
    while !matches!(stream.peek_kind(), Some(TokenKind::RBrace) | None) {
        stmts.push(parse_statement(stream)?);
        // Optional separator between statements (semicolon). Newlines are
        // already auto-skipped by TokenStream::peek/advance.
        if matches!(stream.peek_kind(), Some(TokenKind::Semicolon)) {
            stream.advance();
        }
    }
    let rbrace = stream.expect(TokenKind::RBrace)?;
    Ok(Block {
        stmts,
        span: Span::new(start, rbrace.span.end, source_id),
    })
}

/// Parse a block — *either* brace-delimited *or* layout-sensitive (T9).
///
/// Dispatch rules:
///
/// 1. If the next raw token is `{`, delegate to [`parse_block_braces`].
/// 2. Otherwise, expect a `:` followed by `Newline Indent ... Dedent`
///    (Python-style offside-rule block).
///
/// The layout form requires a newline immediately after `:`; single-line
/// `func foo(): expr` is not supported in T9. An empty indented block
/// (missing body after `:\n`) yields a [`ParseError`] with the message
/// `"expected indented block after ':'"`.
///
/// # Errors
///
/// Returns [`ParseError`] on any of:
/// - missing `:` when neither `{` nor `:` is present,
/// - missing newline after `:`,
/// - missing `Indent` after the newline,
/// - any malformed inner statement.
///
/// The closing `Dedent` is consumed if present but its absence is not an
/// error (the lexer's `finalize` may have collapsed trailing dedents at
/// EOF in some edge cases — defensive consumption keeps the parser robust).
pub fn parse_block(stream: &mut TokenStream<'_>) -> Result<Block, ParseError> {
    // Form 1: braces — fast path.
    if stream.check_raw(&TokenKind::LBrace) {
        return parse_block_braces(stream);
    }

    // Form 2: layout-sensitive `: NEWLINE INDENT ... DEDENT`.
    let source_id = stream.source_id();
    let colon = stream.expect(TokenKind::Colon)?;
    let start = colon.span.start;

    // Expect Newline right after the colon.
    if !stream.check_raw(&TokenKind::Newline) {
        return Err(ParseError::new(Diagnostic::error(
            "expected newline after `:` for layout block",
            stream.span_here(),
        )));
    }
    stream.advance_raw(); // consume Newline

    // Expect Indent. Stray newlines/blanks between `:` line and first indented
    // line should not happen given the lexer collapses them, but defensively
    // skip extra newlines.
    while stream.consume_newline() {}

    if !stream.consume_indent() {
        return Err(ParseError::new(Diagnostic::error(
            "expected indented block after `:`",
            stream.span_here(),
        )));
    }

    // Parse statements until we hit a Dedent or run out of tokens.
    let mut stmts: Vec<Stmt> = Vec::new();
    let mut end_off = start;
    loop {
        // Stop on Dedent.
        if stream.check_raw(&TokenKind::Dedent) {
            break;
        }
        // Stop at end of significant input.
        if stream.is_at_end() {
            break;
        }
        // Skip stray Newlines/Indents between statements (the lexer emits
        // Newline at the end of every non-blank line; extra Indents should
        // not normally appear here but we tolerate them defensively).
        if stream.check_raw(&TokenKind::Newline) {
            stream.advance_raw();
            continue;
        }
        if stream.check_raw(&TokenKind::Indent) {
            // Defensive: a stray Indent at the start of an inner statement
            // usually means nested layout — let the inner parser handle it.
            // Don't consume here; parse_statement will route via parse_block
            // when it sees the trailing `:` of the inner construct.
            // However if we see Indent without a corresponding construct,
            // silently consume to avoid an infinite loop.
            stream.advance_raw();
            continue;
        }
        let stmt = parse_statement(stream)?;
        end_off = stmt_end(&stmt).max(end_off);
        stmts.push(stmt);
    }

    // Consume the closing Dedent if present (may be absent at EOF).
    let _ = stream.consume_dedent();

    Ok(Block {
        stmts,
        span: Span::new(start, end_off, source_id),
    })
}

/// Parse a function declaration: `func name(params) -> Ret { body }`.
///
/// The leading `func` keyword is consumed here. Modifier keywords (`async`,
/// `unsafe`, `extern`) are NOT supported in T8 — they will be handled in a
/// later task. Encountering them before `func` is the caller's concern: this
/// function is normally reached via [`crate::parser::parse`] which dispatches
/// only on `KwFunc`.
///
/// # Errors
///
/// Returns [`ParseError`] on missing name, parameter list, return type
/// syntax, or body block.
pub fn parse_func_decl(stream: &mut TokenStream<'_>) -> Result<FuncDecl, ParseError> {
    let func_tok = stream.expect(TokenKind::KwFunc)?;
    let start = func_tok.span.start;
    let source_id = stream.source_id();

    // Function name
    let name_tok = stream.advance().ok_or_else(|| {
        ParseError::new(Diagnostic::error(
            "expected function name after `func`",
            stream.eof_span(),
        ))
    })?;
    let name = extract_ident(name_tok)?;

    // Parameter list ( ... )
    stream.expect(TokenKind::LParen)?;
    let params = parse_params(stream)?;
    let rparen = stream.expect(TokenKind::RParen)?;

    // Optional return type: `-> Type`
    let mut end = rparen.span.end;
    let return_type = if matches!(stream.peek_kind(), Some(TokenKind::Arrow)) {
        stream.advance(); // consume `->`
        let ty = parse_type_ref(stream)?;
        end = type_end(&ty);
        Some(ty)
    } else {
        None
    };

    // Body: brace-delimited OR layout-sensitive block (T9).
    let body = parse_block(stream)?;
    let span = Span::new(start, body.span.end.max(end), source_id);

    Ok(FuncDecl {
        name,
        params,
        return_type,
        body,
        is_async: false,
        is_unsafe: false,
        is_extern: false,
        span,
    })
}

/// Parse a type reference: a named type optionally followed by generic
/// arguments in `<...>`.
///
/// Supported forms:
/// - `Int` → [`TypeRef::Named`]
/// - `Vector<Int>` → [`TypeRef::Generic`]
/// - `Map<String, Int>` → [`TypeRef::Generic`] with multiple args
/// - Nested: `Map<String, Vector<Int>>`
///
/// `Option<T>` and function types are recognized structurally as plain
/// [`TypeRef::Generic`] / [`TypeRef::Named`] for T8 — there is no special
/// sugar yet.
///
/// # Errors
///
/// Returns [`ParseError`] if the next token is not an identifier, or if a
/// generic argument list is missing its closing `>`.
pub fn parse_type_ref(stream: &mut TokenStream<'_>) -> Result<TypeRef, ParseError> {
    let source_id = stream.source_id();
    let name_tok = stream.advance().ok_or_else(|| {
        ParseError::new(Diagnostic::error(
            "expected type name, found end of input",
            stream.eof_span(),
        ))
    })?;
    let name_end = name_tok.span.end;
    let start = name_tok.span.start;
    let name = extract_ident(name_tok)?;
    let mut ty = TypeRef::Named {
        name,
        span: Span::new(start, name_end, source_id),
    };

    // Generic arguments: `<Type1, Type2, ...>`
    if matches!(stream.peek_kind(), Some(TokenKind::Lt)) {
        stream.advance(); // consume `<`
        let mut args = Vec::new();
        let mut last_end;
        loop {
            let arg = parse_type_ref(stream)?;
            last_end = type_end(&arg);
            args.push(arg);
            match stream.peek_kind() {
                Some(TokenKind::Comma) => {
                    stream.advance();
                }
                Some(TokenKind::Gt) => {
                    stream.advance();
                    break;
                }
                Some(other) => {
                    return Err(ParseError::new(Diagnostic::error(
                        format!("expected `,` or `>` in type argument list, found `{other}`"),
                        stream
                            .peek()
                            .map(|t| t.span)
                            .unwrap_or_else(|| stream.eof_span()),
                    )));
                }
                None => {
                    return Err(ParseError::new(Diagnostic::error(
                        "unterminated type argument list (missing `>`)",
                        stream.eof_span(),
                    )));
                }
            }
        }
        ty = TypeRef::Generic {
            base: Box::new(ty),
            args,
            span: Span::new(start, last_end, source_id),
        };
    }

    Ok(ty)
}

/// Parse a function parameter list body (without the surrounding parens).
///
/// Expects the cursor to be positioned just after `(`. Stops at the upcoming
/// `)`. Parameters are comma-separated; each one is `name: Type`.
///
/// # Errors
///
/// Returns [`ParseError`] if any parameter is malformed.
pub fn parse_params(stream: &mut TokenStream<'_>) -> Result<Vec<Param>, ParseError> {
    let source_id = stream.source_id();
    let mut params = Vec::new();
    if matches!(stream.peek_kind(), Some(TokenKind::RParen)) {
        return Ok(params); // empty list
    }
    loop {
        let name_tok = stream.advance().ok_or_else(|| {
            ParseError::new(Diagnostic::error(
                "expected parameter name, found end of input",
                stream.eof_span(),
            ))
        })?;
        let start = name_tok.span.start;
        let name = extract_ident(name_tok)?;
        stream.expect(TokenKind::Colon)?;
        let ty = parse_type_ref(stream)?;
        let end = type_end(&ty);
        params.push(Param {
            name,
            ty,
            span: Span::new(start, end, source_id),
        });
        match stream.peek_kind() {
            Some(TokenKind::Comma) => {
                stream.advance();
                // Allow trailing comma: `(a: Int, b: Int,)`
                if matches!(stream.peek_kind(), Some(TokenKind::RParen)) {
                    break;
                }
            }
            Some(TokenKind::RParen) => break,
            _ => {
                return Err(ParseError::new(Diagnostic::error(
                    format!(
                        "expected `,` or `)` in parameter list, found {}",
                        stream
                            .peek_kind()
                            .map(|k| k.to_string())
                            .unwrap_or_else(|| "end of input".to_string())
                    ),
                    stream
                        .peek()
                        .map(|t| t.span)
                        .unwrap_or_else(|| stream.eof_span()),
                )));
            }
        }
    }
    Ok(params)
}

// ---------------------------------------------------------------------------
// Internal: per-statement parsers
// ---------------------------------------------------------------------------

/// `let [mut] name[: Type] = expr`
fn parse_let(stream: &mut TokenStream<'_>) -> Result<Stmt, ParseError> {
    let source_id = stream.source_id();
    let let_tok = stream.expect(TokenKind::KwLet)?;
    let start = let_tok.span.start;

    let mutable = matches!(stream.peek_kind(), Some(TokenKind::KwMut));
    if mutable {
        stream.advance();
    }

    // Expect identifier
    let name_tok = stream.advance().ok_or_else(|| {
        ParseError::new(Diagnostic::error(
            "expected identifier after `let`, found end of input",
            stream.eof_span(),
        ))
    })?;
    let name = extract_ident(name_tok)?;

    // Optional type annotation `: Type`
    let ty = if matches!(stream.peek_kind(), Some(TokenKind::Colon)) {
        stream.advance();
        Some(parse_type_ref(stream)?)
    } else {
        None
    };

    // Expect `=`
    stream.expect(TokenKind::Assign)?;

    // Value expression
    let value = parse_expression(stream)?;
    let end = value.span().end;
    let span = Span::new(start, end, source_id);
    Ok(Stmt::LetDecl {
        name,
        value,
        mutable,
        ty,
        span,
    })
}

/// `return [expr]`
fn parse_return(stream: &mut TokenStream<'_>) -> Result<Stmt, ParseError> {
    let source_id = stream.source_id();
    let ret_tok = stream.expect(TokenKind::KwReturn)?;
    let start = ret_tok.span.start;
    // No value if next is `}`, `;`, EOF, or any obvious statement terminator.
    let terminates = matches!(
        stream.peek_kind(),
        None | Some(TokenKind::RBrace) | Some(TokenKind::Semicolon)
    );
    if terminates {
        return Ok(Stmt::Return(None, ret_tok.span));
    }
    let expr = parse_expression(stream)?;
    let span = Span::new(start, expr.span().end, source_id);
    Ok(Stmt::Return(Some(expr), span))
}

/// `for var in iter { body }` or `for cond { body }`.
///
/// Disambiguation is via two-token lookahead: `IDENT in` → iterator form,
/// otherwise → conditional form.
fn parse_for(stream: &mut TokenStream<'_>) -> Result<Stmt, ParseError> {
    let source_id = stream.source_id();
    let for_tok = stream.expect(TokenKind::KwFor)?;
    let start = for_tok.span.start;

    let is_iterator = matches!(
        (stream.peek_kind(), stream.peek_second_kind()),
        (Some(TokenKind::Ident(_)), Some(TokenKind::KwIn))
    );

    if is_iterator {
        let var_tok = stream.advance().expect("peek guaranteed Ident");
        let var = extract_ident(var_tok)?;
        stream.expect(TokenKind::KwIn)?;
        let iter_expr = parse_expression(stream)?;
        let body = parse_block(stream)?;
        let span = Span::new(start, body.span.end, source_id);
        Ok(Stmt::ForIn {
            var,
            iter: iter_expr,
            body,
            span,
        })
    } else {
        let cond = parse_expression(stream)?;
        let body = parse_block(stream)?;
        let span = Span::new(start, body.span.end, source_id);
        Ok(Stmt::ForWhile { cond, body, span })
    }
}

/// Either an assignment (`x = ...`, `x += ...`) or a bare expression
/// statement (`foo()`, `print(x)`).
///
/// T7's expression parser already folds `=`/`+=`/etc. into
/// [`Expr::BinaryOp`] at the assignment-precedence level. To present these
/// as [`Stmt::Assignment`] we unwrap the resulting BinaryOp here. Any other
/// expression becomes [`Stmt::ExprStmt`].
fn parse_assignment_or_expr_stmt(stream: &mut TokenStream<'_>) -> Result<Stmt, ParseError> {
    let expr = parse_expression(stream)?;

    // If T7's expression parser already produced an assignment BinaryOp,
    // peel it apart into a Stmt::Assignment.
    if let Expr::BinaryOp { op, lhs, rhs, span } = expr {
        if matches!(
            op,
            BinaryOp::Assign
                | BinaryOp::AddAssign
                | BinaryOp::SubAssign
                | BinaryOp::MulAssign
                | BinaryOp::DivAssign
                | BinaryOp::ModAssign
        ) {
            return Ok(Stmt::Assignment {
                target: *lhs,
                op,
                value: *rhs,
                span,
            });
        }
        // Non-assignment BinaryOp falls through to ExprStmt.
        let span_field = span;
        return Ok(Stmt::ExprStmt(
            Expr::BinaryOp {
                op,
                lhs,
                rhs,
                span: span_field,
            },
            span_field,
        ));
    }

    let span = expr.span();
    Ok(Stmt::ExprStmt(expr, span))
}

/// Parse an `if` expression: `if cond BLOCK [else BLOCK | else if ...]`.
///
/// The leading `if` is consumed. The condition is parsed via
/// [`parse_expression`]; the blocks via [`parse_block`] (so both
/// brace-delimited `{ ... }` and layout-sensitive `: NEWLINE INDENT ...
/// DEDENT` forms work, per T9). `else if` chains are desugared into a
/// nested [`Expr::IfExpr`] wrapped in a single-statement block.
///
/// This is invoked from [`parse_statement`] when an `if` starts a statement
/// AND from [`crate::expr::parse_primary`] (T9) so `if` can appear inside
/// arbitrary expressions (e.g. `let x = if c { 1 } else { 2 }`).
///
/// **Dangling-else**: the recursive call inside the `else if` branch binds
/// the else to the nearest (innermost) `if` — the standard lexical-scope
/// rule. Layout blocks enforce the same: the lexer's Dedent tokens
/// naturally delimit inner-if blocks before `else` is observed.
pub fn parse_if_expr(stream: &mut TokenStream<'_>) -> Result<Expr, ParseError> {
    let source_id = stream.source_id();
    let if_tok = stream.expect(TokenKind::KwIf)?;
    let start = if_tok.span.start;

    let cond = parse_expression(stream)?;
    let then_block = parse_block(stream)?;
    let mut end = then_block.span.end;

    let else_block = if matches!(stream.peek_kind(), Some(TokenKind::KwElse)) {
        stream.advance(); // consume `else`
        if matches!(stream.peek_kind(), Some(TokenKind::KwIf)) {
            // `else if` — wrap the nested if-expr in a single-stmt block.
            let nested = parse_if_expr(stream)?;
            end = nested.span().end;
            Some(Block {
                stmts: vec![Stmt::ExprStmt(nested.clone(), nested.span())],
                span: nested.span(),
            })
        } else {
            let blk = parse_block(stream)?;
            end = blk.span.end;
            Some(blk)
        }
    } else {
        None
    };

    Ok(Expr::IfExpr {
        cond: Box::new(cond),
        then_block,
        else_block,
        span: Span::new(start, end, source_id),
    })
}

// ---------------------------------------------------------------------------
// T27 — Enum declarations.
//
// Buff enum syntax (brace form, consistent with map/struct-init "braces are
// data" rule from the README):
//
//   enum Color { Red, Green, Blue }
//   enum Shape { Circle(Float), Rect(Float, Float), Point }
//   enum Result<T, E> { Ok(T), Err(E) }
//
// Each variant is either a unit variant (`Red`) or a data-carrying tuple
// variant (`Circle(Float)`). Generic params `<T, E>` on the enum are parsed
// and stored on the decl; they are NOT validated against variant payloads at
// parse time (that is a later type-checking task). The closing `}` ends the
// span.
// ---------------------------------------------------------------------------

/// Parse a top-level `enum Name<generics> { Variant, Variant(T, U), ... }`
/// declaration (T27).
///
/// Shape:
/// - `enum Name { ... }` — non-generic enum, unit + data variants.
/// - `enum Name<T, E> { ... }` — generic enum; the `<...>` after the name
///   introduces type parameters that variants may reference in their payloads.
///
/// Each variant is one of:
/// - `Ident` — a unit variant (no payload).
/// - `Ident ( Type, Type, ... )` — a data-carrying tuple variant.
///
/// Variants are comma-separated; trailing comma is allowed. The body is
/// brace-delimited ( braces-for-data per the README convention, matching
/// map literals and struct-init). An empty body `enum Empty { }` is allowed
/// (zero variants — useful for type-level tricks and as a parsing edge case).
///
/// # Errors
///
/// Returns [`ParseError`] if:
/// - the token after `enum` is not an identifier,
/// - the opening `{` is missing,
/// - a variant name is missing or not an identifier,
/// - a variant payload type fails to parse via [`parse_type_ref`],
/// - the closing `}` is missing.
pub fn parse_enum_decl(stream: &mut TokenStream<'_>) -> Result<EnumDecl, ParseError> {
    let enum_tok = stream.expect(TokenKind::KwEnum)?;
    let start = enum_tok.span.start;
    let source_id = stream.source_id();

    // Enum name (mandatory identifier).
    let name_tok = stream.advance().ok_or_else(|| {
        ParseError::new(Diagnostic::error(
            "expected enum name after `enum`",
            stream.eof_span(),
        ))
    })?;
    let name = extract_ident(name_tok)?;

    // Optional generic parameters: `<T, E>`. These are bare identifiers (no
    // bounds, no defaults — keep v0.5 minimal). The lexer already special-cases
    // `>>` as a single token in type-arg position via `parse_type_ref`; here we
    // only need to recognise a single `>` to close the param list (since the
    // params themselves are idents, not nested type refs).
    let mut generics: Vec<Ident> = Vec::new();
    if matches!(stream.peek_kind(), Some(TokenKind::Lt)) {
        stream.advance(); // consume `<`
        loop {
            let gtok = stream.advance().ok_or_else(|| {
                ParseError::new(Diagnostic::error(
                    "expected generic parameter name, found end of input",
                    stream.eof_span(),
                ))
            })?;
            let g = extract_ident(gtok)?;
            generics.push(g);
            match stream.peek_kind() {
                Some(TokenKind::Comma) => {
                    stream.advance();
                    // Allow trailing comma: `<T,>`.
                    if matches!(stream.peek_kind(), Some(TokenKind::Gt)) {
                        stream.advance();
                        break;
                    }
                }
                Some(TokenKind::Gt) => {
                    stream.advance();
                    break;
                }
                Some(other) => {
                    return Err(ParseError::new(Diagnostic::error(
                        format!("expected `,` or `>` in generic param list, found `{other}`"),
                        stream
                            .peek()
                            .map(|t| t.span)
                            .unwrap_or_else(|| stream.eof_span()),
                    )));
                }
                None => {
                    return Err(ParseError::new(Diagnostic::error(
                        "unterminated generic param list (missing `>`)",
                        stream.eof_span(),
                    )));
                }
            }
        }
    }

    // Opening `{` of the variant list.
    stream.expect(TokenKind::LBrace)?;
    let mut variants: Vec<EnumVariant> = Vec::new();
    // Empty body: `enum Empty { }`.
    if matches!(stream.peek_kind(), Some(TokenKind::RBrace)) {
        let rb = stream.expect(TokenKind::RBrace)?;
        return Ok(EnumDecl {
            name,
            generics,
            variants,
            span: Span::new(start, rb.span.end, source_id),
        });
    }
    loop {
        // Variant name (mandatory identifier).
        let vname_tok = stream.advance().ok_or_else(|| {
            ParseError::new(Diagnostic::error(
                "expected enum variant name, found end of input",
                stream.eof_span(),
            ))
        })?;
        let vname = extract_ident(vname_tok.clone())?;
        let vstart = vname_tok.span.start;
        // Optional payload `( Type, Type, ... )`.
        let mut data: Option<Vec<TypeRef>> = None;
        if matches!(stream.peek_kind(), Some(TokenKind::LParen)) {
            stream.advance(); // consume `(`
            let mut tys: Vec<TypeRef> = Vec::new();
            // Empty payload `()` is allowed — treat as no payload (unit variant).
            if !matches!(stream.peek_kind(), Some(TokenKind::RParen)) {
                loop {
                    let ty = parse_type_ref(stream)?;
                    tys.push(ty);
                    match stream.peek_kind() {
                        Some(TokenKind::Comma) => {
                            stream.advance();
                            // Allow trailing comma.
                            if matches!(stream.peek_kind(), Some(TokenKind::RParen)) {
                                break;
                            }
                        }
                        Some(TokenKind::RParen) => break,
                        Some(other) => {
                            return Err(ParseError::new(Diagnostic::error(
                                format!("expected `,` or `)` in variant payload, found `{other}`"),
                                stream
                                    .peek()
                                    .map(|t| t.span)
                                    .unwrap_or_else(|| stream.eof_span()),
                            )));
                        }
                        None => {
                            return Err(ParseError::new(Diagnostic::error(
                                "unterminated variant payload (missing `)`)",
                                stream.eof_span(),
                            )));
                        }
                    }
                }
            }
            let rparen = stream.expect(TokenKind::RParen)?;
            // Only record the payload if it has at least one type — `()` is
            // equivalent to no payload (unit variant) for codegen purposes.
            if !tys.is_empty() {
                data = Some(tys);
            }
            // Span end of the variant covers the closing `)`.
            let vend = rparen.span.end;
            variants.push(EnumVariant {
                name: vname,
                data,
                span: Span::new(vstart, vend, source_id),
            });
        } else {
            // Unit variant (no payload).
            let vend = vname_tok.span.end;
            variants.push(EnumVariant {
                name: vname,
                data,
                span: Span::new(vstart, vend, source_id),
            });
        }
        // Comma separator or end of list.
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
                    format!("expected `,` or `}}` in enum body, found `{other}`"),
                    stream
                        .peek()
                        .map(|t| t.span)
                        .unwrap_or_else(|| stream.eof_span()),
                )));
            }
            None => {
                return Err(ParseError::new(Diagnostic::error(
                    "unterminated enum body (missing `}`)",
                    stream.eof_span(),
                )));
            }
        }
    }
    let rb = stream.expect(TokenKind::RBrace)?;
    Ok(EnumDecl {
        name,
        generics,
        variants,
        span: Span::new(start, rb.span.end, source_id),
    })
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Pull an [`Ident`] out of a token whose kind is [`TokenKind::Ident`].
/// Errors on any other kind.
fn extract_ident(tok: Token) -> Result<Ident, ParseError> {
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
fn type_end(ty: &TypeRef) -> usize {
    match ty {
        TypeRef::Named { span, .. }
        | TypeRef::Generic { span, .. }
        | TypeRef::Option(_, span)
        | TypeRef::Function { span, .. } => span.end,
    }
}

/// End byte offset of a [`Stmt`]'s span. Used by [`parse_block`] to compute
/// the parent block's end position from its last child statement.
fn stmt_end(stmt: &Stmt) -> usize {
    match stmt {
        Stmt::LetDecl { span, .. }
        | Stmt::Assignment { span, .. }
        | Stmt::ExprStmt(_, span)
        | Stmt::Return(_, span)
        | Stmt::Break(span)
        | Stmt::Continue(span)
        | Stmt::ForIn { span, .. }
        | Stmt::ForWhile { span, .. } => span.end,
    }
}

// ---------------------------------------------------------------------------
// Unit tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use buff_lang_error::SourceId;

    fn sid() -> SourceId {
        SourceId(0)
    }

    fn stream_of(src: &str) -> TokenStream<'static> {
        // Leak the tokens so we get a 'static lifetime for test ergonomics.
        let toks = buff_lang_lexer::tokenize(src, sid()).expect("lexer should succeed");
        let boxed: &'static [buff_lang_lexer::Token] = Box::leak(toks.into_boxed_slice());
        TokenStream::new(boxed, sid())
    }

    #[test]
    fn unit_let_int() {
        let mut s = stream_of("let x = 42");
        let st = parse_statement(&mut s).expect("parse let");
        match st {
            Stmt::LetDecl {
                name, mutable, ty, ..
            } => {
                assert_eq!(name.name, "x");
                assert!(!mutable);
                assert!(ty.is_none());
            }
            other => panic!("expected LetDecl, got {other:?}"),
        }
    }

    #[test]
    fn unit_break_continue() {
        let mut s = stream_of("break");
        assert!(matches!(parse_statement(&mut s), Ok(Stmt::Break(_))));
        let mut s = stream_of("continue");
        assert!(matches!(parse_statement(&mut s), Ok(Stmt::Continue(_))));
    }

    #[test]
    fn unit_func_at_stmt_level_errors() {
        let mut s = stream_of("func foo() { }");
        let err = parse_statement(&mut s).expect_err("func should error at stmt level");
        assert!(err.diagnostic.message.contains("top-level"));
    }

    #[test]
    fn unit_type_ref_named() {
        let mut s = stream_of("Int");
        let ty = parse_type_ref(&mut s).expect("parse type");
        assert!(matches!(ty, TypeRef::Named { .. }));
    }

    #[test]
    fn unit_type_ref_generic() {
        let mut s = stream_of("Vector<Int>");
        let ty = parse_type_ref(&mut s).expect("parse type");
        match ty {
            TypeRef::Generic { args, .. } => assert_eq!(args.len(), 1),
            other => panic!("expected Generic, got {other:?}"),
        }
    }
}

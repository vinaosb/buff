//! Statement parser — hand-rolled recursive-descent for Deox statements.
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
//! # Layout
//!
//! T8 only handles brace-delimited blocks `{ ... }`. Indent/Dedent-based
//! layout is T9's responsibility; [`TokenStream`] transparently skips those
//! tokens anyway, so this parser composes naturally with future layout work.

use deox_ast::{BinaryOp, Block, Expr, FuncDecl, Ident, Param, Stmt, TypeRef};
use deox_error::{Diagnostic, ParseError, Span};
use deox_lexer::{Token, TokenKind};

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

    // Body: brace-delimited block.
    let body = parse_block_braces(stream)?;
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
        let body = parse_block_braces(stream)?;
        let span = Span::new(start, body.span.end, source_id);
        Ok(Stmt::ForIn {
            var,
            iter: iter_expr,
            body,
            span,
        })
    } else {
        let cond = parse_expression(stream)?;
        let body = parse_block_braces(stream)?;
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

/// Parse an `if` expression: `if cond { then } [else { else } | else if ...]`.
///
/// The leading `if` is consumed. The condition is parsed via
/// [`parse_expression`]; the blocks via [`parse_block_braces`]. `else if`
/// chains are desugared into a nested [`Expr::IfExpr`] wrapped in a
/// single-statement block.
///
/// This is invoked from [`parse_statement`] when an `if` starts a statement.
/// In T8, if-expressions are NOT yet wired into [`crate::expr::parse_primary`],
/// so they cannot appear nested inside other expressions (e.g. inside
/// `let x = if c { 1 } else { 2 }`). That integration arrives in a later task.
pub fn parse_if_expr(stream: &mut TokenStream<'_>) -> Result<Expr, ParseError> {
    let source_id = stream.source_id();
    let if_tok = stream.expect(TokenKind::KwIf)?;
    let start = if_tok.span.start;

    let cond = parse_expression(stream)?;
    let then_block = parse_block_braces(stream)?;
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
            let blk = parse_block_braces(stream)?;
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

// ---------------------------------------------------------------------------
// Unit tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use deox_error::SourceId;

    fn sid() -> SourceId {
        SourceId(0)
    }

    fn stream_of(src: &str) -> TokenStream<'static> {
        // Leak the tokens so we get a 'static lifetime for test ergonomics.
        let toks = deox_lexer::tokenize(src, sid()).expect("lexer should succeed");
        let boxed: &'static [deox_lexer::Token] = Box::leak(toks.into_boxed_slice());
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

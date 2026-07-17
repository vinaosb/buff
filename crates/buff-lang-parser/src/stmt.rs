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
    Attribute, BinaryOp, Block, Decl, EnumDecl, EnumVariant, ExportDecl, Expr, FuncDecl,
    GuardCondition, Ident, ImportDecl, Param, Pattern, ReexportDecl, Stmt, TypeRef,
};
use buff_lang_error::{Diagnostic, ParseError, SourceId, Span};
use buff_lang_lexer::{Token, TokenKind};

use crate::expr::{parse_expression, parse_pattern};
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
        Some(TokenKind::KwGuard) => parse_guard(stream),
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
/// The leading `func` keyword is consumed here. Modifier keywords
/// (`async`, `extern`) preceding `func` are consumed here too (T31 added
/// `async`; T32 added `extern`). Encountering `unsafe` before `func` is the
/// caller's concern (not yet wired through the dispatcher). This function
/// is normally reached via [`crate::parser::parse`] which dispatches on
/// `KwFunc`, `KwAsync`+`KwFunc`, `KwExtern`+`KwFunc`, or `At`+...+`KwFunc`
/// (T35 — attributes).
///
/// # T31 — `async func` modifier
///
/// When this function is called with the cursor positioned at `KwAsync`
/// (the dispatcher routes `async func` here), it consumes the `async`
/// keyword and sets `is_async = true` on the resulting [`FuncDecl`].
/// Otherwise (`KwFunc` is the first token) `is_async` stays `false`.
/// Either way the `func` keyword must follow (and is consumed here).
///
/// # T32 — `extern func` modifier (FFI)
///
/// When the dispatcher routes `extern func` here, the leading `extern`
/// keyword is consumed and `is_extern = true` is set. **Extern funcs have
/// NO body** (they are foreign-function declarations); after parsing the
/// signature (`name(params) -> Ret`) the parser DOES NOT expect a block.
/// The codegen lowers an `is_extern` FuncDecl to a Rust
/// `extern "C" { fn name(params) -> Ret; }` foreign-mod item (the empty
/// placeholder [`Block`] stored on the AST is dropped at codegen time).
///
/// # T35 — `attributes` parameter
///
/// The caller may pass a `Vec<Attribute>` of already-parsed leading `@name`
/// attributes (collected by the top-level dispatcher when it saw `@` before
/// the function). These are attached verbatim to the resulting [`FuncDecl`].
/// The vast majority of call sites pass `Vec::new()` (no attributes).
///
/// # Errors
///
/// Returns [`ParseError`] on missing name, parameter list, return type
/// syntax, or (for non-extern funcs) body block.
pub fn parse_func_decl(
    stream: &mut TokenStream<'_>,
    attributes: Vec<Attribute>,
) -> Result<FuncDecl, ParseError> {
    // T32: consume the optional leading `extern` modifier (FFI declaration).
    let is_extern = if matches!(stream.peek_kind(), Some(TokenKind::KwExtern)) {
        let extern_tok = stream.advance().expect("peek guaranteed KwExtern");
        let _ = extern_tok; // span tracking not needed for v0.5
        true
    } else {
        false
    };
    // T31: consume the optional leading `async` modifier.
    let is_async = if matches!(stream.peek_kind(), Some(TokenKind::KwAsync)) {
        let async_tok = stream.advance().expect("peek guaranteed KwAsync");
        let _ = async_tok; // span tracking not needed for v0.5
        true
    } else {
        false
    };
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

    // Body: brace-delimited OR layout-sensitive block (T9), OR expression
    // function shorthand `=>` (T102).
    //
    // T102: `func f(x) => EXPR` is syntactic sugar for
    // `func f(x) { return EXPR }`. If the next token is `=>`, consume it,
    // parse a single expression, and synthesize a Block whose single
    // statement is `return EXPR`.
    //
    // T32: extern funcs are foreign-function declarations and have NO
    // body — synthesize an empty placeholder Block whose span ends at the
    // signature. The codegen detects `is_extern` and emits a Rust
    // `extern "C" { fn ...; }` foreign-mod item instead of a body-having
    // `ItemFn`, so the placeholder is never rendered.
    let body = if is_extern {
        Block {
            stmts: Vec::new(),
            span: Span::new(end, end, source_id),
        }
    } else if matches!(stream.peek_kind(), Some(TokenKind::FatArrow)) {
        // T102: expression function shorthand `=>`.
        let arrow_tok = stream.advance().ok_or_else(|| {
            ParseError::new(Diagnostic::error(
                "expected `=>` after function signature",
                stream.eof_span(),
            ))
        })?;
        let expr = parse_expression(stream)?;
        let expr_end = expr.span().end;
        let ret_stmt = Stmt::Return(
            Some(expr),
            Span::new(arrow_tok.span.start, expr_end, source_id),
        );
        Block {
            stmts: vec![ret_stmt],
            span: Span::new(arrow_tok.span.start, expr_end, source_id),
        }
    } else {
        parse_block(stream)?
    };
    let span = Span::new(start, body.span.end.max(end), source_id);

    Ok(FuncDecl {
        name,
        params,
        return_type,
        body,
        is_async,
        is_unsafe: false,
        is_extern,
        attributes,
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

    // T71: destructuring dispatch. Two shapes route to `Stmt::LetPattern`
    // (the existing bare-name `Stmt::LetDecl` path is left 100% untouched):
    // - `let (x, y) = ...`        → tuple pattern (next token is `(`).
    // - `let Point { x, y } = ...` → struct pattern (next is an `Ident`
    //   immediately followed by `{`; in `let`-target position `Ident {` can
    //   ONLY be a struct destructuring pattern — a struct literal can't be a
    //   binding target).
    let is_tuple_pat = matches!(stream.peek_kind(), Some(TokenKind::LParen));
    let is_struct_pat = matches!(
        (stream.peek_kind(), stream.peek_second_kind()),
        (Some(TokenKind::Ident(_)), Some(TokenKind::LBrace))
    );
    if is_tuple_pat || is_struct_pat {
        let pattern = parse_pattern(stream)?;
        // Optional type annotation `: Type` (rare for destructuring, but the
        // AST carries the field so we honour it for parity with `LetDecl`).
        let ty = if matches!(stream.peek_kind(), Some(TokenKind::Colon)) {
            stream.advance();
            Some(parse_type_ref(stream)?)
        } else {
            None
        };
        stream.expect(TokenKind::Assign)?;
        let value = parse_expression(stream)?;
        let end = value.span().end;
        let span = Span::new(start, end, source_id);
        return Ok(Stmt::LetPattern {
            pattern,
            value,
            mutable,
            ty,
            span,
        });
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

    // T72: `for let PATTERN = EXPR { body }` — detect the `let` keyword
    // immediately after `for` and route to the looping-binding path. The
    // existing iterator-form (`for v in iter`) and conditional-form
    // (`for cond`) paths stay 100% untouched. Single let-binding only;
    // let-chains are T74.
    if matches!(stream.peek_kind(), Some(TokenKind::KwLet)) {
        return parse_for_let(stream, start, source_id);
    }

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

/// Parse the looping-binding form `for let PATTERN = EXPR { body }` (T72).
/// The leading `for` is already consumed; the cursor is positioned at the
/// `let` keyword. `start` is the `for`'s start offset; `source_id` is the
/// current source.
///
/// Shape: `let PATTERN = EXPR`. The PATTERN is parsed via the shared
/// [`parse_pattern`] (same one `match` arms, T71 destructuring, and T72
/// if-let use), so `for let Some(x) = iter.next()`,
/// `for let (a, b) = pair_stream.next()`, etc. all work. The `=` is
/// mandatory. The EXPR is re-evaluated each iteration; when it stops
/// matching the pattern, the loop terminates.
///
/// Codegen lowers this to Rust's `while let PAT = EXPR { body }` (Buff
/// spells it `for let` because `while` is NOT a reserved Buff keyword and
/// the loop reads like the iterator-form `for v in iter`).
///
/// Single let-binding only. Let-chains are T74.
fn parse_for_let(
    stream: &mut TokenStream<'_>,
    start: usize,
    source_id: SourceId,
) -> Result<Stmt, ParseError> {
    // Consume `let`.
    let _let_tok = stream.expect(TokenKind::KwLet)?;
    // Parse the pattern.
    let pattern = parse_pattern(stream)?;
    // Expect `=`.
    stream.expect(TokenKind::Assign)?;
    // Parse the value expression.
    let value = parse_expression(stream)?;
    // Parse the body block.
    let body = parse_block(stream)?;
    let span = Span::new(start, body.span.end, source_id);
    Ok(Stmt::ForLet {
        pattern,
        value,
        body,
        span,
    })
}

/// Parse an early-return guard: `guard <cond>[, <cond>...] else { block }`
/// (T73).
///
/// Shape:
/// - `guard BOOL_EXPR else { ... }` — boolean condition.
/// - `guard let PATTERN = EXPR else { ... }` — let-binding (introduces the
///   pattern's names into the enclosing scope when the guard passes).
/// - `guard let PATTERN = EXPR, BOOL_EXPR, ... else { ... }` — multiple
///   comma-separated conditions (mixed let/bool); ALL must succeed for the
///   guard to pass.
///
/// The leading `guard` is consumed here. Conditions are parsed left-to-right;
/// for each, a peek of `let` routes to the let-binding path (reusing the
/// shared [`parse_pattern`], same one match/T71-destructuring/T72-if-let use),
/// anything else is parsed as a boolean expression via [`parse_expression`].
/// The separator is `,` (comma). The list ends at the mandatory `else`
/// keyword. The else-block is parsed via [`parse_block`] (brace OR layout).
///
/// Codegen emits, IN ORDER, one Rust statement per condition:
/// - `let PATTERN = EXPR else { ... };` (Rust let-else; bindings stay in
///   scope — that's the whole point of guard).
/// - `if !(BOOL_EXPR) { ... }` (negated condition; the else-block runs when
///   the original is false).
///
/// # Errors
///
/// Returns [`ParseError`] on:
/// - missing `else` after the condition list,
/// - empty condition list (`guard else { ... }`),
/// - malformed pattern / expression / else-block.
fn parse_guard(stream: &mut TokenStream<'_>) -> Result<Stmt, ParseError> {
    let source_id = stream.source_id();
    let guard_tok = stream.expect(TokenKind::KwGuard)?;
    let start = guard_tok.span.start;

    // Parse a comma-separated list of conditions. Stop at `else`.
    //
    // Shape: `cond ("," cond)* ","? "else"`. The first condition is
    // mandatory (an empty `guard else { ... }` is a parse error). After
    // each condition we accept either `,` (with optional trailing comma
    // before `else`) or `else` directly.
    let mut conditions: Vec<GuardCondition> = Vec::new();
    loop {
        // After the first condition, the next token must be `,` or `else`.
        if !conditions.is_empty() {
            match stream.peek_kind() {
                Some(TokenKind::KwElse) => break,
                Some(TokenKind::Comma) => {
                    stream.advance(); // consume `,`
                                      // Trailing comma: `guard a, else { ... }` is allowed.
                    if matches!(stream.peek_kind(), Some(TokenKind::KwElse)) {
                        break;
                    }
                }
                Some(other) => {
                    return Err(ParseError::new(Diagnostic::error(
                        format!("expected `,` or `else` in guard, found `{other}`"),
                        stream
                            .peek()
                            .map(|t| t.span)
                            .unwrap_or_else(|| stream.eof_span()),
                    )));
                }
                None => {
                    return Err(ParseError::new(Diagnostic::error(
                        "unterminated guard (missing `else`)",
                        stream.eof_span(),
                    )));
                }
            }
        }
        // The next token starts a condition (or it's `else`/EOF, which is
        // an error: empty condition list, or trailing-comma-only).
        if matches!(stream.peek_kind(), Some(TokenKind::KwElse)) | stream.peek_kind().is_none() {
            return Err(ParseError::new(Diagnostic::error(
                "expected at least one condition after `guard`",
                stream
                    .peek()
                    .map(|t| t.span)
                    .unwrap_or_else(|| stream.eof_span()),
            )));
        }
        // Parse one condition: either `let PATTERN = EXPR` or a BOOL expr.
        let cond = if matches!(stream.peek_kind(), Some(TokenKind::KwLet)) {
            let let_tok = stream.expect(TokenKind::KwLet)?;
            let cond_start = let_tok.span.start;
            let pattern = parse_pattern(stream)?;
            stream.expect(TokenKind::Assign)?;
            let value = parse_expression(stream)?;
            let end = value.span().end;
            GuardCondition::Let {
                pattern,
                value,
                span: Span::new(cond_start, end, source_id),
            }
        } else {
            let expr = parse_expression(stream)?;
            GuardCondition::Bool(expr)
        };
        conditions.push(cond);
    }

    // Consume `else`.
    stream.expect(TokenKind::KwElse)?;

    // Parse the else-block (brace OR layout form).
    let else_block = parse_block(stream)?;
    let span = Span::new(start, else_block.span.end, source_id);
    Ok(Stmt::Guard {
        conditions,
        else_block,
        span,
    })
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
///
/// **T74 — let-chains**: the condition may be a comma-separated list of
/// conditions, each either `let PATTERN = EXPR` or a boolean `EXPR`. A
/// multi-condition `if` desugars to NESTED single-condition [`Expr::IfLet`]
/// / [`Expr::IfExpr`] via [`fold_if_chain`] (see its docs for the exact
/// nesting shape and else-block replication). A single-condition `if`
/// produces the identical AST shape as before T74 (zero regression).
pub fn parse_if_expr(stream: &mut TokenStream<'_>) -> Result<Expr, ParseError> {
    let source_id = stream.source_id();
    let if_tok = stream.expect(TokenKind::KwIf)?;
    let start = if_tok.span.start;

    // Parse a comma-separated list of conditions. Each is either
    // `let PATTERN = EXPR` (a binding condition) or a boolean `EXPR`. Stop
    // at the then-block starter (`{` or `:` for the layout form) or `else`.
    //
    // A single condition reproduces the pre-T74 behaviour exactly. Two or
    // more conditions trigger the nested-desugar in [`fold_if_chain`].
    let mut conditions: Vec<IfCondition> = Vec::new();
    loop {
        // After the first condition, expect `,` (with optional trailing
        // comma before the block/else) or stop at the block/else directly.
        if !conditions.is_empty() {
            match stream.peek_kind() {
                Some(TokenKind::Comma) => {
                    stream.advance(); // consume `,`
                                      // Trailing comma: `if a, { }` / `if a, else { }`.
                    if matches!(
                        stream.peek_kind(),
                        Some(TokenKind::LBrace) | Some(TokenKind::Colon) | Some(TokenKind::KwElse)
                    ) {
                        break;
                    }
                }
                Some(TokenKind::LBrace) | Some(TokenKind::Colon) | Some(TokenKind::KwElse) => break,
                Some(other) => {
                    return Err(ParseError::new(Diagnostic::error(
                        format!("expected `,`, `{{`, or `else` in if-chain, found `{other}`"),
                        stream
                            .peek()
                            .map(|t| t.span)
                            .unwrap_or_else(|| stream.eof_span()),
                    )));
                }
                None => {
                    return Err(ParseError::new(Diagnostic::error(
                        "unterminated if (missing block)",
                        stream.eof_span(),
                    )));
                }
            }
        }
        // Parse one condition. `let` starts a binding condition; anything
        // else is a boolean expression.
        let cond = if matches!(stream.peek_kind(), Some(TokenKind::KwLet)) {
            let let_tok = stream.expect(TokenKind::KwLet)?;
            let cond_start = let_tok.span.start;
            let pattern = parse_pattern(stream)?;
            stream.expect(TokenKind::Assign)?;
            let value = parse_expression(stream)?;
            let end = value.span().end;
            IfCondition::Let {
                pattern,
                value,
                span: Span::new(cond_start, end, source_id),
            }
        } else {
            let expr = parse_expression(stream)?;
            IfCondition::Bool(expr)
        };
        conditions.push(cond);
    }

    // Then-block.
    let then_block = parse_block(stream)?;
    let mut end = then_block.span.end;

    // Optional else: `else BLOCK` or `else if ...` (chains).
    let else_block = if matches!(stream.peek_kind(), Some(TokenKind::KwElse)) {
        stream.advance(); // consume `else`
        if matches!(stream.peek_kind(), Some(TokenKind::KwIf)) {
            // `else if ...` — recurse (handles both `else if let` chains and
            // plain `else if`). Wrap the nested if-expr in a single-stmt
            // block, matching the plain IfExpr path's shape.
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

    // Fold the conditions into a (possibly nested) IfLet/IfExpr chain.
    // `end` carries the overall span end (includes the else block if present).
    Ok(fold_if_chain(
        conditions, then_block, else_block, start, end, source_id,
    ))
}

/// Parser-internal accumulator for one condition in a T74 let-chain (`if cond,
/// cond, ... { }`). Mirrors the shape of [`buff_lang_ast::GuardCondition`]
/// (T73) but kept parser-local to avoid semantic conflation with the `guard`
/// statement. The chain is desugared to nested [`Expr::IfLet`] / [`Expr::IfExpr`]
/// by [`fold_if_chain`], so this type never appears in the final AST.
#[derive(Debug, Clone)]
enum IfCondition {
    /// `let PATTERN = EXPR` — a binding condition. On match the pattern's
    /// bindings are in scope for later conditions and the then-block.
    Let {
        pattern: Pattern,
        value: Expr,
        span: Span,
    },
    /// A boolean condition (`x > 0`, `flag`, …).
    Bool(Expr),
}

/// Fold a comma-separated list of if-conditions into a (possibly nested)
/// chain of single-condition [`Expr::IfLet`] / [`Expr::IfExpr`] (T74).
///
/// The desugar nests from the OUTER condition inward:
///
/// ```text
/// if c1, c2, c3 { BODY } else { ELSE }
/// ```
///
/// becomes
///
/// ```text
/// if c1 {
///     if c2 {
///         if c3 {
///             BODY
///         } else { ELSE }
///     } else { ELSE }
/// } else { ELSE }
/// ```
///
/// where each `ci` is `let PATTERN = EXPR` (→ [`Expr::IfLet`]) or a boolean
/// `EXPR` (→ [`Expr::IfExpr`]).
///
/// **Else-block replication**: when an `else` is present, it is cloned at
/// EVERY nesting level so ANY failing condition triggers it. This is
/// semantically equivalent to a single shared else (each failing condition
/// independently runs the same user-written else) and is the simplest path
/// that avoids control-flow-graph reshaping. [`Block`] derives `Clone`, so
/// the clone is cheap and correct. When there is NO else, each level gets
/// `else_block: None` (Rust `if let ... { if let ... { body } }`).
///
/// **Single condition**: when `conditions` has exactly one element the fold
/// produces a single flat [`Expr::IfLet`] or [`Expr::IfExpr`] whose shape is
/// byte-identical to the pre-T74 single-condition `if`/`if let` — zero
/// regression for existing programs.
///
/// `conditions` MUST be non-empty (the caller, [`parse_if_expr`], parses at
/// least one condition or returns a [`ParseError`] first). This invariant is
/// upheld by the peel-then-fold structure below (the first element is always
/// consumed as the outermost), so no `unwrap`/`expect`/`unreachable!` is
/// needed.
fn fold_if_chain(
    conditions: Vec<IfCondition>,
    then_block: Block,
    else_block: Option<Block>,
    start: usize,
    end: usize,
    source_id: SourceId,
) -> Expr {
    // Peel the FIRST condition (the outermost). Fold the REMAINING conditions
    // (indices 1..) into the then-block, building from the INNERMOST (last)
    // outward; then wrap the first condition around the folded body.
    let mut conds = conditions;
    let outermost = conds.remove(0);
    // Remaining conditions, innermost-first (reversed) for the fold.
    let inner_conds_rev: Vec<IfCondition> = conds.into_iter().rev().collect();

    // `body_block` is the then-block for the level currently being built.
    // It starts as the original then-block (the innermost body) and is
    // replaced by a wrapper block after each inner condition is folded in.
    let mut body_block = then_block;
    let mut body_end = body_block.span.end;

    for cond in inner_conds_rev {
        // The else for THIS level: a clone of the original (replicated).
        let level_else = else_block.clone();
        let cond_end = match &cond {
            IfCondition::Let { span, .. } => span.end,
            IfCondition::Bool(e) => e.span().end,
        };
        let level_end = body_end.max(cond_end);
        let expr = match cond {
            IfCondition::Let { pattern, value, .. } => Expr::IfLet {
                pattern,
                value: Box::new(value),
                then_block: body_block,
                else_block: level_else,
                span: Span::new(start, level_end, source_id),
            },
            IfCondition::Bool(e) => Expr::IfExpr {
                cond: Box::new(e),
                then_block: body_block,
                else_block: level_else,
                span: Span::new(start, level_end, source_id),
            },
        };
        let es = expr.span();
        // Wrap the built expression as the then-block for the NEXT (outer)
        // level. (Mirrors how `else if` wraps its nested if in a single-
        // statement block.)
        body_block = Block {
            stmts: vec![Stmt::ExprStmt(expr, es)],
            span: es,
        };
        body_end = es.end;
    }

    // Build the outermost condition wrapping the folded body. The outermost
    // span end is the overall `end` (which includes the else block if any).
    let outermost_else = else_block; // original moved here (last consumer)
    match outermost {
        IfCondition::Let { pattern, value, .. } => Expr::IfLet {
            pattern,
            value: Box::new(value),
            then_block: body_block,
            else_block: outermost_else,
            span: Span::new(start, end, source_id),
        },
        IfCondition::Bool(e) => Expr::IfExpr {
            cond: Box::new(e),
            then_block: body_block,
            else_block: outermost_else,
            span: Span::new(start, end, source_id),
        },
    }
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
// T29 — Import / Export declarations.
//
// Buff v0.5 module system syntax (ES6-style):
//
//   import { greet, farewell } from "./hello.buff"
//   import * from "./utils.buff"
//   import greet from "./hello.buff"            (default import — sugar
//                                                 for `import { default as greet }`)
//   export func public() { ... }
//   export enum Color { Red, Green, Blue }
//   export * from "./other.buff"
//   export { greet } from "./other.buff"
//
// Visibility rules:
// - `export <decl>` wraps the decl in `Decl::ExportDecl` and marks it PUBLIC.
// - Any top-level decl NOT wrapped in `export` is module-PRIVATE.
// - `export * from "..."` re-exports ALL of the target module's public
//   symbols through this module.
// - `export { names } from "..."` re-exports specific named symbols.
//
// Path resolution & cycle detection happen in the module-graph pass
// (`buff_lang_types::modules`), not here.
// ---------------------------------------------------------------------------

/// Parse an `import` declaration (T29).
///
/// Supported shapes:
/// - `import { a, b } from "./path"` — ES6 named imports.
/// - `import * from "./path"` — wildcard import.
/// - `import name from "./path"` — default import (one identifier, no
///   braces). Sugar stored as `imports: [name]`.
/// - `import a.b.c [as alias]` — legacy dotted module path.
///
/// The leading `import` keyword is consumed here.
///
/// # Errors
///
/// Returns [`ParseError`] on malformed shapes (missing braces, missing
/// `from`, missing path string, missing `}`).
pub fn parse_import_decl(stream: &mut TokenStream<'_>) -> Result<ImportDecl, ParseError> {
    let source_id = stream.source_id();
    let import_tok = stream.expect(TokenKind::KwImport)?;
    let start = import_tok.span.start;

    // Disambiguate by peeking at the next significant token:
    //   `*`    → wildcard ES6
    //   `{`    → named ES6
    //   Ident  → either default-import ES6 (followed by `from`) or legacy
    //            dotted path (followed by `.` or `as`/EOF)
    //   String → not valid syntactically; fall through to error
    let next = stream.peek_kind();
    match next {
        Some(TokenKind::Star) => {
            // `import * from "<path>"`
            stream.advance(); // consume `*`
            stream.expect(TokenKind::KwFrom)?;
            let (path_str, path_end) = expect_path_string(stream)?;
            Ok(ImportDecl {
                path: Vec::new(),
                imports: Vec::new(),
                alias: None,
                from_path: Some(path_str),
                wildcard: true,
                span: Span::new(start, path_end, source_id),
            })
        }
        Some(TokenKind::LBrace) => {
            // `import { a, b } from "<path>"`
            stream.advance(); // consume `{`
            let mut imports: Vec<Ident> = Vec::new();
            // Empty `{}` is allowed (rare but valid).
            if !matches!(stream.peek_kind(), Some(TokenKind::RBrace)) {
                loop {
                    let tok = stream.advance().ok_or_else(|| {
                        ParseError::new(Diagnostic::error(
                            "expected import name, found end of input",
                            stream.eof_span(),
                        ))
                    })?;
                    imports.push(extract_ident(tok)?);
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
                                format!("expected `,` or `}}` in import list, found `{other}`"),
                                stream
                                    .peek()
                                    .map(|t| t.span)
                                    .unwrap_or_else(|| stream.eof_span()),
                            )));
                        }
                        None => {
                            return Err(ParseError::new(Diagnostic::error(
                                "unterminated import list (missing `}`)",
                                stream.eof_span(),
                            )));
                        }
                    }
                }
            }
            let rb = stream.expect(TokenKind::RBrace)?;
            let after_brace = stream.peek_kind();
            // `from` is required after the closing brace.
            if !matches!(after_brace, Some(TokenKind::KwFrom)) {
                return Err(ParseError::new(Diagnostic::error(
                    format!(
                        "expected `from` after import list, found `{}`",
                        after_brace
                            .map(|k| k.to_string())
                            .unwrap_or_else(|| "end of input".into())
                    ),
                    stream
                        .peek()
                        .map(|t| t.span)
                        .unwrap_or_else(|| stream.eof_span()),
                )));
            }
            stream.advance(); // consume `from`
            let (path_str, path_end) = expect_path_string(stream)?;
            Ok(ImportDecl {
                path: Vec::new(),
                imports,
                alias: None,
                from_path: Some(path_str),
                wildcard: false,
                span: Span::new(start, path_end.max(rb.span.end), source_id),
            })
        }
        Some(TokenKind::Ident(_)) => {
            // Either ES6 default-import (`ident from "..."`) or legacy
            // dotted module path (`ident.ident...`).
            let ident_tok = stream.advance().expect("peek guaranteed Ident");
            let ident = extract_ident(ident_tok.clone())?;
            match stream.peek_kind() {
                Some(TokenKind::KwFrom) => {
                    // `import name from "..."` — default import.
                    stream.advance(); // consume `from`
                    let (path_str, path_end) = expect_path_string(stream)?;
                    Ok(ImportDecl {
                        path: Vec::new(),
                        imports: vec![ident],
                        alias: None,
                        from_path: Some(path_str),
                        wildcard: false,
                        span: Span::new(start, path_end, source_id),
                    })
                }
                Some(TokenKind::Dot) => {
                    // Legacy: `import a.b.c [as alias]`.
                    let mut path = vec![ident];
                    while matches!(stream.peek_kind(), Some(TokenKind::Dot)) {
                        stream.advance(); // consume `.`
                        let tok = stream.advance().ok_or_else(|| {
                            ParseError::new(Diagnostic::error(
                                "expected module path segment after `.`, found end of input",
                                stream.eof_span(),
                            ))
                        })?;
                        path.push(extract_ident(tok)?);
                    }
                    let (alias, alias_end) = if matches!(stream.peek_kind(), Some(TokenKind::KwAs))
                    {
                        stream.advance(); // consume `as`
                        let a = stream.advance().ok_or_else(|| {
                            ParseError::new(Diagnostic::error(
                                "expected alias name after `as`, found end of input",
                                stream.eof_span(),
                            ))
                        })?;
                        let end = a.span.end;
                        let a = extract_ident(a)?;
                        (Some(a), end)
                    } else {
                        (None, path.last().map(|i| i.span.end).unwrap_or(start))
                    };
                    let end = alias_end.max(path.last().map(|i| i.span.end).unwrap_or(start));
                    Ok(ImportDecl {
                        path,
                        imports: Vec::new(),
                        alias,
                        from_path: None,
                        wildcard: false,
                        span: Span::new(start, end, source_id),
                    })
                }
                Some(other) => Err(ParseError::new(Diagnostic::error(
                    format!("expected `from` or `.` in import, found `{other}`"),
                    stream
                        .peek()
                        .map(|t| t.span)
                        .unwrap_or_else(|| stream.eof_span()),
                ))),
                None => {
                    // `import name` with nothing after — treat as legacy
                    // single-segment module path (rare, but allow).
                    let end = ident.span.end;
                    Ok(ImportDecl {
                        path: vec![ident],
                        imports: Vec::new(),
                        alias: None,
                        from_path: None,
                        wildcard: false,
                        span: Span::new(start, end, source_id),
                    })
                }
            }
        }
        other => Err(ParseError::new(Diagnostic::error(
            format!(
                "expected `*`, `{{`, or identifier after `import`, found `{}`",
                other
                    .map(|k| k.to_string())
                    .unwrap_or_else(|| "end of input".into())
            ),
            stream
                .peek()
                .map(|t| t.span)
                .unwrap_or_else(|| stream.eof_span()),
        ))),
    }
}

/// Parse an `export` declaration (T29).
///
/// Supported shapes:
/// - `export func ...` / `export enum ...` — wraps the inner decl in
///   [`Decl::ExportDecl`].
/// - `export * from "..."` — wildcard re-export → [`Decl::ReexportDecl`].
/// - `export { a, b } from "..."` — named re-export → [`Decl::ReexportDecl`].
/// - `export { a, b }` (no `from`) — names a local symbol for export without
///   re-export (also [`Decl::ReexportDecl`] with empty `from`).
///
/// The leading `export` keyword is consumed here.
///
/// # Errors
///
/// Returns [`ParseError`] on:
/// - `export export`, `export import` (nested/non-exportable forms),
/// - missing `from` after `export { ... }` followed by string path,
/// - missing path string after `from`,
/// - missing `}` in named list.
pub fn parse_export_decl(stream: &mut TokenStream<'_>) -> Result<Decl, ParseError> {
    let source_id = stream.source_id();
    let export_tok = stream.expect(TokenKind::KwExport)?;
    let start = export_tok.span.start;

    match stream.peek_kind() {
        // `export func ...` → wrap FuncDecl in ExportDecl.
        Some(TokenKind::KwFunc) => {
            let f = parse_func_decl(stream, Vec::new())?;
            let span = f.span;
            Ok(Decl::ExportDecl(ExportDecl {
                inner: Box::new(Decl::FuncDecl(f)),
                span,
            }))
        }
        // T31: `export async func ...` → wrap an async FuncDecl in ExportDecl.
        // The dispatcher reaches this arm when the user wrote `export async
        // func name(...)`; `parse_func_decl` consumes the leading `async`
        // and sets `is_async = true` on the inner FuncDecl.
        Some(TokenKind::KwAsync)
            if matches!(stream.peek_second_kind(), Some(TokenKind::KwFunc)) =>
        {
            let f = parse_func_decl(stream, Vec::new())?;
            let span = f.span;
            Ok(Decl::ExportDecl(ExportDecl {
                inner: Box::new(Decl::FuncDecl(f)),
                span,
            }))
        }
        // `export enum ...` → wrap EnumDecl.
        Some(TokenKind::KwEnum) => {
            let e = parse_enum_decl(stream)?;
            let span = e.span;
            Ok(Decl::ExportDecl(ExportDecl {
                inner: Box::new(Decl::EnumDecl(e)),
                span,
            }))
        }
        // `export * from "..."` → wildcard ReexportDecl.
        Some(TokenKind::Star) => {
            stream.advance(); // consume `*`
            stream.expect(TokenKind::KwFrom)?;
            let (path_str, path_end) = expect_path_string(stream)?;
            Ok(Decl::ReexportDecl(ReexportDecl {
                from: path_str,
                names: Vec::new(),
                wildcard: true,
                span: Span::new(start, path_end, source_id),
            }))
        }
        // `export { a, b } [from "..."]` → named ReexportDecl.
        Some(TokenKind::LBrace) => {
            stream.advance(); // consume `{`
            let mut names: Vec<Ident> = Vec::new();
            if !matches!(stream.peek_kind(), Some(TokenKind::RBrace)) {
                loop {
                    let tok = stream.advance().ok_or_else(|| {
                        ParseError::new(Diagnostic::error(
                            "expected exported name, found end of input",
                            stream.eof_span(),
                        ))
                    })?;
                    names.push(extract_ident(tok)?);
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
                                format!("expected `,` or `}}` in export list, found `{other}`"),
                                stream
                                    .peek()
                                    .map(|t| t.span)
                                    .unwrap_or_else(|| stream.eof_span()),
                            )));
                        }
                        None => {
                            return Err(ParseError::new(Diagnostic::error(
                                "unterminated export list (missing `}`)",
                                stream.eof_span(),
                            )));
                        }
                    }
                }
            }
            let rb = stream.expect(TokenKind::RBrace)?;
            // Optional `from "..."` — without it, this is a "export names"
            // statement re-exporting already-declared locals. We store an
            // empty string in `from` to signal "no source path".
            let (from, end) = if matches!(stream.peek_kind(), Some(TokenKind::KwFrom)) {
                stream.advance();
                let (s, e) = expect_path_string(stream)?;
                (s, e)
            } else {
                (String::new(), rb.span.end)
            };
            Ok(Decl::ReexportDecl(ReexportDecl {
                from,
                names,
                wildcard: false,
                span: Span::new(start, end, source_id),
            }))
        }
        // `export export`, `export import`, `export module`, `export trait`,
        // `export struct` (until struct parsing lands) — not exportable yet.
        Some(other) => Err(ParseError::new(Diagnostic::error(
            format!(
                "only `func`, `enum`, `*`, or `{{` are allowed after `export`, found `{other}`"
            ),
            stream
                .peek()
                .map(|t| t.span)
                .unwrap_or_else(|| stream.eof_span()),
        ))),
        None => Err(ParseError::new(Diagnostic::error(
            "expected item after `export`, found end of input",
            stream.eof_span(),
        ))),
    }
}

/// Parse an `extern crate "name"` declaration (T32 — FFI basics).
///
/// Records a dependency on an external Rust crate. The Buff form uses a
/// STRING literal for the crate name (`extern crate "serde"`) — unlike
/// Rust's `extern crate serde;`, which uses a bare path — so the crate
/// name may be any crates.io identifier (including names with `-`, which
/// are not valid Buff identifiers). The codegen emits a `use <name>;`
/// item and records the name in its extern-crate dep set.
///
/// The leading `extern` keyword is consumed here; this function expects
/// the NEXT significant token to be the bare identifier `crate`. The
/// crate name string follows immediately.
///
/// # Errors
///
/// Returns [`ParseError`] on:
/// - missing `crate` identifier after `extern`,
/// - missing crate-name string after `crate`,
/// - interpolation inside the crate-name string.
pub fn parse_extern_crate_decl(stream: &mut TokenStream<'_>) -> Result<Decl, ParseError> {
    let source_id = stream.source_id();
    let extern_tok = stream.expect(TokenKind::KwExtern)?;
    let start = extern_tok.span.start;

    // Expect the bare identifier `crate` (NOT a keyword in Buff — it is
    // parsed as a regular Ident). Reject anything else with a helpful msg.
    let crate_tok = stream.advance().ok_or_else(|| {
        ParseError::new(Diagnostic::error(
            "expected `crate` after `extern`, found end of input",
            stream.eof_span(),
        ))
    })?;
    match &crate_tok.kind {
        TokenKind::Ident(s) if s == "crate" => {}
        other => {
            return Err(ParseError::new(Diagnostic::error(
                format!(
                    "expected `crate` after `extern`, found `{other}` \
                     (Buff supports `extern crate \"<name>\"` and `extern func ...`)"
                ),
                crate_tok.span,
            )));
        }
    }

    // Consume the crate-name string literal (`"serde"`). Reuse the same
    // StringStart/StringPart/StringEnd machinery the lexer uses for every
    // string — interpolation is rejected (crate names must be plain).
    let (name, name_end) = expect_crate_name_string(stream)?;
    let span = Span::new(start, name_end, source_id);
    Ok(Decl::ExternCrateDecl(buff_lang_ast::ExternCrateDecl {
        name,
        span,
    }))
}

/// Expect a plain string literal naming a crate (the `"serde"` part of
/// `extern crate "serde"`). Returns the crate name + the end offset of
/// the closing `"`.
///
/// Mirrors [`expect_path_string`] but with crate-specific error messages.
/// The Buff lexer tokenizes every `"..."` as `StringStart, StringPart,
/// StringEnd`; we consume that three-token sequence and reject any
/// interpolated form.
///
/// # Errors
///
/// Returns [`ParseError`] if the next token is not a `StringStart`, if
/// the `StringPart` is missing, if interpolation appears, or if the
/// closing `StringEnd` is missing.
fn expect_crate_name_string(stream: &mut TokenStream<'_>) -> Result<(String, usize), ParseError> {
    let start_tok = stream.advance().ok_or_else(|| {
        ParseError::new(Diagnostic::error(
            "expected crate-name string after `extern crate`, found end of input",
            stream.eof_span(),
        ))
    })?;
    if !matches!(start_tok.kind, TokenKind::StringStart) {
        return Err(ParseError::new(Diagnostic::error(
            format!(
                "expected crate-name string after `extern crate`, found `{}`",
                start_tok.kind
            ),
            start_tok.span,
        )));
    }

    let part_tok = stream.advance().ok_or_else(|| {
        ParseError::new(Diagnostic::error(
            "expected crate-name string content, found end of input",
            stream.eof_span(),
        ))
    })?;
    let name = match part_tok.kind {
        TokenKind::StringPart(s) => s,
        TokenKind::InterpStart => {
            return Err(ParseError::new(Diagnostic::error(
                "crate-name string cannot contain interpolation",
                part_tok.span,
            )));
        }
        other => {
            return Err(ParseError::new(Diagnostic::error(
                format!("expected crate-name string content, found `{other}`"),
                part_tok.span,
            )));
        }
    };

    let end_tok = stream.advance().ok_or_else(|| {
        ParseError::new(Diagnostic::error(
            "unterminated crate-name string (missing closing quote)",
            stream.eof_span(),
        ))
    })?;
    if !matches!(end_tok.kind, TokenKind::StringEnd) {
        return Err(ParseError::new(Diagnostic::error(
            format!(
                "crate-name string cannot contain interpolation; expected end of string, found `{}`",
                end_tok.kind
            ),
            end_tok.span,
        )));
    }

    Ok((name, end_tok.span.end))
}

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
fn expect_path_string(stream: &mut TokenStream<'_>) -> Result<(String, usize), ParseError> {
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
        let at_tok = stream.advance().expect("peek guaranteed At");
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
                    let arg = match &arg_tok.kind {
                        TokenKind::Ident(s) => s.clone(),
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
            span: Span::new(start, end, source_id),
        });
    }
    Ok(attrs)
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
        | Stmt::ForWhile { span, .. }
        | Stmt::LetPattern { span, .. }
        | Stmt::ForLet { span, .. }
        | Stmt::Guard { span, .. } => span.end,
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

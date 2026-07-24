//! Postfix expression parsers - extracted from `expr.rs` (T106 mechanical split).
//!
//! Level 13: function call `(...)`, method call `.method(...)`, indexing
//! `[...]`, field access `.field`, and call-argument helpers.

use buff_lang_ast::{Expr, Ident, Literal, Span, Stmt};
use buff_lang_error::{Diagnostic, ParseError};
use buff_lang_lexer::TokenKind;

use super::{cursor_at_struct_init_body, is_numeric_primary, parse_expression, parse_primary, parse_struct_init_fields};
use crate::stream::TokenStream;
// ---------------------------------------------------------------------------
// Level 13 — postfix: function call `(...)` and method call `.method(...)`.
// ---------------------------------------------------------------------------

pub fn parse_postfix(stream: &mut TokenStream<'_>) -> Result<Expr, ParseError> {
    let mut expr = parse_primary(stream)?;
    loop {
        match stream.peek_kind() {
            // T26: struct-init `Type { field: value, ... }`. Fires ONLY when
            // the receiver is a bare Ident (a type name) AND the brace
            // contents match the struct-init field shape — we peek ahead
            // using save/restore to confirm the `{` is followed by either
            // `}` (empty init) or `Ident :` (first field). This avoids
            // misinterpreting `if cond { ... }` / `for x in iter { ... }`
            // block bodies as struct-inits: those bodies don't start with
            // `Ident :`, so they fall through and the outer parser handles
            // the `{` as a block.
            //
            // See [`parse_struct_init_fields`] for the field-list shape.
            Some(TokenKind::LBrace) => {
                if matches!(&expr, Expr::Ident(_, _)) && cursor_at_struct_init_body(stream) {
                    let (type_name, type_span) = if let Expr::Ident(name, sp) = &expr {
                        (name.clone(), *sp)
                    } else {
                        // Unreachable: matches! above gates this arm.
                        break;
                    };
                    stream.expect(TokenKind::LBrace)?; // consume `{`
                    let fields = parse_struct_init_fields(stream)?;
                    let rb = stream.expect(TokenKind::RBrace)?;
                    let span = Span::new(type_span.start, rb.span.end, stream.source_id());
                    expr = Expr::StructInit {
                        type_name,
                        fields,
                        span,
                    };
                } else {
                    break;
                }
            }
            Some(TokenKind::LParen) => {
                // T57: in the scientific edition, `<number>(...)` is IMPLICIT
                // MULTIPLICATION, not a function call. Break out of the
                // postfix loop so the parent `parse_multiplicative` sees the
                // numeric-primary LHS followed by `(` and synthesises the
                // `*`. (In the standard edition the call-arm fires as
                // before, preserving the historical behaviour where
                // `2(...)` was a call — though that was always a type
                // error downstream, never a valid program.)
                if stream.edition().is_scientific() && is_numeric_primary(&expr) {
                    break;
                }
                stream.advance(); // consume '('
                let args = parse_call_args(stream)?;
                let rparen = stream.expect(TokenKind::RParen)?;
                let span = Span::new(expr.span().start, rparen.span.end, stream.source_id());
                expr = Expr::FuncCall {
                    callee: Box::new(expr),
                    args,
                    span,
                };
            }
            // T23/T24: Indexing on a postfix expression. String LITERAL receivers
            // are rejected with the T21 helpful message (UTF-8 strings have no
            // sound byte indexing). All other receivers build an `Expr::Index`
            // node carrying one or more comma-separated indices; a later type
            // check rejects typed-String indexing. The receiver type is
            // generally unknown at parse time, so only the direct string-literal
            // case is rejected here. T24 generalized the index list to a
            // `Vec<Expr>` so `m[row, col]` (2-D Matrix) and `v[i]` (1-D Vector)
            // share one node shape.
            Some(TokenKind::LBracket) => {
                // Preserve the T21 string-literal rejection: `"abc"[0]` is
                // rejected at parse time with the helpful message. Any other
                // receiver (ident, call, nested index, …) builds an Index node.
                if matches!(&expr, Expr::Literal(Literal::String(_), _)) {
                    let lb = stream.advance().expect("peek guaranteed an LBracket");
                    return Err(ParseError::new(
                        Diagnostic::error(
                            "direct indexing `expr[...]` is not supported on strings; \
                             use .chars() or .first() instead",
                            lb.span,
                        )
                        .with_note(
                            "Buff does not provide byte indexing on UTF-8 strings. \
                             Iterate with `.chars()`, or use `.first()` / `.last()` \
                             for an Option<Char>.",
                        ),
                    ));
                }
                stream.advance(); // consume '['
                                  // Parse the first index; then keep consuming comma-separated
                                  // indices (T24). A trailing comma is allowed. The collected
                                  // vec drives codegen arity (1 → Vector, 2 → Matrix).
                let mut indices = vec![parse_expression(stream)?];
                while matches!(stream.peek_kind(), Some(TokenKind::Comma)) {
                    stream.advance(); // consume ','
                                      // Allow trailing comma: `v[a,]` / `m[r, c,]`.
                    if matches!(stream.peek_kind(), Some(TokenKind::RBracket)) {
                        break;
                    }
                    indices.push(parse_expression(stream)?);
                }
                let rb = stream.expect(TokenKind::RBracket)?;
                let span = Span::new(expr.span().start, rb.span.end, stream.source_id());
                expr = Expr::Index {
                    base: Box::new(expr),
                    indices,
                    span,
                };
            }
            Some(TokenKind::Dot) => {
                stream.advance(); // consume '.'
                let method_tok = match stream.advance() {
                    Some(t) if matches!(t.kind, TokenKind::Ident(_)) => t,
                    Some(other) => {
                        return Err(ParseError::new(Diagnostic::error(
                            format!("expected method name after `.`, found `{}`", other.kind),
                            other.span,
                        )));
                    }
                    None => {
                        return Err(ParseError::new(Diagnostic::error(
                            "expected method name after `.`, found end of input",
                            stream.eof_span(),
                        )));
                    }
                };
                let TokenKind::Ident(name) = method_tok.kind.clone() else {
                    unreachable!("matched Ident above");
                };
                let method = Ident::new(name, method_tok.span);
                // Method calls must be followed by an argument list `(...)`.
                // (Bare field access like `obj.field` is not supported in T7
                // — there is no `FieldAccess` AST variant. If no `(` follows,
                // treat as a zero-arg method call so `obj.field` parses.)
                let (args, end_off) = if matches!(stream.peek_kind(), Some(TokenKind::LParen)) {
                    stream.advance();
                    let args = parse_call_args(stream)?;
                    let rparen = stream.expect(TokenKind::RParen)?;
                    (args, rparen.span.end)
                } else {
                    (Vec::new(), method_tok.span.end)
                };
                let span = Span::new(expr.span().start, end_off, stream.source_id());
                expr = Expr::MethodCall {
                    receiver: Box::new(expr),
                    method,
                    args,
                    span,
                };
            }
            // T30: the `?` error-propagation postfix operator. `expr?`
            // wraps its operand in `Expr::Try`. The `?` token
            // (`TokenKind::Question`) is NOT a reserved keyword — it lexes
            // as a single-byte punctuation token. Chaining (`expr??`) works
            // naturally because the loop continues after consuming one `?`.
            // This mirrors Rust's `?` operator: the operand must be a
            // `Result<T, E>` (or `Option<T>`); the codegen lowers it 1:1 to
            // Rust's native `?`.
            Some(TokenKind::Question) => {
                let q = stream.advance().expect("peek guaranteed Question");
                let span = Span::new(expr.span().start, q.span.end, stream.source_id());
                expr = Expr::Try {
                    expr: Box::new(expr),
                    span,
                };
            }
            // T57: postfix adjoint/transpose operator `A'`. Desugars IN THE
            // PARSER to `A.transpose()` (a method call). The lexer emits
            // `Adjoint` only when the previous significant token is
            // expression-ending, so reaching this arm means the user wrote
            // `<expr>'` in a postfix context. Rejected in the Standard
            // edition via `Edition::require_for`. ASCII alternative: the
            // `.transpose()` method itself.
            Some(TokenKind::Adjoint) => {
                stream
                    .edition()
                    .require_for(&TokenKind::Adjoint)
                    .map_err(|msg| {
                        ParseError::new(Diagnostic::error(
                            msg,
                            stream
                                .peek()
                                .map(|t| t.span)
                                .unwrap_or_else(|| stream.eof_span()),
                        ))
                    })?;
                let adj = stream.advance().expect("peek guaranteed Adjoint");
                let span = Span::new(expr.span().start, adj.span.end, stream.source_id());
                expr = Expr::MethodCall {
                    receiver: Box::new(expr),
                    method: Ident::new("transpose".to_string(), adj.span),
                    args: Vec::new(),
                    span,
                };
            }
            // T70: null-conditional `?.` operator. `receiver ?. name`
            // desugars IN THE PARSER to `receiver.and_then(|x| x.name)`,
            // an `Option`-chain with short-circuit semantics (the closure
            // is NOT called when the receiver is `None`). Chaining
            // `a?.b?.c` nests left-associatively because the loop
            // continues after each `?.`, so the receiver of the next
            // iteration is the just-built `and_then` MethodCall:
            //   inner  = a.and_then(|x| x.b)
            //   outer  = inner.and_then(|x| x.c)
            //
            // If a `(` follows the field/method name, it's the method-call
            // form: `u?.m(args)` desugars to `u.and_then(|x| x.m(args))`.
            //
            // The closure param name is `x` (matches the spec's literal
            // `|x| x.name` output). The placeholder param type reuses the
            // SAME mechanism as `parse_closure` (TypeRef::Named{"_"}) so
            // type inference + codegen treat it identically to a normal
            // `{ x => ... }` closure (codegen emits `|x|` with no
            // annotation; Rust infers the inner type from `and_then`).
            //
            // The desugar produces existing AST nodes (MethodCall + Lambda)
            // so NO codegen arm, NO type-inference change, and NO new Expr
            // variant are needed — same strategy as T69 (`|>`) and T101
            // (`??`).
            Some(TokenKind::QuestionDot) => {
                let qd = stream.advance().expect("peek guaranteed QuestionDot");
                // Parse the field/method name (must be an Ident, like the
                // Dot arm above).
                let name_tok = match stream.advance() {
                    Some(t) if matches!(t.kind, TokenKind::Ident(_)) => t,
                    Some(other) => {
                        return Err(ParseError::new(Diagnostic::error(
                            format!(
                                "expected field or method name after `?.`, found `{}`",
                                other.kind
                            ),
                            other.span,
                        )));
                    }
                    None => {
                        return Err(ParseError::new(Diagnostic::error(
                            "expected field or method name after `?.`, found end of input",
                            stream.eof_span(),
                        )));
                    }
                };
                let TokenKind::Ident(field_name) = name_tok.kind.clone() else {
                    unreachable!("matched Ident above");
                };
                let field_ident = Ident::new(field_name, name_tok.span);
                // Method-call form: `u?.m(args...)` — parse the arg list.
                // Field form: `u?.name` — zero args. Same shape as the Dot
                // arm above.
                let (field_args, end_off) = if matches!(stream.peek_kind(), Some(TokenKind::LParen))
                {
                    stream.advance();
                    let args = parse_call_args(stream)?;
                    let rparen = stream.expect(TokenKind::RParen)?;
                    (args, rparen.span.end)
                } else {
                    (Vec::new(), name_tok.span.end)
                };
                // Build the closure body: `x.name` or `x.m(args...)` — a
                // MethodCall whose receiver is the closure's `x` param.
                let x_ident = Ident::new("x".to_string(), qd.span);
                let x_expr = Expr::Ident(x_ident.clone(), qd.span);
                let body_inner = Expr::MethodCall {
                    receiver: Box::new(x_expr),
                    method: field_ident,
                    args: field_args,
                    span: Span::new(name_tok.span.start, end_off, stream.source_id()),
                };
                // Build the closure `|x| body_inner` — single-ExprStmt body
                // (same shape as parse_closure).
                let param = buff_lang_ast::common::Param {
                    name: x_ident,
                    // Placeholder type — closures infer their param types;
                    // codegen emits `|x|` (no annotation). Mirrors
                    // parse_closure's placeholder exactly so type inference
                    // + codegen handle this identically to a user-written
                    // `{ x => ... }` closure.
                    ty: buff_lang_ast::TypeRef::Named {
                        name: Ident::new("_", qd.span),
                        span: qd.span,
                    },
                    default_value: None,
                    is_comptime: false,
                    span: qd.span,
                };
                let lambda = Expr::Lambda {
                    params: vec![param],
                    body: buff_lang_ast::common::Block {
                        stmts: vec![Stmt::ExprStmt(body_inner, qd.span)],
                        span: Span::new(qd.span.start, end_off, stream.source_id()),
                    },
                    return_type: None,
                    span: Span::new(qd.span.start, end_off, stream.source_id()),
                };
                // Build the outer MethodCall: `receiver.and_then(lambda)`.
                let outer_span = Span::new(expr.span().start, end_off, stream.source_id());
                expr = Expr::MethodCall {
                    receiver: Box::new(expr),
                    method: Ident::new("and_then".to_string(), qd.span),
                    args: vec![lambda],
                    span: outer_span,
                };
            }
            _ => break,
        }
    }
    Ok(expr)
}

/// Parse a comma-separated argument list, *excluding* the surrounding parens.
/// The opening `(` must already have been consumed; the closing `)` is left
/// for the caller to expect.
///
/// Each argument may be either:
/// - **Positional** — a bare expression (`f(1, 2)`).
/// - **Named** — `name: value` (`f(host: "x")`), parsed into an
///   [`Expr::NamedArg`] node (T105).
///
/// Named args can appear in ANY order at any position; reorder to the callee's
/// declared param order is done at codegen time (the parser does NOT have
/// cross-function param-name info). Mixed positional + named is allowed (the
/// common convention is positional-first; the parser is permissive). A
/// trailing comma is allowed (`foo(a, b,)`).
pub fn parse_call_args(stream: &mut TokenStream<'_>) -> Result<Vec<Expr>, ParseError> {
    let mut args = Vec::new();
    if matches!(stream.peek_kind(), Some(TokenKind::RParen)) {
        return Ok(args);
    }
    args.push(parse_one_call_arg(stream)?);
    while matches!(stream.peek_kind(), Some(TokenKind::Comma)) {
        stream.advance();
        // Allow trailing comma: `foo(a, b,)`.
        if matches!(stream.peek_kind(), Some(TokenKind::RParen)) {
            break;
        }
        args.push(parse_one_call_arg(stream)?);
    }
    Ok(args)
}

/// Parse ONE call argument — either a positional expression or a
/// `name: value` named arg (T105).
///
/// The disambiguation is purely lexical: peek the next TWO significant tokens.
/// If they are `Ident Colon`, this is a named arg; otherwise it's a positional
/// expression (parsed via the normal [`parse_expression`] path). This is the
/// same shape test that `parse_params` uses (`name: Type`), but for argument
/// position. The `:` token is not consumed by any Pratt operator, so its
/// presence unambiguously signals a named arg.
///
/// **Why two-token lookahead and not trial parse?** Trial parsing (save /
/// restore) would also work, but it adds complexity for no gain here: the
/// `Ident Colon` shape at an argument position is unambiguous (Rust has no
/// type-ascript expressions, and no Buff operator consumes `:`). A two-token
/// peek is cheaper and clearer.
pub fn parse_one_call_arg(stream: &mut TokenStream<'_>) -> Result<Expr, ParseError> {
    // Named arg: `Ident Colon Expr`. Peek the next two significant tokens.
    let is_named = matches!(
        (stream.peek_kind(), stream.peek_second_kind()),
        (Some(TokenKind::Ident(_)), Some(TokenKind::Colon))
    );
    if is_named {
        let name_tok = stream
            .advance()
            .expect("peek guaranteed an Ident for named arg");
        let name_span = name_tok.span;
        let name = match name_tok.kind {
            TokenKind::Ident(s) => Ident::new(s, name_span),
            _ => unreachable!("peek guaranteed an Ident for named arg"),
        };
        // Consume the `:` — its position is implied by `name_span.end` so we
        // don't need to keep the token; the value's span end is the
        // authoritative arg-end.
        stream.expect(TokenKind::Colon)?;
        let value = parse_expression(stream)?;
        let span = Span::new(name_span.start, value.span().end, stream.source_id());
        Ok(Expr::NamedArg {
            name,
            value: Box::new(value),
            span,
        })
    } else {
        parse_expression(stream)
    }
}

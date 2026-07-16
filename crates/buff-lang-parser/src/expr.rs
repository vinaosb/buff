//! Expression parser — hand-rolled recursive-descent with operator-precedence
//! climbing (Pratt-style) for binary operators.
//!
//! Precedence ladder (lowest to highest):
//!
//! 1. Assignment (`=`, `+=`, …) — right-assoc
//! 2. Logical OR `||`
//! 3. Logical AND `&&`
//! 4. Equality `==`, `!=`
//! 5. Comparison `<`, `>`, `<=`, `>=`
//! 6. Bitwise OR `|`
//! 7. Bitwise XOR `^`
//! 8. Bitwise AND `&`
//! 9. Shift `<<`, `>>`
//! 10. Additive `+`, `-`
//! 11. Multiplicative `*`, `/`, `%`
//! 12. Unary prefix `-`, `!`, `~`
//! 13. Postfix `(...)` call, `.method(...)` call
//! 14. Primary — literals, identifiers, `( expr )`
//!
//! All functions are fallible: they return [`Result<Expr, ParseError>`]. No
//! panics, no `unwrap`/`expect` in non-test code.

use buff_lang_ast::{BinaryOp, Expr, Ident, Literal, UnaryOp};
use buff_lang_error::{Diagnostic, ParseError, Span};
use buff_lang_lexer::TokenKind;

use crate::stream::TokenStream;

/// Public entry point — parse one expression (precedence: assignment level).
///
/// The stream cursor is advanced past everything needed to construct the
/// expression. On error, the cursor position is unspecified (the parser does
/// not currently do error-recovery — that is a future task).
pub fn parse_expression(stream: &mut TokenStream<'_>) -> Result<Expr, ParseError> {
    parse_assignment(stream)
}

// ---------------------------------------------------------------------------
// Level 1 — assignment (right-associative)
// ---------------------------------------------------------------------------

fn parse_assignment(stream: &mut TokenStream<'_>) -> Result<Expr, ParseError> {
    let lhs = parse_or(stream)?;
    let Some(op) = stream.peek_kind().and_then(assignment_op) else {
        return Ok(lhs);
    };
    // Right-associative: consume the operator, then recursively parse the
    // assignment-level RHS so chains like `a = b = c` work.
    stream.advance();
    let rhs = parse_assignment(stream)?;
    let span = combine_span(&lhs, &rhs);
    Ok(Expr::BinaryOp {
        op,
        lhs: Box::new(lhs),
        rhs: Box::new(rhs),
        span,
    })
}

// ---------------------------------------------------------------------------
// Levels 2–11 — left-associative binary operators.
//
// Each level follows the same shape: parse the next-higher precedence LHS,
// then loop while the upcoming token maps to one of our operators.
// ---------------------------------------------------------------------------

fn parse_or(stream: &mut TokenStream<'_>) -> Result<Expr, ParseError> {
    let mut lhs = parse_and(stream)?;
    while matches!(stream.peek_kind(), Some(TokenKind::OrOr)) {
        stream.advance();
        let rhs = parse_and(stream)?;
        let span = combine_span(&lhs, &rhs);
        lhs = Expr::BinaryOp {
            op: BinaryOp::Or,
            lhs: Box::new(lhs),
            rhs: Box::new(rhs),
            span,
        };
    }
    Ok(lhs)
}

fn parse_and(stream: &mut TokenStream<'_>) -> Result<Expr, ParseError> {
    let mut lhs = parse_equality(stream)?;
    while matches!(stream.peek_kind(), Some(TokenKind::AndAnd)) {
        stream.advance();
        let rhs = parse_equality(stream)?;
        let span = combine_span(&lhs, &rhs);
        lhs = Expr::BinaryOp {
            op: BinaryOp::And,
            lhs: Box::new(lhs),
            rhs: Box::new(rhs),
            span,
        };
    }
    Ok(lhs)
}

fn parse_equality(stream: &mut TokenStream<'_>) -> Result<Expr, ParseError> {
    let mut lhs = parse_comparison(stream)?;
    while let Some(op) = stream.peek_kind().and_then(eq_op) {
        stream.advance();
        let rhs = parse_comparison(stream)?;
        let span = combine_span(&lhs, &rhs);
        lhs = Expr::BinaryOp {
            op,
            lhs: Box::new(lhs),
            rhs: Box::new(rhs),
            span,
        };
    }
    Ok(lhs)
}

fn parse_comparison(stream: &mut TokenStream<'_>) -> Result<Expr, ParseError> {
    let mut lhs = parse_bitor(stream)?;
    while let Some(op) = stream.peek_kind().and_then(cmp_op) {
        stream.advance();
        let rhs = parse_bitor(stream)?;
        let span = combine_span(&lhs, &rhs);
        lhs = Expr::BinaryOp {
            op,
            lhs: Box::new(lhs),
            rhs: Box::new(rhs),
            span,
        };
    }
    Ok(lhs)
}

fn parse_bitor(stream: &mut TokenStream<'_>) -> Result<Expr, ParseError> {
    let mut lhs = parse_bitxor(stream)?;
    while matches!(stream.peek_kind(), Some(TokenKind::Pipe)) {
        stream.advance();
        let rhs = parse_bitxor(stream)?;
        let span = combine_span(&lhs, &rhs);
        lhs = Expr::BinaryOp {
            op: BinaryOp::BitOr,
            lhs: Box::new(lhs),
            rhs: Box::new(rhs),
            span,
        };
    }
    Ok(lhs)
}

fn parse_bitxor(stream: &mut TokenStream<'_>) -> Result<Expr, ParseError> {
    let mut lhs = parse_bitand(stream)?;
    while matches!(stream.peek_kind(), Some(TokenKind::Caret)) {
        stream.advance();
        let rhs = parse_bitand(stream)?;
        let span = combine_span(&lhs, &rhs);
        lhs = Expr::BinaryOp {
            op: BinaryOp::BitXor,
            lhs: Box::new(lhs),
            rhs: Box::new(rhs),
            span,
        };
    }
    Ok(lhs)
}

fn parse_bitand(stream: &mut TokenStream<'_>) -> Result<Expr, ParseError> {
    let mut lhs = parse_shift(stream)?;
    while matches!(stream.peek_kind(), Some(TokenKind::Amp)) {
        stream.advance();
        let rhs = parse_shift(stream)?;
        let span = combine_span(&lhs, &rhs);
        lhs = Expr::BinaryOp {
            op: BinaryOp::BitAnd,
            lhs: Box::new(lhs),
            rhs: Box::new(rhs),
            span,
        };
    }
    Ok(lhs)
}

fn parse_shift(stream: &mut TokenStream<'_>) -> Result<Expr, ParseError> {
    let mut lhs = parse_additive(stream)?;
    while let Some(op) = stream.peek_kind().and_then(shift_op) {
        stream.advance();
        let rhs = parse_additive(stream)?;
        let span = combine_span(&lhs, &rhs);
        lhs = Expr::BinaryOp {
            op,
            lhs: Box::new(lhs),
            rhs: Box::new(rhs),
            span,
        };
    }
    Ok(lhs)
}

fn parse_additive(stream: &mut TokenStream<'_>) -> Result<Expr, ParseError> {
    let mut lhs = parse_multiplicative(stream)?;
    while let Some(op) = stream.peek_kind().and_then(additive_op) {
        stream.advance();
        let rhs = parse_multiplicative(stream)?;
        let span = combine_span(&lhs, &rhs);
        lhs = Expr::BinaryOp {
            op,
            lhs: Box::new(lhs),
            rhs: Box::new(rhs),
            span,
        };
    }
    Ok(lhs)
}

fn parse_multiplicative(stream: &mut TokenStream<'_>) -> Result<Expr, ParseError> {
    let mut lhs = parse_unary(stream)?;
    while let Some(op) = stream.peek_kind().and_then(mul_op) {
        stream.advance();
        let rhs = parse_unary(stream)?;
        let span = combine_span(&lhs, &rhs);
        lhs = Expr::BinaryOp {
            op,
            lhs: Box::new(lhs),
            rhs: Box::new(rhs),
            span,
        };
    }
    Ok(lhs)
}

// ---------------------------------------------------------------------------
// Level 12 — unary prefix operators (`-`, `!`, `~`).
// ---------------------------------------------------------------------------

fn parse_unary(stream: &mut TokenStream<'_>) -> Result<Expr, ParseError> {
    let op = match stream.peek_kind() {
        Some(TokenKind::Minus) => Some(UnaryOp::Neg),
        Some(TokenKind::Not) => Some(UnaryOp::Not),
        Some(TokenKind::Tilde) => Some(UnaryOp::BitNot),
        _ => None,
    };
    if let Some(op) = op {
        let op_tok = stream.advance().expect("peek guaranteed a token");
        let operand = parse_unary(stream)?;
        let span = Span::new(op_tok.span.start, operand.span().end, stream.source_id());
        return Ok(Expr::UnaryOp {
            op,
            operand: Box::new(operand),
            span,
        });
    }
    parse_postfix(stream)
}

// ---------------------------------------------------------------------------
// Level 13 — postfix: function call `(...)` and method call `.method(...)`.
// ---------------------------------------------------------------------------

fn parse_postfix(stream: &mut TokenStream<'_>) -> Result<Expr, ParseError> {
    let mut expr = parse_primary(stream)?;
    loop {
        match stream.peek_kind() {
            Some(TokenKind::LParen) => {
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
            _ => break,
        }
    }
    Ok(expr)
}

/// Parse a comma-separated argument list, *excluding* the surrounding parens.
/// The opening `(` must already have been consumed; the closing `)` is left
/// for the caller to expect.
fn parse_call_args(stream: &mut TokenStream<'_>) -> Result<Vec<Expr>, ParseError> {
    let mut args = Vec::new();
    if matches!(stream.peek_kind(), Some(TokenKind::RParen)) {
        return Ok(args);
    }
    args.push(parse_expression(stream)?);
    while matches!(stream.peek_kind(), Some(TokenKind::Comma)) {
        stream.advance();
        // Allow trailing comma: `foo(a, b,)`
        if matches!(stream.peek_kind(), Some(TokenKind::RParen)) {
            break;
        }
        args.push(parse_expression(stream)?);
    }
    Ok(args)
}

// ---------------------------------------------------------------------------
// Level 14 — primary: literals, identifiers, parenthesized expressions.
// ---------------------------------------------------------------------------

fn parse_primary(stream: &mut TokenStream<'_>) -> Result<Expr, ParseError> {
    let Some(tok) = stream.peek().cloned() else {
        return Err(ParseError::new(Diagnostic::error(
            "expected an expression, found end of input",
            stream.eof_span(),
        )));
    };

    // T9: `if` can appear as an expression (e.g. `let x = if c { 1 } else { 2 }`).
    // Delegate to stmt::parse_if_expr which handles both braces and layout
    // blocks.
    if matches!(tok.kind, TokenKind::KwIf) {
        return crate::stmt::parse_if_expr(stream);
    }

    // If it's an open paren, parse a parenthesized expression.
    if matches!(tok.kind, TokenKind::LParen) {
        stream.advance(); // consume '('
        let inner = parse_expression(stream)?;
        stream.expect(TokenKind::RParen)?;
        // Parens don't change the span of the inner expression — keep inner.
        return Ok(inner);
    }

    // Otherwise consume the token and turn it into a literal/ident node.
    // `tok` is already an owned clone from `peek().cloned()` above; we just
    // advance the cursor here.
    stream.advance();
    let span = tok.span;
    let expr = match &tok.kind {
        TokenKind::IntLit(v) => Expr::Literal(Literal::Int(*v), span),
        TokenKind::FloatLit(v) => Expr::Literal(Literal::Float(*v), span),
        TokenKind::DoubleLit(v) => Expr::Literal(Literal::Double(*v), span),
        TokenKind::ByteLit(v) => Expr::Literal(Literal::Byte(*v), span),
        TokenKind::KwTrue => Expr::Literal(Literal::Bool(true), span),
        TokenKind::KwFalse => Expr::Literal(Literal::Bool(false), span),
        TokenKind::Ident(name) => Expr::Ident(Ident::new(name.clone(), span), span),
        // Simple (non-interpolated) strings look like:
        //   StringStart [StringPart(text)] StringEnd
        TokenKind::StringStart => parse_simple_string(stream, span)?,
        other => {
            return Err(ParseError::new(Diagnostic::error(
                format!("expected an expression, found `{other}`"),
                span,
            )));
        }
    };
    Ok(expr)
}

/// Parse a string literal whose `StringStart` token has already been
/// consumed-start: we are now at the optional `StringPart`/`InterpStart`/
/// `StringEnd` sequence.
///
/// `start_span` is the span of the opening `"` so we can build a full-string
/// span even when the literal is empty (`""`).
///
/// For T7, interpolation is rejected — the caller gets a [`ParseError`]
/// mentioning future support. Plain (possibly-empty) text concatenation of
/// consecutive `StringPart` tokens is supported.
fn parse_simple_string(stream: &mut TokenStream<'_>, start_span: Span) -> Result<Expr, ParseError> {
    let mut text = String::new();
    // Only the StringEnd token's end offset contributes to the final span —
    // intermediate StringPart offsets are overwritten before use.
    let end_off;
    loop {
        let Some(tok) = stream.advance() else {
            return Err(ParseError::new(Diagnostic::error(
                "unterminated string literal (missing `StringEnd`)",
                start_span,
            )));
        };
        match tok.kind {
            TokenKind::StringPart(s) => {
                text.push_str(&s);
            }
            TokenKind::StringEnd => {
                end_off = tok.span.end;
                break;
            }
            TokenKind::InterpStart => {
                return Err(ParseError::new(
                    Diagnostic::error("string interpolation is not supported in T7", tok.span)
                        .with_note(
                            "T7 implements literals only; interpolation arrives in a later task",
                        ),
                ));
            }
            other => {
                return Err(ParseError::new(Diagnostic::error(
                    format!(
                        "malformed string literal: unexpected `{other}` inside string token stream"
                    ),
                    tok.span,
                )));
            }
        }
    }
    let span = Span::new(start_span.start, end_off, start_span.source_id);
    Ok(Expr::Literal(Literal::String(text), span))
}

// ---------------------------------------------------------------------------
// Operator-class lookup helpers.
// ---------------------------------------------------------------------------

fn assignment_op(kind: &TokenKind) -> Option<BinaryOp> {
    Some(match kind {
        TokenKind::Assign => BinaryOp::Assign,
        TokenKind::PlusEq => BinaryOp::AddAssign,
        TokenKind::MinusEq => BinaryOp::SubAssign,
        TokenKind::StarEq => BinaryOp::MulAssign,
        TokenKind::SlashEq => BinaryOp::DivAssign,
        TokenKind::PercentEq => BinaryOp::ModAssign,
        _ => return None,
    })
}

fn eq_op(kind: &TokenKind) -> Option<BinaryOp> {
    Some(match kind {
        TokenKind::EqEq => BinaryOp::Eq,
        TokenKind::NotEq => BinaryOp::Neq,
        _ => return None,
    })
}

fn cmp_op(kind: &TokenKind) -> Option<BinaryOp> {
    Some(match kind {
        TokenKind::Lt => BinaryOp::Lt,
        TokenKind::Gt => BinaryOp::Gt,
        TokenKind::LtEq => BinaryOp::Lte,
        TokenKind::GtEq => BinaryOp::Gte,
        _ => return None,
    })
}

fn shift_op(kind: &TokenKind) -> Option<BinaryOp> {
    Some(match kind {
        TokenKind::Shl => BinaryOp::Shl,
        TokenKind::Shr => BinaryOp::Shr,
        _ => return None,
    })
}

fn additive_op(kind: &TokenKind) -> Option<BinaryOp> {
    Some(match kind {
        TokenKind::Plus => BinaryOp::Add,
        TokenKind::Minus => BinaryOp::Sub,
        _ => return None,
    })
}

fn mul_op(kind: &TokenKind) -> Option<BinaryOp> {
    Some(match kind {
        TokenKind::Star => BinaryOp::Mul,
        TokenKind::Slash => BinaryOp::Div,
        TokenKind::Percent => BinaryOp::Mod,
        _ => return None,
    })
}

/// Combine the spans of two expressions into a parent span.
fn combine_span(lhs: &Expr, rhs: &Expr) -> Span {
    let l = lhs.span();
    let r = rhs.span();
    // Both spans should share the same source_id; use lhs's.
    Span::new(l.start.min(r.start), l.end.max(r.end), l.source_id)
}

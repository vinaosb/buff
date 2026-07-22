//! Expression parser — hand-rolled recursive-descent with operator-precedence
//! climbing (Pratt-style) for binary operators.
//!
//! Precedence ladder (lowest to highest):
//!
//! 1. Assignment (`=`, `+=`, …) — right-assoc
//! 2. Pipeline `|>` — left-assoc; desugars `LHS |> f(args)` to `f(LHS, args)`
//! 3. Range `..`, `..=`
//! 4. Null-coalesce `??`
//! 5. Logical OR `||`
//! 6. Logical AND `&&`
//! 7. Equality `==`, `!=`
//! 8. Comparison `<`, `>`, `<=`, `>=`
//! 9. Bitwise OR `|`
//! 10. Bitwise XOR `^`
//! 11. Bitwise AND `&`
//! 12. Shift `<<`, `>>`
//! 13. Additive `+`, `-`
//! 14. Multiplicative `*`, `/`, `%`
//! 15. Unary prefix `-`, `!`, `~`
//! 16. Postfix `(...)` call, `.method(...)` call
//! 17. Primary — literals, identifiers, `( expr )`
//!
//! All functions are fallible: they return [`Result<Expr, ParseError>`]. No
//! panics, no `unwrap`/`expect` in non-test code.

use buff_lang_ast::{
    BinaryOp, Block, Expr, Ident, InterpPart, Literal, MatchArm, Pattern, Stmt, UnaryOp,
};
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
    let lhs = parse_pipeline(stream)?;
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
// Level 2 — pipeline `|>` (left-associative; desugars to a FuncCall).
//
// `LHS |> RHS` rewrites the RHS so the LHS becomes its FIRST argument:
//   - `x |> f(args...)`  -> `f(x, args...)`
//   - `x |> f`           -> `f(x)`          (bare-callee shorthand)
//   - `x |> obj.m(args)` -> `obj.m(x, args)` (method-call RHS)
// Chaining is left-associative: `a |> f() |> g()` = `(a |> f()) |> g()` =
// `g(f(a))`. Pipeline binds looser than every other binary operator (it sits
// just below assignment), so `a + b |> f()` parses as `f(a + b)` and
// `x |> f() * 2` parses as `(x |> f()) * 2`.
//
// The desugar happens ENTIRELY here in the parser: no new AST variant, no
// codegen change — the result is a plain `Expr::FuncCall` (or `MethodCall`),
// which codegen already lowers. This is the T69-recommended approach.
// ---------------------------------------------------------------------------

fn parse_pipeline(stream: &mut TokenStream<'_>) -> Result<Expr, ParseError> {
    let mut lhs = parse_range(stream)?;
    while matches!(stream.peek_kind(), Some(TokenKind::PipeGt)) {
        let pipe_tok = stream.advance().expect("peek guaranteed PipeGt");
        let rhs = parse_range(stream)?;
        lhs = desugar_pipeline(lhs, rhs, &pipe_tok)?;
    }
    Ok(lhs)
}

/// Rewrite `LHS |> RHS` into a call with `LHS` prepended as the first arg.
///
/// Accepts (T69):
/// - `Expr::FuncCall { callee, args }` → `FuncCall { callee, args: [LHS, ...args] }`
/// - `Expr::MethodCall { receiver, method, args }` → same prepend pattern.
/// - `Expr::Ident(name)` (bare callee, no parens) → `FuncCall { callee: name, args: [LHS] }`.
///
/// Any other RHS shape (e.g. `x |> 5`, `x |> a + b`) is a [`ParseError`] — the
/// RHS of `|>` MUST be callable. `pipe_tok` is the `|>` token (used for the
/// error span when the RHS is invalid).
fn desugar_pipeline(
    lhs: Expr,
    rhs: Expr,
    pipe_tok: &buff_lang_lexer::Token,
) -> Result<Expr, ParseError> {
    let span = combine_span(&lhs, &rhs);
    match rhs {
        Expr::FuncCall {
            callee,
            mut args,
            span: _,
        } => {
            // Prepend LHS as the first argument: `x |> f(a, b)` -> `f(x, a, b)`.
            let mut new_args = Vec::with_capacity(args.len() + 1);
            new_args.push(lhs);
            new_args.append(&mut args);
            Ok(Expr::FuncCall {
                callee,
                args: new_args,
                span,
            })
        }
        Expr::MethodCall {
            receiver,
            method,
            mut args,
            span: _,
        } => {
            // `x |> obj.m(a, b)` -> `obj.m(x, a, b)`.
            let mut new_args = Vec::with_capacity(args.len() + 1);
            new_args.push(lhs);
            new_args.append(&mut args);
            Ok(Expr::MethodCall {
                receiver,
                method,
                args: new_args,
                span,
            })
        }
        Expr::Ident(name, _) => {
            // Bare-callee shorthand: `x |> f` -> `f(x)`.
            Ok(Expr::FuncCall {
                callee: Box::new(Expr::Ident(name, span)),
                args: vec![lhs],
                span,
            })
        }
        other => Err(ParseError::new(Diagnostic::error(
            format!("right-hand side of `|>` must be a function call, found `{other}`"),
            pipe_tok.span,
        ))),
    }
}

// ---------------------------------------------------------------------------
// Level 3 — range `..` and `..=` (lower than additive, higher than pipeline)
// ---------------------------------------------------------------------------

fn parse_range(stream: &mut TokenStream<'_>) -> Result<Expr, ParseError> {
    let lhs = parse_null_coalesce(stream)?;
    // Check for `..` or `..=` after the LHS.
    match stream.peek_kind() {
        Some(TokenKind::DotDot) => {
            stream.advance();
            let rhs = parse_null_coalesce(stream)?;
            let span = combine_span(&lhs, &rhs);
            Ok(Expr::Range {
                start: Box::new(lhs),
                end: Box::new(rhs),
                inclusive: false,
                span,
            })
        }
        Some(TokenKind::DotDotEq) => {
            stream.advance();
            let rhs = parse_null_coalesce(stream)?;
            let span = combine_span(&lhs, &rhs);
            Ok(Expr::Range {
                start: Box::new(lhs),
                end: Box::new(rhs),
                inclusive: true,
                span,
            })
        }
        _ => Ok(lhs),
    }
}

// ---------------------------------------------------------------------------
// Level 4 — null coalescing `??` (between range and logical OR)
// ---------------------------------------------------------------------------

fn parse_null_coalesce(stream: &mut TokenStream<'_>) -> Result<Expr, ParseError> {
    let mut lhs = parse_or(stream)?;
    while matches!(stream.peek_kind(), Some(TokenKind::QuestionQuestion)) {
        stream.advance();
        let rhs = parse_or(stream)?;
        let span = combine_span(&lhs, &rhs);
        lhs = Expr::BinaryOp {
            op: BinaryOp::NullCoalesce,
            lhs: Box::new(lhs),
            rhs: Box::new(rhs),
            span,
        };
    }
    Ok(lhs)
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
    loop {
        if let Some(op) = stream.peek_kind().and_then(mul_op) {
            stream.advance();
            let rhs = parse_unary(stream)?;
            let span = combine_span(&lhs, &rhs);
            lhs = Expr::BinaryOp {
                op,
                lhs: Box::new(lhs),
                rhs: Box::new(rhs),
                span,
            };
        } else if stream.edition().is_scientific()
            && !stream.in_matrix_row()
            && ends_with_numberlike(&lhs)
            && is_implicit_mult_start(stream.peek_kind())
        {
            // T57: implicit multiplication — `2x`, `2(x+y)`, `3sin(x)`.
            // Synthesise a `*` without consuming any operator token. The
            // LHS-shape check (`ends_with_numberlike`) prevents `x y` from
            // being treated as `x * y` — only numeric-primary LHS triggers
            // the rewrite. Closing-delimiter LHS (`(a+b)`) also qualifies
            // so `2(x+y)` works when wrapped: `(2)(x)` → `(2) * (x)`.
            let rhs = parse_unary(stream)?;
            let span = combine_span(&lhs, &rhs);
            lhs = Expr::BinaryOp {
                op: BinaryOp::Mul,
                lhs: Box::new(lhs),
                rhs: Box::new(rhs),
                span,
            };
        } else {
            break;
        }
    }
    Ok(lhs)
}

/// Whether `expr` is a numeric primary suitable as the LHS of an implicit
/// multiplication (T57). Returns `true` for any numeric literal, a
/// parenthesised numeric expression, or any closing-delimiter-terminated
/// postfix expression (`f(x)`, `v[i]`) — i.e. the contexts where `2x` /
/// `2(...)` / `f(x)g(y)` make sense.
fn ends_with_numberlike(expr: &Expr) -> bool {
    match expr {
        Expr::Literal(
            Literal::Int(_)
            | Literal::Float(_)
            | Literal::Double(_)
            | Literal::Byte(_)
            | Literal::Decimal(_),
            _,
        ) => true,
        // Any postfix-built expression (`f(x)`, `v[i]`, `a.b`, `x?`)
        // terminates with a closing delimiter or identifier; implicit mult
        // after one of these matches the Julia rule `(a+b)(c+d)` → mul.
        Expr::FuncCall { .. }
        | Expr::MethodCall { .. }
        | Expr::Index { .. }
        | Expr::Try { .. }
        | Expr::TupleLit(_, _) => true,
        _ => false,
    }
}

/// Whether `expr` is a bare numeric literal (T57). Used by the
/// `parse_postfix` call-arm to break out and let implicit multiplication
/// handle `<number>(...)` instead of treating the number as a callee.
fn is_numeric_primary(expr: &Expr) -> bool {
    matches!(
        expr,
        Expr::Literal(
            Literal::Int(_)
                | Literal::Float(_)
                | Literal::Double(_)
                | Literal::Byte(_)
                | Literal::Decimal(_)
                | Literal::Char(_),
            _,
        )
    )
}

/// Whether the upcoming token would start a fresh postfix primary suitable
/// as the RHS of an implicit multiplication (T57).
///
/// Accepts identifiers (`2x`, `3sin(x)`), opening parens (`2(x+y)`), and
/// opening brackets (`2[1,2,3]`). Numeric literals are also accepted so
/// `2 3` parses as `2 * 3` (rare but unambiguous in expression position).
fn is_implicit_mult_start(kind: Option<&TokenKind>) -> bool {
    matches!(
        kind,
        Some(
            TokenKind::Ident(_)
                | TokenKind::LParen
                | TokenKind::LBracket
                | TokenKind::IntLit(_)
                | TokenKind::FloatLit(_)
                | TokenKind::DoubleLit(_)
                | TokenKind::ByteLit(_)
                | TokenKind::DecimalLit(_)
        )
    )
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
    // T57: Unicode prefix operators ∑ ∏ √ desugar to free prelude function
    // calls (`sum(expr)` / `product(expr)` / `sqrt(expr)`). The operand is
    // parsed at the unary level so `√x + 1` parses as `(sqrt(x)) + 1`, not
    // `sqrt(x + 1)` — matching Julia's high-precedence prefix behaviour.
    // Rejected in the Standard edition via `Edition::require_for`.
    if let Some(kind) = stream.peek_kind().cloned() {
        if let Some(callee_name) = unicode_prefix_desugar(&kind) {
            stream.edition().require_for(&kind).map_err(|msg| {
                ParseError::new(Diagnostic::error(
                    msg,
                    stream
                        .peek()
                        .map(|t| t.span)
                        .unwrap_or_else(|| stream.eof_span()),
                ))
            })?;
            let prefix_tok = stream
                .advance()
                .expect("peek guaranteed a Unicode prefix token");
            let operand = parse_unary(stream)?;
            let span = Span::new(
                prefix_tok.span.start,
                operand.span().end,
                stream.source_id(),
            );
            let callee = Expr::Ident(
                Ident::new(callee_name.to_string(), prefix_tok.span),
                prefix_tok.span,
            );
            return Ok(Expr::FuncCall {
                callee: Box::new(callee),
                args: vec![operand],
                span,
            });
        }
    }
    parse_postfix(stream)
}

/// Maps a Unicode prefix operator token to its ASCII prelude-function name
/// (T57). Returns `Some(name)` for `∑`/`∏`/`√` and `None` for every other
/// token kind. The ASCII alternative is the documented contract: a user can
/// always write `sum(xs)` instead of `∑ xs`.
fn unicode_prefix_desugar(kind: &TokenKind) -> Option<&'static str> {
    match kind {
        TokenKind::Sum => Some("sum"),
        TokenKind::Product => Some("product"),
        TokenKind::Sqrt => Some("sqrt"),
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// Level 13 — postfix: function call `(...)` and method call `.method(...)`.
// ---------------------------------------------------------------------------

fn parse_postfix(stream: &mut TokenStream<'_>) -> Result<Expr, ParseError> {
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
fn parse_call_args(stream: &mut TokenStream<'_>) -> Result<Vec<Expr>, ParseError> {
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
fn parse_one_call_arg(stream: &mut TokenStream<'_>) -> Result<Expr, ParseError> {
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

// ---------------------------------------------------------------------------
// Level 14 — primary: literals, identifiers, parenthesized expressions.
// ---------------------------------------------------------------------------

/// Parse a `[...]` collection literal, dispatching between the historical
/// comma-separated `ArrayLit` (T23) and the scientific-edition matrix
/// literal `[1 2; 3 4]` (T57).
///
/// The two shapes are distinguished by content, NOT by edition:
/// - `[1, 2, 3]` (only commas) → flat `ArrayLit` (backward compat, BOTH
///   editions).
/// - `[1 2 3]` (whitespace-separated, no commas) → nested `ArrayLit` (a
///   1-row matrix). Scientific edition only.
/// - `[1; 2; 3]` (semicolons) → nested `ArrayLit` (a column vector).
///   Scientific edition only.
/// - `[1 2; 3 4]` (whitespace + semicolons) → nested `ArrayLit` (2x2
///   matrix). Scientific edition only.
///
/// The dispatch is content-driven because `[1, 2, 3]` is unambiguously a
/// comma-separated Vector in either edition (preserving the standard
/// edition's parsing 100% unchanged), while any non-comma separation is
/// unambiguously a matrix (the standard edition would reject such input
/// as a parse error anyway — `[1 2]` is not a valid Vector literal under
/// historical rules).
fn parse_collection_literal(stream: &mut TokenStream<'_>) -> Result<Expr, ParseError> {
    let lb = stream.expect(TokenKind::LBracket)?;
    let source_id = stream.source_id();
    // Empty `[]` → empty ArrayLit (historical).
    if matches!(stream.peek_kind(), Some(TokenKind::RBracket)) {
        let rb = stream.expect(TokenKind::RBracket)?;
        let span = Span::new(lb.span.start, rb.span.end, source_id);
        return Ok(Expr::ArrayLit {
            elements: Vec::new(),
            span,
        });
    }
    // Parse first row, tracking separator usage. If neither whitespace nor
    // `;` was seen, the row's contents are the comma-separated Vector
    // elements (or a single element) — fall through to the historical
    // shape by continuing the comma loop inline.
    let (mut row_elements, saw_whitespace_sep) = parse_matrix_row(stream)?;
    let is_matrix = saw_whitespace_sep || matches!(stream.peek_kind(), Some(TokenKind::Semicolon));
    if is_matrix {
        // Scientific-edition gate: matrix literals are an opt-in extension.
        // Uses a direct edition check (NOT `Edition::require_for`) because
        // the trigger token here is `Semicolon` (or whitespace separation),
        // neither of which is a token-level edition marker — the matrix
        // SYNTACTIC FORM is what's gated, not a single token.
        if !stream.edition().is_scientific() {
            return Err(ParseError::new(Diagnostic::error(
                "matrix literals (`[1 2; 3 4]`) require `edition = \"scientific\"` in buff.toml",
                lb.span,
            )));
        }
        let mut rows: Vec<Expr> = Vec::new();
        let row_span_start = row_elements
            .first()
            .map(|e| e.span().start)
            .unwrap_or(lb.span.start);
        let row_span_end = row_elements
            .last()
            .map(|e| e.span().end)
            .unwrap_or(lb.span.end);
        rows.push(Expr::ArrayLit {
            elements: std::mem::take(&mut row_elements),
            span: Span::new(row_span_start, row_span_end, source_id),
        });
        while matches!(stream.peek_kind(), Some(TokenKind::Semicolon)) {
            stream.advance();
            // Allow trailing semicolon: `[1; 2; 3;]`.
            if matches!(stream.peek_kind(), Some(TokenKind::RBracket)) {
                break;
            }
            let (mut elems, _) = parse_matrix_row(stream)?;
            let s = elems
                .first()
                .map(|e| e.span().start)
                .unwrap_or_else(|| stream.span_here().start);
            let e = elems
                .last()
                .map(|e| e.span().end)
                .unwrap_or_else(|| stream.span_here().end);
            rows.push(Expr::ArrayLit {
                elements: std::mem::take(&mut elems),
                span: Span::new(s, e, source_id),
            });
        }
        let rb = stream.expect(TokenKind::RBracket)?;
        let span = Span::new(lb.span.start, rb.span.end, source_id);
        return Ok(Expr::ArrayLit {
            elements: rows,
            span,
        });
    }
    // Comma-separated shape: continue the historical ArrayLit loop, picking
    // up where `parse_matrix_row` left off (it already collected the first
    // element + any comma-separated ones).
    while matches!(stream.peek_kind(), Some(TokenKind::Comma)) {
        stream.advance();
        if matches!(stream.peek_kind(), Some(TokenKind::RBracket)) {
            break;
        }
        row_elements.push(parse_expression(stream)?);
    }
    let rb = stream.expect(TokenKind::RBracket)?;
    let span = Span::new(lb.span.start, rb.span.end, source_id);
    Ok(Expr::ArrayLit {
        elements: row_elements,
        span,
    })
}

/// Parse ONE row of a matrix literal: a sequence of expressions separated
/// by whitespace and/or commas, terminated by `;` or `]` (which are NOT
/// consumed here — the caller handles them).
///
/// Returns `(elements, saw_whitespace_separator)`. The flag is `true` when
/// at least one pair of elements was separated by whitespace (no comma
/// between them) — that's the signal the caller uses to detect a
/// scientific-edition matrix vs. a historical comma-separated Vector.
///
/// Inside the [`TokenStream`] layer, whitespace is already collapsed — the
/// parser sees two consecutive expression-starter tokens (`IntLit` `IntLit`)
/// as the marker of whitespace separation. This matches how Julia's lexer
/// handles matrix literals.
fn parse_matrix_row(stream: &mut TokenStream<'_>) -> Result<(Vec<Expr>, bool), ParseError> {
    // T57: while inside a matrix row, implicit multiplication is SUPPRESSED
    // — whitespace is the row-element separator, not juxtaposition. The
    // enter/exit bracket makes this local to this function; the rest of
    // the parser is unaffected.
    stream.enter_matrix_row();
    let result = parse_matrix_row_inner(stream);
    stream.exit_matrix_row();
    result
}

fn parse_matrix_row_inner(stream: &mut TokenStream<'_>) -> Result<(Vec<Expr>, bool), ParseError> {
    let mut elements: Vec<Expr> = Vec::new();
    let mut saw_whitespace_sep = false;
    elements.push(parse_expression(stream)?);
    loop {
        match stream.peek_kind() {
            Some(TokenKind::Comma) => {
                stream.advance();
                if matches!(
                    stream.peek_kind(),
                    Some(TokenKind::RBracket) | Some(TokenKind::Semicolon)
                ) {
                    break;
                }
                elements.push(parse_expression(stream)?);
            }
            // Whitespace-separated continuation: any token that could start
            // a fresh expression. In scientific-edition matrix context this
            // means `[1 2 3]` parses as three elements.
            Some(k) if starts_matrix_element(k) => {
                saw_whitespace_sep = true;
                elements.push(parse_expression(stream)?);
            }
            _ => break,
        }
    }
    Ok((elements, saw_whitespace_sep))
}

/// Whether a token kind can begin a fresh matrix-row element (T57). Used
/// by [`parse_matrix_row`] to detect whitespace-separated continuation.
fn starts_matrix_element(kind: &TokenKind) -> bool {
    matches!(
        kind,
        TokenKind::IntLit(_)
            | TokenKind::FloatLit(_)
            | TokenKind::DoubleLit(_)
            | TokenKind::ByteLit(_)
            | TokenKind::DecimalLit(_)
            | TokenKind::CharLit(_)
            | TokenKind::StringStart
            | TokenKind::RegexLit(_)
            | TokenKind::Ident(_)
            | TokenKind::LParen
            | TokenKind::LBracket
            | TokenKind::LBrace
            | TokenKind::Minus
            | TokenKind::Not
            | TokenKind::Tilde
            | TokenKind::KwTrue
            | TokenKind::KwFalse
            | TokenKind::KwIf
            | TokenKind::KwMatch
            | TokenKind::KwSpawn
            | TokenKind::Sum
            | TokenKind::Product
            | TokenKind::Sqrt
    )
}

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
        stream.expect(TokenKind::FatArrow)?;
        let body_expr = parse_expression(stream)?;
        // Wrap the body in a one-statement block (consistent with closures).
        let arm_tok_span = pat.span();
        arm_end = body_expr.span().end;
        let body = Block {
            stmts: vec![Stmt::ExprStmt(body_expr, arm_tok_span)],
            span: Span::new(arm_tok_span.start, arm_end, source_id),
        };
        arms.push(MatchArm {
            pattern: pat,
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

/// Parse a single pattern (T27, extended in T71).
///
/// This parser is shared by `match` arms and `let`-destructuring bindings, so
/// extending it benefits both contexts.
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
/// recursively call `parse_pattern`, so nesting (`Ok(Err(_))`, `(a, (b, c))`,
/// `Outer { inner: Inner { x } }`) works. Trailing comma is allowed in all
/// delimited forms.
///
/// # Errors
///
/// Returns [`ParseError`] if a subpattern fails to parse or a closing
/// delimiter (`)` or `}`) is missing.
pub fn parse_pattern(stream: &mut TokenStream<'_>) -> Result<Pattern, ParseError> {
    let source_id = stream.source_id();
    // Wildcard `_`. The lexer produces this as `Ident("_")` (underscore is a
    // valid identifier character), so we detect the wildcard by matching the
    // ident's NAME rather than a dedicated token kind.
    if let Some(TokenKind::Ident(name)) = stream.peek_kind() {
        if name == "_" {
            let tok = stream.advance().expect("peek guaranteed `_`");
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
                unreachable!("matched Ident above");
            };
            let ident = Ident::new(name, tok.span);
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
            // allowed.
            if matches!(stream.peek_kind(), Some(TokenKind::LBrace)) {
                stream.expect(TokenKind::LBrace)?; // consume `{`
                let mut fields: Vec<(Ident, Pattern)> = Vec::new();
                if !matches!(stream.peek_kind(), Some(TokenKind::RBrace)) {
                    loop {
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
fn parse_closure(stream: &mut TokenStream<'_>) -> Result<Expr, ParseError> {
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
            unreachable!("matched Ident above");
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

/// Speculative lookahead: return `true` if the cursor is positioned at an
/// `LBrace` whose contents match the struct-init field shape (T26).
///
/// Used by [`parse_postfix`] to disambiguate `Ident { ... }` (struct init)
/// from block bodies that happen to follow an Ident-typed expression (e.g.
/// `if cond { ... }`, `for x in iter { ... }`).
///
/// Returns `true` when the token AT the cursor is `{` AND the first
/// significant token AFTER `{` is either:
/// - `}` — empty struct-init `Type { }`, or
/// - `Ident` followed by `:` — the start of `field: value`.
///
/// Returns `false` for any other shape (e.g. `{ 1 }`, `{ print(x) }` —
/// these are block bodies, not struct-inits).
///
/// Implementation: save the cursor, advance past `{`, peek the next two
/// significant tokens, then restore. The cursor is UNCHANGED on return.
fn cursor_at_struct_init_body(stream: &mut TokenStream<'_>) -> bool {
    // The caller already saw `{` at the cursor via `peek_kind`; re-check.
    if !matches!(stream.peek_kind(), Some(TokenKind::LBrace)) {
        return false;
    }
    let saved = stream.save();
    // Consume the `{` then peek the next two significant tokens.
    stream.advance(); // past `{`
    let result = match stream.peek_kind().cloned() {
        Some(TokenKind::RBrace) => true,
        Some(TokenKind::Ident(_)) => {
            matches!(stream.peek_second_kind(), Some(TokenKind::Colon))
        }
        _ => false,
    };
    stream.restore(saved);
    result
}

///
/// Called by [`parse_postfix`] AFTER the opening `{` has been consumed
/// (i.e. the cursor is positioned AT the first field name, or `}` for an
/// empty struct-init). The closing `}` is left for the caller to expect.
///
/// Shape: each entry is `ident : expr` (named field). Trailing comma is
/// allowed. Returns the list of `(Ident, Expr)` pairs.
///
/// This is structurally similar to [`parse_map_literal`]'s entry loop, but
/// the KEY difference is that map keys are arbitrary expressions while
/// struct-init field names are bare identifiers. The two parsers don't
/// share code because the entry-point disambiguation already runs in
/// `parse_brace_primary` (which only fires at PRIMARY position) — by the
/// time `parse_struct_init_fields` runs, we KNOW we're in a struct-init
/// (the leading `Type` Ident has been consumed).
fn parse_struct_init_fields(
    stream: &mut TokenStream<'_>,
) -> Result<Vec<(Ident, Expr)>, ParseError> {
    let mut fields: Vec<(Ident, Expr)> = Vec::new();
    // Empty struct-init `Type { }` — no fields.
    if matches!(stream.peek_kind(), Some(TokenKind::RBrace)) {
        return Ok(fields);
    }
    loop {
        // Field name MUST be a bare identifier.
        let Some(tok) = stream.advance() else {
            return Err(ParseError::new(Diagnostic::error(
                "expected struct field name, found end of input",
                stream.eof_span(),
            )));
        };
        let TokenKind::Ident(name) = tok.kind.clone() else {
            return Err(ParseError::new(Diagnostic::error(
                format!(
                    "expected struct field name (identifier), found `{}`",
                    tok.kind
                ),
                tok.span,
            )));
        };
        let field_ident = Ident::new(name, tok.span);
        // `:` separator between field name and value.
        stream.expect(TokenKind::Colon)?;
        // Field value is a full expression.
        let value = parse_expression(stream)?;
        fields.push((field_ident, value));
        if matches!(stream.peek_kind(), Some(TokenKind::Comma)) {
            stream.advance(); // consume ','
                              // Allow trailing comma: `Type { a: 1, }`.
            if matches!(stream.peek_kind(), Some(TokenKind::RBrace)) {
                break;
            }
            continue;
        }
        break;
    }
    Ok(fields)
}

/// Parse a map literal `{"k": v, ...}` or `{:}` (empty) whose opening `{` is
/// the next significant token (T25).
///
/// Shape: `{ key: value (, key: value)* ,? }`. Each entry is a colon-separated
/// `(key, value)` pair of arbitrary expressions. Trailing comma is allowed.
/// The empty form is `{:}` (bare `{}` is rejected — it's ambiguous with code
/// blocks per the layout rules). Builds an [`Expr::MapLit`].
fn parse_map_literal(stream: &mut TokenStream<'_>) -> Result<Expr, ParseError> {
    let lb = stream.expect(TokenKind::LBrace)?;
    // Empty map: `{:}`. The `:` is required to disambiguate from a bare `{}`
    // (which would be ambiguous with an empty code block — Buff's layout
    // rules reserve bare `{}` for future use, never as a value).
    if matches!(stream.peek_kind(), Some(TokenKind::Colon)) {
        stream.advance(); // consume ':'
        let rb = stream.expect(TokenKind::RBrace)?;
        let span = Span::new(lb.span.start, rb.span.end, stream.source_id());
        return Ok(Expr::MapLit {
            entries: Vec::new(),
            span,
        });
    }
    // One-or-more entries: parse `key: value` pairs separated by commas.
    let mut entries: Vec<(Expr, Expr)> = Vec::new();
    loop {
        let key = parse_expression(stream)?;
        stream.expect(TokenKind::Colon)?;
        let value = parse_expression(stream)?;
        entries.push((key, value));
        if matches!(stream.peek_kind(), Some(TokenKind::Comma)) {
            stream.advance(); // consume ','
                              // Allow trailing comma: `{"a": 1,}`.
            if matches!(stream.peek_kind(), Some(TokenKind::RBrace)) {
                break;
            }
            continue;
        }
        break;
    }
    let rb = stream.expect(TokenKind::RBrace)?;
    let span = Span::new(lb.span.start, rb.span.end, stream.source_id());
    Ok(Expr::MapLit { entries, span })
}

/// Disambiguate a `{` at primary position into a closure or a map literal
/// (T25).
///
/// Buff uses braces for three things at primary position:
/// 1. **Closure**: `{ x, y => x + y }` — params followed by `=>`.
/// 2. **Map literal**: `{"k": v, ...}` — entries of `key: value`.
/// 3. **Empty map**: `{:}` — explicit empty marker.
///
/// (Code blocks come through `if`/`for`/etc. via layout rules — never at
/// primary position. Struct-init `Type { ... }` is not yet wired through
/// `parse_primary`; when it lands, it'll start with an Ident for `Type`, not
/// a bare `{`.)
///
/// **Disambiguation strategy**: speculative parsing with cursor save/restore.
/// Save the position, try `parse_closure`; if it succeeds, return its result.
/// If it fails, restore the position and try `parse_map_literal`. If both
/// fail, return the closure's error (closures are the historical default
/// since T23, so a bare `{` reads as "attempt closure first" — preserving
/// backwards compatibility for existing closure-using programs).
///
/// This preserves all existing block/closure/struct-init parsing because it
/// only changes what happens *after* a `{` is seen at primary position, and
/// it tries the closure shape first.
fn parse_brace_primary(stream: &mut TokenStream<'_>) -> Result<Expr, ParseError> {
    let saved = stream.save();
    // Try the closure shape first (historical default since T23).
    match parse_closure(stream) {
        Ok(expr) => Ok(expr),
        Err(closure_err) => {
            // Roll back and try the map shape.
            stream.restore(saved);
            match parse_map_literal(stream) {
                Ok(expr) => Ok(expr),
                // Both failed — prefer the closure error (it's the more
                // common shape and produces the more helpful message when
                // a user really meant a closure but mistyped).
                Err(map_err) => {
                    // If the map parse made MORE progress than the closure
                    // parse (consumed more tokens), prefer the map error —
                    // it's a stronger signal that the user meant a map.
                    let map_progress = stream.save();
                    stream.restore(saved);
                    let _ = parse_closure(stream);
                    let closure_progress = stream.save();
                    stream.restore(map_progress.min(closure_progress));
                    if map_progress > closure_progress {
                        Err(map_err)
                    } else {
                        Err(closure_err)
                    }
                }
            }
        }
    }
}

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

    // T27: `match scrutinee { arms }` — pattern-matching expression. Like
    // `if`, `match` is a primary expression that can appear anywhere
    // (e.g. `let x = match c { Red => 1, _ => 0 }`).
    if matches!(tok.kind, TokenKind::KwMatch) {
        return parse_match(stream);
    }

    // T31: `spawn <expr>` — async task spawn. `spawn` is a reserved
    // keyword (`TokenKind::KwSpawn`), so it can never be parsed as an
    // ordinary identifier. The operand is the next primary expression
    // (which goes through the full postfix chain after parse_primary
    // returns, so `spawn task()` parses as `Expr::Spawn { task: Call(task) }`
    // — exactly the shape codegen wants).
    if matches!(tok.kind, TokenKind::KwSpawn) {
        let spawn_tok = stream.advance().expect("peek guaranteed KwSpawn");
        let start = spawn_tok.span.start;
        // Parse the task body. Use `parse_unary` (one level above
        // parse_postfix) so `spawn task()` captures the full call, not
        // just the bare `task`. The task expression's end determines the
        // span's end.
        let task = parse_unary(stream)?;
        let end = task.span().end;
        return Ok(Expr::Spawn {
            task: Box::new(task),
            span: Span::new(start, end, stream.source_id()),
        });
    }

    // If it's an open paren, parse a parenthesized expression. T103: a
    // comma-separated list with 2+ elements is a TUPLE `(e1, e2, ...)` →
    // `Expr::TupleLit`; a single `( e )` is grouping → return `e` (existing
    // behaviour, zero regression). Trailing comma `(a, b,)` is allowed and
    // produces a 2-element tuple (NOT a 1-element `(a,)` — Buff does not
    // have single-element tuples at the value layer for v0.5).
    if matches!(tok.kind, TokenKind::LParen) {
        let lp = stream.expect(TokenKind::LParen)?;
        let source_id = stream.source_id();
        // Empty `()` is not a valid expression in v0.5 (unit is not a value).
        if matches!(stream.peek_kind(), Some(TokenKind::RParen)) {
            return Err(ParseError::new(Diagnostic::error(
                "empty `()` is not a valid expression",
                lp.span,
            )));
        }
        let first = parse_expression(stream)?;
        // No comma → plain grouping `( expr )`. Return `first` (no span wrap).
        if !matches!(stream.peek_kind(), Some(TokenKind::Comma)) {
            stream.expect(TokenKind::RParen)?;
            return Ok(first);
        }
        // One or more commas → tuple. Collect the rest of the members.
        let mut members = vec![first];
        loop {
            // We saw a comma (or the loop advanced past one); consume it.
            stream.advance(); // consume `,`
                              // Trailing comma: `(a, b,)` is allowed and terminates the tuple.
            if matches!(stream.peek_kind(), Some(TokenKind::RParen)) {
                break;
            }
            members.push(parse_expression(stream)?);
            if !matches!(stream.peek_kind(), Some(TokenKind::Comma)) {
                break;
            }
        }
        let rp = stream.expect(TokenKind::RParen)?;
        let span = Span::new(lp.span.start, rp.span.end, source_id);
        // The 2+-element disambiguation: a single `(e,)` would be a 1-element
        // tuple, which Buff does not support at the value layer for v0.5.
        // The parser produces grouping instead — drop the trailing comma's
        // effect by returning the lone member when there's exactly one.
        // (In practice `(e,)` reaches here as a 1-element vec; we treat it
        // as grouping `(e)` so the value layer matches the type layer's
        // single-element rule.)
        if members.len() == 1 {
            return Ok(members.swap_remove(0));
        }
        return Ok(Expr::TupleLit(members, span));
    }

    // T23: A collection literal `[e1, e2, ...]` (or empty `[]`). Allow a
    // trailing comma. The element expressions are full expressions so
    // `[a + b, f(x)]` works.
    //
    // T57: In the scientific edition, `[` can ALSO begin a matrix literal
    // — `[1 2 3]` (row vector), `[1; 2; 3]` (column vector),
    // `[1 2; 3 4]` (2x2). The disambiguation is done inside
    // [`parse_collection_literal`]: if the contents use `;` as a row
    // separator OR use whitespace as an element separator (no comma), it
    // is a matrix (nested `ArrayLit`); otherwise it falls through to the
    // historical comma-separated `ArrayLit` shape.
    if matches!(tok.kind, TokenKind::LBracket) {
        return parse_collection_literal(stream);
    }

    // T25: A `{` at primary position is ambiguous between a closure
    // `{ params => expr }` (T23) and a map literal `{"k": v, ...}` (T25).
    // Speculative save/restore picks the right shape; see
    // `parse_brace_primary` for the disambiguation contract.
    if matches!(tok.kind, TokenKind::LBrace) {
        return parse_brace_primary(stream);
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
        // T21: `'A'`, `'é'`, `'🚀'` parse as a Char literal (single quotes).
        TokenKind::CharLit(c) => Expr::Literal(Literal::Char(*c), span),
        // T20: Decimal literal — raw text carried straight from the lexer so
        // exactness is preserved through to `dec!()` codegen.
        TokenKind::DecimalLit(s) => Expr::Literal(Literal::Decimal(s.clone()), span),
        // T79: Regex literal — the raw pattern text (between the slashes) is
        // carried straight from the lexer. Codegen is a documented stub in
        // v0.5 (no `regex` crate in the generated project yet); see the
        // `Literal::Regex` doc on the AST for the deferral rationale.
        TokenKind::RegexLit(s) => Expr::Literal(Literal::Regex(s.clone()), span),
        TokenKind::KwTrue => Expr::Literal(Literal::Bool(true), span),
        TokenKind::KwFalse => Expr::Literal(Literal::Bool(false), span),
        TokenKind::Ident(name) => Expr::Ident(Ident::new(name.clone(), span), span),
        // A string literal (single- or triple-quoted). The lexer emits the
        // same token sequence for both; triple-quote is just raw (no escapes,
        // no interpolation). Both arrive as:
        //   StringStart [StringPart|InterpStart ... InterpEnd]* StringEnd
        TokenKind::StringStart => parse_string_literal(stream, span)?,
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
/// consumed: we are now at the optional `StringPart`/`InterpStart`/`StringEnd`
/// sequence.
///
/// `start_span` is the span of the opening `"` (or `"""`) so we can build a
/// full-string span even when the literal is empty.
///
/// Behaviour (T21):
/// - If the literal contains NO interpolation (`InterpStart`), collapse the
///   `StringPart` text runs into one `Literal::String`. This preserves the
///   pre-T21 fast path so `"abc"` still lowers to `Literal::String("abc")`.
/// - If at least one `{expr}` is present, build an `Expr::StringInterp`
///   whose `parts` is a `Vec<InterpPart>` alternating between literal text
///   runs and embedded expressions.
/// - The interpolation expressions are parsed by the *full* expression
///   parser so `{a + b * c}` works. Layout tokens are NOT significant inside
///   the interpolation — they were lexed in "no-indent" mode by the lexer's
///   `InterpLexer` callback.
fn parse_string_literal(
    stream: &mut TokenStream<'_>,
    start_span: Span,
) -> Result<Expr, ParseError> {
    let mut parts: Vec<InterpPart> = Vec::new();
    let mut text_buf = String::new();
    let mut has_interp = false;
    // The end offset is set when we consume `StringEnd` below. The loop only
    // exits via `break`, so the variable is always assigned by then.
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
                text_buf.push_str(&s);
            }
            TokenKind::InterpStart => {
                // Flush accumulated literal text (if any) before the expr.
                if !text_buf.is_empty() {
                    parts.push(InterpPart::Literal(std::mem::take(&mut text_buf)));
                }
                let expr = parse_expression(stream)?;
                parts.push(InterpPart::Expr(Box::new(expr)));
                has_interp = true;
                // The lexer emits InterpEnd immediately after the inner tokens.
                stream.expect(TokenKind::InterpEnd)?;
            }
            TokenKind::StringEnd => {
                end_off = tok.span.end;
                break;
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
    if has_interp {
        // Trailing literal text (e.g. the `!` in `"Hi {name}!"`) still
        // contributes one final Literal part.
        if !text_buf.is_empty() {
            parts.push(InterpPart::Literal(text_buf));
        }
        Ok(Expr::StringInterp { parts, span })
    } else {
        // Fast path: no interpolation — collapse to a plain String literal.
        Ok(Expr::Literal(Literal::String(text_buf), span))
    }
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

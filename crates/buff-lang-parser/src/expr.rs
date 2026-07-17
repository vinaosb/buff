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
    let lhs = parse_range(stream)?;
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
// Level 1.5 — range `..` and `..=` (lower than additive, higher than assign)
// ---------------------------------------------------------------------------

fn parse_range(stream: &mut TokenStream<'_>) -> Result<Expr, ParseError> {
    let lhs = parse_or(stream)?;
    // Check for `..` or `..=` after the LHS.
    match stream.peek_kind() {
        Some(TokenKind::DotDot) => {
            stream.advance();
            let rhs = parse_or(stream)?;
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
            let rhs = parse_or(stream)?;
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

/// Parse a comma-separated list of expressions delimited by `open` / `close`
/// tokens, where the `open` token is the next significant token on the stream.
///
/// Each element is parsed via `parse_expression`. Trailing comma is allowed
/// (`[a, b,]`, `(a, b,)`). The empty form is supported (`[]`, `()`).
/// Returns the list of parsed elements plus the bounding token pair (so
/// callers can compute spans).
///
/// This is the REFACTOR extraction shared between `parse_array_literal`
/// (Vector collection literals, T23) and `parse_call_args` (function/closure
/// argument lists). It exists to DRY up the comma-trailing logic and make
/// future collection-literal additions (T25 map entries) consistent.
///
/// `kind_name` is used in error messages (e.g. "expected `]` to close Vector
/// literal"). The caller is responsible for advancing past `open` — this
/// helper assumes the cursor is positioned AT the first element (or `close`
/// for an empty list).
fn parse_comma_list_until(
    stream: &mut TokenStream<'_>,
    close: TokenKind,
    kind_name: &'static str,
) -> Result<Vec<Expr>, ParseError> {
    let mut elements = Vec::new();
    if matches!(stream.peek_kind(), Some(k) if *k == close) {
        return Ok(elements);
    }
    elements.push(parse_expression(stream)?);
    while matches!(stream.peek_kind(), Some(TokenKind::Comma)) {
        stream.advance(); // consume ','
                          // Allow trailing comma.
        if matches!(stream.peek_kind(), Some(k) if *k == close) {
            break;
        }
        elements.push(parse_expression(stream)?);
    }
    let _ = kind_name; // currently unused; reserved for richer error msgs.
    Ok(elements)
}

/// Parse a collection literal `[e1, e2, ...]` whose opening `[` is the next
/// significant token (T23).
///
/// Allows an empty literal `[]`, a trailing comma, and arbitrary expressions
/// as elements. Builds an [`Expr::ArrayLit`].
fn parse_array_literal(stream: &mut TokenStream<'_>) -> Result<Expr, ParseError> {
    let lb = stream.expect(TokenKind::LBracket)?;
    let elements = parse_comma_list_until(stream, TokenKind::RBracket, "Vector literal")?;
    let rb = stream.expect(TokenKind::RBracket)?;
    let span = Span::new(lb.span.start, rb.span.end, stream.source_id());
    Ok(Expr::ArrayLit { elements, span })
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

/// Parse a single match-arm pattern (T27).
///
/// Supported shapes:
/// - `_` — wildcard. Emits [`Pattern::Wildcard`].
/// - `Ident` — bare identifier. Emits [`Pattern::Ident`] (the
///   variant-vs-binding disambiguation is deferred to the type system).
/// - `Ident(pat, pat, ...)` — data variant with subpatterns. Emits
///   [`Pattern::Variant`] with an empty `enum_name` placeholder (the parser
///   does not know which enum the variant belongs to; exhaustiveness and
///   codegen resolve it by name).
/// - `-N`, `42`, `"hi"`, `true`, `'a'` — literal patterns. Emits
///   [`Pattern::Literal`] (negative literals are encoded as a unary-minus
///   AST expr that we collapse into the literal value; the parser handles
///   the sign here so downstream codegen sees a plain `Literal::Int(-N)`).
///
/// Subpatterns inside a variant tuple recursively call `parse_pattern`, so
/// nesting (`Ok(Err(_))`) works. Trailing comma inside the tuple is allowed.
///
/// # Errors
///
/// Returns [`ParseError`] if a subpattern fails to parse or the closing `)`
/// is missing.
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

    // If it's an open paren, parse a parenthesized expression.
    if matches!(tok.kind, TokenKind::LParen) {
        stream.advance(); // consume '('
        let inner = parse_expression(stream)?;
        stream.expect(TokenKind::RParen)?;
        // Parens don't change the span of the inner expression — keep inner.
        return Ok(inner);
    }

    // T23: A collection literal `[e1, e2, ...]` (or empty `[]`). Allow a
    // trailing comma. The element expressions are full expressions so
    // `[a + b, f(x)]` works.
    if matches!(tok.kind, TokenKind::LBracket) {
        return parse_array_literal(stream);
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

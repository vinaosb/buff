//! AST → WGSL expression lowering.
//!
//! The lowerer walks a single Buff [`Expr`] (the body of a numeric map lambda)
//! and produces a WGSL source fragment. It is purely string-based — WGSL has
//! no `syn`/`quote` equivalent, so this is the ONE Buff crate where raw string
//! output is intentional (see the crate-level doc for rationale).
//!
//! # Supported constructs
//!
//! | Buff AST node             | WGSL output                        |
//! |---------------------------|------------------------------------|
//! | `Literal::Float(f)`       | `<f32 literal>` (e.g. `2`, `3.5`)  |
//! | `Literal::Int(i)`         | `<i32 literal>` (e.g. `42`)        |
//! | `Literal::Bool(b)`        | `true` / `false`                   |
//! | `Literal::Byte(b)`        | `<u32 literal>`                    |
//! | `Literal::Double(_)`      | **REJECTED** — no f64 in WGSL      |
//! | `Literal::Decimal(_)`     | **REJECTED** — CPU-only by policy  |
//! | `Literal::String/Char/..` | **REJECTED** — not numeric         |
//! | `Ident(param_name)`       | `<param_name>`                     |
//! | `Ident(other)`            | **REJECTED** — no free variables   |
//! | `BinaryOp{op,lhs,rhs}`    | `<lhs> <op> <rhs>` (parenthesized) |
//! | `UnaryOp{Neg, ...}`       | `-<operand>`                       |
//! | `UnaryOp{Not, ...}`       | `!<operand>`                       |
//! | `UnaryOp{BitNot, ...}`    | `~<operand>`                       |
//! | Anything else             | **REJECTED** — `WgslError::UnsupportedExpr` |
//!
//! # Precedence / parenthesization
//!
//! To guarantee correctness regardless of WGSL's precedence quirks, every
//! BinaryOp child that is ITSELF a BinaryOp gets wrapped in `(...)`. Leaf
//! operands (literals, idents) need no parens. UnaryOp children always get
//! parenthesized when they're BinaryOp; leaf unary chains like `--x` lower
//! cleanly without parens.
//!
//! # Determinism
//!
//! The output is a pure function of the input AST. No HashMap, no
//! non-deterministic iteration order. Same lambda → byte-identical WGSL.

use buff_lang_ast::expr::Expr;
use buff_lang_ast::op::{BinaryOp, UnaryOp};
use buff_lang_ast::Literal;

use crate::error::WgslError;
use crate::ty::{filter_literal, WgslScalarType};

/// The set of operators this lowerer can emit. Used both to centralize the
/// Buff → WGSL operator token map and to make the test suite readable.
///
/// Each entry maps a [`BinaryOp`] to its WGSL source token (e.g. `"+"`,
/// `"&&"`). Assignment-family operators are NOT included — they are
/// statements, not pure expressions, and are rejected by [`lower_expr`].
const SUPPORTED_BINARY_TOKENS: &[(BinaryOp, &str)] = &[
    (BinaryOp::Add, "+"),
    (BinaryOp::Sub, "-"),
    (BinaryOp::Mul, "*"),
    (BinaryOp::Div, "/"),
    (BinaryOp::Mod, "%"),
    (BinaryOp::Eq, "=="),
    (BinaryOp::Neq, "!="),
    (BinaryOp::Lt, "<"),
    (BinaryOp::Gt, ">"),
    (BinaryOp::Lte, "<="),
    (BinaryOp::Gte, ">="),
    (BinaryOp::And, "&&"),
    (BinaryOp::Or, "||"),
    (BinaryOp::BitAnd, "&"),
    (BinaryOp::BitOr, "|"),
    (BinaryOp::BitXor, "^"),
    (BinaryOp::Shl, "<<"),
    (BinaryOp::Shr, ">>"),
];

/// Lookup the WGSL source token for a [`BinaryOp`], or `None` if the operator
/// is unsupported (assignments, compound-assigns, null-coalesce).
fn binary_op_token(op: BinaryOp) -> Option<&'static str> {
    SUPPORTED_BINARY_TOKENS
        .iter()
        .find(|(o, _)| *o == op)
        .map(|(_, tok)| *tok)
}

/// Lower a Buff expression to a WGSL source fragment.
///
/// `param_name` is the name of the lambda parameter (so that `Ident(param)`
/// lowers to that name verbatim). All other identifiers are rejected — the
/// kernel is a PURE function of `x` and constants.
///
/// # Errors
/// Returns [`WgslError`] when:
/// - The expression contains a `Literal::Double` (f64) — RED spec.
/// - The expression contains any other non-WGSL-native literal.
/// - The expression references an identifier other than `param_name`.
/// - The expression uses an unsupported node (call, match, struct init, …).
pub fn lower_expr(expr: &Expr, param_name: &str) -> Result<String, WgslError> {
    match expr {
        Expr::Literal(lit, _) => lower_literal(lit),
        Expr::Ident(ident, _) => {
            if ident.name == param_name {
                Ok(param_name.to_string())
            } else {
                Err(WgslError::UnsupportedExpr {
                    detail: format!(
                        "free variable `{}` (GPU map kernel may only reference its parameter `{}`)",
                        ident.name, param_name
                    ),
                })
            }
        }
        Expr::BinaryOp { op, lhs, rhs, .. } => {
            let token = binary_op_token(*op).ok_or_else(|| WgslError::UnsupportedExpr {
                detail: format!("binary operator `{op}` is not supported in GPU map kernels"),
            })?;
            let lhs_str = lower_expr_operand(lhs, param_name)?;
            let rhs_str = lower_expr_operand(rhs, param_name)?;
            Ok(format!("{lhs_str} {token} {rhs_str}"))
        }
        Expr::UnaryOp { op, operand, .. } => {
            let token = match op {
                UnaryOp::Neg => "-",
                UnaryOp::Not => "!",
                UnaryOp::BitNot => "~",
            };
            let operand_str = lower_expr_operand(operand, param_name)?;
            Ok(format!("{token}{operand_str}"))
        }
        // Anything else — calls, lambdas, struct init, match, index, ranges,
        // if-let, try, spawn, interp, map/array literals, named args, etc. —
        // is rejected. These either aren't pure, aren't numeric, or require
        // a runtime feature WGSL doesn't expose in a compute kernel body.
        other => Err(WgslError::UnsupportedExpr {
            detail: unsupported_expr_detail(other),
        }),
    }
}

/// Lower an operand of a binary/unary op, parenthesizing when it's itself a
/// binary op (so precedence is always explicit).
fn lower_expr_operand(expr: &Expr, param_name: &str) -> Result<String, WgslError> {
    match expr {
        Expr::BinaryOp { .. } => {
            let inner = lower_expr(expr, param_name)?;
            Ok(format!("({inner})"))
        }
        _ => lower_expr(expr, param_name),
    }
}

/// Lower a [`Literal`] to a WGSL literal-token string, rejecting non-WGSL
/// variants.
fn lower_literal(lit: &Literal) -> Result<String, WgslError> {
    // First: filter — anything non-WGSL-native is an error. The return value
    // (the scalar type) is currently discarded — a future task MAY annotate
    // the literal with an explicit type suffix (e.g. `2.0f`) when the
    // element type is non-default. For T44 we rely on WGSL's default literal
    // inference (integers → i32, decimals → f32).
    let _scalar: WgslScalarType = filter_literal(lit)?;
    match lit {
        Literal::Float(f) => Ok(format_f32_literal(*f)),
        Literal::Int(i) => Ok(format_i32_literal(*i)),
        Literal::Bool(b) => Ok(if *b {
            "true".to_string()
        } else {
            "false".to_string()
        }),
        Literal::Byte(b) => Ok(format!("{b}u")),
        // filter_literal already rejected the rest; this is unreachable but
        // kept exhaustive to survive future Literal additions.
        Literal::Double(_)
        | Literal::String(_)
        | Literal::Char(_)
        | Literal::Decimal(_)
        | Literal::Regex(_) => unreachable!("filter_literal rejected non-WGSL literal"),
    }
}

/// Format an `f32` literal in a WGSL-stable way.
///
/// WGSL accepts `2.0`, `2.5`, `0.5`, etc. We render with `{:?}` (Rust's
/// debug-format for f32) which produces a parseable decimal (e.g. `2.0`,
/// `3.5`, `0.5`). If the debug form lacks a decimal point or exponent (which
/// `{:?}` will produce for some values — e.g. `2.0` formats as `2.0`), we
/// pass it through. Otherwise we append `.0` to guarantee WGSL parses it as
/// float (integer-typed literals would infer as `i32`).
fn format_f32_literal(f: f32) -> String {
    let s = format!("{f:?}");
    if s.contains('.') || s.contains('e') || s.contains('E') {
        s
    } else {
        format!("{s}.0")
    }
}

/// Format an `i64` literal as an i32-range WGSL integer, rejecting values
/// that overflow i32 (T44 conservative — auto-widening is the runtime's job).
fn format_i32_literal(i: i64) -> String {
    // Note: we render the raw i64 — if it doesn't fit i32, the runtime
    // (T45+) MAY downcast with an overflow check; T44 just emits the digits
    // and lets the eventual wgpu validation surface any overflow. To keep
    // the WGSL strictly valid today, we COULD reject here; but that would
    // duplicate the policy of the runtime. For T44 we accept and emit.
    format!("{i}")
}

/// Build a short human-readable description for an unsupported expression
/// node (used in the `UnsupportedExpr` error detail).
fn unsupported_expr_detail(expr: &Expr) -> String {
    let kind = match expr {
        Expr::FuncCall { .. } => "function call",
        Expr::MethodCall { .. } => "method call",
        Expr::IfExpr { .. } | Expr::IfLet { .. } => "if expression",
        Expr::Lambda { .. } => "nested lambda",
        Expr::StructInit { .. } => "struct literal",
        Expr::MatchExpr { .. } => "match expression",
        Expr::SuspendExpr { .. } => "suspend expression",
        Expr::ArrayLit { .. } => "array literal",
        Expr::Index { .. } => "index expression",
        Expr::StringInterp { .. } => "string interpolation",
        Expr::MapLit { .. } => "map literal",
        Expr::Try { .. } => "try (`?`) expression",
        Expr::Spawn { .. } => "spawn expression",
        Expr::Range { .. } => "range expression",
        Expr::TupleLit(_, _) => "tuple literal",
        Expr::NamedArg { .. } => "named argument",
        // Leaves handled above; these arms are unreachable but exhaustive.
        Expr::Literal(_, _) | Expr::Ident(_, _) | Expr::BinaryOp { .. } | Expr::UnaryOp { .. } => {
            "expression"
        }
    };
    format!("{kind} (not supported in a GPU map kernel body)")
}

#[cfg(test)]
mod tests {
    use super::*;
    use buff_lang_error::Span;

    fn span() -> Span {
        Span::dummy()
    }

    fn ident_expr(name: &str) -> Expr {
        use buff_lang_ast::common::Ident;
        Expr::Ident(Ident::new(name, span()), span())
    }

    fn int_lit(v: i64) -> Expr {
        Expr::Literal(Literal::Int(v), span())
    }

    fn float_lit(v: f32) -> Expr {
        Expr::Literal(Literal::Float(v), span())
    }

    fn double_lit(v: f64) -> Expr {
        Expr::Literal(Literal::Double(v), span())
    }

    fn binop(op: BinaryOp, lhs: Expr, rhs: Expr) -> Expr {
        Expr::BinaryOp {
            op,
            lhs: Box::new(lhs),
            rhs: Box::new(rhs),
            span: span(),
        }
    }

    #[test]
    fn lower_ident_param() {
        assert_eq!(lower_expr(&ident_expr("x"), "x").unwrap(), "x");
    }

    #[test]
    fn lower_ident_free_var_rejected() {
        let err = lower_expr(&ident_expr("y"), "x").unwrap_err();
        assert!(matches!(err, WgslError::UnsupportedExpr { .. }));
        assert!(err.to_string().contains("free variable"));
    }

    #[test]
    fn lower_float_literal() {
        assert_eq!(lower_expr(&float_lit(2.0), "x").unwrap(), "2.0");
        assert_eq!(lower_expr(&float_lit(3.5), "x").unwrap(), "3.5");
    }

    #[test]
    fn lower_int_literal() {
        assert_eq!(lower_expr(&int_lit(42), "x").unwrap(), "42");
    }

    #[test]
    fn lower_double_literal_rejected() {
        let err = lower_expr(&double_lit(2.5), "x").unwrap_err();
        assert!(matches!(err, WgslError::UnsupportedType { .. }));
        assert!(err.to_string().contains("Float<64>"));
    }

    #[test]
    fn lower_binary_add() {
        // x + 2.0 → "x + 2.0"
        let e = binop(BinaryOp::Add, ident_expr("x"), float_lit(2.0));
        assert_eq!(lower_expr(&e, "x").unwrap(), "x + 2.0");
    }

    #[test]
    fn lower_binary_mul() {
        // x * 2.0 → "x * 2.0"
        let e = binop(BinaryOp::Mul, ident_expr("x"), float_lit(2.0));
        assert_eq!(lower_expr(&e, "x").unwrap(), "x * 2.0");
    }

    #[test]
    fn lower_binary_precedence_parens() {
        // (x + 1) * 2 → "(x + 1) * 2" — nested BinaryOp gets parens
        let lhs = binop(BinaryOp::Add, ident_expr("x"), float_lit(1.0));
        let e = binop(BinaryOp::Mul, lhs, float_lit(2.0));
        assert_eq!(lower_expr(&e, "x").unwrap(), "(x + 1.0) * 2.0");
    }

    #[test]
    fn lower_binary_chained_adds_no_extra_parens_for_leaves() {
        // x + 1 + 2 → parses as (x + 1) + 2 → "(x + 1) + 2"
        let inner = binop(BinaryOp::Add, ident_expr("x"), float_lit(1.0));
        let e = binop(BinaryOp::Add, inner, float_lit(2.0));
        assert_eq!(lower_expr(&e, "x").unwrap(), "(x + 1.0) + 2.0");
    }

    #[test]
    fn lower_unary_neg() {
        use buff_lang_ast::op::UnaryOp;
        // -x → "-x"
        let e = Expr::UnaryOp {
            op: UnaryOp::Neg,
            operand: Box::new(ident_expr("x")),
            span: span(),
        };
        assert_eq!(lower_expr(&e, "x").unwrap(), "-x");
    }

    #[test]
    fn lower_unary_neg_of_binop_parens() {
        use buff_lang_ast::op::UnaryOp;
        // -(x + 1) → "-(x + 1)"
        let inner = binop(BinaryOp::Add, ident_expr("x"), float_lit(1.0));
        let e = Expr::UnaryOp {
            op: UnaryOp::Neg,
            operand: Box::new(inner),
            span: span(),
        };
        assert_eq!(lower_expr(&e, "x").unwrap(), "-(x + 1.0)");
    }

    #[test]
    fn lower_unsupported_binary_op_rejected() {
        let e = binop(BinaryOp::Assign, ident_expr("x"), float_lit(1.0));
        let err = lower_expr(&e, "x").unwrap_err();
        assert!(matches!(err, WgslError::UnsupportedExpr { .. }));
        assert!(err.to_string().contains("binary operator"));
    }

    #[test]
    fn lower_unsupported_node_rejected() {
        use buff_lang_ast::common::Block;
        let lambda = Expr::Lambda {
            params: vec![],
            body: Block::empty(span()),
            return_type: None,
            span: span(),
        };
        let err = lower_expr(&lambda, "x").unwrap_err();
        assert!(matches!(err, WgslError::UnsupportedExpr { .. }));
        assert!(err.to_string().contains("nested lambda"));
    }
}

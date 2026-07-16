//! Expression nodes for the Buff AST.
//!
//! Every variant of [`Expr`] carries a [`Span`] so diagnostics can point at the
//! exact source range. Expressions own all their data (no lifetimes).

use std::fmt;

use crate::common::{Block, Ident, Param};
use crate::op::{BinaryOp, UnaryOp};
use crate::ty::TypeRef;
use buff_lang_error::Span;

/// A literal value embedded directly in the source.
///
/// NOTE: derives [`PartialEq`] but **not** [`Eq`] because `f32`/`f64` don't
/// implement `Eq`. Same applies to any type that contains a [`Literal`].
///
/// # Migration notes (additive AST changes)
///
/// ## T20 — `Literal::Decimal`
///
/// `Literal::Decimal` was **added** in T20 (v0.5) to carry the raw text of a
/// 128-bit fixed-point decimal literal (e.g. `99.90m`). It stores the source
/// text verbatim as a [`String`] so the value never rounds through `f32`/`f64`
/// — exactness is preserved all the way to the `rust_decimal_macros::dec!()`
/// codegen call. [`Type`](buff_lang_types::Type)::Decimal already existed in
/// `buff-lang-types`; the literal variant closed the gap.
///
/// ## T21 — `Literal::Char`
///
/// `Literal::Char` was **added** in T21 (v0.5) to represent a single Unicode
/// scalar value literal written with single quotes: `'A'`, `'é'`, `'🚀'`. It
/// stores a Rust [`char`] (which is always exactly one scalar value, never a
/// grapheme cluster). This is **additive**: no existing variant was renamed,
/// reordered, or had its payload altered, so all prior `match` arms remain
/// exhaustive. [`Type`](buff_lang_types::Type)::Char was added in lockstep.
/// `char` is `Copy + Eq`, so the `PartialEq`-but-not-`Eq` derivation rule is
/// unaffected.
#[derive(Debug, Clone, PartialEq)]
pub enum Literal {
    /// An integer literal, e.g. `42`. Stored as `i64`.
    Int(i64),
    /// A 32-bit float literal, e.g. `3.14f`.
    Float(f32),
    /// A 64-bit float literal, e.g. `99.9d`.
    Double(f64),
    /// A boolean literal: `true` / `false`.
    Bool(bool),
    /// A string literal, e.g. `"hello"`.
    String(String),
    /// A single byte literal, e.g. `0xFF`.
    Byte(u8),
    /// A single Unicode scalar value literal, e.g. `'A'`, `'é'`, `'🚀'` (T21).
    ///
    /// Always exactly one `char`; never a grapheme cluster. The lexer rejects
    /// `''` (empty) and `'ab'` (two scalar values).
    Char(char),
    /// A 128-bit fixed-point decimal literal, e.g. `99.90m` (T20).
    ///
    /// Stores the **raw source text** (without the trailing `m`/`M` suffix)
    /// so the value is never rounded through `f32`/`f64`. Codegen emits this
    /// verbatim as `rust_decimal_macros::dec!(<text>)`, preserving exactness.
    Decimal(String),
}

impl fmt::Display for Literal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Literal::Int(v) => write!(f, "Int({v})"),
            Literal::Float(v) => write!(f, "Float({v})"),
            Literal::Double(v) => write!(f, "Double({v})"),
            Literal::Bool(v) => write!(f, "Bool({v})"),
            Literal::String(v) => write!(f, "String({v:?})"),
            Literal::Byte(v) => write!(f, "Byte(0x{v:02X})"),
            // T21: render the char in single quotes for visual distinction
            // from String (which uses double quotes) and Byte (0x..).
            Literal::Char(c) => write!(f, "Char({c:?})"),
            // Show the decimal text double-quoted (consistent with String)
            // so it's visually distinct from a bare number.
            Literal::Decimal(v) => write!(f, "Decimal({v:?})"),
        }
    }
}

/// One piece of a string-interpolation expression (T21).
///
/// An interpolation like `"Hello {name}, you are {age}!"` parses to:
///
/// ```text
/// StringInterp {
///     parts: vec![
///         InterpPart::Literal("Hello ".to_string()),
///         InterpPart::Expr(Box::new(Expr::Ident("name"))),
///         InterpPart::Literal(", you are ".to_string()),
///         InterpPart::Expr(Box::new(Expr::Ident("age"))),
///         InterpPart::Literal("!".to_string()),
///     ],
///     span: ...,
/// }
/// ```
///
/// Literal runs of zero or more `StringPart` tokens are collapsed into one
/// `InterpPart::Literal` so adjacent strings stay readable. Adjacent
/// expressions are kept separate (one `InterpPart::Expr` each).
#[derive(Debug, Clone, PartialEq)]
pub enum InterpPart {
    /// A literal text run from the source string (no escapes processed yet).
    Literal(String),
    /// An embedded expression `{expr}`.
    Expr(Box<Expr>),
}

impl fmt::Display for InterpPart {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            InterpPart::Literal(s) => write!(f, "Lit({s:?})"),
            InterpPart::Expr(e) => write!(f, "Expr({e})"),
        }
    }
}

/// A top-level expression. Every variant is annotated with its source [`Span`].
#[derive(Debug, Clone, PartialEq)]
pub enum Expr {
    /// A literal: `42`, `"hi"`, `true`, …
    Literal(Literal, Span),
    /// A bare identifier reference: `x`, `foo_bar`.
    Ident(Ident, Span),
    /// A binary operator application: `lhs op rhs`.
    BinaryOp {
        op: BinaryOp,
        lhs: Box<Expr>,
        rhs: Box<Expr>,
        span: Span,
    },
    /// A unary operator application: `op operand`.
    UnaryOp {
        op: UnaryOp,
        operand: Box<Expr>,
        span: Span,
    },
    /// An `if` expression: `if cond { then } else { else }`.
    IfExpr {
        cond: Box<Expr>,
        then_block: Block,
        else_block: Option<Block>,
        span: Span,
    },
    /// A free function call: `callee(args...)`.
    FuncCall {
        callee: Box<Expr>,
        args: Vec<Expr>,
        span: Span,
    },
    /// A method call: `receiver.method(args...)`.
    MethodCall {
        receiver: Box<Expr>,
        method: Ident,
        args: Vec<Expr>,
        span: Span,
    },
    /// A lambda / anonymous function.
    Lambda {
        params: Vec<Param>,
        body: Block,
        return_type: Option<TypeRef>,
        span: Span,
    },
    /// A struct literal: `TypeName { field: value, ... }`.
    StructInit {
        type_name: Ident,
        fields: Vec<(Ident, Expr)>,
        span: Span,
    },
    /// A `match` expression: ` scrutinee match { arms... } `.
    MatchExpr {
        scrutinee: Box<Expr>,
        arms: Vec<MatchArm>,
        span: Span,
    },
    /// A suspension point in an async context (placeholder for future async work).
    SuspendExpr { inner: Box<Expr>, span: Span },
    /// A string interpolation: `"text {expr} more {expr2}"` (T21).
    ///
    /// Built from the lexer's `StringStart / StringPart / InterpStart / ...
    /// InterpEnd / StringEnd` token stream. Each part is either literal text
    /// or an arbitrary expression. Codegen lowers this to a Rust `format!(...)`
    /// macro call with one `{}` per expression and comma-separated arguments.
    ///
    /// This is an **additive** change: no existing variant was renamed or
    /// reordered. A simple `"abc"` (no `{...}`) still parses as
    /// `Expr::Literal(Literal::String(_), _)`; only strings that actually
    /// contain interpolation produce this variant.
    StringInterp { parts: Vec<InterpPart>, span: Span },
}

impl Expr {
    /// Returns the [`Span`] associated with this expression.
    pub fn span(&self) -> Span {
        match self {
            Expr::Literal(_, s)
            | Expr::Ident(_, s)
            | Expr::BinaryOp { span: s, .. }
            | Expr::UnaryOp { span: s, .. }
            | Expr::IfExpr { span: s, .. }
            | Expr::FuncCall { span: s, .. }
            | Expr::MethodCall { span: s, .. }
            | Expr::Lambda { span: s, .. }
            | Expr::StructInit { span: s, .. }
            | Expr::MatchExpr { span: s, .. }
            | Expr::SuspendExpr { span: s, .. }
            | Expr::StringInterp { span: s, .. } => *s,
        }
    }
}

impl fmt::Display for Expr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Expr::Literal(lit, _) => write!(f, "Lit({lit})"),
            Expr::Ident(ident, _) => write!(f, "Ident({ident})"),
            Expr::BinaryOp { op, lhs, rhs, .. } => {
                write!(f, "BinaryOp({op}, {lhs}, {rhs})")
            }
            Expr::UnaryOp { op, operand, .. } => write!(f, "UnaryOp({op}, {operand})"),
            Expr::IfExpr {
                cond,
                then_block,
                else_block,
                ..
            } => match else_block {
                Some(els) => write!(f, "If({cond}, {then_block}, {els})"),
                None => write!(f, "If({cond}, {then_block})"),
            },
            Expr::FuncCall { callee, args, .. } => {
                write!(f, "Call({callee}, [")?;
                for (i, a) in args.iter().enumerate() {
                    if i > 0 {
                        f.write_str(", ")?;
                    }
                    write!(f, "{a}")?;
                }
                f.write_str("])")
            }
            Expr::MethodCall {
                receiver,
                method,
                args,
                ..
            } => {
                write!(f, "MethodCall({receiver}.{method}, [")?;
                for (i, a) in args.iter().enumerate() {
                    if i > 0 {
                        f.write_str(", ")?;
                    }
                    write!(f, "{a}")?;
                }
                f.write_str("])")
            }
            Expr::Lambda {
                params,
                body,
                return_type,
                ..
            } => {
                f.write_str("Lambda(fn(")?;
                for (i, p) in params.iter().enumerate() {
                    if i > 0 {
                        f.write_str(", ")?;
                    }
                    write!(f, "{p}")?;
                }
                f.write_str(")")?;
                if let Some(rt) = return_type {
                    write!(f, " -> {rt}")?;
                }
                write!(f, " {body})")
            }
            Expr::StructInit {
                type_name, fields, ..
            } => {
                write!(f, "StructInit({type_name} {{ ")?;
                for (i, (n, v)) in fields.iter().enumerate() {
                    if i > 0 {
                        f.write_str(", ")?;
                    }
                    write!(f, "{n}: {v}")?;
                }
                f.write_str(" })")
            }
            Expr::MatchExpr {
                scrutinee, arms, ..
            } => {
                write!(f, "Match({scrutinee}, [")?;
                for (i, arm) in arms.iter().enumerate() {
                    if i > 0 {
                        f.write_str(", ")?;
                    }
                    write!(f, "{arm}")?;
                }
                f.write_str("])")
            }
            Expr::SuspendExpr { inner, .. } => write!(f, "Suspend({inner})"),
            Expr::StringInterp { parts, .. } => {
                f.write_str("Interp[")?;
                for (i, p) in parts.iter().enumerate() {
                    if i > 0 {
                        f.write_str(", ")?;
                    }
                    write!(f, "{p}")?;
                }
                f.write_str("]")
            }
        }
    }
}

/// A single arm of a `match` expression.
#[derive(Debug, Clone, PartialEq)]
pub struct MatchArm {
    pub pattern: Pattern,
    pub body: Block,
    pub span: Span,
}

impl fmt::Display for MatchArm {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} => {}", self.pattern, self.body)
    }
}

/// A pattern usable inside a [`MatchArm`].
#[derive(Debug, Clone, PartialEq)]
pub enum Pattern {
    /// The wildcard `_`.
    Wildcard(Span),
    /// A literal pattern: `1`, `"foo"`, `true`.
    Literal(Literal, Span),
    /// A binding pattern: `x`.
    Ident(Ident, Span),
    /// An enum variant pattern: `Option::Some(x)` or `Color::Rgb(r, g, b)`.
    Variant {
        enum_name: Ident,
        variant: Ident,
        subpatterns: Vec<Pattern>,
        span: Span,
    },
}

impl fmt::Display for Pattern {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Pattern::Wildcard(_) => f.write_str("_"),
            Pattern::Literal(lit, _) => write!(f, "{lit}"),
            Pattern::Ident(name, _) => write!(f, "{name}"),
            Pattern::Variant {
                enum_name,
                variant,
                subpatterns,
                ..
            } => {
                write!(f, "{enum_name}::{variant}")?;
                if !subpatterns.is_empty() {
                    f.write_str("(")?;
                    for (i, p) in subpatterns.iter().enumerate() {
                        if i > 0 {
                            f.write_str(", ")?;
                        }
                        write!(f, "{p}")?;
                    }
                    f.write_str(")")?;
                }
                Ok(())
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dummy_span() -> Span {
        Span::dummy()
    }

    #[test]
    fn literal_display() {
        assert_eq!(Literal::Int(42).to_string(), "Int(42)");
        assert_eq!(Literal::Bool(true).to_string(), "Bool(true)");
        assert_eq!(Literal::Byte(255).to_string(), "Byte(0xFF)");
        assert_eq!(
            Literal::String("hi".to_string()).to_string(),
            "String(\"hi\")"
        );
        // T20: Decimal displays its raw source text, double-quoted.
        assert_eq!(
            Literal::Decimal("99.90".to_string()).to_string(),
            "Decimal(\"99.90\")"
        );
        // T21: Char renders in single quotes via Debug ({:?}) form.
        assert_eq!(Literal::Char('A').to_string(), "Char('A')");
        assert_eq!(Literal::Char('é').to_string(), "Char('é')");
        assert_eq!(Literal::Char('🚀').to_string(), "Char('🚀')");
    }

    #[test]
    fn string_interp_display() {
        // T21: StringInterp renders as `Interp[Lit("..."), Expr(...), ...]`.
        let parts = vec![
            InterpPart::Literal("Hello ".into()),
            InterpPart::Expr(Box::new(Expr::Ident(
                Ident::new("name", dummy_span()),
                dummy_span(),
            ))),
            InterpPart::Literal("!".into()),
        ];
        let e = Expr::StringInterp {
            parts,
            span: dummy_span(),
        };
        assert_eq!(
            e.to_string(),
            "Interp[Lit(\"Hello \"), Expr(Ident(name)), Lit(\"!\")]"
        );
    }

    #[test]
    fn ident_expr_display() {
        let e = Expr::Ident(Ident::new("x", dummy_span()), dummy_span());
        assert_eq!(e.to_string(), "Ident(x)");
    }

    #[test]
    fn binary_op_expr_display() {
        let e = Expr::BinaryOp {
            op: BinaryOp::Add,
            lhs: Box::new(Expr::Literal(Literal::Int(1), dummy_span())),
            rhs: Box::new(Expr::Literal(Literal::Int(2), dummy_span())),
            span: dummy_span(),
        };
        assert_eq!(e.to_string(), "BinaryOp(+, Lit(Int(1)), Lit(Int(2)))");
    }

    #[test]
    fn variant_pattern_display() {
        let p = Pattern::Variant {
            enum_name: Ident::new("Option", dummy_span()),
            variant: Ident::new("Some", dummy_span()),
            subpatterns: vec![Pattern::Ident(Ident::new("x", dummy_span()), dummy_span())],
            span: dummy_span(),
        };
        assert_eq!(p.to_string(), "Option::Some(x)");
    }
}

//! Statement nodes for the Buff AST.
//!
//! Statements produce no value (or `unit`). They live inside blocks, function
//! bodies, and module tops.

use std::fmt;

use crate::common::{Block, Ident};
use crate::expr::Expr;
use crate::op::BinaryOp;
use crate::ty::TypeRef;
use buff_lang_error::Span;

/// A single statement.
#[derive(Debug, Clone, PartialEq)]
pub enum Stmt {
    /// A `let` binding: `let name[: ty] = value;` (optionally `mut`).
    LetDecl {
        name: Ident,
        value: Expr,
        mutable: bool,
        ty: Option<TypeRef>,
        span: Span,
    },
    /// An assignment: `target op value;` (covers `=`, `+=`, …).
    Assignment {
        target: Expr,
        op: BinaryOp,
        value: Expr,
        span: Span,
    },
    /// A bare expression used as a statement.
    ExprStmt(Expr, Span),
    /// A `return` statement, optionally with a value.
    Return(Option<Expr>, Span),
    /// A `break` statement.
    Break(Span),
    /// A `continue` statement.
    Continue(Span),
    /// An iterator loop: `for var in iter { body }`.
    ForIn {
        var: Ident,
        iter: Expr,
        body: Block,
        span: Span,
    },
    /// A conditional loop (while-style): `for cond { body }`.
    ForWhile { cond: Expr, body: Block, span: Span },
}

impl fmt::Display for Stmt {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Stmt::LetDecl {
                name,
                value,
                mutable,
                ty,
                ..
            } => {
                f.write_str("LetDecl(")?;
                if *mutable {
                    f.write_str("mut ")?;
                }
                write!(f, "{name}")?;
                if let Some(t) = ty {
                    write!(f, ": {t}")?;
                }
                write!(f, " = {value})")
            }
            Stmt::Assignment {
                target, op, value, ..
            } => write!(f, "Assign({target} {op} {value})"),
            Stmt::ExprStmt(e, _) => write!(f, "ExprStmt({e})"),
            Stmt::Return(Some(e), _) => write!(f, "Return({e})"),
            Stmt::Return(None, _) => f.write_str("Return"),
            Stmt::Break(_) => f.write_str("Break"),
            Stmt::Continue(_) => f.write_str("Continue"),
            Stmt::ForIn {
                var, iter, body, ..
            } => write!(f, "ForIn({var} in {iter} {body})"),
            Stmt::ForWhile { cond, body, .. } => write!(f, "ForWhile({cond} {body})"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::expr::Literal;

    #[test]
    fn let_decl_display() {
        let s = Stmt::LetDecl {
            name: Ident::new("x", Span::dummy()),
            value: Expr::Literal(Literal::Int(42), Span::dummy()),
            mutable: false,
            ty: None,
            span: Span::dummy(),
        };
        assert_eq!(s.to_string(), "LetDecl(x = Lit(Int(42)))");
    }

    #[test]
    fn let_decl_mut_with_type_display() {
        let s = Stmt::LetDecl {
            name: Ident::new("y", Span::dummy()),
            value: Expr::Literal(Literal::Int(0), Span::dummy()),
            mutable: true,
            ty: Some(TypeRef::Named {
                name: Ident::new("Int", Span::dummy()),
                span: Span::dummy(),
            }),
            span: Span::dummy(),
        };
        assert_eq!(s.to_string(), "LetDecl(mut y: Int = Lit(Int(0)))");
    }

    #[test]
    fn break_continue_display() {
        assert_eq!(Stmt::Break(Span::dummy()).to_string(), "Break");
        assert_eq!(Stmt::Continue(Span::dummy()).to_string(), "Continue");
    }
}

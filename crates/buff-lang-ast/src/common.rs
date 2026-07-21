//! Common AST building blocks shared across expressions, statements, and declarations.
//!
//! - [`Ident`]: an identifier with span info.
//! - [`Block`]: a `{ ... }` sequence of statements.
//! - [`Param`]: a function parameter (name + type).

use std::fmt;

use crate::expr::Expr;
use crate::stmt::Stmt;
use crate::ty::TypeRef;
use buff_lang_error::Span;

/// An identifier — a name plus its source location.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Ident {
    pub name: String,
    pub span: Span,
}

impl Ident {
    /// Create a new identifier from anything string-like.
    pub fn new(name: impl Into<String>, span: Span) -> Self {
        Self {
            name: name.into(),
            span,
        }
    }
}

impl fmt::Display for Ident {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.name)
    }
}

/// A brace-delimited block of statements: `{ stmt; stmt; ... }`.
#[derive(Debug, Clone, PartialEq)]
pub struct Block {
    pub stmts: Vec<Stmt>,
    pub span: Span,
}

impl Block {
    /// Create an empty block with the given span.
    pub fn empty(span: Span) -> Self {
        Self {
            stmts: Vec::new(),
            span,
        }
    }
}

impl fmt::Display for Block {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("{ ")?;
        for (i, s) in self.stmts.iter().enumerate() {
            if i > 0 {
                f.write_str("; ")?;
            }
            write!(f, "{s}")?;
        }
        if !self.stmts.is_empty() {
            f.write_str(" ")?;
        }
        f.write_str("}")
    }
}

/// A function parameter: `name: Type`, optionally with a default value
/// `name: Type = expr` (T106).
///
/// The `default_value` field carries the parsed default expression when the
/// parameter was declared with `= expr`. Rust has NO native default-param
/// support, so the codegen fills omitted trailing args at the CALL SITE
/// (`fetch("x")` → `fetch("x", 30)` when `timeout` defaults to `30`) — it
/// does NOT rely on Rust defaults. [`Option::None`] means the parameter is
/// required (no default).
///
/// This field is **additive** (T106): existing params without a default
/// carry [`Option::None`], so all prior `Param { ... }` construction sites
/// just need `default_value: None` appended.
#[derive(Debug, Clone, PartialEq)]
pub struct Param {
    pub name: Ident,
    pub ty: TypeRef,
    pub default_value: Option<Expr>,
    pub is_comptime: bool,
    pub span: Span,
}

impl Param {
    pub fn plain(name: impl Into<String>, ty: TypeRef, span: Span) -> Self {
        Self {
            name: Ident::new(name, span),
            ty,
            default_value: None,
            is_comptime: false,
            span,
        }
    }
}

impl fmt::Display for Param {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.is_comptime {
            f.write_str("comptime ")?;
        }
        write!(f, "{}: {}", self.name, self.ty)?;
        if let Some(dv) = &self.default_value {
            write!(f, " = {dv}")?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ident_display() {
        let i = Ident::new("foo", Span::dummy());
        assert_eq!(i.to_string(), "foo");
    }

    #[test]
    fn empty_block_display() {
        let b = Block::empty(Span::dummy());
        assert_eq!(b.to_string(), "{ }");
    }
}

//! Type reference nodes for the Deox AST.
//!
//! A [`TypeRef`] is a *reference* to a type (used in annotations, signatures,
//! and parameters). It is not the type itself — type resolution happens in the
//! `deox-types` crate.

use std::fmt;

use crate::common::Ident;
use deox_error::Span;

/// A reference to a type.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TypeRef {
    /// A named type, e.g. `Int`, `String`, `MyType`.
    Named { name: Ident, span: Span },
    /// A generic application, e.g. `Vector<Int>`, `Map<String, Int>`.
    Generic {
        base: Box<TypeRef>,
        args: Vec<TypeRef>,
        span: Span,
    },
    /// The builtin `Option<T>` wrapper.
    Option(Box<TypeRef>, Span),
    /// A function type: `(T1, T2) -> T3`, optionally `async`.
    Function {
        params: Vec<TypeRef>,
        return_type: Box<TypeRef>,
        is_async: bool,
        span: Span,
    },
}

impl fmt::Display for TypeRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TypeRef::Named { name, .. } => write!(f, "{name}"),
            TypeRef::Generic { base, args, .. } => {
                write!(f, "{base}<")?;
                for (i, a) in args.iter().enumerate() {
                    if i > 0 {
                        f.write_str(", ")?;
                    }
                    write!(f, "{a}")?;
                }
                f.write_str(">")
            }
            TypeRef::Option(inner, _) => write!(f, "Option<{inner}>"),
            TypeRef::Function {
                params,
                return_type,
                is_async,
                ..
            } => {
                if *is_async {
                    f.write_str("async ")?;
                }
                f.write_str("(")?;
                for (i, p) in params.iter().enumerate() {
                    if i > 0 {
                        f.write_str(", ")?;
                    }
                    write!(f, "{p}")?;
                }
                write!(f, ") -> {return_type}")
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
    fn named_type_display() {
        let t = TypeRef::Named {
            name: Ident::new("Int", dummy_span()),
            span: dummy_span(),
        };
        assert_eq!(t.to_string(), "Int");
    }

    #[test]
    fn generic_type_display() {
        let t = TypeRef::Generic {
            base: Box::new(TypeRef::Named {
                name: Ident::new("Vector", dummy_span()),
                span: dummy_span(),
            }),
            args: vec![TypeRef::Named {
                name: Ident::new("Int", dummy_span()),
                span: dummy_span(),
            }],
            span: dummy_span(),
        };
        assert_eq!(t.to_string(), "Vector<Int>");
    }

    #[test]
    fn function_type_display() {
        let t = TypeRef::Function {
            params: vec![TypeRef::Named {
                name: Ident::new("Int", dummy_span()),
                span: dummy_span(),
            }],
            return_type: Box::new(TypeRef::Named {
                name: Ident::new("Bool", dummy_span()),
                span: dummy_span(),
            }),
            is_async: false,
            span: dummy_span(),
        };
        assert_eq!(t.to_string(), "(Int) -> Bool");
    }
}

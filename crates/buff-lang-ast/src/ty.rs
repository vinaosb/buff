//! Type reference nodes for the Buff AST.
//!
//! A [`TypeRef`] is a *reference* to a type (used in annotations, signatures,
//! and parameters). It is not the type itself — type resolution happens in the
//! `buff-lang-types` crate.

use std::fmt;

use crate::common::Ident;
use buff_lang_error::Span;

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
    /// A union (sum) type: `A | B | C` (T76).
    ///
    /// Rust has no anonymous unions, so codegen emits a named enum wrapper
    /// (e.g. `String | Int` → `enum StringOrInt { String(String), Int(i64) }`).
    /// The wrapper name is deterministic: join the member type names with
    /// `Or` in source order (`String | Int` → `StringOrInt`,
    /// `Int | Float | Bool` → `IntOrFloatOrBool`). Codegen collects unique
    /// unions and emits each wrapper enum ONCE as a top-level item.
    ///
    /// Each member is itself a [`TypeRef`] (so unions compose with named,
    /// generic, option, and even nested-union members — though nested-union
    /// codegen flattening is deferred). The span covers the whole
    /// `A | B | C` sequence.
    Union(Vec<TypeRef>, Span),
    /// A tuple type: `(T, U, ...)`, e.g. `(String, Int)` (T103).
    ///
    /// Each member is itself a [`TypeRef`] (so nested tuples like
    /// `(String, (Int, Bool))` compose). The 2+-element rule lives at
    /// parse time: a single `(T)` is grouping (returns the bare `T`),
    /// NOT a `TypeRef::Tuple`. So this variant always carries 2+ members
    /// — there is no single-element tuple in Buff (a trailing comma
    /// `(T,)` is the established Rust idiom but is deferred in Buff;
    /// the parser treats `(T,)` as `(T)` grouping for v0.5). The span
    /// covers the whole `( ... )` sequence.
    ///
    /// This is **additive** (T103): no existing variant was renamed,
    /// reordered, or had its payload altered. See the migration-note
    /// pattern on [`TypeRef`] and in `.sisyphus/notepads/` (T76 union
    /// types for the TypeRef ripple template).
    Tuple(Vec<TypeRef>, Span),
    /// A trait object type: `Box<dyn Trait>` or `&dyn Trait` (DR-020 / P2.1a).
    ///
    /// `dyn` is recognized contextually in type position (NOT a reserved
    /// keyword — it lexes as `Ident`). The `trait_name` identifies the
    /// trait (`Drawable`, `LexCallback`, etc.). The `lifetime` is `None`
    /// for the owned `Box<dyn Trait>` form; the borrowed `&dyn Trait`
    /// form carries an explicit lifetime (MVP: `'static` only — see
    /// DR-020 §Autoboxing Rules).
    ///
    /// This is **additive** (P2.1a): no existing variant was renamed,
    /// reordered, or had its payload altered. The downstream lowering
    /// (`typeref_to_type`) maps this to the pre-existing
    /// `Type::DynamicDispatch(Box<Type>)` (T68, v1.19); codegen already
    /// handles `Type::DynamicDispatch` at
    /// `buff-lang-codegen-rust/src/rust_codegen/type_lowering.rs:240`.
    TraitObject {
        trait_name: Ident,
        lifetime: Option<String>,
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
            // T76: union `A | B | C`.
            TypeRef::Union(members, _) => {
                for (i, m) in members.iter().enumerate() {
                    if i > 0 {
                        f.write_str(" | ")?;
                    }
                    write!(f, "{m}")?;
                }
                Ok(())
            }
            // T103: tuple types `(T, U, ...)`. Each member is itself a
            // [`TypeRef`]. Renders with leading/trailing parens and
            // comma-separated members, mirroring the source form.
            TypeRef::Tuple(members, _) => {
                f.write_str("(")?;
                for (i, m) in members.iter().enumerate() {
                    if i > 0 {
                        f.write_str(", ")?;
                    }
                    write!(f, "{m}")?;
                }
                f.write_str(")")
            }
            // DR-020 / P2.1a: trait object `Box<dyn Trait>` (owned, the
            // canonical Buff form) or `&dyn Trait` (borrowed, MVP
            // parameters-only). Renders the source-form shape so the
            // Display round-trips through `buff check --dump-ast`.
            TypeRef::TraitObject {
                trait_name,
                lifetime: Some(lt),
                ..
            } => write!(f, "&dyn {trait_name} {lt}"),
            TypeRef::TraitObject {
                trait_name,
                lifetime: None,
                ..
            } => write!(f, "Box<dyn {trait_name}>"),
        }
    }
}

// ---------------------------------------------------------------------------
// JSON serialization (P0.1.2b)
// ---------------------------------------------------------------------------

impl TypeRef {
    /// Deterministic JSON serialization for `buff check --dump-ast` (P0.1.2b).
    pub fn to_json(&self) -> serde_json::Value {
        use crate::common::span_to_json;
        use serde_json::json;
        match self {
            TypeRef::Named { name, span } => json!({
                "type": "Named",
                "name": name.to_json(),
                "span": span_to_json(*span),
            }),
            TypeRef::Generic { base, args, span } => {
                let args_json: Vec<serde_json::Value> =
                    args.iter().map(TypeRef::to_json).collect();
                json!({
                    "type": "Generic",
                    "base": base.to_json(),
                    "args": args_json,
                    "span": span_to_json(*span),
                })
            }
            TypeRef::Option(inner, span) => json!({
                "type": "Option",
                "inner": inner.to_json(),
                "span": span_to_json(*span),
            }),
            TypeRef::Function {
                params,
                return_type,
                is_async,
                span,
            } => {
                let params_json: Vec<serde_json::Value> =
                    params.iter().map(TypeRef::to_json).collect();
                json!({
                    "type": "Function",
                    "params": params_json,
                    "return_type": return_type.to_json(),
                    "is_async": is_async,
                    "span": span_to_json(*span),
                })
            }
            TypeRef::Union(members, span) => {
                let members_json: Vec<serde_json::Value> =
                    members.iter().map(TypeRef::to_json).collect();
                json!({
                    "type": "Union",
                    "members": members_json,
                    "span": span_to_json(*span),
                })
            }
            TypeRef::Tuple(members, span) => {
                let members_json: Vec<serde_json::Value> =
                    members.iter().map(TypeRef::to_json).collect();
                json!({
                    "type": "Tuple",
                    "members": members_json,
                    "span": span_to_json(*span),
                })
            }
            // DR-020 / P2.1a: trait object. `lifetime` is `null` for the
            // owned `Box<dyn Trait>` form; a string for the borrowed
            // `&dyn Trait ('static)` form. Deterministic ordering per
            // the Equivalence Contract (BTreeMap-backed serde_json by
            // default).
            TypeRef::TraitObject {
                trait_name,
                lifetime,
                span,
            } => {
                json!({
                    "type": "TraitObject",
                    "trait_name": trait_name.to_json(),
                    "lifetime": lifetime,
                    "span": span_to_json(*span),
                })
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

    // DR-020 / P2.1a: trait object Display round-trips.
    #[test]
    fn trait_object_owned_display() {
        let t = TypeRef::TraitObject {
            trait_name: Ident::new("Drawable", dummy_span()),
            lifetime: None,
            span: dummy_span(),
        };
        assert_eq!(t.to_string(), "Box<dyn Drawable>");
    }

    #[test]
    fn trait_object_borrowed_display() {
        let t = TypeRef::TraitObject {
            trait_name: Ident::new("Drawable", dummy_span()),
            lifetime: Some("'static".to_string()),
            span: dummy_span(),
        };
        assert_eq!(t.to_string(), "&dyn Drawable 'static");
    }

    #[test]
    fn trait_object_to_json_round_trips() {
        let t = TypeRef::TraitObject {
            trait_name: Ident::new("Drawable", dummy_span()),
            lifetime: None,
            span: dummy_span(),
        };
        let json = t.to_json();
        assert_eq!(json["type"], "TraitObject");
        assert_eq!(json["trait_name"]["name"], "Drawable");
        assert!(json["lifetime"].is_null());
    }
}

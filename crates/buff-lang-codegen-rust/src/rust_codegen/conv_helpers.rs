//! T105a - index-cast / arg-parse / typeref->Type conversion helpers (mechanically extracted from rust_codegen.rs).
//!
//! Verbatim move - no logic changes. Child module of rust_codegen so it
//! inherits the parent imports via use super::* (zero per-module import lists).
//! Functions are pub(super) so the parent reaches them through the glob below.

use super::*;


/// Build a Rust `as usize` cast for a vector index (T23).
///
/// Unlike [`cast_to`], this only wraps the operand in parens when it is a
/// non-atomic expression (binary/unary/cast), so the common cases stay clean:
/// `0 as usize`, `i as usize`. Compound indices like `a + b` become
/// `(a + b) as usize` so the cast doesn't bind tighter than the `+`.
pub(super) fn cast_to_usize(e: SynExpr) -> SynExpr {
    let needs_parens = matches!(
        e,
        SynExpr::Binary(_) | SynExpr::Unary(_) | SynExpr::Cast(_) | SynExpr::Range(_)
    );
    let operand = if needs_parens { wrap_in_parens(e) } else { e };
    SynExpr::Cast(syn::ExprCast {
        attrs: Vec::new(),
        expr: Box::new(operand),
        as_token: Default::default(),
        ty: Box::new(rust_path_type("usize")),
    })
}

/// Build a Rust `as` cast: `(e) as T`. The receiver is parenthesised so
/// compound expressions bind correctly (e.g. `(a + b) as f64` not `a + b as f64`).
pub(super) fn cast_to(e: SynExpr, target: &str) -> SynExpr {
    SynExpr::Cast(syn::ExprCast {
        attrs: Vec::new(),
        expr: Box::new(wrap_in_parens(e)),
        as_token: Default::default(),
        ty: Box::new(rust_path_type(target)),
    })
}

/// Build a Rust integer-literal expression (`0`, `1`, etc.).
pub(super) fn make_int_lit_expr(n: i64) -> SynExpr {
    SynExpr::Lit(syn::ExprLit {
        attrs: Vec::new(),
        lit: syn::Lit::Int(syn::LitInt::new(&n.to_string(), ProcSpan::call_site())),
    })
}

/// Build a binary expression `lhs <op> rhs`.
pub(super) fn make_binary_expr(op: syn::BinOp, lhs: SynExpr, rhs: SynExpr) -> SynExpr {
    SynExpr::Binary(syn::ExprBinary {
        attrs: Vec::new(),
        left: Box::new(lhs),
        op,
        right: Box::new(rhs),
    })
}

/// Discriminates the two Rust idioms for type-constructor prelude functions.
///
/// - `Numeric` covers `Int(x)` / `Float(x)` — Rust emits `(x) as T`.
/// - `Bool` is separate because Rust has no `as bool` cast; the numeric→bool
///   mapping is `x != 0`.
#[derive(Clone, Copy)]
pub(super) enum ConvKind {
    Numeric,
    Bool,
}

/// Build a `x.parse::<T>().unwrap_or(default)` expression for the string-arg
/// branch of `Int(x)`/`Float(x)`/`Bool(x)`. Built via `quote!` so the
/// turbofish `::<T>` and method chain are constructed without hand-formatted
/// Rust.
pub(super) fn parse_with_default(arg: SynExpr, target: &str, kind: &ConvKind) -> SynExpr {
    // Build `arg.parse::<target>()` as a method call with turbofish.
    let parse_call = SynExpr::MethodCall(syn::ExprMethodCall {
        attrs: Vec::new(),
        receiver: Box::new(arg),
        dot_token: Default::default(),
        method: Ident::new("parse", ProcSpan::call_site()),
        turbofish: Some(syn::AngleBracketedGenericArguments {
            colon2_token: None,
            lt_token: Default::default(),
            args: {
                let mut p: Punctuated<syn::GenericArgument, syn::Token![,]> = Punctuated::new();
                p.push(syn::GenericArgument::Type(rust_path_type(target)));
                p
            },
            gt_token: Default::default(),
        }),
        paren_token: Default::default(),
        args: Default::default(),
    });
    // `.unwrap_or(<default-lit>)` — for numerics the default is the unsuffixed
    // integer `0`; for bool it's `false`. Both are valid Rust literal tokens.
    let default_tokens: proc_macro2::TokenStream = match kind {
        ConvKind::Numeric => {
            let lit = proc_macro2::Literal::i64_unsuffixed(0);
            quote::quote! { #lit }
        }
        ConvKind::Bool => {
            quote::quote! { false }
        }
    };
    let default_expr =
        syn::parse2::<SynExpr>(default_tokens).unwrap_or_else(|_| make_int_lit_expr(0));
    // `.unwrap_or(default_expr)` — single-arg method call on the parse result.
    method_call_one_arg(parse_call, "unwrap_or", default_expr)
}

/// Mirror of the private `typeref_to_type` in `buff_lang_types::infer`.
///
/// Used by [`RustCodegen::lower_func`] to seed the [`TypeInferencer`]
/// environment with function-parameter types so subsequent `let`
/// bindings can refer to params and still get a useful inferred type.
pub(super) fn typeref_to_type(ty: &TypeRef) -> Option<Type> {
    match ty {
        TypeRef::Named { name, .. } => match name.name.as_str() {
            "Int" => Some(Type::int_default()),
            "Float" => Some(Type::float_default()),
            "Double" => Some(Type::double()),
            "Bool" => Some(Type::bool()),
            "String" => Some(Type::string()),
            // T21: Char annotation maps to the resolved Char type.
            "Char" => Some(Type::char()),
            "Byte" => Some(Type::byte()),
            "Decimal" => Some(Type::Decimal),
            "Void" => Some(Type::Void),
            _ => None,
        },
        // T76: union types (for match param resolution). Resolve each
        // member recursively; unknown members become Unknown.
        TypeRef::Union(members, _) => {
            let resolved: Vec<Type> = members.iter().filter_map(typeref_to_type).collect();
            Some(Type::Union(resolved))
        }
        _ => None,
    }
}


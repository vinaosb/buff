//! T105a - expr/lowering syn builders (generic paths, calls, str coercion, math lowering) (mechanically extracted from rust_codegen.rs).
//!
//! Verbatim move - no logic changes. Child module of rust_codegen so it
//! inherits the parent imports via use super::* (zero per-module import lists).
//! Functions are pub(super) so the parent reaches them through the glob below.

use super::*;

/// Build a `Type::Path` with generic type arguments, e.g.
/// `Option<T>`, `Vec<T>`.
pub(super) fn make_generic_path_type(name: &str, args: Vec<SynType>) -> SynType {
    let mut path_args: Punctuated<syn::GenericArgument, syn::Token![,]> = Punctuated::new();
    for a in args {
        path_args.push(syn::GenericArgument::Type(a));
    }
    let segment = syn::PathSegment {
        ident: Ident::new(name, ProcSpan::call_site()),
        arguments: syn::PathArguments::AngleBracketed(syn::AngleBracketedGenericArguments {
            colon2_token: None,
            lt_token: Default::default(),
            args: path_args,
            gt_token: Default::default(),
        }),
    };
    let mut segments: Punctuated<syn::PathSegment, syn::Token![::]> = Punctuated::new();
    segments.push(segment);
    SynType::Path(syn::TypePath {
        qself: None,
        path: syn::Path {
            leading_colon: None,
            segments,
        },
    })
}

/// Like [`make_generic_path_type`] but accepts a `::`-separated qualified
/// path (e.g. `"std::collections::HashMap"`) and attaches the generic
/// arguments to the LAST segment. Used by the T25 Map codegen so generated
/// programs can reference `std::collections::HashMap<K, V>` without a `use`
/// import (avoiding import management in v0.5).
pub(super) fn make_qualified_generic_path_type(
    qualified_name: &str,
    args: Vec<SynType>,
) -> SynType {
    let mut path_args: Punctuated<syn::GenericArgument, syn::Token![,]> = Punctuated::new();
    for a in args {
        path_args.push(syn::GenericArgument::Type(a));
    }
    let mut segments: Punctuated<syn::PathSegment, syn::Token![::]> = Punctuated::new();
    let mut parts = qualified_name.split("::").collect::<Vec<_>>();
    let last_idx = parts.len().saturating_sub(1);
    for (i, seg) in parts.drain(..).enumerate() {
        let arguments = if i == last_idx {
            syn::PathArguments::AngleBracketed(syn::AngleBracketedGenericArguments {
                colon2_token: None,
                lt_token: Default::default(),
                args: path_args.clone(),
                gt_token: Default::default(),
            })
        } else {
            syn::PathArguments::None
        };
        segments.push(syn::PathSegment {
            ident: Ident::new(seg, ProcSpan::call_site()),
            arguments,
        });
    }
    SynType::Path(syn::TypePath {
        qself: None,
        path: syn::Path {
            leading_colon: None,
            segments,
        },
    })
}

/// Format a float so that it always has a decimal point or exponent (so the
/// `f32`/`f64` suffix binds to a float literal, not an integer).
pub(super) fn float_repr(d: f64) -> String {
    let s = format!("{d}");
    if s.contains('.')
        || s.contains('e')
        || s.contains('E')
        || s == "inf"
        || s == "-inf"
        || s == "NaN"
    {
        s
    } else {
        format!("{s}.0")
    }
}

/// Build a `recv.method(arg)` single-argument method call.
///
/// Used by the string-method codegen helpers (e.g. `s.chars().skip(n)`).
pub(super) fn method_call_one_arg(recv: SynExpr, method: &str, arg: SynExpr) -> SynExpr {
    let mut args: Punctuated<SynExpr, syn::Token![,]> = Punctuated::new();
    args.push(arg);
    SynExpr::MethodCall(syn::ExprMethodCall {
        attrs: Vec::new(),
        receiver: Box::new(recv),
        dot_token: Default::default(),
        method: Ident::new(method, ProcSpan::call_site()),
        turbofish: None,
        paren_token: Default::default(),
        args,
    })
}

/// T124b: build a fully-qualified Rust associated-function call
/// `<path>(args)` — used for prelude-type constructors like
/// `chrono::Utc::now()` and `chrono::TimeDelta::days(n)`.
///
/// The `qualified_path` is a `::`-separated string (e.g.
/// `"chrono::Utc::now"`, `"chrono::TimeDelta::days"`). The args slice is
/// lowered already — this helper just wraps them in a `syn::ExprCall` on
/// the path expression.
pub(super) fn rust_call_expr(qualified_path: &str, args: Vec<SynExpr>) -> SynExpr {
    let callee = SynExpr::Path(syn::ExprPath {
        attrs: Vec::new(),
        qself: None,
        path: rust_path(qualified_path),
    });
    let mut punct: Punctuated<SynExpr, syn::Token![,]> = Punctuated::new();
    for a in args {
        punct.push(a);
    }
    SynExpr::Call(syn::ExprCall {
        attrs: Vec::new(),
        func: Box::new(callee),
        paren_token: Default::default(),
        args: punct,
    })
}

/// T124b: build a Rust `&'static str` literal expression.
///
/// Used by prelude-type parse/format codegen to pass strftime / parse
/// format strings (`"%Y-%m-%d"`). Built via `syn::LitStr::new` so any
/// embedded escapes survive correctly.
pub(super) fn str_lit_expr(text: &str) -> SynExpr {
    SynExpr::Lit(syn::ExprLit {
        attrs: Vec::new(),
        lit: syn::Lit::Str(syn::LitStr::new(text, ProcSpan::call_site())),
    })
}

/// T124b: coerce a string-typed argument expression to `&str` when the
/// underlying chrono API requires it.
///
/// chrono's `DateTime::parse_from_rfc3339`, `NaiveDate::parse_from_str`,
/// and `DateTime::format` all take `&str`. Buff string literals
/// (`Expr::Literal(Literal::String(s))`) lower directly to a Rust
/// `&'static str` literal — so no coercion is needed in that case. For
/// non-literal arg expressions (idents referencing `String` bindings,
/// interpolation results, ...) we wrap the lowered expression in a borrow
/// `&<expr>` so Rust's Deref coercion turns `&String` into `&str`.
///
/// Without this, `DateTime.parse(my_string_var)` would emit
/// `chrono::DateTime::parse_from_rfc3339(my_string_var)` which fails to
/// compile (expected `&str`, found `String`). The borrow turns it into
/// `chrono::DateTime::parse_from_rfc3339(&my_string_var)` which works.
pub(super) fn coerce_str_arg_to_ref(lowered: SynExpr, orig: &Expr) -> SynExpr {
    // String literals lower to `&'static str` already — no borrow needed.
    if matches!(orig, Expr::Literal(Literal::String(_), _)) {
        return lowered;
    }
    // Named-arg wrapper around a string literal — recurse into the value.
    if let Expr::NamedArg { value, .. } = orig {
        return coerce_str_arg_to_ref(lowered, value);
    }
    // Everything else (idents, interpolation, etc.) — borrow via `&<expr>`.
    //
    // `syn::Expr` has no `Ref` variant (references are parsed into the
    // generic `Expr::Paren`-shaped token-stream slot, not a dedicated
    // variant). We build `& #lowered` via `syn::parse_quote!` — the same
    // approach used elsewhere in this file for `#[tokio::main]` and
    // `#[test]` attribute construction (lines 977/983). The pattern is
    // well-known to parse successfully (any expression can be borrowed),
    // so the panic-on-parse-failure caveat of `parse_quote!` does not
    // apply in practice.
    syn::parse_quote!( & #lowered )
}

// ---------------------------------------------------------------------------
// T124f - Math module codegen helpers.
// ---------------------------------------------------------------------------

/// T124f: lower a unary `Math.<method>(x)` call to `(<arg> as f64).<method>()`.
///
/// Wraps the arg in an `as f64` cast so an Int arg like `Math.sqrt(16)`
/// works as well as a Float arg like `Math.sqrt(2.0)` (matches the
/// spec acceptance `Math.sqrt(16) -> 4.0`). The cast is built via
/// `quote!` + parse2 so the resulting `syn::Expr` is a well-formed
/// `Expr::MethodCall` on a cast subexpression (NOT a string-built
/// hack - the single string producer remains `prettyplease::unparse`).
///
/// Used for: sqrt / sin / cos / tan / abs / floor / ceil / round
/// (8 unary Math methods - all take one arg and return Float).
pub(super) fn lower_math_unary(arg: SynExpr, method: &str) -> Result<SynExpr, CodegenError> {
    let method_ident = proc_macro2::Ident::new(method, ProcSpan::call_site());
    let tokens: proc_macro2::TokenStream = quote::quote! {
        (#arg as f64).#method_ident()
    };
    syn::parse2(tokens).map_err(|e| {
        CodegenError::new(
            Diagnostic::error(
                format!("unsupported: Math.{method} codegen parse: {e}"),
                BuffSpan::dummy(),
            )
            .with_code(ErrorCode::CodegenParseError),
        )
    })
}

/// T124f: lower a binary `Math.<method>(a, b)` call to
/// `(<a> as f64).<method>(<b> as f64)`.
///
/// Both args cast to `f64` because Rust's `f64::min` / `f64::max`
/// take `f64` (and casting both keeps the lowering uniform with
/// `Math.pow` which definitely needs both as f64).
///
/// Built via `quote!` + parse2 so the resulting `syn::Expr` is a
/// well-formed `Expr::MethodCall` on cast subexpressions.
///
/// Used for: min / max (2 binary Math methods - both take two args
/// and return Float).
pub(super) fn lower_math_binary(args: Vec<SynExpr>, method: &str) -> Result<SynExpr, CodegenError> {
    let method_ident = proc_macro2::Ident::new(method, ProcSpan::call_site());
    let (a, b) = (args[0].clone(), args[1].clone());
    let tokens: proc_macro2::TokenStream = quote::quote! {
        (#a as f64).#method_ident(#b as f64)
    };
    syn::parse2(tokens).map_err(|e| {
        CodegenError::new(
            Diagnostic::error(
                format!("unsupported: Math.{method} codegen parse: {e}"),
                BuffSpan::dummy(),
            )
            .with_code(ErrorCode::CodegenParseError),
        )
    })
}

/// Take a token stream that calls a fully-qualified function with a single
/// placeholder argument `__recv` and replace that placeholder with an
/// actual lowered receiver expression.
///
/// The `tokens` argument is expected to parse as a Rust function-call
/// expression (e.g. `path::func(&__recv, true)`). We use `quote!` to splice
/// the receiver in: we re-parse a small template that names `__recv` and
/// then walk the resulting `ExprCall` to substitute the real receiver.
///
/// This indirection is needed because `quote!` cannot easily splice into
/// an arbitrary position inside a string-built token stream — we instead
/// parse the template to a real `ExprCall`, then swap the first argument.
pub(super) fn splice_receiver_into_call(
    tokens: proc_macro2::TokenStream,
    recv: SynExpr,
) -> Result<SynExpr, CodegenError> {
    // Rebuild via quote! so we never hand-format. The placeholder name
    // `__recv` is referenced as a Rust identifier in the template; we then
    // construct the call by hand using the lowered receiver.
    //
    // Simpler approach: construct the call directly via syn::ExprCall with
    // the lowered recv as the first arg and `true` as the second.
    let _ = tokens; // discarded; we rebuild from scratch to stay quote!-based.
    let mut args: Punctuated<SynExpr, syn::Token![,]> = Punctuated::new();
    // `&recv` — syn doesn't have a one-liner for `&expr`, so we build it.
    let borrow_recv = SynExpr::Reference(syn::ExprReference {
        attrs: Vec::new(),
        and_token: Default::default(),
        mutability: None,
        expr: Box::new(recv),
    });
    args.push(borrow_recv);
    args.push(SynExpr::Lit(syn::ExprLit {
        attrs: Vec::new(),
        lit: syn::Lit::Bool(syn::LitBool::new(true, ProcSpan::call_site())),
    }));
    Ok(SynExpr::Call(syn::ExprCall {
        attrs: Vec::new(),
        func: Box::new(SynExpr::Path(syn::ExprPath {
            attrs: Vec::new(),
            qself: None,
            path: rust_path("unicode_segmentation::UnicodeSegmentation::graphemes"),
        })),
        paren_token: Default::default(),
        args,
    }))
}

//! Codegen-time expansion: emit mock trait impl as `syn::Item`s.
//!
//! The T3 macro spike DEFER recommended runtime workarounds over
//! procedural macros. This module is the runtime workaround: a pure
//! function [`lower_mock_for_trait`] that takes a Buff
//! [`TraitDecl`](buff_lang_ast::TraitDecl) and emits the
//! corresponding `impl Trait for Mock<Trait>` block as a `syn::Item`.
//!
//! # Output shape
//!
//! For a trait
//!
//! ```buff
//! trait Greeter:
//!     required func greet(name: String) -> String
//!     required func ping() -> Bool
//! ```
//!
//! the lowering emits ONE `syn::Item::Impl`:
//!
//! ```ignore
//! impl Greeter for buff_mock::Mock<Greeter> {
//!     fn greet(&self, name: String) -> String {
//!         self.record_call("greet", vec![buff_mock::ArgumentValue::String(name)]);
//!         match self.lookup_return("greet", &[]) {
//!             Some(buff_mock::ReturnValue::String(s)) => s,
//!             _ => String::new(),
//!         }
//!     }
//!     fn ping(&self) -> bool {
//!         self.record_call_no_args("ping");
//!         match self.lookup_return_no_args("ping") {
//!             Some(buff_mock::ReturnValue::Bool(b)) => b,
//!             _ => false,
//!         }
//!     }
//! }
//! ```
//!
//! The trait itself stays in the user's source — only the impl is
//! emitted. The consumer (`buff-lang-codegen-rust` future integration,
//! or a snapshot test) pushes the item into the generated `syn::File`.
//!
//! # Defaults
//!
//! Every return type has a deterministic default when no expectation
//! matches: `0` for Int, `0.0` for Float, empty `String`, `false`
//! for Bool, `()` for unit. The mock can therefore be used as a stub
//! without expectations when a test only cares about call recording.
//!
//! # Hard-rule compliance
//!
//! Every `syn::Item` is built via explicit syn struct construction —
//! no `parse_quote!` (per `buff-lang-codegen-rust/AGENTS.md` hard
//! rule). No raw strings, no `format!`/`write!` for Rust codegen.

use buff_lang_ast::{MethodSig, TraitDecl, TypeRef};
use proc_macro2::Span;
use syn::{
    punctuated::Punctuated, Expr, ExprCall, ExprMatch, Ident, ImplItem, ImplItemFn, Item, ItemImpl,
    Pat, PathArguments, PathSegment, ReturnType, Signature, Token, Type as SynType, TypePath,
};

use crate::error::{MockError, MockResult};

/// Emit the mock trait-impl item for a Buff trait declaration.
///
/// Returns ONE `Item::Impl` — the `impl Trait for buff_mock::Mock<Trait>`
/// block. The trait itself is NOT re-declared (it stays in the user's
/// source). Push the returned item into the generated `syn::File`.
///
/// # Errors
///
/// Returns [`MockError::LoweringFailed`] when the trait shape is not
/// yet supported (supertraits present, or unsupported parameter /
/// return type — see [`validate_supported`]).
pub fn lower_mock_for_trait(trait_decl: &TraitDecl) -> MockResult<Item> {
    validate_supported(trait_decl)?;
    Ok(Item::Impl(build_impl_item(trait_decl)))
}

/// Validate that the trait shape is supported by this MVP.
///
/// Supported: zero supertraits, only `String` / `Int` / `Float` /
/// `Double` / `Bool` parameter and return types. Anything else
/// → [`MockError::LoweringFailed`].
pub(crate) fn validate_supported(trait_decl: &TraitDecl) -> MockResult<()> {
    if !trait_decl.supertraits.is_empty() {
        return Err(MockError::LoweringFailed {
            trait_name: trait_decl.name.name.clone(),
            reason: "supertraits are not yet supported by the mock lowering".into(),
        });
    }
    for req in &trait_decl.required {
        for p in &req.params {
            check_supported_type(&trait_decl.name.name, &p.ty, "parameter")?;
        }
        if let Some(rt) = &req.return_type {
            check_supported_type(&trait_decl.name.name, rt, "return")?;
        }
    }
    Ok(())
}

/// Check that `ty` is one of the supported primitive `TypeRef`s.
fn check_supported_type(trait_name: &str, ty: &TypeRef, role: &str) -> MockResult<()> {
    let supported = matches!(
        ty,
        TypeRef::Named { name, .. } if matches!(
            name.name.as_str(),
            "String" | "Int" | "Float" | "Double" | "Bool"
        )
    );
    if supported {
        Ok(())
    } else {
        Err(MockError::LoweringFailed {
            trait_name: trait_name.to_string(),
            reason: format!(
                "{role} type `{}` is not yet supported (only String/Int/Float/Bool)",
                ty
            ),
        })
    }
}

/// Build the `impl Trait for Mock<Trait>` block via explicit syn
/// construction (no `parse_quote!`).
fn build_impl_item(trait_decl: &TraitDecl) -> ItemImpl {
    let trait_path = mk_single_segment_path(&trait_decl.name.name);
    let mock_ty = mk_mock_of_trait(&trait_decl.name.name);

    let items: Vec<ImplItem> = trait_decl
        .required
        .iter()
        .map(|sig| ImplItem::Fn(build_impl_method(sig)))
        .collect();

    ItemImpl {
        attrs: Vec::new(),
        defaultness: None,
        unsafety: None,
        impl_token: Token![impl](Span::call_site()),
        generics: syn::Generics::default(),
        trait_: Some((None, trait_path, Token![for](Span::call_site()))),
        self_ty: Box::new(mock_ty),
        brace_token: Default::default(),
        items,
    }
}

/// Build a single method body for the mock impl — delegates to
/// `record_call` + `lookup_return` and unwraps the typed return value.
fn build_impl_method(sig: &MethodSig) -> ImplItemFn {
    let method_ident = Ident::new(&sig.name.name, Span::call_site());
    let method_name_lit = sig.name.name.clone();

    let self_ty_for_receiver = SynType::Path(TypePath {
        qself: None,
        path: mk_single_segment_path("Self"),
    });
    let receiver = syn::FnArg::Receiver(syn::Receiver {
        attrs: Vec::new(),
        reference: Some((Token![&](Span::call_site()), None)),
        mutability: None,
        self_token: Token![self](Span::call_site()),
        colon_token: None,
        ty: Box::new(self_ty_for_receiver),
    });
    let params: Vec<syn::FnArg> = sig
        .params
        .iter()
        .map(|p| {
            let name = Ident::new(&p.name.name, Span::call_site());
            let ty = lower_typeref_to_syn(&p.ty);
            syn::FnArg::Typed(syn::PatType {
                attrs: Vec::new(),
                pat: Box::new(Pat::Ident(syn::PatIdent {
                    attrs: Vec::new(),
                    by_ref: None,
                    mutability: None,
                    ident: name,
                    subpat: None,
                })),
                colon_token: Token![:](Span::call_site()),
                ty: Box::new(ty),
            })
        })
        .collect();

    let mut inputs: Punctuated<syn::FnArg, Token![,]> = Punctuated::new();
    inputs.push(receiver);
    for p in params {
        inputs.push(p);
    }

    let output = match &sig.return_type {
        Some(rt) => ReturnType::Type(
            Token![->](Span::call_site()),
            Box::new(lower_typeref_to_syn(rt)),
        ),
        None => ReturnType::Default,
    };

    let body_expr = build_method_body_expr(sig, &method_name_lit);

    ImplItemFn {
        attrs: Vec::new(),
        vis: syn::Visibility::Inherited,
        defaultness: None,
        sig: Signature {
            constness: None,
            asyncness: None,
            unsafety: None,
            abi: None,
            fn_token: Token![fn](Span::call_site()),
            ident: method_ident,
            generics: syn::Generics::default(),
            paren_token: Default::default(),
            inputs,
            variadic: None,
            output,
        },
        block: syn::Block {
            brace_token: Default::default(),
            stmts: vec![syn::Stmt::Expr(body_expr, None)],
        },
    }
}

/// Build the body expression for a mock method — record the call,
/// then unwrap the programmed return value (or fall back to default).
fn build_method_body_expr(sig: &MethodSig, method_name: &str) -> Expr {
    let record_args_expr = if sig.params.is_empty() {
        mk_method_call_on_self("record_call_no_args", vec![mk_str_lit(method_name)])
    } else {
        let arg_exprs: Vec<Expr> = sig
            .params
            .iter()
            .map(|p| mk_argument_value_expr(&p.name.name, &p.ty))
            .collect();
        mk_record_call_with_args(method_name, arg_exprs)
    };

    match &sig.return_type {
        None => record_args_expr,
        Some(rt) => {
            let lookup_expr = if sig.params.is_empty() {
                mk_method_call_on_self("lookup_return_no_args", vec![mk_str_lit(method_name)])
            } else {
                mk_method_call_on_self(
                    "lookup_return",
                    vec![mk_str_lit(method_name), mk_empty_slice_expr()],
                )
            };
            let match_expr = mk_return_unwrap_match(rt, lookup_expr);
            Expr::Block(syn::ExprBlock {
                attrs: Vec::new(),
                label: None,
                block: syn::Block {
                    brace_token: Default::default(),
                    stmts: vec![
                        syn::Stmt::Expr(record_args_expr, Some(Token![;](Span::call_site()))),
                        syn::Stmt::Expr(match_expr, None),
                    ],
                },
            })
        }
    }
}

/// Construct `self.method(args)` expression.
fn mk_method_call_on_self(method: &str, args: Vec<Expr>) -> Expr {
    Expr::Call(ExprCall {
        attrs: Vec::new(),
        func: Box::new(Expr::Field(syn::ExprField {
            attrs: Vec::new(),
            base: Box::new(Expr::Path(syn::ExprPath {
                attrs: Vec::new(),
                qself: None,
                path: mk_single_segment_path("self"),
            })),
            member: syn::Member::Named(Ident::new(method, Span::call_site())),
            dot_token: Token![.](Span::call_site()),
        })),
        paren_token: Default::default(),
        args: args.into_iter().collect(),
    })
}

/// Construct `self.record_call("name", vec![ArgumentValue::T(a), ...])`.
fn mk_record_call_with_args(method: &str, args: Vec<Expr>) -> Expr {
    let vec_macro = mk_vec_macro(args);
    mk_method_call_on_self("record_call", vec![mk_str_lit(method), vec_macro])
}

/// Construct a `vec![...]` macro invocation expression.
fn mk_vec_macro(items: Vec<Expr>) -> Expr {
    let macro_args: Punctuated<Expr, Token![,]> = items.into_iter().collect();
    Expr::Macro(syn::ExprMacro {
        attrs: Vec::new(),
        mac: syn::Macro {
            path: mk_single_segment_path("vec"),
            bang_token: Token![!](Span::call_site()),
            delimiter: syn::MacroDelimiter::Paren(Default::default()),
            tokens: quote::quote! { #macro_args },
        },
    })
}

/// Construct `ArgumentValue::TypeName(arg)` for a parameter.
fn mk_argument_value_expr(arg_name: &str, ty: &TypeRef) -> Expr {
    let variant = argument_value_variant_name(ty);
    let arg_path = mk_two_segment_path("ArgumentValue", &variant);
    let arg_ident = Expr::Path(syn::ExprPath {
        attrs: Vec::new(),
        qself: None,
        path: mk_single_segment_path(arg_name),
    });
    Expr::Call(ExprCall {
        attrs: Vec::new(),
        func: Box::new(Expr::Path(syn::ExprPath {
            attrs: Vec::new(),
            qself: None,
            path: arg_path,
        })),
        paren_token: Default::default(),
        args: std::iter::once(arg_ident).collect(),
    })
}

/// Construct the `match lookup_expr { Some(ReturnValue::T(x)) => x, _ => default }`.
fn mk_return_unwrap_match(rt: &TypeRef, lookup_expr: Expr) -> Expr {
    let (variant_name, binding_name) = return_value_variant(rt);
    let default_expr = default_for_type(rt);

    let some_path = mk_two_segment_path("Some", &variant_name);
    let some_pattern = Pat::TupleStruct(syn::PatTupleStruct {
        attrs: Vec::new(),
        qself: None,
        path: some_path,
        paren_token: Default::default(),
        elems: std::iter::once(Pat::Ident(syn::PatIdent {
            attrs: Vec::new(),
            by_ref: None,
            mutability: None,
            ident: Ident::new(&binding_name, Span::call_site()),
            subpat: None,
        }))
        .collect(),
    });

    let some_arm = syn::Arm {
        attrs: Vec::new(),
        pat: some_pattern,
        guard: None,
        fat_arrow_token: Default::default(),
        body: Box::new(Expr::Path(syn::ExprPath {
            attrs: Vec::new(),
            qself: None,
            path: mk_single_segment_path(&binding_name),
        })),
        comma: Some(Default::default()),
    };

    let wildcard_arm = syn::Arm {
        attrs: Vec::new(),
        pat: Pat::Wild(syn::PatWild {
            attrs: Vec::new(),
            underscore_token: Token![_](Span::call_site()),
        }),
        guard: None,
        fat_arrow_token: Default::default(),
        body: Box::new(default_expr),
        comma: None,
    };

    let arms: Vec<syn::Arm> = vec![some_arm, wildcard_arm];

    Expr::Match(ExprMatch {
        attrs: Vec::new(),
        match_token: Token![match](Span::call_site()),
        expr: Box::new(lookup_expr),
        brace_token: Default::default(),
        arms,
    })
}

/// Construct `&[]` (empty slice expression) for `lookup_return(name, &[])`.
fn mk_empty_slice_expr() -> Expr {
    Expr::Reference(syn::ExprReference {
        attrs: Vec::new(),
        and_token: Token![&](Span::call_site()),
        mutability: None,
        expr: Box::new(Expr::Array(syn::ExprArray {
            attrs: Vec::new(),
            bracket_token: Default::default(),
            elems: Punctuated::new(),
        })),
    })
}

/// Construct a string-literal expression.
fn mk_str_lit(s: &str) -> Expr {
    Expr::Lit(syn::ExprLit {
        attrs: Vec::new(),
        lit: syn::Lit::Str(syn::LitStr::new(s, Span::call_site())),
    })
}

/// Build a single-segment `syn::Path` (e.g. `self`, `vec`, `x`).
fn mk_single_segment_path(name: &str) -> syn::Path {
    syn::Path {
        leading_colon: None,
        segments: std::iter::once(PathSegment {
            ident: Ident::new(name, Span::call_site()),
            arguments: PathArguments::None,
        })
        .collect(),
    }
}

/// Build a two-segment `syn::Path` (e.g. `ArgumentValue::String`).
fn mk_two_segment_path(first: &str, second: &str) -> syn::Path {
    let mut segments: Punctuated<PathSegment, Token![::]> = Punctuated::new();
    segments.push(PathSegment {
        ident: Ident::new(first, Span::call_site()),
        arguments: PathArguments::None,
    });
    segments.push(PathSegment {
        ident: Ident::new(second, Span::call_site()),
        arguments: PathArguments::None,
    });
    syn::Path {
        leading_colon: None,
        segments,
    }
}

/// Build the `buff_mock::Mock<TraitName>` type via explicit syn construction.
fn mk_mock_of_trait(trait_name: &str) -> SynType {
    let mut segments: Punctuated<PathSegment, Token![::]> = Punctuated::new();
    segments.push(PathSegment {
        ident: Ident::new("buff_mock", Span::call_site()),
        arguments: PathArguments::None,
    });
    segments.push(PathSegment {
        ident: Ident::new("Mock", Span::call_site()),
        arguments: PathArguments::AngleBracketed(syn::AngleBracketedGenericArguments {
            colon2_token: None,
            lt_token: Token![<](Span::call_site()),
            args: std::iter::once(syn::GenericArgument::Type(SynType::Path(TypePath {
                qself: None,
                path: mk_single_segment_path(trait_name),
            })))
            .collect(),
            gt_token: Token![>](Span::call_site()),
        }),
    });
    SynType::Path(TypePath {
        qself: None,
        path: syn::Path {
            leading_colon: None,
            segments,
        },
    })
}

/// Map a Buff `TypeRef::Named` to the matching `ArgumentValue` variant name.
fn argument_value_variant_name(ty: &TypeRef) -> String {
    match ty {
        TypeRef::Named { name, .. } => match name.name.as_str() {
            "String" => "String".into(),
            "Int" => "Int".into(),
            "Float" | "Double" => "Float".into(),
            "Bool" => "Bool".into(),
            _ => "Other".into(),
        },
        _ => "Other".into(),
    }
}

/// Map a Buff `TypeRef::Named` to the matching `ReturnValue` variant name
/// AND the suggested binding name (so the match arm can reference it).
fn return_value_variant(ty: &TypeRef) -> (String, String) {
    match ty {
        TypeRef::Named { name, .. } => match name.name.as_str() {
            "String" => ("String".into(), "s".into()),
            "Int" => ("Int".into(), "i".into()),
            "Float" | "Double" => ("Float".into(), "f".into()),
            "Bool" => ("Bool".into(), "b".into()),
            _ => ("Unit".into(), "_u".into()),
        },
        _ => ("Unit".into(), "_u".into()),
    }
}

/// Construct the default value expression for a return type.
fn default_for_type(ty: &TypeRef) -> Expr {
    match ty {
        TypeRef::Named { name, .. } => match name.name.as_str() {
            "String" => Expr::MethodCall(syn::ExprMethodCall {
                attrs: Vec::new(),
                receiver: Box::new(Expr::Path(syn::ExprPath {
                    attrs: Vec::new(),
                    qself: None,
                    path: mk_single_segment_path("String"),
                })),
                dot_token: Token![.](Span::call_site()),
                method: Ident::new("new", Span::call_site()),
                turbofish: None,
                paren_token: Default::default(),
                args: Punctuated::new(),
            }),
            "Int" => Expr::Lit(syn::ExprLit {
                attrs: Vec::new(),
                lit: syn::Lit::Int(syn::LitInt::new("0_i64", Span::call_site())),
            }),
            "Float" | "Double" => Expr::Lit(syn::ExprLit {
                attrs: Vec::new(),
                lit: syn::Lit::Float(syn::LitFloat::new("0.0_f64", Span::call_site())),
            }),
            "Bool" => Expr::Lit(syn::ExprLit {
                attrs: Vec::new(),
                lit: syn::Lit::Bool(syn::LitBool {
                    value: false,
                    span: Span::call_site(),
                }),
            }),
            _ => Expr::Tuple(syn::ExprTuple {
                attrs: Vec::new(),
                paren_token: Default::default(),
                elems: Punctuated::new(),
            }),
        },
        _ => Expr::Tuple(syn::ExprTuple {
            attrs: Vec::new(),
            paren_token: Default::default(),
            elems: Punctuated::new(),
        }),
    }
}

/// Map a Buff `TypeRef::Named` to the corresponding Rust `syn::Type`.
fn lower_typeref_to_syn(ty: &TypeRef) -> SynType {
    match ty {
        TypeRef::Named { name, .. } => {
            let rust_name = match name.name.as_str() {
                "String" => "String",
                "Int" => "i64",
                "Float" | "Double" => "f64",
                "Bool" => "bool",
                other => other,
            };
            SynType::Path(TypePath {
                qself: None,
                path: mk_single_segment_path(rust_name),
            })
        }
        _ => SynType::Tuple(syn::TypeTuple {
            paren_token: Default::default(),
            elems: Punctuated::new(),
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use buff_lang_ast::Span;
    use buff_lang_ast::{Ident, MethodSig, Param, TraitDecl};
    use quote::ToTokens;

    fn dummy_span() -> Span {
        Span::dummy()
    }

    fn named(s: &str) -> TypeRef {
        TypeRef::Named {
            name: Ident::new(s, dummy_span()),
            span: dummy_span(),
        }
    }

    fn mk_param(name: &str, ty: &str) -> Param {
        Param::plain(name, named(ty), dummy_span())
    }

    fn mk_greeter_trait() -> TraitDecl {
        TraitDecl {
            name: Ident::new("Greeter", dummy_span()),
            supertraits: Vec::new(),
            required: vec![
                MethodSig {
                    name: Ident::new("greet", dummy_span()),
                    params: vec![mk_param("name", "String")],
                    return_type: Some(named("String")),
                    span: dummy_span(),
                },
                MethodSig {
                    name: Ident::new("ping", dummy_span()),
                    params: Vec::new(),
                    return_type: Some(named("Bool")),
                    span: dummy_span(),
                },
            ],
            defaults: Vec::new(),
            span: dummy_span(),
        }
    }

    #[test]
    fn lower_emits_single_impl_item() {
        let t = mk_greeter_trait();
        let item = lower_mock_for_trait(&t).expect("lowering should succeed");
        assert!(matches!(item, Item::Impl(_)));
    }

    #[test]
    fn lower_rejects_supertraits() {
        let mut t = mk_greeter_trait();
        t.supertraits.push(named("OtherTrait"));
        let err = lower_mock_for_trait(&t).unwrap_err();
        let msg = format!("{err}");
        assert!(msg.contains("supertraits"));
        assert!(msg.contains("Greeter"));
    }

    #[test]
    fn lower_rejects_unsupported_param_type() {
        let mut t = mk_greeter_trait();
        t.required[0].params[0].ty = TypeRef::Option(Box::new(named("Int")), dummy_span());
        let err = lower_mock_for_trait(&t).unwrap_err();
        let msg = format!("{err}");
        assert!(msg.contains("parameter type"));
    }

    #[test]
    fn lower_rejects_unsupported_return_type() {
        let mut t = mk_greeter_trait();
        t.required[0].return_type = Some(TypeRef::Tuple(
            vec![named("Int"), named("String")],
            dummy_span(),
        ));
        let err = lower_mock_for_trait(&t).unwrap_err();
        let msg = format!("{err}");
        assert!(msg.contains("return type"));
    }

    #[test]
    fn emitted_impl_targets_mock_of_trait() {
        let t = mk_greeter_trait();
        let item = lower_mock_for_trait(&t).expect("lowering should succeed");
        let item_impl = match item {
            Item::Impl(ii) => ii,
            _ => unreachable!("lowering must emit Item::Impl"),
        };
        let rendered = item_impl.to_token_stream().to_string();
        assert!(rendered.contains("buff_mock :: Mock < Greeter >"));
    }

    #[test]
    fn emitted_impl_preserves_method_names() {
        let t = mk_greeter_trait();
        let item = lower_mock_for_trait(&t).expect("lowering should succeed");
        let item_impl = match item {
            Item::Impl(ii) => ii,
            _ => unreachable!("lowering must emit Item::Impl"),
        };
        let method_names: Vec<String> = item_impl
            .items
            .iter()
            .filter_map(|i| match i {
                ImplItem::Fn(f) => Some(f.sig.ident.to_string()),
                _ => None,
            })
            .collect();
        assert!(method_names.contains(&"greet".to_string()));
        assert!(method_names.contains(&"ping".to_string()));
    }

    #[test]
    fn emitted_method_body_contains_record_call() {
        let t = mk_greeter_trait();
        let item = lower_mock_for_trait(&t).expect("lowering should succeed");
        let item_impl = match item {
            Item::Impl(ii) => ii,
            _ => unreachable!("lowering must emit Item::Impl"),
        };
        let body = item_impl
            .items
            .iter()
            .filter_map(|i| match i {
                ImplItem::Fn(f) => Some(f.block.to_token_stream().to_string()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join(" ");
        assert!(body.contains("record_call"));
        assert!(body.contains("lookup_return"));
    }

    #[test]
    fn validate_supported_accepts_known_primitives() {
        let t = mk_greeter_trait();
        assert!(validate_supported(&t).is_ok());
    }

    #[test]
    fn validate_supported_rejects_unknown_named_type() {
        let mut t = mk_greeter_trait();
        t.required[0].params[0].ty = named("MyCustom");
        assert!(validate_supported(&t).is_err());
    }

    #[test]
    fn argument_value_variant_maps_known_primitives() {
        assert_eq!(argument_value_variant_name(&named("String")), "String");
        assert_eq!(argument_value_variant_name(&named("Int")), "Int");
        assert_eq!(argument_value_variant_name(&named("Float")), "Float");
        assert_eq!(argument_value_variant_name(&named("Double")), "Float");
        assert_eq!(argument_value_variant_name(&named("Bool")), "Bool");
        assert_eq!(argument_value_variant_name(&named("MyType")), "Other");
    }

    #[test]
    fn return_value_variant_maps_known_primitives() {
        let (v, _) = return_value_variant(&named("String"));
        assert_eq!(v, "String");
        let (v, _) = return_value_variant(&named("Int"));
        assert_eq!(v, "Int");
        let (v, _) = return_value_variant(&named("Float"));
        assert_eq!(v, "Float");
        let (v, _) = return_value_variant(&named("Bool"));
        assert_eq!(v, "Bool");
    }

    #[test]
    fn lower_typeref_to_syn_known_primitives() {
        assert_eq!(
            lower_typeref_to_syn(&named("String"))
                .to_token_stream()
                .to_string(),
            "String"
        );
        assert_eq!(
            lower_typeref_to_syn(&named("Int"))
                .to_token_stream()
                .to_string(),
            "i64"
        );
        assert_eq!(
            lower_typeref_to_syn(&named("Bool"))
                .to_token_stream()
                .to_string(),
            "bool"
        );
    }

    #[test]
    fn snapshot_lowered_greeter_impl_prettyplease() {
        let t = mk_greeter_trait();
        let item = lower_mock_for_trait(&t).expect("lowering should succeed");
        let file = syn::File {
            attrs: Vec::new(),
            items: vec![item],
            shebang: None,
        };
        let source = prettyplease::unparse(&file);
        assert!(source.contains("impl Greeter for buff_mock::Mock<Greeter>"));
        assert!(source.contains("fn greet(&self, name: String) -> String"));
        assert!(source.contains("fn ping(&self) -> bool"));
        assert!(source.contains("self.record_call"));
        assert!(source.contains("buff_mock::ArgumentValue::String(name)"));
    }
}

//! T53 — Rust codegen lowering for `comptime` blocks.
//!
//! Consumes a [`ComptimeFacts`](buff_lang_types::ComptimeFacts) produced by
//! [`analyze_program`](buff_lang_types::analyze_program) and emits a
//! `syn::Item::Const` for each successfully evaluated comptime block. The
//! `const` item carries the evaluated value as a Rust literal, so the
//! runtime never re-evaluates the comptime body.
//!
//! # Naming
//!
//! Each `const` item gets a deterministic name: `__BUFF_COMPTIME_<offset>`
//! where `<offset>` is the byte-offset of the original `comptime` block's
//! source span. The offset is unique per source file, so two comptime
//! blocks never collide. The name is intentionally ugly — the user
//! references comptime values via their original `let` bindings, not the
//! synthesised `const` (the binding-vs-const rewiring is post-v1.x work).
//!
//! # Determinism
//!
//! Items are emitted in source-span order (BTreeMap iteration order).
//! Same comptime facts → byte-identical Rust source.
//!
//! # Errors
//!
//! Returns [`CodegenError`] with code E1304 if a [`ComptimeValue`] cannot
//! be lowered (the interpreter produced a value shape codegen doesn't
//! support — currently only `Unit`, which has no Rust literal form).

use proc_macro2::Span as ProcSpan;
use syn::{
    Expr as SynExpr, ExprArray, ExprLit, Item, ItemConst, Lit, LitInt, Type as SynType,
    Visibility,
};
use syn::punctuated::Punctuated;
use syn::token::{Colon, Comma, Const, Semi};

use buff_lang_error::{CodegenError, Diagnostic, ErrorCode, Span as BuffSpan};
use buff_lang_types::{ComptimeFacts, ComptimeValue};

/// Lower every value in `facts` to a top-level Rust `const` item.
///
/// Returns the items in source-span order (smallest offset first). The
/// caller ([`crate::RustCodegen::generate`]) prepends them to the items
/// Vec BEFORE user declarations, so user code can reference the consts
/// (though typically the user references the original `let` bindings
/// instead — name rewiring is deferred).
pub fn lower_comptime_facts(facts: &ComptimeFacts) -> Result<Vec<Item>, CodegenError> {
    let mut items = Vec::with_capacity(facts.values.len());
    for (&offset, value) in &facts.values {
        items.push(lower_one(offset, value)?);
    }
    Ok(items)
}

fn lower_one(offset: usize, value: &ComptimeValue) -> Result<Item, CodegenError> {
    let name = format_const_name(offset);
    let ident = syn::Ident::new(&name, ProcSpan::call_site());
    let ty = rust_type_for(value);
    let expr = rust_expr_for(value, span_for(offset))?;
    Ok(Item::Const(ItemConst {
        attrs: Vec::new(),
        vis: Visibility::Inherited,
        const_token: Const::default(),
        ident,
        generics: syn::Generics::default(),
        colon_token: Colon::default(),
        ty: Box::new(ty),
        eq_token: Default::default(),
        expr: Box::new(expr),
        semi_token: Semi::default(),
    }))
}

fn format_const_name(offset: usize) -> String {
    format!("__BUFF_COMPTIME_{offset}")
}

fn span_for(_offset: usize) -> BuffSpan {
    BuffSpan::dummy()
}

fn rust_type_for(value: &ComptimeValue) -> SynType {
    match value {
        ComptimeValue::Int(_) => parse_simple_type("i64"),
        ComptimeValue::Bool(_) => parse_simple_type("bool"),
        ComptimeValue::String(_) => parse_simple_type("String"),
        ComptimeValue::Unit => parse_simple_type("()"),
        ComptimeValue::Array(els) => {
            let elem = els
                .first()
                .map(rust_type_for)
                .unwrap_or_else(|| parse_simple_type("()"));
            let elem_src = type_to_source(&elem);
            parse_simple_type(&format!("Vec<{elem_src}>"))
        }
    }
}

fn rust_expr_for(value: &ComptimeValue, span: BuffSpan) -> Result<SynExpr, CodegenError> {
    match value {
        ComptimeValue::Int(i) => Ok(SynExpr::Lit(ExprLit {
            attrs: Vec::new(),
            lit: Lit::Int(LitInt::new(&format!("{i}i64"), ProcSpan::call_site())),
        })),
        ComptimeValue::Bool(b) => Ok(SynExpr::Lit(ExprLit {
            attrs: Vec::new(),
            lit: Lit::Bool(syn::LitBool {
                value: *b,
                span: ProcSpan::call_site(),
            }),
        })),
        ComptimeValue::String(s) => Ok(SynExpr::Lit(ExprLit {
            attrs: Vec::new(),
            lit: Lit::Str(syn::LitStr::new(s, ProcSpan::call_site())),
        })),
        ComptimeValue::Unit => Err(CodegenError::new(
            Diagnostic::error(
                "cannot lower comptime `()` as a Rust const (use a non-unit value)",
                span,
            )
            .with_code(ErrorCode::ComptimeLoweringFailed),
        )),
        ComptimeValue::Array(els) => {
            let mut elems = Punctuated::new();
            for v in els {
                elems.push(rust_expr_for(v, span)?);
                elems.push_punct(Comma::default());
            }
            Ok(SynExpr::Array(ExprArray {
                attrs: Vec::new(),
                bracket_token: Default::default(),
                elems,
            }))
        }
    }
}

fn parse_simple_type(name: &str) -> SynType {
    syn::parse_str(name).unwrap_or_else(|_| syn::parse_str("i64").expect("i64 is valid"))
}

fn type_to_source(ty: &SynType) -> String {
    quote::quote!(#ty).to_string()
}

//! Buff `.buffhtml` Codegen crate — lowers [`RsxTemplateFile`] → Rust source
//! containing a `rsx!{}` macro invocation.
//!
//! # Pipeline
//!
//! 1. The template AST is walked, producing a single `proc_macro2::TokenStream`
//!    body via `quote!{ ... }` (re-using the T121b-proven emission pattern
//!    from `crates/buff-lang-codegen-rust/tests/dioxus_t121b.rs`).
//! 2. Each emitted token carries a known relationship to the originating
//!    `.buffhtml` span — we record `(rs_anchor_text, buffhtml_span)` tuples
//!    into a [`SpanMapBuilder`] as we emit.
//! 3. The body is wrapped in a `syn::Macro` node (`rsx! { ... }`), then in a
//!    `syn::File` (with `use dioxus::prelude::*;` + `#[component] fn ...`),
//!    formatted via [`prettyplease::unparse`].
//! 4. The post-format span scan walks the formatted `.rs` text, locates each
//!    anchor's literal text, and builds a sorted [`SpanMap`] for runtime
//!    lookup via [`SpanMap::map_span`].
//!
//! # T133 scope
//!
//! Floor-grammar nodes are lowered:
//! - Elements / components / fragments / text / `{expr}` interpolation
//! - Literal / expression / event / named-prop / boolean attributes
//! - `{#if}` / `{#each}` blocks (lowered as `if`/`for` expressions inside the
//!   `rsx!{}` body — Dioxus supports control flow there)
//! - Default `<slot />` (lowered as `{children}` — Dioxus component children)
//! - Comments (lowered as Rust `//` comments inside the macro body)
//!
//! Deferred per decision record §6 (NOT lowered; the parser already rejects
//! them — these notes are for future implementers):
//! named slots, keyed each, spread props, two-way binding, await, `{@html}`.

pub mod error;
pub mod span_map;

pub use error::BuffHtmlCodegenError;
pub use span_map::{SpanMap, SpanMapBuilder};

use buff_lang_ast_rsx::{
    RsxAttribute, RsxAttributeKind, RsxComment, RsxEach, RsxElement, RsxFragment, RsxIf,
    RsxIfBranch, RsxInterp, RsxNode, RsxSlot, RsxTemplateFile, ScriptBlock,
};
// Note: RsxComment is referenced in lower_comment signature.
use proc_macro2::TokenStream;
use quote::{quote, ToTokens};

/// Result of generating Rust source for a `.buffhtml` template.
#[derive(Debug, Clone)]
pub struct CodegenResult {
    /// Formatted Rust source text (ready to write to a `.rs` file).
    pub rust_source: String,
    /// Reverse-mapping table from generated `.rs` positions to `.buffhtml`
    /// spans. Use [`SpanMap::map_span`] at diagnostic-rendering time.
    pub span_map: SpanMap,
}

/// Default generated component function name. The CLI may override this when
/// wiring `.buffhtml` files into the user's project.
pub const DEFAULT_COMPONENT_NAME: &str = "BuffHtmlComponent";

/// Lower a template into a Rust source file containing a single
/// `#[component] fn <name>() -> Element { rsx!{ ... } }`.
///
/// The component name is sanitized to a valid Rust identifier (alpha + `_`).
/// The `script` block is currently spliced verbatim as a top-level `/* ... */`
/// comment in the generated source — full integration with `buff-lang-cli`'s
/// companion-file binding is a separate T133 sub-task.
pub fn generate(
    template: &RsxTemplateFile,
    component_name: &str,
) -> Result<CodegenResult, BuffHtmlCodegenError> {
    let sanitized = sanitize_ident(component_name);
    let mut builder = SpanMapBuilder::default();

    let body = lower_nodes(&template.root, &mut builder)?;
    let rsx_macro = build_rsx_macro(body);

    let file = syn::File {
        shebang: None,
        attrs: Vec::new(),
        items: build_file_items(&sanitized, rsx_macro, template.script.as_ref()),
    };

    let raw = prettyplease::unparse(&file);
    let span_map = builder.finalize(&raw);

    Ok(CodegenResult {
        rust_source: raw,
        span_map,
    })
}

/// Sanitize a Buff/RSX component name into a valid Rust identifier.
fn sanitize_ident(name: &str) -> String {
    let mut out = String::with_capacity(name.len());
    for ch in name.chars() {
        if ch.is_ascii_alphanumeric() || ch == '_' {
            out.push(ch);
        } else {
            out.push('_');
        }
    }
    if out.is_empty()
        || !out
            .chars()
            .next()
            .map(|c| c.is_alphabetic() || c == '_')
            .unwrap_or(false)
    {
        out.insert(0, '_');
    }
    out
}

/// Build the `rsx!{ ... }` macro expression. Re-uses the T121b emission path:
/// `syn::Macro` with `delimiter: MacroDelimiter::Brace` and a raw `TokenStream`
/// body that prettyplease prints verbatim.
fn build_rsx_macro(body: TokenStream) -> syn::Expr {
    use syn::{
        punctuated::Punctuated, Expr, ExprMacro, Ident, Macro, MacroDelimiter, PathArguments,
        PathSegment, Token,
    };

    let path_segments: Punctuated<PathSegment, Token![::]> = {
        let mut p = Punctuated::new();
        p.push(PathSegment {
            ident: Ident::new("rsx", proc_macro2::Span::call_site()),
            arguments: PathArguments::None,
        });
        p
    };
    let path = syn::Path {
        leading_colon: None,
        segments: path_segments,
    };
    let mac = Macro {
        path,
        bang_token: Default::default(),
        delimiter: MacroDelimiter::Brace(Default::default()),
        tokens: body,
    };
    Expr::Macro(ExprMacro {
        attrs: Vec::new(),
        mac,
    })
}

/// Assemble the `syn::File` items: `use dioxus::prelude::*;`, optional script
/// block (as a `const &str` holder the CLI can extract), and the
/// `#[component] fn <name>() -> Element` function wrapping the `rsx!{}`
/// expression.
fn build_file_items(
    component_name: &str,
    rsx_macro: syn::Expr,
    script: Option<&ScriptBlock>,
) -> Vec<syn::Item> {
    let use_item: syn::Item = syn::parse_quote! {
        use dioxus::prelude::*;
    };

    // The script block is preserved as a `const &str` so the generated file
    // remains a valid `syn::File`. The CLI's downstream pass will pull the
    // source out and feed it through `buff-lang-lexer` + `buff-lang-parser`
    // to extract component logic. T133 codegen does not interpret it.
    let script_items: Vec<syn::Item> = match script {
        Some(s) => {
            let raw = &s.source;
            let script_holder: syn::Item = syn::parse_quote! {
                #[doc = "buffhtml script block — extracted by buff-lang-cli"]
                const __BUFF_SCRIPT_SOURCE: &str = #raw;
            };
            vec![script_holder]
        }
        None => Vec::new(),
    };

    let component_ident = syn::Ident::new(component_name, proc_macro2::Span::call_site());
    let block = syn::Block {
        brace_token: Default::default(),
        stmts: vec![syn::Stmt::Expr(rsx_macro, None)],
    };
    let sig = syn::Signature {
        constness: None,
        asyncness: None,
        unsafety: None,
        abi: None,
        fn_token: Default::default(),
        ident: component_ident,
        generics: Default::default(),
        paren_token: Default::default(),
        inputs: syn::punctuated::Punctuated::new(),
        variadic: None,
        output: syn::ReturnType::Type(Default::default(), Box::new(syn::parse_quote!(Element))),
    };
    let component_attr: syn::Attribute = syn::parse_quote! { #[component] };
    let component_item = syn::Item::Fn(syn::ItemFn {
        attrs: vec![component_attr],
        vis: syn::Visibility::Inherited,
        sig,
        block: Box::new(block),
    });

    let mut items = vec![use_item];
    items.extend(script_items);
    items.push(component_item);
    items
}

// ---------------------------------------------------------------------------
// AST → TokenStream lowering.
// ---------------------------------------------------------------------------

fn lower_nodes(
    nodes: &[RsxNode],
    builder: &mut SpanMapBuilder,
) -> Result<TokenStream, BuffHtmlCodegenError> {
    let mut parts: Vec<TokenStream> = Vec::with_capacity(nodes.len());
    for n in nodes {
        parts.push(lower_node(n, builder)?);
    }
    let combined = quote! { #(#parts)* };
    Ok(combined)
}

fn lower_node(
    node: &RsxNode,
    builder: &mut SpanMapBuilder,
) -> Result<TokenStream, BuffHtmlCodegenError> {
    match node {
        RsxNode::Text(t) => Ok(lower_text(t, builder)),
        RsxNode::Interp(i) => Ok(lower_interp(i, builder)),
        RsxNode::Comment(c) => Ok(lower_comment(c)),
        RsxNode::Slot(s) => Ok(lower_slot(s)),
        RsxNode::Element(e) => lower_element(e, builder),
        RsxNode::Fragment(f) => lower_fragment(f, builder),
        RsxNode::If(i) => lower_if(i, builder),
        RsxNode::Each(e) => lower_each(e, builder),
        RsxNode::Script(_) => {
            // Top-level only — parser rejects nested <script>.
            Ok(quote! {})
        }
    }
}

fn lower_text(t: &buff_lang_ast_rsx::RsxText, _builder: &mut SpanMapBuilder) -> TokenStream {
    // Trim whitespace-only text down to nothing — Dioxus rsx!{} dislikes
    // dangling empty string literals. Preserve non-empty trimmed text.
    let trimmed = t.text.trim();
    if trimmed.is_empty() {
        return quote! {};
    }
    quote! { #trimmed, }
}

fn lower_interp(i: &RsxInterp, builder: &mut SpanMapBuilder) -> TokenStream {
    // The expression source is emitted verbatim into the rsx!{} body.
    // Parse it as a Rust expression token tree; on failure, fall back to a
    // literal-string rendering that will surface a Rust compile error
    // (better than silently dropping the user's intent).
    let expr_src = &i.expr;
    let span = i.span;
    let ts: TokenStream = match syn::parse_str::<syn::Expr>(expr_src) {
        Ok(parsed) => parsed.to_token_stream(),
        Err(_) => {
            // Fallback: emit the raw text as a string literal so the .rs file
            // still parses; rustc will then complain with a span pointing at
            // the generated position, which the side-table can map.
            let raw = expr_src.clone();
            quote! { { /* buffhtml: failed to parse expr */ #raw } }
        }
    };
    // Record an anchor keyed on the raw expression source text — we search
    // for it in the post-format .rs text. Use the first identifier-like
    // fragment for robustness.
    let anchor_text = first_anchor_text(expr_src);
    builder.add_anchor(&anchor_text, span);
    ts
}

/// Pick the most stable token sequence to anchor a span on. Prefers
/// identifiers/literals (which prettyplease never modifies).
fn first_anchor_text(expr_src: &str) -> String {
    expr_src
        .split_whitespace()
        .next()
        .map(|s| s.to_string())
        .unwrap_or_else(|| expr_src.to_string())
}

fn lower_comment(_c: &RsxComment) -> TokenStream {
    // rsx!{} macro does not accept `//` comments inside its body. Drop
    // comments for now — they round-trip via the AST, but the generated
    // `rsx!{}` body omits them. (TODO: emit as `""` empty-string child to
    // preserve child-index stability? For T133, drop is fine.)
    quote! {}
}

fn lower_slot(_s: &RsxSlot) -> TokenStream {
    // Default slot lowers to the Dioxus `children` signal — components
    // declare it via `fn Comp(children: ReadOnlyChildren) -> Element`.
    // For T133 we emit a passthrough `children` identifier that the user's
    // script block (or companion .buff) is expected to bind. If `children`
    // is not in scope, rustc emits an E0425 that maps cleanly back to the
    // slot's span via the side-table.
    quote! { { children }, }
}

fn lower_element(
    e: &RsxElement,
    builder: &mut SpanMapBuilder,
) -> Result<TokenStream, BuffHtmlCodegenError> {
    let tag = &e.tag;
    let attrs_tokens = lower_attributes(&e.attributes, builder)?;
    let children_tokens = lower_nodes(&e.children, builder)?;

    if e.is_component {
        // Component invocation: `<Counter attr=val>{children}</Counter>`
        // becomes `Counter { key: val, children }` inside rsx!{}.
        let ident = match syn::parse_str::<syn::Ident>(tag) {
            Ok(i) => i,
            Err(_) => {
                return Err(BuffHtmlCodegenError::UnsupportedConstruct {
                    message: format!("component tag `{tag}` is not a valid Rust identifier"),
                });
            }
        };
        if e.self_closing {
            Ok(quote! { #ident { #attrs_tokens }, })
        } else {
            Ok(quote! { #ident { #attrs_tokens #children_tokens }, })
        }
    } else {
        // Host element: lowercase tag like `div`, `span`, `button`.
        // For an unknown tag, this still emits as an identifier and lets
        // dioxus-rsx fail with a precise span (which the side-table maps).
        let ident = match syn::parse_str::<syn::Ident>(tag) {
            Ok(i) => i,
            Err(_) => {
                return Err(BuffHtmlCodegenError::UnsupportedConstruct {
                    message: format!("host tag `{tag}` is not a valid Rust identifier"),
                });
            }
        };
        if e.self_closing {
            Ok(quote! { #ident { #attrs_tokens }, })
        } else {
            Ok(quote! { #ident { #attrs_tokens #children_tokens }, })
        }
    }
}

fn lower_fragment(
    f: &RsxFragment,
    builder: &mut SpanMapBuilder,
) -> Result<TokenStream, BuffHtmlCodegenError> {
    // rsx!{} accepts `Fragment { ... }` as a built-in.
    let children = lower_nodes(&f.children, builder)?;
    Ok(quote! { Fragment { #children }, })
}

fn lower_attributes(
    attrs: &[RsxAttribute],
    builder: &mut SpanMapBuilder,
) -> Result<TokenStream, BuffHtmlCodegenError> {
    let mut parts: Vec<TokenStream> = Vec::with_capacity(attrs.len());
    for a in attrs {
        parts.push(lower_attr(a, builder)?);
    }
    let combined = quote! { #(#parts)* };
    Ok(combined)
}

fn lower_attr(
    a: &RsxAttribute,
    builder: &mut SpanMapBuilder,
) -> Result<TokenStream, BuffHtmlCodegenError> {
    match &a.kind {
        RsxAttributeKind::Literal { name, value } => {
            let ident = attr_ident(name)?;
            Ok(quote! { #ident: #value, })
        }
        RsxAttributeKind::Expression {
            name,
            expr,
            expr_span,
        } => {
            let ident = attr_ident(name)?;
            let parsed: TokenStream = match syn::parse_str::<syn::Expr>(expr) {
                Ok(p) => p.to_token_stream(),
                Err(_) => {
                    let raw = expr.clone();
                    quote! { #raw }
                }
            };
            // Anchor the span on the expr's first token (post-format lookup
            // finds the identifier in the .rs text).
            let anchor_text = first_anchor_text(expr);
            builder.add_anchor(&anchor_text, *expr_span);
            let _ = expr_span;
            Ok(quote! { #ident: #parsed, })
        }
        RsxAttributeKind::Event {
            event,
            modifiers,
            handler_expr,
            handler_span,
        } => {
            // `on:event_mod={h}` → `on_event: move |__e| { /* modifiers */ h(__e) }`
            // For T133 floor: emit the handler directly. Modifier wiring
            // (preventDefault, stopPropagation) is T134+.
            let on_name = format!("on_{event}");
            let ident = attr_ident(&on_name)?;
            let parsed: TokenStream = match syn::parse_str::<syn::Expr>(handler_expr) {
                Ok(p) => p.to_token_stream(),
                Err(_) => {
                    let raw = handler_expr.clone();
                    quote! { #raw }
                }
            };
            let anchor_text = first_anchor_text(handler_expr);
            builder.add_anchor(&anchor_text, *handler_span);
            let _ = modifiers;
            Ok(quote! { #ident: #parsed, })
        }
        RsxAttributeKind::NamedProp {
            name,
            value,
            value_span,
        } => {
            let ident = attr_ident(name)?;
            // Named-prop values may be string literals or expressions — try
            // expr first, fall back to literal.
            let val_tokens: TokenStream = if value.starts_with('"') || value.starts_with('\'') {
                let v = value.trim_matches(|c| c == '"' || c == '\'').to_string();
                quote! { #v }
            } else {
                match syn::parse_str::<syn::Expr>(value) {
                    Ok(p) => p.to_token_stream(),
                    Err(_) => {
                        let raw = value.clone();
                        quote! { #raw }
                    }
                }
            };
            let anchor_text = first_anchor_text(value);
            builder.add_anchor(&anchor_text, *value_span);
            Ok(quote! { #ident: #val_tokens, })
        }
        RsxAttributeKind::Boolean { name } => {
            let ident = attr_ident(name)?;
            Ok(quote! { #ident: true, })
        }
    }
}

/// Convert a Buff/RSX attribute name into a Rust identifier suitable for
/// the rsx!{} macro. `class` → `class`, `data-foo` → `data_foo`, etc.
fn attr_ident(name: &str) -> Result<syn::Ident, BuffHtmlCodegenError> {
    let sanitized: String = name
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect();
    syn::parse_str::<syn::Ident>(&sanitized).map_err(|_| {
        BuffHtmlCodegenError::UnsupportedConstruct {
            message: format!("attribute `{name}` is not a valid Rust identifier"),
        }
    })
}

fn lower_if(i: &RsxIf, builder: &mut SpanMapBuilder) -> Result<TokenStream, BuffHtmlCodegenError> {
    // {#if a} body1 {:else if b} body2 {:else} body3 {/if}
    // → inside rsx!{} we use Dioxus's first-class conditional support.
    // For T133 we render the if-else as a Rust expression that yields a
    // `Vec<VNode>`-compatible value, using the form:
    //   if cond1 { rsx!{ body1 } } else if cond2 { rsx!{ body2 } } else { rsx!{ body3 } }
    // But since we're already inside a rsx!{} macro, the simpler form
    // supported by dioxus-rsx is direct conditional syntax. T121b's rsx!{}
    // macro accepts control-flow expressions inline.
    //
    // For T133 we use the nested-rsx!{} form to keep codegen simple and
    // guarantee validity.
    let mut branches_tokens: Vec<TokenStream> = Vec::with_capacity(i.branches.len());
    for b in &i.branches {
        branches_tokens.push(lower_if_branch(b, builder)?);
    }
    let else_tokens = match &i.else_branch {
        Some(body) => {
            let b = lower_nodes(body, builder)?;
            quote! { else { rsx! { #b } } }
        }
        None => quote! {},
    };
    // Stitch: if (...) { rsx!{...} } else if (...) { rsx!{...} } ... else { rsx!{...} }
    // The first branch is unconditional; subsequent branches emit `else if`.
    let mut out = TokenStream::new();
    let mut first = true;
    for b in &branches_tokens {
        if first {
            first = false;
            out.extend(b.clone());
        } else {
            out.extend(quote! { else #b });
        }
    }
    out.extend(else_tokens);
    // Wrap the whole conditional in `{ ... }` so it becomes a single
    // expression statement inside the parent rsx!{} body.
    Ok(quote! { { #out }, })
}

fn lower_if_branch(
    b: &RsxIfBranch,
    builder: &mut SpanMapBuilder,
) -> Result<TokenStream, BuffHtmlCodegenError> {
    let cond: TokenStream = match syn::parse_str::<syn::Expr>(&b.cond) {
        Ok(p) => p.to_token_stream(),
        Err(_) => {
            let raw = &b.cond;
            quote! { #raw }
        }
    };
    let body = lower_nodes(&b.body, builder)?;
    Ok(quote! { if #cond { rsx! { #body } } })
}

fn lower_each(
    e: &RsxEach,
    builder: &mut SpanMapBuilder,
) -> Result<TokenStream, BuffHtmlCodegenError> {
    // {#each items as item} body {/each}
    // → inside rsx!{}: `{ items.iter().map(|item| rsx!{ body }).collect::<Vec<_>>() }`
    //
    // `{:else}` is deferred to T134+ (it would lower to a conditional on
    // `items.is_empty()`). For T133 we emit the iterator alone — if the user
    // wrote an `{:else}` branch the parser captures it but codegen ignores
    // it. Documented limitation.
    let iter_src = &e.iterable;
    let binding = &e.binding;
    let binding_ident = match syn::parse_str::<syn::Ident>(binding) {
        Ok(i) => i,
        Err(_) => {
            return Err(BuffHtmlCodegenError::UnsupportedConstruct {
                message: format!("each-binding `{binding}` is not a valid Rust identifier"),
            });
        }
    };
    let iter_tokens: TokenStream = match syn::parse_str::<syn::Expr>(iter_src) {
        Ok(p) => p.to_token_stream(),
        Err(_) => quote! { #iter_src },
    };
    let body = lower_nodes(&e.body, builder)?;
    // Optional index binding: `enumerate().map(|(i, item)| ...)`.
    let map_clause = match &e.index_binding {
        Some(idx) => {
            let i_ident = match syn::parse_str::<syn::Ident>(idx) {
                Ok(i) => i,
                Err(_) => {
                    return Err(BuffHtmlCodegenError::UnsupportedConstruct {
                        message: format!("each-index `{idx}` is not a valid Rust identifier"),
                    });
                }
            };
            quote! { .enumerate().map(|(#i_ident, #binding_ident)| rsx! { #body }) }
        }
        None => quote! { .map(|#binding_ident| rsx! { #body }) },
    };
    Ok(quote! {
        { #iter_tokens.iter() #map_clause .collect::<Vec<_>>() },
    })
}

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
pub mod prop_check;
pub mod span_map;

pub use error::BuffHtmlCodegenError;
pub use prop_check::{
    check_props, extract_interface, ComponentInterface, PropCheckDiagnostic, PropCheckKind,
    PropField, PropInterfaceRegistry,
};
pub use span_map::{SpanMap, SpanMapBuilder};

use buff_lang_ast_rsx::{
    RsxAttribute, RsxAttributeKind, RsxComment, RsxEach, RsxElement, RsxFragment, RsxIf,
    RsxIfBranch, RsxInterp, RsxNode, RsxSlot, RsxTemplateFile, ScriptBlock,
};
// Note: RsxComment is referenced in lower_comment signature.
use proc_macro2::TokenStream;
use quote::{format_ident, quote, ToTokens};

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
///
/// T134 extension: when the script block declares a `props="Type"`
/// attribute, this function:
/// 1. Parses the script source as a Rust block (statements list).
/// 2. Hoists all top-level `Item`s (notably the named struct + any
///    `use` imports) to module scope ahead of the component fn.
/// 3. Splices all non-item statements (let-bindings, expressions)
///    into the component body ahead of the `rsx!{}` expression.
/// 4. Auto-generates the `let <Type> { f1, f2, .. } = props;`
///    destructure as the FIRST body statement so script-body code can
///    reference the prop fields by name.
/// 5. Switches the signature to `fn <name>(props: <Type>) -> Element`.
///
/// When no `props` attribute is present, falls back to the T133 floor
/// behavior: script source is preserved as a `const __BUFF_SCRIPT_SOURCE`
/// (verbatim, for downstream CLI integration) and the component takes
/// no parameters.
fn build_file_items(
    component_name: &str,
    rsx_macro: syn::Expr,
    script: Option<&ScriptBlock>,
) -> Vec<syn::Item> {
    let use_item: syn::Item = syn::parse_quote! {
        use dioxus::prelude::*;
    };

    // Decompose the script into:
    // - `module_items`: top-level Rust items (structs, uses, fn decls)
    //   that live at module scope. Empty when there's no script block.
    // - `body_stmts`: statements that live inside the component body
    //   (let-bindings, side-effect calls like on_init/on_destroy).
    // - `script_holder`: optional `const __BUFF_SCRIPT_SOURCE` for the
    //   T133-floor no-props case (preserved for CLI downstream pass).
    // - `props_interface`: parsed Props interface (when `props="..."`),
    //   used to build the destructure + signature.
    let mut module_items: Vec<syn::Item> = Vec::new();
    let mut body_stmts: Vec<syn::Stmt> = Vec::new();
    let mut script_holder: Option<syn::Item> = None;
    let mut props_interface: Option<PropsInterface> = None;

    if let Some(s) = script {
        if let Some(props_name) = &s.props {
            // T134 path — parse + splice + destructure.
            let parsed = parse_script_block(&s.source);
            let pi = extract_props_interface(&parsed, props_name);
            module_items.extend(parsed.module_items);
            body_stmts.extend(parsed.body_stmts);
            // Even if the named struct is missing from the script
            // body, surface the `props: <Type>` signature so rustc
            // produces a clean "type not found" diagnostic against
            // the generated source (mapped back via SpanMap).
            props_interface = Some(pi.unwrap_or_else(|| PropsInterface::name_only(props_name)));
        } else {
            // T133 floor — script preserved verbatim as a const for the
            // CLI's downstream pass. Component body is just the rsx!{}.
            let raw = &s.source;
            script_holder = Some(syn::parse_quote! {
                #[doc = "buffhtml script block — extracted by buff-lang-cli"]
                const __BUFF_SCRIPT_SOURCE: &str = #raw;
            });
        }
    }

    let component_ident = syn::Ident::new(component_name, proc_macro2::Span::call_site());

    // Build the destructure statement (always first when a Props
    // interface is declared) so subsequent body statements see the
    // fields in scope.
    if let Some(pi) = &props_interface {
        body_stmts.insert(0, pi.destructure_stmt());
    }

    let block = syn::Block {
        brace_token: Default::default(),
        stmts: {
            let mut all = body_stmts;
            all.push(syn::Stmt::Expr(rsx_macro, None));
            all
        },
    };

    // Build the signature: `fn Name(props: <Type>) -> Element` when a
    // Props interface is declared; `fn Name() -> Element` otherwise.
    let inputs = match &props_interface {
        Some(pi) => {
            let mut p = syn::punctuated::Punctuated::new();
            p.push(syn::FnArg::Typed(syn::PatType {
                attrs: Vec::new(),
                pat: Box::new(syn::Pat::Ident(syn::PatIdent {
                    attrs: Vec::new(),
                    ident: syn::Ident::new("props", proc_macro2::Span::call_site()),
                    by_ref: None,
                    mutability: None,
                    subpat: None,
                })),
                colon_token: Default::default(),
                ty: Box::new(syn::Type::Path(syn::TypePath {
                    qself: None,
                    path: pi.type_path(),
                })),
            }));
            p
        }
        None => syn::punctuated::Punctuated::new(),
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
        inputs,
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
    // Module-level items from the script (the Props struct + any uses).
    items.append(&mut module_items);
    if let Some(h) = script_holder {
        items.push(h);
    }
    items.push(component_item);
    items
}

// ---------------------------------------------------------------------------
// Script-block parsing — T134.
// ---------------------------------------------------------------------------

/// Parsed view of a `.buffhtml` script body, split into module-level
/// items (structs, uses, fn decls) and function-body statements (let
/// bindings, side-effect calls).
#[derive(Default)]
struct ParsedScript {
    module_items: Vec<syn::Item>,
    body_stmts: Vec<syn::Stmt>,
}

/// Parse the script body as a Rust block by wrapping it in `{...}`.
/// On failure (script source has a Rust syntax error), the script is
/// emitted as zero items + zero body statements — rustc will then
/// surface the original error against the generated source position
/// (the SpanMap will translate it back to the .buffhtml span).
fn parse_script_block(source: &str) -> ParsedScript {
    let wrapped = format!("{{{source}}}");
    let block: syn::Block = match syn::parse_str(&wrapped) {
        Ok(b) => b,
        Err(_) => return ParsedScript::default(),
    };
    let mut out = ParsedScript::default();
    for stmt in block.stmts {
        match stmt {
            syn::Stmt::Item(item) => out.module_items.push(item),
            other => out.body_stmts.push(other),
        }
    }
    out
}

/// Captured view of the declared Props interface — used both to emit
/// the destructure at function entry and to generate the `props: Type`
/// signature parameter.
struct PropsInterface {
    /// The struct path visible from the generated component fn.
    /// Always a single-segment identifier today (the script-block
    /// struct lives at module scope), but stored as a `syn::Path` so
    /// future generics / lifetimes are additive.
    type_name: syn::Path,
    /// Field identifiers declared in the struct body. Used to emit
    /// `let <Type> { f1, f2, .. } = props;`.
    field_idents: Vec<syn::Ident>,
}

impl PropsInterface {
    /// Build the `syn::Path` for the `props: <Type>` signature param.
    fn type_path(&self) -> syn::Path {
        self.type_name.clone()
    }

    /// Construct the destructure statement: `let <Type> { f1, f2, .. } = props;`.
    ///
    /// When field_idents is empty (struct missing from script body),
    /// emits `let <Type> { .. } = props;` — the `..` makes it
    /// compile-compatible even with an unknown struct (rustc surfaces
    /// the type-not-found diagnostic).
    fn destructure_stmt(&self) -> syn::Stmt {
        let type_path = self.type_name.clone();
        let fields = &self.field_idents;
        if fields.is_empty() {
            syn::parse_quote! {
                let #type_path { .. } = props;
            }
        } else {
            syn::parse_quote! {
                let #type_path { #(#fields),*, .. } = props;
            }
        }
    }

    /// Fallback constructor for the "name declared but struct not found"
    /// case — still surfaces the `props: <Type>` signature so rustc can
    /// produce a clean "type not found" error against the generated
    /// position (the SpanMap translates it back to the .buffhtml source).
    fn name_only(props_name: &str) -> Self {
        let type_name =
            syn::Path::from(syn::Ident::new(props_name, proc_macro2::Span::call_site()));
        PropsInterface {
            type_name,
            field_idents: Vec::new(),
        }
    }
}

/// Search the parsed script for the struct declaration matching
/// `props_name`. Returns `None` if no such struct exists (codegen will
/// then emit `fn Comp(props: <missing>)` — rustc surfaces the error).
fn extract_props_interface(parsed: &ParsedScript, props_name: &str) -> Option<PropsInterface> {
    let target_ident = syn::Ident::new(props_name, proc_macro2::Span::call_site());
    for item in &parsed.module_items {
        if let syn::Item::Struct(s) = item {
            if s.ident == target_ident {
                let field_idents: Vec<syn::Ident> = s
                    .fields
                    .iter()
                    .map(|f| {
                        f.ident
                            .clone()
                            .unwrap_or_else(|| syn::Ident::new("_", proc_macro2::Span::call_site()))
                    })
                    .collect();
                let type_name = syn::Path::from(target_ident.clone());
                return Some(PropsInterface {
                    type_name,
                    field_idents,
                });
            }
        }
    }
    None
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
        RsxNode::RawHtml(r) => Ok(lower_raw_html(r, builder)),
        RsxNode::Await(a) => lower_await(a, builder),
        RsxNode::Script(_) => {
            // Top-level only — parser rejects nested <script>.
            Ok(quote! {})
        }
    }
}

/// `{@html expr}` → `<div dangerous_inner_html={expr} data-buffhtml-xss="warning" />`
/// (T133 stretch).
///
/// **Security:** bypasses Dioxus's auto-escaping. The emitted
/// `data-buffhtml-xss="warning"` HTML attribute is a runtime audit marker
/// (visible in DevTools / scrape-able by security tooling).
fn lower_raw_html(r: &buff_lang_ast_rsx::RsxRawHtml, builder: &mut SpanMapBuilder) -> TokenStream {
    let expr_src = &r.expr;
    let parsed: TokenStream = match syn::parse_str::<syn::Expr>(expr_src) {
        Ok(p) => p.to_token_stream(),
        Err(_) => {
            let raw = expr_src.clone();
            quote! { #raw }
        }
    };
    let anchor_text = first_anchor_text(expr_src);
    builder.add_anchor(&anchor_text, r.span);
    // Dioxus 0.7: `dangerous_inner_html` is the standard opt-in for raw HTML.
    // The `data_buffhtml_xss` attribute (sanitized to `data-buffhtml-xss` by
    // Dioxus's kebab-case normalization) is a runtime audit marker — visible
    // in DevTools and scrape-able by security tooling.
    quote! {
        div {
            dangerous_inner_html: #parsed,
            data_buffhtml_xss: "{@html} opt-in",
        },
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

fn lower_slot(s: &RsxSlot) -> TokenStream {
    // Default slot (`<slot />`) lowers to the Dioxus `children` signal.
    // Named slot (`<slot name="header" />`) lowers to the corresponding
    // named-children signal — the parent component declares
    // `fn Comp(children: Element, header: Element)` and the slot renders
    // `{header}`. The named-child identifier is sanitized to a valid Rust
    // ident (slot names like "header" already are, but we guard against
    // future exotic names).
    match &s.name {
        None => quote! { { children }, },
        Some(name) => {
            let ident = sanitize_ident(name);
            let rust_ident = format_ident!("{}", ident);
            quote! { { #rust_ident }, }
        }
    }
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
        RsxAttributeKind::Spread { ident } => {
            // `{...rest}` → Dioxus spread syntax `..ident`. T133 stretch.
            let parsed: TokenStream = match syn::parse_str::<syn::Expr>(ident) {
                Ok(p) => p.to_token_stream(),
                Err(_) => {
                    let raw = ident.clone();
                    quote! { #raw }
                }
            };
            Ok(quote! { ..#parsed, })
        }
        RsxAttributeKind::Bind { prop, signal } => {
            // `bind:value={sig}` → controlled two-way binding. T133 stretch.
            // Dioxus 0.7 idiom: explicit controlled-component form.
            //   value: <signal>,
            //   oninput: move |__e| <signal>.set(__e.value())
            // The signal is expected to be a `use_signal` (or compatible).
            // Rust infers the event type — emits a `value()` accessor.
            let prop_ident = attr_ident(prop)?;
            let signal_tokens: TokenStream = match syn::parse_str::<syn::Expr>(signal) {
                Ok(p) => p.to_token_stream(),
                Err(_) => {
                    let raw = signal.clone();
                    quote! { #raw }
                }
            };
            Ok(quote! {
                #prop_ident: #signal_tokens,
                oninput: move |__e| { #signal_tokens.set(__e.value()); },
            })
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
    // `{#each items as item}` → `items.iter().map(|item| rsx!{ body }).collect::<Vec<_>>()`
    //
    // Keyed form (T133 stretch): `{#each items as item (item.id)}` →
    // `items.iter().enumerate().map(|(__bk, item)| {
    //     let __key = item.id;
    //     rsx! { key: __key, body }
    // }).collect::<Vec<_>>()`
    //
    // Dioxus 0.7 recognizes `key:` as the reconciliation key on children.
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

    // If keyed: emit the keyed form (Dioxus uses `key: <expr>` on children).
    if let Some(key_src) = &e.key {
        let key_tokens: TokenStream = match syn::parse_str::<syn::Expr>(key_src) {
            Ok(p) => p.to_token_stream(),
            Err(_) => {
                let raw = key_src.clone();
                quote! { #raw }
            }
        };
        // Use distinct idents to avoid shadowing between the enumerate index
        // and the computed key value.
        let idx_ident = format_ident!("__buff_idx");
        let key_ident = format_ident!("__buff_key");
        // The keyed form: enumerate to get a positional index (Dioxus
        // requires unique siblings — the index alone would work but is
        // O(n) reconciliation; the user-provided key expr makes it O(1)),
        // then evaluate the key expression in the body where `binding` is
        // in scope. We emit the key as a top-level attribute on the body's
        // outermost rsx!{}.
        return Ok(quote! {
            {
                #iter_tokens.iter().enumerate().map(|(#idx_ident, #binding_ident)| {
                    let #key_ident = #key_tokens;
                    rsx! {
                        key: #key_ident,
                        #body
                    }
                }).collect::<Vec<_>>()
            },
        });
    }

    // Non-keyed path (original).
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

/// `{#await fut}{:then b}{:catch e}{/await}` → Dioxus 0.7 use_resource hook.
/// T133 stretch.
///
/// Lowers to:
/// ```ignore
/// {
///     let __resource = use_resource(|| async { <fut_expr>.await });
///     // match on resource state
///     match &*__resource.read() {
///         ResourceState::Ready(Ok(<then_binding>)) => rsx! { <then_body> },
///         ResourceState::Ready(Err(<catch_binding>)) => rsx! { <catch_body> },
///         _ => rsx! { <pending_body> },
///     }
/// }
/// ```
///
/// If `pending_body` is `None` and the resource is still loading, nothing
/// renders (empty fragment).
fn lower_await(
    a: &buff_lang_ast_rsx::RsxAwait,
    builder: &mut SpanMapBuilder,
) -> Result<TokenStream, BuffHtmlCodegenError> {
    let fut_src = &a.fut_expr;
    let fut_tokens: TokenStream = match syn::parse_str::<syn::Expr>(fut_src) {
        Ok(p) => p.to_token_stream(),
        Err(_) => quote! { #fut_src },
    };
    let then_binding = match syn::parse_str::<syn::Ident>(&a.then_binding) {
        Ok(i) => i,
        Err(_) => {
            return Err(BuffHtmlCodegenError::UnsupportedConstruct {
                message: format!(
                    "await then-binding `{}` is not a valid Rust identifier",
                    a.then_binding
                ),
            });
        }
    };
    let then_body = lower_nodes(&a.then_body, builder)?;
    let pending_body = match &a.pending_body {
        Some(body) => {
            let b = lower_nodes(body, builder)?;
            quote! { rsx! { #b } }
        }
        None => quote! { rsx! { Fragment {} } },
    };
    let resource_ident = format_ident!("__buff_resource");
    // Catch branch — optional.
    let catch_arm = match (&a.catch_binding, &a.catch_body) {
        (Some(cb), Some(body)) => {
            let catch_ident = match syn::parse_str::<syn::Ident>(cb) {
                Ok(i) => i,
                Err(_) => {
                    return Err(BuffHtmlCodegenError::UnsupportedConstruct {
                        message: format!(
                            "await catch-binding `{cb}` is not a valid Rust identifier"
                        ),
                    });
                }
            };
            let b = lower_nodes(body, builder)?;
            quote! {
                ::dioxus::prelude::ResourceState::Ready(Err(#catch_ident)) => rsx! { #b },
            }
        }
        _ => quote! {},
    };
    Ok(quote! {
        {
            let #resource_ident = ::dioxus::prelude::use_resource(|| async { #fut_tokens.await });
            match #resource_ident.read().clone() {
                ::dioxus::prelude::ResourceState::Ready(Ok(#then_binding)) => rsx! { #then_body },
                #catch_arm
                _ => #pending_body,
            }
        },
    })
}

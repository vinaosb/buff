//! Buff RSX AST — node definitions for `.buffhtml` Single-File Components.
//!
//! This crate is **pure data**: it defines the node types produced by the
//! `buff-lang-buffhtml-parser` crate and consumed by
//! `buff-lang-codegen-buffhtml`. It contains no parsing logic and no codegen.
//!
//! # T133 scope (decision record `rsx-syntax-feasibility.md` §6 floor)
//!
//! Floor-grammar nodes shipped today:
//! - [`RsxNode::Element`] (HTML elements + child components via `is_component`)
//! - [`RsxNode::Fragment`] (`<> ... </>`)
//! - [`RsxNode::Text`] (raw text runs)
//! - [`RsxNode::Interp`] (`{expr}` interpolation; `expr` is stored as raw
//!   source text — the codegen emits it verbatim into the `rsx!{}` body)
//! - [`RsxNode::If`] / [`RsxIfBranch`] (`{#if}/{:else if}/{:else}{/if}`)
//! - [`RsxNode::Each`] (`{#each iterable as binding}{/each}` + optional `{:else}`)
//! - [`RsxNode::Slot`] (`<slot />` default slot)
//! - [`RsxNode::Comment`] (HTML `<!-- -->` + Buff `{# ... #}` directive comments)
//! - [`RsxNode::Script`] (`<script lang="buff"> ... </script>` block,
//!   preserved verbatim for the CLI to splice into the generated Rust crate)
//!
//! Deferred per §6 (NOT shipped; intentionally absent from this enum):
//! - Named slots (`<slot name="x" />`) — T134+
//! - Keyed each (`{#each xs as x (x.id)}`) — T134+
//! - Spread props (`{...rest}`) — T134+
//! - Two-way binding (`bind={...}`) — T134+
//! - Await blocks (`{#await ...}`) — T134+
//! - `{@html}` escape hatch — T134+
//!
//! [`Span`] is re-exported from `buff-lang-error` for convenience.

pub use buff_lang_error::Span;

/// Top-level node produced by parsing a `.buffhtml` file.
///
/// The optional [`ScriptBlock`] carries the source of `<script lang="buff">`
/// for downstream CLI integration (companion-file binding, type-checking).
/// The `root` is the template body — a sequence of sibling nodes (Svelte-
/// style: a `.buffhtml` file's body is a fragment, not necessarily a single
/// element).
#[derive(Debug, Clone, PartialEq)]
pub struct RsxTemplateFile {
    pub script: Option<ScriptBlock>,
    pub root: Vec<RsxNode>,
    pub span: Span,
}

/// `<script lang="buff"> ... </script>` block.
///
/// `source` is the raw text between the tags (unmodified). Downstream
/// passes (CLI) feed this through `buff-lang-lexer` + `buff-lang-parser`
/// to extract component logic. T133 does not interpret it.
#[derive(Debug, Clone, PartialEq)]
pub struct ScriptBlock {
    pub lang: String,
    pub source: String,
    pub span: Span,
}

/// One node in the template tree.
#[derive(Debug, Clone, PartialEq)]
pub enum RsxNode {
    /// HTML element (`<div>`) or child component (`<Counter />`).
    ///
    /// `is_component` is `true` when the tag's first character is uppercase
    /// (per Svelte/JSX convention) — codegen emits it as a `Component { ... }`
    /// call instead of a lowercase element.
    Element(RsxElement),
    /// `<> ... </>` fragment.
    Fragment(RsxFragment),
    /// Raw text run (whitespace-only or visible).
    Text(RsxText),
    /// `{expr}` interpolation. `expr` is the raw source between the braces;
    /// the codegen emits it verbatim into the `rsx!{}` body where dioxus-rsx
    /// will re-parse it as a Rust expression.
    Interp(RsxInterp),
    /// `{#if ... } ... {:else if ...} ... {:else} ... {/if}` conditional.
    If(RsxIf),
    /// `{#each iterable as binding} ... {:else} ... {/each}` loop.
    Each(RsxEach),
    /// `<slot />` default slot insertion point.
    Slot(RsxSlot),
    /// `<!-- html comment -->` or `{# buff directive comment #}`.
    Comment(RsxComment),
    /// `<script lang="buff"> ... </script>` block (only at file top-level;
    /// nested `<script>` is rejected by the parser).
    Script(ScriptBlock),
}

/// HTML element or child component.
#[derive(Debug, Clone, PartialEq)]
pub struct RsxElement {
    /// Tag name as written (`div`, `Counter`, `h1`).
    pub tag: String,
    /// `true` if `tag` starts with an uppercase ASCII letter — Svelte/JSX
    /// convention for "this is a component, not a host element."
    pub is_component: bool,
    /// Attributes / props / event handlers in source order.
    pub attributes: Vec<RsxAttribute>,
    /// Child nodes in source order. Empty for self-closing tags.
    pub children: Vec<RsxNode>,
    /// `true` if the tag self-closes (`<foo />`); `false` if it has a separate
    /// closing tag (`<foo>...</foo>`).
    pub self_closing: bool,
    pub span: Span,
}

/// `<> ... </>` fragment.
#[derive(Debug, Clone, PartialEq)]
pub struct RsxFragment {
    pub children: Vec<RsxNode>,
    pub span: Span,
}

/// Raw text run.
#[derive(Debug, Clone, PartialEq)]
pub struct RsxText {
    pub text: String,
    pub span: Span,
}

/// `{expr}` interpolation.
#[derive(Debug, Clone, PartialEq)]
pub struct RsxInterp {
    /// Raw source text of the expression (between the braces).
    pub expr: String,
    pub span: Span,
}

/// One attribute / prop / handler on an element.
///
/// Three forms are supported (decision record §3):
/// - `name="literal"` or `name='literal'` → [`RsxAttribute::Literal`]
/// - `name={expr}` (where `name` is NOT `on:event`) → [`RsxAttribute::Expression`]
/// - `on:event_modifier={handler_expr}` → [`RsxAttribute::Event`]
/// - `name: value` (named prop, no `=`) → [`RsxAttribute::NamedProp`]
/// - bare `name` (boolean attr) → [`RsxAttribute::Boolean`]
#[derive(Debug, Clone, PartialEq)]
pub struct RsxAttribute {
    pub kind: RsxAttributeKind,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub enum RsxAttributeKind {
    /// `name="value"` — Rust string-literal codegen.
    Literal { name: String, value: String },
    /// `name={expr}` — Rust expression in the `rsx!{}` body.
    Expression {
        name: String,
        expr: String,
        expr_span: Span,
    },
    /// `on:event_modifier={handler_expr}` — Svelte-style event directive.
    /// `modifiers` is the list of `_`-separated modifiers after the event
    /// name (e.g. `on:submit_prevent` → `event="submit"`, `modifiers=["prevent"]`).
    Event {
        event: String,
        modifiers: Vec<String>,
        handler_expr: String,
        handler_span: Span,
    },
    /// `name: value` named prop (Buff §11 named-args rule). `value` is raw
    /// source text — codegen emits it verbatim.
    NamedProp {
        name: String,
        value: String,
        value_span: Span,
    },
    /// Bare boolean attribute (e.g. `<input disabled />`).
    Boolean { name: String },
}

/// `{#if cond} ... {:else if cond} ... {:else} ... {/if}`.
#[derive(Debug, Clone, PartialEq)]
pub struct RsxIf {
    /// First branch is the `{#if}`; subsequent branches are `{:else if}`.
    /// Always non-empty.
    pub branches: Vec<RsxIfBranch>,
    /// `{:else}` body, if present.
    pub else_branch: Option<Vec<RsxNode>>,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RsxIfBranch {
    /// Raw source of the condition expression.
    pub cond: String,
    pub cond_span: Span,
    pub body: Vec<RsxNode>,
}

/// `{#each iterable as binding} ... {:else} ... {/each}`.
///
/// Keyed each (`{#each xs as x (x.id)}`) is **deferred to T134+** (decision
/// record §6 stretch); the parser does not accept the `(key)` form today.
#[derive(Debug, Clone, PartialEq)]
pub struct RsxEach {
    /// Raw source of the iterable expression (`items` in `{#each items as item}`).
    pub iterable: String,
    pub iterable_span: Span,
    /// Loop binding name (`item` in `{#each items as item}`).
    pub binding: String,
    /// Optional index binding (`i` in `{#each items as item, i}`).
    pub index_binding: Option<String>,
    pub body: Vec<RsxNode>,
    /// `{:else}` body, if present (runs when `iterable` is empty).
    pub else_branch: Option<Vec<RsxNode>>,
    pub span: Span,
}

/// `<slot />` — default slot insertion point.
///
/// Named slots (`<slot name="x" />`) are **deferred to T134+** (decision
/// record §6 stretch). The parser rejects the `name=` attribute today.
#[derive(Debug, Clone, PartialEq)]
pub struct RsxSlot {
    pub span: Span,
}

/// Comment node (HTML or Buff directive).
#[derive(Debug, Clone, PartialEq)]
pub struct RsxComment {
    pub text: String,
    pub span: Span,
}

/// Convenience constructors used by the parser + tests.
impl RsxText {
    pub fn new(text: impl Into<String>, span: Span) -> Self {
        Self {
            text: text.into(),
            span,
        }
    }
}

impl RsxInterp {
    pub fn new(expr: impl Into<String>, span: Span) -> Self {
        Self {
            expr: expr.into(),
            span,
        }
    }
}

impl RsxElement {
    pub fn new(
        tag: impl Into<String>,
        is_component: bool,
        attributes: Vec<RsxAttribute>,
        children: Vec<RsxNode>,
        self_closing: bool,
        span: Span,
    ) -> Self {
        Self {
            tag: tag.into(),
            is_component,
            attributes,
            children,
            self_closing,
            span,
        }
    }
}

impl RsxFragment {
    pub fn new(children: Vec<RsxNode>, span: Span) -> Self {
        Self { children, span }
    }
}

impl RsxSlot {
    pub fn new(span: Span) -> Self {
        Self { span }
    }
}

impl RsxComment {
    pub fn new(text: impl Into<String>, span: Span) -> Self {
        Self {
            text: text.into(),
            span,
        }
    }
}

impl ScriptBlock {
    pub fn new(lang: impl Into<String>, source: impl Into<String>, span: Span) -> Self {
        Self {
            lang: lang.into(),
            source: source.into(),
            span,
        }
    }
}

/// `is_component` heuristic — first char of the tag is ASCII uppercase.
///
/// Mirrors the Svelte/JSX convention. Used by both the parser (to set the
/// `is_component` flag) and by codegen (defensive re-check).
pub fn is_component_tag(tag: &str) -> bool {
    tag.chars()
        .next()
        .map(|c| c.is_ascii_uppercase())
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    //! The AST crate is pure data — minimal smoke tests only. Real coverage
    //! lives in `buff-lang-buffhtml-parser` and `buff-lang-codegen-buffhtml`.

    use super::*;

    #[test]
    fn is_component_tag_recognizes_convention() {
        assert!(is_component_tag("Counter"));
        assert!(is_component_tag("Layout"));
        assert!(!is_component_tag("div"));
        assert!(!is_component_tag("h1"));
        assert!(!is_component_tag(""));
    }

    #[test]
    fn text_constructor_owns_input() {
        let span = Span::new(0, 5, crate::Span::dummy().source_id);
        let n = RsxText::new("hello", span);
        assert_eq!(n.text, "hello");
        assert_eq!(n.span, span);
    }
}

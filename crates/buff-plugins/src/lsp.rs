//! LSP plugin trait + supporting types.
//!
//! An [`LspPlugin`] hooks into two LSP handlers in `buff-lsp`:
//!
//! 1. **Code actions** — the `textDocument/codeAction` handler
//!    surfaces refactor suggestions / quick-fixes at the cursor.
//! 2. **Hover** — the `textDocument/hover` handler surfaces
//!    documentation + type info at the cursor.
//!
//! Both hooks are object-safe and dispatched via `&dyn LspPlugin` so
//! the [`PluginRegistry`](crate::PluginRegistry) can hold a
//! `Vec<Box<dyn LspPlugin>>` and fan-out a call to every registered
//! plugin in declaration order. Results are concatenated (code
//! actions) or first-wins (hover — the first plugin to return
//! `Some` wins; later plugins are not consulted).
//!
//! # Why not reuse `lsp_types`?
//!
//! The `lsp-types` crate is the canonical Rust mapping for the LSP
//! JSON schema — but it is a heavy dep for plugin authors (pulls in
//! `url`, `serde_json`, etc.). The plugin-local types here
//! ([`PluginPosition`], [`PluginCodeAction`], [`PluginHover`]) are
//! minimal value-types that the `buff-lsp` host converts into
//! `lsp_types::*` at the dispatch boundary. This keeps the
//! `buff-plugins` crate dependency-light and lets plugin authors
//! write plugins without learning the `lsp-types` surface.

use crate::error::Result;

/// A 0-based UTF-16 LSP position (line + character).
///
/// Mirrors `lsp_types::Position`'s wire shape but is owned + cheap
/// to construct in tests. The host (buff-lsp) converts these into
/// `lsp_types::Position` at the dispatch boundary.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct PluginPosition {
    /// 0-based line number.
    pub line: u32,
    /// 0-based UTF-16 character offset within the line.
    pub character: u32,
}

impl PluginPosition {
    /// Construct a position from `line` + `character` (both 0-based).
    pub fn new(line: u32, character: u32) -> Self {
        Self { line, character }
    }
}

/// A code action surfaced by an LSP plugin.
///
/// Mirrors the essential fields of `lsp_types::CodeAction` (title +
/// kind). The host (buff-lsp) wraps this in the full LSP envelope
/// at the dispatch boundary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluginCodeAction {
    /// Human-readable title shown in the quick-fix menu.
    pub title: String,
    /// Optional LSP kind string (e.g. `"quickfix"`,
    /// `"refactor.extract"`). When `None`, the host defaults to
    /// `"quickfix"`.
    pub kind: Option<String>,
}

impl PluginCodeAction {
    /// Construct a code action with a title and the default
    /// `"quickfix"` kind.
    pub fn new(title: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            kind: None,
        }
    }

    /// Attach a specific LSP kind (e.g. `"refactor.extract"`).
    pub fn with_kind(mut self, kind: impl Into<String>) -> Self {
        self.kind = Some(kind.into());
        self
    }
}

/// A hover response surfaced by an LSP plugin.
///
/// Mirrors the essential fields of `lsp_types::Hover` (markdown
/// content). The host (buff-lsp) wraps this in the full LSP
/// envelope at the dispatch boundary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluginHover {
    /// Markdown content to display in the hover popup.
    pub content: String,
}

impl PluginHover {
    /// Construct a hover response from markdown content.
    pub fn new(content: impl Into<String>) -> Self {
        Self {
            content: content.into(),
        }
    }
}

/// The LSP plugin trait.
///
/// Object-safe + `Send + Sync` so the registry can hold a
/// `Vec<Box<dyn LspPlugin>>`.
///
/// # Default methods
///
/// Both `code_actions` and `hover` have default no-op
/// implementations so a plugin author can implement only the hook
/// they care about (e.g. a hover-only plugin skips code actions).
pub trait LspPlugin: Send + Sync {
    /// Human-readable name. Used in tracing logs.
    fn name(&self) -> &str;

    /// Surface code actions at the cursor. Returns a list of
    /// actions (possibly empty) — the host concatenates results
    /// from all registered plugins.
    ///
    /// Default: no actions.
    fn code_actions(&self, _uri: &str, _cursor: PluginPosition) -> Vec<PluginCodeAction> {
        Vec::new()
    }

    /// Surface hover info at the cursor. Returns `Some` to provide
    /// hover content, `None` to defer to the next plugin (or the
    /// built-in buff-lsp hover handler).
    ///
    /// Default: `Ok(None)` (defer to the built-in handler).
    fn hover(&self, _uri: &str, _cursor: PluginPosition) -> Result<Option<PluginHover>> {
        Ok(None)
    }
}

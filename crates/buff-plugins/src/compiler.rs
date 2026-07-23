//! Compiler plugin trait + supporting types.
//!
//! A [`CompilerPlugin`] hooks into two places in the compile pipeline:
//!
//! 1. **Lint pass** — runs over the parsed AST BEFORE codegen. Used
//!    to surface project-specific warnings/errors (e.g. reject
//!    `todo!()` / `unwrap()` calls, enforce naming conventions
//!    stricter than the built-in `buff check` linter).
//! 2. **Codegen pass** — runs over the generated `syn::File` AFTER
//!    codegen but BEFORE `prettyplease::unparse` + rustc. Used to
//!    transform the Rust output (e.g. inject `#[inline(always)]`,
//!    strip `derive(Debug)` from release builds, swap trait
//!    implementations).
//!
//! Both hooks are object-safe and dispatched via `&dyn
//! CompilerPlugin` so the [`PluginRegistry`](crate::PluginRegistry)
//! can hold a `Vec<Box<dyn CompilerPlugin>>` and fan-out a call to
//! every registered plugin in declaration order.
//!
//! # Why AST-shaped inputs?
//!
//! `run_lint` takes `&[buff_lang_ast::Decl]` (the SAME AST the
//! parser produces) so a plugin never re-parses the source — it
//! walks the real parsed tree. `run_codegen_pass` takes a `&mut
//! syn::File` for the same reason: the Rust output is already
//! materialised, the plugin mutates it in place.

use buff_lang_ast::Decl;
use buff_lang_error::Span;

use crate::error::Result;

/// A single warning emitted by a compiler plugin's lint pass.
///
/// Stored as a struct (rather than reusing `buff_lang_error::Diagnostic`)
/// so plugins can be authored without pulling in the entire
/// `buff-lang-error` Diagnostic type — only `Span` (a leaf type) is
/// required. The host (e.g. `buff check`) converts these into
/// `Diagnostic` instances at the dispatch boundary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LintWarning {
    /// Human-readable message describing the violation. Surfaced
    /// verbatim to the user.
    pub message: String,
    /// Source span of the offending construct. Used by the host to
    /// render a caret-style diagnostic (rustc-style).
    pub span: Span,
    /// Optional plugin-supplied code (e.g. `"BUFF001"`,
    /// `"no-todo"`). When `None`, the host falls back to the
    /// plugin's name.
    pub code: Option<String>,
}

impl LintWarning {
    /// Construct a `LintWarning` with no code (host falls back to
    /// the plugin name).
    pub fn new(message: impl Into<String>, span: Span) -> Self {
        Self {
            message: message.into(),
            span,
            code: None,
        }
    }

    /// Attach a diagnostic code (e.g. `"BUFF001"`).
    pub fn with_code(mut self, code: impl Into<String>) -> Self {
        self.code = Some(code.into());
        self
    }
}

/// The compiler plugin trait.
///
/// Object-safe (no generics, no `Self` by value) so the
/// [`PluginRegistry`](crate::PluginRegistry) can hold a
/// `Vec<Box<dyn CompilerPlugin>>` and dispatch via virtual call.
///
/// `Send + Sync` is required so the registry can be shared across
/// threads (matches the project-wide `Send + Sync` rule for all
/// public types — FFI guide R4).
///
/// # Default methods
///
/// Both `run_lint` and `run_codegen_pass` have default no-op
/// implementations so a plugin author can implement only the hook
/// they care about (e.g. a pure lint plugin skips codegen).
pub trait CompilerPlugin: Send + Sync {
    /// Human-readable name. Used in error messages + diagnostic
    /// codes when the warning's `code` is `None`.
    fn name(&self) -> &str;

    /// Run the lint pass over the parsed AST. Returns a list of
    /// warnings (the host decides whether they are surfaced as
    /// warnings or promoted to errors via `--deny-warnings`).
    ///
    /// Default: no warnings (the plugin has no lint opinion).
    fn run_lint(&self, _ast: &[Decl]) -> Vec<LintWarning> {
        Vec::new()
    }

    /// Run the codegen pass over the generated `syn::File`. Mutates
    /// the file in place. Returns `Err` only on a fatal failure —
    /// the host aborts the build when this returns `Err`.
    ///
    /// Default: `Ok(())` (the plugin has no codegen opinion).
    fn run_codegen_pass(&self, _rust: &mut syn::File) -> Result<()> {
        Ok(())
    }
}

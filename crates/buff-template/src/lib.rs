//! `buff-template` — HTML templating for the Buff language.
//!
//! Pure-Rust MVP wrapping the [`handlebars`](https://docs.rs/handlebars)
//! crate. Runtime-only (no compile-time macros). Per T19 spec:
//! `Template.from_path(path)`, `Template.from_string(source)`,
//! `template.render(context)` returns String.
//!
//! # Pipeline
//!
//! ```text
//!   Template.from_string(src) ──┐
//!                               ▼
//!   Template.from_path(p) ──────▶ Template { handlebars::Handlebars }
//!                                        │
//!                                        └─ template.render(ctx) → String
//! ```
//!
//! # FFI safety
//!
//! Every public entry point follows the 6 hard rules from
//! `crates/buff-lang-ffi-guide/GUIDE.md`:
//!
//! | Rule | How this crate complies |
//! |------|-------------------------|
//! | R1 — No raw pointers | Public surface exposes only `Template`, `TemplateError`. No `*const` / `*mut` anywhere. |
//! | R2 — Ownership boundary | `from_path` / `from_string` return owned `Template`. `render` returns owned `String`. |
//! | R3 — Error mapping | Every fallible op returns `Result<T, TemplateError>`. handlebars errors mapped via `From`. |
//! | R4 — Thread safety | `Template` is `Send + Sync` (wraps `handlebars::Handlebars` which is itself `Send + Sync`). |
//! | R5 — Lifetime hiding | No public lifetime parameters. `Template` owns its `Handlebars` registry. |
//! | R6 — Panic boundary | `from_path` / `from_string` / `render` wrap their bodies in `catch_unwind` (per FFI guide §6). |
//!
//! # Panic-free contract
//!
//! No `unwrap` / `expect` / `panic!` / `todo!` / `unimplemented!` in
//! non-test code.

pub mod error;

pub use error::TemplateError;

use std::panic::{catch_unwind, AssertUnwindSafe};

/// A compiled HTML template, ready to render with a context.
///
/// Constructed via [`Template::from_string`] (parse a template source
/// string) or [`Template::from_path`] (load a `.html` file from disk).
/// Rendered via [`Template::render`] with a JSON-like context.
///
/// Internally wraps `handlebars::Handlebars<'static>` — a compiled
/// template registry. The template is registered under the name
/// `"__buff_template_main"` and rendered by that name.
#[derive(Debug, Clone)]
pub struct Template {
    inner: handlebars::Handlebars<'static>,
}

impl Template {
    /// Compile a template from a source string.
    ///
    /// The source uses handlebars syntax: `{{ variable }}`,
    /// `{% if cond %}...{% endif %}`, `{% for item in list %}...{% endfor %}`.
    ///
    /// Wraps `handlebars::Handlebars::register_template_string`. The body
    /// is wrapped in `catch_unwind` per T4 FFI guide R6 so a panic in the
    /// parser becomes a stable `Err(TemplateError::Panic)` instead of
    /// process abort.
    pub fn from_string(source: &str) -> Result<Self, TemplateError> {
        let source_owned = source.to_string();
        let result = catch_unwind(AssertUnwindSafe(|| {
            let mut hb = handlebars::Handlebars::new();
            hb.register_template_string("__buff_template_main", &source_owned)
                .map_err(|e| TemplateError::Parse(e.to_string()))?;
            Ok(Template { inner: hb })
        }));
        match result {
            Ok(Ok(t)) => Ok(t),
            Ok(Err(e)) => Err(e),
            Err(_) => Err(TemplateError::Panic),
        }
    }

    /// Load a template from a file path. The file is read as UTF-8 text
    /// and compiled as a handlebars template.
    ///
    /// Wraps `std::fs::read_to_string` + `Template::from_string`. The
    /// body is wrapped in `catch_unwind` per T4 FFI guide R6.
    pub fn from_path<P: AsRef<std::path::Path>>(path: P) -> Result<Self, TemplateError> {
        let path_owned = path.as_ref().to_path_buf();
        let result = catch_unwind(AssertUnwindSafe(|| {
            let source = std::fs::read_to_string(&path_owned)
                .map_err(|e| TemplateError::Parse(format!("failed to read file: {e}")))?;
            Template::from_string(&source)
        }));
        match result {
            Ok(Ok(t)) => Ok(t),
            Ok(Err(e)) => Err(e),
            Err(_) => Err(TemplateError::Panic),
        }
    }

    /// Render the template with the given context.
    ///
    /// The context is a JSON string representing a map of variable names
    /// to values. For example: `{"name": "Buff", "items": ["a", "b"]}`.
    ///
    /// Returns the rendered HTML string. Wraps
    /// `handlebars::Handlebars::render`. The body is wrapped in
    /// `catch_unwind` per T4 FFI guide R6.
    pub fn render(&self, context_json: &str) -> Result<String, TemplateError> {
        let ctx_owned = context_json.to_string();
        let result = catch_unwind(AssertUnwindSafe(|| {
            // Parse the JSON context into a serde_json::Value
            let ctx: serde_json::Value = serde_json::from_str(&ctx_owned)
                .map_err(|e| TemplateError::Render(format!("invalid context JSON: {e}")))?;
            self.inner
                .render("__buff_template_main", &ctx)
                .map_err(|e| TemplateError::Render(e.to_string()))
        }));
        match result {
            Ok(Ok(s)) => Ok(s),
            Ok(Err(e)) => Err(e),
            Err(_) => Err(TemplateError::Panic),
        }
    }
}

impl Default for Template {
    fn default() -> Self {
        // An empty template that renders to empty string.
        // Direct construction to avoid recursion (from_string -> catch_unwind
        // -> Ok(Template { inner: hb }) is fine, but unwrap_or_default would
        // loop).
        let mut hb = handlebars::Handlebars::new();
        let _ = hb.register_template_string("__buff_template_main", "");
        Template { inner: hb }
    }
}

impl std::fmt::Display for Template {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Template(compiled)")
    }
}

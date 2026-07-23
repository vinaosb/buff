//! `buff-plugin.toml` manifest format.
//!
//! A `buff-plugin.toml` file describes ONE plugin. It mirrors the
//! shape of the existing [`buff_lang_cli::config::BuffConfig`]
//! (`buff.toml`) parsing pattern: serde derive + `toml::from_str`,
//! permissive optionals, unknown fields silently ignored for forward
//! compatibility, deterministic ordering via `BTreeMap` for any
//! future collection-typed field.
//!
//! # Layout
//!
//! ```toml
//! name = "my-lint-plugin"
//! version = "0.1.0"
//! kind = "compiler"   # one of: compiler | lsp | runtime
//! entry_point = "my_lint_plugin::NoTodoLint"
//! description = "Rejects `todo!()` / `unwrap()` calls."
//! ```
//!
//! `name`, `version`, `kind`, and `entry_point` are REQUIRED —
//! missing or invalid values surface as
//! [`PluginError::ManifestInvalid`](crate::PluginError::ManifestInvalid).
//! `description` is optional (defaults to the empty string).
//!
//! [`buff_lang_cli::config::BuffConfig`]: https://github.com/buff-lang/buff/blob/master/crates/buff-lang-cli/src/config.rs

use serde::{Deserialize, Serialize};

use crate::error::{PluginError, Result};

/// The kind of plugin — selects which trait the registered plugin
/// object must implement.
///
/// Mirrors the `kind = "compiler"` field in `buff-plugin.toml`.
/// Stored as a separate enum (rather than `String`) so the registry
/// can dispatch via match instead of by-name lookup.
///
/// Serialises as the lowercase name (`"compiler"`, `"lsp"`,
/// `"runtime"`) so the round-trip through TOML is transparent.
/// Unknown values fall back to a deserialise error (forward-compat
/// would hide typos in `kind = "Compiler"`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum PluginKind {
    /// Compiler plugin — implements
    /// [`CompilerPlugin`](crate::CompilerPlugin). Hooks into lint +
    /// codegen passes via the `buff check` / `buff build` pipelines.
    Compiler,
    /// LSP plugin — implements
    /// [`LspPlugin`](crate::LspPlugin). Hooks into code-action +
    /// hover handlers via `buff-lsp`.
    Lsp,
    /// Runtime plugin — implements
    /// [`RuntimePlugin`](crate::RuntimePlugin). Hooks into span +
    /// metric dispatch via `buff-lang-runtime`.
    Runtime,
}

impl PluginKind {
    /// Lowercase TOML value used in `buff-plugin.toml` (`"compiler"`,
    /// `"lsp"`, `"runtime"`).
    pub fn as_str(&self) -> &'static str {
        match self {
            PluginKind::Compiler => "compiler",
            PluginKind::Lsp => "lsp",
            PluginKind::Runtime => "runtime",
        }
    }
}

impl std::fmt::Display for PluginKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// The parsed form of a single `buff-plugin.toml` file.
///
/// Fields are kept `pub` because the registry reads them directly
/// during load — there is no invariant to guard that would justify
/// getter methods for v1.0.
///
/// # Required vs optional
///
/// - REQUIRED: `name`, `version`, `kind`, `entry_point`.
/// - OPTIONAL: `description` (defaults to empty string).
///
/// Unknown fields are silently ignored by serde (forward-compat —
/// new manifest keys added later don't break older hosts).
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct PluginManifest {
    /// Plugin name. Must be a non-empty string. Used as the
    /// human-readable identifier in logs + error messages.
    pub name: String,
    /// Semver-style version string (e.g. `"0.1.0"`). Stored verbatim
    /// — the manifest parser does not enforce semver well-formedness
    /// in v1.0 (matches the [`BuffConfig`] precedent).
    ///
    /// [`BuffConfig`]: https://github.com/buff-lang/buff/blob/master/crates/buff-lang-cli/src/config.rs
    pub version: String,
    /// Plugin kind — selects which trait the registered object must
    /// implement. One of `"compiler"`, `"lsp"`, `"runtime"`.
    pub kind: PluginKind,
    /// Fully-qualified entry-point string naming the plugin's
    /// concrete type (e.g. `"my_lint_plugin::NoTodoLint"`). The host
    /// uses this to look up the registered trait object — there is
    /// NO dynamic loading (the plugin must have been statically
    /// linked into the binary via
    /// [`PluginRegistry::register`](crate::PluginRegistry::register)
    /// or the `register_static!` macro).
    pub entry_point: String,
    /// Optional human-readable description. Defaults to the empty
    /// string when absent from the manifest. Surfaced in `buff
    /// plugins list` (T72 follow-up) and error messages.
    #[serde(default)]
    pub description: String,
}

impl PluginManifest {
    /// Parse a `buff-plugin.toml` document from its text form.
    ///
    /// Returns [`PluginError::ManifestParse`] on syntax / serde
    /// errors and [`PluginError::ManifestInvalid`] on structural
    /// validation failures (empty `name`, empty `entry_point`, etc.).
    /// Never panics.
    ///
    /// # Example
    ///
    /// ```
    /// use buff_plugins::PluginManifest;
    ///
    /// let toml = r#"
    /// name = "demo"
    /// version = "0.1.0"
    /// kind = "compiler"
    /// entry_point = "demo::Demo"
    /// description = "demo plugin"
    /// "#;
    /// let m = PluginManifest::parse(toml).expect("valid manifest");
    /// assert_eq!(m.name, "demo");
    /// assert_eq!(m.kind.as_str(), "compiler");
    /// ```
    pub fn parse(toml_text: &str) -> Result<Self> {
        let manifest: PluginManifest = toml::from_str(toml_text)?;
        manifest.validate()?;
        Ok(manifest)
    }

    /// Load and parse a `buff-plugin.toml` file from disk.
    ///
    /// Reads the file synchronously then delegates to
    /// [`PluginManifest::parse`]. A missing file surfaces as
    /// [`PluginError::ManifestIo`].
    pub fn load_from_file(path: &std::path::Path) -> Result<Self> {
        let text = std::fs::read_to_string(path).map_err(|source| PluginError::ManifestIo {
            path: path.display().to_string(),
            source,
        })?;
        Self::parse(&text)
    }

    /// Structural validation — surfaces REQUIRED-field violations as
    /// [`PluginError::ManifestInvalid`].
    ///
    /// Called by [`PluginManifest::parse`] AFTER serde succeeds so
    /// the user sees a helpful "field X is required" message rather
    /// than serde's generic missing-field error.
    fn validate(&self) -> Result<()> {
        if self.name.trim().is_empty() {
            return Err(PluginError::ManifestInvalid {
                field: "name",
                detail: "must be a non-empty string".to_string(),
            });
        }
        if self.version.trim().is_empty() {
            return Err(PluginError::ManifestInvalid {
                field: "version",
                detail: "must be a non-empty string (semver recommended)".to_string(),
            });
        }
        if self.entry_point.trim().is_empty() {
            return Err(PluginError::ManifestInvalid {
                field: "entry_point",
                detail: "must be a non-empty string \
                         (fully-qualified path to a statically-registered plugin)"
                    .to_string(),
            });
        }
        Ok(())
    }
}

impl std::fmt::Display for PluginManifest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{} v{} [{}] → {}",
            self.name, self.version, self.kind, self.entry_point
        )
    }
}

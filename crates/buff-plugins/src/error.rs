//! `buff-plugins` error type.
//!
//! A single [`PluginError`] enum wraps every fallible surface in this
//! crate: manifest parsing, manifest validation, plugin loading, and
//! dispatch-time plugin failures. Mirrors the pattern used by the
//! other workspace crates (e.g. [`buff_lang_error::ConfigError`] in
//! `buff-lang-cli/src/config.rs`, [`buff_cache::CacheError`]) —
//! `thiserror::Error` derive, no `unwrap`/`expect`/`panic!`, every
//! variant carries enough context to surface a useful diagnostic.
//!
//! [`buff_cache::CacheError`]: https://github.com/buff-lang/buff/blob/master/crates/buff-cache/src/error.rs

use thiserror::Error;

/// Errors surfaced by the plugin system.
///
/// Every variant preserves enough context for the host (compiler /
/// LSP / runtime) to render a useful user-facing diagnostic without
/// losing the underlying cause. The error never aborts the process —
/// hosts decide whether a plugin failure is fatal (e.g. `buff check`
/// continues with the remaining plugins and reports the failure as a
/// warning).
#[derive(Debug, Error)]
pub enum PluginError {
    /// TOML syntax error OR serde deserialisation failure when
    /// parsing a `buff-plugin.toml` manifest. Wraps the underlying
    /// `toml::de::Error` so the user sees the line/col of the bad
    /// field.
    #[error("failed to parse buff-plugin.toml: {0}")]
    ManifestParse(#[from] toml::de::Error),

    /// File-system error reading a manifest (or a config file that
    /// references one) from disk.
    #[error("failed to read plugin manifest `{path}`: {source}")]
    ManifestIo {
        /// The path that was being read when the I/O error occurred.
        path: String,
        /// The underlying io::Error, wrapped for context.
        #[source]
        source: std::io::Error,
    },

    /// Manifest structural validation failure — a required field was
    /// missing or had an invalid value. The `field` carries the
    /// dotted path (e.g. `"kind"`, `"entry_point"`) so the user can
    /// find the offending key quickly; `detail` explains the
    /// constraint that was violated.
    #[error("invalid buff-plugin.toml: field `{field}` — {detail}")]
    ManifestInvalid {
        /// Dotted path of the offending field (e.g. `"kind"`,
        /// `"entry_point"`).
        field: &'static str,
        /// Human-readable explanation of the constraint that failed
        /// (e.g. `"must be one of: compiler, lsp, runtime"`).
        detail: String,
    },

    /// A plugin was registered with an entry_point string that
    /// doesn't resolve to a known statically-registered plugin.
    ///
    /// Trait-object dispatch (NO dlopen) means a plugin must have
    /// been linked into the binary and registered via
    /// [`PluginRegistry::register`](crate::PluginRegistry::register)
    /// (or via the `register_static!` macro) before it can be
    /// referenced from a manifest. This error signals the lookup miss.
    #[error(
        "plugin entry_point `{entry_point}` not found in registry \
             (did you forget to call `PluginRegistry::register`?)"
    )]
    EntryPointNotFound {
        /// The fully-qualified entry-point string from the manifest
        /// (e.g. `"my_lint_plugin::NoTodoLint"`).
        entry_point: String,
    },

    /// A codegen pass returned an error. The plugin's `run_codegen_pass`
    /// returns `Result<()>`; this variant carries the plugin's
    /// human-readable error message verbatim.
    #[error("plugin `{plugin}` codegen pass failed: {detail}")]
    CodegenPassFailed {
        /// The name returned by the failing plugin's `name()` method.
        plugin: String,
        /// The plugin-supplied error message (no structuring — verbatim).
        detail: String,
    },

    /// Catch-all for unexpected failures inside plugin dispatch that
    /// don't fit one of the more specific variants. The `detail`
    /// string is the plugin's own explanation.
    #[error("plugin `{plugin}` failed: {detail}")]
    PluginFailed {
        /// The name returned by the failing plugin's `name()` method.
        plugin: String,
        /// Verbatim detail string supplied by the caller.
        detail: String,
    },
}

/// A type alias used throughout the crate for fallible operations.
///
/// Every public function that can fail returns `Result<T, PluginError>`.
pub type Result<T> = std::result::Result<T, PluginError>;

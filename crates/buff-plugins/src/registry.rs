//! Plugin registry — loads + dispatches plugin trait objects.
//!
//! The [`PluginRegistry`] is the central type plugins register with
//! and the host (compiler / LSP / runtime) dispatches through. It
//! holds three `Vec<Box<dyn T>>` collections (one per plugin kind)
//! and exposes one `dispatch_*` method per trait method.
//!
//! # Loading
//!
//! Plugins are loaded in two ways:
//!
//! 1. **Programmatic** — `register_compiler(plugin)`,
//!    `register_lsp(plugin)`, `register_runtime(plugin)`. Used by
//!    in-tree plugins (the examples in `examples/plugins/`) and by
//!    integration tests.
//! 2. **Manifest-driven** — `load_from_config(path)` reads one or
//!    more `buff-plugin.toml` files and registers the plugins they
//!    describe. The lookup from manifest's `entry_point` string to
//!    a registered trait object is mediated by a
//!    [`PluginFactory`](PluginFactory) callback the host provides
//!    (the registry itself does NOT do dynamic loading).
//!
//! # NO dlopen
//!
//! Per the T72 spec: "Plugin loading via dynamic dispatch (NOT
//! dlopen — use trait objects)". The registry never loads a `.so` /
//! `.dll` / `.dylib`. Plugins are statically linked into the host
//! binary and registered via `register_*` / the `register_static!`
//! macro at startup. The manifest's `entry_point` string is purely
//! a lookup key — it does NOT name a file to load.

use std::path::Path;

use buff_lang_ast::Decl;

use crate::compiler::{CompilerPlugin, LintWarning};
use crate::error::{PluginError, Result};
use crate::lsp::{LspPlugin, PluginCodeAction, PluginHover, PluginPosition};
use crate::manifest::{PluginKind, PluginManifest};
use crate::runtime::{PluginMetric, PluginSpan, RuntimePlugin};

/// A factory function that constructs a trait object from an
/// `entry_point` string.
///
/// The registry uses this to bridge a manifest's `entry_point`
/// string to a concrete `Box<dyn T>` trait object — the registry
/// itself does NOT know how to construct plugin types (that's the
/// host's job).
///
/// In practice the host registers ONE factory per kind that owns a
/// lookup table from `entry_point` to a constructor closure.
pub trait PluginFactory: Send + Sync {
    /// Construct a [`CompilerPlugin`] trait object for the given
    /// `entry_point`. Returns `None` when this factory doesn't know
    /// how to build the named plugin (the registry tries the next
    /// factory if one is registered — future extension).
    fn build_compiler(&self, entry_point: &str) -> Option<Box<dyn CompilerPlugin>> {
        let _ = entry_point;
        None
    }

    /// Construct an [`LspPlugin`] trait object for the given
    /// `entry_point`.
    fn build_lsp(&self, entry_point: &str) -> Option<Box<dyn LspPlugin>> {
        let _ = entry_point;
        None
    }

    /// Construct a [`RuntimePlugin`] trait object for the given
    /// `entry_point`.
    fn build_runtime(&self, entry_point: &str) -> Option<Box<dyn RuntimePlugin>> {
        let _ = entry_point;
        None
    }
}

/// A simple in-memory factory backed by three `BTreeMap`s of
/// `entry_point → constructor closure`.
///
/// The host populates this at startup (e.g. via
/// [`StaticPluginRegistry`](crate::register_static!)) and passes it
/// to [`PluginRegistry::load_from_config`] so manifests can be
/// resolved.
#[derive(Default)]
pub struct StaticPluginRegistry {
    compiler_builders: std::collections::BTreeMap<
        String,
        std::sync::Arc<dyn Fn() -> Box<dyn CompilerPlugin> + Send + Sync>,
    >,
    lsp_builders: std::collections::BTreeMap<
        String,
        std::sync::Arc<dyn Fn() -> Box<dyn LspPlugin> + Send + Sync>,
    >,
    runtime_builders: std::collections::BTreeMap<
        String,
        std::sync::Arc<dyn Fn() -> Box<dyn RuntimePlugin> + Send + Sync>,
    >,
}

impl StaticPluginRegistry {
    /// Construct an empty registry.
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a compiler plugin constructor under `entry_point`.
    pub fn register_compiler(
        &mut self,
        entry_point: impl Into<String>,
        builder: impl Fn() -> Box<dyn CompilerPlugin> + Send + Sync + 'static,
    ) {
        self.compiler_builders
            .insert(entry_point.into(), std::sync::Arc::new(builder));
    }

    /// Register an LSP plugin constructor under `entry_point`.
    pub fn register_lsp(
        &mut self,
        entry_point: impl Into<String>,
        builder: impl Fn() -> Box<dyn LspPlugin> + Send + Sync + 'static,
    ) {
        self.lsp_builders
            .insert(entry_point.into(), std::sync::Arc::new(builder));
    }

    /// Register a runtime plugin constructor under `entry_point`.
    pub fn register_runtime(
        &mut self,
        entry_point: impl Into<String>,
        builder: impl Fn() -> Box<dyn RuntimePlugin> + Send + Sync + 'static,
    ) {
        self.runtime_builders
            .insert(entry_point.into(), std::sync::Arc::new(builder));
    }

    /// `true` when `entry_point` is registered under ANY kind.
    pub fn contains(&self, entry_point: &str) -> bool {
        self.compiler_builders.contains_key(entry_point)
            || self.lsp_builders.contains_key(entry_point)
            || self.runtime_builders.contains_key(entry_point)
    }
}

impl PluginFactory for StaticPluginRegistry {
    fn build_compiler(&self, entry_point: &str) -> Option<Box<dyn CompilerPlugin>> {
        self.compiler_builders.get(entry_point).map(|f| f())
    }

    fn build_lsp(&self, entry_point: &str) -> Option<Box<dyn LspPlugin>> {
        self.lsp_builders.get(entry_point).map(|f| f())
    }

    fn build_runtime(&self, entry_point: &str) -> Option<Box<dyn RuntimePlugin>> {
        self.runtime_builders.get(entry_point).map(|f| f())
    }
}

/// The central plugin registry.
///
/// Holds three `Vec<Box<dyn T>>` collections (one per plugin kind)
/// and exposes one `dispatch_*` method per trait method. Hosts
/// (compiler / LSP / runtime) hold a single `PluginRegistry` and
/// call the relevant dispatch method at each hook point.
///
/// # Empty-registry semantics
///
/// A fresh `PluginRegistry::new()` returns a registry with zero
/// plugins in every kind. Every `dispatch_*` method on an empty
/// registry returns the empty result (`Vec::new()` / `Ok(None)` /
/// `Ok(())`) — never errors. This means hooking plugin dispatch
/// into existing tools (e.g. `buff check`) is a pure no-op when no
/// plugins are registered.
pub struct PluginRegistry {
    compiler: Vec<Box<dyn CompilerPlugin>>,
    lsp: Vec<Box<dyn LspPlugin>>,
    runtime: Vec<Box<dyn RuntimePlugin>>,
}

impl Default for PluginRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl PluginRegistry {
    /// Construct an empty registry.
    pub fn new() -> Self {
        Self {
            compiler: Vec::new(),
            lsp: Vec::new(),
            runtime: Vec::new(),
        }
    }

    /// Register a compiler plugin.
    pub fn register_compiler(&mut self, plugin: Box<dyn CompilerPlugin>) -> &mut Self {
        self.compiler.push(plugin);
        self
    }

    /// Register an LSP plugin.
    pub fn register_lsp(&mut self, plugin: Box<dyn LspPlugin>) -> &mut Self {
        self.lsp.push(plugin);
        self
    }

    /// Register a runtime plugin.
    pub fn register_runtime(&mut self, plugin: Box<dyn RuntimePlugin>) -> &mut Self {
        self.runtime.push(plugin);
        self
    }

    /// Number of compiler plugins registered.
    pub fn compiler_count(&self) -> usize {
        self.compiler.len()
    }

    /// Number of LSP plugins registered.
    pub fn lsp_count(&self) -> usize {
        self.lsp.len()
    }

    /// Number of runtime plugins registered.
    pub fn runtime_count(&self) -> usize {
        self.runtime.len()
    }

    /// `true` when at least one compiler plugin is registered.
    pub fn has_compiler(&self) -> bool {
        !self.compiler.is_empty()
    }

    /// `true` when at least one LSP plugin is registered.
    pub fn has_lsp(&self) -> bool {
        !self.lsp.is_empty()
    }

    /// `true` when at least one runtime plugin is registered.
    pub fn has_runtime(&self) -> bool {
        !self.runtime.is_empty()
    }

    // -----------------------------------------------------------------
    // Manifest-driven loading.
    // -----------------------------------------------------------------

    /// Load plugins described by one or more `buff-plugin.toml`
    /// files. Each manifest's `entry_point` is resolved through the
    /// provided [`PluginFactory`]; the resulting trait objects are
    /// added to the registry.
    ///
    /// `paths` is a list of paths to `buff-plugin.toml` files (or
    /// directories containing them — directories are scanned
    /// non-recursively). Unknown entry points surface as
    /// [`PluginError::EntryPointNotFound`].
    pub fn load_from_config(&mut self, paths: &[&Path], factory: &dyn PluginFactory) -> Result<()> {
        for path in paths {
            let manifests = collect_manifests(path)?;
            for manifest in manifests {
                self.load_one(&manifest, factory)?;
            }
        }
        Ok(())
    }

    /// Resolve a single manifest through the factory and register
    /// the resulting trait object.
    fn load_one(&mut self, manifest: &PluginManifest, factory: &dyn PluginFactory) -> Result<()> {
        match manifest.kind {
            PluginKind::Compiler => {
                let plugin = factory
                    .build_compiler(&manifest.entry_point)
                    .ok_or_else(|| PluginError::EntryPointNotFound {
                        entry_point: manifest.entry_point.clone(),
                    })?;
                self.compiler.push(plugin);
            }
            PluginKind::Lsp => {
                let plugin = factory.build_lsp(&manifest.entry_point).ok_or_else(|| {
                    PluginError::EntryPointNotFound {
                        entry_point: manifest.entry_point.clone(),
                    }
                })?;
                self.lsp.push(plugin);
            }
            PluginKind::Runtime => {
                let plugin = factory
                    .build_runtime(&manifest.entry_point)
                    .ok_or_else(|| PluginError::EntryPointNotFound {
                        entry_point: manifest.entry_point.clone(),
                    })?;
                self.runtime.push(plugin);
            }
        }
        Ok(())
    }

    // -----------------------------------------------------------------
    // Dispatch — compiler.
    // -----------------------------------------------------------------

    /// Run every registered compiler plugin's `run_lint` over `ast`
    /// and concatenate the warnings (in registration order).
    ///
    /// Empty registry → empty `Vec`.
    pub fn dispatch_compiler_lint(&self, ast: &[Decl]) -> Vec<LintWarning> {
        let mut out = Vec::new();
        for plugin in &self.compiler {
            out.extend(plugin.run_lint(ast));
        }
        out
    }

    /// Run every registered compiler plugin's `run_codegen_pass`
    /// over `rust` (in registration order). Stops at the first
    /// plugin that returns `Err`.
    ///
    /// Empty registry → `Ok(())`.
    pub fn dispatch_compiler_codegen(&self, rust: &mut syn::File) -> Result<()> {
        for plugin in &self.compiler {
            plugin.run_codegen_pass(rust)?;
        }
        Ok(())
    }

    // -----------------------------------------------------------------
    // Dispatch — LSP.
    // -----------------------------------------------------------------

    /// Run every registered LSP plugin's `code_actions` for the
    /// cursor and concatenate the actions (in registration order).
    ///
    /// Empty registry → empty `Vec`.
    pub fn dispatch_lsp_code_actions(
        &self,
        uri: &str,
        cursor: PluginPosition,
    ) -> Vec<PluginCodeAction> {
        let mut out = Vec::new();
        for plugin in &self.lsp {
            out.extend(plugin.code_actions(uri, cursor));
        }
        out
    }

    /// Run every registered LSP plugin's `hover` for the cursor.
    /// First plugin to return `Ok(Some(_))` wins; later plugins are
    /// not consulted. `Ok(None)` when no plugin provides hover
    /// content (host falls back to the built-in handler).
    ///
    /// Errors propagate immediately (later plugins not consulted).
    pub fn dispatch_lsp_hover(
        &self,
        uri: &str,
        cursor: PluginPosition,
    ) -> Result<Option<PluginHover>> {
        for plugin in &self.lsp {
            if let Some(hover) = plugin.hover(uri, cursor)? {
                return Ok(Some(hover));
            }
        }
        Ok(None)
    }

    // -----------------------------------------------------------------
    // Dispatch — runtime.
    // -----------------------------------------------------------------

    /// Notify every registered runtime plugin that a span was
    /// entered. Plugins are notified in registration order.
    ///
    /// Empty registry → no-op.
    pub fn dispatch_runtime_span(&self, span: &PluginSpan) {
        for plugin in &self.runtime {
            plugin.on_span_enter(span);
        }
    }

    /// Notify every registered runtime plugin that a metric was
    /// recorded. Plugins are notified in registration order.
    ///
    /// Empty registry → no-op.
    pub fn dispatch_runtime_metric(&self, metric: &PluginMetric) {
        for plugin in &self.runtime {
            plugin.on_metric(&metric.name, metric.value);
        }
    }
}

impl std::fmt::Debug for PluginRegistry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PluginRegistry")
            .field("compiler", &self.compiler.len())
            .field("lsp", &self.lsp.len())
            .field("runtime", &self.runtime.len())
            .finish()
    }
}

// ---------------------------------------------------------------------
// Manifest collection helpers.
// -----------------------------------------------------------------

/// Collect every `buff-plugin.toml` reachable from `path`.
///
/// - If `path` is a file: parse it as a single manifest.
/// - If `path` is a directory: scan it non-recursively for files
///   named `buff-plugin.toml` (or `*.buff-plugin.toml`). Other
///   files are silently ignored.
///
/// Returns the parsed manifests in lexicographic-path order so the
/// dispatch order is deterministic across hosts (project hard rule).
fn collect_manifests(path: &Path) -> Result<Vec<PluginManifest>> {
    if path.is_file() {
        let manifest = PluginManifest::load_from_file(path)?;
        return Ok(vec![manifest]);
    }
    if path.is_dir() {
        let mut entries: Vec<std::path::PathBuf> = std::fs::read_dir(path)
            .map_err(|source| PluginError::ManifestIo {
                path: path.display().to_string(),
                source,
            })?
            .flatten()
            .map(|e| e.path())
            .filter(|p| {
                p.file_name()
                    .and_then(|n| n.to_str())
                    .is_some_and(|n| n == "buff-plugin.toml" || n.ends_with(".buff-plugin.toml"))
            })
            .collect();
        entries.sort();
        let mut out = Vec::with_capacity(entries.len());
        for entry in entries {
            let manifest = PluginManifest::load_from_file(&entry)?;
            out.push(manifest);
        }
        return Ok(out);
    }
    // Neither file nor dir — surface a ManifestIo so the user sees
    // the offending path.
    Err(PluginError::ManifestIo {
        path: path.display().to_string(),
        source: std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "path is neither a file nor a directory",
        ),
    })
}

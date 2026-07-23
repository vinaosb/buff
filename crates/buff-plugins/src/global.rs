//! Global plugin registry + free-function dispatch helpers.
//!
//! The host (compiler / LSP / runtime) holds a single
//! [`PluginRegistry`] at startup and dispatches through it at each
//! hook point. To keep the host-side wiring minimal, this module
//! exposes:
//!
//! - A process-wide `static` registry guarded by a `Mutex`
//!   ([`global_registry()`]).
//! - Free-function dispatch helpers
//!   ([`dispatch_global_compiler_lint`] etc.) that hosts can call
//!   directly without managing their own registry.
//! - An env-var-driven loader ([`try_load_global_from_env`]) that
//!   reads `BUFF_PLUGIN_DIR` / `BUFF_PLUGIN_PATH` at startup and
//!   resolves every `buff-plugin.toml` it finds against a host-
//!   supplied [`PluginFactory`].
//!
//! # Why a global?
//!
//! The plugin registry is a *process-wide* resource: a single
//! plugin-loaded set is shared across every `buff check` /
//! `buff-lsp` invocation within the same process. A global
//! `Mutex<PluginRegistry>` is the simplest way to expose this —
//! the alternative (threading a registry through every function
//! signature in `buff-lang-cli` + `buff-lsp`) would be a massive
//! ripple for a v1.0 MVP.
//!
//! The global is `OnceLock<Mutex<PluginRegistry>>` so:
//!
//! - first access lazily constructs the registry,
//! - subsequent accesses return the same instance,
//! - `Mutex` guards mutation; poisoning falls back to an empty
//!   registry (NEVER panics — matches the project-wide
//!   panic-free contract).
//!
//! # Loader protocol
//!
//! At startup the host calls [`try_load_global_from_env`] with a
//! host-supplied [`PluginFactory`]. If neither
//! `BUFF_PLUGIN_DIR` nor `BUFF_PLUGIN_PATH` is set, the loader is
//! a no-op (the registry stays empty → every dispatch returns
//! the empty result → behaviour is identical to a host with no
//! plugin hook).

use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};

use buff_lang_ast::Decl;

use crate::compiler::LintWarning;
use crate::error::Result;
use crate::lsp::{PluginCodeAction, PluginHover, PluginPosition};
use crate::registry::{PluginFactory, PluginRegistry};
use crate::runtime::{PluginMetric, PluginSpan};

/// Env var naming a directory containing `buff-plugin.toml` files
/// to load at startup. Single path; subdirectories are scanned
/// non-recursively.
pub const ENV_PLUGIN_DIR: &str = "BUFF_PLUGIN_DIR";

/// Env var naming a `:`-separated (Unix) / `;`-separated (Windows)
/// list of paths to `buff-plugin.toml` files OR directories
/// containing them. Takes precedence over [`ENV_PLUGIN_DIR`] when
/// both are set.
pub const ENV_PLUGIN_PATH: &str = "BUFF_PLUGIN_PATH";

fn global_cell() -> &'static Mutex<PluginRegistry> {
    static CELL: OnceLock<Mutex<PluginRegistry>> = OnceLock::new();
    CELL.get_or_init(|| Mutex::new(PluginRegistry::new()))
}

/// Borrow the global registry for mutation (registration +
/// manifest-driven loading).
///
/// Poisoning falls back to a fresh empty registry — the function
/// NEVER panics. Returns the `MutexGuard` so the caller can chain
/// method calls.
pub fn global_registry() -> std::sync::MutexGuard<'static, PluginRegistry> {
    global_cell().lock().unwrap_or_else(|p| p.into_inner())
}

// ---------------------------------------------------------------------
// Registration helpers.
// ---------------------------------------------------------------------

/// Register a compiler plugin in the global registry.
pub fn register_global_compiler(plugin: Box<dyn crate::CompilerPlugin>) {
    global_registry().register_compiler(plugin);
}

/// Register an LSP plugin in the global registry.
pub fn register_global_lsp(plugin: Box<dyn crate::LspPlugin>) {
    global_registry().register_lsp(plugin);
}

/// Register a runtime plugin in the global registry.
pub fn register_global_runtime(plugin: Box<dyn crate::RuntimePlugin>) {
    global_registry().register_runtime(plugin);
}

// ---------------------------------------------------------------------
// Loader.
// ---------------------------------------------------------------------

/// Read `BUFF_PLUGIN_DIR` / `BUFF_PLUGIN_PATH` and load every
/// reachable `buff-plugin.toml` into the global registry.
///
/// Both env vars being unset → no-op (empty registry, every
/// dispatch returns the empty result).
///
/// Errors propagate from [`PluginRegistry::load_from_config`] —
/// the host decides whether to abort or log + continue. The
/// recommended pattern is:
///
/// ```no_run
/// use buff_plugins::try_load_global_from_env;
/// use buff_plugins::StaticPluginRegistry;
///
/// let factory = StaticPluginRegistry::new();
/// if let Err(e) = try_load_global_from_env(&factory) {
///     eprintln!("warning: plugin load failed: {e}");
/// }
/// ```
pub fn try_load_global_from_env(factory: &dyn PluginFactory) -> Result<()> {
    let paths = collect_plugin_paths()?;
    if paths.is_empty() {
        return Ok(());
    }
    let path_refs: Vec<&std::path::Path> = paths.iter().map(std::path::PathBuf::as_path).collect();
    global_registry().load_from_config(&path_refs, factory)
}

/// Collect plugin paths from `BUFF_PLUGIN_PATH` (precedence) +
/// `BUFF_PLUGIN_DIR`. Empty when neither is set.
fn collect_plugin_paths() -> Result<Vec<PathBuf>> {
    let mut out: Vec<PathBuf> = Vec::new();
    if let Ok(raw) = std::env::var(ENV_PLUGIN_PATH) {
        for part in raw.split(path_separator()) {
            if part.is_empty() {
                continue;
            }
            out.push(PathBuf::from(part));
        }
    }
    if let Ok(raw) = std::env::var(ENV_PLUGIN_DIR) {
        if !raw.is_empty() {
            out.push(PathBuf::from(raw));
        }
    }
    Ok(out)
}

/// Platform path separator (`:` on Unix, `;` on Windows).
fn path_separator() -> char {
    if cfg!(windows) {
        ';'
    } else {
        ':'
    }
}

// ---------------------------------------------------------------------
// Dispatch helpers — call into the global registry.
// ---------------------------------------------------------------------

/// Run every registered compiler plugin's `run_lint` via the
/// global registry. Empty registry → empty `Vec`.
///
/// Hosts (e.g. `buff check`) call this AFTER their built-in linters
/// and merge the warnings into the report.
pub fn dispatch_global_compiler_lint(ast: &[Decl]) -> Vec<LintWarning> {
    match global_cell().lock() {
        Ok(guard) => guard.dispatch_compiler_lint(ast),
        Err(p) => p.into_inner().dispatch_compiler_lint(ast),
    }
}

/// Run every registered LSP plugin's `code_actions` via the global
/// registry. Empty registry → empty `Vec`.
pub fn dispatch_global_lsp_code_actions(
    uri: &str,
    cursor: PluginPosition,
) -> Vec<PluginCodeAction> {
    match global_cell().lock() {
        Ok(guard) => guard.dispatch_lsp_code_actions(uri, cursor),
        Err(p) => p.into_inner().dispatch_lsp_code_actions(uri, cursor),
    }
}

/// Run every registered LSP plugin's `hover` via the global
/// registry. Empty registry → `Ok(None)`.
pub fn dispatch_global_lsp_hover(uri: &str, cursor: PluginPosition) -> Result<Option<PluginHover>> {
    match global_cell().lock() {
        Ok(guard) => guard.dispatch_lsp_hover(uri, cursor),
        Err(p) => p.into_inner().dispatch_lsp_hover(uri, cursor),
    }
}

/// Notify every registered runtime plugin of a span enter via the
/// global registry. Empty registry → no-op.
pub fn dispatch_global_runtime_span(span: &PluginSpan) {
    match global_cell().lock() {
        Ok(guard) => guard.dispatch_runtime_span(span),
        Err(p) => p.into_inner().dispatch_runtime_span(span),
    }
}

/// Notify every registered runtime plugin of a metric via the
/// global registry. Empty registry → no-op.
pub fn dispatch_global_runtime_metric(metric: &PluginMetric) {
    match global_cell().lock() {
        Ok(guard) => guard.dispatch_runtime_metric(metric),
        Err(p) => p.into_inner().dispatch_runtime_metric(metric),
    }
}

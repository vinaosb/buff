//! `buff-plugins` — plugin extension points for Buff
//! (compiler + LSP + runtime).
//!
//! Pure-Rust trait-object dispatch (NO `dlopen` / `libloading` —
//! per the T72 task spec). Defines three plugin traits, a
//! `buff-plugin.toml` manifest format, and a [`PluginRegistry`]
//! that loads + dispatches plugin trait objects.
//!
//! # The three plugin traits
//!
//! | Trait | Hooks into | Hook points |
//! |-------|------------|-------------|
//! | [`CompilerPlugin`] | compiler pipeline (`buff check` / `buff build`) | lint pass + codegen pass |
//! | [`LspPlugin`] | `buff-lsp` server | code actions + hover |
//! | [`RuntimePlugin`] | `buff-lang-runtime` | span enter + metric emit |
//!
//! All three are object-safe + `Send + Sync` so the registry can
//! hold `Vec<Box<dyn T>>` and dispatch via virtual call.
//!
//! # Manifest format (`buff-plugin.toml`)
//!
//! ```toml
//! name = "my-lint-plugin"
//! version = "0.1.0"
//! kind = "compiler"   # one of: compiler | lsp | runtime
//! entry_point = "my_lint_plugin::NoTodoLint"
//! description = "Rejects `todo!()` / `unwrap()` calls."
//! ```
//!
//! See [`PluginManifest`](manifest::PluginManifest) for the full
//! struct + parsing semantics.
//!
//! # Loading + dispatch
//!
//! Plugins are loaded via [`PluginRegistry::load_from_config`]
//! which reads `buff-plugin.toml` files and resolves each
//! manifest's `entry_point` through a [`PluginFactory`]. The
//! factory pattern (rather than dlopen) means the host binary
//! statically links the plugins at build time and registers their
//! constructors in a [`StaticPluginRegistry`] at startup — the
//! manifest is purely a lookup key.
//!
//! Hosts dispatch through one of:
//!
//! - [`PluginRegistry::dispatch_compiler_lint`]
//! - [`PluginRegistry::dispatch_compiler_codegen`]
//! - [`PluginRegistry::dispatch_lsp_code_actions`]
//! - [`PluginRegistry::dispatch_lsp_hover`]
//! - [`PluginRegistry::dispatch_runtime_span`]
//! - [`PluginRegistry::dispatch_runtime_metric`]
//!
//! # Empty-registry semantics
//!
//! A fresh `PluginRegistry::new()` returns a registry with zero
//! plugins. Every dispatch method on an empty registry returns the
//! empty result (`Vec::new()` / `Ok(None)` / `Ok(())`) — never
//! errors. Hooking plugin dispatch into existing tools is a pure
//! no-op when no plugins are registered.
//!
//! # Panic-free contract
//!
//! No `unwrap` / `expect` / `panic!` / `todo!` / `unimplemented!`
//! in non-test code. Capacity validation returns `Result`.
//!
//! # Examples
//!
//! See `examples/plugins/` for three reference plugins:
//!
//! - `no_todo_lint` — compiler plugin that rejects `todo!()` /
//!   `unwrap()` calls.
//! - `math_hover` — LSP plugin that adds hover docs for math
//!   operations.
//! - `json_tracing` — runtime plugin that exports tracing spans as
//!   JSON.

pub mod compiler;
pub mod error;
pub mod global;
pub mod lsp;
pub mod manifest;
pub mod registry;
pub mod runtime;

// Convenience re-exports — the canonical entry points.
pub use compiler::{CompilerPlugin, LintWarning};
pub use error::{PluginError, Result};
pub use global::{
    dispatch_global_compiler_lint, dispatch_global_lsp_code_actions, dispatch_global_lsp_hover,
    dispatch_global_runtime_metric, dispatch_global_runtime_span, register_global_compiler,
    register_global_lsp, register_global_runtime, try_load_global_from_env,
};
pub use lsp::{LspPlugin, PluginCodeAction, PluginHover, PluginPosition};
pub use manifest::{PluginKind, PluginManifest};
pub use registry::{PluginFactory, PluginRegistry, StaticPluginRegistry};
pub use runtime::{PluginMetric, PluginSpan, RuntimePlugin};

/// Convenience macro for registering a plugin constructor in a
/// [`StaticPluginRegistry`] at startup.
///
/// Generates a static block that — when the macro is invoked inside
/// a function — adds the constructor to the registry. This is the
/// closest analog to `inventory::submit!` we can get without adding
/// the `inventory` crate as a runtime dep (T72 spec says "or
/// similar" — a plain function call is simpler and avoids the
/// linker-section trickery `inventory` uses).
///
/// # Example
///
/// ```no_run
/// use buff_plugins::{register_static, CompilerPlugin, StaticPluginRegistry};
///
/// struct MyLint;
/// impl CompilerPlugin for MyLint {
///     fn name(&self) -> &str { "my-lint" }
///     // ...
/// }
///
/// fn register(reg: &mut StaticPluginRegistry) {
///     register_static!(reg, compiler, "my_plugin::MyLint", || Box::new(MyLint));
/// }
/// ```
#[macro_export]
macro_rules! register_static {
    ($registry:expr, compiler, $entry_point:expr, $builder:expr) => {
        $registry.register_compiler($entry_point, $builder)
    };
    ($registry:expr, lsp, $entry_point:expr, $builder:expr) => {
        $registry.register_lsp($entry_point, $builder)
    };
    ($registry:expr, runtime, $entry_point:expr, $builder:expr) => {
        $registry.register_runtime($entry_point, $builder)
    };
}

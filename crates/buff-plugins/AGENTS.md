# buff-plugins

Plugin extension points for the Buff language. Pure-Rust trait-object dispatch (NO `dlopen` / `libloading`).

**Status: experimental** (T72 v1.21 community & quality wave).

## OVERVIEW

Three plugin traits + a manifest format + a registry. Plugins are statically linked into the host binary and registered via `StaticPluginRegistry` at startup; manifests are purely lookup keys (no dynamic loading). The registry dispatches via virtual calls — no `unsafe`, no FFI shims, no linker-section tricks.

| Trait | Hooks into | Hook points |
|-------|------------|-------------|
| `CompilerPlugin` | compiler pipeline (`buff check` / `buff build`) | `run_lint(&[Decl])` + `run_codegen_pass(&mut syn::File)` |
| `LspPlugin` | `buff-lsp` server | `code_actions(uri, cursor)` + `hover(uri, cursor)` |
| `RuntimePlugin` | `buff-lang-runtime` | `on_span_enter(&PluginSpan)` + `on_metric(name, value)` |

## STRUCTURE

```
buff-plugins/
├── Cargo.toml          # serde + toml + thiserror + syn + buff-lang-ast + buff-lang-error
├── src/
│   ├── lib.rs          # Public API + re-exports + register_static! macro
│   ├── error.rs        # PluginError enum (ManifestParse / ManifestIo / ManifestInvalid / EntryPointNotFound / CodegenPassFailed / PluginFailed)
│   ├── manifest.rs     # PluginManifest (buff-plugin.toml serde struct) + PluginKind enum
│   ├── compiler.rs     # CompilerPlugin trait + LintWarning
│   ├── lsp.rs          # LspPlugin trait + PluginPosition + PluginCodeAction + PluginHover
│   ├── runtime.rs      # RuntimePlugin trait + PluginSpan + PluginMetric
│   └── registry.rs     # PluginRegistry + PluginFactory + StaticPluginRegistry + collect_manifests helper
└── tests/
    └── core.rs         # 15 tests (manifest parse + trait dispatch + registry loading + example plugins)
```

Total: ~1100 LOC (well under the 3000 LOC T72 cap).

## WHERE TO LOOK

| Task | File |
|---|---|
| Add a new PluginError variant | `src/error.rs` |
| Add a new manifest field | `src/manifest.rs` (add field + serde annotation + validate() check if required) |
| Add a new CompilerPlugin hook | `src/compiler.rs` (add method to trait) + `src/registry.rs::dispatch_compiler_*` + tests |
| Add a new LspPlugin hook | `src/lsp.rs` + `src/registry.rs::dispatch_lsp_*` + tests |
| Add a new RuntimePlugin hook | `src/runtime.rs` + `src/registry.rs::dispatch_runtime_*` + tests |
| Change how manifests are discovered | `src/registry.rs::collect_manifests` |
| Hook plugin dispatch into `buff check` | `crates/buff-lang-cli/src/check.rs` — call `PluginRegistry::dispatch_compiler_lint` after the built-in linters |
| Hook plugin dispatch into `buff-lsp` | `crates/buff-lsp/src/handlers.rs` — call `PluginRegistry::dispatch_lsp_code_actions` / `dispatch_lsp_hover` after the built-in handlers |

## PUBLIC API

### `PluginRegistry` (10 methods)
- Constructors: `new()`, `default()`
- Registration: `register_compiler(Box<dyn CompilerPlugin>)`, `register_lsp(Box<dyn LspPlugin>)`, `register_runtime(Box<dyn RuntimePlugin>)`
- Counts: `compiler_count()`, `lsp_count()`, `runtime_count()`, `has_compiler()`, `has_lsp()`, `has_runtime()`
- Loading: `load_from_config(&[&Path], &dyn PluginFactory)`
- Dispatch (compiler): `dispatch_compiler_lint(&[Decl]) -> Vec<LintWarning>`, `dispatch_compiler_codegen(&mut syn::File) -> Result<()>`
- Dispatch (LSP): `dispatch_lsp_code_actions(uri, cursor) -> Vec<PluginCodeAction>`, `dispatch_lsp_hover(uri, cursor) -> Result<Option<PluginHover>>`
- Dispatch (runtime): `dispatch_runtime_span(&PluginSpan)`, `dispatch_runtime_metric(&PluginMetric)`

### `PluginManifest` (3 methods)
- `parse(toml_text) -> Result<PluginManifest>` — parse from string
- `load_from_file(path) -> Result<PluginManifest>` — load from disk
- `validate() -> Result<()>` — structural validation (called by parse)

### `PluginKind` (2 methods)
- `as_str() -> &'static str` — lowercase TOML name
- `Display` impl (same as `as_str`)

## CONVENTIONS

- **NO `unwrap`/`expect`/`panic!`/`todo!`/`unimplemented!`** in non-test code (project-wide rule).
- **Object-safe traits**: every plugin trait is `Send + Sync` with no generics and no `Self` by value. The registry holds `Vec<Box<dyn T>>` and dispatches via virtual call.
- **`BTreeMap` everywhere**: any future collection-typed field is `BTreeMap` (NOT `HashMap`) — project hard rule for deterministic output.
- **Default no-op methods**: every trait hook has a default no-op so plugin authors implement only the hooks they care about.
- **Plugin-local types**: `PluginPosition`, `PluginCodeAction`, `PluginHover`, `PluginSpan`, `PluginMetric` are owned value-types — NOT `lsp_types::*` / `buff_lang_error::Span` aliases. The host converts at the dispatch boundary. This keeps the `buff-plugins` dep surface minimal.
- **Empty-registry no-op**: every dispatch method returns the empty result (`Vec::new()` / `Ok(None)` / `Ok(())`) when no plugins are registered. Hooking plugin dispatch into existing tools is a pure no-op when no plugins are registered.

## MANIFEST FORMAT (`buff-plugin.toml`)

```toml
name = "my-lint-plugin"
version = "0.1.0"
kind = "compiler"   # one of: compiler | lsp | runtime
entry_point = "my_lint_plugin::NoTodoLint"
description = "Rejects `todo!()` / `unwrap()` calls."
```

REQUIRED: `name`, `version`, `kind`, `entry_point`. OPTIONAL: `description` (defaults to empty string). Unknown fields silently ignored (forward-compat — matches `BuffConfig` precedent).

## NO dlopen

Per the T72 spec: "Plugin loading via dynamic dispatch (NOT dlopen — use trait objects)". The registry NEVER loads a `.so` / `.dll` / `.dylib`. Plugins are statically linked into the host binary and registered via `register_*` / the `register_static!` macro at startup. The manifest's `entry_point` string is purely a lookup key into the [`StaticPluginRegistry`] — it does NOT name a file to load.

This matches the project-wide "no C library, no Docker" hard rule (avoids cc-rs / native-dep issues that pushed hand-rolled lexer/parser).

## RELATIONSHIP TO OTHER CRATES

| Crate | Relationship |
|---|---|
| `buff-lang-cli` | `buff check` calls `PluginRegistry::dispatch_compiler_lint` if any compiler plugins registered. Minimal hook — empty registry is a no-op. |
| `buff-lsp` | LSP handlers call `PluginRegistry::dispatch_lsp_code_actions` / `dispatch_lsp_hover` if any LSP plugins registered. Minimal hook — empty registry is a no-op. |
| `buff-lang-runtime` | (Future T73+) Runtime dispatch calls `PluginRegistry::dispatch_runtime_span` / `dispatch_runtime_metric` if any runtime plugins registered. |
| `buff-lang-ast` | `CompilerPlugin::run_lint` takes `&[buff_lang_ast::Decl]` (the SAME AST the parser produces — no re-parse). |
| `buff-lang-error` | `LintWarning.span: buff_lang_error::Span` — reuses the leaf Span type rather than inventing a plugin-local span newtype. |

## DEFERRED (v1.22+)

Per the T72 spec ("6 examples + 15 tests" is the MVP bar):

- **dlopen-based plugins**: NOT in scope (T72 explicitly forbids). If a future task wants runtime-loaded plugins, it would add a `DynamicPlugin` variant to `PluginFactory` that wraps `libloading`. Pure-Rust host + unsafe-loaded plugin boundary would need careful design (likely deferred indefinitely — the trait-object pattern is strictly safer).
- **Plugin manifest schema versioning**: `version = "0.1.0"` is the manifest VERSION (the plugin's version), not the schema version. A future `schema_version = 1` field could be added for forward-compat when the manifest format evolves.
- **Plugin dependency resolution**: manifests do NOT list dependencies on other plugins. If A depends on B, the host must register B before A — explicit ordering is the host's responsibility.
- **Per-plugin configuration**: manifests do NOT carry plugin-specific config tables. Plugins that need configuration should read env vars / a separate config file at construction time.

## NOTES

- **`register_static!` macro**: a thin wrapper around `register_compiler` / `register_lsp` / `register_runtime` so plugin authoring reads declaratively. NOT a proc-macro — just a `macro_rules!` exported from `lib.rs`. Mirrors how `inventory::submit!` would look without the `inventory` dep + linker-section trickery.
- **`StaticPluginRegistry` is `Default`**: an empty registry is the starting point; hosts call `register_*` at startup to populate it.
- **`PluginFactory` is a trait**: `StaticPluginRegistry` is the canonical impl, but hosts can swap in their own (e.g. a factory that wraps `libloading` for dlopen-style loading in a future task).

## DEPS

- `serde` + `toml` (manifest parsing) — workspace pinned.
- `thiserror` (error enum) — workspace pinned.
- `syn` (compiler codegen pass takes `&mut syn::File`) — workspace pinned.
- `buff-lang-ast` (compiler lint pass takes `&[Decl]`) — workspace path dep.
- `buff-lang-error` (`Span` re-used in `LintWarning`) — workspace path dep.
- `insta` (dev-only — snapshot tests in `tests/core.rs`).

## LAUNCH (for plugin authors)

```bash
# Build the crate
cargo build -p buff-plugins

# Run the test suite
cargo test -p buff-plugins

# Lint
cargo clippy -p buff-plugins --all-targets -- -D warnings

# Run an example plugin
cargo run -p buff-plugins --example no_todo_lint
```

## LICENSE

Dual-licensed under [MIT](../../LICENSE) or [Apache-2.0](../../LICENSE), matching the rest of the Buff workspace.

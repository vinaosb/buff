# Changelog

All notable changes to the Buff transpiler are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [1.12.0] - 2026-07-21

### Added

- **buffup**: Rust toolchain-style version manager (`crates/buffup/`) — `install`/`default`/`list`/`update` subcommands. Downloads pre-built Buff binaries from GitHub Releases (gzip tarball via pure-Rust `flate2`); manages `~/.buff/versions/<ver>/` install dirs + `~/.buff/bin/buff` symlink. Async with `#[tokio::main]` + reqwest rustls-tls. 18 tests hermetic (httpmock).
- **setup-buff Action**: TypeScript GitHub Action (`actions/setup-buff/`) — installs buffup, then uses buffup to install Buff, caches `~/.buff/versions/` across runs by `${{ runner.os }}-${{ runner.arch }}-buff-${{ version }}` key. Node 20 runtime, 27 jest tests, 86.3% line coverage.
- **Docker images**: `docker/builder.Dockerfile` (Rust toolchain + Buff CLI pre-installed, ~2.5GB) + `docker/slim.Dockerfile` (minimal runtime, ~90MB, non-root `buff` user) + `docker/Dockerfile.example` (multi-stage demo) + `docker/docker-bake.hjson` (parallel multi-arch `linux/amd64,linux/arm64` buildx config) + `.github/workflows/docker.yml` (CI on tag push).

### Fixed

- **T135 SSR regression** (`fix(ssr): T135 pass-1`): `buff ssr <file>.buffhtml` was failing end-to-end with `invalid character '.' in crate name` — `make_driver_path()` produced filenames with dots that rustc rejected. Switched to underscores; integration tests now exercise the live rustc path.

## [1.11.0] - 2026-07-21

### Added

- **bufflings**: Rustlings-style exercise runner (`crates/bufflings/`) — 7 subcommands (`list`/`start`/`verify`/`progress`/`watch`/`hint`/`VerifyAllWithSolutions`). 25 exercises across 11 topics (basics/functions/types/control_flow/enums/traits/pattern_matching/error_handling/async/collections/generics_modules). Each exercise = `.buff` (TODO markers) + `.README.md` (concept) + `.sol.buff` (hidden solution). CI gate (`bufflings verify-all-with-solutions`) runs `buff check` against every hidden solution before merge — catches unsolvable exercises. 41 tests (27 unit + 14 integration).

## [1.10.0] - 2026-07-21

### Added

- **buff-dap**: Debug Adapter Protocol translation proxy (`crates/buff-dap/`) — translates DAP requests from editors (VSCode-LLDB, lldb-dap) into `lldb-dap` Rust-debugger sessions, with `.buff` → `.rs` source mapping via `buff-lang-error::SourceMap`. `buff debug` CLI subcommand. GPU kernel debugging, watch expressions, and reverse debugging explicitly out of scope (v2.0+).
- **buff coverage**: Rust-line → `.buff` source-line mapping (`crates/buff-lang-cli/src/coverage/`) — wraps `cargo llvm-cov` / `cargo tarpaulin`, maps Rust-line coverage back to original `.buff` source. LCOV + HTML report output. `buff coverage` CLI subcommand.

## [1.9.0] - 2026-07-20

### Added

- **`.buffhtml` SFC pipeline** (Option C pivot — parallel file format, **ZERO compiler changes**): three new sibling crates:
  - `buff-lang-ast-rsx` — pure-data AST for `.buffhtml` SFC (`<script>buff</script>` + `<template>RSX</template>` + `<style>CSS</style>`)
  - `buff-lang-buffhtml-parser` — hand-rolled 3-mode lexer + recursive-descent parser
  - `buff-lang-codegen-buffhtml` — lowers RSX template AST to `rsx!{}` TokenStream via syn/quote/prettyplease (+ post-format `SpanMap` side-table for rustc diagnostic reverse-mapping)
- **Component model** (T134): lifecycle hooks (`on_init`/`on_destroy`), typed `Props` interface, `prop_check.rs` pre-checker, 4 component examples + `docs/component-model.md` guide.
- **SSR** (T135): `buff-ui-dioxus::render_to_string` via `dioxus-ssr` 0.7; `buff ssr` CLI subcommand. Hydration + iOS/Android recipes documented as USER ACTIONs.

### Decision

- **RSX-for-Buff syntax**: Oracle verdict `e1a5e74` selected **Option C** (`.buffhtml` SFC, Svelte-style) over Options A/B (compiler extension). The 25-keyword freeze and existing compiler crates are untouched. See `.sisyphus/decisions/rsx-syntax-feasibility.md`.

## [1.8.0] - 2026-07-20

### Added

- **buff-ui-dioxus**: In-tree Dioxus 0.7 wrapper (`crates/buff-ui-dioxus/`) — `use_signal`, `render_to_string`, lifecycle hooks. Precondition T121b verdict PASS.
- **buff ui dev**: WebSocket live-reload dev server (`crates/buff-lang-cli/src/ui_dev/`) — file watcher + Wasm builder + axum 0.8 server; browser auto-reloads on `.buffhtml` save.
- **Tauri scaffolding**: `buff ui new --desktop <name>` writes a Tauri 2 project template (runtime dep isolated to template's Cargo.toml — `buff-lang-cli` itself gains no Tauri runtime dep).

## [1.7.0] - 2026-07-19

### Added

- **buff-jupyter**: Pure-Rust Jupyter kernel (`crates/buff-jupyter/`) — `zeromq` 0.4 (NOT `zmq` which links C libzmq — fails on Windows). 5-socket protocol (shell/control/iopub/stdin/heartbeat) + HMAC-SHA256 auth. Cross-cell state via shared `buff-eval`. Rich MIME display (`text/html` + `text/plain` fallback) + `?`/`??` introspection. `buff jupyter install` writes kernelspec.

## [1.6.0] - 2026-07-19

### Added

- **buff-registry**: Pure-Rust package registry HTTP server (`crates/buff-registry/`) — axum 0.8 + semver 1 + in-memory storage (NO diesel/postgres/S3 — matches the "no C library, no Docker" hard rule). Endpoints: `/api/v1/{publish,package,download,resolve}`. Rate limiting, dependency cycle rejection, path-traversal rejection, semver compat resolution.
- **CLI registry integration** (T127): `buff login`/`add`/`publish`/`install` for end-to-end package authoring.
- **buff deps / buff outdated** (T128): dependency-tree inspection and outdated-check (deferred `buff audit` security scan to v2.0).

## [1.5.0] - 2026-07-19

### Added

- **buff-eval**: Shared evaluation engine extracted (`crates/buff-eval/`) — the reuse pivot for REPL (T125), Jupyter (T129b), and Bufflings (T138c). Pure thin orchestration over existing lexer/parser/types/codegen primitives; no new compilation logic.
- **buff-repl**: REPL core (`crates/buff-repl/`) using rustyline 15. Session state persists across eval lines; `:type <expr>` introspection returns inferred type without evaluating; `:load <file>` accumulates decls into session state; multi-line input; history to `~/.buff_history`.

## [1.4.0] - 2026-07-19

### Added

- **Stdlib expansion (T124b–m)**: 12 stdlib modules registered in the 4527-line `prelude_types.rs` extensible registry, codegen-lowered to mature Rust crates:
  - **DateTime** (chrono), **Log** (tracing), **Regex** (regex), **Toml** (toml)
  - **Math/Random/Sort/Strings**, **Args/Env/input/sleep**
  - **URL/Base64/Hex/URLEncode/UUID**, **Yaml/Csv**
  - **Path/Dir/Tempfile**, **Hash/HMAC** (sha2/md5/hmac), **Process/OS**, **TCP/UDP/WebSocket** (tokio/tokio-tungstenite)
- **Stable error codes (T124)**: E10xx (lex) / E11xx (parse) / E12xx (type) / E13xx (codegen) registry in `buff-lang-error`. Codes are STABLE FOREVER (never renumber/reuse/silently-remove). 29 static HTML pages under `docs/errors/E*.html` with explanations + examples.
- **Git-based dependencies (T122)**: `buff add <git-url>` clones + caches for source builds.
- **Workspace support (T123)**: `buff` in a workspace root with `members = [...]` runs `cargo` passthrough.

## [1.3.0] - 2026-07-18

### Added

- **extern bindgen (T119)**: `extern "C" from "serde_json" func parse(s: String) -> Value` syntax — Buff's existing `extern` keyword (one of the v1.0 reserved 25) made functional. Codegen emits Rust `extern "C"` foreign-mod blocks + `#[link(wasm_import_module)]` for Wasm. Rejects generics with clear error.
- **Cargo polish (T120)**: `buff build` (release-mode optimization flags), `buff clean`, `buff update`. Idempotent `Cargo.toml` generation via `buff.toml` manifest.
- **Example library (T121)**: 14 side-by-side Rust-vs-Buff example pairs under `examples/rust-vs-buff/` covering borrow-checker, lifetimes, async/await, error handling, pattern matching, iterators, structs, traits, generics, concurrency, closures, collections, enums, recursion, null safety.
- **Dioxus feasibility spike (T121b)**: Throwaway spike — emitted `rsx!{}` via existing `buff-lang-codegen-rust`, rendered a counter in browser. Verdict **PASS** (`.sisyphus/decisions/dioxus-feasibility.md`) unlocked T130 UI work.

## [1.2.0] - 2026-07-19

### Added

- **buff-lsp**: Language Server Protocol server (`crates/buff-lsp`) with diagnostics, hover, completion, single-file goto-definition, document symbols, and formatting (via `buff fmt`). Communicates over stdio. Includes a typecheck-only analysis mode that runs the `TypeInferencer` without Rust codegen.
- **VSCode extension**: Editor support in [`editors/vscode/`](editors/vscode/) with TextMate syntax highlighting derived from the tree-sitter-buff grammar, automatic `buff-lsp` integration, `buff.run`/`buff.build`/`buff.check` commands, 16 code snippets, and format-on-save. Packaged as `buff-vscode-1.2.0.vsix`.

## [1.1.0] - 2026-07-19

### Added

- **Playground**: Wasm-based transpile-only playground (`playground/`) that runs the lexer, parser, and Rust codegen entirely in the browser. Includes URL-fragment sharing (`#s=<base64>`) so snippets link directly to an editor state. No runtime or GPU code is bundled, keeping the payload to ~2.3 MB.
- **tree-sitter grammar**: Full [tree-sitter-buff](tree-sitter-buff/) grammar with an external C scanner for the offside-rule INDENT/DEDENT/NEWLINE tokens. Ships highlight, fold, indent, and local queries. All 55 corpus tests pass, and every shipped `.buff` example parses without error nodes.
- **Website**: Static landing page (`website/`) with a "Rust performance with Go productivity" hero, six side-by-side Rust-vs-Buff code examples, and "Try this" links that load directly into the playground.

## [1.0.0] - 2026-07-16

- Heterogeneous CPU/GPU computing (Rayon + wgpu runtime), WGSL shader codegen, CLI tooling (`buff run` / `buff build` / `buff check` / `buff fmt` / `buff new` / `buff init`), lossless AST with trivia preservation, recursion detection, and pipeline caching.

## [0.5.0]

- Full type system, modules (`import`/`export`), async functions, closures, pattern matching, collections, error handling with `Result`/`?`, and the `buff fmt` formatter.

## [0.1.0]

- End-to-end transpilation proof of concept: hand-rolled lexer with offside rule, recursive-descent + Pratt parser, type inference, Rust codegen via `syn`/`quote`, and the `ola.buff` hello-world runs to native.

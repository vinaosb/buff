# Changelog

All notable changes to the Buff transpiler are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

> **Note on v1.13+ versioning:** tags `v1.13.0`–`v1.23.0` mark waves of
> framework-crate work (the `buff-v1x-frameworks` roadmap). A given tag
> may bundle several crate MVPs; the entries below attribute work to the
> wave where it landed. Crate `version` fields in each `Cargo.toml` track
> the SDK tier (`1.0.0` for tooling/framework crates, `1.2.0`/`2.0.0` for
> the core compiler crates) and are bumped independently of release tags.

## [1.25.0] - 2026-07-23

### Added

- **Generics + monomorphization** (T13): user-defined generic types and functions — `struct Pair<T, U>`, `func id<T>(x: T) -> T`. Full pipeline support across AST type-param lists, lexer/parser, type-inference resolution, and codegen lowering to Rust generics.
- **`buff bench` subcommand** (T22): benchmark harness with baseline capture for tracking compile-time and runtime regressions across releases.
- **Multi-span diagnostics** (T1): `SpanLabel` with secondary spans — a single diagnostic can now annotate multiple source locations (e.g. "borrowed here" / "used here").
- **Fix suggestions** (T1): `CodeSuggestion` with `Applicability` rating — diagnostics propose machine-applicable fixes, surfaced to editors as LSP `CodeAction`s.
- **`--error-format json`** (T1): machine-readable diagnostic output mode for editor and tooling integration.
- **Dynamic workload-aware dispatch** (T5): runtime `WorkloadContext` + `decide_dynamic` selects CPU vs GPU at execution time based on live workload characteristics, extending the static `@prefer` hint.
- **`--explain` flag** (T6): emits human-readable dispatch diagnostics explaining why a function ran on CPU or GPU.
- **`--linker {auto,mold,lld,system}` flag** (T2): fast-linker defaults (rust-lld / mold) with graceful fallback to the system linker for faster dev link times.
- **Code-hygiene audit** (T104): inventory published at `.sisyphus/audits/code-hygiene-v1.25.md`.
- **AGENTS.md refresh** (T108): root knowledge base updated for the 69-crate workspace.
- **Direction decision record** (T110): strategic moat analysis at `.sisyphus/decisions/buff-direction-speed-moat-selfhost.md`.
- **Error doc pages** (T52): missing catalog pages added for E1110, E1210, E1211, E1212, E1304.
- **Memory-safety statement** (T59): `MEMORY_SAFETY.md` added documenting Buff's memory-safety guarantees.
- **CI arm64 matrix** (T61): continuous integration now builds and tests `aarch64` targets across the 3-OS matrix.
- **Community health files** (T116): `CODE_OF_CONDUCT.md`, `SECURITY.md`, issue/PR templates, and Dependabot configuration added.

### Changed

- **Dev debuginfo default** (T3): dev builds now default to `--debuginfo=line-tables-only` for faster incremental builds; full debuginfo remains available via an explicit flag.
- **Unified generics representation:** `EnumDecl` migrated from `generics: Vec<Ident>` to `type_params: Vec<TypeParam>`, aligning enums with structs and functions under a single type-parameter model.

### Fixed

- `buff-plugins`: restored `Send`/`Sync` bounds on trait objects (pre-existing regression).
- `buff-game`: corrected `AudioBuffer` public-function visibility to respect the 40-fn cap (F2 M2 finding).
- `buff-pubsub`: closure return-type mismatch (F3 finding, fixed in v1.24.0).
- `buff-web`: missing `IntoResponse` import (F3 finding, fixed in v1.24.0).
- `buff-xml`: `quick-xml` 0.37 API drift (F3 finding, fixed in v1.24.0).
- `buff-plugins`: missing `RuntimePlugin` import (F3 finding, fixed in v1.24.0).

## [1.24.0] - 2026-07-23

### Added

- **T28 iterative audit (Wave 12):** comprehensive documentation & codebase refinement pass that iterates until a full scan finds zero new trivial issues. Owns the v1.24.0 release.
  - `CHANGELOG.md` backfilled with v1.13.0–v1.23.0 entries (this release).
  - `README.md` status table extended through v1.24; examples table extended with the v1.14–v1.23 framework examples (tensor, pipeline, science, ml, game, integration, data-science-workbench).
  - Per-crate `AGENTS.md` added for `buff-observe` (required) plus `buff-msgpack`, `buff-xml`, `buff-http-client`, `buff-dap`, `bufflings`, `buffup`; per-crate `README.md` added for `buff-observe`.
  - Root `AGENTS.md` + `CONTRIBUTING.md` refreshed to the v1.24 crate count and current conventions.

### Fixed

- `CONTRIBUTING.md` outdated "9-crate workspace" + stale "CI runs clippy without `--all-targets`" note corrected (CI enforces `--all-targets`).

### Decision

- Non-trivial findings (5 T22-FU\* codegen-layer gaps, buff-ml bias-gradient 3x factor, `DataFrame.to_json`, multi-threaded `Signal`, full code-comment version-scoping audit) logged to `.sisyphus/decisions/v1.24-followup.md` for v1.25+ planning — not implemented in this release per the 5000-LOC cap.

## [1.23.0] - 2026-07-23

### Added

- **T23 flagship — Data Science Workbench:** integration example composing `buff-dataframe` + `buff-ml` + `buff-pipeline` + `buff-web` + `buff-reactive` into a single notebook-style app.
- **T22 API compatibility spike:** 4 integration examples (`dataframe_to_json`, `tensor_to_web`, `pipeline_with_dataframe`, `reactive_to_web`) + mismatch report at `.sisyphus/decisions/api-compat-v20.md` documenting 5 codegen-layer gaps (Type variants + lowering arms).

### Fixed

- **Lexer-compat:** converted `#` line comments to `//` across 64 `.buff` files (Buff lexer only supports `//` and `/* */`).
- **Antipattern sweep:** `elif` → `else if`, `var` → `let mut`, `Tempfile.create` → `Type.new` across examples.
- `buff-ml` clippy lints, fmt, example API, proptest assertions (T15 follow-up).
- `buff-pipeline` clippy lints, snapshot format, `Sender` clone workaround (T14 follow-up).

## [1.22.0] - 2026-07-22

### Added

- **T13 buff-science:** MVP linear algebra, ODE integration, and statistics via extern `nalgebra`.
- **T14 buff-pipeline:** MVP DAG pipelines with `Channel`-based inter-stage queues + parallel workers.
- **T15 buff-ml:** MVP reverse-mode autodiff, layers (Linear/ReLU/Sigmoid/Softmax/Dropout), losses, SGD/Adam optimizers, training loop — built on `buff-tensor`.
- **T16 buff-game:** MVP game loop, asset pipeline, rendering (40-fn cap).

### Fixed

- `buff-lang-runtime`: corrected `Sender<T>` `Clone` bound.
- `buff-lang-error`: use `.contains()` instead of `.iter().any` (clippy `manual_contains`).

## [1.21.0] - 2026-07-22

### Added

- **T72 buff-plugins:** plugin architecture (compiler + LSP + runtime trait objects).
- **T70 registry:** package quality signals (verified/maintained/tested/documented badges).
- **T69 onboarding:** 4 tailored guides by developer background.
- **T68 cookbook:** 50+ recipe-style patterns guide.
- **T71 stability:** formal stability promise document (`stability-promise.md`).

## [1.20.0] - 2026-07-22

### Added

- **T64 buff watch:** file watcher + auto-rebuild.
- **T66 refactoring tools:** `buff refactor rename/extract/inline`.
- **T63 error:** suggestion engine + rustc→Buff span mapping + error docs.
- **T67 docs:** documentation site with Zola + 10-page content seed.
- PGO (profile-guided optimization) support; cold-start benchmark suite (`buff bench-cold-start`).
- Multiple-dispatch specificity fix (exact match wins over widened).

## [1.19.0] - 2026-07-21

### Added

- **Multiple dispatch** for numerical APIs (Julia-inspired).
- **Mathematical syntax edition** (Julia-inspired, opt-in).
- **Property wrappers** `@State`/`@Published`/`@Cached` (Swift-inspired).
- **buff-actors:** MVP actor model + supervisor trees (Gleam/Erlang-inspired).
- **buff-simd:** MVP `Simd<T,N>` first-class SIMD types (Mojo-inspired).
- CLI compile-speed optimization program (caching + mold + sccache + bench).
- CLI binary size minimization (`--minimal` flag + `profile.minimal`).

## [1.18.0] - 2026-07-21

### Added

Wave 6 — interop / protocol crates (all pure-Rust, FFI-guide compliant):
- **buff-crypto-extras** (T49): AES/RSA/ECDH/Argon2/RsaKeypair.
- **buff-web3** (T48): Provider/Wallet/Contract/ContractMethod.
- **buff-chat** (T47): Bot/Message/Platform.
- **buff-protobuf** (T52): Protobuf/Message.
- **buff-xml** (T50): Xml/XmlDocument/XmlElement (wraps `quick-xml`).
- **buff-nlp** (T46): Text/Language/StemAlgorithm.
- **buff-geo** (T45): Point/LineString/Polygon.
- **buff-msgpack** (T51): MessagePack binary format via `rmp-serde`.
- Prelude + codegen lowering wired for all of the above.

## [1.17.0] - 2026-07-20

### Added

Wave 5 — text / data crates:
- **buff-scrape:** MVP HTML parsing + crawling via `scraper`+`reqwest`.
- **buff-i18n:** MVP internationalization via `fluent`.
- **buff-archive:** MVP zip/tar/gz/zstd compression.
- **buff-fsm:** MVP state machine library.
- **buff-pubsub:** MVP in-process event bus.
- **buff-fake:** MVP fake data generation via the Faker crate.
- **buff-assertions:** MVP fluent test assertions.

## [1.16.0] - 2026-07-20

### Added

Wave 4 — infrastructure crates:
- **buff-cache:** MVP in-memory + distributed cache.
- **buff-auth:** MVP JWT + OAuth2 + password hashing + RBAC.
- **buff-validate:** MVP declarative schema validation.
- **buff-resilience:** MVP retry + circuit breaker + rate limiter + timeout.
- **buff-http-client:** MVP idiomatic HTTP client via `reqwest`.
- **buff-jobs:** MVP background job queue + scheduler.
- **buff-cli:** MVP CLI framework for user programs.
- **buff-config:** MVP layered config with hot reload.

## [1.15.0] - 2026-07-20

### Added

Wave 3 — server / runtime crates:
- **buff-fuzz:** MVP property-based fuzzing framework.
- **buff-web:** MVP HTTP server with routing + middleware via extern `axum`.
- **buff-db:** MVP connection pool + query builder via extern `sqlx`.
- **buff-reactive:** MVP signals, computed, effect callbacks.
- **buff-audit:** MVP security scanning + code signing.
- **buff-observe:** MVP structured spans + metrics via extern `tracing`+OTLP.
- **buff-template:** MVP HTML templating via extern `handlebars`.

## [1.14.0] - 2026-07-20

### Added

Wave 2 — data / numerical crates:
- **buff-dataframe:** MVP columnar DataFrame + CSV/JSON load + relational ops.
- **buff-tensor:** MVP N-dimensional arrays (rank ≤ 4) with matmul + reduce + GPU dispatch.
- **buff-image:** MVP image framework via extern `image` crate.
- **buff-audio:** MVP `AudioBuffer` framework.
- **buff-ecs:** MVP World, spawn, query, systems via extern `hecs`.
- **buff-dsp:** MVP FFT, filters, windows via extern `rustfft`.
- **buff-mock:** MVP mocking framework with expect/verify/spy.

## [1.13.0] - 2026-07-20

### Added

- **T53 comptime:** AST + parser skeleton + type-level interpreter + codegen lowering (Zig-inspired); 5 stable error codes + 5 examples + 22 tests.
- **`Channel<T>` MPSC primitive** (Stream/select deferred to v1.18+).
- **T1 multi-file project linking** end-to-end (workspace + cross-file `import`/`export`).
- **Buff-span stack traces** via source map + panic hook (`buff-lang-debug-info`).
- **buff.toml v2 schema** (workspace, features, lints, profiles, prelude, edition).
- Workspace support + `workspace.dependencies` inheritance.
- 7 built-in templates for `buff new --template`; `buff gen` module/test/example generators.
- Attributes: `@internal`, `@deprecated`, `@bench`, `@property`, `@should_panic`, `@ignore`, `@feature` (conditional compilation).
- Stability badge + `@deprecated` warning in `buff check`.
- **Buff SDK 2.0 conventions specification** (`sdk-conventions-v1x.md`).

### Decision

- Macro system (v1.13–v1.17) verdict: **DEFER-POST-v1.17** (`.sisyphus/decisions/macro-system-v1x.md`).
- WGSL extensibility assessment for v1.13–v1.17 frameworks (`.sisyphus/decisions/wgsl-extensibility-v1x.md`).

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

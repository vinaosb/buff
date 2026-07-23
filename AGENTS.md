# PROJECT KNOWLEDGE BASE

**Generated:** 2026-07-23 (v1.24 audit refresh by T28; originally v0.1 → v1.0 → v1.9 → v1.24)
**Commit:** 0bc3f17 (`v1x-frameworks`, T28 v1.24 audit in progress; v1.0-v1.23 shipped)
**Branch:** `v1x-frameworks` (tags: v0.1.0, v0.5.0, v1.0.0 … v1.23.0; v0.1-dev preserved as historical marker; v1.24.0 tagged at T28 completion)

## OVERVIEW

Buff — high-level language that transpiles `.buff` → Rust → native via rustc/LLVM (and `.buffhtml` SFC → Dioxus 0.7 component → wasm32-unknown-unknown). Implemented as a **60+-crate** Rust workspace (`members = ["crates/*"]` glob): the **core compiler** (~10 `buff-lang-*` crates), the **tooling** crates (LSP/REPL/Jupyter/registry/playground-wasm/ui-dioxus/buffup/bufflings/buff-dap), and the **framework** crates shipped across v1.13–v1.23 (`buff-{dataframe, tensor, image, audio, dsp, ecs, science, pipeline, ml, game, web, db, reactive, observe, …}`). Hides borrow-checker pain from users; compiler emits only "easy" Rust. v1.0–v1.12 shipped (production compiler + Try/Use Buff + stdlib + REPL + registry + Jupyter + UI/RSX + education + distribution). v1.13–v1.24 shipped (Buff SDK 2.0 foundations + framework-crate waves 2–11 + this v1.24 audit/polish release). Next: v1.25+ launch readiness (see `.sisyphus/plans/buff-launch-readiness.md`).

## STRUCTURE

```
buff/
├── crates/                              # 60+ workspace members via `members = ["crates/*"]` glob
│   ├── buff-lang-error/                 # LEAF: Span + Diagnostic + SourceMap + ErrorCode (depended on by all)
│   ├── buff-lang-ast/                   # Pure AST data nodes (decl/expr/stmt/ty/op/ir/lossless) + T57 byte-exact roundtrip
│   ├── buff-lang-lexer/                 # Hand-rolled byte-scanner + offside-rule indent tracker
│   ├── buff-lang-parser/                # Hand-rolled recursive-descent + Pratt (NOT chumsky)
│   ├── buff-lang-types/                 # Type inference + 12-module analysis suite + prelude + prelude_types (T124b 4527-line stdlib registry)
│   ├── buff-lang-codegen-rust/          # AST → syn::File → prettyplease → Rust source (+ race/atomic/gpu_alignment analyses + T124 stdlib lowering); rust_codegen.rs is ~17k lines (largest file in the workspace)
│   ├── buff-lang-codegen-wgsl/          # AST → WGSL GPU shaders (T44; the ONE format!() exception to no-raw-string-codegen rule)
│   ├── buff-lang-codegen-buffhtml/      # RSX template AST → rsx!{} TokenStream (T133-T135; post-format SpanMap side-table)
│   ├── buff-lang-ast-rsx/               # Pure-data AST for .buffhtml SFC (T133; sibling to buff-lang-ast, separate blast radius)
│   ├── buff-lang-buffhtml-parser/       # Hand-rolled 3-mode lexer + recursive-descent for .buffhtml (T133)
│   ├── buff-lang-runtime/               # Heterogeneous compute host: rayon + wgpu + tokio (~170KB, 11 src files)
│   ├── buff-lang-debug-info/            # Buff-span stack traces via SourceMap + panic hook (T1)
│   ├── buff-lang-cli/                   # Binary + library: pipeline orchestration (21 subcommands, ui_dev/, coverage/)
│   ├── buff-lsp/                        # LSP server v1.2 (lsp-server 0.10 + lsp-types 0.97; stdio; full-reparse)
│   ├── buff-eval/                       # T125-prep thin eval engine (REPL + Jupyter consumer)
│   ├── buff-repl/                       # T125a REPL (rustyline 15; wraps buff-eval)
│   ├── buff-jupyter/                    # T129 Jupyter kernel (pure-Rust zeromq 0.4, 5 sockets + HMAC)
│   ├── buff-registry/                   # T126-T127 package registry HTTP server (axum 0.8 + semver, in-memory storage)
│   ├── buff-playground-wasm/            # T114 wasm-transpile-only entry (cdylib+rlib; no runtime/GPU/rustc)
│   ├── buff-ui-dioxus/                  # T130 in-tree Dioxus 0.7 wrapper (component runtime + T135 SSR via dioxus-ssr)
│   ├── buffup/                          # T139 Rust toolchain-style version manager (v1.12)
│   ├── bufflings/                       # T138c Rustlings-style exercise runner (v1.11)
│   ├── buff-dap/                        # T60/T136 Debug Adapter Protocol translation proxy (v1.10)
│   ├── buff-cli/                        # CLI framework for user programs (Wave 4)
│   ├── buff-lang-ffi-guide/             # GUIDE.md: 6 hard rules for all extern wrapper crates
│   └── buff-{framework crates}/         # v1.13–v1.23 framework MVPs (50+ crates): dataframe, tensor, image, audio,
│                                        #   dsp, ecs, science, pipeline, ml, game, mock, audit, fuzz, web, db, template,
│                                        #   reactive, observe, cache, auth, validate, resilience, http-client, jobs,
│                                        #   config, scrape, i18n, archive, fsm, pubsub, fake, assertions, crypto-extras,
│                                        #   web3, chat, protobuf, xml, nlp, geo, msgpack, actors, simd, plugins, …
├── crates-io/                           # EMPTY (reserved for future crates.io publishing)
├── examples/                            # .buff + .buffhtml + rust-vs-buff/ side-by-side comparisons
├── tests/                               # Golden .buff fixtures (valid/ + invalid/) + snapshot README
├── tree-sitter-buff/                    # T115 tree-sitter grammar (grammar.js → generated parser.c + hand-written scanner.c for offside-rule)
├── editors/vscode/                      # T118 VSCode extension (TypeScript → out/extension.js; bundles buff-lsp + TextMate + snippets)
├── website/                             # v1.1 static landing page (HTML/CSS/JS, no build step, playwright tests)
├── playground/                          # v1.1 static transpile-only playground (HTML/CSS/JS + pkg/buff_playground_bg.wasm)
├── docs/                                # Generated error pages (docs/errors/E*.html) + component-model/extern-guide markdown
├── .sisyphus/                           # Project orchestration: boulder.json + plans/ (10 files) + decisions/ + evidence/ + notepads/
├── .github/workflows/ci.yml             # 3-OS matrix: fmt --check + clippy --all-targets -D warnings + test
├── Cargo.toml                           # Pure workspace (no [package]); ~50 deps centralized in [workspace.dependencies] with T-numbered rationale
└── rust-toolchain.toml                  # Pin: 1.95.0 + rustfmt + clippy
```

## WHERE TO LOOK

| Task | Location | Notes |
|---|---|---|
| Add a CLI subcommand | `crates/buff-lang-cli/src/cli.rs` (Command enum) + `commands/<name>.rs` + `main.rs` dispatch arm | 21 subcommands today; new variant + new commands/ file + new main.rs match arm |
| Add a new AST node | `crates/buff-lang-ast/src/{decl,expr,stmt,ty}.rs` | Ripple: parser + types + codegen-rust (+ codegen-wgsl if GPU-relevant) |
| Add a new RSX/`.buffhtml` node | `crates/buff-lang-ast-rsx/src/lib.rs` | Ripple: buffhtml-parser + codegen-buffhtml |
| Add a TokenKind | `crates/buff-lang-lexer/src/token.rs` → `lexer.rs` → parser `stream.rs` | Also check `regex_context()` `/`-disambiguation |
| Add a prelude/builtin fn (free) | `crates/buff-lang-types/src/prelude.rs` (PreludeFn + return_type) + codegen-rust `lower_prelude_call` | Implicit — no `import` needed |
| Add a prelude type (DateTime/Regex/URL/etc) | `crates/buff-lang-types/src/prelude_types.rs` (PreludeType + PreludeAssocFn + PreludeInstanceFn) + codegen-rust `lower_prelude_type_assoc_fn` + `extern_crates` BTreeSet | THE 4527-line registry every T124 stdlib task extends |
| Add a `buff check` lint | `crates/buff-lang-cli/src/naming_lint.rs` + `check.rs::check_source` | Standalone typecheck T55 shipped; no codegen needed |
| Add an LSP capability | `crates/buff-lsp/src/handlers.rs` + `server.rs` capability registration | Pure handlers; only server.rs has I/O |
| Add a registry endpoint | `crates/buff-registry/src/handlers.rs` + `lib.rs::app()` route arm | axum 0.8 `{name}` path syntax |
| Add buff-ui component lifecycle hook | `crates/buff-lang-codegen-buffhtml/src/prop_check.rs` + `lib.rs` + `crates/buff-ui-dioxus/src/lib.rs` | T134 pre-checker + lowering + runtime API |
| Add an error variant | `crates/buff-lang-error/src/span.rs` (or `code.rs` for ErrorCode) | Leaf crate, ripple everywhere; ErrorCodes are STABLE forever once shipped |
| Add a snapshot test | `crates/<crate>/tests/*.rs` + commit the `.snap` | Per-crate `tests/snapshots/` |
| Add a `.buffhtml` example | `examples/<name>.buffhtml` | SFC: `<script>buff</script>` + `<template>RSX</template>` + `<style>CSS</style>` |
| Find phase status (v0.1/v0.5/v1.0/v1.x) | `.sisyphus/plans/buff-{v01,v05,v10,post-v10-tooling,v1x-frameworks}-*.md` | Master: `buff-master.md` |
| Find the conventions doc | `.sisyphus/plans/buff-conventions.md` | 18+ conventions for BUFF LANGUAGE (not Rust) |

## CODE MAP — Pipeline

```
.buff source                .buffhtml SFC
    │                            │
    ▼ read_to_string             ▼
buff-lang-lexer::tokenize    buff-lang-buffhtml-parser (3-mode lexer + parser)
    │                            │
    ▼                            ▼
buff-lang-parser::parse      buff-lang-ast-rsx (RsxTemplateFile AST)
    │                            │
    ▼                            ▼
buff-lang-ast (Vec<Decl>)    buff-lang-codegen-buffhtml
    │                       (lowers to rsx!{} TokenStream + SpanMap)
    │                            │
    ├─ types analyses ───────────┤
    │  (async, ownership,        │
    │   recursion, exhaust,      │
    │   modules, range)          │
    │                            │
    ▼                            ▼
buff-lang-codegen-rust::generate_rust  (type inference INSIDE codegen; race/atomic/gpu_alignment pre-passes)
    │                            │
    │ (also lowers prelude_calls,     │ (also emits #[component] + rsx!{}
    │  prelude_type_assoc_fns,        │  via T121b-proven pattern)
    │  populates extern_crates)       │
    │                            │
    ▼                            ▼
syn::File → prettyplease::unparse → String
    │
    ▼  pipeline::compile_rust_to_exe   (rustc --edition 2021)
native executable
    │
    ▼  (parallel: buff-ui-dioxus / Dioxus 0.7 for UI apps)
wasm32-unknown-unknown (UI)   OR   native binary (CLI/server)


WGSL PARALLEL PATH (GPU-dispatched fns only):
buff-lang-ast::Expr (map lambda)
    │
    ▼
buff-lang-codegen-wgsl::generate_wgsl   (T44; format!() exception; one-param numeric lambdas only)
    │
    ▼
buff-lang-runtime (T38-T50; rayon CPU + wgpu GPU + tokio async; @prefer(gpu) hints + arithmetic intensity threshold)
```

**Pipeline wiring**: lexer → parser → codegen-rust → rustc. Type errors are surfaced via `buff check` (T55 standalone typecheck SHIPPED in v1.0 — runs TypeInferencer directly, no codegen). WGSL codegen + runtime shipped in v1.0. RSX `.buffhtml` pipeline shipped in v1.9 (T133-T135). Multiple consumers: CLI, LSP, REPL, Jupyter, playground-wasm all reuse the same front-end crates.

## CONVENTIONS

- **Workspace dependency resolution**: every crate uses `dep.workspace = true`. NEVER pin a version in a crate `Cargo.toml` — add to root `[workspace.dependencies]` first (heavily documented with T-numbered rationale).
- **Edition 2021, license `MIT OR Apache-2.0`**. Two version tiers: `1.2.0` (10 core compiler crates), `1.0.0` (9 tooling crates: eval/repl/jupyter/registry/lsp/playground-wasm/ui-dioxus/ast-rsx/buffhtml-parser/codegen-buffhtml).
- **Rust crate naming**: `buff-lang-<thing>` (compiler crates, hyphen) → `buff_lang_<thing>` (underscore) → import `buff_lang_<thing>::...`. Tooling crates use `buff-<thing>` (no `lang` infix) → `buff_<thing>`.
- **Derive defaults**: `Debug, Clone, PartialEq` (+ `Eq, Hash` when used in maps/sets).
- **Errors**: `thiserror::Error` derive; map to `buff_lang_error::*Error` variants. ErrorCodes (E10xx lex / E11xx parse / E12xx type / E13xx codegen) are STABLE FOREVER — never renumber/reuse/silently-remove.
- **Tests in per-crate `tests/`** (not src). Inline `#[cfg(test)]` ok for unit smoke tests.
- **No `[features]`, `[lints]`, `[profile.*]` sections** in any Cargo.toml. No crate-level `#![deny(...)]` / `#![forbid(unsafe_code)]` (CI enforces via `cargo clippy --workspace --all-targets -- -D warnings`).
- **Conservative pin philosophy**: pin to long-standing stable majors (rand 0.8 NOT 0.9, chrono 0.4, rustyline 15, dirs 5, zeromq 0.4). Documented inline in root Cargo.toml.
- **Pure-Rust preference**: reqwest uses `rustls-tls` (NOT native-tls); zeromq (NOT zmq which links C libzmq); no diesel/libpq/S3 SDK in registry. Matches the "no C library, no Docker" hard rule from T126/T127 task specs.

## ANTI-PATTERNS (THIS REPO)

- ❌ **Raw-string Rust codegen** — must build via `syn`/`quote`, format via `prettyplease`. The single string producer is `prettyplease::unparse` in `crates/buff-lang-codegen-rust/src/format.rs`. **Exception**: `crates/buff-lang-codegen-wgsl/src/shader.rs` uses `format!()` for WGSL (no syn equivalent for WGSL — documented inline).
- ❌ **`unwrap`/`expect`/`panic!`/`unimplemented!`/`todo!`** in non-test code (hard rule from README).
- ❌ **Tabs** — Buff lexer rejects them (4 spaces only).
- ❌ **`_async` suffix** on async functions (Buff language rule §6).
- ❌ **Positional boolean args** — Buff mandates named args: `fetch(url, cache: true)` (§11).
- ❌ **`new Person()` / `Person.create()` / `Person.build()`** — Buff constructors are `Type.new()` / `Type.from()` only (§7).
- ❌ **Re-introducing chumsky or logos** — both removed at commit 9af2f5c (chumsky 1.0.0-alpha.8 needed stacker → cc-rs → C shim that failed on Windows; same root cause pushed the lexer to hand-roll).
- ❌ **Committing `.snap.new` / `.pending-snap`** — those are insta pending files (gitignored).
- ❌ **Populating `crates-io/`** — currently empty/reserved; do not add without coordination.
- ❌ **Trailing whitespace, >2 consecutive blank lines, missing trailing commas in multi-line collections** (Buff fmt rules).
- ❌ **Renumbering/reusing/silently-removing/back-filling ErrorCode variants** — stability guarantee (§19 of conventions).
- ❌ **Editing `tree-sitter-buff/src/parser.c` by hand** — it is GENERATED from grammar.js via `tree-sitter generate`.
- ❌ **Forking Dioxus** — it is a vendored upstream dependency. Re-test on every minor bump (dioxus-rsx proc-macro internals NOT covered by semver).

## UNIQUE STYLES

- **PT-BR example names**: `ola`, `calculadora` — keep the convention when adding Portuguese-language examples. English for feature demos (fibonacci, closures, etc).
- **`.sisyphus/` orchestration**: plans track v0.1/v0.5/v1.0/v1.x task breakdown. `boulder.json` is active session state. Read `buff-conventions.md` for Buff-language rules. `evidence/` dir is gitignored.
- **Dual bin+lib in THREE crates** (cli/lsp/registry): `main.rs` (thin dispatch) + `lib.rs` (real logic). Lets integration tests drive the pipeline without subprocess.
- **`compile_to_rust` vs `compile_rust_to_exe`** split in `pipeline.rs`: callers can inspect intermediate Rust source before invoking rustc. `.buffhtml` adds `compile_buffhtml_to_rust` with a `SpanMap` side-table for reverse error mapping.
- **Standalone typecheck SHIPPED**: `buff check` (T55) at `buff-lang-cli/src/check.rs::check_source()` runs lex → parse → TypeInferencer → naming_lint WITHOUT codegen. (Earlier docs said "post-v1.0 work" — OUTDATED.)
- **Type-checking is ALSO INSIDE codegen** for `buff build`/`buff run` (TypeInferencer embedded in RustCodegen, consulted at each `let` binding; failures fall back to no annotation).
- **Prelude**: free fns (`print`, etc.) AND prelude types (`DateTime`, `Regex`, `URL`, `Hash`, `TCP`, etc) are implicit (no `import`). Type sigs in `buff-lang-types/src/prelude.rs` + `prelude_types.rs` (4527-line extensible registry). Codegen-lowered to mature Rust crates (chrono/tracing/regex/toml/rand/base64/sha2/hmac/tokio-tungstenite/etc).
- **Two parser entry points**: `parse()` fail-fast (production) + `parse_recovering()` accumulating (LSP/`buff check`). Both share `parse_one_decl()` dispatcher.
- **Three parse-time desugars** (no new AST nodes): `|>` (pipeline) → FuncCall; `?.` (null-conditional) → `and_then` MethodCall + Lambda; `??` (null-coalesce) → BinaryOp.
- **`buff-ui dev` server** (`crates/buff-lang-cli/src/ui_dev/`): WebSocket live reload + file watcher + Wasm builder (T131). `buff ssr` for server-side rendering (T135 via `dioxus-ssr`).
- **buffhtml SFC**: T133 RSX-for-Buff Option C. `<script>buff</script>` + `<template>RSX</template>` + `<style>CSS</style>` — hand-rolled 3-mode lexer, parallel pipeline to `.buff`, post-format SpanMap for rustc diagnostic reverse-mapping.
- **tree-sitter-buff is a DERIVED APPROXIMATION**: the authoritative parser is `buff-lang-parser` (hand-rolled Rust). The tree-sitter grammar (grammar.js + hand-written C scanner for offside-rule) exists for editor tooling (Neovim/Helix/Zed/GitHub highlighting). If they diverge, the Rust parser wins.
- **buff-jupyter pure-Rust ZMQ**: uses `zeromq` 0.4 (pure-Rust, async, tokio-based) NOT `zmq` (links C libzmq — fails on Windows). Same family of cc-rs avoidance that pushed hand-rolled lexer/parser.

## COMMANDS

```bash
# Build (release)
cargo build --release -p buff-lang-cli    # → target/release/buff[.exe]

# Run an example
cargo run -p buff-lang-cli -- run examples/ola.buff
cargo run -p buff-lang-cli -- run examples/fibonacci.buff    # → 55

# Run a .buffhtml UI app (T133+)
cargo run -p buff-lang-cli -- ui dev examples/<name>.buffhtml

# Scaffold a new project (21 subcommands total)
cargo run -p buff-lang-cli -- new my_app
cargo run -p buff-lang-cli -- run my_app/src/main.buff

# Standalone typecheck (T55 — NO codegen, fast)
cargo run -p buff-lang-cli -- check examples/ola.buff

# REPL (T125a)
cargo run -p buff-lang-cli -- repl

# Registry server (T126)
cargo run -p buff-registry

# LSP server (v1.2 — bundled by editors/vscode)
cargo build --release -p buff-lsp

# Jupyter kernel install (T129)
cargo run -p buff-lang-cli -- jupyter install

# SSR for .buffhtml (T135)
cargo run -p buff-lang-cli -- ssr examples/<name>.buffhtml

# Check / test / lint (what CI runs)
cargo check --workspace
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace

# Snapshot workflow (insta)
cargo insta review        # interactively accept pending snapshots
cargo insta accept        # accept ALL pending (use sparingly)

# tree-sitter regenerate parser (after editing grammar.js)
cd tree-sitter-buff && tree-sitter generate

# VSCode extension build (after editing editors/vscode/src/)
cd editors/vscode && npm run build
```

## NOTES

- **Toolchain mismatch**: `rust-toolchain.toml` pins `1.95.0`, but `.github/workflows/ci.yml` uses `dtolnay/rust-toolchain@master` with `toolchain: 1.95.0`. CI MAY diverge from local on dtolnay action master bumps.
- **CI clippy DOES include `--all-targets`** (CI line 18). CONTRIBUTING.md was updated in the T28 v1.24 audit to match CI; README.md "Building from source" omits `--all-targets` (cosmetic).
- **CI runs on 3 OSes**: ubuntu-latest, windows-latest, macos-latest.
- **`crates-io/` is empty** — reserved for future crates.io publishing workflow.
- **`buff.lock`** is gitignored — Buff's future lockfile (not yet generated).
- **Hand-rolled lexer/parser**: chumsky 1.0.0-alpha.8 transitively required `stacker` → `cc-rs` → C shim that failed on Windows hosts missing `excpt.h` from the Windows SDK. Same family of issues pushed the lexer to hand-roll. Unused `logos`/`chumsky` deps were removed in cleanup commit 9af2f5c.
- **buff-lang-codegen-wgsl is the ONE exception** to "no raw-string codegen": WGSL has no `syn` equivalent. Documented inline in `shader.rs`. Project rule still applies to all Rust codegen.
- **Dioxus 0.7 version drift risk**: `dioxus = "0.7"` and `dioxus-ssr = "0.7"` are workspace caret pins. dioxus-rsx proc-macro internals are NOT covered by semver guarantees — re-test on every minor bump. See `.sisyphus/decisions/dioxus-feasibility.md` (T121b spike).
- **`with_exe_extension` + `compile_rust_to_exe` logic is DUPLICATED** between `buff-lang-cli/src/pipeline.rs` and `buff-eval/src/lib.rs` (to avoid CLI pulling clap+tokio transitively into REPL/Jupyter). Keep the two copies in sync manually.
- **Three independent binaries**: `buff-lang-cli` (compiler), `buff-registry` (HTTP server), `buff-lsp` (LSP server). All dual bin+lib.
- **Two version tiers**: `1.2.0` for the core compiler crates (ast/lexer/parser/types/codegen-rust/codegen-wgsl/runtime/error) and `1.0.0` for the tooling + framework crates (eval/repl/jupyter/registry/lsp/playground-wasm/ui-dioxus/ast-rsx/buffhtml-parser/codegen-buffhtml + all `buff-{framework}` crates). `buff-dataframe` is pinned at `2.0.0` (API-bumped during its MVP). Bump these explicitly when cutting a release.
- **`coverage/` module in CLI** maps llvm-cov Rust-line coverage back to `.buff` source lines (T137). Not a subcommand — helper module.
- **`ui_dev/` module in CLI** is BOTH a module root AND the `buff ui dev` handler (T131). `commands/ui_dev.rs` is a thin dispatch wrapper.
- **WGSL binding contract** between `buff-lang-codegen-wgsl` and `buff-lang-runtime`: stable layout `@group(0) @binding(0)` input storage (read), `@group(0) @binding(1)` output storage (read_write). Workgroup size 64 default. Both crates MUST stay in sync.
- **See per-crate `AGENTS.md`** in every `crates/buff-{lang-*,lsp,eval,repl,jupyter,registry,playground-wasm,ui-dioxus}/` and `tests/AGENTS.md` + `tree-sitter-buff/AGENTS.md` + `editors/vscode/AGENTS.md` for detailed file-level guidance.
- **FFI Safety Guide**: `crates/buff-lang-ffi-guide/GUIDE.md` defines 6 hard rules for all `extern` wrapper crates. Wave 4 wrappers (T17-T21) and community bindings must comply.
- **`buff-cache` (T31) in-memory MVP only**: distributed Redis backend DEFERRED to v1.18+ per the T31 task spec ("If problematic, defer distributed to v1.18+ and ship in-memory MVP only"). The MVP wraps `moka::sync::Cache` (LRU + per-entry TTL via stored `Option<Instant>` deadlines). The `Cache.new(max_capacity)` / `cache.set(k, v, ttl: Duration)` surface is shaped so the future Redis backend is a drop-in (single `Backend::{Memory, Redis}` match arm per method). Cache invalidation pub/sub + multi-tier orchestration deferred to v1.22+. See `crates/buff-cache/AGENTS.md` "DEFERRED" section.
- **`buff-pubsub` (T41) in-process MVP only**: distributed pub/sub (Redis Pub/Sub, NATS, Kafka, RabbitMQ bridges) DEFERRED to v1.18+ per the T41 task spec ("In-process only — distributed pub/sub deferred to v1.18+"). The MVP wraps `crossbeam-channel` (per-subscription queue) + `tokio` (runtime-aware worker spawn — falls back to `std::thread::spawn` for sync use). The `EventBus.new() / bus.subscribe(topic, handler) / bus.publish(topic, payload) / bus.unsubscribe(id)` surface is shaped so the future distributed backend is a drop-in (single `Backend::{InProcess, Redis}` match arm per method, same migration shape as `buff-cache`). 10-fn cap met exactly: 3 on `Event` (new/topic/payload) + 7 on `EventBus` (new/subscribe/publish/unsubscribe/subscriber_count/topic_count/clear). Prelude+codegen wiring is a separate follow-up commit per the buff-image T9 two-commit precedent ("MVP first, wiring second"). Typed events (`Event<T>`), bounded channels, request/reply, topic wildcards, per-subscriber error callbacks all deferred to v1.18+. See `crates/buff-pubsub/AGENTS.md` "DEFERRED" section.

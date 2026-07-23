# Buff

> **Buff** — a high-level language that transpiles to Rust.
> Removes the "rust" (complexity), leaving pure performance.

> ✅ **v1.2 *Use Buff* shipped** — LSP server + VSCode extension.
> ✅ **v1.1 *Try Buff* shipped** — playground, tree-sitter grammar, website.
> ✅ **v0.1 *Olá, Buff* shipped** — transpiles & runs end-to-end.

---

## Why does this project exist?

Every modern language forces a painful trade-off:

| You want… | You pick… | You pay… |
|---|---|---|
| Maximum performance & memory safety | **Rust** | A brutal learning curve — fighting the borrow checker, annotating lifetimes |
| Simplicity & productivity | **Go / C# / Java** | A garbage collector: pauses, extra RAM, hidden overhead |
| Both | — | *"The Holy Grail"* — supposedly impossible |

**Buff exists to break that trilemma.** The bet:

> You can deliver Rust's performance *without* exposing the developer to the
> borrow checker — if the compiler, not the human, is the one arguing with Rust.

### The three ideas behind Buff

1. **Transpile, don't reimplement.** Buff is a source-to-source compiler
   (`.buff` → Rust → native binary via `rustc`/LLVM). It piggybacks on the
   engineering already sunk into `rustc` instead of reinventing codegen. The
   borrow checker becomes a *free safety reviewer* of generated code, never an
   obstacle the user sees.
2. **Hide memory management from the user.** No references (`&`), no visible
   lifetimes (`'a`), no manual pointers in Buff syntax. The transpiler emits
   only "easy" Rust — owned data, intelligent clones, `Arc`/copy-on-write where
   sharing is needed.
3. **Invisible heterogeneous computing.** The same Buff function can run on CPU
   *or* be dispatched to GPU automatically — the compiler analyzes arithmetic
   intensity and emits both a Rayon path **and** a WGSL shader, then the runtime
   picks at execution time. Optional hints like `@prefer(gpu)` nudge the
   decision but **never break** when hardware is absent.

### The Rust pain Buff refuses to inherit

- **No borrow-checker fights** — the user never sees a lifetime or ownership error.
- **No function-coloring problem** — no `await` keyword; `async` propagates up
  the call graph automatically. ~95% of user code never knows async exists.
- **No null pointers** — absence is `Option<T>`.
- **No class hierarchies** — OOP ergonomics via structs + traits + embedding.

> **In one sentence:** *Rust performance with Go productivity* — write clean,
> indentation-based code, get a binary that fans out across CPU cores and
> dispatches hot loops to the GPU, without writing a single lock, thread,
> shader, or lifetime annotation.

---

## Status

| Phase | Codename | Goal | State |
|---|---|---|---|
| **v0.1** | *Olá, Buff* | Prove transpilation end-to-end | ✅ Shipped |
| **v0.5** | *Real Language* | Full type system, modules, async, FFI | ✅ Core shipped |
| **v1.0** | *Production* | Heterogeneous CPU/GPU computing, tooling, release | ✅ Core shipped |
| **v1.1** | *Try Buff* | Playground, tree-sitter grammar, website — discover and try Buff | ✅ Shipped |
| **v1.2** | *Use Buff* | LSP server + VSCode extension — editor intelligence | ✅ Shipped |
| **v1.3** | *Rust interop* | `extern` bindgen, Cargo polish, side-by-side example library, Dioxus feasibility spike | ✅ Shipped |
| **v1.4** | *Stdlib + Cargo* | 12 stdlib modules (DateTime/Log/Regex/Toml/Math/Filesystem/Crypto/Networking/…), stable error codes with online catalog, git deps, workspace support | ✅ Shipped |
| **v1.5** | *REPL* | Shared `buff-eval` crate extracted; REPL with rustyline, session state, `:type` introspection, `:load`, multi-line input, history | ✅ Shipped |
| **v1.6** | *Registry* | Pure-Rust `buff-registry` HTTP server (axum + semver, in-memory store); `buff add`/`publish`/`install` CLI; `buff deps`/`outdated` | ✅ Shipped |
| **v1.7** | *Jupyter* | Pure-Rust ZMQ kernel (`buff-jupyter`); 5-socket protocol + HMAC; cross-cell state; rich MIME display + `?`/`??` introspection | ✅ Shipped |
| **v1.8** | *Web / frontend* | `buff-ui-dioxus` wrapper (Dioxus 0.7); `buff ui dev` WebSocket hot-reload server; `buff ui new --desktop` Tauri scaffolding | ✅ Shipped |
| **v1.9** | *RSX for Buff* | `.buffhtml` SFC pipeline (Option C: parallel format, no compiler changes) — AST + parser + codegen; component model with lifecycle hooks + typed props; SSR via `dioxus-ssr` | ✅ Shipped |
| **v1.10** | *Production hardening* | `buff-dap` Debug Adapter Protocol translation proxy; `buff coverage` Rust-line → `.buff` source-line mapping (llvm-cov/tarpaulin) | ✅ Shipped |
| **v1.11** | *Education* | `bufflings` Rustlings-style exercise runner (list/start/verify/progress/watch); 25 exercises across 11 topics; verification engine + CI solvability gate | ✅ Shipped |
| **v1.12** | *Distribution scale* | `buffup` Rust toolchain-style version manager; `setup-buff` GitHub Action (TypeScript, caches `~/.buff/versions/`); Docker `builder` + `slim` images with multi-arch buildx | ✅ Shipped |
| **v1.13** | *SDK 2.0 foundations* | Comptime (T53, Zig-inspired); `Channel<T>` MPSC; multi-file project linking (T1); buff-span stack traces; `buff.toml` v2 schema; 7 built-in templates + `buff gen`; attributes `@internal`/`@deprecated`/`@bench`/`@property`/`@feature` | ✅ Shipped |
| **v1.14** | *Wave 2 — data/numerical* | Framework crate MVPs: `buff-dataframe`, `buff-tensor`, `buff-image`, `buff-audio`, `buff-ecs`, `buff-dsp`, `buff-mock` | ✅ Shipped |
| **v1.15** | *Wave 3 — server/runtime* | Framework crate MVPs: `buff-fuzz`, `buff-web`, `buff-db`, `buff-reactive`, `buff-audit`, `buff-observe`, `buff-template` | ✅ Shipped |
| **v1.16** | *Wave 4 — infrastructure* | Framework crate MVPs: `buff-cache`, `buff-auth`, `buff-validate`, `buff-resilience`, `buff-http-client`, `buff-jobs`, `buff-cli`, `buff-config` | ✅ Shipped |
| **v1.17** | *Wave 5 — text/data* | Framework crate MVPs: `buff-scrape`, `buff-i18n`, `buff-archive`, `buff-fsm`, `buff-pubsub`, `buff-fake`, `buff-assertions` | ✅ Shipped |
| **v1.18** | *Wave 6 — interop/protocol* | Framework crate MVPs: `buff-crypto-extras`, `buff-web3`, `buff-chat`, `buff-protobuf`, `buff-xml`, `buff-nlp`, `buff-geo`, `buff-msgpack` (+ prelude/codegen wiring) | ✅ Shipped |
| **v1.19** | *Language surface* | Multiple dispatch (Julia-inspired); mathematical syntax edition; property wrappers `@State`/`@Published`/`@Cached`; `buff-actors`, `buff-simd`; CLI `--minimal` + compile-speed program | ✅ Shipped |
| **v1.20** | *DX tooling* | `buff watch` (T64); `buff refactor` rename/extract/inline (T66); error suggestion engine + rustc→Buff span mapping (T63); docs site (Zola, T67); PGO + cold-start benchmark | ✅ Shipped |
| **v1.21** | *Tooling & docs polish* | `buff-plugins` (T72); registry quality signals (T70); onboarding guides (T69); 50+ recipe cookbook (T68); formal stability promise (T71) | ✅ Shipped |
| **v1.22** | *Wave 10 — science/ML/game* | `buff-science` (T13, nalgebra); `buff-pipeline` (T14, DAG + Channel); `buff-ml` (T15, autodiff + layers + optimizers); `buff-game` (T16, loop/assets/render) | ✅ Shipped |
| **v1.23** | *Wave 11 — flagship* | T22 API-compat spike (4 integration examples + mismatch report); T23 Data Science Workbench flagship; lexer-compat `#`→`//` sweep + antipattern fixes | ✅ Shipped |
| **v1.24** | *Audit & Polish* | T28 iterative documentation & codebase refinement pass (convergence-gated); CHANGELOG backfill v1.13–v1.24; per-crate AGENTS.md for missing crates; root AGENTS.md + CONTRIBUTING refresh | ✅ Shipped |

**Compiles today:** hand-rolled lexer (byte-scanner + offside rule), hand-rolled
parser (recursive-descent + Pratt), AST with spans, type inference, Rust
codegen infrastructure with move-by-default semantics, error/source-map crate,
testing infrastructure (insta + proptest), CLI (`buff build` / `buff run` /
`buff new` / `buff init`).

**v0.1 exit criteria (met):**

- `buff run examples/ola.buff` → `Olá, Buff!`
- `buff run examples/fibonacci.buff` → `55` (recursive fib(10))
- `buff run examples/calculadora.buff` → `5` (add(2,3))
- `cargo test --workspace` → 100% pass
- `cargo clippy --workspace --all-targets -- -D warnings` → clean
- `buff new <NAME>` scaffolds a runnable project
- `buff init` scaffolds the current directory

Full task breakdown: [`.sisyphus/plans/`](./.sisyphus/plans/)

---

## Installation

The `buff` CLI can be installed from source:

```bash
cargo install --path crates/buff-lang-cli --locked
# Or, in a clone of this repo:
cargo install --path crates/buff-lang-cli --locked --force
```

A `cargo install buff-cli` flow (publishing to crates.io) is planned for a
future release.

## Quick start

```bash
git clone https://github.com/buff-lang/buff.git
cd buff
cargo test --workspace

# Run an example:
cargo run -p buff-lang-cli -- run examples/ola.buff

# v0.5 examples (collections, pattern matching, error handling, closures):
cargo run -p buff-lang-cli -- run examples/collections.buff
cargo run -p buff-lang-cli -- run examples/error_handling.buff

# Scaffold a new project:
cargo run -p buff-lang-cli -- new my_app
cargo run -p buff-lang-cli -- run my_app/src/main.buff
```

## Try it online

The repo ships two static assets you can deploy as-is:

- **Playground** ([`playground/index.html`](./playground/index.html)) — transpile Buff to Rust in the browser. Share code via URL fragments (`#s=<base64>`). No server needed, no runtime or GPU bundled.
- **Website** ([`website/index.html`](./website/index.html)) — landing page with side-by-side Rust-vs-Buff examples and links into the playground.

Both are plain HTML/CSS/JS with no build step. Deploy them as static sites to any host (GitHub Pages, Netlify, a bucket, wherever). Hosting URLs are not yet assigned; check back for links.

## Editor support

A VSCode extension is included in [`editors/vscode/`](./editors/vscode/) and ships as `buff-vscode-1.2.0.vsix`. It provides syntax highlighting, LSP-powered diagnostics, hover, completion, goto-definition, document symbols, formatting, and `buff.run`/`buff.build`/`buff.check` commands.

To build the LSP server: `cargo build --release -p buff-lsp`. To install the extension: `code --install-extension editors/vscode/buff-vscode-1.2.0.vsix`.

## Examples

| Example | Demonstrates | Status |
|---|---|---|
| [`examples/ola.buff`](./examples/ola.buff) | Hello world | ✅ v0.1 (runs) |
| [`examples/fibonacci.buff`](./examples/fibonacci.buff) | Recursion, typed params, arithmetic | ✅ v0.1 (runs) |
| [`examples/calculadora.buff`](./examples/calculadora.buff) | Function calls with multiple args | ✅ v0.1 (runs) |
| [`examples/closures.buff`](./examples/closures.buff) | Lambdas `{ x => ... }`, `.map()` combinators | ✅ v0.5 (runs) |
| [`examples/collections.buff`](./examples/collections.buff) | `Vector<T>`, `Map<K,V>`, `.pop()`/`.len()` | ✅ v0.5 (runs) |
| [`examples/pattern_matching.buff`](./examples/pattern_matching.buff) | `match`, `Option<T>`, `Result<T,E>` arms | ✅ v0.5 (runs) |
| [`examples/error_handling.buff`](./examples/error_handling.buff) | `Result`, `?` propagation, builtin `Error` | ✅ v0.5 (runs) |
| [`examples/prelude_demo.buff`](./examples/prelude_demo.buff) | Minimal `print(1+2)` prelude smoke test | ✅ v0.1 (runs) |
| [`examples/minimal_console.buff`](./examples/minimal_console.buff) | Smallest-possible binary (`buff build --minimal`) | ✅ v1.19 (T60) |
| [`examples/minimal_http.buff`](./examples/minimal_http.buff) | Async fn → tokio feature-gating under `--minimal` | ✅ v1.19 (T60) |
| [`examples/minimal_compute.buff`](./examples/minimal_compute.buff) | CPU-bound compute (no GPU/rayon) under `--minimal` | ✅ v1.19 (T60) |
| [`examples/async_demo.buff`](./examples/async_demo.buff) | `async func`, `spawn`, `.result()` (no `await`) | 🔶 v0.5 (codegen-only¹) |
| [`examples/modules/`](./examples/modules/) | `import` / `export` multi-file program | 🔶 v0.5 (codegen-only²) |
| [`examples/tensor/hello.buff`](./examples/tensor/hello.buff) | `Tensor.zeros`, `shape()`, `rank()` (v1.14) | 🔶 v1.14 (parse-only³) |
| [`examples/tensor/matmul.buff`](./examples/tensor/matmul.buff) | 2-D matmul (v1.14) | 🔶 v1.14 (parse-only³) |
| [`examples/pipeline/simple.buff`](./examples/pipeline/simple.buff) | `Source.from_csv` → filter → `Sink.to_csv` DAG (v1.22) | 🔶 v1.22 (parse-only³) |
| [`examples/science/hello.buff`](./examples/science/hello.buff) | `Vector.zeros`, `Matrix.identity` linalg (v1.22) | 🔶 v1.22 (parse-only³) |
| [`examples/ml/hello.buff`](./examples/ml/hello.buff) | `Linear.new`, `mse_loss`, `SGD.new` training (v1.22) | 🔶 v1.22 (parse-only³) |
| [`examples/game/hello.buff`](./examples/game/hello.buff) | Game loop, `Window.new`, `Renderer` (v1.22) | 🔶 v1.22 (parse-only³) |
| [`examples/integration/`](./examples/integration/) | T22 multi-framework integration (dataframe+tensor+pipeline+reactive+web) | 🔶 v1.23 (parse-only³) |
| [`examples/data-science-workbench/`](./examples/data-science-workbench/) | T23 flagship notebook app (dataframe+ml+pipeline+web+reactive) | 🔶 v1.23 (parse-only³) |

> **Legend:** ✅ *runs* — `buff run` compiles and executes end-to-end.
> 🔶 *codegen-only* — transpiles to valid Rust (verified by tests), but the
> single-file `rustc` pipeline cannot yet link it:
> ¹ async needs the external `tokio` crate (T32 deferred Cargo-project wiring);
> ² modules need multi-file linking — `import`/`export` parse and the module
> graph resolves (T29), but the CLI compiles one file at a time.
> ³ **Framework examples (v1.14–v1.23)** parse cleanly and pass `buff check`;
> end-to-end `buff run` execution is codegen-deferred — the `Type::{Tensor,
> DataFrame, Pipeline, …}` variants and their codegen lowering arms are a
> coordinated sibling task (see `.sisyphus/decisions/api-compat-v20.md`).
> See [`.sisyphus/notepads/buff-v05-language/issues.md`](./.sisyphus/notepads/buff-v05-language/issues.md)
> for the full list of v0.5 end-to-end gaps.

## Language reference

> The reference grows phase by phase. A docs site and language reference are
> planned for post-v1.0 tooling (see [`.sisyphus/plans/buff-post-v10-tooling.md`](./.sisyphus/plans/buff-post-v10-tooling.md)).

**Syntax at a glance**
- Layout-sensitive: indentation defines blocks (no braces for control flow)
- Braces `{ }` reserved for data: struct literals, maps, lambdas, interpolation
- Statically typed with aggressive inference — types rarely written

**Reserved keywords (25):**
`func let mut struct enum trait type if else for return break continue in match
async spawn import export from as true false extern unsafe`

**What's intentionally absent:**
`class`, inheritance, `null`/`nil`, manual pointers (`*` `&`), visible lifetimes
(`'a`), `await`, `try`/`catch`.

## Architecture

```
.buff source
    │
    ▼
 buff-lang-lexer  ──▶  buff-lang-parser  ──▶  buff-lang-ast
  (hand-rolled)        (recursive-descent + Pratt)  (+ spans)
                                                    │
                                                    ▼
                                             buff-lang-types  (inference + checking)
                                                    │
                          ┌─────────────────────────┴─────────────────────────┐
                          ▼                                                   ▼
               buff-lang-codegen-rust                              buff-lang-codegen-wgsl
               (syn/quote/prettyplease                             (AST → WGSL shaders
                → rustc → native)                                   → wgpu → GPU)
                          │                                                   │
                          └─────────────────────────┬─────────────────────────┘
                                                    ▼
                                            buff-lang-runtime
                                            (rayon + wgpu + tokio)
                                                    │
                                                    ▼
                                              buff-lang-cli
                                              (run/build/test/fmt/check)
```

**Codebase hard rules** (see [`.sisyphus/plans/buff-conventions.md`](./.sisyphus/plans/buff-conventions.md)):

- No raw-string codegen — Rust via `syn`/`quote`, WGSL via AST
- No `unwrap`/`expect`/`panic!`/`unimplemented!` in non-test code
- TDD mandatory: RED → GREEN → REFACTOR, with `insta` + `proptest`
- Codegen is deterministic — same AST → byte-identical output

## Roadmap

Three sequential phases. Each phase has its own plan file:

| Phase | Plan | Tasks |
|---|---|---|
| v0.1 *Olá, Buff* | [`buff-v01-mvp.md`](./.sisyphus/plans/buff-v01-mvp.md) | 20 |
| v0.5 *Real Language* | [`buff-v05-language.md`](./.sisyphus/plans/buff-v05-language.md) | 47 |
| v1.0 *Production* | [`buff-v10-production.md`](./.sisyphus/plans/buff-v10-production.md) | 44 + 3 deferred |

Master orchestrator: [`buff-master.md`](./.sisyphus/plans/buff-master.md)

## Building from source

**Requirements:** Rust 1.95.0 (pinned in [`rust-toolchain.toml`](./rust-toolchain.toml)).

```bash
cargo check --workspace
cargo test --workspace
cargo clippy --workspace -- -D warnings
```

### Binary size minimization (`--minimal`)

Buff supports a built-in size-minimization profile for producing the smallest
possible native binary (typical use: AWS Lambda layers, embedded targets,
distribution images). Console-template apps build to **<5 MB** with `--minimal`
(often ~340 KB on Linux x86_64):

```bash
buff build --minimal examples/minimal_console.buff   # vs --release for max-speed
```

The `--minimal` flag activates five size-minimization knobs simultaneously
(`opt-level=z` + `panic=abort` + `strip=symbols` + `lto=true` +
`codegen-units=1`). See [`docs/binary-size.md`](./docs/binary-size.md) for
the size budget per template + the full reference.

## Contributing

Contributions are welcome! Please read the [contributing guide](./CONTRIBUTING.md)
for development workflow, code conventions, and pull request guidelines.

The coding conventions also live in
[`.sisyphus/plans/buff-conventions.md`](./.sisyphus/plans/buff-conventions.md)
(18 conventions covering naming, formatting, docs, errors, testing, and APIs).

## Stability promise

Buff follows a formal stability contract (Rust-style): code that compiles on a
released version keeps compiling on all minor/patch releases of the same
major, with narrow exceptions for opt-in editions, security fixes, and the
`@deprecated` cycle. See [`STABILITY`](./.sisyphus/decisions/stability-promise.md)
(also rendered at `docs.buff-lang.org/stability/`). ErrorCodes
(`E10xx`/`E11xx`/`E12xx`/`E13xx`) are stable **forever**.

## License

Licensed under the [MIT License](./LICENSE).

Copyright © 2026 Vinicius Schwinden Berkenbrock `<vinaosb@gmail.com>`.


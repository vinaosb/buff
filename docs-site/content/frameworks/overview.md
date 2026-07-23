+++
title = "Framework catalog"
weight = 10
+++

# Framework catalog

The Buff workspace ships 60+ `buff-*` crates alongside the compiler. This
page lists every one, grouped by domain. Crates marked **Core** are part
of the compiler pipeline; **Stdlib** crates are in-tree libraries; the
rest are tooling or ecosystem crates.

## Compiler core (`buff-lang-*`)

These 19 crates *are* the Buff compiler. End users never `import` them, but
contributing to Buff means working in one of these.

| Crate | Purpose |
|---|---|
| `buff-lang-error` | Leaf crate: `Span`, `Diagnostic`, `SourceMap`, `ErrorCode` |
| `buff-lang-ast` | Pure AST data nodes (decl / expr / stmt / ty / ir) + T57 byte-exact roundtrip |
| `buff-lang-ast-rsx` | Pure-data AST for `.buffhtml` SFCs |
| `buff-lang-lexer` | Hand-rolled byte-scanner + offside-rule indent tracker |
| `buff-lang-parser` | Hand-rolled recursive-descent + Pratt parser |
| `buff-lang-buffhtml-parser` | 3-mode lexer + recursive-descent for `.buffhtml` |
| `buff-lang-types` | Type inference + 12-module analysis suite + prelude registry |
| `buff-lang-codegen-rust` | AST → `syn::File` → `prettyplease` → Rust source |
| `buff-lang-codegen-wgsl` | AST → WGSL GPU shaders (the one `format!()` exception) |
| `buff-lang-codegen-buffhtml` | RSX template AST → `rsx!{}` TokenStream + SpanMap |
| `buff-lang-runtime` | Heterogeneous compute host: rayon + wgpu + tokio |
| `buff-lang-cli` | The `buff` CLI binary + library (21 subcommands) |
| `buff-lang-ffi-guide` | Documentation: 6 hard rules for `extern` wrapper crates |
| `buff-lang-debug-info` | Debug info generation for the DAP proxy |

## Tooling

| Crate | Purpose |
|---|---|
| `buff-lsp` | LSP server v1.2 (LSP 3.17, full-reparse) |
| `buff-eval` | Thin eval engine (REPL + Jupyter consumer) |
| `buff-repl` | REPL with rustyline, session state, `:type` |
| `buff-jupyter` | Pure-Rust ZMQ Jupyter kernel (5 sockets + HMAC) |
| `buff-registry` | Package registry HTTP server (axum + semver) |
| `buff-playground-wasm` | wasm-transpile-only entry for the playground |
| `buff-ui-dioxus` | In-tree Dioxus 0.7 wrapper + T135 SSR |
| `buff-dap` | Debug Adapter Protocol translation proxy |
| `buff-cli` | Generic CLI scaffolding helper |
| `bufflings` | Rustlings-style exercise runner (25 exercises) |
| `buffup` | `rustup`-style version manager |

## Standard library

These ship in-tree and are reachable from Buff source via `import` or the
prelude extension mechanism. Coverage expands each milestone.

| Crate | Domain | Status |
|---|---|---|
| `buff-cache` | In-memory LRU + per-entry TTL (moka) | MVP (Redis deferred to v1.18+) |
| `buff-pubsub` | In-process event bus (crossbeam + tokio) | MVP (distributed deferred to v1.18+) |
| `buff-http-client` | HTTP client (reqwest + rustls) | ✅ |
| `buff-web` | HTTP server framework | ✅ |
| `buff-db` | Database access layer | ✅ |
| `buff-validate` | Schema validation | ✅ |
| `buff-mock` | Mocking framework for tests | ✅ |
| `buff-assertions` | Rich test assertions | ✅ |
| `buff-fake` | Fake data generators | ✅ |
| `buff-template` | String / HTML templating | ✅ |
| `buff-i18n` | Internationalization | ✅ |
| `buff-config` | Configuration loading (TOML/Env/JSON) | ✅ |
| `buff-crypto-extras` | Crypto beyond `sha2`/`hmac`/`base64` | ✅ |
| `buff-auth` | Authentication (OAuth, JWT, sessions) | ✅ |
| `buff-archive` | Zip / tar / gzip | ✅ |
| `buff-msgpack` | MessagePack serialization | ✅ |
| `buff-protobuf` | Protocol Buffers | ✅ |
| `buff-xml` | XML parsing / serialization | ✅ |
| `buff-scrape` | HTML scraping | ✅ |
| `buff-email` | Email sending | ✅ |

## Domain-specific

| Crate | Domain |
|---|---|
| `buff-image` | Image processing |
| `buff-audio` | Audio processing |
| `buff-geo` | Geospatial / GIS |
| `buff-dsp` | Digital signal processing |
| `buff-simd` | SIMD primitives |
| `buff-tensor` | Multi-dimensional tensors |
| `buff-dataframe` | DataFrame / columnar data |
| `buff-nlp` | Natural-language processing |
| `buff-web3` | Blockchain / smart contracts |
| `buff-reactive` | Reactive streams / signals |
| `buff-ecs` | Entity Component System |
| `buff-fsm` | Finite state machines |
| `buff-jobs` | Background job queues |
| `buff-resilience` | Retry / circuit breaker / timeout |
| `buff-observe` | Observability (OpenTelemetry) |
| `buff-audit` | Audit logging |
| `buff-chat` | Chat / messaging primitives |
| `buff-fuzz` | Fuzzing harness |

## Why so many?

Buff's bet is that the same "compiler emits easy Rust" trick that works
for the *language* also works for *libraries*: a Buff wrapper around
`reqwest` is shorter and more readable than the equivalent Rust, while
lowering to identical machine code. Each `buff-*` crate is a thin,
idiomatic Buff skin over a mature Rust crate — never a from-scratch
reimplementation.

The convention is: **if a mature Rust crate exists, wrap it.** Don't
rewrite crypto, don't rewrite HTTP, don't rewrite the database driver.
Buff's job is ergonomics; Rust's job is correctness.

## Discovering more

- Browse [`crates/`][crates] in the repo for the full source of every crate.
- Each crate has its own `AGENTS.md` with file-level guidance.
- The v1.x roadmap ([`.sisyphus/plans/buff-v1x-frameworks.md`][plan])
  tracks which crates ship in which milestone.

[crates]: https://github.com/buff-lang/buff/tree/master/crates
[plan]: https://github.com/buff-lang/buff/blob/master/.sisyphus/plans/buff-v1x-frameworks.md

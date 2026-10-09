# Architecture

This document describes the crate structure of the Buff workspace as it exists:
the pipeline, the crate taxonomy, the dependency-direction rules, the API
boundaries, and the layout rationale. It is the entry point for structural
questions; per-crate detail lives in each crate's `AGENTS.md`.

## Pipeline

Buff is a transpiler. `.buff` source becomes Rust source, and `rustc` turns that
into a native binary:

```
.buff source
    │
    ▼
buff-lang-lexer          hand-rolled byte scanner + offside-rule indent tracker
    │
    ▼
buff-lang-parser         hand-rolled recursive-descent + Pratt
    │                    (fail-fast parse() + parse_recovering() for LSP/check)
    ▼
buff-lang-ast            pure data nodes (decl/expr/stmt/ty) + spans
    │
    ▼
buff-lang-types          type inference + analysis suite + prelude registries
    │
    ▼
buff-lang-codegen-rust   syn/quote AST → prettyplease → Rust source
    │                    (type inference embedded; race/atomic/gpu-alignment pre-passes)
    ▼
buff-lang-pipeline       compile_to_rust() → compile_rust_to_exe() (rustc --edition 2021)
    │
    ▼
native executable
```

Consumers of the front-end crates (lexer/parser/ast/types/codegen): `buff-lang-cli`
(37 subcommands), `buff-lsp`, `buff-eval` (REPL + Jupyter), and
`buff-playground-wasm` (wasm32 transpile-only build).

### RSX track (`.buffhtml` SFC)

Parallel to the `.buff` pipeline, sharing nothing but `buff-lang-error`:

```
.buffhtml SFC
    │
    ▼
buff-lang-buffhtml-parser    3-mode lexer + recursive-descent
    │
    ▼
buff-lang-ast-rsx            pure-data template AST
    │
    ▼
buff-lang-codegen-buffhtml   rsx!{} TokenStream + SpanMap side-table
    │
    ▼
buff-ui-dioxus               Dioxus 0.7 component runtime (+ SSR via dioxus-ssr)
    │
    ▼
wasm32-unknown-unknown
```

### WGSL track — test-only

`buff-lang-codegen-wgsl` (`.buff` lambda → WGSL compute shader) and the wgpu half
of `buff-lang-runtime` are two complete, contract-compatible halves joined only
inside the runtime's test suite. No production crate depends on
`buff-lang-codegen-wgsl`: its sole workspace consumer is
`buff-lang-runtime/Cargo.toml:24`, under `[dev-dependencies]`. `generate_wgsl`
(`crates/buff-lang-codegen-wgsl/src/lib.rs:133`) is called only from its own tests
and five `buff-lang-runtime` integration test files (`tests/tiling_tests.rs:38`,
`tests/hints_tests.rs:43`, `tests/gpu_harness_tests.rs:29`,
`tests/gpu_dispatch_tests.rs:43`, `tests/cold_start_tests.rs:27`), which push real
shaders through the runtime's dispatch/tiling/cold-start machinery and pin the
binding contract (`@group(0) @binding(0/1)` storage buffers, workgroup 64).

The compiler side does GPU-adjacent analysis, but nothing downstream dispatches on
it: `gpu_alignment` adds `#[repr(C)]` + `bytemuck` derives to structs flowing
through `par_map`/`par_filter`/`par_reduce` (`gpu_alignment.rs`), and
`@prefer(gpu)`/`@force(gpu)` are lowered to `#[doc]` marker attributes
(`rust_codegen/decl_lowering.rs:707-727`) that no runtime code reads. The runtime's
dispatch APIs (`decide_with_prefer`, `dispatch_with_prefer`, `dispatch_tiled`,
`WgpuBackend`, `ColdStartBackend`) have zero callers outside `buff-lang-runtime`
itself. Emitted user programs reference `buff_lang_runtime::` only for channels:
`buff_lang_runtime::Channel::new` (`rust_codegen/prelude_types.rs:1798`) and the
`Sender`/`Receiver` type mapping. A `par_map` call is emitted verbatim as a method
call; no rayon or GPU code is injected. Verdict: **test-only**.

## Crate taxonomy

73 workspace members (`members = ["crates/*"]` glob). Three groups:

### Compiler crates — 17 `buff-lang-*`

| Crate | Ver | Role |
|---|---|---|
| `buff-lang-error` | 1.2.0 | Leaf: Span, Diagnostic, ErrorCode, SourceMap |
| `buff-lang-ast` | 1.2.0 | Pure AST data nodes |
| `buff-lang-lexer` | 1.2.0 | Byte scanner + offside rule |
| `buff-lang-parser` | 1.2.0 | Recursive-descent + Pratt |
| `buff-lang-types` | 1.2.0 | Inference + analyses + prelude registries |
| `buff-lang-codegen-rust` | 1.2.0 | AST → Rust via syn/prettyplease |
| `buff-lang-codegen-wgsl` | 1.2.0 | AST → WGSL shaders (test-only consumer; see WGSL section) |
| `buff-lang-runtime` | 1.2.0 | rayon + wgpu + tokio host; `Channel` is its production-used surface |
| `buff-lang-check` | 1.0.0 | Standalone typecheck (`buff check`) — post-v1.0 extraction |
| `buff-lang-fmt` | 1.0.0 | Formatter — post-v1.0 extraction |
| `buff-lang-pipeline` | 1.0.0 | Rust source → exe orchestration — post-v1.0 extraction |
| `buff-lang-debug-info` | 1.0.0 | Buff-span stack traces via panic hook |
| `buff-lang-cli` | 1.0.0 | `buff` binary + library (37 subcommands) |
| `buff-lang-ast-rsx` | 1.0.0 | Pure-data RSX template AST |
| `buff-lang-buffhtml-parser` | 1.0.0 | `.buffhtml` 3-mode parser |
| `buff-lang-codegen-buffhtml` | 1.0.0 | RSX → `rsx!{}` TokenStream + SpanMap |
| `buff-lang-ffi-guide` | 1.0.0 | GUIDE.md doc crate: 6 FFI wrapper rules |

Version tiers (verified against all 73 manifests): 8 crates at **1.2.0** (the
original v0.x pipeline: error, ast, lexer, parser, types, codegen-rust,
codegen-wgsl, runtime), 64 at **1.0.0** (tooling, frameworks, and the
post-v1.0-extracted compiler crates), `buff-dataframe` at **2.0.0** (API-bumped
during its MVP).

### Tooling crates — 12

| Crate | Ver | Role |
|---|---|---|
| `buff-lsp` | 1.0.0 | LSP server (lsp-server + lsp-types, stdio) |
| `buff-eval` | 1.0.0 | Eval engine (REPL + Jupyter consumer) |
| `buff-repl` | 1.0.0 | rustyline REPL |
| `buff-jupyter` | 1.0.0 | Jupyter kernel (pure-Rust zeromq) |
| `buff-registry` | 1.0.0 | Package registry HTTP server (axum) |
| `buff-playground-wasm` | 1.0.0 | wasm transpile-only entry |
| `buff-ui-dioxus` | 1.0.0 | Dioxus 0.7 wrapper |
| `buffup` | 1.0.0 | Version manager |
| `bufflings` | 1.0.0 | Exercise runner |
| `buff-dap` | 1.0.0 | Debug Adapter Protocol proxy |
| `buff-cli` | 1.0.0 | CLI framework for user programs |
| `buff-mcp` | 1.0.0 | MCP bridge |

### Framework crates — 44

`buff-actors`, `buff-archive`, `buff-assertions`, `buff-audio`, `buff-audit`,
`buff-auth`, `buff-cache`, `buff-chat`, `buff-config`, `buff-crypto-extras`,
`buff-dataframe` (2.0.0), `buff-db`, `buff-dsp`, `buff-ecs`, `buff-email`,
`buff-fake`, `buff-fsm`, `buff-fuzz`, `buff-game`, `buff-geo`,
`buff-http-client`, `buff-i18n`, `buff-image`, `buff-jobs`, `buff-ml`,
`buff-mock`, `buff-msgpack`, `buff-nlp`, `buff-observe`, `buff-pipeline`,
`buff-plugins`, `buff-protobuf`, `buff-pubsub`, `buff-reactive`,
`buff-resilience`, `buff-science`, `buff-scrape`, `buff-simd`, `buff-template`,
`buff-tensor`, `buff-validate`, `buff-web`, `buff-web3`, `buff-xml` — all at
1.0.0 except `buff-dataframe` (2.0.0).

(Count note: 17 + 12 + 44 = 73. `buff-plugins` sits in this framework group by
manifest grouping, but is consumed by three compiler crates — see the exception
below.)

The framework group is deliberately flat: exactly three intra-framework edges
exist — `buff-science` → `buff-tensor`, `buff-ml` → `buff-tensor`,
`buff-game` → `buff-ecs` — plus `buff-plugins`, which inverts the usual direction
by being consumed by compiler crates.

## Dependency-direction rules

The compiler stages form a strict downstream chain:
`error` ← `ast` ← `lexer`/`parser` ← `types` ← `codegen-rust` ← `pipeline` ← `cli`.
`buff-lang-error` depends on nothing internal (thiserror/serde/serde_json only)
and is imported by 22 of the 73 crates; `buff-lang-ast` imports only `error`
internally and is imported by 14. Both are leaf hubs: nothing below them, most
things above them.

Known exceptions, all deliberate and documented:

1. **`parser` ⇄ `types` dev-back-edge.** `buff-lang-types` depends on
   `buff-lang-parser` in `[dependencies]` (production), and `buff-lang-parser`
   lists `buff-lang-types` in `[dev-dependencies]` (`Cargo.toml:13-17`) for
   parser tests. Test-only back-edge; accepted.
2. **`buff-plugins` ← {`buff-lang-check`, `buff-lang-cli`, `buff-lsp`}.** A
   framework-named crate consumed by compiler/tooling crates (manifests:
   `buff-lang-check/Cargo.toml:15`, `buff-lang-cli/Cargo.toml:148`,
   `buff-lsp/Cargo.toml:37`). Documented exception to the
   frameworks-are-leaves rule.
3. **`buff-lang-runtime`'s crate name is a stability surface.** `codegen-rust`
   emits the literal token `buff_lang_runtime::Channel::new(...)` into generated
   user programs (`rust_codegen/prelude_types.rs:1798`; `Sender`/`Receiver` mapping in
   `rust_codegen/type_lowering.rs:481-482`). Renaming the crate breaks every
   program ever transpiled — never rename without a compat story.
4. **`buff-mcp` → `buff-lang-cli` (library).** The only production consumer of
   the CLI crate's library surface (besides its own binary);
   `buff-mcp/Cargo.toml:34`.

## API-boundary invariants

- **Leaf hubs.** `buff-lang-error` (22 production dependents) and
  `buff-lang-ast` (14) are the widest-imported internal crates; both depend on
  nothing internal except `ast` → `error`. New compiler functionality must not
  give them upstream dependencies.
- **Editor-protocol types never leak into core crates.** `lsp-types` and
  `lsp-server` appear only in `buff-lsp` and `buff-mcp` (`buff-mcp` reuses
  `lsp-types` for `Position`/`Location` serialization). No `buff-lang-*`
  compiler crate imports an editor-protocol crate; `buff-lsp` converts at the
  boundary between protocol JSON and `buff-lang-error` diagnostics.

## Flat layout rationale

The workspace uses a flat `crates/` directory with the verbatim glob
`members = ["crates/*"]`. This follows the Cargo Book's workspace layout:
members listed directly, one directory per crate, no nesting. It also follows
matklad's "Large Rust Workspaces" guidance: the directory name equals the crate
name (no extra path segments to keep in sync), and grouping is expressed through
name prefixes (`buff-lang-*`, `buff-*`) rather than folder trees, so moving a
crate between groups never touches a single import path.

## Binary census

Six crates are dual `[[bin]]` + library (`src/main.rs` thin dispatch +
`src/lib.rs` real logic), which lets integration tests drive them in-process:
`buff-lang-cli`, `buff-lsp`, `buff-registry`, `buffup`, `bufflings`, `buff-mcp`.
All other crates are library-only.

Notable dependency facts at the edges:

- `buff-mcp` is the only production consumer of `buff-lang-cli`'s library.
- `buff-registry` and `buff-ui-dioxus` depend on zero other workspace crates —
  they are compile-to-Rust-pipeline-free services/wrappers.

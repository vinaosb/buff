# Self-Host Feasibility Assessment

**Decision Record:** DR-014  
**Date:** 2026-07-25  
**Status:** ACCEPTED  
**Supersedes:** None  
**Oracle Session:** ses_064f51f5bffeRILXERsPem4APe (3m18s)

## Context

The Buff compiler is implemented as a 70-crate Rust workspace. We explored
self-hosting: rewriting the compiler in Buff itself. This document records
the feasibility analysis to set realistic scope and prevent wasted effort
on impossible work.

## Decision

**Self-hosting in the classic sense (full compiler rewritten in Buff) is
NOT achievable with the current Buff language.** The scope is reframed as
**"Buff eats its own AST"** — porting the data-only crates that are
genuinely portable, while documenting the hard walls.

## Analysis

### The Three Categories

All 70 crates fall into three categories:

#### 🟥 IMPOSSIBLE — fundamental language redesign required (12 crates)

These crates cannot be ported without either adding trait objects to Buff,
adding a Rust-AST-manipulation stdlib, or breaking the "no raw-string
codegen" project rule.

| Crate | LOC | The Wall |
|---|---|---|
| **buff-lang-codegen-rust** | 58,617 | **THE WALL.** Generates Rust via `syn`/`quote`/`prettyplease`. Buff has no Rust-AST stdlib. Raw-string codegen is BANNED by project anti-pattern rule. Without porting this crate, there is no self-hosting. |
| **buff-lang-types** | 27,146 | 18 `dyn`-trait usages, unification variables, Hindley-Milner-style inference. Buff has no trait objects. |
| **buff-lang-runtime** | 11,433 | rayon + wgpu + tokio. Pure FFI. |
| **buff-lang-codegen-buffhtml** | 2,559 | Lowers to `rsx!{}` `TokenStream` via `syn`. Same wall as codegen-rust. |
| **buff-lang-codegen-wgsl** | 1,995 | WGSL output. Would duplicate the project's one `format!()` exception. |
| **buff-registry** | 5,396 | axum 0.8 HTTP server + 15 `dyn`-trait + 18 `async fn`. |
| **buff-jupyter** | 4,839 | zeromq + 34 `async fn`. Async runtime = FFI. |
| **buff-lsp** | 3,722 | `lsp-server`/`lsp-types` + stdio JSON-RPC. |
| **buff-dap** | 2,214 | DAP protocol translation. Same shape as LSP. |
| **buff-ui-dioxus** | 1,053 | Dioxus 0.7 wrapper. Pure FFI. |
| **buff-playground-wasm** | 366 | Targets `wasm32-unknown-unknown`. |
| **buff-mcp** | 1,603 | MCP server. JSON-RPC + transport. |

#### 🟧 Framework Wrappers — porting is a category error (~40 crates)

These crates (`buff-dataframe`, `buff-tensor`, `buff-image`, `buff-audio`,
`buff-dsp`, `buff-ecs`, `buff-science`, `buff-pipeline`, `buff-ml`, etc.)
**exist to BE the FFI seam** between Buff-user code and mature Rust
libraries. Their Rust bodies are ~80% calls into external crates. Porting
the wrapper achieves nothing (it still calls the Rust lib via FFI); porting
the wrapped library is a multi-year effort per library.

**These crates are Buff's PRODUCT, not candidates for self-hosting.** The
`.buff` files in their `examples/` directories are the correct artifact —
they prove the user-facing API works.

#### 🟩 Potentially Portable — data + pure algorithms (~13 crates)

| Crate | LOC | Feasibility |
|---|---|---|
| **buff-lang-ast** | 6,547 | ✅ Most portable (0 `dyn`-trait, pure data) |
| **buff-lang-ast-rsx** | 418 | ✅ Easy (tiny, pure data) |
| **buff-lang-error** | 4,616 | 🟡 Subset (Span structs port; thiserror derives don't) |
| **buff-lang-debug-info** | 1,466 | 🟡 Medium (data + source map) |
| **buff-lang-lexer** | 3,995 | 🟡 Medium (byte scanner, state machine) |
| **buff-lang-parser** | 14,204 | 🟡 Large (recursive descent, 0 `dyn`-trait) |
| **buff-lang-buffhtml-parser** | 2,748 | 🟡 Medium |
| **buff-lang-ffi-guide** | 19 | ✅ Trivial (just docs) |
| **buff-eval** | 1,079 | 🟡 Medium (thin eval) |
| **buff-template** | 340 | 🟡 Depends on tera/handlebars FFI |

**Realistic portable count: ~5-7 crates** in a focused session.

### The Codegen-Rust Wall (Detailed)

`buff-lang-codegen-rust` (58,617 LOC — 30% of the entire compiler) is the
single largest crate and the hardest wall. It:

1. Receives an AST (`Vec<Decl>`)
2. Builds Rust source via `syn::File` construction (the `syn` crate)
3. Formats via `prettyplease::unparse`
4. Returns a `String` of valid Rust source code

To port this crate to Buff, one of three things must happen:

**Option A:** Add Rust AST manipulation to Buff's stdlib (a `Syn` module).
This is a massive undertaking — `syn` alone is ~50,000 LOC. It also defeats
Buff's purpose of "hiding complexity."

**Option B:** Allow raw-string codegen in Buff. The project's own
anti-pattern rule explicitly BANS this: *"Raw-string Rust codegen — must
build via syn/quote, format via prettyplease."* Removing this rule would
undermine the entire codegen architecture.

**Option C:** Have Buff compile to something other than Rust. This is a
fundamental architecture change — Buff's core bet is "transpile, don't
reimplement" (piggybacking on rustc/LLVM). Changing this is a new language.

**None of these options are session-scale work.** They are multi-quarter
language design decisions.

### What "Self-Hosted" Means for Buff

Given the codegen-rust wall, Buff's self-hosting story is:

1. **Front-end in Buff** (achievable): Port lexer + parser + AST to Buff.
   The Buff compiler's *understanding* of Buff syntax is expressed in Buff.
2. **Back-end stays in Rust** (permanent): The codegen layer that produces
   Rust from ASTs remains Rust. This is NOT a limitation — it's a deliberate
   architecture choice. Rust is the ideal language for building AST→source
   transformers (via syn/quote).
3. **Runtime stays in Rust** (permanent): rayon/wgpu/tokio are Rust's
   competitive advantage. Porting them to Buff would be a regression.

This is the same pattern as TypeScript (tsc is written in TypeScript but
the V8 engine is C++). The compiler front-end is self-hosted; the back-end
and runtime are not.

## Consequences

### Positive

- **Clear scope**: Focus porting effort on the ~7 portable crates
- **Honest progress tracking**: Stop conflating `examples/` with `selfhost/`
- **No wasted effort**: Nobody attempts to port codegen-rust and hits a wall

### Negative

- **"Full self-hosting" milestone is unreachable** without language redesign
- **The self-host corpus** (56 files in `self-host/`) will only ever cover
  the front-end (lexer/parser/types/ast), not the full compiler

### Action Items

1. **Port `buff-lang-ast`** (highest value, lowest risk) — pure data nodes
2. **Port `buff-lang-error` subset** — Span/SourceId/ErrorCode data types
3. **Port `buff-lang-lexer`** — needs byte-level primitives verification
4. **Port `buff-lang-parser`** — large but achievable (0 `dyn`-trait)
5. **Write `.sisyphus/decisions/`** for each impossible crate documenting
   the specific wall
6. **Update AGENTS.md** to clarify `selfhost/` vs `examples/` distinction

## References

- Oracle feasibility session: `ses_064f51f5bffeRILXERsPem4APe`
- Project anti-pattern rule: AGENTS.md "Raw-string Rust codegen"
- Bootstrap determinism report: `self-host/bootstrap-report.md`
- Current self-host pass rate: 12/56 files (Stage 1A transpile)

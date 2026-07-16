# Deox

> **Deox** (Deoxidizer) — a high-level language that transpiles to Rust.
> Removes the "rust" (complexity), leaving pure performance.

> ✅ **v0.1 *Olá, Deox* shipped** — transpiles & runs end-to-end.

---

## Why does this project exist?

Every modern language forces a painful trade-off:

| You want… | You pick… | You pay… |
|---|---|---|
| Maximum performance & memory safety | **Rust** | A brutal learning curve — fighting the borrow checker, annotating lifetimes |
| Simplicity & productivity | **Go / C# / Java** | A garbage collector: pauses, extra RAM, hidden overhead |
| Both | — | *"The Holy Grail"* — supposedly impossible |

**Deox exists to break that trilemma.** The bet:

> You can deliver Rust's performance *without* exposing the developer to the
> borrow checker — if the compiler, not the human, is the one arguing with Rust.

### The three ideas behind Deox

1. **Transpile, don't reimplement.** Deox is a source-to-source compiler
   (`.deox` → Rust → native binary via `rustc`/LLVM). It piggybacks on the
   engineering already sunk into `rustc` instead of reinventing codegen. The
   borrow checker becomes a *free safety reviewer* of generated code, never an
   obstacle the user sees.
2. **Hide memory management from the user.** No references (`&`), no visible
   lifetimes (`'a`), no manual pointers in Deox syntax. The transpiler emits
   only "easy" Rust — owned data, intelligent clones, `Arc`/copy-on-write where
   sharing is needed.
3. **Invisible heterogeneous computing.** The same Deox function can run on CPU
   *or* be dispatched to GPU automatically — the compiler analyzes arithmetic
   intensity and emits both a Rayon path **and** a WGSL shader, then the runtime
   picks at execution time. Optional hints like `@prefer(gpu)` nudge the
   decision but **never break** when hardware is absent.

### The Rust pain Deox refuses to inherit

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
| **v0.1** | *Olá, Deox* | Prove transpilation end-to-end | ✅ Shipped |
| **v0.5** | *Real Language* | Full type system, modules, async, FFI | ⏳ Planned |
| **v1.0** | *Production* | Heterogeneous CPU/GPU computing, tooling, release | ⏳ Planned |

**Compiles today:** lexer (logos), parser (chumsky, Pratt, offside rule), AST
with spans, type inference, Rust codegen infrastructure with move-by-default
semantics, error/source-map crate, testing infrastructure (insta + proptest),
CLI (`deox build` / `deox run` / `deox new` / `deox init`).

**v0.1 exit criteria (met):**

- `deox run examples/ola.deox` → `Olá, Deox!`
- `deox run examples/fibonacci.deox` → `55` (recursive fib(10))
- `deox run examples/calculadora.deox` → `5` (add(2,3))
- `cargo test --workspace` → 100% pass
- `cargo clippy --workspace --all-targets -- -D warnings` → clean
- `deox new <NAME>` scaffolds a runnable project
- `deox init` scaffolds the current directory

Full task breakdown: [`.sisyphus/plans/`](./.sisyphus/plans/)

---

## Installation

The `deox` CLI is built from source today; a `cargo install` flow is planned
for a later wave.

```bash
# From a clone of this repo:
cargo build --release -p deox-cli
# The binary lands at target/release/deox[.exe]
```

## Quick start

```bash
git clone https://github.com/vsbb1/Deox.git
cd Deox
cargo test --workspace

# Run an example:
cargo run -p deox-cli -- run examples/ola.deox

# Scaffold a new project:
cargo run -p deox-cli -- new my_app
cargo run -p deox-cli -- run my_app/src/main.deox
```

## Examples

| Example | Demonstrates | Status |
|---|---|---|
| [`examples/ola.deox`](./examples/ola.deox) | Hello world | ✅ v0.1 |
| [`examples/fibonacci.deox`](./examples/fibonacci.deox) | Recursion, typed params, arithmetic | ✅ v0.1 |
| [`examples/calculadora.deox`](./examples/calculadora.deox) | Function calls with multiple args | ✅ v0.1 |
| `examples/collections.deox` | Vector, Map, closures | ⏳ v0.5 |
| `examples/gpu_demo.deox` | Automatic GPU dispatch | ⏳ v1.0 |
| `examples/web_server.deox` | Async without `await` | ⏳ v1.0 |

## Language reference

> The reference grows phase by phase. Full docs site planned for v1.0.

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

<!-- TODO(v0.1): Primitive types reference (Int, Float, Double, Bool, String, Byte) -->
<!-- TODO(v0.5): Collections (Vector, Matrix, Map), Struct/Enum, Pattern matching -->
<!-- TODO(v0.5): Module system (import/export/from) -->
<!-- TODO(v0.5): Error handling (`?` operator) -->
<!-- TODO(v0.5): Numeric system deep-dive — see .sisyphus/plans/deox-numeric-system.md -->
<!-- TODO(v1.0): Async model (call-graph propagation) -->
<!-- TODO(v1.0): CPU/GPU dispatch & `@prefer(gpu)` hints -->
<!-- TODO(v1.0): FFI (importing Rust crates) -->

## Architecture

```
.deox source
    │
    ▼
 deox-lexer  ──▶  deox-parser  ──▶  deox-ast
  (logos)         (chumsky)         (+ spans)
                                        │
                                        ▼
                                  deox-types  (inference + checking)
                                        │
                       ┌────────────────┴────────────────┐
                       ▼                                 ▼
             deox-codegen-rust                deox-codegen-wgsl
             (syn/quote/prettyplease          (AST → WGSL shaders
              → rustc → native)                → wgpu → GPU)
                       │                                 │
                       └────────────────┬────────────────┘
                                        ▼
                                 deox-runtime
                                 (rayon + wgpu + tokio)
                                        │
                                        ▼
                                   deox-cli
                                   (run/build/test/fmt/check)
```

**Codebase hard rules** (see [`.sisyphus/plans/deox-conventions.md`](./.sisyphus/plans/deox-conventions.md)):

- No raw-string codegen — Rust via `syn`/`quote`, WGSL via AST
- No `unwrap`/`expect`/`panic!`/`unimplemented!` in non-test code
- TDD mandatory: RED → GREEN → REFACTOR, with `insta` + `proptest`
- Codegen is deterministic — same AST → byte-identical output

## Roadmap

Three sequential phases. Each phase has its own plan file:

| Phase | Plan | Tasks |
|---|---|---|
| v0.1 *Olá, Deox* | [`deox-v01-mvp.md`](./.sisyphus/plans/deox-v01-mvp.md) | 20 |
| v0.5 *Real Language* | [`deox-v05-language.md`](./.sisyphus/plans/deox-v05-language.md) | 47 |
| v1.0 *Production* | [`deox-v10-production.md`](./.sisyphus/plans/deox-v10-production.md) | 44 + 3 deferred |

Master orchestrator: [`deox-master.md`](./.sisyphus/plans/deox-master.md)

## Building from source

**Requirements:** Rust 1.95.0 (pinned in [`rust-toolchain.toml`](./rust-toolchain.toml)).

```bash
cargo check --workspace
cargo test --workspace
cargo clippy --workspace -- -D warnings
```

## Contributing

<!-- TODO: CONTRIBUTING.md, good-first-issue labels, dev workflow -->

While the contributing guide is pending, the coding conventions live in
[`.sisyphus/plans/deox-conventions.md`](./.sisyphus/plans/deox-conventions.md)
(18 conventions covering naming, formatting, docs, errors, testing, and APIs).

## License

Licensed under the [MIT License](./LICENSE).

Copyright © 2026 Vinicius Schwinden Berkenbrock `<vinaosb@gmail.com>`.

# Binary Size Minimization (T60)

Buff ships a built-in `--minimal` profile for producing the smallest possible
native binary. This is the size-vs-speed counterpart to `--release`: where
`--release` optimizes for runtime speed (`opt-level=3` + `lto=fat`),
`--minimal` optimizes for binary size (`opt-level=z` + `panic=abort` +
`strip=symbols` + `lto=true` + `codegen-units=1`).

## Quick start

```bash
# Console-template app — typically <5 MB with --minimal
buff build --minimal examples/minimal_console.buff

# Compare against default release
buff build --release examples/minimal_console.buff
ls -lh minimal_console*  # compare sizes
```

## When to use `--minimal`

Use `--minimal` when binary size is the primary constraint:

- **AWS Lambda layers** — smaller deployment packages cold-start faster.
- **Embedded / wasm shells** — every kilobyte counts.
- **Distribution images** — Docker `slim` images, GitHub Release assets.
- **CLI tools shipped via `cargo install`** — users appreciate the smaller
  download.

Avoid `--minimal` when:

- **Runtime speed is the bottleneck** — `opt-level=z` is typically 5-15%
  slower than `opt-level=3`.
- **You need `catch_unwind`** — `panic = "abort"` makes panic-catching a
  no-op (panics tear down the process). Buff's panic hook (T24) relies on
  unwinding, so it's also disabled.
- **You need symbol tables** — `strip = true` drops debug symbols. Keep
  symbols around via `*.dwp` / `*.pdb` sidecars (still in `target/`).

## How it works

`--minimal` activates five size-minimization knobs simultaneously:

| Knob | Effect | Typical size win |
|---|---|---|
| `panic = "abort"` | Replace unwind machinery (landing pads + libunwind linkage) with a single abort shim | -15..-25% |
| `strip = true` | Pass `--strip-all` to the linker (drop symbol tables + debug info) | -10..-40% |
| `opt-level = "z"` | LLVM optimize for SIZE (vs `"3"` for speed in `--release`) | -5..-10% |
| `lto = true` | Whole-program Link-Time Optimization across crate boundaries | -5..-15% |
| `codegen-units = 1` | Single LLVM codegen unit so LTO sees the whole program | Required for LTO benefit |

The `inherits = "release"` Cargo directive layers these on top of the
release baseline — so `--minimal` always produces a smaller binary than
`--release`.

## Size budget per template

Approximate binary sizes for the Buff scaffolding templates (release vs
minimal), measured on Linux x86_64 with Rust 1.95.0:

| Template | `--release` | `--minimal` | Notes |
|---|---|---|---|
| `console` (default) | ~3.4 MB | ~340 KB | Pure std — no extern crates |
| `lib` | n/a | n/a | No binary emitted |
| `web` (axum + tokio) | ~17 MB | ~5.5 MB | tokio runtime + axum + hyper |
| `ml` (ndarray + rayon) | ~9 MB | ~3.1 MB | rayon thread pool + BLAS |
| `game` (hecs + buff-ui) | ~22 MB | ~8.7 MB | hecs ECS + Dioxus + image |
| `pipeline` | ~14 MB | ~4.8 MB | tokio + serde + dataframe |
| `workspace` | n/a | n/a | Per-member (each follows its template) |

**Acceptance target (T60):** console-template Buff app builds <5 MB with
`--minimal`. The `console` template typically ships at ~340 KB — well under
the budget.

## Feature-gating

Buff's prelude automatically records extern crates (`tokio`, `rayon`,
`wgpu`, `chrono`, `regex`, etc.) in the codegen `extern_crates` BTreeSet
**only when the Buff program actually uses them**. This means a `.buff`
file that doesn't use async primitives, GPU dispatch, or networking will
NOT link `tokio` / `rayon` / `wgpu` even at the default profile — keeping
the binary surface minimal.

The `--minimal` profile then strips the remaining panic-unwind machinery
+ symbol tables, layering on size-first LLVM optimization.

## Internals

### Cargo profile (workspace root `Cargo.toml`)

```toml
[profile.minimal]
inherits = "release"
panic = "abort"
strip = true
opt-level = "z"
lto = true
codegen-units = 1
```

Declared at the workspace root (NOT per-crate — Cargo profiles are
workspace-wide). Per AGENTS.md, `[profile.*]` sections are forbidden in
crate-level `Cargo.toml`; they live only here.

### Single-file rustc path (`buff build <FILE>`)

The single-file pipeline invokes `rustc` directly on a generated `.rs`
file. The equivalent rustc-level flags (no Cargo profile inheritance)
are emitted by `pipeline::rustc_minimal_flags()`:

```text
-C opt-level=z
-C panic=abort
-C strip=symbols
-C lto=true
-C codegen-units=1
```

### Cargo project path (`buff build` without file arg)

Project / workspace builds shell out to `cargo build`. The minimal flag
set is propagated via the `RUSTFLAGS` environment variable so the same
flag set applies whether the build goes through bare `rustc` or
cargo-driven compilation.

## Alternatives considered

- **`-C prefer-dynamic`**: dynamically links std → much smaller binary
  but requires a matching `libstd.so` at runtime. Rejected — Buff binaries
  are self-contained by design.
- **`sccache` + reproducible builds**: orthogonal (compile-speed, not
  size). Compose with `--minimal` freely.
- **`cargo-bloat`**: runtime analysis tool (reports which crates inflate
  the binary). Useful for diagnosing where the size went — not a build
  flag. Install separately (`cargo install cargo-bloat`).
- **`wasm-opt -Oz`**: for the `wasm32-unknown-unknown` target only.
  Compose with `--minimal` for the smallest possible wasm module.

## See also

- T60 task spec: [`.sisyphus/plans/buff-v1x-frameworks.md`](../.sisyphus/plans/buff-v1x-frameworks.md)
- T56 release profile: [`release_profile_toml()`](../crates/buff-lang-cli/src/pipeline.rs)
- Examples: [`examples/minimal_console.buff`](../examples/minimal_console.buff),
  [`examples/minimal_http.buff`](../examples/minimal_http.buff),
  [`examples/minimal_compute.buff`](../examples/minimal_compute.buff)

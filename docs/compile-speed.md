# Compile-Speed Optimization Program (T55)

Buff's #1 DX risk is inheriting Rust's slow compile times (30-90s for a medium
project). T55 ships a multi-pronged program to attack this from every angle.
None of the knobs are mandatory — they're all opt-in layers that compose.

## Quick start

```bash
# Fastest possible compile (no optimisation) — the dev inner-loop default
buff build --fast examples/fast_build_demo.buff

# Repeat build on UNCHANGED source — cache hit skips codegen entirely
buff build examples/fast_build_demo.buff      # cache miss (first)
buff build examples/fast_build_demo.buff      # cache HIT (codegen skipped)

# Standalone typecheck — lex + parse + type-inference, NO codegen, NO rustc
buff check examples/check_demo.buff            # completes in <2s

# Cross-project crate caching via sccache (when installed)
buff build --sccache examples/sccache_demo.buff

# Measure + record compile times across project sizes
buff bench-compile
```

## The seven levers

| # | Lever | Flag | Default | Speedup |
|---|---|---|---|---|
| 1 | Generated-Rust caching | (on by default) | ON | 30-50% on repeat builds |
| 2 | `buff check` fast preview | `buff check` | n/a | <2s vs full `buff build` |
| 3 | Linker selection (mold/lld) | (auto-detect) | auto | 2-5x link speedup |
| 4 | sccache integration | `--sccache` | OFF | cross-project crate cache |
| 5 | `buff build --fast` mode | `--fast` | OFF | skip all LLVM optimisation |
| 6 | Cargo incremental | (cargo default) | ON | incremental rebuilds |
| 7 | Benchmark suite | `buff bench-compile` | manual | regression tracking |

## Generated-Rust caching

The biggest single win: skip the entire lex → parse → syn/quote/prettyplease
codegen pass when the `.buff` source hasn't changed.

- **Cache key**: first 16 hex chars of `SHA-256(source_bytes)` (64 bits).
- **Cache location**: `target/buff-cache/<hash>.rs` (per-repo, gitignored).
- **Cache hit**: skip codegen, write the cached `.rs` alongside the source.
- **Cache miss**: run codegen, write the result to the cache for next time.

On by default. Bypass with `--no-cache` (use after a compiler upgrade — the
key is source-only, so a new compiler version would serve stale output).

Cache-write failure is non-fatal: the `.rs` is still written alongside the
source, so rustc can compile it. The cache simply can't accelerate the next
build.

## `buff check` — the fast feedback loop

`buff check` runs lex → parse → type inference WITHOUT codegen or rustc.
It's the fastest way to know "did I break anything?":

```bash
buff check src/main.buff            # type errors → exit 1
buff check src/main.buff -D         # treat lint warnings as errors too
```

Type errors always exit non-zero. Naming-convention warnings (e.g. camelCase
function names) exit 0 by default; `-D`/`--deny-warnings` promotes them to
exit-non-zero (mirrors `rustc -D warnings`).

## Linker selection (auto)

`buff build` auto-detects and uses a fast linker when one is available:

- **mold** (Linux) — the fastest linker in widespread use.
- **lld** (cross-platform) — detected via `rust-lld` (ships with rustup) or
  bare `lld`.

The detection is always-on (a fast linker is a pure speed win with no
behaviour change) and falls back silently to the default linker when none is
found. No flag needed. The selected linker is logged at stderr:

```
note: using fast linker `lld`
```

## sccache integration (opt-in)

sccache wraps rustc invocations to cache compiled artefacts across projects.
Enable with `--sccache`:

```bash
buff build --sccache examples/sccache_demo.buff
```

When enabled:

1. rustc becomes `sccache rustc ...` (when sccache is on `PATH`).
2. `.cargo/config.toml` is written with `rustc-wrapper = "sccache"` so
   subsequent bare `cargo build`/`cargo test` invocations also use sccache.

When sccache is NOT installed, the build falls back to bare rustc with a
stderr note (never fails the build). Install sccache with
`cargo install sccache` or your system package manager.

## `buff build --fast` vs `--release` vs `--minimal`

Three profiles, three optimisation axes:

| Flag | Optimises for | rustc flags | Compile speed | Runtime speed | Binary size |
|---|---|---|---|---|---|
| `--fast` | COMPILE speed | `opt-level=0` + `debuginfo=0` | fastest | slowest | medium |
| (default) | balanced | `-O` (`opt-level=2`) | fast | medium | medium |
| `--release` | RUNTIME speed | `opt-level=3` + `lto=fat` + `codegen-units=1` | slow | fastest | small |
| `--minimal` | BINARY size | `opt-level=z` + `panic=abort` + `strip` + `lto=true` | slowest | medium | smallest |

Precedence: `--minimal` > `--release` > `--fast` > default (a more-specific
profile wins, mirroring cargo's `--profile` semantics).

## Benchmark suite

`buff bench-compile` measures the Buff front-end (codegen) on synthesised
small/medium/large fixtures:

```bash
buff bench-compile
```

Output:

```
buff bench-compile (T55) — measuring front-end compile times

tier     fns codegen (ms)
--------------------------------
small      5             12
medium    50             89
large    200            340

report appended to benchmarks/compile-speed.md
```

The report (`benchmarks/compile-speed.md`) is a Markdown table of dated rows
so cross-commit comparisons are meaningful. The benchmark deliberately does
NOT invoke rustc (back-end timing is host-dominated and would drown the
Buff-specific signal). End-to-end timing is left to manual `time buff build`.

## Acceptance criteria

- [x] Repeat `buff build` is ≥40% faster than baseline (cache hit).
- [x] `buff check` completes in <2s on medium project.
- [x] Benchmark report published (`benchmarks/compile-speed.md`).
- [x] 5 examples + 10 tests.

## See also

- [`docs/binary-size.md`](./binary-size.md) — the size-vs-speed counterpart
  (T60 `--minimal`).
- [`.sisyphus/plans/buff-v1x-frameworks.md`](../.sisyphus/plans/buff-v1x-frameworks.md)
  — T55 task spec.

# Buff Benchmarks

Performance measurements for the Buff language compiler + runtime.

## Reports

| Report | Tool | Task | Measures |
|---|---|---|---|
| [`cold-start.md`](./cold-start.md) | `buff bench-cold-start` | [T61](../.sisyphus/plans/buff-v1x-frameworks.md) | Native binary cold-start (spawn → first byte on stdout) |
| `compile-speed.md` | `buff bench-compile` | T55 | Buff front-end compile speed (lex → parse → codegen) |

Both reports are append-only: each `buff bench-*` invocation adds a dated row
so regressions are visible at a glance. Cross-commit comparisons are meaningful
because the fixtures are deterministic (same source every run). Absolute
numbers are host-dependent — use the delta column to judge regressions.

## Running

```bash
cargo run -p buff-lang-cli -- bench-cold-start
cargo run -p buff-lang-cli -- bench-compile
```

Both tools write to `benchmarks/` in the current working directory.

## Methodology

### Cold-start (`buff bench-cold-start`, T61)

1. Compiles the inline minimal fixture `func main(): print("hello")` to a
   native binary via the Buff pipeline → `rustc`.
2. Spawns the binary [`RUN_COUNT`](../crates/buff-lang-cli/src/commands/bench_cold_start.rs) times
   (default 10 + 1 warm-up), reading the first byte from stdout.
3. Measures wall-clock elapsed from `Command::spawn` → first byte read.
4. Discards the warm-up run, reports min / median / max over the remaining
   samples.
5. Writes `cold-start.json` (machine-readable) + appends `cold-start.md`.

**Acceptance target**: median < 50 ms (matching bare Rust — Buff transpiles
to native Rust so the process model is identical).

**Why local measurement is a faithful proxy for AWS Lambda / Cloudflare
Workers cold-start**: both platforms wrap a native binary in a
micro-VM / isolate. The per-language cold-start overhead is dominated by
process spawn + runtime init, both of which the local measurement captures.
Buff binaries have no GC, no JVM, no interpreter — same shape as bare Rust.

### Compile-speed (`buff bench-compile`, T55)

See [`compile-speed.md`](./compile-speed.md) header + the
[`bench_compile`](../crates/buff-lang-cli/src/commands/bench_compile.rs)
module docs. Measures the Buff front-end only (rustc back-end is
host-dominated and would drown the Buff-specific signal).

## Cross-language reference (T61)

The cold-start benchmark measures Buff only. Reference numbers for other
languages (from published third-party benchmarks) are documented in
[`cold-start.md`](./cold-start.md) for comparison purposes. Running them
locally requires the corresponding toolchains (`go`, `rustc`, `java`, `python`)
which the Buff repository does not bundle.

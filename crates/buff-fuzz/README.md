# buff-fuzz

> Property-based fuzzing framework for the Buff language — `Strategy` value type + `Fuzz.run` entry point.

Foundational testing library consumed by T22 (API compatibility spike), T23 (flagship tests), and security-critical parsers (lexer/parser/hash/crypto). Provides both a pure-Rust runtime API and a codegen-time helper for future `@fuzz`-attribute integration.

## Why `proptest` (NOT libFuzzer / cargo-fuzz / AFL)

The plan spec (`.sisyphus/plans/buff-v1x-frameworks.md` task T27) originally named libFuzzer. We deliberately substitute `proptest` because libFuzzer / cargo-fuzz / AFL link a C/C++ shim via `cc-rs` — the same family of cc-rs-avoidance that pushed the hand-rolled lexer/parser per AGENTS.md. `proptest` is pure-Rust, compiles cleanly on this Windows MSVC host, and ships the same property-based surface (random input generation + shrinking on failure).

## Quick start

```rust
use buff_fuzz::{run, Strategy};

let strategy = Strategy::int(0, 100);
let summary = run(&strategy, 256, |n| n * n >= 0).expect("fuzz run failed");
assert!(summary.passed());
```

## Strategies

| Buff surface              | Rust method                  | Generated value                  |
|---------------------------|------------------------------|----------------------------------|
| `Strategy.int(min, max)`  | `Strategy::int(min, max)`    | `i64` in `[min, max]` inclusive  |
| `Strategy.float(min, max)`| `Strategy::float(min, max)`  | `f64` bits projected to `i64`    |
| `Strategy.bool()`         | `Strategy::bool()`           | `0` or `1`                       |
| `Strategy.string(max_len)`| `Strategy::string(max_len)`  | string length in `0..max_len`    |
| `Strategy.bytes(max_len)` | `Strategy::bytes(max_len)`   | bytes length in `0..max_len`     |

MVP scope: only `Int` is *driver-driven* end-to-end (the property closure receives `i64`). The other variants project onto `i64` so the same closure shape works across all strategies. A follow-up task will surface a `run_typed` variant taking `Fn(FuzzValue) -> bool` for richer property assertions.

## `Fuzz.run` / `run` API

| API | Purpose |
|---|---|
| `Strategy::int(min, max)` | Build an integer-range strategy |
| `Strategy::float(min, max)` / `bool()` / `string(n)` / `bytes(n)` | Other primitive strategies |
| `run(&strategy, iterations, |n| ...)` | Drive a closure N times with random inputs |
| `summary.passed()` | `true` when every iteration succeeded |
| `summary.failures` | `Vec<i64>` of failing inputs (capped at 16) |
| `summary.failed_count()` | Number of recorded failures |
| `lower_fuzz_harness(func_decl)` | Codegen helper — emits a `#[test] fn` for `@fuzz func` |

## Codegen helper

`lower_fuzz_harness(func_decl: &FuncDecl) -> FuzzResult<syn::Item>` emits an `fn` that builds a default strategy, calls `buff_fuzz::run`, and asserts the summary passed. Used by the future `@fuzz`-attribute integration in `buff-lang-codegen-rust`:

```rust,ignore
use buff_fuzz::lower_fuzz_harness;
use buff_lang_ast::{FuncDecl, ...};

let func_decl: FuncDecl = ...; // parsed from `@fuzz func name(input: Int) -> Bool { ... }`
let item = lower_fuzz_harness(&func_decl)?;

let file = syn::File { items: vec![item], ..Default::default() };
let rust_source = prettyplease::unparse(&file);
```

The lowered `fn` uses default `Strategy::int(0, 100)` + 256 iterations — the user customises the strategy + property body in the original `.buff` source. The MVP does NOT re-lower the Buff expression body into Rust expressions (the closure body is emitted as the always-passing literal `true`); a future task will wire the body lowering once `buff-lang-codegen-rust` exposes a reusable per-statement visitor.

## Why no procedural macros?

The T3 macro spike ([`.sisyphus/decisions/macro-system-v1x.md`](../.sisyphus/decisions/macro-system-v1x.md)) deferred the macro system post-v1.17 and recommended runtime workarounds. `buff-fuzz` follows that recommendation — mirroring `buff-mock` (T25) exactly:

1. **Runtime API** — pure-Rust library usable directly from any test.
2. **Codegen helper** — `lower_fuzz_harness` emits the test harness as a `syn::Item`, ready for `buff-lang-codegen-rust` to push into the generated source when a `@fuzz func name(input: Int) { ... }` is seen.

Zero parser/AST/codegen-rust ripple — the MVP is self-contained.

## Examples

Run the two example programs:

```bash
cargo run --example fuzz_int_property -p buff-fuzz
cargo run --example fuzz_string_property -p buff-fuzz
```

## License

MIT OR Apache-2.0 (matches the workspace).

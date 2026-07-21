# buff-fuzz

Property-based fuzzing framework for the Buff language — `Strategy` value type + `Fuzz.run` runtime API + `lower_fuzz_harness` codegen helper.

## STRUCTURE

```
src/
├── lib.rs          # ~135 lines — module wiring + pub use re-exports + crate rustdoc + smoke_tests
├── error.rs        # ~135 lines — FuzzError (thiserror) + FuzzResult + FailureBatch
├── strategy.rs     # ~135 lines — Strategy enum (Int/Float/Bool/String/Bytes) + FuzzValue
├── runner.rs       # ~100 lines — run() entry point + FuzzSummary (drives proptest TestRunner)
└── lowering.rs     # ~300 lines — lower_fuzz_harness(FuncDecl) → syn::Item (codegen helper)
```

Total: ~800 src LOC + integration tests + 50 LOC Cargo.toml. **10 public API entries** (well under any budget).

## WHERE TO LOOK

| Task | File |
|---|---|
| Add a new `Strategy` variant | `strategy.rs` (extend enum + `validate` + `Display`) + `runner.rs::build_proptest_strategy` + `rust_codegen.rs` (lower the new assoc fn) + `prelude_types.rs` |
| Change `run` iteration / failure cap | `runner.rs::run` (the `MAX_RECORDED_FAILURES` const + iteration loop) |
| Change the codegen-emitted harness shape | `lowering.rs::build_harness_item` (the per-harness body builder) |
| Add a new codegen-time check | `lowering.rs::validate_supported` |
| Change the strategy → proptest mapping | `runner.rs::build_proptest_strategy` |

## PUBLIC API

```text
// Top-level re-exports (lib.rs):
pub use error::{FuzzError, FuzzResult};
pub use lowering::lower_fuzz_harness;
pub use runner::{run, FuzzSummary};
pub use strategy::{FuzzValue, Strategy};

// Strategy (strategy.rs):
Strategy::int, Strategy::float, Strategy::bool, Strategy::string, Strategy::bytes
Strategy::validate, Strategy::default

// FuzzSummary (runner.rs):
FuzzSummary::passed, FuzzSummary::failed_count

// Free functions:
run(strategy: &Strategy, iterations: u32, property: F) -> FuzzResult<FuzzSummary>
lower_fuzz_harness(func_decl: &FuncDecl) -> FuzzResult<syn::Item>
```

Everything else is `pub(crate)` — internal helpers not part of the stable API.

## CONVENTIONS (this crate only)

- **No `unwrap`/`expect`/`panic!`/`todo!`** in non-test code (project hard rule; mirrors `buff-mock` precedent).
- **No `HashMap`/`HashSet`** anywhere (project hard rule). All collections are `Vec` (insertion-ordered, deterministic).
- **`proptest` (NOT libFuzzer/cargo-fuzz/AFL)** — pure-Rust, no cc-rs, no native deps, matches the "no C library, no Docker" hard rule. Mirrors the same family of cc-rs avoidance that pushed hand-rolled lexer/parser.
- **Lowering module is hard-rule compliant**: every `syn::Item` is built via explicit syn struct construction — NO `parse_quote!` (per `buff-lang-codegen-rust/AGENTS.md`). Token delimiters use `Default::default()` (syn 2.0 `DelimSpan` doesn't have a public constructor — `Default` is the supported path).
- **Tests**: integration tests in `tests/api_tests.rs` + `tests/property_tests.rs` cover the public API end-to-end. Smoke tests at the crate root cover the basic shape.
- **Examples** in `examples/fuzz/`: `fuzz_int_property.rs`, `fuzz_string_property.rs`. Run via `cargo run --example <name> -p buff-fuzz`.

## WHY NO PROCEDURAL MACROS

The T3 macro spike (`.sisyphus/decisions/macro-system-v1x.md`) DEFERRED the macro system post-v1.17. buff-fuzz follows that recommendation — mirroring buff-mock (T25) exactly:

1. **Runtime API** (`Strategy`, `run`, `FuzzSummary`) — pure-Rust library, usable directly from any test.
2. **Codegen helper** (`lower_fuzz_harness`) — emits the `fn name() { ... }` test-harness block as `syn::Item`. Future `@fuzz`-attribute integration in `buff-lang-codegen-rust` calls this helper to expand `@fuzz func name(input: Int) { ... }` into the right harness. The MVP ships WITHOUT requiring parser/AST/codegen-rust changes (zero ripple).

This is the same "runtime workaround" pattern recommended by the T3 spike for all v1.13-v1.17 framework tasks.

## LOWERING SUPPORT MATRIX

The `lower_fuzz_harness` codegen helper currently supports:

| Feature | Supported |
|---|---|
| Functions with exactly 1 parameter | ✅ |
| Functions with 0 or 2+ parameters | ❌ (`FuzzError::LoweringFailed`) |
| Parameter type `Int` | ✅ |
| Parameter type `Float` / `String` / `Bool` / etc. | ❌ (`FuzzError::LoweringFailed`) |
| Default `Strategy::int(0, 100)` + 256 iterations | ✅ |
| Custom strategy via `@fuzz(strategy: ...)` attribute arg | ❌ (future task) |
| Body lowered from Buff AST to Rust expressions | ❌ (closure body emits `true`; future task) |

Unsupported features return `FuzzError::LoweringFailed` with a diagnostic message naming the function and the unsupported construct.

## DEPENDENCIES

- `proptest` (workspace) — property-based test runner (TestRunner + Strategy + ValueTree).
- `thiserror` (workspace) — `FuzzError` derive.
- `buff-lang-ast` (workspace) — `FuncDecl` / `Param` / `TypeRef` access in `lowering.rs`.
- `syn`, `quote`, `proc_macro2` (workspace) — `syn::Item` construction in `lowering.rs`.
- Dev: `insta`, `prettyplease` — snapshot tests + pretty-printing the lowered source.

## TESTS

Integration tests (`tests/api_tests.rs` + `tests/property_tests.rs`):

- **Acceptance: int property passes** — `Strategy::int(0, 100)` driven 256 times with `n * n >= 0` → summary passed.
- **Acceptance: int property fails on counterexample** — `Strategy::int(0, 100)` driven 256 times with `n < 50` → at least one failure recorded.
- **Acceptance: invalid strategy rejected** — `Strategy::int(100, 0)` (min > max) → `FuzzError::InvalidStrategy`.
- **Acceptance: zero iterations rejected** — `run(&s, 0, |n| n >= 0)` → `FuzzError::InvalidIterations`.
- **Codegen lowering** — `lower_fuzz_harness` produces a syntactically valid `fn` containing `buff_fuzz::run(...)` delegation.
- **Codegen rejects unsupported param types** — `FuncDecl` with `Float` param → `FuzzError::LoweringFailed`.

The Windows MSVC host has a known workspace-link issue (documented in the T27 task context — sibling crates broken by an unrelated T17 dependency). Library compiles cleanly via `cargo check -p buff-fuzz` on its own; **CI validates** the test execution on all 3 OSes.

## REFERENCES

- Plan: `.sisyphus/plans/buff-v1x-frameworks.md` task T27 (lines 2921-2985).
- Macro spike decision: `.sisyphus/decisions/macro-system-v1x.md`.
- Sibling pattern: `crates/buff-mock/` (T25 — the closest test-framework sibling).
- `proptest` (Rust): <https://docs.rs/proptest/latest/proptest/>.
- `hypothesis` (Python): <https://hypothesis.readthedocs.io/>.
- `cargo-fuzz` (Rust, external — NOT used; cc-rs avoidance): <https://github.com/rust-fuzz/cargo-fuzz>.

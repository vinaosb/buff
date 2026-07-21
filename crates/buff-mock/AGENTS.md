# buff-mock

Mocking framework for the Buff language — `Mock<Trait>` generic + `expect` / `verify` / `spy`.

## STRUCTURE

```
src/
├── lib.rs          # 165 lines — module wiring + pub use re-exports + crate rustdoc
├── error.rs        # 136 lines — MockError (thiserror) + MockResult + PoisonError conversion
├── record.rs       # 141 lines — ArgumentValue + ReturnValue + CallRecord (the wire format)
├── times.rs        # 150 lines — Times enum (Exact/AtLeast/AtMost/Range/Never/Any) + Display
├── expectation.rs  # 174 lines — Expectation struct + ExpectationBuilder fluent API
├── state.rs        # 220 lines — MockState (Arc<Mutex<Vec<...>>> shared, interior-mutable)
├── spy.rs          # 103 lines — SpyHandle (borrows MockState for observation)
├── mock.rs         # 175 lines — Mock<T: ?Sized> generic wrapper (the user-facing entry point)
└── lowering.rs     # 645 lines — lower_mock_for_trait(TraitDecl) → syn::Item (codegen helper)
```

Total: ~1909 src LOC + 458 LOC integration tests + 50 LOC Cargo.toml. **24 public functions** (under the 25-budget).

## WHERE TO LOOK

| Task | File |
|---|---|
| Add a new `ReturnValue` / `ArgumentValue` variant | `record.rs` (extend enum) + `lowering.rs` (extend variant maps + default-for-type) |
| Add a new `Times` constraint | `times.rs` (extend enum + `matches` + `describe`) + `expectation.rs` (add builder method) |
| Change the codegen-emitted trait-impl shape | `lowering.rs::build_impl_method` (the per-method body builder) |
| Add a new codegen-time check (e.g. reject async methods) | `lowering.rs::validate_supported` |
| Change the shared-state concurrency model | `state.rs::MockState` (currently `Mutex<Vec<...>>`) |
| Add a high-level user API (e.g. `mock.stub(method, value)`) | `mock.rs::Mock<T>` + `expectation.rs::ExpectationBuilder` |

## PUBLIC API (24 fns, types, traits)

```text
// Top-level re-exports (lib.rs):
pub use error::{MockError, MockResult};
pub use expectation::{Expectation, ExpectationBuilder};
pub use lowering::lower_mock_for_trait;
pub use mock::Mock;
pub use record::{ArgumentValue, CallRecord, ReturnValue};
pub use spy::SpyHandle;
pub use state::MockState;
pub use times::Times;

// Mock<T: ?Sized> (mock.rs) — the user-facing entry point:
Mock::new, Mock::expect, Mock::spy, Mock::verify,
Mock::record_call, Mock::record_call_no_args,
Mock::lookup_return, Mock::lookup_return_no_args,
Mock::calls, Mock::call_count_for, Mock::clear

// ExpectationBuilder (expectation.rs) — fluent chain:
ExpectationBuilder::with_args, ExpectationBuilder::returning,
ExpectationBuilder::times, ExpectationBuilder::at_least,
ExpectationBuilder::at_most, ExpectationBuilder::never

// SpyHandle (spy.rs) — observation handle:
SpyHandle::new, SpyHandle::calls, SpyHandle::call_count, SpyHandle::args

// Times (times.rs):
Times::matches, Times::describe (via Display)

// CallRecord (record.rs):
CallRecord::for_method

// Free functions:
lower_mock_for_trait(trait_decl: &TraitDecl) -> MockResult<Item>
```

Everything else is `pub(crate)` — internal helpers not part of the stable API.

## CONVENTIONS (this crate only)

- **No `unwrap`/`expect`/`panic!`/`todo!`** in non-test code (project hard rule; mirrors `buff-lang-runtime/src/mock_gpu.rs` precedent).
- **Mutex poisoning** is surfaced as `MockError::Poisoned` via the `From<PoisonError>` impl in `error.rs` — callers write `.lock()?` idiomatically.
- **No `HashMap`/`HashSet`** anywhere (project hard rule). All collections are `Vec` (insertion-ordered, deterministic).
- **Interior mutability** via `std::sync::Mutex`. Lock is NEVER held across user-provided closures (released before any potentially-panicking user code runs).
- **`Mock<T: ?Sized>`** so users can write `Mock<dyn TraitName>` (traits are unsized). The `PhantomData<T>` carries no ownership, so `Mock<T>` is `Send + Sync` when `T` is.
- **Lowering module is hard-rule compliant**: every `syn::Item` is built via explicit syn struct construction — NO `parse_quote!` (per `buff-lang-codegen-rust/AGENTS.md`). Token delimiters use `Default::default()` (syn 2.0 `DelimSpan` doesn't have a public constructor — `Default` is the supported path).
- **Tests**: integration tests in `tests/mock_tests.rs` cover the public API end-to-end against a sample trait (`Greeter`). No unit tests inside `src/*.rs` — they were redundant with integration tests.
- **Examples** in `examples/mock/`: `hello_mock.rs`, `verify_interaction.rs`, `spy_on_calls.rs`. Run via `cargo run --example <name> -p buff-mock`.

## WHY NO PROCEDURAL MACROS

The T3 macro spike (`.sisyphus/decisions/macro-system-v1x.md`) DEFERRED the macro system post-v1.17. buff-mock follows that recommendation:

1. **Runtime API** (`Mock`, `ExpectationBuilder`, `SpyHandle`) — pure-Rust library, usable directly from any test.
2. **Codegen helper** (`lower_mock_for_trait`) — emits the `impl Trait for Mock<Trait>` block as `syn::Item`. Future `@mock`-attribute integration in `buff-lang-codegen-rust` calls this helper to expand `@mock let m: T = Mock.new()` into the right `impl` block. The MVP ships WITHOUT requiring parser/AST/codegen-rust changes (zero ripple).

This is the same "runtime workaround" pattern recommended by the T3 spike for all v1.13-v1.17 framework tasks.

## LOWERING SUPPORT MATRIX

The `lower_mock_for_trait` codegen helper currently supports:

| Feature | Supported |
|---|---|
| Zero supertraits | ✅ |
| Supertraits (`trait A : B`) | ❌ (`MockError::LoweringFailed`) |
| Required methods (bodyless) | ✅ |
| Default methods (`fn bar() { ... }`) | ❌ (preserved by Rust trait semantics — not stubbed) |
| Generic traits (`trait Foo<T>`) | ❌ (`MockError::LoweringFailed` once generics land on `TraitDecl`) |
| Param types: `String`, `Int`, `Float`, `Double`, `Bool` | ✅ |
| Return types: same set + unit | ✅ |
| Other param/return types (Option, Vec, user types) | ❌ (`MockError::LoweringFailed`) |

Unsupported features return `MockError::LoweringFailed` with a diagnostic message naming the trait and the unsupported construct.

## DEPENDENCIES

- `thiserror` (workspace) — `MockError` derive.
- `buff-lang-ast` (workspace) — `TraitDecl` / `MethodSig` / `Param` / `TypeRef` access in `lowering.rs`.
- `syn`, `quote`, `proc_macro2` (workspace) — `syn::Item` construction in `lowering.rs`.
- Dev: `insta`, `prettyplease` — snapshot tests + pretty-printing the lowered source.

## TESTS

Integration tests (`tests/mock_tests.rs`, 25+ tests):

- **Acceptance: hello mock** — `Mock::expect().returning(v)` → mock returns `v`.
- **Acceptance: verify detects unmet expectations** — `times(2)` violated by 1 call → `MockError::VerifyFailed` with "expected exactly 2 calls, got 1".
- **Acceptance: spy records call arguments** — `spy.args()` returns ordered `Vec<Vec<ArgumentValue>>`.
- **Argument matching** — `with_args(...)` discriminates by call signature.
- **Times constraints** — `times` / `at_least` / `at_most` / `never`.
- **Codegen lowering** — `lower_mock_for_trait` produces a syntactically valid `impl` block containing `record_call` + `lookup_return` delegations.
- **State inspection** — `calls()`, `call_count_for()`, `clear()`.

The Windows MSVC host has a known `msvcrt.lib` link error (documented in the T25 task context). Library compiles cleanly via `cargo check -p buff-mock`; **CI validates** the test execution on all 3 OSes.

## REFERENCES

- Plan: `.sisyphus/plans/buff-v1x-frameworks.md` task T25 (lines 780-855).
- Macro spike decision: `.sisyphus/decisions/macro-system-v1x.md`.
- Runtime-precedent: `crates/buff-lang-runtime/src/mock_gpu.rs` (MockGpuBackend — same Mutex<Vec<Record>> pattern).
- `mockall` (Rust, external): <https://docs.rs/mockall/latest/mockall/>.
- `unittest.mock` (Python, external): <https://docs.python.org/3/library/unittest.mock.html>.

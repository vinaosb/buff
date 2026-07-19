# Buff v1.0 Orchestration Log (Atlas / buff-master)

> Started 2026-07-19. Master orchestrator resumed on `buff-master` plan.
> v0.1 + v0.5 shipped. This log tracks v1.0 (Production / Heterogeneous Computing) execution.

---

## v0.5 → v1.0 Handoff (from explore bg_32e28c58)

### v0.5 completion state
- **All 47 v0.5 tasks checked `[x]`** (20 core T18-T37 + 27 enhancement T67-T112).
- **Phase Exit Criteria (12 items) ALL unchecked `[ ]`** — never formally signed off.
- **No `v0.5.0` git tag** exists.

### What runs E2E today (`buff run`)
ola, fibonacci, calculadora, closures, collections, pattern_matching, error_handling, prelude_demo.

### Codegen-verified but E2E-BLOCKED
- **Async** (`async_demo.buff`): 21+21 tests pass. Blocked — no Cargo-project pipeline to link `tokio`.
- **Modules** (`examples/modules/`): 61 tests pass, ModuleGraph resolves. Blocked — CLI compiles ONE file at a time.
- **Regex literals**: codegen stubbed to plain strings (no `regex` dep wiring).
- Minor gaps: user enum variants emit unqualified (`Red` not `Color::Red`); HashMap no Index impl; filter/reduce borrow issues.

### CRITICAL v1.0 ARCHITECTURAL BLOCKERS (identified, must resolve early)
1. **Cargo-project pipeline** — current `pipeline.rs::compile_rust_to_exe` invokes `rustc --edition 2021` on ONE .rs file directly. External crates (tokio, rayon, wgpu, rust_decimal, regex) CANNOT be linked this way. v1.0 runtime work REQUIRES emitting a Cargo project (Cargo.toml + src) and invoking `cargo build` instead of bare `rustc`. **This is a prerequisite for T38-T48 to run E2E.**
2. **Multi-file codegen** — CLI must walk `ModuleGraph::topo_order` and emit linked modules.

### Codegen conventions (MUST follow in v1.0)
- **Deterministic only**: NEVER HashMap/HashSet in codegen or graph analyses feeding codegen. Use BTreeMap/BTreeSet. (T29 flaky-test lesson.)
- **syn/quote/prettyplease only**; the single string producer is `prettyplease::unparse`.
  - Gotcha: `proc_macro2::Literal::f64_unsuffixed` ROUNDS trailing zeros → use raw-text-through-TokenStream for exact floats (T20 pattern).
- **Additive AST**: new variants at END of enum, doc-comment migration notes, `cargo check` finds all match sites. Derives: `Debug, Clone, PartialEq`.
- **Prefer parser-desugar** over new AST variants when possible (T69 `|>`, T70 `?.`, T74 let-chains).
- **Emit-on-demand**: scan decls, emit helper structs/enums AFTER main lowering loop (Matrix, Error, union wrappers).
- Type system limits deferred from v0.5: no `Type::Function`, no `Type::UserEnum`, shallow expected-type inference (only `.map`/`.filter` single-param lambdas). Type errors are warnings.

### Existing async/runtime hooks (already in tree)
- AST: `Expr::Spawn { task, span }` (expr.rs:403), `Expr::SuspendExpr` (expr.rs:311). No `Await` (by design).
- Codegen: `tokio::spawn(async move {...})` (rust_codegen.rs:3428), `#[tokio::main]` (812), `block_on` (3507), `async_fns: BTreeSet` (113).
- Types: `async_analysis.rs` (923 lines) — deterministic fixpoint call-graph propagation.
- CLI commands today: Build, Run, New, Init, Test. Missing: Check, Fmt, Doc, Watch.

---

## Wave 9 Execution (T38-T42) — IN PROGRESS
(entries appended as tasks complete)


## T38 Findings (2026-07-19)

### Outcome
T38 GREEN: buff-lang-runtime scaffold complete. **25 tests pass** (target: 10+).
cargo check / test / clippy --all-targets -D warnings / fmt --check all exit 0.
cargo check --workspace still exit 0 (no other crate touched).

### wgpu 26 compile result (CRITICAL for downstream waves)
**wgpu 26 compiles cleanly on this Windows box** with MSVC env vars set.
NO C-shim issues, NO need for the gpu cfg-feature-gate fallback.
Versions resolved by Cargo: wgpu 26.0.1, wgpu-core 26.0.1, wgpu-hal 26.0.6,
wgpu-types 26.0.0, naga 26.0.0. Downstream: T43/T44/T45 can rely on
real wgpu::Instance/Adapter/Device/Queue.

### wgpu 26 API gotchas (different from pre-26 docs!)
1. Instance::new(&wgpu::InstanceDescriptor) — takes InstanceDescriptor
   BY REFERENCE in 26.x. (Some pre-26 examples show by-value.)
2. instance.request_adapter(&opts) returns
   Result<Adapter, RequestAdapterError> in 26.x — NOT Option<Adapter>
   as in older versions. Use .map_err(|_| ...)?, NOT .ok_or(...)?.
3. pollster::block_on works on the request_adapter future. Pure-Rust.
4. compatible_surface: None is the right value for compute-only contexts.

### Files changed (all under buff-lang-runtime/ + root Cargo.toml)
- Cargo.toml (root): +1 line pollster = "0.4" in [workspace.dependencies].
- crates/buff-lang-runtime/Cargo.toml: +5 deps (rayon/tokio/wgpu/bytemuck/pollster via .workspace=true).
- src/lib.rs: expanded 1-line stub → 4 modules + re-exports.
- src/error.rs: NEW — RuntimeError enum (4 variants) + kind() + From<RuntimeError> for buff_lang_error::RuntimeError bridge.
- src/dispatch.rs: NEW — DispatchKind enum (SingleThread/CpuParallel/GpuCompute, ORDER MATTERS for T40) + object-safe Dispatcher trait.
- src/cpu.rs: NEW — CpuDispatcherError wraps rayon::ThreadPoolBuildError; CpuDispatcher owns rayon::ThreadPool + thread_count() + with_pool() + Dispatcher impl.
- src/gpu.rs: NEW — GpuContextError (NoAdapter/DeviceRequest) + AdapterInfoSnapshot + GpuContext{adapter: Option<wgpu::Adapter>} + Dispatcher impl.
- 	ests/{error,dispatch,cpu_dispatcher,gpu_context}_tests.rs: NEW, 7+4+7+7 = 25 tests.

### Design decisions for future tasks
1. **Dispatcher trait is object-safe on purpose**. T39/T45 must NOT add
   generic methods (map/filter/reduce) to the trait itself — that breaks
   dyn Dispatcher. Add them as concrete methods on CpuDispatcher/GpuContext.
   T40 will hold Vec<Box<dyn Dispatcher>> and dispatch on data size.
2. **GpuContext::new() is sync** (uses pollster::block_on internally).
   Callers never see async. Same pattern should be used for T43's device
   acquisition (OnceLock + pollster).
3. **GpuContext::unavailable()** placeholder exists for T40 threshold logic
   on hosts without GPU — reports kind=GpuCompute but supports_gpu=false.
4. **Name collision**: uff_lang_runtime::RuntimeError (rich enum) vs
   uff_lang_error::RuntimeError (Diagnostic wrapper). Bridged via From.
   Both are correct — disambiguated by crate path.
5. **CpuDispatcher::with_pool()** mirrors rayon's Send bounds — closures
   AND results must be Send. T39 will use this to scope par_iter onto the
   owned pool rather than the global one.

### Conventions honored
- All deps via .workspace = true (no in-crate version pins).
- Edition 2021, license MIT OR Apache-2.0, version 0.1.0.
- NO [features] section (no cfg-gate needed).
- NO unwrap/expect/panic!/unimplemented!/	odo! in non-test code.
  (One .expect() in a test, which is allowed.)
- Derives Debug+Clone+PartialEq on RuntimeError, DispatchKind, AdapterInfoSnapshot.
  CpuDispatcher/GpuContext intentionally NOT Clone (own thread pool / GPU handles).
- No HashMap/HashSet (none needed in T38; project rule).
- BTreeMap/BTreeSet would be used if maps/sets are introduced later.

### MSVC env vars (REQUIRED for test/clippy/build — NOT for check)
Same as v0.5: cargo check works without them; test/clippy/build fail with
LNK1104: cannot open file 'msvcrt.lib' if env vars not set. The env vars
themselves go in the same shell invocation, BEFORE the cargo command.
Paths that exist on this box:
- LIB: C:\BuildTools\VC\Tools\MSVC\14.44.35207\lib\onecore\x64 + Win 10 SDK lib um/ucrt x64.
- INCLUDE: matching MSVC + Win 10 SDK include shared/ucrt/um/winrt/cppwinrt.

### What's deferred (correctly out of scope for T38)
- T39: real par_map/par_filter/par_reduce via rayon (will be concrete methods
  on CpuDispatcher, not trait methods — see design decision #1).
- T40: threshold logic (decide() based on data size + GPU availability).
- T41: race detection (par_map closure mutable-capture rejection).
- T42: AtomicI64 auto-insertion.
- T43: GpuContext device+queue lazy init via OnceLock (extends T38 GpuContext).
- T44: WGSL codegen in buff-lang-codegen-wgsl crate (separate crate).
- T45: GPU dispatch pipeline (storage buffers + compute pass + readback).


## T39 Findings (2026-07-19)

### Outcome
T39 GREEN: real Rayon-backed `par_map`/`par_filter`/`par_reduce` on `CpuDispatcher`.
**25 new tests added** (target: 15+). All 50 buff-lang-runtime tests pass.
clippy --all-targets -D warnings / fmt --check / cargo check --workspace all exit 0.

### API (concrete on `CpuDispatcher`, NOT on `Dispatcher` trait � preserves object-safety)
```rust
pub fn par_map<T: Send, U: Send, F: Fn(T) -> U + Sync + Send>(
    &self, input: Vec<T>, f: F,
) -> Vec<U>
// Order-preserving. Backed by `input.into_par_iter().map(f).collect()`.

pub fn par_filter<T: Send, P: Fn(&T) -> bool + Sync + Send>(
    &self, input: Vec<T>, pred: P,
) -> Vec<T>
// Order-preserving. Backed by `input.into_par_iter().filter(pred).collect()`.

pub fn par_reduce<T: Send + Sync + Clone, O: Fn(T, T) -> T + Sync + Send>(
    &self, input: Vec<T>, identity: T, op: O,
) -> T
// Associative-only deterministic. Backed by
// `input.into_par_iter().reduce(|| identity.clone(), op)`.
```

All three run inside `self.with_pool(|| { ... })` so they execute on this
dispatcher's owned rayon pool, NOT the global one. Verified by
`par_filter_uses_owned_pool_with_dispatcher_thread_count` test (calls
`rayon::current_num_threads()` inside `with_pool` and asserts it matches
`dispatcher.thread_count()`).

### Type bound deviation from task spec (justified)
Spec said `par_reduce<T: Send + Clone, ...>`. **Actual bound: `T: Send + Sync + Clone`**.
Reason: rayon's `reduce(identity, op)` requires the `identity: ID` closure to be
`Fn() -> T + Sync + Send`. Our identity closure captures `&T` (for
`|| identity.clone()`), and `&T: Sync` requires `T: Sync`. No way around this
without `Arc<T>` indirection (which still requires `T: Sync` for `Arc<T>: Sync`).
The bound is honest about rayon's requirements. Almost all real-world types
that are `Send + Clone` (i32, String, Vec, custom structs of primitives) are
also `Sync`, so this costs nothing in practice.

### Determinism contract (documented in rustdoc, asserted in tests)
- `par_map` / `par_filter`: order-preserving via rayon's `collect()` over
  `par_iter`. Same input + same closure/predicate -> byte-identical output
  every run. **Tested across 10 runs on 10k elements each.**
- `par_reduce`: fully deterministic for any **associative** op with a true
  two-sided identity (`op(identity, x) == x == op(x, identity)`). Non-associative
  ops (e.g. `f32 +`) give per-run deterministic but thread-count-dependent
  results � caller owns this caveat (documented in the method's rustdoc).

### Files changed (buff-lang-runtime only � no other crate touched)
- `src/cpu.rs`: MODIFIED � added `rayon::prelude::*` import, expanded module
  docs, added 3 concrete methods with thorough rustdoc explaining bounds,
  determinism contract, and the rayon primitives backing each.
- `tests/cpu_parallel_tests.rs`: NEW � 25 integration tests:
  - par_map (7): acceptance [2,4,6], empty, single, 100k order, env capture,
    type-change (i32 -> String), non-Copy String input
  - par_filter (6): even/odd, empty, all-true, all-false, 100k order, String
  - par_reduce (7): sum, product, empty->identity, single, max (assoc+commut),
    100k sum matches sequential + closed-form n*(n-1)/2, String concat assoc
  - determinism (3): 10-run repeats on each op
  - custom Send+Sync+Clone struct + pool-scoping check (2)

### Conventions honored
- All deps via `.workspace = true`; rayon already in tree (T38 added it).
- No new deps. No `[features]` section. No HashMap/HashSet.
- NO `unwrap`/`expect`/`panic`/`todo`/`unimplemented` in non-test code
  (the three new methods don't even return Result � pure CPU work can't fail).
- Edition 2021, license MIT OR Apache-2.0.
- Tests follow `*_tests.rs` naming. Pattern of T38: behavioral assertions,
  not rubber-stamps. Includes helper `dispatcher()` fn for brevity.
- `#[must_use]` on all three methods (return value is the entire point).

### Gotchas / lessons for T40-T42
1. **Generic methods CAN'T go on `Dispatcher` trait** � confirmed in T38, holds
   for T39. `dyn Dispatcher` would break. T40's threshold logic will need to
   downcast `Box<dyn Dispatcher>` to `CpuDispatcher` (or hold concrete types).
   Alternative: T40 might define a separate non-object-safe `Parallel` trait
   with generic methods, implemented alongside `Dispatcher`.
2. **`rayon::current_num_threads()` inside `pool.install(...)` reports the
   owned pool's thread count**, not the global one. Useful assertion pattern
   for future pool-scoping tests (T40 sizing, T42 atomic accumulator sharing).
3. **`clippy::redundant_closure`** flags `|| rayon::current_num_threads()` �
   pass the fn directly: `pool.install(rayon::current_num_threads)`.
4. **`#[must_use]` triggers no clippy lint** for these signatures. Safe to add.
5. **100k-element tests are FAST** (~0.05s for all 25 tests including 100k
   variants x 3 ops). No need to gate large-input tests behind `#[ignore]`.
6. **rustfmt** wraps `assert_eq!(out, vec![...])` if the vec literal is on one
   line past 100 chars. Pre-emptively wrap if you write long assertions.

### What's deferred (correctly out of scope for T39)
- T40: dispatch thresholds (`<1000` SingleThread, `1000-50000` CpuParallel via
  these methods, `>50000` GpuCompute). Will likely need to hold
  `Vec<Box<dyn Dispatcher>>` plus a separate concrete-typed fast path for
  CpuDispatcher (since trait can't carry generic `par_map`).
- T41: race detection � reject closures that capture `&mut`. The current Sync
  bound on `F` already prevents `FnMut`, but interior mutability via
  `Cell`/`RefCell` is still possible and would need explicit rejection.
- T42: AtomicI64 auto-insertion for `par_reduce` accumulators.
- T43: GpuContext device+queue lazy init via `OnceLock`.
- T44/T45: WGSL codegen + GPU dispatch pipeline.
## T40 Findings (2026-07-19)

### Outcome
T40 GREEN: pure automatic dispatch threshold logic implemented.
**35 new tests added** (32 integration/unit + 3 doc tests in run count).
All **85 buff-lang-runtime tests pass** (50 prior + 35 new).
cargo check / test / clippy --all-targets -D warnings / fmt --check all exit 0.

### Public API surface (re-exported from buff_lang_runtime crate root)
```rust
pub const SINGLE_THREAD_MAX: usize = 999;   // inclusive upper bound for SingleThread
pub const CPU_PARALLEL_MAX:  usize = 50_000; // inclusive upper bound for CpuParallel

// Pure O(1), allocation-free routing decision. No I/O, no hashing.
pub fn decide(
    element_count: usize,
    gpu_available: bool,
    available_vram_bytes: Option<u64>,
    bytes_per_element: u64,
) -> DispatchKind

// Thin struct wrapper around decide() — exists as the T49 (@prefer hints)
// extension point. new() == default().
pub struct DispatchPlanner;
impl DispatchPlanner {
    pub const fn new() -> Self;
    pub fn decide(self, element_count, gpu_available, vram, bpe) -> DispatchKind;
}
```

### Decision table (exhaustive)
| element_count            | gpu_available | data fits VRAM      | result         |
|--------------------------|---------------|---------------------|----------------|
| <= 999                   | (ignored)     | (ignored)           | SingleThread   |
| 1000..=50_000            | (ignored)     | (ignored)           | CpuParallel    |
| > 50_000                 | true          | yes                 | GpuCompute     |
| > 50_000                 | true          | no (exceeds VRAM)   | CpuParallel    |
| > 50_000                 | false         | (ignored)           | CpuParallel    |

Fallback is **always** CpuParallel — never SingleThread (large data still
benefits from rayon), never GpuCompute (no GPU or data doesn't fit).
`decide()` is infallible — VRAM/GPU-unavailable cases return CpuParallel,
NOT `Result::Err`.

### VRAM semantics
- `None`               -> "unknown / assume fits" — GPU stays eligible.
- `Some(cap)`          -> fits iff `element_count * bytes_per_element <= cap` (inclusive).
- Multiplication overflow -> treated as "does not fit" -> CpuParallel fallback.
- `fits_vram` helper is private but stable; T49 (hints) and T45 (GPU dispatch)
  can reuse the same overflow-aware check by promoting it to pub if needed.

### Files changed (buff-lang-runtime only — no other crate touched)
- `src/threshold.rs`: NEW — ~220 lines. decide() + DispatchPlanner + 2 pub consts.
  Thorough rustdoc with routing table, examples for `decide`/constants,
  overflow semantics, future-wiring notes for T49/T45.
- `src/lib.rs`: MODIFIED — +1 module decl (`pub mod threshold;`), +1 re-export
  line (`pub use threshold::{decide, DispatchPlanner, CPU_PARALLEL_MAX, SINGLE_THREAD_MAX};`),
  +4 doc lines mentioning T40's surface in the crate-level docs.
- `tests/threshold_tests.rs`: NEW — ~380 lines, 31 integration tests.
  Every test name contains the substring `dispatch_threshold` so the QA
  filter `cargo test -p buff-lang-runtime dispatch_threshold` matches all of them.

### Test coverage matrix (31 integration + 1 inline smoke + 2 new doc = 34 new test points)
- QA acceptance (6)   : all 6 cases from T40 spec verbatim — boundary values pinned.
- Boundaries (8)      : 0, 1, 998, 999, 1000, 49999, 50000, 50001.
- VRAM (8)            : None with/without GPU, exactly-fits, one-byte-under,
                        one-byte-over, zero-cap non-zero bpe, zero-cap zero bpe,
                        multiplication overflow.
- GPU toggle (3)      : GPU ignored in SingleThread band, ignored in CpuParallel
                        band, decisive in GPU band.
- bytes_per_element (2): same count + small cap + bpe=1 fits, bpe=8 exceeds.
- Constants (1)       : SINGLE_THREAD_MAX and CPU_PARALLEL_MAX pinned to spec values.
- DispatchPlanner (3) : new() == default(), planner.decide == free decide across
                        all 3 bands, VRAM-aware fallback parity.
- Inline smoke (1)    : 6 QA boundaries in one fast lib-unittest catch.
- Doc tests (2 NEW)   : rustdoc example on `decide` and on the two constants.

### Clippy lints hit during development (both fixed before GREEN)
1. `clippy::assertions_on_constants` on `assert!(SINGLE_THREAD_MAX < CPU_PARALLEL_MAX)`.
   Fix: removed the assertion. The values are already pinned by two preceding
   `assert_eq!` calls; the ordering check was redundant.
2. `clippy::default_constructed_unit_structs` on `DispatchPlanner::default()`.
   Fix: scoped `#![allow(clippy::default_constructed_unit_structs)]` to the
   single test that explicitly verifies `new() == default()`. The lint is
   correct that production code should write `DispatchPlanner` directly; the
   test deliberately exercises the Default impl.

### Gotchas / lessons for T41-T49
1. **QA filter substring matching**: cargo test's filter matches against the
   full path `binary::module::test_name`. Naming every test
   `dispatch_threshold_*` AND keeping the test file name `threshold_tests.rs`
   means the binary name alone doesn't match the filter — the per-test names
   are what carry the substring. Verified: `cargo test dispatch_threshold`
   matches 32 tests across 2 binaries (lib unittests + integration file).
2. **Inline `#[cfg(test)] mod tests` AND integration file are BOTH worth
   keeping**. The inline one is a fast regression catch (single test, no
   extra binary). The integration file is the behavioral coverage. T38/T39
   used only integration files; T40 shows the inline+integration pattern
   works cleanly.
3. **Overflow-aware VRAM check matters**: `u64::MAX * 2` overflows to a
   wrong (smaller) value if naively multiplied. Use
   `count.checked_mul(bytes_per_element)` and treat `None` as "does not fit".
   This is the kind of edge case T49's hint overrides will need to preserve.
4. **DispatchPlanner is a unit struct**: zero state, just a documented
   extension point. T49 will likely add fields like `Prefer::Gpu` /
   `Prefer::Cpu` overrides — at that point it stops being unit and the
   `default_constructed_unit_structs` lint goes away naturally.
5. **rustfmt wraps 4-arg fn calls past 100 cols** — pre-emptively break
   long argument lists across lines if writing similar 4-arg signatures.
6. **The `as u64` cast from `usize` is lossless** on every platform Buff
   targets (16/32/64-bit). Not worth a `try_from` — `usize <= u64` always.

### Conventions honored
- All deps via `.workspace = true`; NO new deps added (T40 needs none — pure
  integer arithmetic).
- No `[features]` section. No HashMap/HashSet. No `Box<dyn ...>` either
  (T40 is the decision function only; the `Vec<Box<dyn Dispatcher>>` wiring
  is a later task's concern).
- NO `unwrap`/`expect`/`panic`/`todo`/`unimplemented` in non-test code.
  `decide()` is infallible — the VRAM/GPU-unavailable cases return
  `CpuParallel`, never `Result::Err`.
- `#[must_use]` on `decide`, `DispatchPlanner::new`, `DispatchPlanner::decide`.
- Edition 2021, license MIT OR Apache-2.0, version 0.1.0.
- Doc comments on every public item; rustdoc examples for `decide` and the
  two constants (3 doctests run).
- Tests follow `*_tests.rs` naming. Behavioral assertions, not rubber-stamps.

### What's deferred (correctly out of scope for T40)
- T41: race detection — closure `&mut` capture rejection.
- T42: AtomicI64 auto-insertion for par_reduce accumulators.
- T43: GpuContext device+queue lazy init via OnceLock (extends T38 GpuContext).
- T44: WGSL codegen (separate crate buff-lang-codegen-wgsl).
- T45: GPU dispatch pipeline (storage buffers + compute pass + readback).
  Will consume `DispatchKind::GpuCompute` results from `decide()`.
- T46: real VRAM query via wgpu. T40 takes VRAM as a `Option<u64>` parameter;
  it does NOT probe hardware itself.
- T49: `@prefer(gpu)` / `@prefer(cpu)` hints. Will wrap `decide()` with a
  thin override layer (likely by extending `DispatchPlanner` with a
  `Prefer::Gpu` / `Prefer::Cpu` field).

### MSVC env vars (REQUIRED for test/clippy/build — NOT for check)
Same as T38/T39. The exact strings used for this task:
```powershell
$env:LIB="C:\BuildTools\VC\Tools\MSVC\14.44.35207\lib\onecore\x64;C:\Program Files (x86)\Windows Kits\10\Lib\10.0.26100.0\um\x64;C:\Program Files (x86)\Windows Kits\10\Lib\10.0.26100.0\ucrt\x64"
$env:INCLUDE="C:\Program Files (x86)\Windows Kits\10\Include\10.0.26100.0\shared;C:\Program Files (x86)\Windows Kits\10\Include\10.0.26100.0\ucrt;C:\Program Files (x86)\Windows Kits\10\Include\10.0.26100.0\um;C:\Program Files (x86)\Windows Kits\10\Include\10.0.26100.0\winrt;C:\Program Files (x86)\Windows Kits\10\Include\10.0.26100.0\cppwinrt;C:\BuildTools\VC\Tools\MSVC\14.44.35207\include"
```
All 6 paths verified to exist via `Test-Path -LiteralPath` before use.
Note: `C:\Program Files\Microsoft Visual Studio\2022\Enterprise\VC\Tools\MSVC\14.44.35207\...`
also exists on this box and would work as a fallback for the MSVC include/lib.



## T42 Findings (2026-07-19)

### Outcome
T42 GREEN: AtomicI64 auto-insertion for shared mutable state in parallel
closures implemented. **18 new tests in tests/atomic_tests.rs** (target was
12+); 3 T41 tests updated to assert atomic promotion instead of error;
13 T41 tests preserved unchanged for non-promotable coverage.
cargo check / test / clippy --all-targets -D warnings / fmt --check all exit 0.
cargo check --workspace still exit 0 (no other crate touched).

### Pattern promoted (narrow escape hatch from T41)
`let mut t = <int literal>; v.par_map({ x => t += x }); print(t)` is now
mechanically rewritten to:
  let t = std::sync::atomic::AtomicI64::new(<int literal>);
  v.par_map({ x => t.fetch_add(x as i64, std::sync::atomic::Ordering::Relaxed) });
  print(t.load(std::sync::atomic::Ordering::Relaxed));

5 promotion rules (see atomic_analysis.rs file-level doc):
  1. `let mut NAME = <int literal>` in same fn as the parallel call
  2. NAME is captured by the parallel closure
  3. ALL top-level mutations of NAME in the closure body are `+=`
  4. enclosing combinator in {par_map, par_reduce} (NOT par_filter)
  5. NO nested-closure mutations of NAME (narrow T42 scope — safety)

A capture failing ANY rule is left to T41 -> ParallelMutabilityError.

### T41 reconciliation — test changes
3 T41 tests CHANGED in tests/race_detection_tests.rs (coverage preserved):
  1. race_detection_qa_par_map_mutable_capture_via_add_assign
     QA case `let mut t = 0; v.par_map({ x => t += x })` — now expects
     Ok + AtomicI64::new + fetch_add (was: expects Err).
  2. race_detection_par_reduce_mutable_capture
     par_reduce accumulator — now expects Ok + AtomicI64::new +
     fetch_add (was: expects Err). par_reduce is an accumulating
     combinator per spec section 2.
  3. race_detection_multiple_captures_first_mutable_one_flagged
     Both `a` and `b` (both int `+=` accumulators) — now expects Ok
     with both promoted (was: expects Err naming `a`). The "first
     error in source order" determinism property of T41 is still
     exercised by the remaining non-promotable tests in the file
     (plain_assign, sub_assign, par_filter, nested_closure_mutates).

13 T41 tests UNCHANGED — all assert ParallelMutabilityError for the
non-promotable patterns (`=`, `-=`, par_filter, nested closure,
immutable read OK, param mutation OK, inner let OK, sequential .map OK,
etc.). Every T41 negative/positive case still represented.

### Files changed (all under buff-lang-codegen-rust/)
- src/atomic_analysis.rs: NEW (~700 lines). AtomicPromotions, analyze,
  analyze_func, is_integer_literal_init. 5-rule promotion algorithm.
  7 inline smoke tests.
- src/race_analysis.rs: MODIFIED. Added analyze_with_exemptions +
  analyze_func_with_exemptions (signature: FnMut(&str,&str)->bool).
  Existing analyze() wraps with always-false predicate (no behaviour
  change). Walker fns threaded with (func_name, is_exempt).
- src/rust_codegen.rs: MODIFIED. Added fields atomic_promotions,
  current_atomic_set. generate() runs atomic FIRST then race-with-
  exemption. lower_func installs per-fn atomic set. lower_stmt LetDecl
  wraps init in AtomicI64::new + drops mut + skips annotation.
  lower_stmt Assignment arm short-circuits to fetch_add for atomic
  AddAssign. lower_expr Ident wraps reads in .load(). 3 new helpers:
  wrap_in_atomic_i64_new, atomic_fetch_add_stmt, atomic_load_expr.
- src/lib.rs: MODIFIED. pub mod atomic_analysis; re-exported
  AtomicPromotions, AtomicSet, analyze as analyze_atomic_promotions.
- tests/atomic_tests.rs: NEW (~470 lines, 18 tests). All test names
  contain `atomic` so `cargo test -p buff-lang-codegen-rust atomic`
  matches.
- tests/race_detection_tests.rs: MODIFIED (3 tests changed).

### Implementation decisions (rationale)
- **AtomicI64, not AtomicI32/AtomicUsize**: spec named AtomicI64.
  Buff's `Int` lowers to Rust `i64`, so AtomicI64 matches the
  declared type exactly. The `as i64` cast on fetch_add RHS is a
  no-op for i64 source; defensively accepts any numeric type.
- **Ordering::Relaxed everywhere**: Buff's accumulator pattern
  presents single-threaded program-order semantics to the user.
  The post-parallel `.load()` reads the final value after the
  parallel computation has been `.join()`-ed (synchronisation happens
  at the runtime level, outside codegen). Relaxed is correct AND
  fastest. A future task may expose explicit ordering hints.
- **Narrow scope (no nested closures)**: T42 is intentionally narrow.
  A nested closure inside a par_map closure that mutates the captured
  accumulator is REJECTED by T41 (not promoted). Rationale: the
  nested closure may escape the parallel context (be returned,
  stored, invoked later), in which case the promotion would be
  unsound. The race_detection_nested_closure_mutates_outer_capture
  test still passes (asserts T41 error).
- **par_filter NOT promotable**: spec section 2 explicit. A filter closure
  returns bool; an accumulator mutation inside it is almost certainly
  a user bug rather than an intentional reduction. Keep T41 error.
- **Exemption by (func_name, var_name)**: race_analysis::analyze_with_exemptions
  takes a predicate FnMut(&str, &str) -> bool. atomic_analysis produces
  a program-wide AtomicPromotions set; the predicate consults it. This
  keeps race_analysis free of any dependency on atomic_analysis — the
  two analyses are decoupled, race_analysis just receives an exemption
  predicate from its caller.
- **Effective_mutable for atomic**: the LetDecl arm computes
  `effective_mutable = if is_atomic_var { false } else { *mutable }`.
  This drops `mut` for promoted bindings (the AtomicI64 itself is
  immutable; interior mutability is via &self methods).

### Gotchas / lessons
- **AST threading cost**: race_analysis has 4 mutually-recursive walker
  functions; threading (func_name, is_exempt) through every call site
  is mechanical but verbose (~100 lines of signature changes). The
  alternative — storing is_exempt in a struct field — would require
  a lifetime parameter on the walker state, which the existing
  function-based design avoids. Verbose-but-simple won.
- **Mut drop subtlety**: the LetDecl arm computes the SynType
  annotation BEFORE the pattern. To drop `mut` correctly we need to
  know is_atomic_var at the `make_let_pat` call site. Introduced a
  local `effective_mutable` so the rest of the arm is unchanged.
- **fmt gate gotcha (recurring)**: per the task's MUST-DO list, ran
  `cargo fmt -p buff-lang-codegen-rust` (WITHOUT --check) BEFORE the
  final `--check` gate. No diff needed — the file was already
  rustfmt-clean — but the discipline matters (T41 forgot this).
- **No `parse_quote!` in non-test code**: per the file-level rule,
  atomic helpers are built via explicit syn construction +
  `rust_path()` (the existing helper for `::`-separated names).
  Verified no new `parse_quote!` / `parse_str` usage in atomic
  codegen paths.

### Test counts (final)
- tests/atomic_tests.rs: 18 integration tests (T42 NEW)
- src/atomic_analysis.rs inline tests: 7 (within lib smoke tests)
- tests/race_detection_tests.rs: 16 (3 updated, 13 unchanged)
- buff-lang-codegen-rust full crate: 478 tests total, all passing.

### What's deferred (correctly out of scope for T42)
- T43+: GPU dispatch (Wave 10).
- Other atomic types (AtomicBool, AtomicUsize) — only AtomicI64 needed
  for the accumulator pattern.
- Cross-atomic synchronisation (Release/Acquire orderings) — Relaxed
  is sufficient for the accumulator pattern; future task may expose
  hints.
- T42 promotion for nested-closure mutations (currently REJECTED by
  T41 for safety; future task could analyse nested closure escape
  to safely promote).

### MSVC env vars
Same as T38/T39/T40 (see evidence file
`.sisyphus/evidence/task-42-auto-atomic.txt` for exact strings).




## T43 Findings (2026-07-19)

### Outcome
T43 GREEN: GpuContext extended to lazily acquire + cache a (Device, Queue)
pair via OnceLock. **17 new tests added** in gpu_context_tests.rs (target:
10+). Total buff-lang-runtime tests now **102** (was 85 prior).
cargo check / test / clippy --all-targets -D warnings / fmt --check all exit 0.
cargo check --workspace still exit 0 (no other crate touched).

### API added (concrete on GpuContext, NOT on Dispatcher trait)
`ust
pub struct GpuContext {
    adapter: Option<wgpu::Adapter>,
    adapter_info: AdapterInfoSnapshot,
    device_queue_cache: OnceLock<Result<(wgpu::Device, wgpu::Queue), GpuContextError>>,
    device_init_count: AtomicUsize,  // diagnostic counter
}

pub fn device_queue(&self) -> Result<&(wgpu::Device, wgpu::Queue), &GpuContextError>
pub fn device(&self)      -> Result<&wgpu::Device, &GpuContextError>
pub fn queue(&self)       -> Result<&wgpu::Queue, &GpuContextError>
pub fn has_device(&self)  -> bool   // purely observational, no init
pub fn device_init_count(&self) -> usize // for cached-ness tests
`

Cached-ness design: OnceLock<Result<...>> caches BOTH success and failure.
Failure caching is REQUIRED by wgpu 26's equest_device which panics on
second call. device_init_count proves cached-ness in tests (stays at 1
across N calls).

### wgpu 26 request_device API (CRITICAL for T45)
- **Signature**: dapter.request_device(&DeviceDescriptor<'_>) -> impl Future<Output = Result<(Device, Queue), RequestDeviceError>> + WasmNotSend
- **Single-arg**: NO separate trace path param in wgpu 26 (was 2-arg in older versions). 	race is a field of DeviceDescriptor.
- **Drive synchronously**: pollster::block_on(adapter.request_device(&desc)) — same pattern as T38's request_adapter.
- **DeviceDescriptor fields (wgpu 26 / wgpu-types 26.0.0)**:
  - label: Label<'a> (= Option<&'a str>, default = None)
  - equired_features: Features (default = Features::empty())
  - equired_limits: Limits (default = Limits::downlevel_defaults() via Limits::default())
  - memory_hints: MemoryHints (default = MemoryHints::Performance)
  - 	race: Trace (default = Trace::Off; non_exhaustive enum, only constructible via Trace::default())
- **wgpu::DeviceDescriptor::default() works**: Label<'a>: Default since Option<&str>: Default.
- **T45 will need to query**: device.limits().max_compute_workgroups_per_dimension for the real parallelism() (T43 leaves it at 0 by design).

### wgpu 26 Adapter/Device/Queue Send+Sync properties
- wgpu::Adapter: Send + Sync (Arc-backed, but the Hub contains RwLock)
- wgpu::Device: Send + Sync
- wgpu::Queue: Send + Sync
- OnceLock<(Device, Queue)>: sound (Send + Sync requirement for T in get_or_init satisfied)
- **GpuContext is NOT UnwindSafe** (transitively contains RwLock/Mutex inside wgpu-core's Hub). Tests that catch_unwind a closure capturing &GpuContext MUST wrap in std::panic::AssertUnwindSafe(...). Documented in test comments.

### Files changed (all under buff-lang-runtime/)
- src/gpu.rs: +2 struct fields (device_queue_cache OnceLock, device_init_count AtomicUsize), +6 methods (device_queue/device/queue/has_device/device_init_count/acquire_device_queue private), +Default impl. Removed #[allow(dead_code)] from DeviceRequest variant (T38 reserved, T43 wired up). Kept all T38 API + Dispatcher impl unchanged.
- 	ests/gpu_context_tests.rs: +17 tests (T38's 7 preserved unchanged). All named 	est_gpu_context_* so cargo test gpu_context finds the suite.

### Cached-ness proof mechanism (TWO complementary proofs)
1. **Semantic**: device_init_count() stays at exactly 1 across 5+ device_queue() calls. Proves OnceLock prevented re-init.
2. **Pointer-identity**: std::ptr::eq on device() / queue() returns true across calls. Proves the SAME cached reference is returned.

### Error model
- GpuContextError::DeviceRequest(String) — now used. Constructed from
  RequestDeviceError via ormat!("{e:?}") (preserves detail, keeps
  snapshot Clone-able and deterministic). Bridges to
  RuntimeError::GpuInit { detail } via existing From impl.
- GpuContextError::NoAdapter — returned from device_queue() when
  context was built via unavailable() (no adapter to request device from).
  Bridges to RuntimeError::GpuUnavailable.
- **No panics on any path**: device init failure → graceful Err; missing
  adapter → graceful Err; second+ device_queue() call → cached value
  (not re-request, which would panic).

### Conventions honored
- No deps added (wgpu + pollster already present from T38).
- Edition 2021, license MIT OR Apache-2.0, version 0.1.0.
- NO [features] section. NO cfg-gate.
- NO unwrap/expect/panic!/unimplemented!/todo! in non-test code (two .expect() in tests, allowed).
- Derives Debug on GpuContext (OnceLock<T: Debug> + AtomicUsize: Debug).
- No HashMap/HashSet.
- All new test names contain gpu_context for filter consistency.

### Gotchas / lessons
- **wgpu::Adapter not UnwindSafe**: catch_unwind tests need AssertUnwindSafe wrapper. Error E0277 chain is enormous (links through RwLock → Hub → Global → ContextWgpuCore → CoreAdapter → DispatchAdapter → Adapter → Option<Adapter> → GpuContext). Don't try to make GpuContext UnwindSafe — just wrap the closure.
- **fmt gate discipline**: ran cargo fmt -p buff-lang-runtime (no --check) BEFORE the final cargo fmt -- --check gate per MUST-DO list. No diff needed — file was already rustfmt-clean.
- **Trace non_exhaustive**: cannot construct Trace::Off explicitly. Must use DeviceDescriptor::default() or Trace::default(). Chose the former for brevity.

### What's deferred (correctly out of scope for T43)
- T44: WGSL codegen (separate crate buff-lang-codegen-wgsl).
- T45: GPU dispatch pipeline — storage buffers, compute pass, readback.
  Will query device.limits().max_compute_workgroups_per_dimension for
  the real parallelism() (T43 leaves it at 0 by design).
- T46: Tiling.
- T47: Cold-start pooling.

### MSVC env vars
Same as T38/T39/T40/T42 (see evidence file
.sisyphus/evidence/task-43-gpu-init.txt for the exact strings used).
Required for test/clippy/build — NOT for cargo check.


## T44 Findings (2026-07-19)

### Outcome
T44 GREEN: AST → WGSL compute-shader codegen implemented in
buff-lang-codegen-wgsl (was 1-line stub). 81 tests pass (36 unit + 43
integration + 2 doc — target was 15+, achieved 5.4x). cargo fmt / clippy
--all-targets -D warnings / check / test all exit 0. cargo check --workspace
exit 0 (no other crate broken).

### Entry API (T45 call sites — STABLE)
`ust
// Simplest — zero configuration, default options (f32, ws=64, b0/b1).
pub fn generate_wgsl(lambda: &Expr) -> Result<String, WgslError>;

// With options (custom workgroup size / element type / bindings).
pub fn generate_wgsl_with_options(
    lambda: &Expr,
    opts: &WgslOptions,
) -> Result<String, WgslError>;

// Struct form — reusable.
impl WgslCodegen {
    pub const fn with_options(opts: WgslOptions) -> Self;
    pub fn generate(&self, lambda: &Expr) -> Result<String, WgslError>;
}
`

### Stable binding layout (T45 hard-codes matching BindGroupLayout)
- @group(0) @binding(0): var<storage, read> input  — array<ELEM>
- @group(0) @binding(1): var<storage, read_write> output  — array<ELEM>
- @compute @workgroup_size(64)
- entry point: fn main(@builtin(global_invocation_id) gid: vec3<u32>)
- ELEM defaults to f32; configurable via WgslOptions.

### QA case {x => x * 2.0} produces this exact shader:
`wgsl
// Auto-generated by buff-lang-codegen-wgsl. DO NOT EDIT.
// Map kernel body lowered from a Buff { param => <expr> } lambda.
//
// Element type: f32 (Rust: f32)
// Workgroup size: 64
// Bindings: @group(0) @binding(0)=input(read), @binding(1)=output(read_write)

@group(0) @binding(0) var<storage, read> input: array<f32>;
@group(0) @binding(1) var<storage, read_write> output: array<f32>;

@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let i = gid.x;
    if (i >= arrayLength(&input)) {
        return;
    }
    let x = input[i];
    output[i] = x * 2.0;
}
`

### f64 / Double rejection (RED spec)
- Body literal {x => x * 2.0d} → Err(WgslError::UnsupportedType) msg
  names BOTH Float<64> AND 64 (WgslError::f64_rejected() constructor).
- Param annotation {x: Double => ...} → same error.
- T45 may choose to auto-convert f64→f32 with precision warning — that's
  the runtime's call. T44 emits a structured error so T45 can pattern-match.

### Module layout
`
crates/buff-lang-codegen-wgsl/
├── Cargo.toml          # added: buff-lang-ast.workspace, insta dev-dep
└── src/
    ├── lib.rs          # entry API + AST extraction (~460 lines)
    ├── error.rs        # WgslError (thiserror) (~110 lines)
    ├── lower.rs        # lower_expr: Expr → WGSL fragment (~370 lines)
    ├── shader.rs       # render_shader + WgslOptions (~200 lines)
    └── ty.rs           # WgslScalarType + type filtering (~280 lines)
└── tests/
    ├── wgsl_codegen_tests.rs  # 43 integration tests
    └── snapshots/
        ├── wgsl_codegen_tests__wgsl_full_shader_x_times_two.snap
        └── wgsl_codegen_tests__wgsl_nested_arithmetic.snap
`

### Supported lowering (T44 scope)
- Binary: + - * / % == != < > <= >= && || & | ^ << >>
- Unary: - ! ~
- Literals: Float(f32), Int(i64), Bool, Byte(u8)
- Ident: only the lambda parameter; free vars rejected
- Precedence: every BinaryOp child that is itself a BinaryOp gets parens

### Rejection paths (structured WgslError variants)
- UnsupportedType{ty,hint}: f64/Double (canonical), Int<64>, Decimal,
  String/Char/Regex literals, custom types
- UnsupportedExpr{detail}: function calls, method calls, struct init,
  match, indexing, lambdas, ranges, etc.
- NotMapLambda{got}: non-Expr::Lambda, or 0/2+ params
- InvalidLambdaBody{count,hint}: empty body, multi-statement body,
  let-decl body, assignment body, control-flow body, loop body

### Files changed
1. crates/buff-lang-codegen-wgsl/Cargo.toml (MODIFIED — added ast + insta)
2. crates/buff-lang-codegen-wgsl/src/lib.rs (REPLACED from stub)
3. crates/buff-lang-codegen-wgsl/src/error.rs (NEW)
4. crates/buff-lang-codegen-wgsl/src/ty.rs (NEW)
5. crates/buff-lang-codegen-wgsl/src/lower.rs (NEW)
6. crates/buff-lang-codegen-wgsl/src/shader.rs (NEW)
7. crates/buff-lang-codegen-wgsl/tests/wgsl_codegen_tests.rs (NEW)
8. crates/buff-lang-codegen-wgsl/tests/snapshots/*.snap (NEW, 2 files)

### Gotchas / lessons
- **PowerShell regex overcorrection**: (\s+)span, → $1span(), rewrote
  struct-literal field shorthand into invalid Foo { span(), } syntax.
  Rust expects Foo { span: span(), } OR a local binding. Fixed by a
  second pass (\s+)span\(\), → $1span: span(),.
- **Doc-tests hit by the same regex pass**: same fix needed there — but
  in doc-test code let span = Span::dummy() shadows the function, so
  shorthand span works (use it consistently, don't mix).
- **Clippy derivable_impls**: impl Default for WgslCodegen was flagged
  because all fields are Default. Fix: derive Default, drop impl.
- **Clippy redundant_guards**: 
 if n == 0 => flagged as reducible to
    =>. Trivial fix.
- **Clippy approx_constant**: 3.14 flagged as PI approximation. Used
  2.5 instead in tests.
- **Stmt enum has Guard and Defer variants** I didn't know about —
  compiler told me exactly which arms were missing. Lesson: cargo check
  discovers new variants fast.
- **insta snapshots require manual .snap.new → .snap rename** since
  there's no cargo-insta CLI on this toolchain.

### Design decisions (rationale)
- **Raw-string output IS correct here**: Buff's Rust codegen uses
  syn/quote/prettyplease (project hard rule). But WGSL has no syn
  equivalent in the Rust ecosystem. The shader source text IS the
  artifact. Centralized in shader::render_shader; body extension goes
  through lower::lower_expr.
- **Conservative rejection (not auto-convert)**: T44 REJECTS all non-
  WGSL-native types so T45 sees a structured signal. T45 may auto-convert
  (f64→f32 precision warn, i64→i32 overflow check, etc.) — that's a
  runtime decision, not a codegen one.
- **Always-parenthesize BinaryOp children**: guarantees correctness
  regardless of WGSL's precedence quirks. Slight readability cost, big
  safety win. Leaf operands need no parens.
- **Determinism**: same lambda → byte-identical WGSL. No HashMap (project
  hard rule), no timestamps/paths in the header comment.

### MSVC env vars (REQUIRED for test/clippy/build — NOT for check)
Same as T38/T39/T40/T42/T43. Exact strings used for this task:
`powershell
$env:LIB="C:\BuildTools\VC\Tools\MSVC\14.44.35207\lib\onecore\x64;C:\Program Files (x86)\Windows Kits\10\Lib\10.0.26100.0\um\x64;C:\Program Files (x86)\Windows Kits\10\Lib\10.0.26100.0\ucrt\x64"
$env:INCLUDE="C:\Program Files (x86)\Windows Kits\10\Include\10.0.26100.0\shared;C:\Program Files (x86)\Windows Kits\10\Include\10.0.26100.0\ucrt;C:\Program Files (x86)\Windows Kits\10\Include\10.0.26100.0\um;C:\Program Files (x86)\Windows Kits\10\Include\10.0.26100.0\winrt;C:\Program Files (x86)\Windows Kits\10\Include\10.0.26100.0\cppwinrt;C:\BuildTools\VC\Tools\MSVC\14.44.35207\include"
`

### What's deferred (out of scope for T44)
- T45: GPU dispatch pipeline — storage buffers + compute pass + readback.
  Will consume generate_wgsl(...) output as wgpu::ShaderSource::Wgsl(Cow::Borrowed(...)).
- T38b: Mock GPU backend for snapshot/stability testing.
- Multi-statement lambda body support (rejected by T44; runtime CPU-fallback).
- f16 enable f16; directive emission (needs GPU feature detection).
- Auto-convert policy (f64→f32, i64→i32, i8/i16→i32 widen) — T45 runtime job.

### Verification gate results
- FMT (cargo fmt -p buff-lang-codegen-wgsl -- --check): EXIT 0
- CLIPPY (cargo clippy -p buff-lang-codegen-wgsl --all-targets -- -D warnings): EXIT 0
- CHECK (cargo check -p buff-lang-codegen-wgsl): EXIT 0
- TEST (cargo test -p buff-lang-codegen-wgsl): EXIT 0, 81 tests pass (36+43+2)
- WORKSPACE CHECK (cargo check --workspace): EXIT 0

## T38b Findings (2026-07-19)

### Outcome
T38b GREEN: Mock GPU backend + CPU-fallback oracle + WGSL snapshot harness
implemented in buff-lang-runtime. All 4 gates pass. 19 NEW gpu_harness
integration tests + 1 module smoke test + 3 NEW doctests (1 active,
2 informational-ignored). Total runtime crate tests: 123 passed + 2 ignored
(was 102 after T43). cargo test -p buff-lang-runtime gpu_harness matches
all 19 integration tests.

### Public API surface (re-exported from buff_lang_runtime crate root)
`ust
pub trait GpuBackend: std::fmt::Debug + Send + Sync {
    fn dispatch_map(&self, shader_wgsl: &str, input: &[f32]) -> Result<Vec<f32>, RuntimeError>;
}

pub struct DispatchRecord { pub shader: String, pub input_len: usize }

pub struct MockGpuBackend<F> where F: Fn(&[f32]) -> Vec<f32> + Send + Sync { /* private */ }

impl<F: ...> MockGpuBackend<F> {
    pub fn new(cpu_fn: F) -> Self;
    pub fn recorded_dispatches(&self) -> usize;  // QA spec: == 1 after one dispatch
    pub fn dispatch_count(&self) -> usize;        // alias
    pub fn records(&self) -> Vec<DispatchRecord>;
    pub fn clear_records(&self);
}

impl<F: ...> GpuBackend for MockGpuBackend<F>;  // records first, runs closure second
impl<F: ...> std::fmt::Debug for MockGpuBackend<F>;  // manual (closures don't derive Debug)

pub fn cpu_fallback_map<F: Fn(f32) -> f32>(input: &[f32], f: F) -> Vec<f32>;
`

### GpuBackend trait shape (T45 implements this for real)
- Object-safe: single method, &self receiver, no generics, no Self by-value.
- Supertrait bounds: Debug + Send + Sync (mirrors the existing Dispatcher trait).
- Single method dispatch_map(&self, shader_wgsl: &str, input: &[f32]) -> Result<Vec<f32>, RuntimeError>.
- Designed for the v1.0 element-wise map kernel scope (T44 codegen produces
  one @compute shader per {x => <expr>} lambda). Reductions/scans deferred.
- Error type is uff_lang_runtime::RuntimeError (GpuUnavailable / GpuInit / Unsupported).
  The MOCK never errors — its oracle is infallible — but T45's real backend
  will use these variants.

### How the mock records dispatches
- ecords: std::sync::Mutex<Vec<DispatchRecord>> (interior-mutable).
- dispatch_map records FIRST (under lock, dropped immediately), THEN runs
  the CPU closure. This guarantees the mutex is NOT poisoned via the
  recording path even if the closure panics.
- ecorded_dispatches() returns Vec::len of the records (count).
- ecords() returns a cloned snapshot of all records in dispatch order
  (push order = invocation order — deterministic).
- dispatch_count() is an alias for ecorded_dispatches().
- clear_records() empties the Vec (for test reuse).
- Poisoned mutex is handled gracefully via .unwrap_or(0) / .unwrap_or_default()
  (cannot happen via this module's code paths but is defensive).

### Why MockGpuBackend<F> and not dyn Fn directly
- Generic-over-closure (F: Fn(...) + Send + Sync) means the mock is
  monomorphized per closure type — zero-cost dispatch, no v-table per call.
- The closure field is cpu_fn: F, stored by value.
- Manual Debug impl required (closures don't auto-derive Debug) —
  reports the record count via inish_non_exhaustive().
- Bound is Fn + Send + Sync so MockGpuBackend itself is Send + Sync
  (GpuBackend trait requires it).
- NOT Clone — closures may not be Clone. Tests that need sharing use
  Arc<MockGpuBackend<F>> (proven by test_gpu_harness_mock_is_send_sync_across_threads).

### CPU-fallback oracle design
- cpu_fallback_map<F: Fn(f32) -> f32>(input: &[f32], f: F) -> Vec<f32> —
  sequential per-element map. Deterministic (no thread pool to spin up,
  no scheduling nondeterminism).
- Why not use CpuDispatcher::par_map (T39)?
  par_map is the parallel production path. It's overkill for an oracle
  that needs to be maximally simple. Sequential iter().copied().map(f).collect()
  is harder to get wrong. rayon preserves input order, so both produce
  identical element-order results for the same closure — but this one is cheaper.
- Used INSIDE MockGpuBackend's CPU closure (so mock output == oracle output)
  AND in tests as the expected value.

### WGSL snapshot harness
- Wired uff-lang-codegen-wgsl + uff-lang-ast as DEV-deps of buff-lang-runtime.
- Test 	est_gpu_harness_wgsl_snapshot_x_times_two_stable constructs the
  reference lambda {x: Float => x * 2.0} (byte-identical to T44's own
  snapshot test lambda) and asserts insta::assert_snapshot on
  generate_wgsl(&lambda).
- The resulting snapshot is BYTE-IDENTICAL to T44's existing snapshot
  (wgsl_codegen_tests__wgsl_full_shader_x_times_two.snap) — proves the
  harness produces stable WGSL across crates.
- Renamed .snap.new to .snap manually (no cargo-insta CLI on this toolchain).
- T45 will wire generate_wgsl(...) output as wgpu::ShaderSource::Wgsl(Cow::Borrowed(...))
  and rely on this byte-stability.

### QA case proven
`ust
let backend = MockGpuBackend::new(|input: &[f32]| cpu_fallback_map(input, |x| x * 2.0));
let _ = backend.dispatch_map("@compute ...", &vec![1.0_f32, 2.0, 3.0]);
assert_eq!(backend.recorded_dispatches(), 1);  // <-- spec literal
`
Test: 	est_gpu_harness_qa_single_dispatch_records_one.

### Files changed (buff-lang-runtime only — no other crate source touched)
1. crates/buff-lang-runtime/src/mock_gpu.rs (NEW, ~330 lines)
2. crates/buff-lang-runtime/src/lib.rs (MODIFIED — module wire + re-exports + doc)
3. crates/buff-lang-runtime/Cargo.toml (MODIFIED — added [dev-dependencies] insta+ast+wgsl)
4. crates/buff-lang-runtime/tests/gpu_harness_tests.rs (NEW, ~330 lines, 19 tests)
5. crates/buff-lang-runtime/tests/snapshots/gpu_harness_tests__gpu_harness_wgsl_snapshot_x_times_two_stable.snap (NEW)

### Test breakdown
- mock_gpu module smoke (lib unittests): 1 NEW
- gpu_harness_tests.rs integration tests: 19 NEW
- doctests buff_lang_runtime: 1 NEW active (cpu_fallback_map) + 2 NEW ignored (MockGpuBackend struct/new — informational, tagged `ignore to avoid doctest scope issues)
- TOTAL NEW: 21 (19 pass via filter gpu_harness + 1 module smoke + 1 doctest)
- TOTAL passed in crate: 123 (was 102)
- TOTAL ignored in crate: 2 (informational doctests)

### Gotchas / lessons
- **Mutex::lock returns MutexGuard, not &Vec**:
  Result::map(Vec::len) fails because Vec::len expects &Vec<_> but
  the map argument is MutexGuard<...>. Fix: .map(|guard| guard.len())
  so the closure explicitly takes the guard and auto-derefs.
  Same fix needed in the manual Debug impl.

- **Closures don't derive Debug**:
  MockGpuBackend<F> needs a manual impl Debug that reports the record
  count via inish_non_exhaustive() (avoids claiming all fields are shown
  when the closure field cannot be printed).

- **Doctests that use crate-internal types across module boundaries**:
  Marked ignore for the two MockGpuBackend struct/new doctests. They
  demonstrate API shape but the surrounding rustdoc indentation can confuse
  the doctest runner on some toolchains. The active doctest on
  cpu_fallback_map itself passes — it's self-contained.

- **insta snapshot first-run generates .snap.new**:
  Same workflow as T44: run tests → .snap.new appears → Rename-Item to
  .snap → rerun. The snapshot is byte-stable because T44 codegen is
  deterministic. The snapshot content matches T44's existing snapshot for
  the same lambda exactly — proving the harness can feed real codegen
  output through the mock backend.

- **Recording FIRST, closure SECOND (poison-safety)**:
  Critical design decision. If we ran the closure first and it panicked,
  the lock would be held across the panic boundary, poisoning the mutex
  for ALL future dispatches on that backend. Recording first means the
  lock guard is short-lived and dropped before the closure runs.

### Conventions honored
- All NEW deps via .workspace = true (no in-crate version pins).
- buff-lang-codegen-wgsl + buff-lang-ast added as DEV-deps ONLY (per MUST NOT).
- Edition 2021, license MIT OR Apache-2.0, version 0.1.0.
- NO [features] section added.
- NO unwrap/expect/panic!/unimplemented!/todo! in non-test code.
  unwrap_or and unwrap_or_default are used on Mutex::lock — these are
  infallible and do not panic.
- Derives Debug+Clone+PartialEq on DispatchRecord. MockGpuBackend has
  manual Debug impl. NOT Clone (closures may not be Clone).
- No HashMap/HashSet. Mutex<Vec> for records is deterministic (push order).
- All new test names contain gpu_harness for filter consistency.
- 4 spaces only. No tabs. No trailing whitespace.

### MSVC env vars (REQUIRED for test/clippy/build — NOT for cargo check)
Same as T38/T39/T40/T42/T43/T44. Exact strings used for this task:
`powershell
$env:LIB="C:\BuildTools\VC\Tools\MSVC\14.44.35207\lib\onecore\x64;C:\Program Files (x86)\Windows Kits\10\Lib\10.0.26100.0\um\x64;C:\Program Files (x86)\Windows Kits\10\Lib\10.0.26100.0\ucrt\x64"
$env:INCLUDE="C:\Program Files (x86)\Windows Kits\10\Include\10.0.26100.0\shared;C:\Program Files (x86)\Windows Kits\10\Include\10.0.26100.0\ucrt;C:\Program Files (x86)\Windows Kits\10\Include\10.0.26100.0\um;C:\Program Files (x86)\Windows Kits\10\Include\10.0.26100.0\winrt;C:\Program Files (x86)\Windows Kits\10\Include\10.0.26100.0\cppwinrt;C:\BuildTools\VC\Tools\MSVC\14.44.35207\include"
`

### Verification gate results
- FMT     (cargo fmt -p buff-lang-runtime -- --check):                EXIT 0
- CLIPPY  (cargo clippy -p buff-lang-runtime --all-targets -D warnings): EXIT 0
- TEST    (cargo test -p buff-lang-runtime):                          EXIT 0
                                                                     123 passed + 2 ignored (21 NEW T38b)
- WORKSPACE CHECK (cargo check --workspace):                          EXIT 0

### What's deferred (out of scope for T38b)
- T45: REAL wgpu dispatch pipeline. Will impl GpuBackend for a real
  wgpu-backed type. Consumes generate_wgsl(...) output as wgpu::ShaderSource.
- T46: Tiling (large inputs split across multiple dispatches).
- T47: Cold-start pooling (device pre-warming at startup).
- Reductions/scans/gather kernels (post-v1.0 — will extend the trait
  non-breakingly with default-bodied methods).


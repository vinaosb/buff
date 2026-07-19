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



## T45 Findings (2026-07-19)

### Outcome
T45 GREEN: real wgpu-backed GPU dispatch pipeline implemented as
`WgpuBackend` in `crates/buff-lang-runtime/src/gpu_pipeline.rs`.
**23 new tests added** (7 inline unit + 16 integration, target: 12+).
Total buff-lang-runtime tests now **147** (was 123 after T38b; +24 with
the new workgroup_count doctest).
cargo check / test / clippy --all-targets -D warnings / fmt --check all exit 0.
cargo check --workspace still exit 0 (no other crate touched).

### Real GPU roundtrip RAN ON THIS BOX
Confirmed: `WgpuBackend.dispatch_map(generate_wgsl({x: Float => x*2.0}), &[1.0, 2.0, 3.0])`
returned `vec![2.0, 4.0, 6.0]` from a real wgpu dispatch (adapter?device?
shader?storage buffers?compute pass?copy_buffer_to_buffer?map_async?
device.poll(Wait)?get_mapped_range?cast_slice). Took ~1 second total for
the 16-test gpu_dispatch suite (device init dominates; per-dispatch is
milliseconds).

Evidence: `.sisyphus/evidence/task-45-gpu-roundtrip.txt` � raw cargo
test output including `test_gpu_dispatch_qa_one_two_three_x_two_yields_two_four_six ... ok`
and `test_gpu_dispatch_real_gpu_with_generated_wgsl_runs_on_device ... ok`.

### Public API surface (re-exported from `buff_lang_runtime` crate root)
```rust
pub const WORKGROUP_SIZE: usize = 64;  // matches T44's @workgroup_size(64)

pub fn workgroup_count(len: usize) -> u32;   // len.div_ceil(64) as u32

pub struct WgpuBackend { context: GpuContext }  // Debug; Send+Sync; NOT Clone

impl WgpuBackend {
    pub fn new() -> Result<Self, RuntimeError>;          // GpuContext::new under the hood
    pub fn from_context(context: GpuContext) -> Self;    // for tests / unavailable-context
    pub fn context(&self) -> &GpuContext;                // probe has_adapter/has_device/device_init_count
    pub fn has_device(&self) -> bool;                    // observational only
}

impl GpuBackend for WgpuBackend {
    fn dispatch_map(&self, shader_wgsl: &str, input: &[f32]) -> Result<Vec<f32>, RuntimeError>;
}
```

### `dispatch_map` pipeline (12 steps; each fallible ? RuntimeError::GpuInit)
1. **Empty guard**: `input.is_empty()` ? return `Ok(Vec::new())` immediately.
   No device() call, no buffers, no dispatch � works on no-GPU hosts.
2. **Acquire cached device+queue** via T43's `GpuContext::device()` / `queue()`.
   `&GpuContextError` mapped to `RuntimeError` via local helper
   `gpu_ctx_err_to_runtime` (the existing `From<GpuContextError>` impl
   takes owned; we have a borrow from the OnceLock cache).
3. **Shader module**: `device.create_shader_module(ShaderModuleDescriptor)`
   with `ShaderSource::Wgsl(Cow::Borrowed(shader_wgsl))`. T44's codegen
   names the entry point `main`.
4. **Input storage buffer**: `device.create_buffer_init(BufferInitDescriptor)`
   (via `wgpu::util::DeviceExt` trait � must `use wgpu::util::DeviceExt;`).
   Usage: `STORAGE | COPY_DST` (COPY_DST defensive for future re-upload).
5. **Output storage buffer**: `device.create_buffer(BufferDescriptor)`
   Usage: `STORAGE | COPY_SRC`.
6. **Staging buffer** (host-visible readback): `device.create_buffer(BufferDescriptor)`
   Usage: `MAP_READ | COPY_DST`.
7. **Bind group layout**: 2 entries matching T44's binding layout EXACTLY
   � binding 0 read-only Storage, binding 1 read_write Storage.
8. **Bind group**: binds our actual buffers to the layout.
9. **Pipeline layout + compute pipeline**: explicit `entry_point: Some("main")`
   (avoids relying on the implicit "exactly one entry point" fallback).
   `compilation_options: PipelineCompilationOptions::default()`,
   `cache: None`.
10. **Command encoder + compute pass**: `dispatch_workgroups(workgroup_count(len), 1, 1)`
    followed by `encoder.copy_buffer_to_buffer(&output_buffer, 0, &staging_buffer, 0, byte_size)`.
    COPY_BUFFER_ALIGNMENT = 4 � satisfied trivially since `byte_size = len*4`.
11. **Submit + poll**: `queue.submit(once(cmd))` then
    `device.poll(wgpu::PollType::Wait) -> Result<PollStatus, PollError>`.
    PollError mapped to RuntimeError::GpuInit.
12. **map_async + drain + read**: `staging_buffer.slice(..).map_async(MapMode::Read, cb)`
    with an mpsc channel callback; second `device.poll(PollType::Wait)`
    drains the callback; `rx.recv()` returns the map result; bind the
    BufferView to a local, `bytemuck::cast_slice::<u8, f32>(&bytes).to_vec()`,
    explicit `drop(view)` BEFORE `staging_buffer.unmap()` (wgpu panics
    on unmap while a view is alive); defensive `destroy()` on all three
    buffers for prompt GPU-memory reclamation.

### wgpu 26 API specifics (CRITICAL for T46/T47 reuse)
- **`device.poll(poll_type: PollType) -> Result<PollStatus, PollError>`**
  (wgpu 26 signature � NOT `Maintain::Wait` from older versions).
  Use `wgpu::PollType::Wait` to block until queue empty.
  PollStatus has `QueueEmpty` / `WaitSucceeded` / `Poll` variants.
  PollError has only `Timeout`.
- **`map_async` is callback-based, NOT future-based in wgpu 26**:
  `slice(..).map_async(MapMode::Read, |result: Result<(), BufferAsyncError>| { ... })`.
  Callback must be `FnOnce(...) + WasmNotSend + 'static`. Drive
  completion by calling `device.poll(PollType::Wait)` AFTER submit AND
  AFTER the map_async call. Used an mpsc channel to surface the
  callback's result back to the calling thread.
- **`get_mapped_range()` returns `BufferView<'_>`** (a temporary, not
  a borrow into the buffer). For correct unmap-after-read sequencing,
  bind the BufferView to a local (`let view = ...get_mapped_range();`)
  and `drop(view);` BEFORE `staging_buffer.unmap();`. Writing
  `let bytes: &[u8] = &staging_buffer.slice(..).get_mapped_range();`
  would NOT extend the BufferView's lifetime past the statement
  (`bytes` would reference a dropped temporary � clippy::dropping_references
  catches `drop(bytes)` on a `&[u8]` but NOT the subtler issue).
- **`wgpu::util::DeviceExt::create_buffer_init`**: import the trait
  explicitly (`use wgpu::util::DeviceExt;`). Handles `mapped_at_creation`
  + copy + unmap internally. Pads buffer size to COPY_BUFFER_ALIGNMENT=4
  (irrelevant for f32 input which is always 4-aligned).
- **`ComputePipelineDescriptor::entry_point: Option<&str>`**: wgpu 26
  allows `None` if the shader has exactly one entry point. We pass
  `Some("main")` explicitly because T44 codegen names the entry point
  `main` and future shaders might add helpers.
- **`ComputePipelineDescriptor::compilation_options`**: REQUIRED field
  in wgpu 26 (was optional in older versions). Use `PipelineCompilationOptions::default()`.
- **`ComputePipelineDescriptor::cache: Option<&PipelineCache>`**:
  REQUIRED field in wgpu 26. We pass `None` � T47 (cold-start pooling)
  may wire a real cache here.
- **`BindGroupLayoutEntry.ty = BindingType::Buffer { ty: BufferBindingType::Storage { read_only: bool }, has_dynamic_offset, min_binding_size }`**:
  BufferBindingType::Storage is a struct variant with `read_only: bool`
  in wgpu 26. Match T44's shader layout: `read_only: true` for input
  (binding 0), `read_only: false` for output (binding 1).
- **`command_encoder.copy_buffer_to_buffer(src, src_off, dst, dst_off, size: impl Into<Option<BufferAddress>>)`**:
  pass `byte_size` directly (the `impl Into<Option<u64>>` accepts a bare u64).

### Files changed (all under buff-lang-runtime/)
1. `crates/buff-lang-runtime/src/gpu_pipeline.rs` (NEW, ~470 lines).
   WgpuBackend struct, workgroup_count fn, run_dispatch free fn,
   gpu_ctx_err_to_runtime helper, 7 inline unit tests.
2. `crates/buff-lang-runtime/src/lib.rs` (MODIFIED � +1 module decl,
   +1 re-export line for `gpu_pipeline::{workgroup_count, WgpuBackend,
   WORKGROUP_SIZE}`, +6 doc lines in the crate-level doc).
3. `crates/buff-lang-runtime/tests/gpu_dispatch_tests.rs` (NEW, ~560 lines,
   16 integration tests). All names contain `gpu_dispatch` so the QA
   filter `cargo test -p buff-lang-runtime gpu_dispatch` matches all 23
   tests (7 inline unit + 16 integration).
4. `crates/buff-lang-runtime/Cargo.toml`: NO CHANGE � `wgpu` + `bytemuck`
   + `pollster` already in `[dependencies]` from T38; `buff-lang-ast` +
   `buff-lang-codegen-wgsl` + `insta` already in `[dev-dependencies]` from T38b.
5. `.sisyphus/evidence/task-45-gpu-roundtrip.txt` (NEW � raw cargo test
   output proving the QA roundtrip ran on the real GPU).

### Test coverage matrix (24 NEW test points: 7 unit + 16 integration + 1 doc)
- workgroup_count boundaries (7 inline): 0?0, 1?1, 64?1, 65?2, 128?2,
  129?3, plus a property test verifying ceiling-division invariant
  for all n in 1..1000.
- QA roundtrip (1 integration): [1,2,3]?[2,4,6] on real GPU; skips
  with early return on no-GPU hosts.
- Empty input (2 integration): empty ? empty without dispatch (works
  on no-GPU hosts); empty does NOT trigger device init.
- No-GPU graceful path (1 integration): backend from unavailable context
  returns Err(GpuUnavailable), never panics.
- Larger input (1 integration): 1000-element dispatch matches CPU
  oracle within 1e-4 tolerance.
- GPU == CPU oracle (1 integration): {x => x*x + 1} shader, mixed
  sign/magnitude inputs.
- Singleton input (1 integration): [42.0] ? [84.0].
- Workgroup sizing dispatch (1 integration): sizes 1, 64, 65, 128, 129,
  1000 all succeed and match oracle.
- Real GPU with generated WGSL (1 integration): end-to-end test that
  feeds T44's `generate_wgsl` output through the real pipeline on this
  host; asserts has_device()=true after dispatch.
- Object-safety (1 integration): Box<dyn GpuBackend> works.
- Construction + accessors (3 integration): from_context preserves
  context, new() Result shape, has_device() observational.
- Cached device (1 integration): three dispatches share one device-init
  (device_init_count stays at 1).
- Mixed sign/magnitude (1 integration): GPU == CPU oracle for negative
  and large-magnitude inputs.
- Debug repr (1 integration): format!("{backend:?}") contains "WgpuBackend".
- Send + Sync (1 integration): compile-time + Arc<dyn GpuBackend>
  cross-thread dispatch.
- Doctest (1 NEW): workgroup_count rustdoc example.

### GPU-availability-aware testing pattern (reused from T38/T43/T44)
Every real-dispatch test starts with:
```rust
let Some(backend) = try_get_real_backend() else {
    return;  // skip real-GPU assertion on hosts without GPU
};
```
where `try_get_real_backend()` calls `WgpuBackend::new()` and returns
`None` on `GpuUnavailable`. This lets CI hosts without GPU still pass
the test file (they exercise only the host-independent tests: empty
input, no-GPU error path, workgroup_count arithmetic).

### Clippy lints hit during development (all fixed before GREEN)
1. `clippy::manual_div_ceil` on `((len + 63) / 64)`. Fix: use
   `len.div_ceil(WORKGROUP_SIZE) as u32` (Rust 1.73+; we're on 1.95).
2. `clippy::double_must_use` on `pub fn new() -> Result<Self, _>`
   (Result is already must_use). Fix: removed redundant `#[must_use]`.
3. `clippy::approx_constant` on `3.14159` and `2.71828` literals in
   test inputs (math constants PI and E). Fix: used `42.4242` instead.
4. `clippy::dropping_references` on `drop(bytes)` where bytes is `&[u8]`.
   Fix: restructured to bind `let view = ...get_mapped_range();` then
   `drop(view);` before `unmap()` � the actual borrow holder is the
   BufferView, not the byte slice.

### Conventions honored
- All NEW deps via .workspace = true � but T45 added NO new deps. wgpu,
  bytemuck, pollster already in [dependencies] from T38; buff-lang-ast
  + buff-lang-codegen-wgsl + insta already in [dev-dependencies] from T38b.
- Edition 2021, license MIT OR Apache-2.0, version 0.1.0.
- NO [features] section added.
- NO unwrap/expect/panic!/unimplemented!/todo! in non-test code.
  mpsc::Receiver::recv and the send-side `let _ = tx.send(...)` are
  both non-panicking. map_async callback explicitly ignores the send
  error (the receiver may have been dropped if the caller's thread
  terminated � that's correct, the callback's job is to fire the
  completion, not to outlive the caller's interest).
- Derives Debug on WgpuBackend (GpuContext: Debug from T43).
- NOT Clone � wgpu backend handles are uniquely owned.
- No HashMap/HashSet (project hard rule).
- All new test names contain `gpu_dispatch` for filter consistency.
- 4 spaces only. No tabs. No trailing whitespace.
- `#[must_use]` only on `from_context`, `context`, `has_device`
  (workgroup_count and new() would trigger double_must_use per clippy).

### What's deferred (correctly out of scope for T45)
- T46: VRAM tiling (large inputs split across multiple dispatches).
  Current impl uploads the whole input in one buffer; would OOM on
  inputs larger than VRAM.
- T47: Cold-start pooling (pre-warm the device at startup, pipeline
  cache reuse across dispatches). Current impl re-creates shader+pipeline
  per dispatch � acceptable for correctness, suboptimal for throughput.
- T49: `@prefer(gpu)` / `@prefer(cpu)` hints. T45 returns graceful
  Err on no-GPU; T49 will layer an override on top of T40's decide().
- Reductions/scans/gather kernels (post-v1.0 � will extend the trait
  non-breakingly with default-bodied methods).

### MSVC env vars (REQUIRED for test/clippy/build � NOT for cargo check)
Same as T38/T39/T40/T42/T43/T44/T38b. Exact strings used for this task:
```powershell
$env:LIB="C:\BuildTools\VC\Tools\MSVC\14.44.35207\lib\onecore\x64;C:\Program Files (x86)\Windows Kits\10\Lib\10.0.26100.0\um\x64;C:\Program Files (x86)\Windows Kits\10\Lib\10.0.26100.0\ucrt\x64"
$env:INCLUDE="C:\Program Files (x86)\Windows Kits\10\Include\10.0.26100.0\shared;C:\Program Files (x86)\Windows Kits\10\Include\10.0.26100.0\ucrt;C:\Program Files (x86)\Windows Kits\10\Include\10.0.26100.0\um;C:\Program Files (x86)\Windows Kits\10\Include\10.0.26100.0\winrt;C:\Program Files (x86)\Windows Kits\10\Include\10.0.26100.0\cppwinrt;C:\BuildTools\VC\Tools\MSVC\14.44.35207\include"
```

### Verification gate results
- FMT     (cargo fmt -p buff-lang-runtime -- --check):                EXIT 0
- CLIPPY  (cargo clippy -p buff-lang-runtime --all-targets -D warnings): EXIT 0
- TEST    (cargo test -p buff-lang-runtime):                          EXIT 0
                                                                      147 passed + 2 ignored
                                                                      (was 123 after T38b; +24 T45)
- WORKSPACE CHECK (cargo check --workspace):                          EXIT 0

### Gotchas / lessons for T46/T47
1. **map_async is callback-based, not future-based in wgpu 26**. The
   callback is `FnOnce(Result<(), BufferAsyncError>) + WasmNotSend + 'static`.
   Use an mpsc channel to bridge the callback into the calling thread.
   T46 (tiling) will reuse the same pattern for each tile's readback.
2. **Two polls are needed**: one after `queue.submit` (drains the
   dispatch + copy) and one after `map_async` (drains the map
   callback). Forgetting the second poll hangs `rx.recv()` forever.
   T47 may want to batch these into a single poll after multiple
   dispatches share a submit.
3. **`get_mapped_range()` returns a `BufferView`** (owned temporary,
   NOT a borrow into the Buffer). Bind it to a local and `drop(view)`
   BEFORE `unmap()`. The borrow-checker doesn't catch the lifetime
   issue if you slice through a `&[u8]` reference � clippy does.
4. **`GpuContextError` does NOT derive Clone** (only Debug + thiserror::Error).
   T43 stores it in `OnceLock<Result<...>>` which returns `&_`. To
   convert to RuntimeError (which needs owned for the existing From
   impl), match the variants manually � done in `gpu_ctx_err_to_runtime`.
   T46/T47 will likely want to add `Clone, PartialEq` to
   GpuContextError to simplify this � out of scope for T45.
5. **`PipelineCompilationOptions::default()`** is required as a field
   on `ComputePipelineDescriptor` in wgpu 26 � was optional before.
6. **Real GPU dispatch on this box takes ~1 second total** for 16
   tests including device init. Per-dispatch after init is ~30ms.
   T47 cold-start pooling should bring the per-dispatch cost down
   further by amortizing shader+pipeline compilation.
7. **Empty input MUST short-circuit before device init**: wgpu
   forbids 0-sized dispatches AND 0-sized copy_buffer_to_buffer. The
   early-return at the top of `dispatch_map` saves ~1s on empty tests.
8. **Buffer sizes for f32 input are always 4-byte aligned** so
   COPY_BUFFER_ALIGNMENT=4 is satisfied trivially. T46 tiling will
   need to be careful if tile sizes aren't a multiple of 4.
9. **`destroy()` is defensive but recommended**: wgpu frees GPU memory
   on Drop anyway, but explicit `destroy()` triggers faster reclamation
   � important for T46 where many dispatches happen in a loop.


## T46 Findings (2026-07-19)

### Outcome
T46 GREEN: VRAM-aware tiling dispatcher with CPU fallback implemented as
`crates/buff-lang-runtime/src/tiling.rs`. **38 new tests added** (target:
12+; achieved 3.2x). Total buff-lang-runtime tests now **183 passed +
4 ignored** (was 147 passed + 2 ignored after T45). All 4 gates exit 0.
cargo check / test / clippy --all-targets -D warnings / fmt --check all
EXIT 0. cargo check --workspace still EXIT 0.

### Real-GPU tiled dispatch RAN ON THIS BOX
Confirmed: `dispatch_tiled(&WgpuBackend, &generate_wgsl({x:Float=>x*2.0}),
(0..250).map(|i|i as f32).collect::<Vec<f32>>(), 100)` returned a 250-f32
Vec that matched the CPU oracle within 1e-4 tolerance, dispatched through
3 sequential tiles of 100+100+50 elements. Also verified: 1000-element
input at max_tile=100 (10 tiles) produces byte-identical output to a
single non-tiled dispatch (element-wise map is order-independent).

Evidence: `.sisyphus/evidence/task-46-tiling.txt`.

### Public API surface (re-exported from `buff_lang_runtime` crate root)
```rust
// Pure helpers â€” unit-testable without a GPU.
pub fn tile_ranges(total_len: usize, max_tile: usize) -> Vec<(usize, usize)>;
pub fn max_elements_per_tile(vram_budget_bytes: u64, bytes_per_element: u64) -> usize;
pub fn vram_budget_from_device(device: &wgpu::Device) -> u64;

// Mid-level tiled GPU dispatch (no fallback; errors propagate).
pub fn dispatch_tiled(
    backend: &dyn GpuBackend,
    shader_wgsl: &str,
    input: &[f32],
    max_tile_elements: usize,
) -> Result<Vec<f32>, RuntimeError>;

// Fluent wrapper around dispatch_tiled.
pub struct TiledDispatcher<'a> { backend: &'a dyn GpuBackend, max_tile_elements: usize }
impl<'a> TiledDispatcher<'a> {
    pub fn new(backend: &'a dyn GpuBackend, max_tile_elements: usize) -> Self;
    pub fn dispatch(&self, shader_wgsl: &str, input: &[f32]) -> Result<Vec<f32>, RuntimeError>;
    pub fn backend(&self) -> &dyn GpuBackend;
    pub fn max_tile_elements(&self) -> usize;
}

// High-level entry: GPU-tiled with CPU fallback. Always returns Vec<f32>.
pub fn dispatch_map_with_tiling<F: Fn(&[f32]) -> Vec<f32>>(
    gpu_backend: Option<&dyn GpuBackend>,
    shader_wgsl: &str,
    input: &[f32],
    max_tile_elements: usize,
    cpu_oracle: F,
) -> Vec<f32>;
```

### VRAM budget formula (T47 reuses)
```
max_elements_per_tile(vram_budget_bytes, bytes_per_element)
    = vram_budget_bytes / (3 * bytes_per_element)
```
- **3 = headroom factor**: each tile uses THREE buffers (input storage +
  output storage + host-visible staging). Each buffer is `tile_size *
  bytes_per_element` bytes; total VRAM per tile = 3 * tile_size * bpe.
- **vram_budget_bytes** comes from `vram_budget_from_device(device)` =
  `min(max_storage_buffer_binding_size, max_buffer_size)`. wgpu 26 does
  NOT expose total VRAM; the per-buffer binding cap is the practical
  limit. (`max_storage_buffer_binding_size` is u32, always
  `<= max_buffer_size` in practice; the min is defensive for future
  wgpu versions.)
- **Edge cases**: `bytes_per_element == 0` â†’ 0 (avoid div-by-zero);
  `vram < 3*bpe` â†’ 0 (can't fit one element â†’ caller falls back to CPU);
  `3 * bpe` overflow â†’ 0 (saturating_mul prevents wrong small result).

### tile_ranges behavior (documented edge cases)
```
total_len == 0          -> []                          (no tiles)
max_tile == 0           -> [(0, total_len)]            // "no tiling" semantics
total_len <= max_tile   -> [(0, total_len)]            (single tile)
otherwise               -> ceil(total_len/max_tile) tiles, last is partial
```
QA case: `tile_ranges(250, 100) -> [(0,100),(100,200),(200,250)]` (3 tiles).

### `max_tile == 0` semantics â€” IMPORTANT (different per API level)
- **`tile_ranges` / `dispatch_tiled` / `TiledDispatcher`**: `0` means
  "no tiling" â€” a single tile covers the whole input. Caller's manual
  opt-out.
- **`dispatch_map_with_tiling`**: `0` means "VRAM budget too small to
  fit one element â€” CPU fallback". This is because
  `max_elements_per_tile()` returns 0 precisely in that case.
- The two conventions coexist because they serve different callers:
  low-level helper vs high-level entry. Documented in the module-level
  rustdoc.

### CPU fallback decision tree (`dispatch_map_with_tiling`)
1. `input.is_empty()` â†’ return empty Vec (no work).
2. `gpu_backend == None` â†’ run `cpu_oracle(input)` (no GPU adapter).
3. `max_tile_elements == 0` â†’ run `cpu_oracle(input)` (can't fit one
   element through VRAM budget formula).
4. `dispatch_tiled(...)` succeeds â†’ return GPU output.
5. `dispatch_tiled(...)` errors â†’ run `cpu_oracle(input)` (defensive â€”
   includes `GpuUnavailable`, `GpuInit`, etc).

The CPU oracle is typically T38b's `cpu_fallback_map` (sequential,
deterministic) or T39's `CpuDispatcher::par_map` (parallel, rayon).
Both return `Vec<f32>` directly (infallible) â€” so the high-level entry
can promise to always return a value: GPU failure is invisible to the
caller. This is the contract the T46 task spec demands.

### Files changed (all under buff-lang-runtime/ â€” no other crate touched)
1. `crates/buff-lang-runtime/src/tiling.rs` (NEW, ~430 lines).
   - `tile_ranges` pure helper (~30 lines + rustdoc).
   - `max_elements_per_tile` pure helper (~25 lines + rustdoc).
   - `vram_budget_from_device` query (~10 lines + rustdoc).
   - `dispatch_tiled` mid-level dispatch (~40 lines + rustdoc).
   - `TiledDispatcher` struct + impl (~50 lines + rustdoc).
   - `dispatch_map_with_tiling` high-level entry (~45 lines + rustdoc).
   - 11 inline unit tests (`#[cfg(test)] mod tests`).
2. `crates/buff-lang-runtime/src/lib.rs` (MODIFIED â€” +1 module decl,
   +1 re-export block with 6 items, +6 doc lines in crate-level docs).
3. `crates/buff-lang-runtime/tests/tiling_tests.rs` (NEW, ~370 lines,
   22 integration tests). All test names contain `tiling` so the QA
   filter `cargo test -p buff-lang-runtime tiling` matches the whole
   suite (inline + integration + doctests).
4. `crates/buff-lang-runtime/Cargo.toml`: **NO CHANGE** â€” all required
   deps (`wgpu`, `bytemuck`, `pollster`) already in `[dependencies]`
   from T38; `buff-lang-ast` + `buff-lang-codegen-wgsl` + `insta` already
   in `[dev-dependencies]` from T38b. T46 added ZERO new deps.
5. `.sisyphus/evidence/task-46-tiling.txt` (NEW â€” raw cargo test output
   proving the QA roundtrip ran on the real GPU + 4-gate summary).

### Test coverage matrix (38 NEW test points)
- **tile_ranges** (11 inline + 6 integration = 17 tests):
  QA 250/100 â†’ 3 tiles; empty input â†’ empty; input â‰¤ max â†’ 1 tile;
  exact multiple (200/100, 300/100); max_tile=0 disables tiling;
  max_tile=1 yields N tiles; singleton input.
- **max_elements_per_tile** (4 inline + 2 integration = 6 tests):
  basic formula (1200/12=100, 2400/24=100); 4 GiB budget â†’
  357_913_941 elements; budget-too-small (11 bytes < 12 â†’ 0);
  zero budget â†’ 0; zero bpe â†’ 0; exactly fits one element (12/12=1).
- **dispatch_tiled via MockGpuBackend** (6 integration):
  QA 250/100 â†’ 3 recorded dispatches + output == CPU oracle;
  input-order preservation across tiles (per-tile offset encoding);
  per-tile input lengths (100/100/50 for 250@100); single tile when
  input fits; empty input â†’ no dispatch.
- **TiledDispatcher struct API** (2 integration):
  dispatch produces same output as free fn; accessors
  (`max_tile_elements()`, `backend()`).
- **dispatch_map_with_tiling CPU fallback** (5 integration):
  `None` backend â†’ CPU; `max_tile=0` â†’ CPU even with backend; empty
  input â†’ empty Vec; GPU error (`GpuUnavailable` from unavailable
  context) â†’ CPU fallback fires; happy path (mock backend + max_tile>0)
  uses GPU path.
- **Real-GPU tiled dispatch** (3 integration â€” ALL RAN on this host):
  250 elements at max_tile=100 via WgpuBackend matches CPU oracle;
  1000 elements at max_tile=100 (10 tiles) == single dispatch; high-level
  `dispatch_map_with_tiling` with real GPU produces oracle output.
- **Doc-tests** (5 NEW: 3 active + 2 ignored):
  `tile_ranges` QA snippet; `max_elements_per_tile` formula snippet;
  `dispatch_map_with_tiling` CPU-only example; `TiledDispatcher` +
  `vram_budget_from_device` are `ignore`-tagged (they reference types
  across module boundaries that confuse the doctest runner on some
  toolchains â€” same pattern as T38b's MockGpuBackend doctests).

### Determinism + no-unwrap contract
- **Tiles processed sequentially in input order**: `for (start, end) in
  ranges { output.extend(backend.dispatch_map(shader, &input[start..end])?); }`
  No interior reordering, no hashing, no threads.
- **Pre-allocated output**: `Vec::with_capacity(input.len())` â€” zero
  reallocations as tiles append.
- **NO `unwrap`/`expect`/`panic!`/`todo!`/`unimplemented!`** in non-test
  code. `usize::try_from(...).unwrap_or(usize::MAX)` is `unwrap_or`
  (not `unwrap`) â€” handles 32-bit platform u64â†’usize overflow soundly.
- **No HashMap/HashSet** anywhere in the module.

### Conventions honored
- All deps via `.workspace = true`; T46 added ZERO new deps.
- Edition 2021, license MIT OR Apache-2.0, version 0.1.0.
- NO `[features]` section added.
- `#[must_use]` on `tile_ranges`, `max_elements_per_tile`,
  `vram_budget_from_device`, `dispatch_map_with_tiling`,
  `TiledDispatcher::new`, `TiledDispatcher::backend`,
  `TiledDispatcher::max_tile_elements`.
- `#[derive(Debug)]` on `TiledDispatcher<'a>` (works because
  `&dyn GpuBackend: Debug` via the trait's `Debug` supertrait bound).
- All new test names contain `tiling` for filter consistency.
- 4 spaces only. No tabs. No trailing whitespace.

### wgpu 26 API specifics (verified)
- **`device.limits()` returns `wgpu::Limits` by value** â€” copying the
  fields we need (`max_storage_buffer_binding_size: u32`,
  `max_buffer_size: u64`) is cheap.
- **`max_storage_buffer_binding_size` is `u32`** in wgpu 26 â€” cast via
  `u64::from(...)` (zero-cost widening).
- **`max_buffer_size` is `u64`** in wgpu 26 â€” direct assignment to `u64`
  local works without any cast.
- Both fields are `Copy` (lifted from `Limits` which is `Clone + Copy`).

### Verification gate results
- FMT     (cargo fmt -p buff-lang-runtime -- --check):                EXIT 0
- CLIPPY  (cargo clippy -p buff-lang-runtime --all-targets -D warnings): EXIT 0
- TEST    (cargo test -p buff-lang-runtime):                          EXIT 0
                                                                       183 passed + 4 ignored
                                                                       (was 147 + 2; +36 pass + 2 ignored)
- WORKSPACE CHECK (cargo check --workspace):                          EXIT 0

### Gotchas / lessons for T47
1. **T47 (cold-start pooling) can reuse `vram_budget_from_device`**
   verbatim â€” the formula `max_elements_per_tile(vram, bpe) = vram / (3*bpe)`
   is stable. T47 will likely add buffer-pool reuse (keep the three
   per-tile buffers alive across dispatches instead of recreating them).
   The factor of 3 stays the same.
2. **`GpuBackend` trait has no `has_device()` method** â€” the high-level
   `dispatch_map_with_tiling` therefore decides GPU vs CPU by ATTEMPTING
   the dispatch and catching errors. This is more robust than pre-checking
   (the backend could fail mid-dispatch anyway). T49 (`@prefer` hints)
   may want to add `has_device()` to the trait for explicit hint-driven
   routing â€” currently the routing is purely reactive.
3. **`MockGpuBackend` never errors** â€” to test the CPU-fallback-on-error
   path, use `WgpuBackend::from_context(GpuContext::unavailable())`
   which always returns `Err(GpuUnavailable)` from `dispatch_map`. This
   is the same trick T45's `unavailable_backend()` helper uses.
4. **Tiled output == single-dispatch output** is provable for ANY
   element-wise map (the kernel has no inter-element dependencies).
   Reductions/scans (post-v1.0) will need a different tiling strategy
   (cross-tile combining step). T46 only covers maps.
5. **The `cpu_oracle` parameter is `Fn(&[f32]) -> Vec<f32>`** (not
   `Fn(f32) -> f32`) so callers can wrap richer CPU computations (e.g.
   `|input| cpu_dispatcher.par_map(input.to_vec(), |x| x * 2.0)` for
   T39's parallel CPU path, or `|input| cpu_fallback_map(input, |x|
   x * 2.0)` for T38b's sequential oracle).
6. **rustfmt wraps long `assert_eq!` macro args past 100 cols** â€”
   pre-emptively break them across lines. The `tile_ranges(300, 100)`
   test assertion does this.
7. **`u64::from(limits.max_storage_buffer_binding_size)`** avoids
   `as u64` which would trigger `clippy::unnecessary_cast` if the field
   were already u64. Reflexive `From<u32> for u64` is the idiomatic
   zero-cost widening.
8. **No MSVC env vars needed for `cargo check`** â€” only `cargo test`
   and `cargo clippy --all-targets` need them (link step requires
   `msvcrt.lib`). Same convention as T38-T45.

### What's deferred (correctly out of scope for T46)
- T47: cold-start pooling (pre-warm device at startup, pipeline cache
  reuse across dispatches). Currently `dispatch_tiled` recreates the
  shader+pipeline per tile via T45's `WgpuBackend::dispatch_map` â€”
  acceptable for correctness, suboptimal for throughput. T47 will
  likely add a per-backend pipeline cache keyed by shader source hash.
- T48: recursion detection (CPU-only marking for recursive functions).
- T49: `@prefer(gpu)` / `@prefer(cpu)` hints. Will layer an override
  on top of `dispatch_map_with_tiling`'s decision tree.
- Reductions/scans with cross-tile combining (post-v1.0).
- Multi-GPU dispatch (one tile per GPU, run in parallel) â€” post-v1.0.

### MSVC env vars (REQUIRED for test/clippy/build â€” NOT for cargo check)
Same as T38/T39/T40/T42/T43/T44/T38b/T45. Exact strings used for this task:
```powershell
$env:LIB="C:\BuildTools\VC\Tools\MSVC\14.44.35207\lib\onecore\x64;C:\Program Files (x86)\Windows Kits\10\Lib\10.0.26100.0\um\x64;C:\Program Files (x86)\Windows Kits\10\Lib\10.0.26100.0\ucrt\x64"
$env:INCLUDE="C:\Program Files (x86)\Windows Kits\10\Include\10.0.26100.0\shared;C:\Program Files (x86)\Windows Kits\10\Include\10.0.26100.0\ucrt;C:\Program Files (x86)\Windows Kits\10\Include\10.0.26100.0\um;C:\Program Files (x86)\Windows Kits\10\Include\10.0.26100.0\winrt;C:\Program Files (x86)\Windows Kits\10\Include\10.0.26100.0\cppwinrt;C:\BuildTools\VC\Tools\MSVC\14.44.35207\include"
```



## T47 Findings (2026-07-19)

### Outcome
T47 GREEN: cold-start mitigation (pipeline caching + buffer pooling +
async device init) implemented as `crates/buff-lang-runtime/src/cold_start.rs`.
**33 new tests added** (14 inline unit + 18 integration + 1 ignored
doctest; target was 12+, achieved 2.75x). Total buff-lang-runtime tests
now **215 passed + 5 ignored** (was 183 + 4 after T46).
cargo check / test / clippy --all-targets -D warnings / fmt --check all
exit 0. cargo check --workspace still exit 0.

### Real-GPU cold-start QA RAN ON THIS BOX
Confirmed: dispatching the SAME WGSL shader twice through
`ColdStartBackend` compiles the pipeline EXACTLY ONCE
(`pipeline_compile_count() == 1` after 3 dispatches). Dispatching two
DIFFERENT shaders compiles twice (`pipeline_compile_count() == 2`).
Dispatching the same shader + same input size 7 times allocates
exactly 3 buffers (`buffer_allocation_count() == 3` — pool reuse
steady-state). All on a real wgpu device.

Evidence: `.sisyphus/evidence/task-47-cold-start.txt`.

### Public API surface (re-exported from buff_lang_runtime crate root)
```rust
pub struct PipelineCache { /* private */ }
impl PipelineCache {
    pub fn new() -> Self;
    pub fn compile_count(&self) -> usize;   // cache miss count
    pub fn len(&self) -> usize;
    pub fn is_empty(&self) -> bool;
    pub fn contains(&self, shader_wgsl: &str) -> bool;
    // pub(crate) fn get_or_compile(device, shader) -> Result<CachedPipeline, RuntimeError>
}

pub struct BufferPool { /* private */ }
impl BufferPool {
    pub fn new() -> Self;
    pub fn allocation_count(&self) -> usize;  // pool miss count
    pub fn free_count(&self) -> usize;
    pub fn is_empty(&self) -> bool;
    // pub(crate) fn acquire(device, size, usage) -> Result<wgpu::Buffer, RuntimeError>
    // pub(crate) fn release(size, usage, buffer)
}

pub struct ColdStartBackend { /* private */ }
impl ColdStartBackend {
    pub fn new() -> Result<Self, RuntimeError>;
    pub fn from_backend(backend: WgpuBackend) -> Self;
    pub fn from_context(context: GpuContext) -> Self;
    pub fn inner(&self) -> &WgpuBackend;
    pub fn context(&self) -> &GpuContext;
    pub fn has_device(&self) -> bool;
    pub fn pipeline_compile_count(&self) -> usize;
    pub fn buffer_allocation_count(&self) -> usize;
    pub fn cached_pipeline_count(&self) -> usize;
    pub fn pooled_buffer_count(&self) -> usize;
    pub fn spawn_init(&self) -> Result<(), RuntimeError>;
    pub fn is_ready(&self) -> bool;
    pub fn wait_ready_blocking(&self);
    pub async fn wait_ready(&self);
}
impl Default for ColdStartBackend;  // = from_context(GpuContext::unavailable())
impl GpuBackend for ColdStartBackend;  // drop-in for WgpuBackend
```

### Cache design (BTreeMap, not HashMap — full justification in module rustdoc)
- **Key**: `String` (WGSL source text — totally orderable).
- **Value**: `CachedPipeline` (ShaderModule + BindGroupLayout +
  PipelineLayout + ComputePipeline — all Arc-backed cheap-to-clone
  handles in wgpu 26).
- **Counter**: `AtomicUsize compile_count` incremented ONLY on cache
  miss (i.e. when `create_compute_pipeline` actually runs). Concurrent
  first-time dispatches of the same shader serialize on the map Mutex;
  the second caller sees the entry inserted by the first (compile_count
  is a precise count of distinct compilations, not distinct callers).
- **Project rule compliance**: BTreeMap chosen over HashMap for
  deterministic iteration order. The HARD RULE forbids HashMap in
  codegen/graph paths; a runtime pipeline cache is technically
  exempt but we use BTreeMap anyway to stay consistent with the rule's
  spirit.

### Buffer pool design
- **Key**: `(u64 byte_size, BufferUsageKey)` where `BufferUsageKey` is
  a totally-ordered newtype over `u32` (`wgpu::BufferUsages::bits()`).
  Required because `wgpu::BufferUsages` derives Eq+Hash but NOT Ord
  (bitflags types don't auto-derive Ord).
- **Free-list**: `Vec<wgpu::Buffer>` per key, LIFO pop/push for best
  GPU allocator locality. `acquire()` pops from end; `release()`
  pushes to end.
- **Counter**: `AtomicUsize allocation_count` incremented ONLY on pool
  miss (i.e. when `device.create_buffer` actually runs).
- **Lifecycle safety**: staging buffer is `unmap()`'d BEFORE `release()`
  so the next caller can `map_async` without hitting wgpu's
  "already mapped" panic. Input/output buffers are never mapped (only
  bound + written), so they're returned as-is. Pool release happens
  STRICTLY AFTER readback completes.
- **No zeroing on release**: defensive zeroing was considered but
  skipped — the compute pass always fully writes the output before
  readback, and the input is always fully overwritten via
  `queue.write_buffer` before the next dispatch's compute pass reads
  it. Keeps the release path allocation-free.

### Async init design
- **`spawn_init()`**: uses `std::thread::Builder::spawn` (not
  `tokio::spawn`) so it works without an active tokio runtime. The
  spawned thread calls `Arc<WgpuBackend>::context().device_queue()` to
  warm T43's OnceLock cache. Errors are swallowed (cached on context
  OnceLock just like a sync call would).
- **`is_ready()`**: reads an `Arc<AtomicBool>` set by the spawned
  thread's completion callback.
- **`wait_ready_blocking()`**: `JoinHandle::join()` on the spawned
  thread. Returns immediately if `spawn_init` was never called
  (`spawn_attempted` flag check) — this is critical to prevent an
  infinite spin-yield loop.
- **`wait_ready()`** (async): uses `tokio::task::spawn_blocking` to
  await the std JoinHandle from within an async context without
  blocking the runtime.
- **Idempotency**: `spawn_init` checks-and-sets `spawn_attempted`
  atomic flag with `swap(true, AcqRel)`. Second call is a no-op.
- **Arc<WgpuBackend> sharing**: `ColdStartBackend` holds
  `Arc<WgpuBackend>` so the spawned thread can clone the Arc and
  call `backend.context().device_queue()` (the &self method).

### Files changed (all under buff-lang-runtime/ — no other crate touched)
1. `crates/buff-lang-runtime/src/cold_start.rs` (NEW, ~900 lines):
   PipelineCache + BufferPool + ColdStartBackend + 14 inline unit tests.
2. `crates/buff-lang-runtime/src/gpu_pipeline.rs` (MODIFIED — promoted
   `gpu_ctx_err_to_runtime` from `fn` to `pub(crate) fn`).
3. `crates/buff-lang-runtime/src/lib.rs` (MODIFIED — +1 module decl,
   +1 re-export block with 3 items, +9 doc lines in crate-level docs).
4. `crates/buff-lang-runtime/tests/cold_start_tests.rs` (NEW, ~480
   lines, 18 integration tests). All test names contain `cold_start`.
5. `crates/buff-lang-runtime/Cargo.toml`: **NO CHANGE**.
6. `.sisyphus/evidence/task-47-cold-start.txt` (NEW).

### Test coverage matrix (33 NEW test points)
- **PipelineCache state (3 inline)**: default_is_empty,
  compile_count_starts_at_zero, contains_returns_false_for_any_unseen_shader.
- **BufferPool state (2 inline)**: default_is_empty,
  allocation_count_starts_at_zero.
- **ColdStartBackend no-GPU state (8 inline)**: default_is_unavailable,
  is_ready_false_before_spawn, wait_ready_blocking_returns_if_never_spawned
  (the spin-loop bug catch), spawn_init_idempotent_on_unavailable_context,
  dispatch_empty_input_no_compile_no_alloc,
  dispatch_on_unavailable_context_returns_unavailable, send_sync_compile_time,
  has_debug_repr.
- **BufferUsageKey ordering (1 inline)**: orders_correctly.
- **QA case (1 integration)**: same shader dispatched 3 times → compile count 1.
- **Distinct shaders (1 integration)**: 2 different shaders → compile count 2;
  re-dispatching both → still 2.
- **Buffer pool reuse (1 integration)**: same shader+size 7x → 3 allocations;
  different size triggers 3 more (now 6); reuse across both sizes verified.
- **Async init (4 integration)**: is_ready_false_before_spawn,
  is_ready_true_after_wait_ready_blocking, idempotent,
  warms_device_on_real_gpu (asserts device_init_count == 1 after spawn).
- **Async wait_ready (1 integration, #[tokio::test])**: wait_ready_async_path.
- **Roundtrip correctness unchanged (3 integration)**: QA 1,2,3→2,4,6;
  matches CPU oracle across 5 cached dispatches;
  singleton 42→84 after cache warm.
- **No-GPU graceful (2 integration)**: returns GpuUnavailable (never panic);
  empty input returns empty Vec.
- **Object-safety + Send+Sync (2 integration)**: Box<dyn GpuBackend> works;
  Arc<ColdStartBackend> cross-thread dispatch works.
- **Construction + accessors (2 integration)**: from_context/default yield
  unavailable; accessors_initial_state_zero.
- **Multiple input sizes coexist (1 integration)**: 5 distinct sizes allocated
  15 buffers; re-dispatching all 5 sizes served entirely from pool (0 new
  allocations); pipeline_compile_count stayed at 1.
- **Doctest (1 ignored)**: ColdStartBackend example in rustdoc — `ignore`-tagged
  (references multiple types across module boundaries like T38b/T46).

### Clippy lints hit during development (all fixed before GREEN)
1. `dead_code` warning on `shader_module` and `pipeline_layout` fields
   of `CachedPipeline` (kept alive defensively but not directly read
   after `create_compute_pipeline`). Fix: `#[allow(dead_code)]` on the
   struct with a rustdoc comment explaining the defensive retention.
2. `unused_imports` on `wgpu::util::DeviceExt` and `GpuContextError`
   (initially imported but not used after refactoring away from
   `create_buffer_init`). Fix: removed both imports.

### Dispatch flow bug caught during development
Initial implementation of `wait_ready_blocking` had an infinite
spin-yield loop when `spawn_init` was never called: `is_ready()` was
false, no thread would ever set the flag, so the while-loop spun
forever. Caught by the inline test
`cold_start_cold_start_backend_wait_ready_blocking_returns_if_never_spawned`
timing out at 60s. Fix: check `spawn_attempted` flag at the top of
`wait_ready_blocking` / `wait_ready` and return early if no task was
ever scheduled. Same fix applied to both sync and async variants.

### wgpu 26 API specifics (T47 additions to the T45 knowledge base)
- **`ComputePipeline` / `BindGroupLayout` / `PipelineLayout` / `ShaderModule`
  are all Arc-backed cheap-to-clone handles in wgpu 26**. Cloning is one
  atomic increment + small struct copy. The expensive driver work
  (SPIR-V → ISA compilation, descriptor-set layout negotiation) happens
  once during the `create_*` calls; subsequent clones reuse the cached
  driver state. This is the win that makes pipeline caching worthwhile.
- **`wgpu::Buffer` is also Arc-backed** — cloning is cheap. Buffer pool
  stores clones of the same Arc handle, so acquiring from the pool is
  a single atomic increment.
- **`wgpu::BufferUsages` derives `Eq + Hash + Copy` but NOT `Ord`**
  (it's a `bitflags!` type). To use it as a BTreeMap key, wrap the
  underlying `u32` bits in a newtype that derives `Ord`.
- **`queue.write_buffer(&buffer, offset, bytes)`** replaces the need
  for `create_buffer_init` (which always allocates a fresh buffer).
  Used in the cached dispatch path to upload input data into a pooled
  buffer (which is uninitialized after pool-acquire).
- **Staging buffer must be `unmap()`'d before next `map_async`**. The
  pool's `release()` does NOT call `unmap()` itself — the dispatch
  path is responsible for calling `staging_buffer.unmap()` BEFORE
  calling `release()` for the staging buffer.

### Integration with T45 / T43 / T46
- **T45 (WgpuBackend)**: NO PUBLIC API CHANGE. The
  `gpu_ctx_err_to_runtime` helper was promoted from `fn` to
  `pub(crate) fn` so cold_start::ColdStartBackend::dispatch_map can
  reuse it. ColdStartBackend wraps WgpuBackend in `Arc<WgpuBackend>`
  and provides an alternative dispatch_map implementation that adds
  cache + pool — it does NOT modify WgpuBackend's behavior. T45's
  full suite (16 integration + 7 inline) still passes unchanged.
- **T43 (GpuContext)**: spawn_init() warms the OnceLock cache by
  calling `GpuContext::device_queue()`. This is a `&self` method on
  GpuContext, so the background thread needs only `&Arc<WgpuBackend>`
  (which holds `GpuContext`). device_init_count after spawn_init ==
  1, proven by `test_cold_start_async_init_warms_device_on_real_gpu`.
- **T46 (tiling)**: NOT MODIFIED. T46's `dispatch_tiled` /
  `TiledDispatcher` / `dispatch_map_with_tiling` are agnostic to the
  backend type — they take `&dyn GpuBackend`. T46 could route through
  ColdStartBackend instead of WgpuBackend for additional speedup
  (the pipeline cache would amortize across tiles since they share
  the same shader), but T47 does NOT change T46's default behavior.

### Conventions honored
- All deps via `.workspace = true`; T47 added ZERO new deps.
- Edition 2021, license MIT OR Apache-2.0, version 0.1.0.
- NO `[features]` section added.
- NO `unwrap`/`expect`/`panic!`/`todo!`/`unimplemented!` in non-test
  code. `Mutex::lock` errors map to `RuntimeError::GpuInit` via
  `map_err`. `std::thread::Builder::spawn` errors map to
  `RuntimeError::GpuInit`. The init thread swallows
  `device_queue()` errors (they're cached on the context OnceLock).
- Derives Debug on ColdStartBackend + PipelineCache + BufferPool.
  Manual Debug impl on InitState (JoinHandle isn't Debug; uses
  finish_non_exhaustive).
- NOT Clone — Arc<WgpuBackend> field is shared, not cloneable.
- BTreeMap, not HashMap (project hard rule; full justification in
  module-level rustdoc).
- All new test names contain `cold_start` for QA filter consistency.
- 4 spaces only. No tabs. No trailing whitespace.
- `#[must_use]` on accessors returning useful info
  (pipeline_compile_count, buffer_allocation_count,
  cached_pipeline_count, pooled_buffer_count, has_device, inner,
  context, from_backend, from_context).

### Verification gate results
- FMT     (cargo fmt -p buff-lang-runtime -- --check):                EXIT 0
- CLIPPY  (cargo clippy -p buff-lang-runtime --all-targets -- -D warnings): EXIT 0
- TEST    (cargo test -p buff-lang-runtime):                          EXIT 0
                                                                       215 passed + 5 ignored
                                                                       (was 183 + 4; +32 pass + 1 ignored)
- WORKSPACE CHECK (cargo check --workspace):                          EXIT 0

### Gotchas / lessons for T48+
1. **BTreeMap needs Ord keys, wgpu bitflags types don't auto-derive Ord**.
   When using `wgpu::BufferUsages` (or any bitflags type) as a BTreeMap
   key, wrap it in a newtype over its `u32` bits.
2. **`spawn_init` + `wait_ready_*` need a "never spawned" early-return**.
   Without it, `wait_ready_blocking()` enters an infinite spin-yield
   loop when `spawn_init` was never called — `is_ready()` is false,
   no thread will ever set the flag. The fix: check `spawn_attempted`
   at the top of the wait function and return early.
3. **`tokio::task::spawn_blocking` requires an active tokio runtime**.
   The async `wait_ready()` will panic if called outside a runtime.
   Provide a sync `wait_ready_blocking()` alternative for non-async
   callers.
4. **Pool release AFTER unmap**: the dispatch path must call
   `staging_buffer.unmap()` BEFORE `buffer_pool.release(staging)`.
   Otherwise the next caller's `map_async(Read)` panics with
   "buffer already mapped".
5. **`queue.write_buffer` replaces `create_buffer_init` for pooled
   buffers**: `create_buffer_init` allocates a NEW buffer every call,
   defeating the pool. The cached dispatch path uses
   `device.create_buffer` (in `BufferPool::acquire`) to allocate
   fresh, then `queue.write_buffer` to upload input data into the
   pooled buffer.
6. **CachedPipeline holds 4 Arc handles, not just ComputePipeline**.
   `#[allow(dead_code)]` on the struct.
7. **PipelineCache holds the Mutex for the duration of the compile
   on a miss** — concurrent first-time dispatches of DIFFERENT
   shaders serialize. Alternative: lock only for the map read,
   drop the lock during compile, re-lock for insert. T47 chose the
   DROP-THEN-RE-LOCK variant for better concurrency.
8. **`tokio::task::yield_now().await` for async spin-yield**: the
   async `wait_ready()` uses `tokio::task::yield_now().await` instead
   of `std::thread::yield_now()` because the latter would block the
   async runtime's worker thread.

### What's deferred (correctly out of scope for T47)
- **T48**: recursion detection (CPU-only marking for recursive functions).
- **T49**: `@prefer(gpu)` / `@prefer(cpu)` hints layered over T40's
  `decide()`. T47's ColdStartBackend is hint-agnostic — it caches
  whatever shader it's given.
- **T50**: GPU memory alignment / packing concerns (vec3, struct
  layouts).
- **Pipeline cache eviction policy**: T47's cache is unbounded (real-
  world Buff programs have a small finite set of distinct shaders).
- **Pipeline cache persistence** (across process restarts): wgpu 26's
  `ComputePipelineDescriptor::cache: Option<&PipelineCache>` field
  accepts a wgpu-level cache object. T47 passes `None`.
- **Cross-thread dispatch via cached backend**: the integration test
  `test_cold_start_send_sync_across_threads` proves compile-time +
  runtime Send+Sync, but doesn't exercise the cache under concurrent
  dispatch (multi-threaded cache-hit race).

### MSVC env vars (REQUIRED for test/clippy/build — NOT for cargo check)
Same as T38/T39/T40/T42/T43/T44/T38b/T45/T46. Exact strings used for
this task:
```powershell
$env:LIB="C:\BuildTools\VC\Tools\MSVC\14.44.35207\lib\onecore\x64;C:\Program Files (x86)\Windows Kits\10\Lib\10.0.26100.0\um\x64;C:\Program Files (x86)\Windows Kits\10\Lib\10.0.26100.0\ucrt\x64"
$env:INCLUDE="C:\Program Files (x86)\Windows Kits\10\Include\10.0.26100.0\shared;C:\Program Files (x86)\Windows Kits\10\Include\10.0.26100.0\ucrt;C:\Program Files (x86)\Windows Kits\10\Include\10.0.26100.0\um;C:\Program Files (x86)\Windows Kits\10\Include\10.0.26100.0\winrt;C:\Program Files (x86)\Windows Kits\10\Include\10.0.26100.0\cppwinrt;C:\BuildTools\VC\Tools\MSVC\14.44.35207\include"
```

## T48 Findings — Recursion detection (call-graph cycle → cpu_only)

### Task summary
Implemented `crates/buff-lang-types/src/recursion.rs` — builds a deterministic
call graph (REUSES `async_analysis::build_call_graph` from T31), runs DFS
cycle detection with an on-stack set, and marks every function on any cycle
into `RecursionFacts::cpu_only`. `@prefer(gpu)` on a recursive function →
`Err(TypeError)` with a deterministic, lexicographically-first offender name.

### Files changed (ADDITIVE — no other crates touched)
- **NEW** `crates/buff-lang-types/src/recursion.rs` (~600 lines):
  - `pub type CallEdges = BTreeMap<String, BTreeSet<String>>`
  - `pub struct RecursionFacts { cpu_only: BTreeSet<String> }` with
    `is_cpu_only(name) -> bool`, `len`, `is_empty`, `to_sorted_vec`.
  - `pub fn build_call_graph(decls) -> CallEdges` — thin adapter over T31's
    `async_analysis::build_call_graph`. Returns the raw BTreeMap.
  - `pub fn detect_cycles(graph: &CallGraph) -> BTreeSet<String>` — DFS
    with explicit `on_stack` set + sorted iteration (deterministic).
  - `pub fn has_prefer_gpu_attr(f: &FuncDecl) -> bool` — exact-match for
    `@prefer(gpu)` (name="prefer", args=["gpu"]).
  - `pub fn analyze_recursion(decls) -> Result<RecursionFacts, TypeError>`
    — main entry point. Combines build_call_graph + detect_cycles + the
    @prefer(gpu) conflict check.
- **EDIT** `crates/buff-lang-types/src/lib.rs`: added `pub mod recursion;`
  + re-exports `analyze_recursion`, `detect_cycles`, `has_prefer_gpu_attr`,
  `RecursionFacts` at crate root.
- **NEW** `crates/buff-lang-types/tests/recursion_test.rs` (18 tests).
- **NEW** `.sisyphus/evidence/task-48-recursion.txt` — captured test output.

### `@prefer(gpu)` representation in the AST (CRITICAL for T49)
The Buff AST ALREADY supports `@prefer(gpu)` from T35 — no AST changes
were needed for T48. The shape is:
```rust
// crates/buff-lang-ast/src/decl.rs (T35)
pub struct Attribute {
    pub name: Ident,            // "prefer"
    pub args: Vec<String>,      // vec!["gpu".to_string()]
    pub span: Span,
}
```
- Lives on `FuncDecl::attributes: Vec<Attribute>` (declared in T35).
- Parser already accepts `@name(arg, arg, ...)` form (see
  `crates/buff-lang-parser/src/stmt.rs` line ~2484: `parse_attributes`).
- For T49 hint-driven codegen, the predicate is:
  `f.attributes.iter().any(|a| a.name.name == "prefer" && a.args.len() == 1 && a.args[0] == "gpu")`
  — exactly what `recursion::has_prefer_gpu_attr(f)` implements (exported
  at crate root for T49 reuse).
- Future `@prefer(cpu)` and other targets use the same shape — T49 will
  add a sibling `has_prefer_cpu_attr(f)` (or generalise to a
  `PreferTarget` enum).
- Multi-arg `@prefer(gpu, force)` is INTENTIONALLY not matched — only
  the exact `args == ["gpu"]` form counts. If T49 wants to honour
  multi-arg hints, it should widen the predicate.

### Call-graph / DFS design (mirrors async_analysis + modules.rs)
- **Determinism HARD rule**: BTreeMap/BTreeSet only, sorted iteration
  everywhere. No HashMap/HashSet (T29 flaky-test lesson).
- **Reused**: `async_analysis::build_call_graph` (T31) — it already walks
  every compound expression (if/match/lambda/for/binop/string-interp/
  struct-init/...) and records bare-ident callee names. NO duplication.
- **DFS** (in `detect_cycles`): for each unvisited node (sorted by name),
  launch a DFS maintaining `on_stack: BTreeSet<String>` (current path)
  + `stack: Vec<String>` (for marking the cycle slice). When a callee
  already on `on_stack` is encountered, every node from that callee
  upward in `stack` (inclusive) is on a cycle → inserted into
  `on_cycle: BTreeSet<String>`.
- **Edges to undeclared callees** (prelude `print`, free vars): the T31
  walker RECORDS these but they have no graph node. The DFS skips any
  callee not in `graph.edges` — they can't close a cycle.
- **On-cycle classification only** (NOT transitive): a function that
  merely CALLS a recursive fn is NOT cpu_only. The spec defines
  "recursive = on a cycle" explicitly. Transitive marking ("calls
  cpu_only") is deferred to T49's hint-driven codegen if needed.
- **No `unwrap`/`expect`/`panic`/`todo`** in non-test code (hard rule).
- Recursion depth = call-graph depth, ≤ number of declared functions.
  Realistic Buff programs are tiny; default 8 MB stack is plenty.

### Error type
- `TypeError` (from `buff_lang_error`) is a STRUCT wrapping a
  `Diagnostic`, not an enum. NO variant changes needed. The error
  message is:
  ```
  cannot @prefer(gpu) on recursive function `<name>`: recursion is not GPU-dispatchable
  ```
- Deterministic: offenders are collected into a BTreeSet and the
  lex-smallest name is reported first.

### QA confirmed (test names contain `recursion` per filter convention)
1. **fib → cpu_only==true**: `recursion_qa_fib_calls_fib_minus_one_and_two_marks_cpu_only`
   constructs `fib(n) { fib(n-1) + fib(n-2) }` and asserts
   `facts.is_cpu_only("fib") == true`. ✅
2. **non-recursive double → NOT cpu_only**: `recursion_qa_non_recursive_double_not_cpu_only`
   asserts `facts.is_cpu_only("double") == false`. ✅
3. **@prefer(gpu) on recursive → Err**: `recursion_prefer_gpu_on_recursive_returns_err`
   asserts `Err` with message containing `\`fib\``, `@prefer(gpu)`, `recursive`. ✅
4. **@prefer(gpu) on non-recursive → Ok**: `recursion_prefer_gpu_on_non_recursive_returns_ok`. ✅
5. **mutual recursion a↔b → both cpu_only** ✅
6. **3-cycle a→b→c→a → all cpu_only** ✅
7. **deep non-recursive chain a→b→c→d → none cpu_only** ✅
8. **caller-of-recursive-fn (not on cycle) → NOT cpu_only** ✅
9. **disconnected components: self-loop + chain + isolated → only self-loop** ✅
10. **empty program → empty facts** ✅
11. **determinism: same input → byte-identical cpu_only set** ✅
12. **export-wrapped recursive func → still detected** ✅
13. **@prefer(cpu) on recursive → Ok (cpu_only marked but no err)** ✅
14. **@prefer(gpu, force) multi-arg → NOT matched → Ok** ✅
15. **calls to undefined/prelude names ignored** ✅
16. **function with no calls → never cpu_only** ✅
17. **lexicographically-first offender reported (aaa over zzz)** ✅
18. **realistic mixed program (main+fib+helper+double)** ✅

Plus 24 inline `#[cfg(test)] mod tests` in recursion.rs (build_call_graph
shape, detect_cycles primitives, has_prefer_gpu_attr edge cases, etc.).

Total: **42 recursion-related tests**, all passing.

### Verification gate results
- FMT     (cargo fmt -p buff-lang-types -- --check):                EXIT 0
- CLIPPY  (cargo clippy -p buff-lang-types --all-targets -- -D warnings): EXIT 0
- TEST    (cargo test -p buff-lang-types):                          EXIT 0
                                                                       (full crate green)
- WORKSPACE CHECK (cargo check --workspace):                        EXIT 0

### MSVC env vars (REQUIRED for test/clippy — NOT for cargo check)
Same as T38–T47. Exact strings used:
```powershell
$env:LIB="C:\BuildTools\VC\Tools\MSVC\14.44.35207\lib\onecore\x64;C:\Program Files (x86)\Windows Kits\10\Lib\10.0.26100.0\um\x64;C:\Program Files (x86)\Windows Kits\10\Lib\10.0.26100.0\ucrt\x64"
$env:INCLUDE="C:\Program Files (x86)\Windows Kits\10\Include\10.0.26100.0\shared;C:\Program Files (x86)\Windows Kits\10\Include\10.0.26100.0\ucrt;C:\Program Files (x86)\Windows Kits\10\Include\10.0.26100.0\um;C:\Program Files (x86)\Windows Kits\10\Include\10.0.26100.0\winrt;C:\Program Files (x86)\Windows Kits\10\Include\10.0.26100.0\cppwinrt;C:\BuildTools\VC\Tools\MSVC\14.44.35207\include"
```

### Gotchas / lessons for T49+
1. **`@prefer(gpu)` is already representable from T35** — T49 needs NO
   AST changes. The `Attribute { name, args, span }` shape carries it
   directly. Use `buff_lang_types::has_prefer_gpu_attr(f)` (re-exported
   at crate root) for the predicate.
2. **`TypeError` is a struct, not an enum** — additive variant changes
   don't apply. Construct via `TypeError::new(Diagnostic::error(msg, span))`.
3. **Recursion is on-cycle only** — T49 may want to extend to
   transitive ("calls cpu_only") classification. If so, run a fixpoint
   on top of T48's `cpu_only` set (mirrors async_analysis's fixpoint).
4. **Self-edges are detected naturally** by the on-stack check — no
   special-casing needed. A node calling itself has `node` on_stack when
   its own callee list is iterated → back-edge → mark.
5. **Multi-arg `@prefer(gpu, force)` is intentionally NOT matched** by
   `has_prefer_gpu_attr` (requires exactly `args == ["gpu"]`). If T49
   wants to honour multi-arg hints, widen the predicate.
6. **Recursion depth is bounded by call-graph depth** — at most the
   number of declared functions. Recursive DFS is fine for realistic
   inputs; pathological depth would OOM the parser first.

### What's deferred (correctly out of scope for T48)
- **T49**: @prefer hints full hint-driven codegen (T48 only ERRORS when
  @prefer(gpu) conflicts with recursion).
- **T50**: GPU memory alignment / packing concerns.
- **Transitive cpu_only marking** ("calls a cpu_only fn"): deferred to
  T49 if it wants that conservative layer.
- **Trait-default-method recursion / extend-block method recursion**:
  the call graph (T31) intentionally includes only top-level `func`
  declarations; trait-method recursion is a future concern.
- **Commit**: per task instructions, did NOT commit. The commit message
  per the plan is: `feat(types): implement recursion detection via call graph`.


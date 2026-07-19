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

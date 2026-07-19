# Buff v1.0 "Production" — Heterogeneous Computing + Full Tooling

> **Phase 3 of 3.** Depends on [Phase 2 (v0.5)](./buff-v05-language.md) completion.
> Shared context: [Master Plan](./buff-master.md) | Numeric spec: [buff-numeric-system.md](./buff-numeric-system.md) | [Conventions](./buff-conventions.md) | [Project Structure](./buff-project-structure.md)

---

## TL;DR

> **Goal**: Add CPU parallelism (Rayon), GPU compute (wgpu/WebGPU), full tooling, and release v1.0.
>
> **Exit criteria**: GPU-accelerated programs with automatic CPU↔GPU dispatch. Full CLI tooling (run/build/test/fmt/check). Performance within 10% of hand-written Rust.
>
> **Tasks**: 26 core + 18 enhancement = **44 active tasks** (T51-T53 deferred to v2.0)
> **Waves**: 6 (Waves 9-14) + enhancement tasks within same waves

---

## Prerequisites

Phase 2 (v0.5) must be complete:
- All 13 types working
- Pattern matching, modules, async, closures
- Error handling with `?` operator
- Modern syntax (pipeline, destructuring, guards, etc.)

---

## Core Tasks (T38-T66)

### Wave 9 — Runtime + CPU Parallelism (depends on v0.5)

- [x] **T38**: buff-lang-runtime crate scaffold [deep]
  **What to do** (TDD): RED: GpuContext::new() returns Result. CpuDispatcher::new() returns thread pool. GREEN: create crate with wgpu/rayon/tokio deps, define dispatch traits.
  **Acceptance**: `cargo test -p buff-lang-runtime` passes (10+ tests). Crate compiles with all deps.
  **QA**: `cargo check -p buff-lang-runtime` → exit 0. Evidence: task-38-runtime-scaffold.txt
  **Commit**: `feat(runtime): scaffold buff-lang-runtime with dispatch interfaces`

- [x] **T39**: CPU parallel dispatch via Rayon [deep]
  **What to do** (TDD): RED: par_map([1,2,3], {x=>x*2}) → [2,4,6]. par_filter, par_reduce. GREEN: implement using rayon par_iter, work-stealing.
  **Acceptance**: `cargo test -p buff-lang-runtime par_map` passes (15+ tests). Deterministic output.
  **QA**: par_map([1,2,3], {x=>x*2}) → assert [2,4,6]. Evidence: task-39-par-map.txt
  **Commit**: `feat(runtime): implement CPU parallel dispatch via Rayon`

- [x] **T40**: Automatic dispatch threshold logic [deep]
  **What to do** (TDD): RED: <1000→SingleThread, 1000-50000→CpuParallel, >50000→GpuCompute. VRAM check fallback. GREEN: implement decide() with thresholds, <1μs decision.
  **Acceptance**: `cargo test -p buff-lang-runtime dispatch_threshold` passes. Boundaries correct.
  **QA**: decide(999,true,_) → SingleThread. decide(50001,true,_) → GpuCompute. Evidence: task-40-thresholds.txt
  **Commit**: `feat(runtime): implement dispatch threshold with VRAM fallback`

- [x] **T41**: Data race detection [deep]
  **What to do** (TDD): RED: `par_map({x=> total+=x})` where total is external mutable → error. Immutable capture OK. GREEN: analyze closures, reject mutable capture.
  **Acceptance**: `cargo test -p buff-lang-codegen-rust race_detection` passes. Mutable capture rejected.
  **QA**: `let mut t=0; v.par_map({x=>t+=x})` → assert ParallelMutabilityError. Evidence: task-41-race-detection.txt
  **Commit**: `feat(codegen-rust): detect data races in parallel closures`

- [x] **T42**: AtomicI64 auto-insertion [deep]
  **What to do** (TDD): RED: `total+=x` in par_map → AtomicI64::fetch_add. Post-parallel read → .load(). GREEN: auto-promote mutable shared vars to atomics in parallel context.
  **Acceptance**: `cargo test -p buff-lang-codegen-rust atomic` passes. Atomic only in parallel context.
  **QA**: Codegen accumulator in par_map → assert AtomicI64::fetch_add. Evidence: task-42-auto-atomic.txt
  **Commit**: `feat(codegen-rust): auto-insert AtomicI64 for shared mutable state`

### Wave 10 — GPU Compute (depends on Wave 9)

- [x] **T38b**: GPU test harness [deep]
  **What to do** (TDD): RED: MockGpuBackend records dispatches. WGSL snapshot stable. CPU-fallback testable. GREEN: implement mock backend, shader snapshot testing, fallback runner.
  **Acceptance**: `cargo test -p buff-lang-runtime gpu_harness` passes. Mock works without GPU.
  **QA**: MockGpuBackend.dispatch() → assert recorded_dispatches==1. Evidence: task-38b-mock-gpu.txt
  **Commit**: `test(runtime): add mock GPU backend and WGSL snapshot harness`

- [x] **T43**: wgpu context initialization [deep]
  **What to do** (TDD): RED: GpuContext::init() creates Device+Queue. Cached on second call. No GPU → graceful error. GREEN: implement lazy init with OnceLock, platform detection.
  **Acceptance**: `cargo test -p buff-lang-runtime gpu_context` passes. Lazy + cached + graceful fallback.
  **QA**: init() → device is Some. init() again → same instance. Evidence: task-43-gpu-init.txt
  **Commit**: `feat(runtime): implement wgpu context lazy init with caching`

- [x] **T44**: buff-lang-codegen-wgsl crate [deep]
  **What to do** (TDD): RED: `{x=>x*2.0}` → WGSL compute shader. Type filtering: f64 rejected. GREEN: AST→WGSL lowering, shader templates, buffer bindings.
  **Acceptance**: `cargo test -p buff-lang-codegen-wgsl` passes (15+ tests). Valid WGSL output.
  **QA**: Lower `{x=>x*2.0}` → assert `@compute @workgroup_size(64)`. Evidence: task-44-wgsl-codegen.txt
  **Commit**: `feat(codegen-wgsl): implement AST to WGSL compute shader codegen`

- [x] **T45**: GPU dispatch pipeline [deep]
  **What to do** (TDD): RED: full pipeline buffer→shader→dispatch→readback produces correct result. Workgroup sizing ceil(len/64). GREEN: implement storage buffers, compute pass, readback via map_async.
  **Acceptance**: `cargo test -p buff-lang-runtime gpu_dispatch` passes. Roundtrip correct.
  **QA**: dispatch([1.0,2.0,3.0], {x=>x*2}) → assert [2.0,4.0,6.0]. Evidence: task-45-gpu-roundtrip.txt
  **Commit**: `feat(runtime): implement GPU dispatch pipeline with readback`

- [x] **T46**: VRAM check + tiling + CPU fallback [deep]
  **What to do** (TDD): RED: data fits VRAM→single dispatch. Exceeds→tiled. Tile too big→CPU fallback. GREEN: implement VRAM query, tile calculator, sequential tiled dispatch.
  **Acceptance**: `cargo test -p buff-lang-runtime tiling` passes. Tiled result == CPU result.
  **QA**: 250 elements, max_tile=100 → 3 tiles, combined result correct. Evidence: task-46-tiling.txt
  **Commit**: `feat(runtime): implement VRAM check, tiled dispatch, CPU fallback`

- [x] **T47**: Cold start mitigation [unspecified-high]
  **What to do** (TDD): RED: pipeline cache hit avoids recompile. Buffer pool reuses. Async init ready before dispatch. GREEN: HashMap cache, buffer pool, tokio::spawn background init, batch dispatch.
  **Acceptance**: `cargo test -p buff-lang-runtime cold_start` passes. Second dispatch reuses pipeline.
  **QA**: dispatch shader A twice → assert create_pipeline called once. Evidence: task-47-cold-start.txt
  **Commit**: `perf(runtime): add pipeline caching, buffer pooling, async GPU init`

- [x] **T48**: Recursion detection [deep]
  **What to do** (TDD): RED: fib(n) calls fib(n-1) → cycle detected → CPU-only. @prefer(gpu) on recursive → error. GREEN: build call graph, DFS cycle detection, mark cpu_only.
  **Acceptance**: `cargo test -p buff-lang-types recursion` passes. Recursive = CPU-only.
  **QA**: Analyze fib → assert cpu_only==true. Evidence: task-48-recursion.txt
  **Commit**: `feat(types): implement recursion detection via call graph`

### Wave 11 — Advanced GPU Features (depends on Wave 10)

- [x] **T49**: Hints system `@prefer(gpu/npu)` [deep]
  **What to do** (TDD): RED: @prefer(gpu) generates both GPU+CPU code. Cost model overrides for small data. GREEN: parse @prefer attr, multi-version codegen, runtime dispatch.
  **Acceptance**: `cargo test -p buff-lang-runtime hints` passes. GPU chosen when available, CPU for small data.
  **QA**: @prefer(gpu) with 10 elements → assert CPU (cost override). Evidence: task-49-hints.txt
  **Commit**: `feat(runtime): implement @prefer hints with multi-version codegen`

- [x] **T50**: GPU memory alignment [deep]
  **What to do** (TDD): RED: struct going to GPU → #[repr(C)] auto-added. 16-byte alignment. GREEN: detect GPU-bound structs, auto-insert repr(C) + bytemuck::Pod.
  **Acceptance**: `cargo test -p buff-lang-codegen-rust gpu_alignment` passes. repr(C) on GPU structs.
  **QA**: Codegen struct used in par_map → assert #[repr(C)]. Evidence: task-50-alignment.txt
  **Commit**: `feat(codegen-rust): auto-add repr(C) for GPU-bound structs`

- [ ] ~~**T51**: FP8/FP4 bit-packing~~ — **DEFERRED to v2.0** (not WGSL-native)
- [ ] ~~**T52**: Trits bit-packing~~ — **DEFERRED to v2.0** (not WGSL-native)
- [ ] ~~**T53**: Quantization API~~ — **DEFERRED to v2.0** (not WGSL-native)

### Wave 12 — Tooling + Targets (depends on Wave 11)

- [x] **T54**: `buff fmt` formatter [unspecified-high]
  **What to do** (TDD): RED: mixed indent → 4-space. Line >100 → wrapped. Imports unsorted → reordered. Idempotent. GREEN: implement formatter using AST, enforce 18 conventions.
  **Acceptance**: `cargo test -p buff-lang-cli fmt` passes. Idempotent, 10 snapshots stable.
  **QA**: Format file with 2-space indent → assert 4-space output. Evidence: task-54-fmt.txt
  **Commit**: `feat(cli): implement buff fmt with 18 convention rules`

- [x] **T55**: `buff check` type-checker + linter [quick]
  **What to do** (TDD): RED: type error → exit 1. camelCase function → warning. Faster than build (no codegen). GREEN: run lexer+parser+types without codegen, lint naming conventions.
  **Acceptance**: `cargo test -p buff-lang-cli check` passes. Errors + warnings reported.
  **QA**: `buff check file_with_type_error.buff` → exit 1. Evidence: task-55-check.txt
  **Commit**: `feat(cli): implement buff check type-checker and linter`

- [x] **T56**: `buff build --release` [quick]
  **What to do** (TDD): RED: --release → cargo build --release with LTO. Default → debug. GREEN: propagate --release flag, inject [profile.release] lto=true.
  **Acceptance**: `cargo test -p buff-lang-cli build_release` passes. LTO enabled in release.
  **QA**: `buff build --release` → assert Cargo.toml has lto=true. Evidence: task-56-release.txt
  **Commit**: `feat(cli): implement --release optimization mode with LTO`

- [x] **T57**: LSP-friendly AST [deep]
  **What to do** (TDD): RED: parse preserves whitespace+comments. Roundtrip lossless. Incremental reparse. GREEN: implement LosslessAst with trivia, incremental parsing.
  **Acceptance**: `cargo test -p buff-lang-ast lossless` passes. Roundtrip byte-exact.
  **QA**: Parse → to_source → parse → assert identical AST. Evidence: task-57-lossless.txt
  **Commit**: `feat(ast): implement lossless AST with trivia preservation`

- [ ] **T58**: Wasm target support → **covered by T114 prerequisite in [buff-post-v10-tooling.md](buff-post-v10-tooling.md)** (Wasm target co-delivered with playground work, not a separate v1.0 deliverable)
  **What to do** (TDD): RED: --target wasm32 generates wasm-compatible Rust. Rayon→sequential on wasm. GREEN: add wasm target flag, adapt codegen for wasm32.
  **Acceptance**: `cargo test -p buff-lang-cli wasm` passes. Compiles for wasm32.
  **QA**: Build with wasm target → assert no Rayon calls. Evidence: task-58-wasm.txt
  **Commit**: `feat(cli): add wasm32 target support with Rayon fallback`

### Wave 13 — Diagnostics + DX (depends on Wave 12)

- [ ] **T59**: ariadne-based error diagnostics → **v2.0** (not in post-v1.0 tooling plan)
  **What to do** (TDD): RED: type error → colored multi-line with caret. "Did you mean?" suggestions. GREEN: integrate ariadne, convert BuffError→Report, Levenshtein suggestions.
  **Acceptance**: `cargo test -p buff-lang-error diagnostics` passes. Colored, contextual.
  **QA**: Type error → assert source line + caret + message. Evidence: task-59-diagnostics.txt
  **Commit**: `feat(error): implement ariadne-based colored diagnostics`

- [ ] **T60**: Source map improvements → **partially covered by T136/T137 in [buff-post-v10-tooling.md](buff-post-v10-tooling.md); remainder to v2.0**
  **What to do** (TDD): RED: Rust panic → maps to Buff line. Backtrace shows .buff files. GREEN: full Buff→Rust→binary mapping, backtrace filtering.
  **Acceptance**: `cargo test -p buff-lang-error source_map` passes. Panics point to .buff.
  **QA**:  Program panics → assert error references .buff line. Evidence: task-60-source-map.txt
  **Commit**: `feat(error): improve source maps with backtrace filtering`

- [ ] **T61**: Standard library expansion (File/HTTP/JSON) → **v1.4 stdlib tasks T124+ in [buff-post-v10-tooling.md](buff-post-v10-tooling.md)** (File/HTTP/JSON NOT wired in v1.0 — verified absent in `crates/buff-lang-types/src/prelude.rs` and `crates/buff-lang-codegen-rust/src/rust_codegen.rs`)
  **What to do** (TDD): RED: File.read/write. http.get (async). json.parse/stringify. GREEN: implement File I/O, HTTP client, JSON module wrapping Rust crates.
  **Acceptance**: `cargo test -p buff-stdlib` passes. File/HTTP/JSON work.
  **QA**: File.write("test") → File.read → assert contents match. Evidence: task-61-stdlib.txt
  **Commit**: `feat(stdlib): add File I/O, HTTP client, JSON modules`

- [ ] **T62**: Documentation + examples + README → **T116 (website) + post-v1.0 documentation tasks; language reference deferred to v2.0**
  **What to do** (TDD): RED: all examples compile via buff run. GREEN: write README, language reference, 5-10 examples, getting started guide.
  **Acceptance**: All examples pass `buff run`. README complete. 5+ examples.
  **QA**:  `buff run examples/*.buff` → all exit 0. Evidence: task-62-docs.txt
  **Commit**: `docs: add README, language reference, and example programs`

### Wave 14 — Release (depends on Wave 13)

- [ ] **T63**: Performance benchmarks → **v2.0** (not in post-v1.0 tooling plan)
  **What to do** (TDD): RED: Buff within 10% of Rust. Benchmark matrix multiply, par_map, startup. GREEN: implement criterion benchmarks, CI regression tracking.
  **Acceptance**: Buff within 10% of hand-written Rust. Benchmarks in CI.
  **QA**:  Benchmark par_map → assert Buff time < Rust time * 1.1. Evidence: task-63-benchmarks.txt
  **Commit**: `test: add performance benchmarks vs Rust and Go`

- [x] **T64**: Cross-platform testing — `.github/workflows/ci.yml` runs 3-OS matrix (ubuntu/windows/macos). Note: `--all-targets` clippy coverage and GPU-test coverage deferred to v2.0.
  **What to do** (TDD): RED: CI matrix Windows/Linux/macOS. GPU tests skip on CI. GREEN: configure CI matrix, cross-compilation verification.
  **Acceptance**: CI passes on all 3 platforms. GPU tests conditional.
  **QA**:  CI matrix → all 3 platforms green. Evidence: task-64-cross-platform.txt
  **Commit**: `ci: add Windows/Linux/macOS testing matrix`

- [x] **T65**: v1.0 release preparation — git tag `v1.0.0` cut at commit `88c7be0` with annotated release notes. Release binaries and formal CHANGELOG deferred to v2.0 (no public remote yet — local-only repo).
  **What to do** (TDD): RED: changelog accurate, migration guide. GREEN: write changelog v0.1→v1.0, migration guide, release binaries.
  **Acceptance**: Changelog complete. Release binaries built. Tag v1.0.0.
  **QA**:  `git tag v1.0.0` created. Evidence: task-65-release.txt
  **Commit**: `release: v1.0.0 "Production" — heterogeneous computing`

- [ ] **T66**: Comprehensive integration test suite → **v2.0** (T113b backward-compat fixture in [buff-post-v10-tooling.md](buff-post-v10-tooling.md) is the seed)
  **What to do** (TDD): RED: all features combined work together. GPU+CPU mixed. Async+GPU. Edge cases. GREEN: write comprehensive E2E tests covering all v1.0 features.
  **Acceptance**: All integration tests pass. Edge cases covered.
  **QA**:  Run full integration suite → 0 failures. Evidence: task-66-integration.txt
  **Commit**: `test: add comprehensive integration test suite`

---

## Enhancement Tasks

> **All enhancement tasks below are deferred to v2.0.** The post-v1.0 tooling plan ([buff-post-v10-tooling.md](buff-post-v10-tooling.md)) focuses on adoption/expansion (LSP, playground, IDE, package manager), not language enhancements. Each task is preserved here as v2.0 work.

### Wave 8 Enhancement — DX & Testing

- [ ] **T80**: Watch mode `buff watch` [unspecified-high]
  **What to do** (TDD): RED: file change triggers rebuild. 200ms debounce. GREEN: implement notify crate watcher, debounce, auto-rebuild.
  **Acceptance**: `cargo test -p buff-lang-cli watch` passes. Rebuilds on save.
  **QA**: Touch .buff file → assert rebuild triggered within 300ms. Evidence: task-80-watch.txt
  **Commit**: `feat(cli): add buff watch with debounced auto-rebuild`

- [ ] **T81**: Mold fast linker [quick]
  **What to do** (TDD): RED: -fuse-ld=mold on Linux, LLD on Windows. GREEN: detect platform, pass linker flag.
  **Acceptance**: `cargo test -p buff-lang-cli linker` passes. Correct linker selected per platform.
  **QA**:  Linux build → assert mold flag in cargo invocation. Evidence: task-81-linker.txt
  **Commit**: `perf(cli): integrate Mold fast linker`

- [ ] **T82**: Doc comments → HTML docs [unspecified-high]
  **What to do** (TDD): RED: /// comments → HTML docs. `buff doc` generates HTML. GREEN: parse doc comments, generate HTML documentation.
  **Acceptance**: `cargo test -p buff-lang-cli doc` passes. HTML generated from ///.
  **QA**:  `buff doc` → assert HTML output exists. Evidence: task-82-docs.txt
  **Commit**: `feat(cli): implement buff doc HTML documentation generation`

- [ ] **T83**: Doctests [deep]
  **What to do** (TDD): RED: code in /// comments runs as test. `buff test --doc`. GREEN: extract code blocks from doc comments, compile and run as tests.
  **Acceptance**: `cargo test -p buff-lang-cli doctests` passes. Doc examples tested.
  **QA**:  /// `let x = 1` → assert runs as test. Evidence: task-83-doctests.txt
  **Commit**: `feat(cli): add doctests for documentation examples`

### Wave 9 Enhancement — Memory & Parallelism

> **T88 (Algorithm/Schedule IR) moved to Wave 1** — foundational, not enhancement

- [ ] **T33b**: Advanced clone optimization [deep]
  **What to do** (TDD): RED: optimize T33 clone insertion with fast paths. Arc dedup for hot loops. GREEN: profile-guided optimization of clone analysis.
  **Acceptance**: Clone count reduced vs T33 baseline. Hot paths optimized.
  **QA**:  Benchmark clone-heavy code → assert fewer clones than T33. Evidence: task-33b-clone-opt.txt
  **Commit**: `perf(codegen-rust): optimize clone analysis with fast paths`

- [ ] **T89**: Epoch-based reclamation [deep]
  **What to do** (TDD): RED: lock-free GPU memory reclamation. Readers never block. GREEN: implement Crossbeam epoch model for GPU buffers.
  **Acceptance**: `cargo test -p buff-lang-runtime epoch` passes. Safe reclamation.
  **QA**:  Concurrent GPU access → assert no use-after-free. Evidence: task-89-epoch.txt
  **Commit**: `feat(runtime): implement epoch-based reclamation for GPU memory`

- [ ] **T90**: Arena allocators [deep]
  **What to do** (TDD): RED: bulk allocation for GPU regions. Bump pointer O(1). Bulk free. GREEN: implement arena allocator for GPU memory.
  **Acceptance**: `cargo test -p buff-lang-runtime arena` passes. O(1) allocation.
  **QA**:  Allocate 1000 elements → assert single arena allocation. Evidence: task-90-arena.txt
  **Commit**: `feat(runtime): add arena allocators for GPU memory regions`

### Wave 11 Enhancement — Auto-Sizing + GPU Type Enforcement

- [ ] **T94**: Auto-sizing arithmetic [ultrabrain]
  **What to do** (TDD): RED: Int<8> + Int<8> → Int<16> (carry). Int<8> * Int<8> → Int<16>. Int<W> << n → Int<W+n>. GREEN: track widths in type checker, widen results.
  **Acceptance**: `cargo test -p buff-lang-types auto_sizing` passes. No silent overflow.
  **QA**:  Analyze `let a: Int<8> = 100; let b = a + a` → assert Int<16>. Evidence: task-94-auto-sizing.txt
  **Commit**: `feat(types): implement auto-sizing arithmetic with width tracking`

- [ ] **T95**: GPU type enforcement [deep]
  **What to do** (TDD): RED: only f32/f16/i32/u32 reach GPU. f64→f32 warning. i64→i32 overflow check. Decimal→CPU. GREEN: type filtering at GPU dispatch boundary.
  **Acceptance**: `cargo test -p buff-lang-runtime type_enforcement` passes. Non-native → CPU.
  **QA**:  Vector<Float<64>> on GPU → assert precision warning. Evidence: task-95-gpu-types.txt
  **Commit**: `feat(runtime): enforce WGSL-native types at GPU boundary`

### Wave 12 Enhancement — Build Performance

- [ ] **T91**: Incremental compilation [deep]
  **What to do** (TDD): RED: changed file → only recompile that module. Hash-based caching. GREEN: implement content hash cache, dependency graph tracking.
  **Acceptance**: `cargo test -p buff-lang-cli incremental` passes. Only changed modules recompiled.
  **QA**:  Change 1 file → assert 1 module recompiled (not all). Evidence: task-91-incremental.txt
  **Commit**: `perf(cli): implement incremental compilation with hash-based caching`

### Wave 9 Enhancement — Concurrency Primitives

- [ ] **T108**: Channels `chan<T>` [deep]
  **What to do** (TDD): RED: `channel<String>()` creates typed channel. tx.send/rx.recv. Maps to tokio::mpsc. GREEN: implement channel type, codegen to mpsc.
  **Acceptance**: `cargo test -p buff-lang-runtime channels` passes. Send/recv works.
  **QA**:  tx.send("hi"); rx.recv() → assert "hi". Evidence: task-108-channels.txt
  **Commit**: `feat(runtime): add channels chan<T> with tokio::mpsc`

### Wave 13 Enhancement — Contract Programming

- [ ] **T109**: Design by Contract [deep]
  **What to do** (TDD): RED: require(amount>0) checked on entry. ensure(result>=0) on exit. Zero cost in release. GREEN: parse require/ensure, codegen to debug_assert!.
  **Acceptance**: `cargo test -p buff-lang-codegen-rust contracts` passes. Stripped in release.
  **QA**:  require(false) in debug → panic. In release → no-op. Evidence: task-109-contracts.txt
  **Commit**: `feat(codegen-rust): add require/ensure contracts with zero release cost`

- [ ] **T113**: Editions system [deep]
  **What to do** (TDD): RED: edition="2024" in buff.toml. Old code compiles forever. GREEN: parse edition, gate syntax features by edition.
  **Acceptance**: `cargo test -p buff-lang-cli editions` passes. Edition controls syntax.
  **QA**:  edition=2024 → assert new syntax allowed. edition=2024 + old code → compiles. Evidence: task-113-editions.txt
  **Commit**: `feat(cli): add editions system for backward compatibility`

### Enhancement: Testing Tools

- [ ] **T84**: `@test.parametrize` [deep]
  **What to do** (TDD): RED: @test.parametrize("x",[1,2,3]) runs 3 times. GREEN: parse parametrize attr, generate test cases.
  **Acceptance**: `cargo test -p buff-lang-cli parametrize` passes. Multiple inputs tested.
  **QA**:  @test.parametrize("x",[1,2,3]) → assert 3 test runs. Evidence: task-84-parametrize.txt
  **Commit**: `feat(cli): add @test.parametrize for data-driven tests`

- [ ] **T85**: `@fixture` with DI [deep]
  **What to do** (TDD): RED: @fixture functions injected into tests. Setup/teardown. GREEN: parse fixture attr, implement DI for test functions.
  **Acceptance**: `cargo test -p buff-lang-cli fixtures` passes. Fixtures injected.
  **QA**:  @fixture fn db() injected into test param → assert available. Evidence: task-85-fixtures.txt
  **Commit**: `feat(cli): add @fixture with dependency injection`

- [ ] **T86**: Snapshot testing [quick]
  **What to do** (TDD): RED: assert.snapshot(value) compares against saved. GREEN: formalize insta integration, add assert.snapshot.
  **Acceptance**: `cargo test -p buff-lang-cli snapshot` passes. Snapshot compare works.
  **QA**:  assert.snapshot(42) → matches saved snapshot. Evidence: task-86-snapshot.txt
  **Commit**: `feat(cli): formalize snapshot testing with assert.snapshot`

- [ ] **T87**: Mutation testing [ultrabrain]
  **What to do** (TDD): RED: buff test --mutate changes code, checks if tests catch it. GREEN: implement mutation operators, score calculation.
  **Acceptance**: `cargo test -p buff-lang-cli mutation` passes. Mutation score calculated.
  **QA**:  Mutate `+` to `-` → assert test catches it. Evidence: task-87-mutation.txt
  **Commit**: `feat(cli): add mutation testing with buff test --mutate`

---

## GPU Compute Type Policy (WGSL-Native Only)

| Buff Type | WGSL | Action |
|-----------|------|--------|
| `Float<32>` | `f32` | ✅ Direct dispatch |
| `Float<16>` | `f16` | ✅ Direct (if GPU supports `enable f16`) |
| `Int<32>` | `i32` | ✅ Direct dispatch |
| `Bits<32>` | `u32` | ✅ Direct dispatch |
| `Float<64>` | — | Auto-convert → f32 (precision warning) |
| `Int<64>` | — | Auto-convert → i32 (overflow check) |
| `Int<8>`, `Int<16>` | — | Auto-convert → i32 (widen) |
| `Decimal` | — | CPU fallback (Rayon) |
| BFloat16/FP8/FP4/NF4/Trit | — | **DEFERRED to v2.0** |

---

## DEFERRED to v2.0 (Documented for future, NOT in v1.0)

- BFloat16, FP8 (E4M3/E5M2), FP4, NF4 quantization formats
- Trit first-class type and Trits<N> packed storage
- Quantization API (`.quantize()`, `.dequantize()`)
- LSP/IDE server
- Self-hosting (Buff compiler written in Buff)
- Macros/metaprogramming
- Custom operators

Full spec for deferred numeric types: [buff-numeric-system.md](./buff-numeric-system.md)

---

## Phase Exit Criteria

- [x] CPU parallel dispatch (Rayon) via par_map/par_filter — Evidence: T39 commit ba34522, crates/buff-lang-runtime/src/cpu.rs
- [x] GPU compute (wgpu) with automatic dispatch based on thresholds — Evidence: T45 commit 8c1a9f8, crates/buff-lang-runtime/src/gpu.rs (hardware-verified)
- [ ] GPU type enforcement (only WGSL-native types) → **v2.0** — Reason: T95 deferred; auto-sizing (T94) and strict GPU type enforcement not in v1.0
- [x] VRAM check + tiling + CPU fallback — Evidence: T46 commit a612245, crates/buff-lang-runtime/src/tiling.rs
- [x] Cold start mitigation (async background init) — Evidence: T47 commit cf6d4af, crates/buff-lang-runtime/src/cold_start.rs
- [x] Recursion detection (CPU-only marking) — Evidence: T48 commit 96c97bd, crates/buff-lang-types/src/recursion.rs
- [x] Hints system (@prefer) with graceful degradation — Evidence: T49 commit 24043de, crates/buff-lang-runtime/src/hints.rs
- [ ] Auto-sizing arithmetic (overflow prevention) → **v2.0** — Reason: T94 deferred
- [x] Full CLI: run, build, test, fmt, check — Evidence: all 5 commands in `crates/buff-lang-cli/src/commands/` (Build/Run/Test/Fmt/Check submodules). `buff doc` deferred to post-v1.0 T82 (see separate line below).
- [ ] `buff doc` HTML documentation generation → **post-v1.0 T82** — Reason: not implemented in v1.0
- [ ] Watch mode → **v2.0** — Reason: T80 deferred
- [ ] Mold fast linker → **v2.0** — Reason: T81 deferred (Linux-only; Windows host can't verify)
- [ ] Incremental compilation → **v2.0** — Reason: T91 deferred (post-v10 line 126 explicitly "v2.0 non-goal")
- [ ] Doc comments → HTML docs + doctests → **v2.0** — Reason: T82/T83 deferred
- [ ] ariadne error diagnostics → **v2.0** — Reason: T59 deferred
- [ ] Testing: parametrize, fixtures, snapshot, mutation → **v2.0** — Reason: T84-T87 deferred
- [ ] Wasm target support → **post-v1.0 T114** — Reason: covered by playground prerequisite
- [ ] Performance within 10% of hand-written Rust → **v2.0** — Reason: T63 benchmarks deferred; no measurement infrastructure
- [x] Cross-platform CI (Win/Linux/macOS) — Evidence: `.github/workflows/ci.yml` runs 3-OS matrix. Sub-note: `--all-targets` clippy and GPU test coverage deferred to v2.0
- [x] Git tag `v1.0.0` created — Evidence: tag at commit 88c7be0

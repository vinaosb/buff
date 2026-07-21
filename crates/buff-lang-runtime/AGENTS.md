# buff-lang-runtime

Heterogeneous compute host. Rayon (CPU parallel) + wgpu (GPU compute) + tokio (async runtime).

## OVERVIEW

Dispatches Buff functions marked for parallelism to CPU or GPU based on hints (`@prefer(gpu)`), arithmetic intensity thresholds, and recursion facts. The GPU path requires WGSL shaders produced by `buff-lang-codegen-wgsl` (T44). CPU fallback is always available and always correct.

Three routing bands: `SingleThread` (< 1 000 elements), `CpuParallel` (1 000-50 000), `GpuCompute` (> 50 000, GPU-available, fits VRAM). Hints can widen the GpuCompute window down to 1 024 elements.

## STRUCTURE (11 src files)

| File | Lines | Role |
|------|-------|------|
| `lib.rs` | 107 | Public API + re-exports. All map/set types are `BTreeMap`/`BTreeSet` (project hard rule). |
| `dispatch.rs` | 58 | `DispatchKind` enum (SingleThread/CpuParallel/GpuCompute) + `Dispatcher` trait (object-safe). |
| `cpu.rs` | 211 | `CpuDispatcher`: owns a rayon thread pool. Exposes `par_map`, `par_filter`, `par_reduce`. Deterministic order via rayon's ordered `collect`. |
| `gpu.rs` | 308 | `GpuContext`: wgpu adapter handle + `OnceLock`-cached `(Device, Queue)`. First call to `device_queue()` drives the async init via `pollster::block_on`; subsequent calls return the cached pair. Failures cached too. |
| `gpu_pipeline.rs` | 541 | `WgpuBackend` (implements `GpuBackend`): real wgpu dispatch. Shader module creation, buffer upload (input storage + output storage + staging), compute pass, `map_async` readback, `bytemuck` cast. `workgroup_count` helper. Empty input returns empty immediately. |
| `cold_start.rs` | 1222 | `PipelineCache` (BTreeMap, keyed by WGSL source), `BufferPool` (BTreeMap, keyed by `(byte_size, usage)`), `ColdStartBackend` (wraps `WgpuBackend` with both caches + background device init via `std::thread::spawn`). Drop-in `GpuBackend` impl. |
| `threshold.rs` | 258 | `decide()`: pure O(1) routing function. `SINGLE_THREAD_MAX = 999`, `CPU_PARALLEL_MAX = 50_000`. `fits_vram()` overflow-aware check. |
| `hints.rs` | 569 | `Prefer` enum (None/Gpu/Npu), `PREFER_GPU_MIN_ELEMENTS = 1024`. `decide_with_prefer()` layers hints on top of `decide()`. `dispatch_with_prefer()` runs the chosen path end-to-end with GPU-to-CPU fallback. `prefer_from_name_args()` parses AST attributes without coupling to `buff-lang-ast`. |
| `tiling.rs` | 493 | VRAM-aware tiling. `tile_ranges()` splits input into `(start, end)` ranges. `max_elements_per_tile(vram, bpe) = vram / (3 * bpe)`. `dispatch_tiled()` runs each tile through `GpuBackend`. `dispatch_map_with_tiling()` adds CPU fallback on any GPU error. `vram_budget_from_device()` queries wgpu device limits. |
| `mock_gpu.rs` | 318 | `GpuBackend` trait (object-safe, `Send + Sync`). `MockGpuBackend<F>`: records dispatches in `Mutex<Vec<DispatchRecord>>`, produces output via caller-provided CPU closure. `cpu_fallback_map()`: deterministic sequential oracle for tests. No real GPU needed. |
| `error.rs` | 75 | `RuntimeError` (thiserror): GpuUnavailable, GpuInit{detail}, NotImplemented{feature}, Unsupported{detail}. Bridges to `buff_lang_error::BuffError` via `From`. |

## TESTS (11 integration files)

`cold_start_tests`, `cpu_dispatcher_tests`, `cpu_parallel_tests`, `dispatch_tests`, `error_tests`, `gpu_context_tests`, `gpu_dispatch_tests`, `gpu_harness_tests`, `hints_tests`, `threshold_tests`, `tiling_tests`. Plus inline `#[cfg(test)]` units in each src file.

Tests use `MockGpuBackend` to run on hosts without GPU hardware.

## WHERE TO LOOK

| Task | File(s) |
|------|---------|
| Change CPU dispatch logic | `cpu.rs` |
| Tune GPU pipeline (buffers, readback) | `gpu_pipeline.rs` |
| Change CPU/GPU routing thresholds | `threshold.rs` + `dispatch.rs` |
| Add new hint attribute | `hints.rs` + `buff-lang-parser` |
| Test without real GPU | `mock_gpu.rs` + `cpu_fallback_map` |
| Cold-start optimization | `cold_start.rs` |
| VRAM tiling behavior | `tiling.rs` |

## CONVENTIONS

- **NO `unwrap`/`expect`/`panic!` in non-test code** (extra-strict: wgpu panics are opaque and unrecoverable).
- `Result<_, RuntimeError>` everywhere in public API. Mutex poisoning handled via `unwrap_or`/`map_err`.
- All map/set types are `BTreeMap`/`BTreeSet`, never `HashMap`/`HashSet` (project hard rule, even for runtime caches).
- `GpuBackend` trait is object-safe (no generic methods, no `Self` by value) so callers hold `Box<dyn GpuBackend>` or `Option<&dyn GpuBackend>`.
- Empty input returns `Ok(Vec::new())` immediately in every backend (wgpu forbids 0-sized dispatches).
- GPU failure is always invisible to the caller: `dispatch_with_prefer` and `dispatch_map_with_tiling` mask errors behind CPU fallback.
- wgpu version pinned at workspace level (`wgpu.workspace = true`). Currently v26.

## BINDINGS CONTRACT (stable with codegen-wgsl)

| WGSL | Usage |
|------|-------|
| `@group(0) @binding(0) var<storage, read> input: array<T>` | Input buffer |
| `@group(0) @binding(1) var<storage, read_write> output: array<T>` | Output buffer |
| `@compute @workgroup_size(64)` | Default workgroup (configurable 1..=1024) |
| Entry point `fn main(@builtin(global_invocation_id) gid: vec3<u32>)` | Compute shader entry |

Runtime dispatches `ceil(len/64)` workgroups in the X dimension.

## DEPS

`rayon`, `wgpu`, `tokio`, `bytemuck`, `pollster` (workspace). `buff-lang-error` (leaf). `thiserror`.

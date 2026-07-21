# Decision: WGSL Extensibility Assessment (T6)

**Date:** 2026-07-21
**Task:** [T6 — WGSL Extensibility Assessment](../../.sisyphus/plans/buff-v1x-frameworks.md#L1278)
**Author:** Sisyphus-Junior (spike executor)
**Timebox:** 1 session (within 3-day limit)

---

## VERDICT: **GPU: PARTIAL — elementwise YES, tensor ops DEFERRED to v1.18+**

The existing WGSL codegen (`crates/buff-lang-codegen-wgsl/`) can be extended for **elementwise tensor ops** (f32 only) with moderate effort (~300 LOC, ~3 days). **Matrix multiplication, reductions, and convolutions** require significant new infrastructure (2D indexing, workgroup shared memory, multi-buffer layouts) — estimated ~1500 LOC, ~15 days. **Autodiff** is CPU-only for MVP (graph-based, not a GPU kernel).

**Recommendation**: T8 (buff-tensor) proceeds with **CPU-only via rayon** for MVP. GPU dispatch for elementwise ops is feasible as a v1.18+ enhancement. This does NOT block Wave 2 start.

---

## 1. Current WGSL Capabilities

### What `crates/buff-lang-codegen-wgsl/` does today

| Capability | Status | Details |
|---|---|---|
| **Input shape** | Single-param numeric map lambda `{x => <expr>}` | `Expr::Lambda` with exactly 1 param, single-expression body |
| **Supported expressions** | Arithmetic (18 binary ops), unary (Neg/Not/BitNot), literals (Float/Int/Bool/Byte) | See `lower.rs` `SUPPORTED_BINARY_TOKENS` table |
| **Rejected expressions** | Calls, struct init, match, indexing, nested lambdas, free variables, multi-statement bodies | All return `WgslError::UnsupportedExpr` |
| **Scalar types** | f32 (default), f16 (requires `enable f16;`), i32, u32 | `WgslScalarType` enum in `ty.rs` |
| **Rejected types** | f64/Double, i64, Decimal, String, Char, Regex | Clear error → CPU fallback |
| **Binding layout** | `@group(0) @binding(0)` = read-only input storage, `@group(0) @binding(1)` = read_write output storage | Stable contract with `buff-lang-runtime` |
| **Workgroup size** | 64 (configurable 1..=1024) | `WgslOptions::workgroup_size` |
| **Entry point** | `fn main(@builtin(global_invocation_id) gid: vec3<u32>)` | Single invocation per element |
| **Dispatch model** | 1D: each invocation processes one element, bounds-checked | `workgroup_count(len) = ceil(len/64)` |
| **Codegen method** | `format!()`-based raw string (project rule exception) | Documented in `shader.rs` — WGSL has no `syn` equivalent |
| **Determinism** | Byte-identical output for same `(lambda, opts)` | No HashMap, no timestamps |
| **Total LOC** | ~478 (lib.rs) + 374 (lower.rs) + 241 (shader.rs) + 277 (ty.rs) + 96 (error.rs) = ~1466 LOC | 5 source files |

### What the runtime (`buff-lang-runtime`) does today

| Capability | Details |
|---|---|
| **GPU context** | `GpuContext` — wgpu adapter + lazily-cached `(Device, Queue)` via `OnceLock` |
| **GPU dispatch** | `WgpuBackend::dispatch_map(shader, &[f32]) -> Vec<f32>` — full pipeline: shader module, buffers, bind group, compute pass, readback |
| **CPU fallback** | `CpuDispatcher::par_map` — rayon-based parallel map. Always available, always correct |
| **Tiling** | `dispatch_map_with_tiling` — VRAM-aware tiling with CPU fallback on any GPU error |
| **Hints** | `@prefer(gpu)` — hint, not demand. Falls back to CPU if GPU unavailable or input too small |
| **Empty input** | Returns `Ok(Vec::new())` immediately — wgpu forbids 0-sized dispatches |

---

## 2. Tensor Op Gaps

### Gap 1: Elementwise ops (+, -, *, /, unary)

| Property | Value |
|---|---|
| **Feasibility** | **HIGH** |
| **WGSL support** | Already works today for 1D arrays. Tensor elementwise is the same pattern applied per-element |
| **What's needed** | Flatten tensor to 1D buffer, dispatch existing WGSL pipeline. No codegen changes needed |
| **Estimated LOC** | ~50 (tensor flatten + dispatch wrapper in `buff-tensor`) |
| **Estimated days** | 0.5 |
| **CPU fallback** | `CpuDispatcher::par_map` — rayon parallel elementwise. Trivial |

### Gap 2: Matrix multiplication (2D × 2D)

| Property | Value |
|---|---|
| **Feasibility** | **MEDIUM** |
| **WGSL support** | WGSL supports 2D workgroup dispatch (`dispatch_workgroups(x, y, 1)`) and shared memory (`var<workgroup>`). But current codegen is 1D-only |
| **What's needed** | (a) New shader template with 2D dispatch + tiled matmul algorithm (shared memory tiles). (b) New binding layout: 3 buffers (A, B, output) instead of 2. (c) New IR node or direct shader emission (not lambda-based). (d) Runtime changes: 3-buffer bind group, 2D workgroup count |
| **Estimated LOC** | ~600 (200 codegen + 200 runtime + 200 tests) |
| **Estimated days** | 6 |
| **CPU fallback** | `ndarray` or `nalgebra` extern FFI — mature, optimized BLAS. Or pure rayon-based tiled matmul |

### Gap 3: Reductions (sum, mean, max along axis)

| Property | Value |
|---|---|
| **Feasibility** | **MEDIUM** |
| **WGSL support** | WGSL supports workgroup shared memory for tree-reduction pattern. Each workgroup reduces a tile, then a second pass combines workgroup results |
| **What's needed** | (a) Two-pass shader: first pass reduces per-workgroup to shared memory, second pass combines. (b) Workgroup barrier synchronization (`workgroupBarrier()`). (c) Axis-aware indexing (non-contiguous memory access pattern). (d) New binding layout for intermediate buffers |
| **Estimated LOC** | ~500 (200 codegen + 200 runtime + 100 tests) |
| **Estimated days** | 5 |
| **CPU fallback** | Sequential axis-aware reduction in rayon. Simple: iterate over axis, accumulate |

### Gap 4: Convolutions (2D/3D)

| Property | Value |
|---|---|
| **Feasibility** | **LOW** |
| **WGSL support** | Possible with workgroup shared memory + tiled convolution kernels. Complex memory access patterns |
| **What's needed** | (a) Tiled convolution shader with shared memory input reuse. (b) Multiple buffer bindings (input, kernel, output, optional bias). (c) Stride/dilation/padding parameterization. (d) Considerable numerical testing |
| **Estimated LOC** | ~800 (300 codegen + 300 runtime + 200 tests) |
| **Estimated days** | 8 |
| **CPU fallback** | `ndarray` convolution or direct naive O(n²) implementation. Sufficient for MVP |

### Gap 5: Autodiff graph

| Property | Value |
|---|---|
| **Feasibility** | **LOW (CPU-only for MVP)** |
| **WGSL support** | Autodiff is a graph-based computation (forward/backward pass through operation DAG). Not a single GPU kernel. Requires per-op gradient computation |
| **What's needed** | (a) Tape/graph recording of tensor operations. (b) Per-op backward function. (c) Topological traversal of graph. (d) GPU acceleration would require per-op GPU kernels + graph scheduling |
| **Estimated LOC** | ~2000+ for GPU autodiff (full framework) |
| **Estimated days** | 20+ |
| **CPU fallback** | **This IS the MVP path.** Micrograd-style tape-based autodiff on CPU. Candle reference shows this is the standard approach |

---

## 3. Per-Framework GPU Strategy

### `buff-tensor` (T8) — **GPU: PARTIAL (elementwise YES, matmul/reduce DEFERRED)**

| Op | GPU verdict | Rationale |
|---|---|---|
| Elementwise (+, -, *, /) | **YES** | Works today via existing WGSL pipeline. Flatten → dispatch → reshape |
| Matmul (2D × 2D) | **DEFER to v1.18+** | ~600 LOC, 6 days. New binding layout, 2D dispatch, tiled algorithm. CPU via `ndarray`/`nalgebra` FFI is sufficient for MVP |
| Reduce (sum/mean/max) | **DEFER to v1.18+** | ~500 LOC, 5 days. Two-pass reduction with workgroup shared memory. CPU via rayon is sufficient for MVP |
| Reshape/transpose | **NO** (CPU) | Data movement ops. No compute benefit from GPU |

**MVP strategy**: CPU-only via rayon + extern `ndarray`/`nalgebra`. GPU elementwise added as v1.18+ enhancement.

### `buff-science` (T13) — **GPU: NO for MVP**

| Op | GPU verdict | Rationale |
|---|---|---|
| linalg.matmul | **DEFER** (via T8) | Depends on T8 matmul decision. If T8 is CPU-only, science is CPU-only |
| linalg.inverse | **NO** | CPU-only algorithm (LU decomposition). GPU would need iterative solver |
| linalg.determinant | **NO** | CPU-only. GPU not beneficial for single-matrix ops |
| ode.rk4 | **NO** | Sequential algorithm. Not parallelizable |
| stats (mean/variance) | **NO** | Trivial CPU ops. GPU overhead not justified |
| interp.linear | **NO** | Sequential lookup |

**MVP strategy**: CPU-only. All ops are either sequential (ODE, inverse) or trivially parallel on CPU (stats). GPU would add complexity without proportional benefit for MVP scope.

### `buff-ml` (T15) — **GPU: NO for MVP**

| Op | GPU verdict | Rationale |
|---|---|---|
| Autodiff (tape) | **NO** | Graph-based, not a GPU kernel. CPU tape is standard (Micrograd, Candle CPU path) |
| Linear layer forward | **DEFER** (via T8 matmul) | Matmul is the bottleneck. If T8 matmul is CPU, ML forward is CPU |
| ReLU/Sigmoid/Softmax | **YES** (trivial) | Elementwise ops. Could use existing WGSL pipeline, but not worth wiring for MVP |
| Training loop | **NO** | Sequential loop. GPU would accelerate per-step matmul, not the loop itself |

**MVP strategy**: CPU-only. Autodiff is inherently CPU-friendly for MVP (small models, tape-based). GPU acceleration of matmul in forward/backward is a v1.18+ enhancement.

### `buff-image` (T9) — **CPU-only (Metis G7 lock)**

Per the G7 GPU scope matrix (plan line 138): Image/DSP are CPU-only for MVP. GPU deferred to v1.18+.

Rationale: Image codecs (PNG/JPEG) are CPU-bound I/O operations. Pixel ops (brightness/contrast) are trivially parallel on CPU via rayon. GPU would add PCIe transfer overhead that dominates for typical image sizes.

### `buff-dsp` (T11) — **CPU-only (Metis G7 lock)**

Per the G7 GPU scope matrix (plan line 138): Image/DSP are CPU-only for MVP. GPU deferred to v1.18+.

Rationale: FFT is CPU-optimized (rustfft crate). Filters and windows are sequential or small-window operations. GPU not justified for MVP.

### `buff-game` (T16) — **Uses existing WGSL (no new work)**

Per G7: "Game uses existing WGSL." The existing elementwise WGSL pipeline is sufficient for game compute needs (particle systems, simple transforms). No tensor op extensions needed.

---

## 4. Per-Gap Cost Estimate

| Gap | Feasibility | LOC (codegen) | LOC (runtime) | LOC (tests) | Total LOC | Days | Blocks |
|---|---|---|---|---|---|---|---|
| Elementwise (existing) | HIGH | 0 | ~50 | ~30 | ~80 | 0.5 | — |
| Matmul 2D | MEDIUM | ~200 | ~200 | ~200 | ~600 | 6 | T8, T13, T15 |
| Reductions | MEDIUM | ~200 | ~200 | ~100 | ~500 | 5 | T8, T13 |
| Convolutions | LOW | ~300 | ~300 | ~200 | ~800 | 8 | T15 (deferred) |
| Autodiff GPU | LOW | — | — | — | ~2000+ | 20+ | T15 (deferred) |
| **Total tensor GPU** | — | ~700 | ~750 | ~530 | ~1980 | ~19 | — |

**Note**: These estimates assume a single experienced Rust+WGSL developer. Parallel work on independent gaps (e.g., matmul + reductions) could reduce wall-clock time.

---

## 5. CPU Fallback Paths

### `buff-tensor` CPU strategy

| Op | CPU approach | Crate | Performance |
|---|---|---|---|
| Elementwise | `CpuDispatcher::par_map` (rayon) | `buff-lang-runtime` | O(n) parallel, good for 1M+ elements |
| Matmul | `ndarray::Array2::dot` or `nalgebra::Matrix::gemm` | `ndarray`/`nalgebra` extern | BLAS-optimized, near-peak CPU perf |
| Reduce | Sequential axis loop + rayon for independent axes | `buff-tensor` internal | O(n) per axis, sufficient for MVP |
| Reshape | View manipulation (no data copy) | `buff-tensor` internal | O(1) |

### `buff-science` CPU strategy

| Op | CPU approach | Crate |
|---|---|---|
| linalg.inverse | `nalgebra::Matrix::try_inverse` | `nalgebra` extern |
| linalg.determinant | `nalgebra::Matrix::determinant` | `nalgebra` extern |
| linalg.solve | `nalgebra::Matrix::solve` | `nalgebra` extern |
| ode.rk4 | Pure Rust sequential | `buff-science` internal |
| stats | `CpuDispatcher::par_map` + sequential combine | `buff-lang-runtime` |

### `buff-ml` CPU strategy

| Op | CPU approach | Crate |
|---|---|---|
| Autodiff tape | `Vec<Op>` tape with per-op backward | `buff-ml` internal (Micrograd pattern) |
| Linear forward | `ndarray::Array2::dot` | `ndarray` extern |
| ReLU/Sigmoid | Elementwise via rayon | `buff-lang-runtime` |
| Optimizer step | Sequential parameter update | `buff-ml` internal |

---

## 6. IR Impact Assessment

The current `buff-lang-ast/src/ir.rs` dataflow IR (`IrGraph`, `ComputeNode`, `AstLowerer`) operates at **statement-level granularity** — one IR node per AST statement. This is sufficient for the existing WGSL codegen path (single lambda → single shader).

**Tensor ops would need a richer IR** if GPU dispatch is pursued:
- **Multi-buffer dependencies**: matmul reads two input buffers, writes one output. Current IR models single-input → single-output.
- **2D dispatch metadata**: workgroup counts in X and Y dimensions. Current IR has no dispatch dimensionality.
- **Workgroup shared memory annotations**: reductions need `var<workgroup>` declarations. Current IR has no memory-space annotations for GPU.

**Impact**: If GPU tensor ops are deferred to v1.18+, the IR does NOT need changes for MVP. The CPU path uses extern Rust crates directly, bypassing the IR entirely.

---

## 7. Key Constraints & Exceptions

### WGSL codegen is the ONE raw-string exception

Per AGENTS.md: `crates/buff-lang-codegen-wgsl/src/shader.rs` uses `format!()` for WGSL output because WGSL has no `syn` equivalent and `naga`/`wgpu` parsers panic on invalid input rather than return structured errors. This is the **only** crate in the workspace exempt from the "no raw-string codegen" rule. Any tensor GPU extensions would follow the same pattern.

### Binding contract stability

The `@group(0) @binding(0)` / `@group(0) @binding(1)` layout is a **stable contract** between `buff-lang-codegen-wgsl` and `buff-lang-runtime`. Adding matmul (3 buffers) would require a **new binding layout** (e.g., `@binding(2)` for the second input), which must be coordinated between both crates. The existing 2-buffer layout remains unchanged for elementwise ops.

### f64 is not coming to WGSL

WGSL has no f64 scalar type. This is a WGSL spec limitation, not a Buff implementation choice. All tensor GPU ops are f32-only. f64 tensors always use CPU fallback. This is documented in the plan (T8: "dtype f32 only for MVP").

---

## 8. Decision Summary

| Framework | GPU for MVP? | What GPU ops? | CPU fallback | Blocks Wave 2? |
|---|---|---|---|---|
| **buff-tensor** (T8) | **PARTIAL** | Elementwise only (existing WGSL) | rayon + ndarray/nalgebra extern | **NO** — proceed CPU-only |
| **buff-science** (T13) | **NO** | None | nalgebra extern + pure Rust | **NO** — all CPU |
| **buff-ml** (T15) | **NO** | None | Micrograd tape + ndarray extern | **NO** — all CPU |
| **buff-image** (T9) | **NO** (G7 lock) | None | rayon + image crate extern | **NO** |
| **buff-dsp** (T11) | **NO** (G7 lock) | None | rustfft + rayon | **NO** |
| **buff-game** (T16) | **Existing only** | Elementwise (existing WGSL) | rayon | **NO** |

**Bottom line**: T8 (tensor), T13 (science), T15 (ML) are **NOT blocked** by this assessment. All three can proceed with CPU-only MVPs. GPU acceleration for matmul/reduce is a well-understood v1.18+ enhancement with clear cost estimates (~1500 LOC, ~15 days).

---

## 9. References

- WGSL spec: https://www.w3.org/TR/WGSL/
- wgpu compute examples: https://github.com/gfx-rs/wgpu/tree/trunk/wgpu/examples
- Candle (HuggingFace): https://github.com/huggingface/candle — reference Rust tensor + autodiff
- ndarray: https://docs.rs/ndarray/latest/ndarray/
- nalgebra: https://docs.rs/nalgebra/latest/nalgebra/
- Micrograd: https://github.com/karpathy/micrograd — autodiff reference
- Existing decision docs: `.sisyphus/decisions/dioxus-feasibility.md`, `.sisyphus/decisions/rsx-syntax-feasibility.md`

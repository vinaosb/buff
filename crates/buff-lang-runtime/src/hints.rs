//! T49: `@prefer(gpu)` / `@prefer(npu)` hint system — runtime dispatch.
//!
//! T40's [`decide`](crate::threshold::decide) routes work to
//! [`DispatchKind::SingleThread`] / [`DispatchKind::CpuParallel`] /
//! [`DispatchKind::GpuCompute`] based on pure data-size thresholds
//! (`< 1000` / `1000..=50_000` / `> 50_000`). Buff lets the user override
//! that automatic routing with the `@prefer(gpu)` or `@prefer(npu)`
//! attribute on a `func` declaration:
//!
//! ```text
//! @prefer(gpu)
//! func square_all(xs: Vector<Float>) -> Vector<Float>:
//!     return xs.map({ x => x * x })
//! ```
//!
//! The hint is parsed by T35 into an
//! `Attribute { name: Ident("prefer"), args: vec!["gpu".to_string()] }`
//! (see `crates/buff-lang-ast/src/decl.rs`; T48 already uses
//! `has_prefer_gpu_attr` to detect this exact shape). T49's job — THIS
//! module — is the **runtime dispatch decision** that consumes a
//! [`Prefer`] hint and produces a final backend choice.
//!
//! # Cost-model override (the "10 elements → CPU" rule)
//!
//! `@prefer(gpu)` is honored **unless the input is too small for the GPU
//! to be worth programming**. Each GPU dispatch pays a fixed cost in
//! buffer allocation + PCIe transfer + shader dispatch + readback; on
//! any modern GPU+driver stack that cost is on the order of 100 µs–1 ms.
//! For element-wise f32 work the GPU is faster per-element than the CPU
//! but only after enough elements to amortize the dispatch overhead.
//!
//! [`PREFER_GPU_MIN_ELEMENTS`] is the empirical break-even point we
//! document and pin: below it the hint is overridden and [`decide`]
//! runs as if no hint were present. Above it the hint is honored
//! (subject to GPU availability and VRAM headroom). See the constant's
//! rustdoc for the full rationale.
//!
//! # Graceful fallback
//!
//! `@prefer(gpu)` is a *hint*, not a demand: if the host has no GPU
//! adapter, or the input exceeds VRAM, the dispatch falls back to the
//! path [`decide`] would have chosen anyway (typically
//! [`DispatchKind::CpuParallel`] for large inputs).
//!
//! # `@prefer(npu)` semantics (v1.0)
//!
//! Buff targets no real NPU backend in v1.0 (NPU codegen + dispatch is
//! post-v1.0). `@prefer(npu)` is therefore interpreted as **"prefer
//! accelerator"**: route to GPU when one is available and the input is
//! large enough, otherwise CPU. This is the same routing as
//! `@prefer(gpu)`. The two variants share [`Prefer::accelerator_kind`]
//! for downstream inspection but produce identical dispatch decisions
//! today.
//!
//! # "Multi-version codegen" interpretation (v1.0 scope)
//!
//! The v1.0 plan mentions `@prefer(gpu)` "generates both GPU+CPU
//! code". For the runtime, the practical deliverable is
//! [`decide_with_prefer`] picking the backend at runtime plus
//! [`dispatch_with_prefer`] actually running the chosen path — reusing
//! T45's [`WgpuBackend`](crate::WgpuBackend) for GPU and T38b's
//! [`cpu_fallback_map`](crate::cpu_fallback_map) (or any caller-provided
//! CPU oracle) for CPU. The runtime does **not** emit two separate
//! compiled Rust functions for the same Buff func; instead, BOTH paths
//! are reachable at every dispatch site and the decision is made once
//! per call based on the actual input length + runtime GPU availability.
//!
//! # Determinism
//!
//! [`decide_with_prefer`] is a pure function of its five inputs. Same
//! inputs → same output, on every host, every run. No
//! [`std::collections::HashMap`] / [`std::collections::HashSet`]
//! (project hard rule — see [`crate`] docs).
//!
//! # No-panic contract
//!
//! No `unwrap`/`expect`/`panic!`/`todo!`/`unimplemented!` in non-test
//! code. [`decide_with_prefer`] is infallible; [`dispatch_with_prefer`]
//! treats any GPU-side error as "fall back to CPU oracle" and always
//! returns a `Vec<f32>`.
//!
//! [`decide`]: crate::threshold::decide

use crate::mock_gpu::GpuBackend;
use crate::threshold::{decide, decide_dynamic, fits_vram, WorkloadContext};
use crate::DispatchKind;

/// Inclusive lower bound on `element_count` for honoring `@prefer(gpu)`
/// / `@prefer(npu)`.
///
/// Below this size, the cost-model override kicks in: the dispatch is
/// routed through [`decide`] as if no hint were present (i.e. tiny →
/// [`DispatchKind::SingleThread`], medium →
/// [`DispatchKind::CpuParallel`]). The QA case
/// "`@prefer(gpu)` + 10 elements → CPU" relies on this constant being
/// strictly greater than 10.
///
/// # Why 1024
///
/// A single wgpu compute dispatch costs roughly:
///
/// ```text
///   create_shader_module    ≈ 50–200 µs  (first time only — T47 caches)
///   create_buffer_init      ≈ 10–50 µs   (input storage upload)
///   create_buffer           ≈ 5–20 µs    (output + staging)
///   create_pipeline_layout  ≈ 20–100 µs  (first time only — T47 caches)
///   create_compute_pipeline ≈ 100–500 µs (first time only — T47 caches)
///   queue.submit            ≈ 10–50 µs
///   device.poll(Wait)       ≈ 50–200 µs
///   map_async + readback    ≈ 30–100 µs
///   -----------------------------------
///   cold-start total        ≈ 300 µs–1 ms (first dispatch of a shader)
///   warm total (T47 cached) ≈ 100–300 µs
/// ```
///
/// On the CPU side, a sequential f32 element-wise map runs at ~1 ns per
/// element on a modern desktop CPU (cache-resident, branch-free). 1024
/// elements is ~1 µs of CPU work — well under even the warm GPU
/// dispatch cost. Above ~1024 elements the GPU starts winning because:
///
/// 1. The PCIe upload time amortizes (one upload, many invocations).
/// 2. The GPU's per-element throughput (hundreds of GFLOPs on modern
///    discrete cards) overtakes the CPU's ~4 GFLOPs per core.
/// 3. The dispatch overhead becomes a small fraction of the total.
///
/// 1024 is therefore a **defensible lower bound**: below it the CPU is
/// essentially always faster; above it the GPU wins on throughput
/// though the win may be small until ~10 000 elements. Picking 1024
/// (rather than 10 000 or 100 000) errs on the side of honoring the
/// user's `@prefer(gpu)` hint — they wouldn't write the hint if they
/// didn't want GPU dispatch when it's even remotely viable.
///
/// # Pinned by tests
///
/// The QA test `hints_qa_prefer_gpu_with_10_elements_routes_to_cpu`
/// asserts that `@prefer(gpu)` + 10 elements yields a CPU decision
/// (SingleThread, since 10 < [`crate::SINGLE_THREAD_MAX`]). The
/// boundary tests `hints_prefer_gpu_boundary_at_min_elements_*` pin the
/// `PREFER_GPU_MIN_ELEMENTS - 1` (→ CPU) and `PREFER_GPU_MIN_ELEMENTS`
/// (→ GpuCompute, when GPU is available + VRAM fits) cases.
pub const PREFER_GPU_MIN_ELEMENTS: usize = 1024;

/// A user-supplied `@prefer(...)` hint extracted from a Buff `func`
/// declaration.
///
/// Constructed from the AST by [`prefer_from_name_args`] (or a higher-
/// level helper like `buff_lang_types::has_prefer_gpu_attr` + a single
/// match arm — see T48's `recursion.rs`). [`Prefer::default`] is
/// [`Prefer::None`], which gives the un-hinted path through
/// [`decide_with_prefer`]: behavior is byte-identical to T40's
/// [`decide`] alone.
///
/// # `@prefer(npu)` in v1.0
///
/// NPU backend support lands post-v1.0. `@prefer(npu)` is therefore
/// routed the same way as `@prefer(gpu)` (try GPU, else CPU). See the
/// module-level docs for the full rationale.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Prefer {
    /// No `@prefer(...)` attribute present. The dispatch routes through
    /// [`decide`] verbatim — no behavior change vs T40's automatic
    /// threshold logic.
    #[default]
    None,
    /// `@prefer(gpu)`. Honored iff a GPU is available AND the input is
    /// at least [`PREFER_GPU_MIN_ELEMENTS`] elements AND the data fits
    /// VRAM. Otherwise the cost-model override or graceful fallback
    /// kicks in and the dispatch routes through [`decide`].
    Gpu,
    /// `@prefer(npu)`. In v1.0 this maps to "prefer accelerator": the
    /// routing matches [`Prefer::Gpu`] (try GPU, else CPU). NPU-specific
    /// dispatch lands post-v1.0.
    Npu,
}

impl Prefer {
    /// Whether this hint asks for an accelerator (GPU or NPU).
    ///
    /// `@prefer(gpu)` and `@prefer(npu)` both return `true`; `None`
    /// returns `false`. The two accelerator variants produce identical
    /// dispatch decisions in v1.0 (see [`decide_with_prefer`]); this
    /// helper exists for downstream inspection / telemetry.
    #[must_use]
    pub fn is_accelerator(self) -> bool {
        matches!(self, Prefer::Gpu | Prefer::Npu)
    }
}

/// Pure primitive: read a `@prefer(...)` hint from a single
/// `(name, args)` pair (mirroring the [`buff_lang_ast::Attribute`]
/// shape, without forcing this crate to depend on `buff-lang-ast`).
///
/// # Matching rules (mirror T48's `has_prefer_gpu_attr`)
///
/// * `name == "prefer"` AND `args == ["gpu"]` → [`Prefer::Gpu`]
/// * `name == "prefer"` AND `args == ["npu"]` → [`Prefer::Npu`]
/// * Anything else (including `@prefer()` with no args, or multi-arg
///   `@prefer(gpu, force)`) → [`Prefer::None`].
///
/// The multi-arg case is intentionally NOT matched — only the exact
/// single-arg form is honored. This matches T48's
/// `has_prefer_gpu_attr` predicate exactly so a function that T48
/// cleared is routed the same way here.
///
/// # Why this shape (and not `&[Attribute]` directly)
///
/// `buff-lang-ast` is a `[dev-dependencies]` entry for this crate
/// (used by T38b's WGSL snapshot harness). Wiring it into the
/// non-test dependency graph just to read two string fields would
/// pull AST + span + error into every runtime consumer for no
/// architectural benefit. This primitive lets any caller — including
/// a one-line helper inside `buff-lang-types` — translate an
/// `Attribute` into a [`Prefer`] without coupling:
///
/// ```ignore
/// use buff_lang_ast::Attribute;
/// use buff_lang_runtime::{Prefer, prefer_from_name_args};
///
/// fn prefer_from_attributes(attrs: &[Attribute]) -> Prefer {
///     attrs.iter().fold(Prefer::None, |acc, a| {
///         if acc != Prefer::None { acc }
///         else { prefer_from_name_args(&a.name.name, &a.args) }
///     })
/// }
/// ```
///
/// First-match wins (declaration order). If no attribute matches,
/// returns [`Prefer::None`].
///
/// [`buff_lang_ast::Attribute`]: https://docs.rs/buff-lang-ast/latest/buff_lang_ast/struct.Attribute.html
#[must_use]
pub fn prefer_from_name_args(name: &str, args: &[String]) -> Prefer {
    if name != "prefer" {
        return Prefer::None;
    }
    if args.len() != 1 {
        return Prefer::None;
    }
    match args[0].as_str() {
        "gpu" => Prefer::Gpu,
        "npu" => Prefer::Npu,
        _ => Prefer::None,
    }
}

/// Layer a [`Prefer`] hint on top of T40's [`decide`] and produce the
/// final [`DispatchKind`] for one dispatch site.
///
/// # Rules (applied in order)
///
/// 1. **No hint** ([`Prefer::None`]) → delegate to [`decide`]
///    verbatim. **No behavior change** vs T40's automatic threshold
///    logic for un-hinted code.
/// 2. **Cost-model override**: `prefer` is `Gpu` or `Npu` AND
///    `element_count < `[`PREFER_GPU_MIN_ELEMENTS`] → delegate to
///    [`decide`] (the GPU dispatch overhead would exceed the compute
///    savings; the QA "10 elements → CPU" case lands here, yielding
///    [`DispatchKind::SingleThread`] for tiny inputs).
/// 3. **Hint honored**: `prefer` is `Gpu`/`Npu` AND
///    `element_count >= PREFER_GPU_MIN_ELEMENTS` AND `gpu_available`
///    AND the data fits VRAM → [`DispatchKind::GpuCompute`].
/// 4. **Graceful fallback**: hint cannot be honored (no GPU, or VRAM
///    exceeded) → delegate to [`decide`] (typically
///    [`DispatchKind::CpuParallel`] for large inputs).
///
/// [`Prefer::Npu`] is routed identically to [`Prefer::Gpu`] in v1.0
/// (NPU backend is post-v1.0). See the module-level docs.
///
/// # Cost
///
/// Pure O(1) integer arithmetic, allocation-free. Same cost profile
/// as [`decide`].
///
/// # Determinism
///
/// Pure function of its five inputs. No hashing, no allocation, no
/// I/O. Same inputs → same output on every host, every run.
///
/// # Examples
///
/// ```
/// use buff_lang_runtime::{
///     decide_with_prefer, DispatchKind, Prefer, PREFER_GPU_MIN_ELEMENTS,
/// };
///
/// // QA case: @prefer(gpu) + 10 elements → SingleThread (cost override).
/// assert_eq!(
///     decide_with_prefer(10, Prefer::Gpu, true, None, 4),
///     DispatchKind::SingleThread,
/// );
///
/// // @prefer(gpu) + large input + GPU available → GpuCompute.
/// assert_eq!(
///     decide_with_prefer(100_000, Prefer::Gpu, true, None, 4),
///     DispatchKind::GpuCompute,
/// );
///
/// // @prefer(gpu) + large input + NO GPU → CpuParallel (graceful fallback).
/// assert_eq!(
///     decide_with_prefer(100_000, Prefer::Gpu, false, None, 4),
///     DispatchKind::CpuParallel,
/// );
///
/// // No hint — byte-identical to T40's decide() for the same inputs.
/// assert_eq!(
///     decide_with_prefer(10, Prefer::None, true, None, 4),
///     buff_lang_runtime::decide(10, true, None, 4),
/// );
/// ```
#[must_use]
pub fn decide_with_prefer(
    element_count: usize,
    prefer: Prefer,
    gpu_available: bool,
    available_vram_bytes: Option<u64>,
    bytes_per_element: u64,
) -> DispatchKind {
    // Rule 1: no hint — pure delegation. No behavior change vs T40.
    if prefer == Prefer::None {
        return decide(
            element_count,
            gpu_available,
            available_vram_bytes,
            bytes_per_element,
        );
    }

    // From here on, prefer is Gpu or Npu. NPU maps to "prefer accelerator"
    // in v1.0 (no NPU backend yet) → identical routing to Gpu.

    // Rule 2: cost-model override. Small inputs stay on CPU — the GPU
    // dispatch overhead would dominate. Delegating to `decide` preserves
    // the SingleThread (< 1000) and CpuParallel (1000..=50_000) bands.
    if element_count < PREFER_GPU_MIN_ELEMENTS {
        return decide(
            element_count,
            gpu_available,
            available_vram_bytes,
            bytes_per_element,
        );
    }

    // Rule 3: hint honored. Above the cost-override threshold AND a GPU
    // is available AND the data fits VRAM → GpuCompute. This is the
    // honoring path that distinguishes decide_with_prefer from plain
    // decide: T40 would also give GpuCompute here for > 50_000, but the
    // user's @prefer(gpu) lets the GPU path win for the 1024..=50_000
    // band too (where T40 would have picked CpuParallel).
    if gpu_available && fits_vram(element_count, bytes_per_element, available_vram_bytes) {
        return DispatchKind::GpuCompute;
    }

    // Rule 4: graceful fallback. The hint cannot be honored — either no
    // GPU adapter is available, or the data exceeds VRAM. Delegate to
    // T40's decide (which already handles both cases: CpuParallel
    // fallback for > 50_000).
    decide(
        element_count,
        gpu_available,
        available_vram_bytes,
        bytes_per_element,
    )
}

/// T5: Layer a [`Prefer`] hint on top of [`decide_dynamic`] (the
/// workload-aware dynamic dispatcher) and produce the final
/// [`DispatchKind`] for one dispatch site.
///
/// This is the **dynamic** counterpart to [`decide_with_prefer`]. Instead
/// of delegating to T40's static [`decide`] for the no-hint / cost-override
/// / fallback paths, it delegates to [`decide_dynamic`] — which inspects
/// the real runtime [`WorkloadContext`] (element count + GPU availability +
/// arithmetic intensity) to make a workload-aware choice.
///
/// # Rules (applied in order — mirror [`decide_with_prefer`])
///
/// 1. **No hint** ([`Prefer::None`]) → delegate to [`decide_dynamic`].
///    The dynamic dispatcher picks based on workload context: tiny →
///    `SingleThread`, GPU-eligible → `GpuCompute`, else `CpuParallel`.
/// 2. **Cost-model override**: `prefer` is `Gpu`/`Npu` AND
///    `ctx.element_count < `[`PREFER_GPU_MIN_ELEMENTS`] → delegate to
///    [`decide_dynamic`] (the GPU dispatch overhead would exceed the
///    compute savings at this size; the QA "10 elements → CPU" case lands
///    here).
/// 3. **Hint honored**: `prefer` is `Gpu`/`Npu` AND
///    `ctx.element_count >= PREFER_GPU_MIN_ELEMENTS` AND
///    `ctx.gpu_available` AND the data fits VRAM →
///    [`DispatchKind::GpuCompute`]. The user explicitly asked for GPU;
///    intensity is ignored on this path (the hint is an override).
/// 4. **Graceful fallback**: hint cannot be honored (no GPU, or VRAM
///    exceeded) → delegate to [`decide_dynamic`] (which will pick
///    `CpuParallel` for medium/large, or demote GPU-eligible-but-
///    memory-bound work to `CpuParallel`).
///
/// # Difference from [`decide_with_prefer`]
///
/// | Scenario | `decide_with_prefer` (static) | `decide_with_prefer_dynamic` |
/// |----------|-------------------------------|------------------------------|
/// | No hint, medium + GPU + high intensity | `CpuParallel` (static band) | `GpuCompute` (promotion) |
/// | No hint, large + GPU + low intensity | `GpuCompute` (static band) | `CpuParallel` (demotion) |
/// | `@prefer(gpu)` honored | `GpuCompute` | `GpuCompute` (same) |
/// | `@prefer(gpu)` + tiny input | `SingleThread`/`CpuParallel` | same (cost override → dynamic) |
///
/// # Cost
///
/// Pure O(1) — same cost profile as [`decide_with_prefer`] +
/// [`decide_dynamic`]. No I/O, no allocation.
///
/// # Determinism
///
/// Pure function of `(&WorkloadContext, Prefer, Option<u64>, u64)`. No
/// hashing, no clocks, no thread-locals.
///
/// # GPU-fallback guarantee
///
/// When `ctx.gpu_available == false`, this function NEVER returns
/// [`DispatchKind::GpuCompute`] — both the hint-honoring path (rule 3)
/// and [`decide_dynamic`] check `gpu_available` first.
///
/// # Examples
///
/// ```
/// use buff_lang_runtime::{
///     decide_with_prefer_dynamic, DispatchKind, Prefer, WorkloadContext,
/// };
///
/// // No hint + large + GPU + high intensity → GpuCompute (dynamic).
/// let ctx = WorkloadContext::new(100_000, true).with_intensity(8.0);
/// assert_eq!(
///     decide_with_prefer_dynamic(&ctx, Prefer::None, None, 4),
///     DispatchKind::GpuCompute,
/// );
///
/// // @prefer(gpu) + 10 elements → SingleThread (cost override → dynamic).
/// let ctx = WorkloadContext::new(10, true);
/// assert_eq!(
///     decide_with_prefer_dynamic(&ctx, Prefer::Gpu, None, 4),
///     DispatchKind::SingleThread,
/// );
///
/// // @prefer(gpu) + large + GPU → GpuCompute (hint honored).
/// let ctx = WorkloadContext::new(100_000, true);
/// assert_eq!(
///     decide_with_prefer_dynamic(&ctx, Prefer::Gpu, None, 4),
///     DispatchKind::GpuCompute,
/// );
///
/// // @prefer(gpu) + large + NO GPU → CpuParallel (graceful fallback → dynamic).
/// let ctx = WorkloadContext::new(100_000, false);
/// assert_eq!(
///     decide_with_prefer_dynamic(&ctx, Prefer::Gpu, None, 4),
///     DispatchKind::CpuParallel,
/// );
/// ```
#[must_use]
pub fn decide_with_prefer_dynamic(
    ctx: &WorkloadContext,
    prefer: Prefer,
    available_vram_bytes: Option<u64>,
    bytes_per_element: u64,
) -> DispatchKind {
    // Rule 1: no hint — pure dynamic delegation. The workload context
    // (element count + GPU availability + intensity) drives the decision.
    if prefer == Prefer::None {
        return decide_dynamic(ctx);
    }

    // From here on, prefer is Gpu or Npu. NPU maps to "prefer accelerator"
    // in v1.0 (no NPU backend yet) → identical routing to Gpu.

    // Rule 2: cost-model override. Small inputs stay on CPU — the GPU
    // dispatch overhead would dominate. Delegating to decide_dynamic
    // preserves the SingleThread (< 1000) and CpuParallel (1000..=50_000)
    // bands, PLUS the dynamic refinements (intensity-based promote/demote).
    if ctx.element_count < PREFER_GPU_MIN_ELEMENTS {
        return decide_dynamic(ctx);
    }

    // Rule 3: hint honored. Above the cost-override threshold AND a GPU
    // is available AND the data fits VRAM → GpuCompute. The user's
    // @prefer(gpu) is an explicit override — intensity is NOT consulted
    // here (unlike the no-hint dynamic path). If they ask for GPU and
    // it's viable, they get GPU.
    if ctx.gpu_available && fits_vram(ctx.element_count, bytes_per_element, available_vram_bytes) {
        return DispatchKind::GpuCompute;
    }

    // Rule 4: graceful fallback. The hint cannot be honored — either no
    // GPU adapter is available, or the data exceeds VRAM. Delegate to
    // decide_dynamic (which handles both cases via its CpuParallel
    // fallback, plus intensity-aware demotion for memory-bound work).
    decide_dynamic(ctx)
}

/// Top-level dispatch entry that runs the chosen path end-to-end.
///
/// Computes [`decide_with_prefer`] for the input length + hint, then:
///
/// * If the decision is [`DispatchKind::GpuCompute`] AND a GPU backend
///   is provided → attempt `gpu_backend.dispatch_map(shader_wgsl, input)`.
///   On ANY [`RuntimeError`] (device init failure, no adapter mid-flight,
///   shader compile error, map_async error, etc.), fall back to
///   `cpu_oracle(input)`.
/// * Otherwise → run `cpu_oracle(input)` directly.
///
/// **Always** returns a `Vec<f32>` — GPU failure is invisible to the
/// caller, masked by the CPU fallback. This mirrors T46's
/// [`dispatch_map_with_tiling`](crate::dispatch_map_with_tiling)
/// contract.
///
/// # Arguments
///
/// * `prefer` — parsed from the function's `@prefer(...)` attribute
///   via [`prefer_from_name_args`] (or a higher-level AST helper).
/// * `gpu_backend` — `Some(&dyn GpuBackend)` when a GPU is available
///   on this host (typically T45's [`WgpuBackend`](crate::WgpuBackend)
///   or T47's [`ColdStartBackend`](crate::ColdStartBackend)); `None`
///   on hosts without a GPU adapter. The decision function treats
///   `None` as `gpu_available == false`.
/// * `shader_wgsl` — T44 codegen output (the WGSL map kernel). Unused
///   on the CPU path; passed verbatim to the GPU backend on the GPU
///   path.
/// * `input` — the `&[f32]` buffer to map. `bytes_per_element` is
///   hard-coded to 4 (`size_of::<f32>()`); the runtime's element-wise
///   map kernel is f32-only in v1.0.
/// * `available_vram_bytes` — VRAM headroom query (typically T46's
///   [`vram_budget_from_device`](crate::vram_budget_from_device)).
///   `None` means "unknown — assume fits".
/// * `cpu_oracle` — infallible CPU closure (typically T38b's
///   [`cpu_fallback_map`](crate::cpu_fallback_map) wrapping a per-element
///   `f: Fn(f32) -> f32`, or T39's `CpuDispatcher::par_map` for the
///   parallel path).
///
/// # Empty input
///
/// Returns an empty `Vec<f32>` immediately, without invoking either
/// backend. (Both the GPU dispatch and the CPU oracle would return an
/// empty Vec anyway; the early return saves a device init attempt and
/// a thread-pool wakeup.)
///
/// # Example
///
/// ```
/// use buff_lang_runtime::{
///     cpu_fallback_map, dispatch_with_prefer, Prefer,
/// };
///
/// let input = vec![1.0_f32, 2.0, 3.0];
/// let out = dispatch_with_prefer(
///     Prefer::Gpu,
///     None,                       // no GPU on this host → straight to CPU
///     "@compute ...",             // shader source (unused on CPU path)
///     &input,
///     None,                       // VRAM unknown (irrelevant on CPU path)
///     |input| cpu_fallback_map(input, |x| x * 2.0),
/// );
/// assert_eq!(out, vec![2.0, 4.0, 6.0]);
/// ```
#[must_use]
pub fn dispatch_with_prefer<F>(
    prefer: Prefer,
    gpu_backend: Option<&dyn GpuBackend>,
    shader_wgsl: &str,
    input: &[f32],
    available_vram_bytes: Option<u64>,
    cpu_oracle: F,
) -> Vec<f32>
where
    F: Fn(&[f32]) -> Vec<f32>,
{
    // Empty input — short-circuit. Both backends would return empty
    // anyway; this saves a device init attempt and a thread wakeup.
    if input.is_empty() {
        return Vec::new();
    }

    // bytes_per_element is fixed at size_of::<f32>() = 4 for v1.0
    // (runtime's element-wise map kernel is f32-only). Widen to u64
    // losslessly for the fits_vram overflow-aware check.
    const BYTES_PER_F32: u64 = 4;

    let decision = decide_with_prefer(
        input.len(),
        prefer,
        gpu_backend.is_some(),
        available_vram_bytes,
        BYTES_PER_F32,
    );

    if decision == DispatchKind::GpuCompute {
        // decide_with_prefer only yields GpuCompute when gpu_backend was
        // Some(...); the .unwrap-style "must be Some here" is encoded
        // statically via the `if let Some(backend)` pattern, never via
        // unwrap.
        if let Some(backend) = gpu_backend {
            // Attempt the GPU path. On ANY RuntimeError, fall back to
            // the CPU oracle — the caller's contract is "always returns
            // Vec<f32>", matching dispatch_map_with_tiling's contract
            // (T46).
            if let Ok(gpu_out) = backend.dispatch_map(shader_wgsl, input) {
                return gpu_out;
            }
            // GPU-side failure → fall through to the CPU oracle.
        }
    }

    // CPU path (SingleThread / CpuParallel decision, OR GPU-fallback
    // after an error, OR the rare case where decide_with_prefer picked
    // GpuCompute but the backend slot was None — can't happen given
    // decide_with_prefer uses gpu_backend.is_some() as the gpu_available
    // flag, but the fallback is defensive and cheap).
    cpu_oracle(input)
}

/// T6: Explain WHY a dispatch decision was made, including `@prefer` hint info.
///
/// Extends [`explain_dispatch`] with the hint override details. Zero-overhead
/// when not called — the `String` is only allocated on invocation.
///
/// # Output format
///
/// ```text
/// Dispatch: GpuCompute
///   element_count: 100000
///   gpu_available: true
///   arithmetic_intensity: 8.0 (>= 4.0 threshold → GPU-favorable)
///   SINGLE_THREAD_MAX: 999 (branch not taken: count > 999)
///   CPU_PARALLEL_MAX: 50000 (branch not taken: count > 50000)
///   @prefer(gpu): honored (GPU available + count >= PREFER_GPU_MIN_ELEMENTS=1024)
///   Decision: GPU available + high intensity → GpuCompute
/// ```
///
/// The `@prefer` line is omitted when `prefer == Prefer::None`.
#[must_use]
pub fn explain_dispatch_with_prefer(
    ctx: &WorkloadContext,
    prefer: Prefer,
    available_vram_bytes: Option<u64>,
    bytes_per_element: u64,
    decision: DispatchKind,
) -> String {
    let base = crate::threshold::explain_dispatch(ctx, decision);

    if prefer == Prefer::None {
        return base;
    }

    let prefer_line = if decision == DispatchKind::GpuCompute {
        format!(
            "  @prefer({target}): honored (GPU available + count >= PREFER_GPU_MIN_ELEMENTS={min})",
            target = prefer_label(prefer),
            min = PREFER_GPU_MIN_ELEMENTS,
        )
    } else if ctx.element_count < PREFER_GPU_MIN_ELEMENTS {
        format!(
            "  @prefer({target}): cost-model override (count < PREFER_GPU_MIN_ELEMENTS={min})",
            target = prefer_label(prefer),
            min = PREFER_GPU_MIN_ELEMENTS,
        )
    } else if !ctx.gpu_available {
        format!(
            "  @prefer({target}): not honored (no GPU available)",
            target = prefer_label(prefer),
        )
    } else if !crate::threshold::fits_vram(ctx.element_count, bytes_per_element, available_vram_bytes)
    {
        format!(
            "  @prefer({target}): not honored (data exceeds VRAM)",
            target = prefer_label(prefer),
        )
    } else {
        format!(
            "  @prefer({target}): not honored (unknown reason)",
            target = prefer_label(prefer),
        )
    };

    // Insert the prefer line before the Decision line.
    if let Some(pos) = base.rfind("\n  Decision:") {
        let (before, after) = base.split_at(pos);
        format!("{before}\n{prefer_line}{after}")
    } else {
        // Fallback: append at the end (shouldn't happen in practice).
        format!("{base}\n{prefer_line}")
    }
}

/// Render a [`Prefer`] variant as a lowercase label for explain output.
fn prefer_label(prefer: Prefer) -> &'static str {
    match prefer {
        Prefer::None => "none",
        Prefer::Gpu => "gpu",
        Prefer::Npu => "npu",
    }
}

#[cfg(test)]
mod tests {
    //! Inline smoke tests for [`Prefer`], [`prefer_from_name_args`], and
    //! [`decide_with_prefer`]. Full behavioral coverage lives in
    //! `tests/hints_tests.rs` so the QA filter
    //! `cargo test -p buff-lang-runtime hints` matches the whole suite.

    use super::*;

    #[test]
    fn hints_module_smoke_prefer_default_is_none() {
        let p: Prefer = Default::default();
        assert_eq!(p, Prefer::None);
        assert!(!p.is_accelerator());
    }

    #[test]
    fn hints_module_smoke_prefer_is_accelerator() {
        assert!(Prefer::Gpu.is_accelerator());
        assert!(Prefer::Npu.is_accelerator());
        assert!(!Prefer::None.is_accelerator());
    }

    #[test]
    fn hints_module_smoke_qa_prefer_gpu_10_elements_is_single_thread() {
        // The exact QA case in inline form — full integration coverage
        // in tests/hints_tests.rs.
        assert_eq!(
            decide_with_prefer(10, Prefer::Gpu, true, None, 4),
            DispatchKind::SingleThread,
        );
    }

    #[test]
    fn hints_module_smoke_prefer_gpu_large_input_with_gpu_is_gpu_compute() {
        assert_eq!(
            decide_with_prefer(100_000, Prefer::Gpu, true, None, 4),
            DispatchKind::GpuCompute,
        );
    }

    #[test]
    fn hints_module_smoke_prefer_none_matches_decide_verbatim() {
        for (count, gpu, vram, bpe) in [
            (10usize, true, None, 8u64),
            (1_000, true, None, 8),
            (50_001, true, None, 8),
            (50_001, false, None, 8),
            (1_000_000, true, Some(1), 1_073_741_824),
        ] {
            assert_eq!(
                decide_with_prefer(count, Prefer::None, gpu, vram, bpe),
                decide(count, gpu, vram, bpe),
                "Prefer::None must match decide() verbatim for (count={count}, gpu={gpu}, vram={vram:?}, bpe={bpe})"
            );
        }
    }

    #[test]
    fn explain_dispatch_with_prefer_none_omits_prefer_line() {
        let ctx = WorkloadContext::new(100_000, true).with_intensity(8.0);
        let decision = decide_with_prefer_dynamic(&ctx, Prefer::None, None, 4);
        let explain = explain_dispatch_with_prefer(&ctx, Prefer::None, None, 4, decision);
        assert_eq!(decision, DispatchKind::GpuCompute);
        // Prefer::None should produce the same output as explain_dispatch.
        let base = crate::threshold::explain_dispatch(&ctx, decision);
        assert_eq!(explain, base);
        assert!(!explain.contains("@prefer"));
    }

    #[test]
    fn explain_dispatch_with_prefer_gpu_honored() {
        let ctx = WorkloadContext::new(100_000, true);
        let decision = decide_with_prefer_dynamic(&ctx, Prefer::Gpu, None, 4);
        let explain = explain_dispatch_with_prefer(&ctx, Prefer::Gpu, None, 4, decision);
        assert_eq!(decision, DispatchKind::GpuCompute);
        assert!(explain.contains("@prefer(gpu): honored"));
        assert!(explain.contains("PREFER_GPU_MIN_ELEMENTS=1024"));
    }

    #[test]
    fn explain_dispatch_with_prefer_gpu_cost_override() {
        let ctx = WorkloadContext::new(10, true);
        let decision = decide_with_prefer_dynamic(&ctx, Prefer::Gpu, None, 4);
        let explain = explain_dispatch_with_prefer(&ctx, Prefer::Gpu, None, 4, decision);
        assert_eq!(decision, DispatchKind::SingleThread);
        assert!(explain.contains("@prefer(gpu): cost-model override"));
        assert!(explain.contains("count < PREFER_GPU_MIN_ELEMENTS=1024"));
    }

    #[test]
    fn explain_dispatch_with_prefer_gpu_no_gpu_available() {
        let ctx = WorkloadContext::new(100_000, false);
        let decision = decide_with_prefer_dynamic(&ctx, Prefer::Gpu, None, 4);
        let explain = explain_dispatch_with_prefer(&ctx, Prefer::Gpu, None, 4, decision);
        assert_eq!(decision, DispatchKind::CpuParallel);
        assert!(explain.contains("@prefer(gpu): not honored (no GPU available)"));
    }

    #[test]
    fn hints_module_smoke_prefer_from_name_args_exact_match() {
        assert_eq!(
            prefer_from_name_args("prefer", &["gpu".to_string()]),
            Prefer::Gpu
        );
        assert_eq!(
            prefer_from_name_args("prefer", &["npu".to_string()]),
            Prefer::Npu
        );
        assert_eq!(
            prefer_from_name_args("prefer", &[String::new()]),
            Prefer::None,
            "unknown target is None"
        );
        assert_eq!(
            prefer_from_name_args("test", &[]),
            Prefer::None,
            "non-prefer attribute is None"
        );
        assert_eq!(
            prefer_from_name_args("prefer", &["gpu".to_string(), "force".to_string()]),
            Prefer::None,
            "multi-arg prefer is None (intentional — matches T48)"
        );
    }
}

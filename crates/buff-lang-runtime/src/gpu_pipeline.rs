//! T45: Real wgpu-backed GPU dispatch pipeline.
//!
//! [`WgpuBackend`] is the production implementation of the [`GpuBackend`]
//! trait (defined in [`crate::mock_gpu`]). It takes a WGSL compute shader
//! (produced by T44's `buff_lang_codegen_wgsl::generate_wgsl`) and an
//! `&[f32]` input, uploads the input to a storage buffer, runs a compute
//! pass with `ceil(len/64)` workgroups, and reads back the output via
//! `map_async` + `device.poll(PollType::Wait)`.
//!
//! # Pipeline
//!
//! ```text
//! shader_wgsl (T44)  +  input: &[f32]
//!     │
//!     ▼  device.create_shader_module(ShaderSource::Wgsl(Cow::Borrowed))
//! ShaderModule
//!     │
//!     ▼  device.create_buffer_init  (STORAGE | COPY_DST)
//! input_buffer  (binding 0, var<storage, read>)
//!     │
//!     ▼  device.create_buffer  (STORAGE | COPY_SRC)
//! output_buffer  (binding 1, var<storage, read_write>)
//!     │
//!     ▼  device.create_buffer  (MAP_READ | COPY_DST)
//! staging_buffer  (host-visible readback)
//!     │
//!     ▼  device.create_bind_group_layout (b0=read_storage, b1=rw_storage)
//! bind_group_layout
//!     │
//!     ▼  device.create_pipeline_layout + create_compute_pipeline
//! pipeline  (entry_point = "main" — T44 codegen names it thus)
//!     │
//!     ▼  device.create_command_encoder + begin_compute_pass
//!     │           set_pipeline(pipeline)
//!     │           set_bind_group(0, bind_group, &[])
//!     │           dispatch_workgroups(workgroup_count(len), 1, 1)
//!     │           copy_buffer_to_buffer(output → staging)
//!     ▼  encoder.finish()
//! command_buffer
//!     │
//!     ▼  queue.submit(Some(cmd))  +  device.poll(PollType::Wait)
//! GPU work completes
//!     │
//!     ▼  staging.slice(..).map_async(Read, cb)
//!     │           rx.recv()  (callback sends result via mpsc channel)
//!     │           device.poll(PollType::Wait) drains the callback
//!     ▼  staging.slice(..).get_mapped_range()
//! &[u8]  →  bytemuck::cast_slice::<u8, f32>  →  Vec<f32>
//! ```
//!
//! # Workgroup sizing
//!
//! T44 codegen emits `@compute @workgroup_size(64)` so each workgroup
//! processes 64 elements (one per invocation; the shader reads
//! `input[gid.x]` and bounds-checks against `arrayLength(&input)`). For
//! an `N`-element input we dispatch `ceil(N/64)` workgroups in the X
//! dimension. [`workgroup_count`] is the pure ceiling-division helper
//! used here — also unit-tested in-module.
//!
//! # Error model
//!
//! Every fallible wgpu step is mapped to [`RuntimeError`]:
//! * no GPU adapter → [`RuntimeError::GpuUnavailable`]
//! * device/shader/buffer/pipeline failure → [`RuntimeError::GpuInit`]
//! * map_async returns `Err(BufferAsyncError)` → [`RuntimeError::GpuInit`]
//!
//! No `unwrap` / `expect` / `panic!` / `todo!` anywhere in non-test code.
//!
//! # Empty input
//!
//! [`WgpuBackend::dispatch_map`] guards the 0-length case: an empty
//! input returns `Ok(Vec::new())` immediately, without ever creating a
//! command encoder (wgpu forbids 0-sized dispatches and 0-sized
//! `copy_buffer_to_buffer`).
//!
//! # Determinism
//!
//! Same `(shader_wgsl, input)` → same `Vec<f32>` on every run, given the
//! same device. The compute shader itself is deterministic (no atomics,
//! no shared memory races for element-wise maps — one invocation per
//! element, no inter-invocation communication). Readback order is
//! preserved by `copy_buffer_to_buffer`'s contiguous-source semantics.
//!
//! No [`std::collections::HashMap`] / [`std::collections::HashSet`]
//! anywhere in this module (project hard rule).

use std::borrow::Cow;

use wgpu::util::DeviceExt;

use crate::error::RuntimeError;
use crate::gpu::{GpuContext, GpuContextError};
use crate::mock_gpu::GpuBackend;

/// Convert a [`&GpuContextError`] into a [`RuntimeError`] (the
/// `From<GpuContextError>` impl in [`crate::gpu`] requires owned; we
/// only have a borrow from the OnceLock cache, so match manually).
///
/// Kept as a `pub(crate)` helper so both [`WgpuBackend::dispatch_map`]
/// (T45) and [`crate::cold_start::ColdStartBackend::dispatch_map`] (T47)
/// can map the same borrow-only error without duplicating the variant
/// match.
pub(crate) fn gpu_ctx_err_to_runtime(e: &GpuContextError) -> RuntimeError {
    match e {
        GpuContextError::NoAdapter => RuntimeError::GpuUnavailable,
        GpuContextError::DeviceRequest(detail) => RuntimeError::GpuInit {
            detail: detail.clone(),
        },
    }
}

/// Workgroup size hard-coded by T44 codegen (`@compute @workgroup_size(64)`).
///
/// Each invocation processes one input element; one workgroup covers 64
/// elements. Bumping this requires a matching change in T44's codegen
/// and in the bind-group layout — kept as a `const` here so the
/// `dispatch_workgroups` argument and the unit tests reference one
/// source of truth.
pub const WORKGROUP_SIZE: usize = 64;

/// Ceiling division of `len` by [`WORKGROUP_SIZE`].
///
/// Returns the number of workgroups to dispatch along the X dimension
/// so that every element of a `len`-long input is covered by at least
/// one invocation.
///
/// * `len = 0` → `0` (no dispatch needed; callers should also early-
///   return on empty input).
/// * `len = 1..=64` → `1`.
/// * `len = 65..=128` → `2`.
/// * `len = 129..=192` → `3`.
/// * And so on: one workgroup per 64 elements (or fraction thereof).
///
/// # Examples
///
/// ```
/// use buff_lang_runtime::workgroup_count;
/// assert_eq!(workgroup_count(0), 0);
/// assert_eq!(workgroup_count(1), 1);
/// assert_eq!(workgroup_count(64), 1);
/// assert_eq!(workgroup_count(65), 2);
/// assert_eq!(workgroup_count(128), 2);
/// assert_eq!(workgroup_count(129), 3);
/// ```
pub fn workgroup_count(len: usize) -> u32 {
    // usize::div_ceil stabilised in Rust 1.73. toolchain pins 1.95.0.
    // Relaxed about overflow: a usize input over 2^64 - 64 would
    // overflow on the addition in the manual `(len + 63) / 64` form;
    // div_ceil is overflow-safe by construction (single division).
    // Such an input is physically impossible to upload to a GPU anyway
    // (it would exceed every modern GPU's VRAM by many orders of
    // magnitude) and the buffer-size step below would already fail
    // with a wgpu validation error long before reaching the dispatch.
    len.div_ceil(WORKGROUP_SIZE) as u32
}

/// Real wgpu-backed GPU backend — implements [`GpuBackend`] via a
/// straight dispatch → copy → readback pipeline.
///
/// Construct with [`WgpuBackend::new`] (which acquires its own
/// [`GpuContext`] via [`GpuContext::new`]) or with
/// [`WgpuBackend::from_context`] (when the caller already owns a
/// context, e.g. for tests that want to pass an `unavailable()` context
/// to verify graceful no-GPU error handling).
///
/// The backend holds a [`GpuContext`] which lazily caches the
/// `(Device, Queue)` pair (OnceLock-backed, see T43). All dispatches
/// share that one cached pair — the cost of device init is paid once.
///
/// # No GPU? No problem.
///
/// When this host has no GPU adapter, [`WgpuBackend::new`] returns
/// [`RuntimeError::GpuUnavailable`] (the constructor itself does not
/// acquire a device — but [`GpuContext::new`] needs an adapter).
/// [`WgpuBackend::dispatch_map`] then also returns `GpuUnavailable` so
/// callers can fall back to the CPU path (T40 threshold logic, T49
/// `@prefer` hints).
///
/// # Send + Sync
///
/// [`GpuContext`] is `Send + Sync` (wgpu 26 `Adapter`/`Device`/`Queue`
/// are all Send + Sync per T43 findings), so `WgpuBackend` is too — it
/// can be held as `Arc<WgpuBackend>` across threads, or as
/// `Box<dyn GpuBackend>` (the trait requires Send + Sync).
#[derive(Debug)]
pub struct WgpuBackend {
    /// Cached GPU context providing the lazily-acquired
    /// `(Device, Queue)` pair used by every dispatch.
    context: GpuContext,
}

impl WgpuBackend {
    /// Acquire a GPU adapter and construct a backend.
    ///
    /// Returns [`RuntimeError::GpuUnavailable`] on hosts with no GPU
    /// adapter — **never panics**.
    ///
    /// The `(Device, Queue)` pair is NOT acquired here; it is lazily
    /// fetched on the first [`Self::dispatch_map`] call (via
    /// [`GpuContext::device_queue`]`()`'s OnceLock, per T43). This
    /// keeps construction fast and defers the expensive device-init
    /// future to the first dispatch.
    pub fn new() -> Result<Self, RuntimeError> {
        let context = GpuContext::new().map_err(RuntimeError::from)?;
        Ok(Self { context })
    }

    /// Wrap an existing [`GpuContext`] — useful when the caller wants
    /// to share a single context across multiple dispatchers or wants
    /// to inject an `unavailable()` context for graceful-no-GPU tests.
    #[must_use]
    pub fn from_context(context: GpuContext) -> Self {
        Self { context }
    }

    /// Borrow the inner [`GpuContext`] — useful for diagnostics
    /// (`adapter_name()`, `has_device()`, `device_init_count()`).
    #[must_use]
    pub fn context(&self) -> &GpuContext {
        &self.context
    }

    /// Whether the cached `(Device, Queue)` pair has been successfully
    /// initialized on this backend. Purely observational — does NOT
    /// drive initialization. Returns `false` before the first
    /// dispatch and on every host where device init failed.
    #[must_use]
    pub fn has_device(&self) -> bool {
        self.context.has_device()
    }
}

impl GpuBackend for WgpuBackend {
    fn dispatch_map(&self, shader_wgsl: &str, input: &[f32]) -> Result<Vec<f32>, RuntimeError> {
        // Guard: empty input — no buffer, no dispatch. wgpu forbids
        // 0-sized copy_buffer_to_buffer and 0-group dispatches; the
        // caller's intent for empty input is "return empty".
        if input.is_empty() {
            return Ok(Vec::new());
        }

        // Fetch cached device + queue. Maps &GpuContextError -> RuntimeError
        // (the existing From impl takes owned; we only have a borrow from
        // the OnceLock cache, so go through gpu_ctx_err_to_runtime).
        let device = self.context.device().map_err(gpu_ctx_err_to_runtime)?;
        let queue = self.context.queue().map_err(gpu_ctx_err_to_runtime)?;

        run_dispatch(device, queue, shader_wgsl, input)
    }
}

/// Run a single compute dispatch + readback.
///
/// Broken out of [`WgpuBackend::dispatch_map`] so it can be unit-tested
/// with a `(Device, Queue)` pair obtained any other way (e.g. from a
/// different context). All fallible steps are mapped to
/// [`RuntimeError::GpuInit`]; none panic.
///
/// # Arguments
///
/// * `device`, `queue` — the cached wgpu handles from T43's
///   [`GpuContext`].
/// * `shader_wgsl` — WGSL source produced by T44's codegen. MUST match
///   the binding layout hard-coded below: `@group(0) @binding(0)` =
///   read-only storage input array, `@group(0) @binding(1)` =
///   read_write storage output array, `@compute @workgroup_size(64)`,
///   entry point `main`.
/// * `input` — non-empty `&[f32]` (caller guards the empty case).
fn run_dispatch(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    shader_wgsl: &str,
    input: &[f32],
) -> Result<Vec<f32>, RuntimeError> {
    // ----- 1. Shader module ------------------------------------------------
    //
    // T44 codegen produces WGSL with one `@compute @workgroup_size(64)`
    // entry point named `main`. We pass the source through as a borrowed
    // Cow to avoid an allocation.
    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("buff-dispatch-shader"),
        source: wgpu::ShaderSource::Wgsl(Cow::Borrowed(shader_wgsl)),
    });

    // ----- 2. Input storage buffer (binding 0: var<storage, read>) ---------
    //
    // create_buffer_init uses `mapped_at_creation: true` to upload the
    // bytes without needing a queue.write_buffer step (slightly faster
    // on first dispatch). We pay the COPY_DST flag defensively in case
    // a future task wants to re-upload into the same buffer.
    let input_bytes: &[u8] = bytemuck::cast_slice(input);
    let byte_size = input_bytes.len() as wgpu::BufferAddress;
    let input_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("buff-dispatch-input"),
        contents: input_bytes,
        usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
    });

    // ----- 3. Output storage buffer (binding 1: var<storage, read_write>) --
    let output_buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("buff-dispatch-output"),
        size: byte_size,
        usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
        mapped_at_creation: false,
    });

    // ----- 4. Staging buffer (host-visible readback) -----------------------
    //
    // MAP_READ so map_async(Read) succeeds; COPY_DST so the
    // copy_buffer_to_buffer step can target it. Same byte size as the
    // output (no padding needed — input_bytes.len() is always a
    // multiple of 4 since input is &[f32], satisfying the
    // COPY_BUFFER_ALIGNMENT = 4 requirement).
    let staging_buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("buff-dispatch-staging"),
        size: byte_size,
        usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });

    // ----- 5. Bind group layout (matches T44 binding layout EXACTLY) -------
    let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("buff-dispatch-bgl"),
        entries: &[
            wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::COMPUTE,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Storage { read_only: true },
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            },
            wgpu::BindGroupLayoutEntry {
                binding: 1,
                visibility: wgpu::ShaderStages::COMPUTE,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Storage { read_only: false },
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            },
        ],
    });

    // ----- 6. Bind group (binds our actual buffers to the layout) ----------
    let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("buff-dispatch-bg"),
        layout: &bind_group_layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                    buffer: &input_buffer,
                    offset: 0,
                    size: None,
                }),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                    buffer: &output_buffer,
                    offset: 0,
                    size: None,
                }),
            },
        ],
    });

    // ----- 7. Pipeline layout + compute pipeline ---------------------------
    let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("buff-dispatch-pipeline-layout"),
        bind_group_layouts: &[&bind_group_layout],
        push_constant_ranges: &[],
    });

    let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
        label: Some("buff-dispatch-pipeline"),
        layout: Some(&pipeline_layout),
        module: &shader,
        // T44 codegen names the entry point `main`. Passing Some("main")
        // explicitly avoids the implicit "exactly one entry point"
        // fallback in case a future shader adds helper entry points.
        entry_point: Some("main"),
        compilation_options: wgpu::PipelineCompilationOptions::default(),
        cache: None,
    });

    // ----- 8. Encode compute pass + copy -----------------------------------
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("buff-dispatch-encoder"),
    });
    {
        let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
            label: Some("buff-dispatch-pass"),
            timestamp_writes: None,
        });
        pass.set_pipeline(&pipeline);
        pass.set_bind_group(0, &bind_group, &[]);
        // Ceiling division: each workgroup covers WORKGROUP_SIZE elements.
        pass.dispatch_workgroups(workgroup_count(input.len()), 1, 1);
    }
    // Output → staging for host-visible readback. Same byte_size on
    // both sides; satisfies COPY_BUFFER_ALIGNMENT = 4 trivially.
    encoder.copy_buffer_to_buffer(&output_buffer, 0, &staging_buffer, 0, byte_size);

    let command_buffer = encoder.finish();

    // ----- 9. Submit + poll until complete ---------------------------------
    queue.submit(std::iter::once(command_buffer));

    // Drive the queue until the dispatch + copy are complete. wgpu 26
    // signature: poll(PollType) -> Result<PollStatus, PollError>.
    // PollType::Wait blocks the calling thread until the queue empties
    // (or a timeout configured elsewhere expires). PollError::Timeout
    // becomes RuntimeError::GpuInit.
    if let Err(e) = device.poll(wgpu::PollType::Wait) {
        return Err(RuntimeError::GpuInit {
            detail: format!("device.poll(Wait) failed: {e:?}"),
        });
    }

    // ----- 10. map_async + drain via poll + read mapped range --------------
    //
    // map_async takes a `FnOnce(Result<(), BufferAsyncError>) + Send +
    // 'static` callback. The callback fires on the device thread (or
    // during the next poll, depending on backend). Use an mpsc channel
    // to surface the result back to this thread.
    let (tx, rx) = std::sync::mpsc::channel::<Result<(), wgpu::BufferAsyncError>>();
    staging_buffer
        .slice(..)
        .map_async(wgpu::MapMode::Read, move |result| {
            // Send-and-forget: the receiver is dropped before this
            // callback fires in the failure-during-teardown case.
            // Ignoring the send error is correct (the callback's job
            // is to fire the map_async completion, not to outlive the
            // caller's interest).
            let _ = tx.send(result);
        });

    // Drain the callback. Per wgpu docs, the map_async completion is
    // delivered during a poll() call — so a second poll(Wait) here is
    // what actually runs the closure above and unblocks rx.recv().
    if let Err(e) = device.poll(wgpu::PollType::Wait) {
        return Err(RuntimeError::GpuInit {
            detail: format!("device.poll(Wait) for map_async failed: {e:?}"),
        });
    }

    // Receive the callback's result. recv() blocks until the sender
    // fires or is dropped — both lead to a deterministic outcome.
    let map_result = rx.recv().map_err(|e| RuntimeError::GpuInit {
        detail: format!("map_async callback did not fire (sender dropped): {e}"),
    })?;
    map_result.map_err(|e| RuntimeError::GpuInit {
        detail: format!("map_async(BufferAsyncError): {e:?}"),
    })?;

    // ----- 11. Read mapped range → Vec<f32> --------------------------------
    //
    // Bind the BufferView to a local so we control its lifetime: we need
    // the borrow to end (via explicit drop) before `.unmap()` — wgpu
    // panics on unmap while a mapped-range view is still alive.
    let view = staging_buffer.slice(..).get_mapped_range();
    let bytes: &[u8] = &view;
    let output: Vec<f32> = bytemuck::cast_slice::<u8, f32>(bytes).to_vec();
    drop(view);

    // ----- 12. Cleanup -----------------------------------------------------
    //
    // unmap so future map_async calls on this buffer would succeed (we
    // could also destroy() — but Drop is enough; wgpu frees GPU memory
    // on Drop without an explicit destroy() call). Defensive destroy()
    // for the GPU buffers also speeds up resource reclamation.
    staging_buffer.unmap();
    input_buffer.destroy();
    output_buffer.destroy();
    staging_buffer.destroy();

    Ok(output)
}

#[cfg(test)]
mod tests {
    //! Inline unit tests for [`workgroup_count`] — full behavioral
    //! coverage of [`WgpuBackend`] lives in `tests/gpu_dispatch_tests.rs`
    //! so the QA filter `cargo test -p buff-lang-runtime gpu_dispatch`
    //! matches the whole suite.

    use super::workgroup_count;

    #[test]
    fn gpu_dispatch_workgroup_count_zero_returns_zero() {
        assert_eq!(workgroup_count(0), 0);
    }

    #[test]
    fn gpu_dispatch_workgroup_count_one_returns_one() {
        assert_eq!(workgroup_count(1), 1);
    }

    #[test]
    fn gpu_dispatch_workgroup_count_64_returns_one() {
        assert_eq!(workgroup_count(64), 1);
    }

    #[test]
    fn gpu_dispatch_workgroup_count_65_returns_two() {
        assert_eq!(workgroup_count(65), 2);
    }

    #[test]
    fn gpu_dispatch_workgroup_count_128_returns_two() {
        assert_eq!(workgroup_count(128), 2);
    }

    #[test]
    fn gpu_dispatch_workgroup_count_129_returns_three() {
        assert_eq!(workgroup_count(129), 3);
    }

    #[test]
    fn gpu_dispatch_workgroup_count_is_ceiling_division_by_64() {
        // Property: ceiling division. For all n >= 1,
        // workgroup_count(n) * 64 >= n  AND  (workgroup_count(n) - 1) * 64 < n.
        for n in 1..1_000usize {
            let wg = workgroup_count(n) as usize;
            assert!(
                wg * super::WORKGROUP_SIZE >= n,
                "n={n}: wg*64 ({}) should cover n",
                wg * super::WORKGROUP_SIZE
            );
            assert!(
                (wg.saturating_sub(1)) * super::WORKGROUP_SIZE < n,
                "n={n}: (wg-1)*64 should not cover n"
            );
        }
    }
}

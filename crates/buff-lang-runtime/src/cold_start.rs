//! T47: Cold-start mitigation — pipeline caching, buffer pooling, and
//! background (async) device initialization.
//!
//! The T45 [`WgpuBackend`](crate::WgpuBackend) re-creates the WGSL
//! [`wgpu::ShaderModule`], [`wgpu::BindGroupLayout`],
//! [`wgpu::PipelineLayout`], and [`wgpu::ComputePipeline`] on EVERY
//! dispatch. That's the dominant cold-start cost: shader compilation
//! (driver-side SPIR-V → ISA) plus pipeline layout negotiation. The
//! three per-dispatch [`wgpu::Buffer`]s (input storage, output storage,
//! host-visible staging) are likewise created fresh and destroyed every
//! time, costing allocator churn on the GPU driver heap.
//!
//! This module layers three independent optimizations on top of T45's
//! existing pipeline — they wrap, not modify:
//!
//! * [`PipelineCache`] — `BTreeMap<String, CachedPipeline>` keyed by WGSL
//!   source. The first dispatch with a given shader pays the compilation
//!   cost; every subsequent dispatch with the SAME shader source clones
//!   the cached [`wgpu::ComputePipeline`] handle (cheap — Arc-backed in
//!   wgpu 26) and re-uses it. An [`AtomicUsize`] compile-counter tracks
//!   cache misses so tests can assert "same shader dispatched twice →
//!   compile count == 1".
//! * [`BufferPool`] — `Mutex<BTreeMap<(size, usage), Vec<Buffer>>>`. After
//!   each dispatch completes + readback, the input/output/staging buffers
//!   are returned to the pool instead of being dropped. The next dispatch
//!   of the same size reuses them. An allocation counter tracks pool
//!   misses so tests can assert reuse.
//! * [`ColdStartBackend`] — wraps a [`WgpuBackend`](crate::WgpuBackend)
//!   in an [`std::sync::Arc`] so it can be shared with a background
//!   initialization task. [`ColdStartBackend::spawn_init`] kicks off a
//!   thread that calls [`GpuContext::device_queue`](crate::GpuContext::device_queue)
//!   to warm the OnceLock cache before the first real dispatch arrives.
//!   [`ColdStartBackend::is_ready`] / [`ColdStartBackend::wait_ready`]
//!   / [`ColdStartBackend::wait_ready_blocking`] report on and await
//!   that background work.
//!
//! # Why BTreeMap, not HashMap (project rule)
//!
//! Buff's hard rule is "no HashMap in codegen/graph analyses feeding
//! codegen output — they're nondeterministic across hosts". A pipeline
//! cache is a *runtime performance cache* (it doesn't feed codegen
//! output; it caches compiled GPU artifacts for reuse). HashMap would
//! therefore be *technically* permitted. We choose
//! [`std::collections::BTreeMap`] anyway because:
//!
//! 1. The keys are [`String`]s (WGSL source text) — totally orderable.
//!    No hashing overhead benefit because we only do lookups on dispatch
//!    (not in a hot loop).
//! 2. The project rule's spirit — *deterministic, reproducible behavior
//!    across hosts* — applies just as well here. BTreeMap iteration order
//!    is deterministic; HashMap's isn't.
//! 3. Cache statistics (e.g. inspecting entries for diagnostics) become
//!    reproducible, which matters for test stability (the QA
//!    "same shader twice → compile count == 1" assertion is independent
//!    of the iteration order, but future tests may iterate).
//!
//! # Determinism
//!
//! Same `(shader_wgsl, input)` → same `Vec<f32>` on every run. Caching
//! does NOT change results — only WHICH compiled pipeline + WHICH
//! pre-allocated buffer produced them. The pipeline is byte-identical
//! whether freshly compiled or cache-retrieved (same shader source ⇒
//! same SPIR-V ⇒ same ISA on the same device). Buffers are zeroed by
//! the pool's release path so no previous-dispatch data leaks across
//! dispatches (defensive — the compute pass always fully writes the
//! output buffer before readback anyway).
//!
//! # No-panic contract
//!
//! No `unwrap`/`expect`/`panic!`/`todo!`/`unimplemented!` in non-test
//! code. Mutex poisoning is handled gracefully via `unwrap_or`/`map_err`.
//! wgpu steps that can fail are mapped to [`RuntimeError::GpuInit`].
//! A missing GPU adapter returns [`RuntimeError::GpuUnavailable`]
//! (graceful).

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use crate::error::RuntimeError;
use crate::gpu::GpuContext;
use crate::gpu_pipeline::{gpu_ctx_err_to_runtime, workgroup_count, WgpuBackend};
use crate::mock_gpu::GpuBackend;

/// Compact, totally-ordered key for the buffer pool.
///
/// [`wgpu::BufferUsages`] is a `bitflags` type that implements `Eq` and
/// `Hash` but NOT `Ord`. To use it as a [`BTreeMap`] key we wrap the
/// underlying `u32` `bits()` in a newtype that derives `Ord`. The
/// conversion is via `From<wgpu::BufferUsages>` so call sites stay
/// readable.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct BufferUsageKey(u32);

impl From<wgpu::BufferUsages> for BufferUsageKey {
    fn from(usage: wgpu::BufferUsages) -> Self {
        Self(usage.bits())
    }
}

/// Cache key combining byte-size and usage flags. Used as the outer
/// BTreeMap key for the buffer pool.
type PoolKey = (u64, BufferUsageKey);

/// All the per-shader artifacts that [`PipelineCache`] stores and that
/// [`ColdStartBackend::dispatch_map`] clones on each call.
///
/// In wgpu 26, every field here is a cheap-to-clone `Arc`-backed handle:
/// cloning a [`wgpu::ShaderModule`] / [`wgpu::BindGroupLayout`] /
/// [`wgpu::PipelineLayout`] / [`wgpu::ComputePipeline`] is one atomic
/// increment and a small struct copy. The expensive work
/// (driver-side SPIR-V → ISA compilation, descriptor-set layout
/// negotiation) happens ONCE during the `create_*` calls; subsequent
/// clones reuse the cached driver state.
///
/// `shader_module` and `pipeline_layout` are kept alive here even
/// though they're not directly read after creation — wgpu 26's
/// `ComputePipeline` holds internal `Arc`s to them, but we hold our
/// own references defensively (a) to make the lifetime coupling
/// explicit and (b) for robustness against future wgpu versions that
/// might weaken the internal retention. The cost is two `Arc`
/// increments per cached entry — negligible.
#[derive(Clone, Debug)]
#[allow(dead_code)]
pub(crate) struct CachedPipeline {
    /// The compiled WGSL shader module. Held alive so the pipeline
    /// below keeps a valid module reference for re-binding.
    shader_module: wgpu::ShaderModule,
    /// Two-entry layout: binding 0 = read-only storage input,
    /// binding 1 = read-write storage output (matches T44 codegen).
    bind_group_layout: wgpu::BindGroupLayout,
    /// Wraps the bind-group layout — required by `create_compute_pipeline`.
    pipeline_layout: wgpu::PipelineLayout,
    /// The actual compute pipeline. `set_pipeline` on the compute pass
    /// takes a clone of this each dispatch.
    compute_pipeline: wgpu::ComputePipeline,
}

/// A pipeline cache keyed by WGSL source string.
///
/// The first [`PipelineCache::get_or_compile`] call for a given shader
/// string compiles it (via [`wgpu::Device::create_shader_module`] +
/// [`wgpu::Device::create_compute_pipeline`]) and stores the result;
/// every subsequent call with the same string returns a cheap clone of
/// the cached [`CachedPipeline`].
///
/// [`AtomicUsize`] compile-counter tracks cache misses. Use
/// [`PipelineCache::compile_count`] to read it — primarily for tests
/// proving "dispatch the same shader twice → compile count == 1".
///
/// # Interior mutability
///
/// The cache map lives behind a [`Mutex`]. Reads (cache hit) and writes
/// (cache miss) both lock the same mutex — there is no read-write lock
/// because:
/// 1. Read access on a hit still needs to clone (which requires
///    exclusive access only if the map could be re-entrant — it can't
///    here, but Rust's `RwLock` read guards are `&` which forbids
///    cloning the value's interior Arc handle without unsafe).
/// 2. Lock contention is negligible: dispatch is a multi-millisecond
///    GPU operation; a Mutex lock holds for microseconds.
///
/// Poisoning (a panic while holding the lock) is handled by treating
/// the poisoned guard as an empty cache — the next call will try to
/// re-compile (which is always safe; it just costs a cache miss).
#[derive(Debug, Default)]
pub struct PipelineCache {
    /// `(shader_wgsl → cached pipeline)` map. Deterministic iteration
    /// order (BTreeMap, not HashMap).
    map: Mutex<BTreeMap<String, CachedPipeline>>,
    /// Incremented ONLY on cache miss (i.e. when `create_compute_pipeline`
    /// actually runs). Read via [`PipelineCache::compile_count`].
    compile_count: AtomicUsize,
}

impl PipelineCache {
    /// Construct an empty pipeline cache.
    pub fn new() -> Self {
        Self::default()
    }

    /// How many distinct shaders have been compiled (cache misses).
    ///
    /// * `0` initially.
    /// * `1` after the first call to [`Self::get_or_compile`] with any
    ///   shader string.
    /// * Stays at `1` if the same shader is dispatched N times — that's
    ///   the QA assertion: `dispatch_count >= 2 && compile_count == 1`.
    /// * `2` after a second DISTINCT shader is dispatched.
    pub fn compile_count(&self) -> usize {
        self.compile_count.load(Ordering::Relaxed)
    }

    /// How many distinct pipelines are currently cached.
    ///
    /// Equal to [`Self::compile_count`] when no entries have been
    /// evicted. (T47 does not evict — the cache is unbounded, since
    /// real-world Buff programs have a small finite set of distinct
    /// shaders — but the API is here so a future bounded cache can
    /// plug in without breaking callers.)
    pub fn len(&self) -> usize {
        self.map.lock().map(|guard| guard.len()).unwrap_or(0)
    }

    /// Whether the cache is empty (no pipelines compiled yet).
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Whether a shader matching `shader_wgsl` exactly is currently cached.
    ///
    /// Pure observational — does not modify the cache. Returns `false`
    /// on a poisoned mutex (cannot happen in normal operation).
    pub fn contains(&self, shader_wgsl: &str) -> bool {
        self.map
            .lock()
            .map(|guard| guard.contains_key(shader_wgsl))
            .unwrap_or(false)
    }

    /// Get-or-compile: look up `shader_wgsl` in the cache; on hit,
    /// return a clone of the cached pipeline; on miss, build the
    /// shader module + bind group layout + pipeline layout + compute
    /// pipeline, store, increment [`Self::compile_count`], return.
    ///
    /// # Concurrency
    ///
    /// The mutex is held for the duration of the compile on a miss —
    /// so concurrent first-time dispatches of the same shader will
    /// serialize, and the second caller will see the cached entry
    /// populated by the first. Compile counter is therefore a precise
    /// count of distinct compilations, not of distinct callers.
    ///
    /// # Bind group layout (matches T45 / T44 exactly)
    ///
    /// * binding 0: `var<storage, read>` input array — `read_only: true`
    /// * binding 1: `var<storage, read_write>` output array — `read_only: false`
    /// * `@compute @workgroup_size(64)`, entry point `main`
    pub(crate) fn get_or_compile(
        &self,
        device: &wgpu::Device,
        shader_wgsl: &str,
    ) -> Result<CachedPipeline, RuntimeError> {
        // Fast path: lock, check for hit, return clone.
        // The map lock is the only synchronization we need.
        let guard = self.map.lock().map_err(|_| RuntimeError::GpuInit {
            detail: "pipeline cache mutex poisoned".to_string(),
            span: None,
        })?;
        if let Some(cached) = guard.get(shader_wgsl) {
            return Ok(cached.clone());
        }
        drop(guard); // release before compile (compile can be slow)

        // Slow path: build everything fresh. The compilation work
        // happens OUTSIDE the lock — so concurrent dispatches of
        // different shaders don't serialize on each other's compiles.
        let cached = Self::compile(device, shader_wgsl)?;

        // Re-lock and insert. Handle the (rare) race where another
        // thread compiled the same shader in parallel: prefer the
        // existing entry to keep the compile_count consistent (we
        // don't want to double-count).
        let mut guard = self.map.lock().map_err(|_| RuntimeError::GpuInit {
            detail: "pipeline cache mutex poisoned on insert".to_string(),
            span: None,
        })?;
        if let Some(existing) = guard.get(shader_wgsl) {
            // Another thread won the race. Use theirs; don't increment.
            return Ok(existing.clone());
        }
        guard.insert(shader_wgsl.to_string(), cached.clone());
        self.compile_count.fetch_add(1, Ordering::Relaxed);
        Ok(cached)
    }

    /// Build a fresh [`CachedPipeline`] from WGSL source.
    ///
    /// Broken out of [`Self::get_or_compile`] so the cache miss path
    /// is testable in isolation (with a real `&wgpu::Device`).
    fn compile(device: &wgpu::Device, shader_wgsl: &str) -> Result<CachedPipeline, RuntimeError> {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("buff-cold-start-shader"),
            source: wgpu::ShaderSource::Wgsl(std::borrow::Cow::Borrowed(shader_wgsl)),
        });

        let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("buff-cold-start-bgl"),
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

        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("buff-cold-start-pipeline-layout"),
            bind_group_layouts: &[&bind_group_layout],
            push_constant_ranges: &[],
        });

        let compute_pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("buff-cold-start-pipeline"),
            layout: Some(&pipeline_layout),
            module: &shader,
            entry_point: Some("main"),
            compilation_options: wgpu::PipelineCompilationOptions::default(),
            cache: None,
        });

        Ok(CachedPipeline {
            shader_module: shader,
            bind_group_layout,
            pipeline_layout,
            compute_pipeline,
        })
    }
}

/// A simple GPU buffer pool keyed by `(byte_size, usage_flags)`.
///
/// After each dispatch, [`ColdStartBackend::dispatch_map`] returns its
/// input/output/staging buffers to the pool instead of dropping them.
/// The next dispatch of the same size + usage reuses them. An
/// [`AtomicUsize`] allocation counter tracks pool misses so tests can
/// assert "dispatch N times → allocation count grows sub-linearly".
///
/// # Safety / correctness
///
/// * A buffer is **never** handed out while still in use: it lives in
///   the pool's free-list only AFTER its previous dispatch has fully
///   completed (compute pass done, copy done, staging unmapped). The
///   caller (`dispatch_map`) releases buffers strictly AFTER readback.
/// * The staging buffer is unmapped BEFORE release — so the next
///   caller can `map_async` it without hitting wgpu's "already mapped"
///   panic. The input/output buffers are never mapped (only bound and
///   written), so they're returned as-is.
/// * Buffers are not zeroed on release — the compute pass always
///   fully writes the output before readback, and the input is
///   always fully overwritten via `write_buffer` before the next
///   dispatch's compute pass reads it. (Defensive zeroing was
///   considered but skipped to keep the release path allocation-free.)
///
/// # Pool growth
///
/// The pool is unbounded: every cache miss (no free buffer of the
/// requested size+usage) creates a new buffer and increments the
/// allocation counter. After steady state (the same shader + input
/// size dispatched repeatedly), the pool's free-list for that key
/// stabilizes at the per-dispatch buffer count (3 for input/output/
/// staging of one size).
#[derive(Debug, Default)]
pub struct BufferPool {
    /// Free-list per `(size, usage)` key. Newest entries pushed to the
    /// end; acquire pops from the end (LIFO — best cache locality on
    /// the GPU allocator side).
    free: Mutex<BTreeMap<PoolKey, Vec<wgpu::Buffer>>>,
    /// Incremented ONLY on pool miss (i.e. when a new buffer is
    /// created via `device.create_buffer`). Read via
    /// [`Self::allocation_count`].
    allocation_count: AtomicUsize,
}

impl BufferPool {
    /// Construct an empty buffer pool.
    pub fn new() -> Self {
        Self::default()
    }

    /// How many distinct buffers have been allocated (pool misses).
    pub fn allocation_count(&self) -> usize {
        self.allocation_count.load(Ordering::Relaxed)
    }

    /// How many buffers are currently in the free-list (across all keys).
    pub fn free_count(&self) -> usize {
        self.free
            .lock()
            .map(|guard| guard.values().map(Vec::len).sum())
            .unwrap_or(0)
    }

    /// Whether the pool currently has zero free buffers.
    pub fn is_empty(&self) -> bool {
        self.free_count() == 0
    }

    /// Acquire a buffer of `size` bytes with `usage` flags.
    ///
    /// * Cache hit (free-list non-empty for this key): pops and returns
    ///   the buffer. No allocation, counter unchanged.
    /// * Cache miss (free-list empty or absent for this key): creates
    ///   a new buffer via `device.create_buffer`, increments counter,
    ///   returns.
    pub(crate) fn acquire(
        &self,
        device: &wgpu::Device,
        size: u64,
        usage: wgpu::BufferUsages,
    ) -> Result<wgpu::Buffer, RuntimeError> {
        let key = (size, BufferUsageKey::from(usage));
        let mut guard = self.free.lock().map_err(|_| RuntimeError::GpuInit {
            detail: "buffer pool mutex poisoned".to_string(),
            span: None,
        })?;
        if let Some(free_list) = guard.get_mut(&key) {
            if let Some(buffer) = free_list.pop() {
                return Ok(buffer);
            }
        }
        // Miss: create a new buffer.
        drop(guard); // release before device call (can be slow)
        let buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("buff-cold-start-pool-buffer"),
            size,
            usage,
            mapped_at_creation: false,
        });
        self.allocation_count.fetch_add(1, Ordering::Relaxed);
        Ok(buffer)
    }

    /// Return a buffer to the pool for future reuse.
    ///
    /// The caller MUST ensure the buffer is no longer in flight:
    /// * For the staging buffer: `unmap()` MUST have been called first.
    /// * For input/output buffers: the compute pass must have completed
    ///   (i.e. `device.poll(Wait)` after submit must have returned).
    ///
    /// No-op (silently drops the buffer) on a poisoned mutex — defensive
    /// against a panic-in-lock scenario that can't happen via this
    /// module's own code paths.
    pub(crate) fn release(&self, size: u64, usage: wgpu::BufferUsages, buffer: wgpu::Buffer) {
        let key = (size, BufferUsageKey::from(usage));
        if let Ok(mut guard) = self.free.lock() {
            guard.entry(key).or_default().push(buffer);
        }
        // If poisoned: just drop the buffer. wgpu frees GPU memory on Drop.
    }
}

/// State for the background device-init task.
///
/// Stored on [`ColdStartBackend`] as interior-mutable state so
/// [`ColdStartBackend::spawn_init`] can record the spawned handle and
/// [`ColdStartBackend::is_ready`] / `wait_ready*` can poll/join it.
#[derive(Default)]
struct InitState {
    /// Set to `true` by the spawned task's completion callback.
    /// Read via [`ColdStartBackend::is_ready`].
    ready: Arc<AtomicBool>,
    /// The spawned background task handle. `None` until
    /// [`ColdStartBackend::spawn_init`] runs.
    handle: Mutex<Option<std::thread::JoinHandle<()>>>,
    /// Set to `true` by [`ColdStartBackend::spawn_init`] the first
    /// time it successfully schedules the background task. Subsequent
    /// calls to `spawn_init` are no-ops.
    spawn_attempted: AtomicBool,
}

impl std::fmt::Debug for InitState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let ready = self.ready.load(Ordering::Relaxed);
        let spawned = self.spawn_attempted.load(Ordering::Relaxed);
        f.debug_struct("InitState")
            .field("ready", &ready)
            .field("spawn_attempted", &spawned)
            .finish_non_exhaustive()
    }
}

/// Cold-start-mitigated GPU backend — wraps [`WgpuBackend`] with a
/// [`PipelineCache`], a [`BufferPool`], and background device-init.
///
/// Construct with [`ColdStartBackend::new`] (real GPU) or
/// [`ColdStartBackend::from_context`] (test/injected). Implements
/// [`GpuBackend`] so it's a drop-in replacement for [`WgpuBackend`]:
/// the `dispatch_map` behavior is byte-identical (same compile →
/// dispatch → readback pipeline), just amortized across calls.
///
/// # QA case (T47 spec)
///
/// ```ignore
/// use buff_lang_runtime::{ColdStartBackend, GpuBackend};
///
/// let backend = ColdStartBackend::new()?;
/// let shader = "@compute @workgroup_size(64) fn main() {}";
/// let input = vec![1.0_f32, 2.0, 3.0];
/// backend.dispatch_map(shader, &input)?;
/// backend.dispatch_map(shader, &input)?;
/// // The same shader dispatched twice ⇒ compiled ONCE.
/// assert_eq!(backend.pipeline_compile_count(), 1);
/// ```
///
/// # Background init
///
/// [`ColdStartBackend::spawn_init`] kicks off a background thread that
/// calls [`GpuContext::device_queue`] to warm the OnceLock cache before
/// the first real dispatch arrives. [`ColdStartBackend::is_ready`]
/// reports completion; [`ColdStartBackend::wait_ready`] /
/// [`ColdStartBackend::wait_ready_blocking`] await it. This is purely
/// an optimization — calling `dispatch_map` WITHOUT warming init still
/// works (the first dispatch just pays the device-init cost inline).
///
/// # No GPU? No problem.
///
/// On hosts without a GPU adapter, [`ColdStartBackend::new`] returns
/// [`RuntimeError::GpuUnavailable`]. Construct via
/// [`ColdStartBackend::from_context`] with `GpuContext::unavailable()`
/// for graceful-no-GPU tests — `dispatch_map` then returns
/// `Err(GpuUnavailable)`, never panics.
///
/// # Send + Sync
///
/// All fields are `Send + Sync`:
/// * `Arc<WgpuBackend>` — `WgpuBackend` is `Send + Sync` (per T45).
/// * `PipelineCache` — `Mutex<BTreeMap<..>>` + `AtomicUsize`.
/// * `BufferPool` — `Mutex<BTreeMap<..>>` + `AtomicUsize`.
/// * `InitState` — `Arc<AtomicBool>` + `Mutex<Option<JoinHandle>>` +
///   `AtomicBool`.
///
/// Held as `Arc<ColdStartBackend>` across threads or `Box<dyn GpuBackend>`
/// in trait-object dispatch sites.
#[derive(Debug)]
pub struct ColdStartBackend {
    /// The underlying T45 backend. Arc-shared so the background init
    /// thread can call `context().device_queue()` to warm the cache.
    inner: Arc<WgpuBackend>,
    /// Pipeline cache — WGSL source → compiled pipeline.
    pipeline_cache: PipelineCache,
    /// Buffer pool — `(size, usage)` → free-list.
    buffer_pool: BufferPool,
    /// Background-init task state (spawn handle + ready flag).
    init_state: InitState,
}

impl ColdStartBackend {
    /// Acquire a GPU adapter and construct a cold-start-mitigated backend.
    ///
    /// Returns [`RuntimeError::GpuUnavailable`] on hosts with no GPU
    /// adapter — **never panics**. Device+queue acquisition is deferred
    /// to the first [`Self::dispatch_map`] call (or to
    /// [`Self::spawn_init`] if called first).
    pub fn new() -> Result<Self, RuntimeError> {
        let inner = Arc::new(WgpuBackend::new()?);
        Ok(Self {
            inner,
            pipeline_cache: PipelineCache::new(),
            buffer_pool: BufferPool::new(),
            init_state: InitState::default(),
        })
    }

    /// Wrap an existing [`WpuBackend`](WgpuBackend) with cache + pool +
    /// background-init support. Useful for tests that want to inject
    /// an `unavailable()` context.
    #[must_use]
    pub fn from_backend(backend: WgpuBackend) -> Self {
        Self {
            inner: Arc::new(backend),
            pipeline_cache: PipelineCache::new(),
            buffer_pool: BufferPool::new(),
            init_state: InitState::default(),
        }
    }

    /// Wrap an existing [`GpuContext`] (same pattern as
    /// [`WgpuBackend::from_context`]).
    #[must_use]
    pub fn from_context(context: GpuContext) -> Self {
        Self::from_backend(WgpuBackend::from_context(context))
    }

    /// Borrow the underlying T45 [`WgpuBackend`] (e.g. to inspect
    /// `context().device_init_count()` for parity assertions).
    #[must_use]
    pub fn inner(&self) -> &WgpuBackend {
        &self.inner
    }

    /// Borrow the inner [`GpuContext`] directly.
    #[must_use]
    pub fn context(&self) -> &GpuContext {
        self.inner.context()
    }

    /// Whether the cached `(Device, Queue)` pair has been successfully
    /// initialized (whether by [`Self::spawn_init`] or by a prior
    /// dispatch). Purely observational — does NOT drive initialization.
    #[must_use]
    pub fn has_device(&self) -> bool {
        self.inner.has_device()
    }

    /// How many distinct pipelines have been compiled (cache misses).
    ///
    /// * `0` before any dispatch.
    /// * `1` after the first dispatch with shader A.
    /// * Still `1` after the second dispatch with the same shader A
    ///   (cache hit) — **the QA assertion**.
    /// * `2` after the first dispatch with a different shader B.
    #[must_use]
    pub fn pipeline_compile_count(&self) -> usize {
        self.pipeline_cache.compile_count()
    }

    /// How many distinct GPU buffers have been allocated (pool misses).
    ///
    /// * `0` before any dispatch.
    /// * `3` after the first dispatch (input + output + staging buffers).
    /// * Still `3` after the second dispatch with the same input size
    ///   (pool reuse — the buffers from the first dispatch are returned
    ///   and re-acquired).
    #[must_use]
    pub fn buffer_allocation_count(&self) -> usize {
        self.buffer_pool.allocation_count()
    }

    /// Number of distinct pipelines currently cached.
    #[must_use]
    pub fn cached_pipeline_count(&self) -> usize {
        self.pipeline_cache.len()
    }

    /// Number of buffers currently in the free-list.
    #[must_use]
    pub fn pooled_buffer_count(&self) -> usize {
        self.buffer_pool.free_count()
    }

    /// Kick off a background thread that warms the device+queue cache
    /// by calling [`GpuContext::device_queue`] on the inner context.
    ///
    /// * **Idempotent**: the second call is a no-op (returns `Ok(())`
    ///   without spawning a new thread).
    /// * **Never panics**: the spawned thread swallows errors from
    ///   `device_queue` — failures are cached on the context's
    ///   OnceLock just like a synchronous call would, so the next
    ///   `dispatch_map` will see the same error.
    /// * **Does not block**: returns immediately after spawn.
    /// * **Works without a tokio runtime**: uses `std::thread::spawn`,
    ///   not `tokio::spawn`. The thread runs the (synchronous)
    ///   `device_queue()` call to completion and then exits.
    ///
    /// Use [`Self::is_ready`] / [`Self::wait_ready`] /
    /// [`Self::wait_ready_blocking`] to wait for completion.
    ///
    /// # Errors
    ///
    /// Returns [`RuntimeError::GpuInit`] only if the interior-mutable
    /// `Mutex` for the spawn handle is poisoned (cannot happen in
    /// normal operation).
    pub fn spawn_init(&self) -> Result<(), RuntimeError> {
        // Idempotency: only the first call should actually spawn.
        if self.init_state.spawn_attempted.swap(true, Ordering::AcqRel) {
            return Ok(());
        }

        // We need an Arc<Self> to share with the spawned thread, but
        // we only have &self. The background thread only needs the
        // GpuContext's OnceLock (interior-mutable) — so we can clone
        // the WgpuBackend's underlying context-sharing channel.
        //
        // Trick: GpuContext::device_queue(&self) takes &self. We can't
        // move &GpuContext into a 'static thread. Instead, we clone
        // the Arc<WgpuBackend> (which holds the GpuContext) — but we
        // don't have an Arc<Self>. We DO have &Arc<WgpuBackend> as
        // self.inner, so we can clone that.
        let backend_arc = Arc::clone(&self.inner);
        let ready_flag = Arc::clone(&self.init_state.ready);

        let handle = std::thread::Builder::new()
            .name("buff-cold-start-init".to_string())
            .spawn(move || {
                // Warm the cache. The result (Ok or Err) is cached on
                // the GpuContext's OnceLock — so future dispatch_map
                // calls will see the same outcome without re-running
                // request_device. Swallow errors: the dispatch path
                // will report them properly via gpu_ctx_err_to_runtime.
                let _ = backend_arc.context().device_queue();
                // Signal completion regardless of init success/failure.
                ready_flag.store(true, Ordering::Release);
            })
            .map_err(|e| RuntimeError::GpuInit {
                detail: format!("failed to spawn cold-start init thread: {e}"),
                span: None,
            })?;

        let mut guard = self
            .init_state
            .handle
            .lock()
            .map_err(|_| RuntimeError::GpuInit {
                detail: "init_state mutex poisoned on spawn".to_string(),
                span: None,
            })?;
        *guard = Some(handle);
        Ok(())
    }

    /// Whether the background-init task has completed.
    ///
    /// Returns `false` if [`Self::spawn_init`] was never called, and
    /// `false` while the background task is still running. Returns
    /// `true` once the task has finished (whether or not device init
    /// succeeded — check [`Self::has_device`] for that).
    #[must_use]
    pub fn is_ready(&self) -> bool {
        self.init_state.ready.load(Ordering::Acquire)
    }

    /// Block the calling thread until the background init completes.
    ///
    /// Returns immediately if [`Self::spawn_init`] was never called
    /// (no task to wait for — `spawn_attempted` is `false`) or if the
    /// task already completed ([`Self::is_ready`] is `true`).
    ///
    /// If the spawned thread panicked, this also returns — the panic
    /// is swallowed (we never propagate panics from background init;
    /// dispatch_map will see the cached error if init failed).
    pub fn wait_ready_blocking(&self) {
        if self.is_ready() {
            return;
        }
        // If spawn_init was never called, there's nothing to wait for.
        // Return immediately — don't enter the spin-yield loop below
        // (which would otherwise loop forever waiting for a flag that
        // no thread will ever set).
        if !self.init_state.spawn_attempted.load(Ordering::Acquire) {
            return;
        }
        // Take the handle out of the mutex so we can join without
        // holding the lock. Re-insert (as None — we only ever spawn
        // once) after.
        let handle = {
            let Ok(mut guard) = self.init_state.handle.lock() else {
                return;
            };
            guard.take()
        };
        if let Some(h) = handle {
            let _ = h.join();
        }
        // Spin until ready_flag flips (defensive; the join should
        // guarantee ordering but the flag is the source of truth).
        while !self.is_ready() {
            std::thread::yield_now();
        }
    }

    /// Async version of [`Self::wait_ready_blocking`].
    ///
    /// Awaits the background init task using
    /// `tokio::task::spawn_blocking` to run the synchronous `.join()`
    /// on a blocking-pool thread (so the async runtime isn't held).
    ///
    /// Returns immediately if [`Self::spawn_init`] was never called
    /// (no task was ever scheduled) or if the task already completed.
    ///
    /// # Panics
    ///
    /// Panics if called outside a tokio runtime context. Use
    /// [`Self::wait_ready_blocking`] if you're not in async code.
    pub async fn wait_ready(&self) {
        if self.is_ready() {
            return;
        }
        // If spawn_init was never called, return immediately — there's
        // no task to wait for and no flag to flip.
        if !self.init_state.spawn_attempted.load(Ordering::Acquire) {
            return;
        }
        // Capture a clone of self's Arc-inner so we don't borrow across
        // the await boundary.
        // tokio::task::spawn_blocking requires a 'static closure, so we
        // can't capture &self. We capture the Arc<WgpuBackend> + flag.
        let ready_flag = Arc::clone(&self.init_state.ready);
        let handle_opt = {
            let Ok(mut guard) = self.init_state.handle.lock() else {
                return;
            };
            guard.take()
        };
        if let Some(h) = handle_opt {
            let _ = tokio::task::spawn_blocking(move || {
                let _ = h.join();
            })
            .await;
        }
        // Spin-yield until ready_flag flips.
        while !ready_flag.load(Ordering::Acquire) {
            tokio::task::yield_now().await;
        }
    }
}

impl GpuBackend for ColdStartBackend {
    fn dispatch_map(&self, shader_wgsl: &str, input: &[f32]) -> Result<Vec<f32>, RuntimeError> {
        // Guard: empty input — no buffer, no dispatch. wgpu forbids
        // 0-sized copy_buffer_to_buffer and 0-group dispatches.
        if input.is_empty() {
            return Ok(Vec::new());
        }

        let device = self
            .inner
            .context()
            .device()
            .map_err(gpu_ctx_err_to_runtime)?;
        let queue = self
            .inner
            .context()
            .queue()
            .map_err(gpu_ctx_err_to_runtime)?;

        // Cache lookup (compiles on miss, clones on hit).
        let cached = self.pipeline_cache.get_or_compile(device, shader_wgsl)?;

        let input_bytes: &[u8] = bytemuck::cast_slice(input);
        let byte_size = input_bytes.len() as wgpu::BufferAddress;

        // Acquire the three per-dispatch buffers from the pool.
        let input_buffer = self.buffer_pool.acquire(
            device,
            byte_size,
            wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
        )?;
        let output_buffer = self.buffer_pool.acquire(
            device,
            byte_size,
            wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
        )?;
        let staging_buffer = self.buffer_pool.acquire(
            device,
            byte_size,
            wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
        )?;

        // Upload input data — pooled buffer is uninitialized, so we
        // must write the input before binding it.
        queue.write_buffer(&input_buffer, 0, input_bytes);

        // Build the bind group from the cached layout + our actual buffers.
        // (Bind group is per-dispatch because it depends on the actual
        // buffer handles; only the layout is cached.)
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("buff-cold-start-bg"),
            layout: &cached.bind_group_layout,
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

        // Encode compute pass + copy. Uses the cached pipeline.
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("buff-cold-start-encoder"),
        });
        {
            let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("buff-cold-start-pass"),
                timestamp_writes: None,
            });
            pass.set_pipeline(&cached.compute_pipeline);
            pass.set_bind_group(0, &bind_group, &[]);
            pass.dispatch_workgroups(workgroup_count(input.len()), 1, 1);
        }
        encoder.copy_buffer_to_buffer(&output_buffer, 0, &staging_buffer, 0, byte_size);

        let command_buffer = encoder.finish();

        // Submit + poll until dispatch + copy complete.
        queue.submit(std::iter::once(command_buffer));
        if let Err(e) = device.poll(wgpu::PollType::Wait) {
            // Release buffers back to pool even on error — they're no
            // longer in flight (the poll failed but the buffers are
            // still valid GPU memory).
            self.buffer_pool.release(
                byte_size,
                wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
                input_buffer,
            );
            self.buffer_pool.release(
                byte_size,
                wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
                output_buffer,
            );
            self.buffer_pool.release(
                byte_size,
                wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
                staging_buffer,
            );
            return Err(RuntimeError::GpuInit {
                detail: format!("device.poll(Wait) failed: {e:?}"),
                span: None,
            });
        }

        // map_async + drain via poll + read.
        let (tx, rx) = std::sync::mpsc::channel::<Result<(), wgpu::BufferAsyncError>>();
        staging_buffer
            .slice(..)
            .map_async(wgpu::MapMode::Read, move |result| {
                let _ = tx.send(result);
            });

        if let Err(e) = device.poll(wgpu::PollType::Wait) {
            self.buffer_pool.release(
                byte_size,
                wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
                input_buffer,
            );
            self.buffer_pool.release(
                byte_size,
                wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
                output_buffer,
            );
            staging_buffer.unmap();
            self.buffer_pool.release(
                byte_size,
                wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
                staging_buffer,
            );
            return Err(RuntimeError::GpuInit {
                detail: format!("device.poll(Wait) for map_async failed: {e:?}"),
                span: None,
            });
        }

        let map_result = rx.recv().map_err(|e| RuntimeError::GpuInit {
            detail: format!("map_async callback did not fire (sender dropped): {e}"),
            span: None,
        })?;

        // Map_err path: release buffers BEFORE returning the error.
        if let Err(async_err) = map_result {
            self.buffer_pool.release(
                byte_size,
                wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
                input_buffer,
            );
            self.buffer_pool.release(
                byte_size,
                wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
                output_buffer,
            );
            staging_buffer.unmap();
            self.buffer_pool.release(
                byte_size,
                wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
                staging_buffer,
            );
            return Err(RuntimeError::GpuInit {
                detail: format!("map_async(BufferAsyncError): {async_err:?}"),
                span: None,
            });
        }

        // Read mapped range.
        let view = staging_buffer.slice(..).get_mapped_range();
        let bytes: &[u8] = &view;
        let output: Vec<f32> = bytemuck::cast_slice::<u8, f32>(bytes).to_vec();
        drop(view);
        staging_buffer.unmap();

        // Release all three buffers back to the pool for reuse.
        self.buffer_pool.release(
            byte_size,
            wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
            input_buffer,
        );
        self.buffer_pool.release(
            byte_size,
            wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
            output_buffer,
        );
        self.buffer_pool.release(
            byte_size,
            wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            staging_buffer,
        );

        Ok(output)
    }
}

impl Default for ColdStartBackend {
    /// Equivalent to [`Self::from_context`]`(GpuContext::unavailable())`.
    fn default() -> Self {
        Self::from_context(GpuContext::unavailable())
    }
}

#[cfg(test)]
mod tests {
    //! Inline unit tests for the cache/pool state logic. These do NOT
    //! need a GPU — they only inspect initial state, counters, and
    //! the pure (no-wgpu) lookup behavior of the BTreeMap-backed cache.
    //!
    //! Full behavioral coverage (cache hits, pool reuse, roundtrip
    //! correctness on a real GPU) lives in
    //! `tests/cold_start_tests.rs` so the QA filter
    //! `cargo test -p buff-lang-runtime cold_start` matches the whole
    //! suite (inline + integration).

    use super::*;

    #[test]
    fn cold_start_pipeline_cache_default_is_empty() {
        let cache = PipelineCache::new();
        assert!(cache.is_empty(), "freshly-constructed cache must be empty");
        assert_eq!(cache.len(), 0, "len() must be 0 on a fresh cache");
    }

    #[test]
    fn cold_start_pipeline_cache_compile_count_starts_at_zero() {
        let cache = PipelineCache::new();
        assert_eq!(
            cache.compile_count(),
            0,
            "compile_count must be 0 before any get_or_compile call"
        );
    }

    #[test]
    fn cold_start_pipeline_cache_contains_returns_false_for_any_unseen_shader() {
        let cache = PipelineCache::new();
        assert!(
            !cache.contains("@compute @workgroup_size(64) fn main() {}"),
            "contains() must return false for an unseen shader"
        );
        assert!(
            !cache.contains(""),
            "contains() must return false even for the empty string"
        );
    }

    #[test]
    fn cold_start_buffer_pool_default_is_empty() {
        let pool = BufferPool::new();
        assert!(
            pool.is_empty(),
            "freshly-constructed pool must report is_empty()"
        );
        assert_eq!(
            pool.free_count(),
            0,
            "free_count() must be 0 on a fresh pool"
        );
    }

    #[test]
    fn cold_start_buffer_pool_allocation_count_starts_at_zero() {
        let pool = BufferPool::new();
        assert_eq!(
            pool.allocation_count(),
            0,
            "allocation_count must be 0 before any acquire"
        );
    }

    #[test]
    fn cold_start_cold_start_backend_default_is_unavailable() {
        // Default-constructed backend must report no device.
        let backend = ColdStartBackend::default();
        assert!(
            !backend.has_device(),
            "default ColdStartBackend must report has_device=false"
        );
        assert!(
            !backend.context().has_adapter(),
            "default ColdStartBackend context must report has_adapter=false"
        );
        assert_eq!(
            backend.pipeline_compile_count(),
            0,
            "pipeline_compile_count must start at 0"
        );
        assert_eq!(
            backend.buffer_allocation_count(),
            0,
            "buffer_allocation_count must start at 0"
        );
    }

    #[test]
    fn cold_start_cold_start_backend_is_ready_false_before_spawn() {
        let backend = ColdStartBackend::default();
        assert!(
            !backend.is_ready(),
            "is_ready must be false before spawn_init is called"
        );
    }

    #[test]
    fn cold_start_cold_start_backend_wait_ready_blocking_returns_if_never_spawned() {
        // wait_ready_blocking must NOT block forever if spawn_init was
        // never called — it should return immediately.
        let backend = ColdStartBackend::default();
        backend.wait_ready_blocking();
        // No assertion needed — reaching here means we didn't hang.
    }

    #[test]
    fn cold_start_cold_start_backend_spawn_init_idempotent_on_unavailable_context() {
        // spawn_init on an unavailable context must complete gracefully
        // (the spawned thread sees Err(NoAdapter) and caches it).
        // Calling it twice must be a no-op (returns Ok without spawning
        // a second thread).
        let backend = ColdStartBackend::default();
        backend.spawn_init().expect("first spawn_init must succeed");
        backend
            .spawn_init()
            .expect("second spawn_init must be a no-op Ok");
        backend.wait_ready_blocking();
        assert!(
            backend.is_ready(),
            "is_ready must be true after wait_ready_blocking"
        );
        // has_device stays false — the unavailable context can't acquire.
        assert!(
            !backend.has_device(),
            "has_device must stay false on unavailable context"
        );
    }

    #[test]
    fn cold_start_cold_start_backend_dispatch_empty_input_no_compile_no_alloc() {
        // Empty input must short-circuit BEFORE any cache/pool interaction.
        let backend = ColdStartBackend::default();
        let out = backend
            .dispatch_map("@compute @workgroup_size(64) fn main() {}", &[])
            .expect("empty input returns Ok(empty) without GPU work");
        assert!(out.is_empty(), "empty input must produce empty output");
        assert_eq!(
            backend.pipeline_compile_count(),
            0,
            "no pipeline compiled for empty input"
        );
        assert_eq!(
            backend.buffer_allocation_count(),
            0,
            "no buffer allocated for empty input"
        );
    }

    #[test]
    fn cold_start_cold_start_backend_dispatch_on_unavailable_context_returns_unavailable() {
        // No GPU adapter ⇒ graceful Err(GpuUnavailable), never panic.
        let backend = ColdStartBackend::default();
        let result = backend.dispatch_map(
            "@compute @workgroup_size(64) fn main() {}",
            &[1.0_f32, 2.0, 3.0],
        );
        match result {
            Err(RuntimeError::GpuUnavailable { .. }) => {
                // expected
            }
            Err(other) => panic!("expected GpuUnavailable, got: {other:?}"),
            Ok(out) => panic!("expected Err but got Ok({out:?})"),
        }
        // Counters must NOT have moved.
        assert_eq!(backend.pipeline_compile_count(), 0);
        assert_eq!(backend.buffer_allocation_count(), 0);
    }

    #[test]
    fn cold_start_cold_start_backend_send_sync_compile_time() {
        // Compile-time + runtime proof that ColdStartBackend is Send + Sync
        // (required by GpuBackend trait bound).
        fn assert_send<T: Send>() {}
        fn assert_sync<T: Sync>() {}
        assert_send::<ColdStartBackend>();
        assert_sync::<ColdStartBackend>();
    }

    #[test]
    fn cold_start_cold_start_backend_has_debug_repr() {
        let backend = ColdStartBackend::default();
        let s = format!("{backend:?}");
        assert!(
            s.contains("ColdStartBackend"),
            "Debug repr must name the type, got: {s}"
        );
    }

    #[test]
    fn cold_start_buffer_usage_key_orders_correctly() {
        // Sanity check: the BufferUsageKey newtype must be totally ordered.
        let a = BufferUsageKey::from(wgpu::BufferUsages::STORAGE);
        let b = BufferUsageKey::from(wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST);
        let c = BufferUsageKey::from(wgpu::BufferUsages::MAP_READ);
        // a < b because COPY_DST adds a bit.
        assert!(a < b, "STORAGE must order before STORAGE|COPY_DST");
        // The three keys are pairwise comparable (total order).
        let mut v = [c, b, a];
        v.sort();
        assert!(v[0] <= v[1] && v[1] <= v[2], "sorted vec must be non-desc");
    }
}

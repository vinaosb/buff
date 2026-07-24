//! GPU context — a handle to a wgpu adapter (T38) and a lazily-initialized,
//! cached `(Device, Queue)` pair (T43).
//!
//! T38 acquires an adapter only, via [`GpuContext::new`]. The constructor
//! is **synchronous**: it drives the async `request_adapter` future to
//! completion with `pollster::block_on`, so callers never see `async`.
//!
//! T43 layers device+queue acquisition on top, via [`GpuContext::device_queue`].
//! The first call drives `adapter.request_device` to completion (again with
//! `pollster::block_on`) and caches the result in a [`std::sync::OnceLock`].
//! Subsequent calls return the SAME cached pair (verified by
//! [`GpuContext::device_init_count`] staying at 1). Failures are also cached:
//! a host that fails once keeps failing fast, never re-running the request
//! future (which would also panic per wgpu docs).
//!
//! T44/T45 will add WGSL shader dispatch and buffer readback on top of the
//! cached device+queue.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::OnceLock;

use crate::dispatch::{DispatchKind, Dispatcher};
use crate::error::RuntimeError;

/// Error raised while constructing a [`GpuContext`].
#[derive(Debug, thiserror::Error)]
pub enum GpuContextError {
    /// No GPU adapter is available on this host. The runtime must never
    /// panic on this — callers (T40 thresholds) fall back to CPU.
    #[error("no wgpu adapter available")]
    NoAdapter,

    /// wgpu returned a device-request error. Wired up by T43 (was reserved
    /// by T38). Carries the lower-level wgpu `RequestDeviceError` rendered
    /// as a string so the snapshot stays `Clone`-cheap and deterministic.
    #[error("wgpu device request failed: {0}")]
    DeviceRequest(String),
}

impl From<GpuContextError> for RuntimeError {
    fn from(err: GpuContextError) -> Self {
        match err {
            GpuContextError::NoAdapter => Self::GpuUnavailable { span: None },
            GpuContextError::DeviceRequest(detail) => Self::GpuInit {
                detail,
                span: None,
            },
        }
    }
}

/// Deterministic, `Clone`-able snapshot of the fields we care about from
/// `wgpu::AdapterInfo`. The real `wgpu::AdapterInfo` is not `Clone` across
/// all wgpu versions, and we want cheap copies for diagnostics + tests.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdapterInfoSnapshot {
    /// Human-readable adapter name reported by the driver (may be empty).
    pub name: String,
    /// `Debug`-rendered device type, e.g. `"DiscreteGpu"`, `"IntegratedGpu"`,
    /// `"Cpu"`, `"Other"`. Stored as a string so the snapshot stays `Eq`.
    pub device_type: String,
}

/// Handle to a wgpu adapter (T38) and a lazily-cached `(Device, Queue)` (T43).
///
/// Construct with [`GpuContext::new`] when a GPU is expected, or
/// [`GpuContext::unavailable`] when the caller already knows there is no
/// GPU and wants a placeholder that reports [`DispatchKind::GpuCompute`] as
/// its target backend (used by T40's threshold logic to skip GPU paths).
///
/// # Device + Queue caching (T43)
///
/// [`GpuContext::device_queue`] acquires a `(Device, Queue)` pair from the
/// adapter on first call, via `pollster::block_on(adapter.request_device(...))`.
/// The result — success OR failure — is stored in a [`OnceLock`] so the
/// second and later calls return the SAME cached value without re-running
/// `request_device` (which would panic per wgpu 26 docs: "request_device()
/// was already called on this Adapter"). Tests verify cached-ness via
/// [`GpuContext::device_init_count`] staying at 1 across many calls.
///
/// Not `Clone` — adapter handles are uniquely owned. The device+queue
/// cache uses `OnceLock`, not `Clone`.
#[derive(Debug)]
pub struct GpuContext {
    /// The adapter we picked at construction time. `None` only when the
    /// context was built via [`GpuContext::unavailable`].
    adapter: Option<wgpu::Adapter>,
    /// Snapshot of adapter info captured at construction so tests can
    /// assert deterministically without holding GPU state.
    adapter_info: AdapterInfoSnapshot,
    /// T43: lazily-initialized, CACHED device+queue (or the cached error).
    ///
    /// `OnceLock` is sound here: `wgpu::Device` and `wgpu::Queue` are both
    /// `Send + Sync` (Arc-backed handles in wgpu 26). The closure passed to
    /// `get_or_init` only ever runs once across all threads.
    device_queue_cache: OnceLock<Result<(wgpu::Device, wgpu::Queue), GpuContextError>>,
    /// Diagnostic counter — how many times `acquire_device_queue` ran.
    /// Should be 0 before first call, 1 after the first call (success OR
    /// failure), and stay 1 forever (OnceLock prevents re-init). Read via
    /// [`GpuContext::device_init_count`] — primarily for tests proving
    /// cached-ness.
    device_init_count: AtomicUsize,
}

impl GpuContext {
    /// Acquire a GPU adapter synchronously.
    ///
    /// Returns:
    /// * `Ok(GpuContext)` if a usable adapter was found.
    /// * `Err(GpuContextError::NoAdapter)` if no adapter is available
    ///   (e.g. CI without GPU drivers) — **never panics**.
    ///
    /// `request_adapter` is async; we drive it to completion with
    /// `pollster::block_on` so callers stay on a sync API.
    ///
    /// Device+queue acquisition is deferred to [`Self::device_queue`]
    /// (lazy, cached, OnceLock-backed).
    pub fn new() -> Result<Self, GpuContextError> {
        let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor::default());

        // wgpu 26's `request_adapter` returns `Result<Adapter, RequestAdapterError>`
        // (older versions returned `Option<Adapter>`). Any failure here — most
        // commonly "no suitable adapter" on a host without GPU drivers — is
        // mapped to `NoAdapter` so callers see a single graceful variant.
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::default(),
            force_fallback_adapter: false,
            compatible_surface: None,
        }))
        .map_err(|_| GpuContextError::NoAdapter)?;

        let info = adapter.get_info();
        let snapshot = AdapterInfoSnapshot {
            name: info.name,
            device_type: format!("{:?}", info.device_type),
        };

        Ok(Self {
            adapter: Some(adapter),
            adapter_info: snapshot,
            device_queue_cache: OnceLock::new(),
            device_init_count: AtomicUsize::new(0),
        })
    }

    /// Construct a placeholder context that *claims* no GPU is available.
    ///
    /// Reports [`DispatchKind::GpuCompute`] as its target backend (so T40
    /// threshold logic still routes work towards "GPU if it existed") but
    /// [`Dispatcher::supports_gpu`] returns `false`. Used by T40/T43 when
    /// the host has no GPU drivers at all. Any subsequent call to
    /// [`Self::device_queue`] on this context returns the cached
    /// [`GpuContextError::NoAdapter`] — never panics.
    pub fn unavailable() -> Self {
        Self {
            adapter: None,
            adapter_info: AdapterInfoSnapshot {
                name: String::new(),
                device_type: "None".into(),
            },
            device_queue_cache: OnceLock::new(),
            device_init_count: AtomicUsize::new(0),
        }
    }

    /// Human-readable adapter name (empty string when no adapter).
    pub fn adapter_name(&self) -> &str {
        &self.adapter_info.name
    }

    /// Deterministic snapshot of the adapter info (name + device-type).
    /// Mainly useful for diagnostics and tests.
    pub fn adapter_info(&self) -> &AdapterInfoSnapshot {
        &self.adapter_info
    }

    /// Whether this context actually holds a GPU adapter.
    pub fn has_adapter(&self) -> bool {
        self.adapter.is_some()
    }

    /// Lazily acquire and CACHE the `(Device, Queue)` pair from the adapter.
    ///
    /// * **First call**: drives `adapter.request_device(...)` to completion
    ///   via `pollster::block_on`, caches the result, returns a reference
    ///   into the cache.
    /// * **Subsequent calls**: return the SAME cached result WITHOUT
    ///   re-running `request_device` (which would panic per wgpu 26 docs).
    /// * **No adapter** ([`Self::unavailable`] context): immediately caches
    ///   and returns `Err(GpuContextError::NoAdapter)`. Never panics.
    ///
    /// Failures are cached too — a host that fails once keeps failing fast.
    /// Use [`Self::device_init_count`] to verify cached-ness in tests.
    ///
    /// # wgpu 26 API used
    ///
    /// `adapter.request_device(&DeviceDescriptor)` returns
    /// `impl Future<Output = Result<(Device, Queue), RequestDeviceError>>`.
    /// Single-arg signature in wgpu 26 (no separate trace path param —
    /// `trace` is a field of `DeviceDescriptor`). We use
    /// [`wgpu::DeviceDescriptor::default`] (label=None, no features,
    /// downlevel limits, default memory hints, `Trace::Off`).
    pub fn device_queue(&self) -> Result<&(wgpu::Device, wgpu::Queue), &GpuContextError> {
        let cached = self
            .device_queue_cache
            .get_or_init(|| self.acquire_device_queue());
        cached.as_ref()
    }

    /// Convenience: cached `&Device` handle.
    ///
    /// Same caching semantics as [`Self::device_queue`]: first call drives
    /// the request, subsequent calls return the same cached reference.
    pub fn device(&self) -> Result<&wgpu::Device, &GpuContextError> {
        self.device_queue().map(|(device, _)| device)
    }

    /// Convenience: cached `&Queue` handle.
    ///
    /// Same caching semantics as [`Self::device_queue`].
    pub fn queue(&self) -> Result<&wgpu::Queue, &GpuContextError> {
        self.device_queue().map(|(_, queue)| queue)
    }

    /// Whether the device+queue has been initialized AND acquisition
    /// succeeded. Returns `false` before the first [`Self::device_queue`]
    /// call AND on a context that failed device acquisition. Does NOT
    /// drive initialization — purely observational.
    pub fn has_device(&self) -> bool {
        self.device_queue_cache
            .get()
            .map(|result| result.is_ok())
            .unwrap_or(false)
    }

    /// How many times `acquire_device_queue` has run on this context.
    ///
    /// * `0` before the first [`Self::device_queue`] / [`Self::device`] /
    ///   [`Self::queue`] call.
    /// * `1` after the first call (whether success or failure).
    /// * Stays `1` forever — [`OnceLock`] prevents re-initialization.
    ///
    /// Mainly a diagnostic for tests proving cached-ness (T43 spec:
    /// "Cached on second call").
    pub fn device_init_count(&self) -> usize {
        self.device_init_count.load(Ordering::Relaxed)
    }

    /// Drive `adapter.request_device` to completion synchronously.
    ///
    /// This is the **only** place that touches `self.adapter` for device
    /// purposes, and it is only ever called from inside
    /// `OnceLock::get_or_init`, so it runs at most once per `GpuContext`.
    ///
    /// Returns `Err(NoAdapter)` when the context has no adapter
    /// ([`Self::unavailable`]), or `Err(DeviceRequest(_))` when wgpu
    /// rejects the request. Never panics.
    fn acquire_device_queue(&self) -> Result<(wgpu::Device, wgpu::Queue), GpuContextError> {
        // Record the attempt FIRST so the counter is accurate even if the
        // request itself fails. Relaxed is sufficient: the OnceLock
        // guarantees single execution, so there's no race to worry about.
        self.device_init_count.fetch_add(1, Ordering::Relaxed);

        let adapter = self.adapter.as_ref().ok_or(GpuContextError::NoAdapter)?;

        // wgpu 26 DeviceDescriptor fields:
        //   label: Label<'a> = Option<&'a str>  -> None
        //   required_features: Features        -> Features::empty()
        //   required_limits: Limits            -> Limits::downlevel_defaults()
        //   memory_hints: MemoryHints          -> MemoryHints::default()
        //   trace: Trace                       -> Trace::Off (non_exhaustive, only constructible via Default)
        //
        // Use `DeviceDescriptor::default()` — `Label<'a>` has Default
        // (None), Features/Limits/MemoryHints all have Default, Trace has
        // `#[default] Off`. The default `Limits` are the "downlevel_defaults"
        // which every wgpu-supported device must meet.
        let descriptor = wgpu::DeviceDescriptor::default();

        pollster::block_on(adapter.request_device(&descriptor))
            .map_err(|err| GpuContextError::DeviceRequest(format!("{err:?}")))
    }
}

impl Default for GpuContext {
    /// Equivalent to [`Self::unavailable`] — a no-GPU placeholder.
    ///
    /// Provided so `GpuContext` can be constructed in `const`-ish contexts
    /// and tests where the caller does not yet know whether a GPU is
    /// present. Use [`Self::new`] for real adapter acquisition.
    fn default() -> Self {
        Self::unavailable()
    }
}

impl Dispatcher for GpuContext {
    fn kind(&self) -> DispatchKind {
        DispatchKind::GpuCompute
    }

    fn parallelism(&self) -> usize {
        // T45 will report a meaningful workgroup width once we query
        // `device.limits().max_compute_workgroups_per_dimension`. T43
        // intentionally returns 0: we have a device but have not yet
        // negotiated compute-shader limits (T45's job).
        0
    }

    fn supports_gpu(&self) -> bool {
        self.adapter.is_some()
    }
}

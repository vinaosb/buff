//! GPU context — a handle to a wgpu adapter (and, in T43, a cached device+queue).
//!
//! T38 acquires an adapter only, via [`GpuContext::new`]. The constructor
//! is **synchronous**: it drives the async `request_adapter` future to
//! completion with `pollster::block_on`, so callers never see `async`.
//!
//! T43 will add device+queue via a lazy `OnceLock` cache, and T44/T45 will
//! add shader dispatch and buffer readback.

use crate::dispatch::{DispatchKind, Dispatcher};
use crate::error::RuntimeError;

/// Error raised while constructing a [`GpuContext`].
#[derive(Debug, thiserror::Error)]
pub enum GpuContextError {
    /// No GPU adapter is available on this host. The runtime must never
    /// panic on this — callers (T40 thresholds) fall back to CPU.
    #[error("no wgpu adapter available")]
    NoAdapter,

    /// wgpu returned a device-request error. Reserved for T43; declared
    /// here so T38's `From<GpuContextError> for RuntimeError` is complete.
    #[error("wgpu device request failed: {0}")]
    #[allow(dead_code)] // device acquisition arrives in T43
    DeviceRequest(String),
}

impl From<GpuContextError> for RuntimeError {
    fn from(err: GpuContextError) -> Self {
        match err {
            GpuContextError::NoAdapter => Self::GpuUnavailable,
            GpuContextError::DeviceRequest(detail) => Self::GpuInit { detail },
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

/// Handle to a wgpu adapter (and, in T43, a cached device+queue).
///
/// Construct with [`GpuContext::new`] when a GPU is expected, or
/// [`GpuContext::unavailable`] when the caller already knows there is no
/// GPU and wants a placeholder that reports [`DispatchKind::GpuCompute`] as
/// its target backend (used by T40's threshold logic to skip GPU paths).
///
/// Not `Clone` — adapter handles are uniquely owned. (T43's device+queue
/// cache uses `OnceLock`, not `Clone`.)
#[derive(Debug)]
pub struct GpuContext {
    /// The adapter we picked at construction time. `None` only when the
    /// context was built via [`GpuContext::unavailable`].
    //
    // `dead_code` is intentional: T43 will consume this via `OnceLock`
    // when it adds device+queue. T38 just proves acquisition.
    #[allow(dead_code)]
    adapter: Option<wgpu::Adapter>,
    /// Snapshot of adapter info captured at construction so tests can
    /// assert deterministically without holding GPU state.
    adapter_info: AdapterInfoSnapshot,
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
    /// T43 will extend this to also acquire a `(Device, Queue)` via the
    /// same `pollster` pattern, cached in a `OnceLock`.
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
        })
    }

    /// Construct a placeholder context that *claims* no GPU is available.
    ///
    /// Reports [`DispatchKind::GpuCompute`] as its target backend (so T40
    /// threshold logic still routes work towards "GPU if it existed") but
    /// [`Dispatcher::supports_gpu`] returns `false`. Used by T40/T43 when
    /// the host has no GPU drivers at all.
    pub fn unavailable() -> Self {
        Self {
            adapter: None,
            adapter_info: AdapterInfoSnapshot {
                name: String::new(),
                device_type: "None".into(),
            },
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
}

impl Dispatcher for GpuContext {
    fn kind(&self) -> DispatchKind {
        DispatchKind::GpuCompute
    }

    fn parallelism(&self) -> usize {
        // T45 will report a meaningful workgroup width once we have a
        // device to query `limits.max_compute_workgroups_per_dimension`.
        // T38 returns 0 because we have not yet acquired a device (T43).
        0
    }

    fn supports_gpu(&self) -> bool {
        self.adapter.is_some()
    }
}

//! Dispatcher trait — the common shape that CPU and GPU backends implement.
//!
//! For T38 we keep the trait minimal and **object-safe** so callers can
//! hold a `Box<dyn Dispatcher>` and pick a backend at runtime (T40
//! thresholds). T39 adds `par_map`/`par_filter`/`par_reduce` as concrete
//! methods on [`crate::CpuDispatcher`], and T45 mirrors them on
//! [`crate::GpuContext`]. New trait methods with default bodies are
//! non-breaking, so the trait can grow alongside the implementations.
//!
//! # Why object-safe matters
//!
//! T40's threshold logic will hold a `Vec<Box<dyn Dispatcher>>` and pick at
//! runtime based on data size + GPU availability. Trait methods therefore
//! must not have generic parameters or `Self`-by-value receivers — both of
//! which would make `dyn Dispatcher` impossible.

/// Which backend a [`Dispatcher`] will route work to.
///
/// Order matters: `SingleThread < CpuParallel < GpuCompute` is used by
/// T40's threshold logic (`<1000 → SingleThread`, `1000–50000 → CpuParallel`,
/// `>50000 → GpuCompute`). Do not re-order existing variants — T40's
/// threshold tests rely on this ordering.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DispatchKind {
    /// Sequential, single-thread execution. Used for tiny data (`<1000` elems).
    SingleThread,
    /// CPU parallel via rayon. Used for medium data (`1000–50000` elems).
    CpuParallel,
    /// GPU compute via wgpu. Used for large data (`>50000` elems).
    GpuCompute,
}

/// A backend capable of executing dispatch operations.
///
/// Object-safe: callers may use `&dyn Dispatcher` or `Box<dyn Dispatcher>`.
///
/// T38 defines only the *inspection* surface (kind/parallelism/supports_gpu).
/// The real dispatch methods (`map`/`filter`/`reduce`) arrive in T39/T45 as
/// concrete methods on the implementors — keeping them off the trait preserves
/// object-safety (generic methods would break `dyn Dispatcher`).
pub trait Dispatcher: std::fmt::Debug + Send + Sync {
    /// Which backend this dispatcher routes work to.
    fn kind(&self) -> DispatchKind;

    /// Maximum useful parallelism width (e.g. thread count). Returns `1`
    /// for `SingleThread` dispatchers, the rayon thread count for
    /// [`crate::CpuDispatcher`], and a workgroup-width estimate (T45)
    /// for [`crate::GpuContext`].
    fn parallelism(&self) -> usize;

    /// Whether this dispatcher can offload to a GPU. `false` for CPU
    /// dispatchers; `true` for [`crate::GpuContext`] only after an adapter
    /// has been acquired. Default is `false` so single-thread stubs do not
    /// need to override.
    fn supports_gpu(&self) -> bool {
        false
    }
}

//! CPU dispatcher — owns a rayon thread pool, ready for T39's `par_map`.
//!
//! T38 only proves that:
//!
//! 1. A default rayon pool can be built without panicking.
//! 2. The pool's thread count is `>= 1`.
//! 3. The dispatcher implements [`Dispatcher`] for runtime backend
//!    selection (T40).
//!
//! The real parallel `map`/`filter`/`reduce` logic is T39.

use crate::dispatch::{DispatchKind, Dispatcher};
use crate::error::RuntimeError;

/// Error raised while constructing a [`CpuDispatcher`].
///
/// T38 wraps rayon's `ThreadPoolBuildError`; richer variants arrive with
/// T39 (sizing, affinity) and T41 (race rejection).
#[derive(Debug, thiserror::Error)]
pub enum CpuDispatcherError {
    /// The rayon thread pool could not be built. Extremely unlikely with
    /// the default config; usually indicates the host is so starved of
    /// resources that even one thread can't spawn.
    #[error("rayon thread pool build failed: {0}")]
    PoolBuild(#[source] rayon::ThreadPoolBuildError),
}

impl From<CpuDispatcherError> for RuntimeError {
    fn from(err: CpuDispatcherError) -> Self {
        Self::Unsupported {
            detail: err.to_string(),
        }
    }
}

/// Owns a rayon thread pool for CPU-parallel dispatch.
///
/// Construct with [`CpuDispatcher::new`]. The pool's thread count defaults
/// to rayon's default (one per logical core). T39 will add sizing knobs
/// (e.g. `with_num_threads`).
///
/// Not `Clone` — thread pools are uniquely owned. (Sharing requires an
/// `Arc`, which T41/T42 will introduce for atomic accumulator sharing.)
#[derive(Debug)]
pub struct CpuDispatcher {
    pool: rayon::ThreadPool,
}

impl CpuDispatcher {
    /// Construct a CPU dispatcher with rayon's default thread count.
    ///
    /// # Errors
    ///
    /// Returns [`CpuDispatcherError::PoolBuild`] only if rayon itself
    /// refuses to build a pool — extremely unlikely with the default
    /// config.
    pub fn new() -> Result<Self, CpuDispatcherError> {
        let pool = rayon::ThreadPoolBuilder::new()
            .build()
            .map_err(CpuDispatcherError::PoolBuild)?;
        Ok(Self { pool })
    }

    /// Number of worker threads in the owned pool. Always `>= 1` on a
    /// successful build.
    pub fn thread_count(&self) -> usize {
        self.pool.current_num_threads()
    }

    /// Install this pool as the active rayon pool for the duration of the
    /// closure. T39 uses this so `par_iter` inside `par_map` runs on this
    /// specific pool rather than the global one (which may not exist).
    ///
    /// Useful for T38 because it lets us verify the pool is actually
    /// usable end-to-end — the closure runs on a worker thread.
    ///
    /// `Send` bounds mirror rayon's own: the closure and its result must
    /// be sendable across worker threads.
    pub fn with_pool<R: Send>(&self, f: impl FnOnce() -> R + Send) -> R {
        self.pool.install(f)
    }
}

impl Dispatcher for CpuDispatcher {
    fn kind(&self) -> DispatchKind {
        DispatchKind::CpuParallel
    }

    fn parallelism(&self) -> usize {
        self.thread_count()
    }

    fn supports_gpu(&self) -> bool {
        false
    }
}

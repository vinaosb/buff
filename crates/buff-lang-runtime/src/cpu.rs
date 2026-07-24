//! CPU dispatcher — owns a rayon thread pool and exposes the three
//! deterministic parallel primitives [`CpuDispatcher::par_map`],
//! [`CpuDispatcher::par_filter`], [`CpuDispatcher::par_reduce`].
//!
//! T38 proved that:
//!
//! 1. A default rayon pool can be built without panicking.
//! 2. The pool's thread count is `>= 1`.
//! 3. The dispatcher implements [`Dispatcher`] for runtime backend
//!    selection (T40).
//!
//! T39 adds the real parallel `map`/`filter`/`reduce` logic. The methods
//! are **concrete** on [`CpuDispatcher`] (not on the [`Dispatcher`] trait)
//! because their generic bounds would break object-safety — see the trait
//! docs in [`crate::dispatch`] for the rationale.

use crate::dispatch::{DispatchKind, Dispatcher};
use crate::error::RuntimeError;
use rayon::prelude::*;

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
            span: None,
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

    /// Apply `f` to every element of `input` in parallel and collect the
    /// results in **input order**.
    ///
    /// Backed by `rayon::par_iter::map` + `collect::<Vec<_>>()`, which
    /// preserves input order regardless of how work is distributed across
    /// threads. Runs on this dispatcher's owned pool via
    /// [`with_pool`](Self::with_pool), not the global rayon pool.
    ///
    /// Acceptance case: `par_map(vec![1, 2, 3], |x| x * 2) == vec![2, 4, 6]`.
    ///
    /// Determinism contract: same input + same closure → same output,
    /// regardless of thread count (rayon's ordered `collect` guarantees
    /// this).
    ///
    /// The closure consumes its argument (`Fn(T) -> U`, not `Fn(&T) -> U`).
    /// Pass references inside the closure if you need to keep the source
    /// data.
    ///
    /// # Bounds (why each is required)
    ///
    /// * `T: Send` — input elements are moved onto worker threads.
    /// * `U: Send` — output elements are moved back to the collecting
    ///   thread.
    /// * `F: Fn(T) -> U + Sync + Send` — rayon requires the closure be
    ///   callable from multiple threads (`Sync`) and movable to them
    ///   (`Send`). `Fn` (not `FnMut`/`FnOnce`) because it may be invoked
    ///   many times across threads.
    #[must_use]
    pub fn par_map<T, U, F>(&self, input: Vec<T>, f: F) -> Vec<U>
    where
        T: Send,
        U: Send,
        F: Fn(T) -> U + Sync + Send,
    {
        self.with_pool(|| input.into_par_iter().map(f).collect::<Vec<U>>())
    }

    /// Keep only the elements of `input` for which `pred` returns `true`,
    /// preserving **input order**.
    ///
    /// Backed by `rayon::par_iter::filter` + `collect::<Vec<_>>()`, which
    /// preserves input order. Runs on this dispatcher's owned pool.
    ///
    /// Determinism contract: same input + same predicate → same output,
    /// regardless of thread count.
    ///
    /// # Bounds
    ///
    /// * `T: Send` — elements flow across worker threads. `filter` borrows
    ///   each element by reference (`Fn(&T) -> bool`), so `T` does **not**
    ///   need to be `Clone` — ownership is retained and the kept elements
    ///   are returned in their original form.
    /// * `P: Fn(&T) -> bool + Sync + Send` — rayon requires the predicate
    ///   be callable from multiple worker threads.
    #[must_use]
    pub fn par_filter<T, P>(&self, input: Vec<T>, pred: P) -> Vec<T>
    where
        T: Send,
        P: Fn(&T) -> bool + Sync + Send,
    {
        self.with_pool(|| input.into_par_iter().filter(pred).collect::<Vec<T>>())
    }

    /// Reduce `input` to a single value using `op`, parallelized across
    /// this dispatcher's pool.
    ///
    /// Backed by `rayon::par_iter::reduce`. Each worker thread reduces its
    /// slice using `op` starting from a clone of `identity`; the per-thread
    /// results are then combined with `op` again. Runs on this
    /// dispatcher's owned pool.
    ///
    /// # Determinism contract — READ CAREFULLY
    ///
    /// For fully deterministic results **independent of thread count**, the
    /// caller must provide:
    ///
    /// 1. An `identity` that is a true two-sided identity for `op`
    ///    (`op(identity, x) == x == op(x, identity)`).
    /// 2. An `op` that is **associative** (`op(a, op(b, c)) == op(op(a, b), c)`).
    ///
    /// If both hold, the result is reproducible across all thread counts
    /// and all runs (e.g. integer addition, multiplication, `max`, `min`,
    /// string concatenation with a canonical ordering, set union).
    ///
    /// If `op` is **not** associative (e.g. floating-point `+`), the result
    /// is **deterministic per run on a fixed thread count** but may differ
    /// across runs with different thread counts. The caller owns this
    /// caveat — use integer math, fixed-point, or a strictly-associative
    /// monoid where determinism matters.
    ///
    /// Reducing an empty `input` always returns `identity` (cloned).
    ///
    /// # Bounds
    ///
    /// * `T: Send + Sync + Clone` — elements flow across worker threads
    ///   (`Send`); the identity closure captures `&identity` and must be
    ///   callable from multiple threads (`Sync`); `identity` is cloned
    ///   once per worker thread as its starting accumulator (`Clone`).
    /// * `O: Fn(T, T) -> T + Sync + Send` — rayon requires the operator be
    ///   callable from multiple worker threads.
    #[must_use]
    pub fn par_reduce<T, O>(&self, input: Vec<T>, identity: T, op: O) -> T
    where
        T: Send + Sync + Clone,
        O: Fn(T, T) -> T + Sync + Send,
    {
        self.with_pool(|| input.into_par_iter().reduce(|| identity.clone(), op))
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

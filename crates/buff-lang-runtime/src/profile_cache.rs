//! T12: Dispatch profile cache (v1.25 Wave 1).
//!
//! Caches [`DecideKind`] decisions per `(function signature + input shape
//! profile)` so subsequent calls with the same shape skip the cost-model
//! evaluation entirely. In-memory only for v1.25 — no persistence, no
//! cross-process sharing.
//!
//! # Why cache?
//!
//! [`decide_dynamic`](crate::threshold::decide_dynamic) is already
//! sub-microsecond, but in tight loops dispatching millions of elements
//! the per-call overhead adds up. Many real workloads dispatch the SAME
//! kernel with the SAME input shape repeatedly (e.g. a training loop
//! processing fixed-size batches). The profile cache turns the 2nd+ calls
//! into a `BTreeMap` lookup — a single pointer-chase + comparison.
//!
//! # Determinism
//!
//! The cache is backed by a [`BTreeMap`] (NOT `HashMap` — project hard
//! rule). Iteration order is deterministic; the cache produces the same
//! sequence of hits/misses on every host given the same call sequence.
//!
//! # Key design
//!
//! [`ProfileKey`] captures the six factors that affect a dispatch decision:
//!
//! * `signature` — the function/shader identifier (caller-provided string).
//! * `element_count` — exact input length.
//! * `bytes_per_element` — element width (0 = threshold path, >0 = cost model).
//! * `gpu_available` — GPU presence at the time of the original decision.
//! * `data_location` — T10 [`DataLocation`] (Cpu/Gpu).
//! * `intensity_bucket` — quantized arithmetic intensity (0.1 FLOPs/byte
//!   resolution) so nearby intensities hit the same cache slot.
//!
//! Two contexts that produce the same key are guaranteed to produce the
//! same [`DecideKind`] — the decision function is pure.
//!
//! # Concurrency
//!
//! [`Mutex`]-protected, same pattern as [`PipelineCache`](crate::PipelineCache).
//! Lock is held for microseconds (one `BTreeMap` lookup or insert). Poisoning
//! is handled gracefully (treated as empty cache — next call re-evaluates).

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;

use crate::dispatch::DispatchKind;
use crate::threshold::{decide_dynamic, DataLocation, WorkloadContext};

/// Quantization resolution for arithmetic intensity bucketing.
///
/// Intensities are rounded to the nearest 0.1 FLOPs/byte so that nearby
/// values (e.g. 4.03 and 4.07) hit the same cache entry. The bucket is
/// stored as `u32` (`intensity * 10.0`, rounded).
const INTENSITY_BUCKET_RESOLUTION: f64 = 10.0;

/// The bucket value for `None` (unknown) arithmetic intensity.
///
/// Chosen to be distinct from all valid buckets (which are `>= 0`) and
/// from the NaN sentinel. Placed at 0 so it sorts first in the BTreeMap.
const INTENSITY_BUCKET_NONE: u32 = 0;

/// The bucket value for `NaN` arithmetic intensity.
///
/// `u32::MAX` ensures NaN sorts last in the BTreeMap and never collides
/// with a valid bucket (which are at most ~`f64::MAX * 10` but in practice
/// < 10_000).
const INTENSITY_BUCKET_NAN: u32 = u32::MAX;

/// T12: Cache key for a dispatch decision.
///
/// Captures the six factors that affect [`decide_dynamic`]'s output.
/// Two [`WorkloadContext`] values that produce the same [`ProfileKey`]
/// are guaranteed to produce the same [`DispatchKind`] — the decision
/// function is pure.
///
/// # Construction
///
/// Use [`ProfileKey::from_context`] to build a key from a
/// [`WorkloadContext`] + signature string:
///
/// ```
/// use buff_lang_runtime::{ProfileKey, WorkloadContext};
///
/// let ctx = WorkloadContext::new(100_000, true).with_intensity(8.0);
/// let key = ProfileKey::from_context("square_all", &ctx);
/// ```
///
/// # Ordering
///
/// Derives `Ord` — all fields are totally ordered. [`ProfileKey`] sorts
/// first by `signature` (lexicographic), then by the numeric fields.
/// This makes the [`BTreeMap`] iteration order deterministic.
///
/// # Intensity bucketing
///
/// `arithmetic_intensity: Option<f64>` is quantized to `intensity_bucket:
/// u32` at 0.1 FLOPs/byte resolution. `None` → bucket 0; `NaN` →
/// [`u32::MAX`]. This means intensities 4.0, 4.04, and 4.06 all map to
/// bucket 40 — hitting the same cache slot. This is safe because
/// [`decide_dynamic`] uses a `>=` threshold comparison, so sub-bucket
/// variation doesn't change the decision.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct ProfileKey {
    /// Function or shader identifier (caller-provided). Typically the WGSL
    /// source hash, a function name, or any stable string that identifies
    /// the dispatch site.
    pub signature: String,
    /// Exact input element count at dispatch time.
    pub element_count: usize,
    /// Bytes per element (0 = threshold path; >0 = cost model).
    pub bytes_per_element: u64,
    /// Whether a GPU was available when the cached decision was made.
    pub gpu_available: bool,
    /// Where the data lived (T10).
    pub data_location: DataLocation,
    /// Quantized arithmetic intensity (`intensity * 10`, rounded). See
    /// [`bucket_intensity`].
    pub intensity_bucket: u32,
}

impl ProfileKey {
    /// Build a [`ProfileKey`] from a signature string + [`WorkloadContext`].
    ///
    /// The arithmetic intensity is quantized to 0.1 FLOPs/byte resolution
    /// via [`bucket_intensity`] so nearby values share a cache slot.
    ///
    /// # Examples
    ///
    /// ```
    /// use buff_lang_runtime::{ProfileKey, WorkloadContext};
    ///
    /// let ctx = WorkloadContext::new(100_000, true).with_intensity(8.0);
    /// let key = ProfileKey::from_context("my_kernel", &ctx);
    /// assert_eq!(key.signature, "my_kernel");
    /// assert_eq!(key.element_count, 100_000);
    /// ```
    #[must_use]
    pub fn from_context(signature: &str, ctx: &WorkloadContext) -> Self {
        Self {
            signature: signature.to_string(),
            element_count: ctx.element_count,
            bytes_per_element: ctx.bytes_per_element,
            gpu_available: ctx.gpu_available,
            data_location: ctx.data_location,
            intensity_bucket: bucket_intensity(ctx.arithmetic_intensity),
        }
    }
}

/// Quantize an `Option<f64>` arithmetic intensity into a `u32` bucket.
///
/// * `None` → [`INTENSITY_BUCKET_NONE`] (0) — "unknown" intensity.
/// * `Some(NaN)` → [`INTENSITY_BUCKET_NAN`] (`u32::MAX`) — separate from
///   all valid values.
/// * `Some(v)` → `(v * `[`INTENSITY_BUCKET_RESOLUTION`]`)`.round()` as `u32`.
///   Negative intensities clamp to 0.
///
/// # Determinism
///
/// Pure function — same input → same bucket on every host. The rounding
/// uses `f64::round` which is IEEE 754 round-half-to-even (deterministic).
///
/// # Examples
///
/// ```
/// use buff_lang_runtime::bucket_intensity;
///
/// assert_eq!(bucket_intensity(None), 0);
/// assert_eq!(bucket_intensity(Some(4.0)), 40);
/// assert_eq!(bucket_intensity(Some(4.04)), 40);  // rounds to 40
/// assert_eq!(bucket_intensity(Some(4.06)), 41);  // rounds to 41
/// assert_eq!(bucket_intensity(Some(f64::NAN)), u32::MAX);
/// ```
#[must_use]
pub fn bucket_intensity(intensity: Option<f64>) -> u32 {
    match intensity {
        None => INTENSITY_BUCKET_NONE,
        Some(v) if v.is_nan() => INTENSITY_BUCKET_NAN,
        Some(v) => {
            let bucketed = (v * INTENSITY_BUCKET_RESOLUTION).round();
            // Clamp: negative intensities (nonsensical but defensive) → 0.
            // Very large intensities saturate to u32::MAX - 1 (reserved
            // for NaN). This is fine — intensities > ~429M FLOPs/byte are
            // physically meaningless.
            if bucketed <= 0.0 {
                0
            } else if bucketed >= (u32::MAX as f64) - 1.0 {
                u32::MAX - 1
            } else {
                bucketed as u32
            }
        }
    }
}

/// T12: In-memory dispatch profile cache.
///
/// Stores [`DispatchKind`] decisions keyed by [`ProfileKey`]. On a cache
/// hit, the stored decision is returned without re-evaluating
/// [`decide_dynamic`] (or the cost model). On a miss, the decision is
/// computed, stored, and returned.
///
/// # Backing store
///
/// `Mutex<BTreeMap<ProfileKey, DispatchKind>>` — BTreeMap for deterministic
/// iteration order (project hard rule: no HashMap anywhere in this crate).
/// The `Mutex` serializes access; lock contention is negligible (lookups
/// take microseconds).
///
/// # Counters
///
/// [`AtomicUsize`] hit/miss counters track cache effectiveness. Read via
/// [`ProfileCache::hit_count`] and [`ProfileCache::miss_count`]. The QA
/// assertion "dispatch N times → miss count == 1, hit count == N-1" lives
/// in `tests/profile_cache_tests.rs`.
///
/// # v1.25 scope
///
/// In-memory only — no persistence, no cross-process sharing, no eviction
/// policy. The cache is unbounded (real Buff programs have a small finite
/// set of distinct `(signature, shape)` pairs). A bounded LRU variant is
/// deferred to post-v1.25.
///
/// # Concurrency
///
/// `Send + Sync` (Mutex + AtomicUsize are both). Can be held as
/// `Arc<ProfileCache>` across threads. Mutex poisoning is handled
/// gracefully: a poisoned cache is treated as empty (next call
/// re-evaluates and tries to re-insert).
///
/// # Examples
///
/// ```
/// use buff_lang_runtime::{ProfileCache, WorkloadContext, DispatchKind};
///
/// let cache = ProfileCache::new();
/// let ctx = WorkloadContext::new(100_000, true).with_intensity(8.0);
///
/// // First call: cache miss → evaluate → store → return.
/// let d1 = cache.decide_cached("square_all", &ctx);
/// assert_eq!(d1, DispatchKind::GpuCompute);
/// assert_eq!(cache.miss_count(), 1);
/// assert_eq!(cache.hit_count(), 0);
///
/// // Second call: cache hit → return stored decision.
/// let d2 = cache.decide_cached("square_all", &ctx);
/// assert_eq!(d2, d1);
/// assert_eq!(cache.miss_count(), 1);  // unchanged
/// assert_eq!(cache.hit_count(), 1);
/// ```
#[derive(Debug, Default)]
pub struct ProfileCache {
    /// The cache map. BTreeMap for deterministic ordering.
    map: Mutex<BTreeMap<ProfileKey, DispatchKind>>,
    /// Incremented on cache HIT (key found).
    hits: AtomicUsize,
    /// Incremented on cache MISS (key not found → evaluate).
    misses: AtomicUsize,
}

impl ProfileCache {
    /// Construct an empty profile cache.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Look up a cached decision by key.
    ///
    /// Returns `Some(decision)` on a hit, `None` on a miss. Does NOT
    /// modify the cache or increment counters — use
    /// [`Self::get_or_decide`] or [`Self::decide_cached`] for the full
    /// hit/miss tracking flow.
    ///
    /// Returns `None` on a poisoned mutex (cannot happen in normal
    /// operation).
    #[must_use]
    pub fn get(&self, key: &ProfileKey) -> Option<DispatchKind> {
        self.map
            .lock()
            .ok()
            .and_then(|guard| guard.get(key).copied())
    }

    /// Store a decision in the cache.
    ///
    /// Overwrites any existing entry for the same key. Does NOT increment
    /// counters — use [`Self::get_or_decide`] for the full flow.
    ///
    /// Silently no-ops on a poisoned mutex.
    pub fn insert(&self, key: ProfileKey, decision: DispatchKind) {
        if let Ok(mut guard) = self.map.lock() {
            guard.insert(key, decision);
        }
    }

    /// Get-or-decide: look up `key`; on hit return the cached decision;
    /// on miss call `decide()`, store the result, return it.
    ///
    /// Increments [`Self::hit_count`] on a hit, [`Self::miss_count`] on a
    /// miss.
    ///
    /// # Concurrency
    ///
    /// The mutex is held only for the BTreeMap lookup (hit) or lookup +
    /// insert (miss). The `decide` closure runs OUTSIDE the lock — so
    /// concurrent misses for different keys don't serialize on each other's
    /// evaluations. (Concurrent misses for the SAME key may both evaluate
    /// and insert — the second insert overwrites the first, which is
    /// harmless since the decision is deterministic.)
    ///
    /// # Poisoning
    ///
    /// On a poisoned mutex, the cache is treated as empty: every call is a
    /// "miss" that evaluates `decide()` and returns the result without
    /// storing.
    pub fn get_or_decide<F>(&self, key: &ProfileKey, decide: F) -> DispatchKind
    where
        F: FnOnce() -> DispatchKind,
    {
        // Fast path: lock, check for hit, return.
        if let Ok(guard) = self.map.lock() {
            if let Some(&cached) = guard.get(key) {
                self.hits.fetch_add(1, Ordering::Relaxed);
                return cached;
            }
        }
        // Miss (or poisoned): evaluate outside the lock.
        let decision = decide();
        self.misses.fetch_add(1, Ordering::Relaxed);

        // Re-lock and insert. Handle the race where another thread inserted
        // the same key in parallel (harmless — same deterministic decision).
        if let Ok(mut guard) = self.map.lock() {
            guard.insert(key.clone(), decision);
        }
        decision
    }

    /// Convenience: build a [`ProfileKey`] from `signature` + `ctx`, then
    /// [`Self::get_or_decide`] with [`decide_dynamic`].
    ///
    /// This is the one-liner callers use in dispatch sites:
    ///
    /// ```ignore
    /// let decision = cache.decide_cached("square_all", &ctx);
    /// ```
    ///
    /// On the first call for a given `(signature, shape)`, this evaluates
    /// [`decide_dynamic`] and caches the result. On subsequent calls with
    /// the same shape, it returns the cached decision (a single BTreeMap
    /// lookup).
    #[must_use]
    pub fn decide_cached(&self, signature: &str, ctx: &WorkloadContext) -> DispatchKind {
        let key = ProfileKey::from_context(signature, ctx);
        self.get_or_decide(&key, || decide_dynamic(ctx))
    }

    /// How many cache hits have occurred (key found).
    ///
    /// * `0` initially and after [`Self::clear`].
    /// * Incremented by 1 on each [`Self::get_or_decide`] / [`Self::decide_cached`]
    ///   hit.
    pub fn hit_count(&self) -> usize {
        self.hits.load(Ordering::Relaxed)
    }

    /// How many cache misses have occurred (key not found → evaluate).
    ///
    /// * `0` initially and after [`Self::clear`].
    /// * Incremented by 1 on each [`Self::get_or_decide`] / [`Self::decide_cached`]
    ///   miss.
    pub fn miss_count(&self) -> usize {
        self.misses.load(Ordering::Relaxed)
    }

    /// How many distinct `(signature, shape)` entries are cached.
    ///
    /// Equal to [`Self::miss_count`] when no entries have been cleared
    /// (the cache is unbounded in v1.25). Returns 0 on a poisoned mutex.
    #[must_use]
    pub fn len(&self) -> usize {
        self.map.lock().map(|guard| guard.len()).unwrap_or(0)
    }

    /// Whether the cache is empty (no entries).
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Whether a key matching the given `(signature, ctx)` is cached.
    ///
    /// Pure observational — does NOT modify the cache or increment counters.
    /// Returns `false` on a poisoned mutex.
    #[must_use]
    pub fn contains(&self, signature: &str, ctx: &WorkloadContext) -> bool {
        let key = ProfileKey::from_context(signature, ctx);
        self.map
            .lock()
            .map(|guard| guard.contains_key(&key))
            .unwrap_or(false)
    }

    /// Remove all cached entries and reset hit/miss counters.
    ///
    /// After this call: `len() == 0`, `hit_count() == 0`, `miss_count() == 0`.
    /// Silently no-ops on a poisoned mutex (counters still reset).
    pub fn clear(&self) {
        if let Ok(mut guard) = self.map.lock() {
            guard.clear();
        }
        self.hits.store(0, Ordering::Relaxed);
        self.misses.store(0, Ordering::Relaxed);
    }
}

#[cfg(test)]
mod tests {
    //! Inline unit tests for [`bucket_intensity`] — full behavioral
    //! coverage of [`ProfileCache`] lives in `tests/profile_cache_tests.rs`
    //! so the QA filter `cargo test -p buff-lang-runtime profile_cache`
    //! matches the whole suite.

    use super::*;

    #[test]
    fn profile_cache_bucket_none_is_zero() {
        assert_eq!(bucket_intensity(None), INTENSITY_BUCKET_NONE);
    }

    #[test]
    fn profile_cache_bucket_nan_is_max() {
        assert_eq!(bucket_intensity(Some(f64::NAN)), INTENSITY_BUCKET_NAN);
    }

    #[test]
    fn profile_cache_bucket_four_is_forty() {
        assert_eq!(bucket_intensity(Some(4.0)), 40);
    }

    #[test]
    fn profile_cache_bucket_nearby_values_share_slot() {
        // 4.03 and 4.07 both round to bucket 40 (0.1 resolution).
        assert_eq!(bucket_intensity(Some(4.03)), 40);
        assert_eq!(bucket_intensity(Some(4.07)), 41);
    }

    #[test]
    fn profile_cache_bucket_negative_clamps_to_zero() {
        assert_eq!(bucket_intensity(Some(-1.0)), 0);
    }
}

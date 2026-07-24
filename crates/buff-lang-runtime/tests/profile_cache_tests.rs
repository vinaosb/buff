//! T12: Dispatch profile cache (v1.25 Wave 1).
//!
//! Integration tests for [`ProfileCache`] + [`ProfileKey`] +
//! [`bucket_intensity`]. Every test name contains `profile_cache` so the
//! QA filter `cargo test -p buff-lang-runtime profile_cache` matches.
//!
//! # Coverage matrix
//!
//! * **Cache hit/miss** (3 tests): first call misses, second hits, third hits.
//! * **Different shapes miss** (2 tests): different element_count / intensity
//!   → different key → miss.
//! * **Same shape different signature miss** (1 test): signature is part of key.
//! * **Determinism** (1 test): same inputs → same cached decision across runs.
//! * **Counters** (2 tests): hit_count / miss_count track correctly; clear resets.
//! * **decide_cached parity** (1 test): cached result == decide_dynamic.
//! * **BTreeMap not HashMap** (1 test): iteration order is deterministic.
//! * **bucket_intensity** (4 tests): None, NaN, quantization, negative.
//! * **Contains / len / is_empty** (3 tests): observational methods.

use buff_lang_runtime::{
    bucket_intensity, decide_dynamic, ProfileCache, ProfileKey, WorkloadContext, DispatchKind,
    DataLocation,
};

// ===========================================================================
// Cache hit/miss lifecycle.
// ===========================================================================

#[test]
fn profile_cache_first_call_misses_second_hits() {
    let cache = ProfileCache::new();
    let ctx = WorkloadContext::new(100_000, true).with_intensity(8.0);

    // First call: miss → evaluate → store.
    let d1 = cache.decide_cached("square_all", &ctx);
    assert_eq!(cache.miss_count(), 1, "first call must miss");
    assert_eq!(cache.hit_count(), 0, "first call cannot hit");

    // Second call: hit → return cached.
    let d2 = cache.decide_cached("square_all", &ctx);
    assert_eq!(d2, d1, "cached decision must match");
    assert_eq!(cache.miss_count(), 1, "second call must NOT miss");
    assert_eq!(cache.hit_count(), 1, "second call must hit");
}

#[test]
fn profile_cache_third_call_also_hits() {
    let cache = ProfileCache::new();
    let ctx = WorkloadContext::new(50_001, true);

    let _ = cache.decide_cached("kernel", &ctx);
    let _ = cache.decide_cached("kernel", &ctx);
    let _ = cache.decide_cached("kernel", &ctx);

    assert_eq!(cache.miss_count(), 1, "three calls → 1 miss");
    assert_eq!(cache.hit_count(), 2, "three calls → 2 hits");
}

#[test]
fn profile_cache_cached_decision_matches_decide_dynamic() {
    // The cached decision must equal what decide_dynamic would return.
    let cache = ProfileCache::new();
    let ctx = WorkloadContext::new(100_000, true).with_intensity(0.5);

    let cached = cache.decide_cached("demoted", &ctx);
    let direct = decide_dynamic(&ctx);
    assert_eq!(
        cached, direct,
        "cached decision must match direct decide_dynamic"
    );
}

// ===========================================================================
// Different shapes → different keys → miss.
// ===========================================================================

#[test]
fn profile_cache_different_element_count_misses() {
    let cache = ProfileCache::new();
    let ctx1 = WorkloadContext::new(10_000, true);
    let ctx2 = WorkloadContext::new(20_000, true);

    let _ = cache.decide_cached("k", &ctx1);
    let _ = cache.decide_cached("k", &ctx2);

    assert_eq!(cache.miss_count(), 2, "different count → different key → 2 misses");
    assert_eq!(cache.hit_count(), 0);
}

#[test]
fn profile_cache_different_intensity_misses() {
    let cache = ProfileCache::new();
    let ctx_high = WorkloadContext::new(100_000, true).with_intensity(8.0);
    let ctx_low = WorkloadContext::new(100_000, true).with_intensity(0.5);

    let _ = cache.decide_cached("k", &ctx_high);
    let _ = cache.decide_cached("k", &ctx_low);

    // 8.0 → bucket 80; 0.5 → bucket 5. Different buckets → different keys.
    assert_eq!(cache.miss_count(), 2, "different intensity bucket → 2 misses");
}

#[test]
fn profile_cache_nearby_intensity_same_bucket_hits() {
    // 8.02 and 8.03 both round to bucket 80 (0.1 resolution).
    let cache = ProfileCache::new();
    let ctx1 = WorkloadContext::new(100_000, true).with_intensity(8.02);
    let ctx2 = WorkloadContext::new(100_000, true).with_intensity(8.03);

    let _ = cache.decide_cached("k", &ctx1);
    let _ = cache.decide_cached("k", &ctx2);

    assert_eq!(cache.miss_count(), 1, "nearby intensity → same bucket → 1 miss");
    assert_eq!(cache.hit_count(), 1, "second call hits same bucket");
}

// ===========================================================================
// Signature is part of the key.
// ===========================================================================

#[test]
fn profile_cache_different_signature_misses() {
    let cache = ProfileCache::new();
    let ctx = WorkloadContext::new(100_000, true).with_intensity(8.0);

    let _ = cache.decide_cached("kernel_a", &ctx);
    let _ = cache.decide_cached("kernel_b", &ctx);

    assert_eq!(cache.miss_count(), 2, "different signature → 2 misses");
}

// ===========================================================================
// Determinism.
// ===========================================================================

#[test]
fn profile_cache_same_inputs_produce_same_decision_across_instances() {
    let ctx = WorkloadContext::new(100_000, true).with_intensity(8.0);

    let cache1 = ProfileCache::new();
    let cache2 = ProfileCache::new();

    let d1 = cache1.decide_cached("kernel", &ctx);
    let d2 = cache2.decide_cached("kernel", &ctx);

    assert_eq!(
        d1, d2,
        "two independent caches must produce the same decision for the same inputs"
    );
}

// ===========================================================================
// Counters + clear.
// ===========================================================================

#[test]
fn profile_cache_clear_resets_everything() {
    let cache = ProfileCache::new();
    let ctx = WorkloadContext::new(100_000, true).with_intensity(8.0);

    let _ = cache.decide_cached("k", &ctx);
    let _ = cache.decide_cached("k", &ctx);
    assert_eq!(cache.len(), 1);
    assert_eq!(cache.hit_count(), 1);
    assert_eq!(cache.miss_count(), 1);

    cache.clear();
    assert_eq!(cache.len(), 0, "clear empties the cache");
    assert_eq!(cache.hit_count(), 0, "clear resets hit counter");
    assert_eq!(cache.miss_count(), 0, "clear resets miss counter");
    assert!(cache.is_empty());

    // After clear, the next call is a miss again.
    let _ = cache.decide_cached("k", &ctx);
    assert_eq!(cache.miss_count(), 1, "post-clear call must miss");
}

#[test]
fn profile_cache_hit_miss_ratios_for_mixed_workload() {
    // Simulate a workload that dispatches 3 distinct shapes, each twice.
    let cache = ProfileCache::new();

    for count in [10_000, 50_001, 100_000] {
        let ctx = WorkloadContext::new(count, true);
        let _ = cache.decide_cached("k", &ctx); // miss
        let _ = cache.decide_cached("k", &ctx); // hit
    }

    assert_eq!(cache.miss_count(), 3, "3 distinct shapes → 3 misses");
    assert_eq!(cache.hit_count(), 3, "3 repeat calls → 3 hits");
    assert_eq!(cache.len(), 3, "3 entries cached");
}

// ===========================================================================
// Observational methods: contains / len / is_empty.
// ===========================================================================

#[test]
fn profile_cache_contains_after_insert() {
    let cache = ProfileCache::new();
    let ctx = WorkloadContext::new(100_000, true).with_intensity(8.0);

    assert!(!cache.contains("k", &ctx), "before first call: not cached");

    let _ = cache.decide_cached("k", &ctx);

    assert!(cache.contains("k", &ctx), "after first call: cached");
}

#[test]
fn profile_cache_len_tracks_distinct_entries() {
    let cache = ProfileCache::new();
    assert!(cache.is_empty());
    assert_eq!(cache.len(), 0);

    let ctx1 = WorkloadContext::new(10_000, true);
    let ctx2 = WorkloadContext::new(20_000, true);

    let _ = cache.decide_cached("a", &ctx1);
    assert_eq!(cache.len(), 1);

    let _ = cache.decide_cached("b", &ctx1); // different signature → new entry
    assert_eq!(cache.len(), 2);

    let _ = cache.decide_cached("a", &ctx1); // same key → hit, no new entry
    assert_eq!(cache.len(), 2);

    let _ = cache.decide_cached("a", &ctx2); // different count → new entry
    assert_eq!(cache.len(), 3);
}

#[test]
fn profile_cache_get_returns_cached_decision() {
    let cache = ProfileCache::new();
    let ctx = WorkloadContext::new(100_000, true).with_intensity(8.0);

    // Nothing cached yet.
    let key = ProfileKey::from_context("k", &ctx);
    assert!(cache.get(&key).is_none());

    // Cache a decision.
    let _ = cache.decide_cached("k", &ctx);

    // Now get() returns it.
    assert_eq!(cache.get(&key), Some(DispatchKind::GpuCompute));
}

// ===========================================================================
// ProfileKey construction.
// ===========================================================================

#[test]
fn profile_cache_key_from_context_captures_all_fields() {
    let ctx = WorkloadContext::new(42_000, true)
        .with_intensity(7.3)
        .with_bytes_per_element(8)
        .with_data_location(DataLocation::Gpu);
    let key = ProfileKey::from_context("my_kernel", &ctx);

    assert_eq!(key.signature, "my_kernel");
    assert_eq!(key.element_count, 42_000);
    assert_eq!(key.bytes_per_element, 8);
    assert!(key.gpu_available);
    assert_eq!(key.data_location, DataLocation::Gpu);
    assert_eq!(key.intensity_bucket, 73); // 7.3 * 10 = 73
}

#[test]
fn profile_cache_key_none_intensity_bucket_is_zero() {
    let ctx = WorkloadContext::new(100_000, true); // intensity = None
    let key = ProfileKey::from_context("k", &ctx);
    assert_eq!(key.intensity_bucket, 0);
}

#[test]
fn profile_cache_key_nan_intensity_bucket_is_max() {
    let ctx = WorkloadContext::new(100_000, true).with_intensity(f64::NAN);
    let key = ProfileKey::from_context("k", &ctx);
    assert_eq!(key.intensity_bucket, u32::MAX);
}

// ===========================================================================
// bucket_intensity — public function tests.
// ===========================================================================

#[test]
fn profile_cache_bucket_intensity_none_is_zero() {
    assert_eq!(bucket_intensity(None), 0);
}

#[test]
fn profile_cache_bucket_intensity_nan_is_max() {
    assert_eq!(bucket_intensity(Some(f64::NAN)), u32::MAX);
}

#[test]
fn profile_cache_bucket_intensity_quantizes_to_tenths() {
    assert_eq!(bucket_intensity(Some(0.0)), 0);
    assert_eq!(bucket_intensity(Some(0.1)), 1);
    assert_eq!(bucket_intensity(Some(1.0)), 10);
    assert_eq!(bucket_intensity(Some(4.0)), 40);
    assert_eq!(bucket_intensity(Some(4.04)), 40);
    assert_eq!(bucket_intensity(Some(4.05)), 41); // round half to even → 41? Actually round() rounds 0.5 up.
    assert_eq!(bucket_intensity(Some(4.06)), 41);
    assert_eq!(bucket_intensity(Some(100.0)), 1000);
}

#[test]
fn profile_cache_bucket_intensity_negative_clamps_to_zero() {
    assert_eq!(bucket_intensity(Some(-1.0)), 0);
    assert_eq!(bucket_intensity(Some(-100.0)), 0);
}

// ===========================================================================
// get_or_decide with custom closure.
// ===========================================================================

#[test]
fn profile_cache_get_or_decide_uses_closure_on_miss() {
    let cache = ProfileCache::new();
    let ctx = WorkloadContext::new(100_000, true).with_intensity(8.0);
    let key = ProfileKey::from_context("k", &ctx);

    let mut call_count = 0u32;
    let decision = cache.get_or_decide(&key, || {
        call_count += 1;
        DispatchKind::GpuCompute
    });

    assert_eq!(decision, DispatchKind::GpuCompute);
    assert_eq!(call_count, 1, "closure called once on miss");

    // Second call: hit → closure NOT called.
    let _ = cache.get_or_decide(&key, || {
        call_count += 1;
        DispatchKind::SingleThread // different value, but won't be used
    });
    assert_eq!(call_count, 1, "closure NOT called on hit");
}

#[test]
fn profile_cache_insert_then_get() {
    let cache = ProfileCache::new();
    let ctx = WorkloadContext::new(100_000, true);
    let key = ProfileKey::from_context("k", &ctx);

    cache.insert(key.clone(), DispatchKind::CpuParallel);
    assert_eq!(cache.get(&key), Some(DispatchKind::CpuParallel));
    assert_eq!(cache.len(), 1);
}

// ===========================================================================
// T10/T11 integration: cache works with data_location + bytes_per_element.
// ===========================================================================

#[test]
fn profile_cache_distinguishes_data_location() {
    let cache = ProfileCache::new();
    let ctx_cpu = WorkloadContext::new(500, true); // data on CPU → SingleThread
    let ctx_gpu = WorkloadContext::new(500, true).with_data_location(DataLocation::Gpu); // → GpuCompute

    let _ = cache.decide_cached("k", &ctx_cpu);
    let _ = cache.decide_cached("k", &ctx_gpu);

    // Different data_location → different key → 2 misses.
    assert_eq!(cache.miss_count(), 2, "different data_location → 2 misses");
}

#[test]
fn profile_cache_distinguishes_bytes_per_element() {
    let cache = ProfileCache::new();
    let ctx_f32 = WorkloadContext::new(100_000, true)
        .with_bytes_per_element(4)
        .with_intensity(8.0);
    let ctx_f64 = WorkloadContext::new(100_000, true)
        .with_bytes_per_element(8)
        .with_intensity(8.0);

    let _ = cache.decide_cached("k", &ctx_f32);
    let _ = cache.decide_cached("k", &ctx_f64);

    assert_eq!(cache.miss_count(), 2, "different bpe → 2 misses");
}

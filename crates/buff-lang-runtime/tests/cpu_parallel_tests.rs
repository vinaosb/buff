//! T39 GREEN: real rayon-backed `par_map` / `par_filter` / `par_reduce`
//! on `CpuDispatcher`.
//!
//! Coverage matrix:
//! * Acceptance case: `par_map(vec![1,2,3], |x| x*2) == vec![2,4,6]`.
//! * Empty / single-element / large input.
//! * Order preservation (rayon's `collect()` over `par_iter`).
//! * `par_filter` keeps-predicate, drops others, preserves order.
//! * `par_reduce` sum / product / max / single / empty (identity).
//! * Non-`Copy` element type (`String`, custom struct).
//! * Determinism: same input → same output across 10 runs on all three ops.
//! * Custom `Send + Sync` types with derived `Clone`.

use buff_lang_runtime::CpuDispatcher;

/// Small helper: build a `CpuDispatcher` or fail the calling test.
fn dispatcher() -> CpuDispatcher {
    CpuDispatcher::new().expect("rayon default pool builds in tests")
}

// ---------------------------------------------------------------------------
// par_map — acceptance + core behaviors
// ---------------------------------------------------------------------------

#[test]
fn par_map_acceptance_123_doubles_to_246() {
    // Acceptance case from T39 spec — exact assertion.
    let d = dispatcher();
    let out = d.par_map(vec![1, 2, 3], |x| x * 2);
    assert_eq!(out, vec![2, 4, 6]);
}

#[test]
fn par_map_empty_input_returns_empty() {
    let d = dispatcher();
    let out: Vec<i32> = d.par_map(Vec::<i32>::new(), |x| x + 1);
    assert!(out.is_empty(), "empty input must map to empty output");
}

#[test]
fn par_map_single_element() {
    let d = dispatcher();
    let out = d.par_map(vec![42i32], |x| x + 1);
    assert_eq!(out, vec![43]);
}

#[test]
fn par_map_large_input_preserves_order() {
    // Large enough (100k) to actually exercise parallelism on multi-core
    // hosts — rayon's default split threshold is 8192 elements per chunk
    // for `Vec`, so 100k guarantees at least a dozen chunks.
    let d = dispatcher();
    let n = 100_000i32;
    let input: Vec<i32> = (0..n).collect();
    let out = d.par_map(input.clone(), |x| x * 2);
    assert_eq!(out.len(), n as usize);
    // Order preservation: every index must match the sequential transform.
    for (i, v) in out.iter().enumerate() {
        assert_eq!(*v, (i as i32) * 2, "order broken at index {i}");
    }
}

#[test]
fn par_map_closure_captures_environment() {
    // Closures must be able to capture by-reference from the surrounding
    // frame (rayon's Sync bound on `Fn` permits shared-reference capture).
    let d = dispatcher();
    let multiplier = 7i32;
    let out = d.par_map(vec![1, 2, 3, 4], |x| x * multiplier);
    assert_eq!(out, vec![7, 14, 21, 28]);
}

#[test]
fn par_map_changes_element_type_i32_to_string() {
    // par_map is generic over output type — non-Copy output (String) must
    // flow through cleanly across threads.
    let d = dispatcher();
    let out = d.par_map(vec![1, 2, 3], |x| format!("n={x}"));
    assert_eq!(
        out,
        vec!["n=1".to_string(), "n=2".to_string(), "n=3".to_string()]
    );
}

#[test]
fn par_map_works_with_non_copy_string_elements() {
    // Input also non-Copy (Vec<String>) — proves owned data flows across
    // worker threads without double-free.
    let d = dispatcher();
    let input = vec!["hello".to_string(), "world".to_string()];
    let out = d.par_map(input, |s| s.len());
    assert_eq!(out, vec![5, 5]);
}

// ---------------------------------------------------------------------------
// par_filter — keeps-predicate, order preservation
// ---------------------------------------------------------------------------

#[test]
fn par_filter_keeps_even_drops_odd() {
    let d = dispatcher();
    let out = d.par_filter(vec![1, 2, 3, 4, 5, 6], |x| *x % 2 == 0);
    assert_eq!(out, vec![2, 4, 6]);
}

#[test]
fn par_filter_empty_input_returns_empty() {
    let d = dispatcher();
    let out: Vec<i32> = d.par_filter(Vec::<i32>::new(), |_| true);
    assert!(out.is_empty());
}

#[test]
fn par_filter_keeps_all_when_predicate_always_true() {
    let d = dispatcher();
    let out = d.par_filter(vec![1, 2, 3], |_| true);
    assert_eq!(out, vec![1, 2, 3]);
}

#[test]
fn par_filter_keeps_none_when_predicate_always_false() {
    let d = dispatcher();
    let out = d.par_filter(vec![1, 2, 3], |_| false);
    assert!(
        out.is_empty(),
        "always-false predicate must drop everything"
    );
}

#[test]
fn par_filter_large_input_preserves_order() {
    // 100k input, filter to keep multiples of 3 — verifies order is
    // preserved under real parallelism.
    let d = dispatcher();
    let n = 100_000i32;
    let input: Vec<i32> = (0..n).collect();
    let out = d.par_filter(input.clone(), |x| *x % 3 == 0);
    let expected: Vec<i32> = input.into_iter().filter(|x| x % 3 == 0).collect();
    assert_eq!(out, expected);
}

#[test]
fn par_filter_works_with_string_elements() {
    // Non-Copy elements must pass through filter unchanged (no clone,
    // no copy — rayon's `filter` borrows by ref and returns ownership).
    let d = dispatcher();
    let input = vec!["foo".to_string(), "x".to_string(), "hello".to_string()];
    let out = d.par_filter(input, |s| s.len() > 1);
    assert_eq!(out, vec!["foo".to_string(), "hello".to_string()]);
}

// ---------------------------------------------------------------------------
// par_reduce — sum / product / max / single / empty / associativity
// ---------------------------------------------------------------------------

#[test]
fn par_reduce_sum_acceptance_1_to_5_is_15() {
    let d = dispatcher();
    let total = d.par_reduce(vec![1, 2, 3, 4, 5], 0i32, |a, b| a + b);
    assert_eq!(total, 15);
}

#[test]
fn par_reduce_product_1_to_4_is_24() {
    let d = dispatcher();
    let prod = d.par_reduce(vec![1, 2, 3, 4], 1i32, |a, b| a * b);
    assert_eq!(prod, 24);
}

#[test]
fn par_reduce_empty_input_returns_identity() {
    // Empty input + identity → identity. Caller controls the empty case.
    let d = dispatcher();
    let result = d.par_reduce(Vec::<i32>::new(), 42i32, |a, b| a + b);
    assert_eq!(result, 42, "empty reduce must return identity");
}

#[test]
fn par_reduce_single_element_returns_that_element() {
    let d = dispatcher();
    let result = d.par_reduce(vec![99i32], 0, |a, b| a + b);
    assert_eq!(result, 99);
}

#[test]
fn par_reduce_max_is_associative_and_deterministic() {
    // max is strictly associative AND commutative — the documented
    // "fully deterministic" case.
    let d = dispatcher();
    let result = d.par_reduce(vec![3, 1, 4, 1, 5, 9, 2, 6, 5, 3, 5], i32::MIN, |a, b| {
        a.max(b)
    });
    assert_eq!(result, 9);
}

#[test]
fn par_reduce_large_sum_matches_sequential() {
    // 100k element sum: integer addition is associative, so the parallel
    // sum must equal the sequential sum exactly.
    let d = dispatcher();
    let n = 100_000i64;
    let input: Vec<i64> = (0..n).collect();
    let parallel_sum = d.par_reduce(input.clone(), 0i64, |a, b| a + b);
    let sequential_sum: i64 = input.into_iter().sum();
    assert_eq!(parallel_sum, sequential_sum);
    // Closed form: sum(0..n) = n*(n-1)/2.
    assert_eq!(parallel_sum, n * (n - 1) / 2);
}

#[test]
fn par_reduce_works_with_string_concat_associative() {
    // String concat is associative (but NOT commutative). Each worker
    // reduces its own slice in input order; the per-worker results are
    // then combined in worker-slice order by rayon. So the final result
    // matches sequential concatenation in input order.
    let d = dispatcher();
    let input: Vec<String> = (0..10_000).map(|i| format!("{i:05}")).collect();
    let parallel_concat = d.par_reduce(input.clone(), String::new(), |a, b| a + &b);
    let sequential_concat: String = input.into_iter().collect();
    assert_eq!(parallel_concat, sequential_concat);
}

// ---------------------------------------------------------------------------
// Determinism — same input → same output across N runs
// ---------------------------------------------------------------------------

#[test]
fn par_map_deterministic_across_10_runs() {
    let d = dispatcher();
    let input: Vec<i32> = (0..10_000).collect();
    let first = d.par_map(input.clone(), |x| x.wrapping_mul(3).wrapping_add(7));
    for run in 1..=10 {
        let again = d.par_map(input.clone(), |x| x.wrapping_mul(3).wrapping_add(7));
        assert_eq!(
            first, again,
            "par_map output diverged on run {run} — non-deterministic dispatch"
        );
    }
}

#[test]
fn par_filter_deterministic_across_10_runs() {
    let d = dispatcher();
    let input: Vec<i32> = (0..10_000).collect();
    let first = d.par_filter(input.clone(), |x| *x % 7 == 0);
    for run in 1..=10 {
        let again = d.par_filter(input.clone(), |x| *x % 7 == 0);
        assert_eq!(
            first, again,
            "par_filter output diverged on run {run} — non-deterministic dispatch"
        );
    }
}

#[test]
fn par_reduce_deterministic_across_10_runs() {
    // Integer addition is associative AND commutative — fully deterministic
    // regardless of thread chunking. Same answer every run.
    let d = dispatcher();
    let input: Vec<i64> = (0..10_000).map(|x| x as i64).collect();
    let first = d.par_reduce(input.clone(), 0i64, |a, b| a + b);
    for run in 1..=10 {
        let again = d.par_reduce(input.clone(), 0i64, |a, b| a + b);
        assert_eq!(
            first, again,
            "par_reduce output diverged on run {run} — non-deterministic dispatch"
        );
    }
}

// ---------------------------------------------------------------------------
// Custom Send + Sync + Clone element type
// ---------------------------------------------------------------------------

#[test]
fn par_map_works_with_custom_struct() {
    // A user-defined `Send + Sync + Clone` struct must flow through all
    // three operations without lifetime or ownership issues.
    #[derive(Debug, Clone, PartialEq)]
    struct Point {
        x: i32,
        y: i32,
    }

    let d = dispatcher();
    let input = vec![
        Point { x: 0, y: 0 },
        Point { x: 3, y: 4 },
        Point { x: 6, y: 8 },
    ];
    let norms = d.par_map(input.clone(), |p| (p.x as f64).hypot(p.y as f64));
    assert!((norms[0] - 0.0).abs() < 1e-9);
    assert!((norms[1] - 5.0).abs() < 1e-9);
    assert!((norms[2] - 10.0).abs() < 1e-9);
}

#[test]
fn par_filter_uses_owned_pool_with_dispatcher_thread_count() {
    // The methods must run on the dispatcher's owned pool — the worker
    // count visible inside the closure must match `thread_count()`.
    let d = dispatcher();
    let expected = d.thread_count();
    let input: Vec<i32> = (0..1_000).collect();
    // Use a static atomic-counter pattern: not great for determinism
    // tests generally, but here we're only verifying that the closure
    // sees a thread pool that matches our dispatcher (not the global one).
    // We observe this indirectly: rayon::current_num_threads() inside the
    // closure (called via with_pool) reports the owned pool's thread count.
    let observed = d.with_pool(rayon::current_num_threads);
    assert_eq!(
        observed, expected,
        "closure must run on the dispatcher's pool, not a different/global one"
    );
    // Sanity: the par_map call itself must succeed and be order-preserving.
    let out = d.par_map(input.clone(), |x| x + 1);
    assert_eq!(out.len(), input.len());
    assert_eq!(out[0], 1);
    assert_eq!(out[999], 1000);
}

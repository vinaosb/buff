//! T38 RED: CpuDispatcher::new(), thread count, Dispatcher impl.

use buff_lang_runtime::{CpuDispatcher, CpuDispatcherError, DispatchKind, Dispatcher};

#[test]
fn test_cpu_dispatcher_new_returns_ok() {
    let dispatcher = CpuDispatcher::new();
    assert!(
        dispatcher.is_ok(),
        "default rayon pool must build: {:?}",
        dispatcher.err()
    );
}

#[test]
fn test_cpu_dispatcher_thread_count_is_positive() {
    let dispatcher = CpuDispatcher::new().expect("rayon default pool builds in tests");
    assert!(
        dispatcher.thread_count() >= 1,
        "thread_count must be >= 1, got {}",
        dispatcher.thread_count()
    );
}

#[test]
fn test_cpu_dispatcher_kind_is_cpu_parallel() {
    let dispatcher = CpuDispatcher::new().expect("test pool");
    let d: &dyn Dispatcher = &dispatcher;
    assert_eq!(d.kind(), DispatchKind::CpuParallel);
}

#[test]
fn test_cpu_dispatcher_parallelism_matches_thread_count() {
    let dispatcher = CpuDispatcher::new().expect("test pool");
    let d: &dyn Dispatcher = &dispatcher;
    assert_eq!(d.parallelism(), dispatcher.thread_count());
}

#[test]
fn test_cpu_dispatcher_does_not_support_gpu() {
    let dispatcher = CpuDispatcher::new().expect("test pool");
    let d: &dyn Dispatcher = &dispatcher;
    assert!(!d.supports_gpu());
}

#[test]
fn test_cpu_dispatcher_error_is_debug_and_display() {
    // CpuDispatcherError must impl Display + Debug (via thiserror). We only
    // verify the trait bounds at type level — actually fabricating a
    // rayon::ThreadPoolBuildError requires actually breaking rayon's pool
    // build, which is impractical. T39 will add richer error variants with
    // direct constructors.
    fn assert_trait_bounds<T: std::fmt::Display + std::fmt::Debug + std::error::Error>() {}
    assert_trait_bounds::<CpuDispatcherError>();
}

#[test]
fn test_cpu_dispatcher_with_pool_runs_closure_on_owned_pool() {
    // T39 will use this to scope par_iter; T38 just verifies the API exists
    // and runs the closure once.
    let dispatcher = CpuDispatcher::new().expect("test pool");
    let marker = std::sync::Arc::new(std::sync::Mutex::new(0u32));
    let marker_clone = marker.clone();
    let result = dispatcher.with_pool(move || {
        *marker_clone.lock().expect("lock in test") = 42;
        99
    });
    assert_eq!(result, 99);
    assert_eq!(*marker.lock().expect("lock in test"), 42);
}

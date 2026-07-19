//! T38 RED: DispatchKind enum + Dispatcher trait object-safety.

use buff_lang_runtime::{DispatchKind, Dispatcher};

#[test]
fn test_dispatch_kind_variants_ordering() {
    // Order matters: SingleThread < CpuParallel < GpuCompute (T40 thresholds).
    let kinds = [
        DispatchKind::SingleThread,
        DispatchKind::CpuParallel,
        DispatchKind::GpuCompute,
    ];
    // Ensure all three variants exist and are distinct.
    assert_eq!(kinds.len(), 3);
    assert_ne!(kinds[0], kinds[1]);
    assert_ne!(kinds[1], kinds[2]);
    assert_ne!(kinds[0], kinds[2]);
}

#[test]
fn test_dispatch_kind_clone_copy_partial_eq() {
    let a = DispatchKind::CpuParallel;
    let b = a; // Copy
    assert_eq!(a, b);
    // DispatchKind is Copy — no need for clone. Verify Copy by reusing after move.
    let c = b;
    assert_eq!(a, c);
}

#[test]
fn test_dispatch_kind_debug_format() {
    let rendered = format!("{:?}", DispatchKind::GpuCompute);
    assert_eq!(rendered, "GpuCompute");
}

#[test]
fn test_dispatcher_trait_object_safe() {
    // If this compiles, the trait is object-safe (no generics on methods,
    // no Sized supertrait bounds that would break `dyn Dispatcher`).
    fn accepts_dyn(d: &dyn Dispatcher) -> (DispatchKind, usize, bool) {
        (d.kind(), d.parallelism(), d.supports_gpu())
    }
    // A trivially-empty stub dispatcher to prove the trait shape works.
    #[derive(Debug)]
    struct StubDispatcher;
    impl Dispatcher for StubDispatcher {
        fn kind(&self) -> DispatchKind {
            DispatchKind::SingleThread
        }
        fn parallelism(&self) -> usize {
            1
        }
    }
    let stub = StubDispatcher;
    let (kind, par, gpu) = accepts_dyn(&stub);
    assert_eq!(kind, DispatchKind::SingleThread);
    assert_eq!(par, 1);
    assert!(!gpu); // default impl returns false
}

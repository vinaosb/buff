use buff_reactive::{Computed, Effect, Signal};

#[test]
fn signal_new_get_set_roundtrip() {
    let s = Signal::new(10);
    assert_eq!(s.get(), 10);
    s.set(20);
    assert_eq!(s.get(), 20);
}

#[test]
fn signal_update_applies_fn_and_notifies() {
    let s = Signal::new(5);
    let count = Signal::new(0);
    Effect::new({
        let s = s.clone();
        let count = count.clone();
        move || {
            let _ = s.get();
            count.set(count.get() + 1);
        }
    });
    assert_eq!(count.get(), 1);
    s.update(|v| *v += 100).unwrap();
    assert_eq!(s.get(), 105);
    assert_eq!(count.get(), 2);
}

#[test]
fn signal_default_uses_type_default() {
    let s: Signal<i32> = Signal::default();
    assert_eq!(s.get(), 0);
}

#[test]
fn signal_clone_shares_underlying_storage() {
    let s1 = Signal::new(42);
    let s2 = s1.clone();
    s2.set(99);
    assert_eq!(s1.get(), 99);
    assert!(s1 == s2);
}

#[test]
fn computed_caches_until_deps_change() {
    let a = Signal::new(2);
    let b = Signal::new(3);
    let call_count = Signal::new(0);

    let sum: Computed<i64> = Computed::new({
        let a = a.clone();
        let b = b.clone();
        let call_count = call_count.clone();
        move || {
            call_count.set(call_count.get() + 1);
            a.get() + b.get()
        }
    });

    assert_eq!(sum.get(), 5);
    assert_eq!(call_count.get(), 1);
    assert_eq!(sum.get(), 5);
    assert_eq!(call_count.get(), 1);

    a.set(10);
    assert_eq!(sum.get(), 13);
    assert_eq!(call_count.get(), 2);
}

#[test]
fn effect_runs_once_on_creation() {
    let runs = Signal::new(0);
    let source = Signal::new(0);
    Effect::new({
        let source = source.clone();
        let runs = runs.clone();
        move || {
            let _ = source.get();
            runs.set(runs.get() + 1);
        }
    });
    assert_eq!(runs.get(), 1);
}

#[test]
fn effect_reruns_on_dependency_change() {
    let runs = Signal::new(0);
    let source = Signal::new(0);
    Effect::new({
        let source = source.clone();
        let runs = runs.clone();
        move || {
            let v = source.get();
            runs.set(runs.get() + 1 + v);
        }
    });
    assert_eq!(runs.get(), 1);
    source.set(10);
    assert_eq!(runs.get(), 12);
    source.set(100);
    assert_eq!(runs.get(), 113);
}

#[test]
fn effect_chains_through_computed() {
    let src = Signal::new(1);
    let doubled: Computed<i64> = Computed::new({
        let src = src.clone();
        move || src.get() * 2
    });
    let sink = Signal::new(0);
    Effect::new({
        let doubled = doubled.clone();
        let sink = sink.clone();
        move || {
            sink.set(doubled.get());
        }
    });
    assert_eq!(sink.get(), 2);
    src.set(5);
    assert_eq!(sink.get(), 10);
    src.set(0);
    assert_eq!(sink.get(), 0);
}

#[test]
fn batch_defers_notifications() {
    let src = Signal::new(0);
    let runs = Signal::new(0);
    Effect::new({
        let src = src.clone();
        let runs = runs.clone();
        move || {
            let _ = src.get();
            runs.set(runs.get() + 1);
        }
    });
    assert_eq!(runs.get(), 1);

    buff_reactive::batch({
        let src = src.clone();
        move || {
            src.set(1);
            src.set(2);
            src.set(3);
        }
    });
    assert_eq!(runs.get(), 2);
}

#[test]
fn nested_batch_runs_once_at_outer_exit() {
    let src = Signal::new(0);
    let runs = Signal::new(0);
    Effect::new({
        let src = src.clone();
        let runs = runs.clone();
        move || {
            let _ = src.get();
            runs.set(runs.get() + 1);
        }
    });

    buff_reactive::batch({
        let src = src.clone();
        let runs = runs.clone();
        move || {
            src.set(10);
            buff_reactive::batch({
                let src = src.clone();
                move || {
                    src.set(20);
                }
            });
            let _ = runs.get();
        }
    });
    assert_eq!(runs.get(), 2);
}

#[test]
fn no_observer_get_does_not_register_subscriber() {
    let s = Signal::new(7);
    let _ = s.get();
    assert_eq!(s.subscriber_count(), 0);
}

#[test]
fn computed_invalidate_clears_cache() {
    let src = Signal::new(10);
    let c: Computed<i64> = Computed::new({
        let src = src.clone();
        move || src.get() + 1
    });
    assert_eq!(c.get(), 11);
    c.invalidate();
    assert_eq!(c.get(), 11);
}

#[test]
fn signal_equality_by_identity() {
    let s1 = Signal::new(5);
    let s2 = s1.clone();
    let s3 = Signal::new(5);
    assert!(s1 == s2);
    assert!(s1 != s3);
}

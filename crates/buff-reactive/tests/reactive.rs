use buff_reactive::{batch, Computed, Effect, Signal};

#[test]
fn diamond_dependency_single_recompute() {
    let src = Signal::new(1);
    let a: Computed<i64> = {
        let src = src.clone();
        Computed::new(move || src.get() * 2)
    };
    let b: Computed<i64> = {
        let src = src.clone();
        Computed::new(move || src.get() * 3)
    };
    let call_count = Signal::new(0);
    let sum: Computed<i64> = {
        let a = a.clone();
        let b = b.clone();
        let call_count = call_count.clone();
        Computed::new(move || {
            call_count.set(call_count.get() + 1);
            a.get() + b.get()
        })
    };

    assert_eq!(sum.get(), 5);
    assert_eq!(call_count.get(), 1);
    assert_eq!(sum.get(), 5);
    assert_eq!(call_count.get(), 1);

    src.set(10);
    assert_eq!(sum.get(), 50);
    assert_eq!(call_count.get(), 2);
}

#[test]
fn multiple_effects_on_same_signal() {
    let src = Signal::new(0);
    let runs_a = Signal::new(0);
    let runs_b = Signal::new(0);

    Effect::new({
        let src = src.clone();
        let runs_a = runs_a.clone();
        move || {
            let _ = src.get();
            runs_a.set(runs_a.get() + 1);
        }
    });
    Effect::new({
        let src = src.clone();
        let runs_b = runs_b.clone();
        move || {
            let _ = src.get();
            runs_b.set(runs_b.get() + 1);
        }
    });

    assert_eq!(runs_a.get(), 1);
    assert_eq!(runs_b.get(), 1);
    src.set(42);
    assert_eq!(runs_a.get(), 2);
    assert_eq!(runs_b.get(), 2);
}

#[test]
fn chained_computed_propagates_change() {
    let base = Signal::new(10);
    let step1: Computed<i64> = {
        let base = base.clone();
        Computed::new(move || base.get() + 1)
    };
    let step2: Computed<i64> = {
        let step1 = step1.clone();
        Computed::new(move || step1.get() * 2)
    };
    let step3: Computed<i64> = {
        let step2 = step2.clone();
        Computed::new(move || step2.get() - 5)
    };

    assert_eq!(step3.get(), 17);
    base.set(20);
    assert_eq!(step3.get(), 37);
}

#[test]
fn batch_no_change_no_notification() {
    let src = Signal::new(5);
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

    batch(|| {});
    assert_eq!(runs.get(), 1);
}

#[test]
fn effect_with_conditional_dependency_tracks_only_taken_branch() {
    let flag = Signal::new(true);
    let a = Signal::new(10);
    let b = Signal::new(20);
    let last = Signal::new(0);

    Effect::new({
        let flag = flag.clone();
        let a = a.clone();
        let b = b.clone();
        let last = last.clone();
        move || {
            let v = if flag.get() { a.get() } else { b.get() };
            last.set(v);
        }
    });

    assert_eq!(last.get(), 10);
    b.set(99);
    assert_eq!(last.get(), 10);
    a.set(50);
    assert_eq!(last.get(), 50);

    flag.set(false);
    assert_eq!(last.get(), 99);
}

#[test]
fn string_signals_work() {
    let name = Signal::new(String::from("Alice"));
    let greeting: Computed<String> = {
        let name = name.clone();
        Computed::new(move || format!("Hello, {}!", name.get()))
    };
    assert_eq!(greeting.get(), "Hello, Alice!");
    name.set(String::from("Bob"));
    assert_eq!(greeting.get(), "Hello, Bob!");
}

#[test]
fn vector_signals_work() {
    let list = Signal::new(vec![1, 2, 3]);
    let length: Computed<usize> = {
        let list = list.clone();
        Computed::new(move || list.get().len())
    };
    assert_eq!(length.get(), 3);
    list.update(|v| v.push(4)).unwrap();
    assert_eq!(length.get(), 4);
}

#[test]
fn effect_can_be_run_manually() {
    let runs = Signal::new(0);
    let e = Effect::new({
        let runs = runs.clone();
        move || {
            runs.set(runs.get() + 1);
        }
    });
    assert_eq!(runs.get(), 1);
    e.run();
    assert_eq!(runs.get(), 2);
    e.run();
    assert_eq!(runs.get(), 3);
}

#[test]
fn computed_clone_shares_storage() {
    let src = Signal::new(7);
    let c1: Computed<i64> = {
        let src = src.clone();
        Computed::new(move || src.get() * 10)
    };
    let c2 = c1.clone();
    assert_eq!(c1.get(), 70);
    assert_eq!(c2.get(), 70);
    assert!(c1 == c2);
}

#[test]
fn deeply_nested_batch_dedups_correctly() {
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

    let src_outer = src.clone();
    batch(move || {
        let src_inner = src_outer.clone();
        batch(move || {
            src_inner.set(1);
        });
        src_outer.set(2);
    });
    assert_eq!(runs.get(), 2);
}

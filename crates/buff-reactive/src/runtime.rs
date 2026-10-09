use std::cell::RefCell;
use std::rc::Rc;

pub type Callback = Rc<dyn Fn()>;

thread_local! {
    static OBSERVER_STACK: RefCell<Vec<Callback>> = const { RefCell::new(Vec::new()) };
    static PENDING: RefCell<Vec<Callback>> = const { RefCell::new(Vec::new()) };
    static BATCH_DEPTH: RefCell<usize> = const { RefCell::new(0) };
    static RUNNING: RefCell<Vec<Callback>> = const { RefCell::new(Vec::new()) };
}

pub fn current_observer() -> Option<Callback> {
    OBSERVER_STACK.with(|s| s.borrow().last().cloned())
}

pub fn with_observer<F: FnOnce()>(observer: Callback, body: F) {
    OBSERVER_STACK.with(|s| s.borrow_mut().push(observer));
    body();
    OBSERVER_STACK.with(|s| {
        s.borrow_mut().pop();
    });
}

// Re-entrancy guard (ITER-04). Root cause of the former stack overflow: an
// effect body like `count.set(count.get() + 1)` first subscribes the effect's
// callback to `count` (via `get`), then `set` synchronously notifies that same
// callback, which re-runs and sets again -> unbounded synchronous recursion ->
// STATUS_STACK_OVERFLOW. Fix: track the callbacks currently executing and DROP
// any notification aimed at one of them. Dropping (not deferring) is the
// chosen semantics: the running callback already reads fresh values for any
// signal it reads after its own write, so a synchronous re-run would be either
// redundant or an unbounded write/read cycle. Applies uniformly to effects
// and computed invalidation callbacks.
fn is_running(callback: &Callback) -> bool {
    RUNNING.with(|r| r.borrow().iter().any(|cb| Rc::ptr_eq(cb, callback)))
}

struct RunningPop;

impl Drop for RunningPop {
    fn drop(&mut self) {
        RUNNING.with(|r| {
            r.borrow_mut().pop();
        });
    }
}

/// Run `body` with `callback` marked as executing and as the active observer.
/// Notifications scheduled for `callback` while it executes are dropped by
/// `schedule` and `batch`. The `RunningPop` guard keeps `RUNNING` consistent
/// if `body` panics.
pub fn run_scoped<F: FnOnce()>(callback: &Callback, body: F) {
    RUNNING.with(|r| r.borrow_mut().push(Rc::clone(callback)));
    let _pop = RunningPop;
    with_observer(Rc::clone(callback), body);
}

fn run_notification(callback: Callback) {
    if !is_running(&callback) {
        run_scoped(&callback, || callback());
    }
}

pub fn schedule(callbacks: Vec<Callback>) {
    BATCH_DEPTH.with(|d| {
        if *d.borrow() > 0 {
            PENDING.with(|p| {
                let mut slot = p.borrow_mut();
                for cb in &callbacks {
                    if !slot.iter().any(|existing| Rc::ptr_eq(existing, cb)) {
                        slot.push(Rc::clone(cb));
                    }
                }
            });
        } else {
            for cb in callbacks {
                run_notification(cb);
            }
        }
    });
}

pub fn batch<F: FnOnce()>(body: F) {
    BATCH_DEPTH.with(|d| *d.borrow_mut() += 1);
    body();
    let depth = BATCH_DEPTH.with(|d| {
        *d.borrow_mut() -= 1;
        *d.borrow()
    });
    if depth == 0 {
        let pending: Vec<Callback> = PENDING.with(|p| p.borrow_mut().drain(..).collect());
        for cb in pending {
            run_notification(cb);
        }
    }
}

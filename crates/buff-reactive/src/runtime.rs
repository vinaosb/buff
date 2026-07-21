use std::cell::RefCell;
use std::rc::Rc;

pub type Callback = Rc<dyn Fn()>;

thread_local! {
    static OBSERVER_STACK: RefCell<Vec<Callback>> = const { RefCell::new(Vec::new()) };
    static PENDING: RefCell<Vec<Callback>> = const { RefCell::new(Vec::new()) };
    static BATCH_DEPTH: RefCell<usize> = const { RefCell::new(0) };
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
                let cb_for_observer = Rc::clone(&cb);
                with_observer(cb_for_observer, || cb());
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
            let cb_for_observer = Rc::clone(&cb);
            with_observer(cb_for_observer, || cb());
        }
    }
}

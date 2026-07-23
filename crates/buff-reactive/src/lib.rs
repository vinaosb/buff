mod error;
mod runtime;

pub use error::{ReactiveError, Result};
pub use runtime::batch;

use std::cell::RefCell;
use std::fmt;
use std::rc::Rc;

pub type Callback = Rc<dyn Fn()>;

#[derive(Clone)]
pub struct Signal<T> {
    inner: Rc<RefCell<SignalInner<T>>>,
}

struct SignalInner<T> {
    value: T,
    subscribers: Vec<Callback>,
}

impl<T> Signal<T> {
    #[must_use]
    pub fn new(value: T) -> Self {
        Self {
            inner: Rc::new(RefCell::new(SignalInner {
                value,
                subscribers: Vec::new(),
            })),
        }
    }

    pub fn get(&self) -> T
    where
        T: Clone,
    {
        if let Some(observer) = runtime::current_observer() {
            let mut slot = self.inner.borrow_mut();
            if !slot.subscribers.iter().any(|cb| Rc::ptr_eq(cb, &observer)) {
                slot.subscribers.push(observer);
            }
        }
        self.inner.borrow().value.clone()
    }

    pub fn set(&self, value: T) {
        {
            let mut slot = self.inner.borrow_mut();
            slot.value = value;
        }
        let subs: Vec<Callback> = self.inner.borrow().subscribers.iter().cloned().collect();
        runtime::schedule(subs);
    }

    pub fn update<F>(&self, f: F) -> Result<()>
    where
        F: FnOnce(&mut T),
    {
        {
            let mut slot = self.inner.borrow_mut();
            f(&mut slot.value);
        }
        let subs: Vec<Callback> = self.inner.borrow().subscribers.iter().cloned().collect();
        runtime::schedule(subs);
        Ok(())
    }

    pub fn subscriber_count(&self) -> usize {
        self.inner.borrow().subscribers.len()
    }
}

impl<T: fmt::Debug> fmt::Debug for Signal<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let inner = self.inner.borrow();
        f.debug_struct("Signal")
            .field("value", &inner.value)
            .field("subscribers", &inner.subscribers.len())
            .finish()
    }
}

impl<T> PartialEq for Signal<T> {
    fn eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.inner, &other.inner)
    }
}

impl<T: Eq> Eq for Signal<T> {}

impl<T: Default> Default for Signal<T> {
    fn default() -> Self {
        Self::new(T::default())
    }
}

pub struct Computed<T> {
    cell: Rc<RefCell<ComputedCell<T>>>,
    compute: Rc<dyn Fn() -> T>,
    invalidate_cb: Callback,
}

struct ComputedCell<T> {
    cached: Option<T>,
    subscribers: Vec<Callback>,
}

impl<T: Clone + 'static> Computed<T> {
    #[must_use]
    pub fn new<F>(compute: F) -> Self
    where
        F: Fn() -> T + 'static,
    {
        let compute: Rc<dyn Fn() -> T> = Rc::new(compute);
        let cell: Rc<RefCell<ComputedCell<T>>> = Rc::new(RefCell::new(ComputedCell {
            cached: None,
            subscribers: Vec::new(),
        }));

        let cell_for_cb = Rc::clone(&cell);
        let invalidate_cb: Callback = Rc::new(move || {
            let subs: Vec<Callback> = {
                let mut slot = cell_for_cb.borrow_mut();
                slot.cached = None;
                slot.subscribers.iter().cloned().collect()
            };
            runtime::schedule(subs);
        });

        let cb_for_init = Rc::clone(&invalidate_cb);
        let compute_for_init = Rc::clone(&compute);
        let cell_for_init = Rc::clone(&cell);
        runtime::with_observer(cb_for_init, || {
            let value = compute_for_init();
            cell_for_init.borrow_mut().cached = Some(value);
        });

        Self {
            cell,
            compute,
            invalidate_cb,
        }
    }

    pub fn get(&self) -> T {
        if let Some(observer) = runtime::current_observer() {
            let mut slot = self.cell.borrow_mut();
            if !slot.subscribers.iter().any(|cb| Rc::ptr_eq(cb, &observer)) {
                slot.subscribers.push(observer);
            }
        }
        if let Some(v) = self.cell.borrow().cached.clone() {
            return v;
        }
        let cb = Rc::clone(&self.invalidate_cb);
        let compute = Rc::clone(&self.compute);
        let cell = Rc::clone(&self.cell);
        runtime::with_observer(cb, || {
            let value = compute();
            cell.borrow_mut().cached = Some(value);
        });
        self.cell
            .borrow()
            .cached
            .clone()
            .unwrap_or_else(|| (self.compute)())
    }

    pub fn invalidate(&self) {
        self.cell.borrow_mut().cached = None;
        let subs: Vec<Callback> = self.cell.borrow().subscribers.iter().cloned().collect();
        runtime::schedule(subs);
    }
}

impl<T: fmt::Debug> fmt::Debug for Computed<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let cell = self.cell.borrow();
        f.debug_struct("Computed")
            .field("cached", &cell.cached)
            .field("subscribers", &cell.subscribers.len())
            .finish()
    }
}

impl<T> PartialEq for Computed<T> {
    fn eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.cell, &other.cell)
    }
}

impl<T: Eq> Eq for Computed<T> {}

impl<T: Clone + 'static> Clone for Computed<T> {
    fn clone(&self) -> Self {
        Self {
            cell: Rc::clone(&self.cell),
            compute: Rc::clone(&self.compute),
            invalidate_cb: Rc::clone(&self.invalidate_cb),
        }
    }
}

#[derive(Clone)]
pub struct Effect {
    callback: Callback,
}

impl Effect {
    #[must_use]
    pub fn new<F>(body: F) -> Self
    where
        F: Fn() + 'static,
    {
        let callback: Callback = Rc::new(body);
        let cb_for_observer = Rc::clone(&callback);
        runtime::with_observer(cb_for_observer, || {
            callback();
        });
        Self { callback }
    }

    pub fn run(&self) {
        (self.callback)();
    }
}

impl fmt::Debug for Effect {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Effect").finish_non_exhaustive()
    }
}

impl PartialEq for Effect {
    fn eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.callback, &other.callback)
    }
}

impl Eq for Effect {}

//! `buff-pubsub` — in-process event bus for the Buff language.
//!
//! Pure-Rust MVP built standalone on T2 Channel<T> semantics
//! (per T41 spec: "no extern needed"). Wraps
//! [`crossbeam-channel`](https://crates.io/crates/crossbeam-channel)
//! for the per-subscription queue + [`tokio`](https://crates.io/crates/tokio)
//! for runtime-aware worker spawning (sync-first API with graceful
//! fallback to `std::thread::spawn` when no tokio runtime is active).
//!
//! Distributed pub/sub (Redis NATS pubsub / Kafka / NATS / RabbitMQ
//! bridges) is **deferred to v1.18+** per the T41 task spec —
//! in-process only for the MVP.
//!
//! # Pipeline
//!
//! ```text
//!   EventBus.new() ──▶ EventBus { topic_map, next_id }
//!                          │
//!                          ├─ bus.subscribe(topic, handler) ──▶ SubscriptionId
//!                          │       │
//!                          │       └─ spawns worker:
//!                          │            loop { rx.recv() ─▶ handler(event) }
//!                          │
//!                          ├─ bus.publish(topic, payload)  ──▶ usize (delivered count)
//!                          │       │
//!                          │       └─ iterates topic's senders,
//!                          │          tx.send(event.clone()) per subscriber
//!                          │
//!                          ├─ bus.unsubscribe(id)  ──▶ drops sender, worker exits
//!                          │
//!                          ├─ bus.subscriber_count(topic) ──▶ usize
//!                          ├─ bus.topic_count()             ──▶ usize
//!                          └─ bus.clear()                   ──▶ drops all senders
//! ```
//!
//! # FFI safety
//!
//! Every public entry point follows the 6 hard rules from
//! `crates/buff-lang-ffi-guide/GUIDE.md`:
//!
//! | Rule | How this crate complies |
//! |------|-------------------------|
//! | R1 — No raw pointers | Public surface exposes only `EventBus`, `Event`, `SubscriptionId` (type alias for `u64`), `PubSubError`. No `*const` / `*mut`. |
//! | R2 — Ownership boundary | `subscribe` takes an owned `Fn` closure (boxed + Arc-shared). `publish` takes an owned `String` payload. `Event` is owned. |
//! | R3 — Error mapping | Fallible ops return `Result<T, PubSubError>`. `crossbeam_channel::SendError` mapped via `From`. |
//! | R4 — Thread safety | `EventBus` is `Send + Sync` (wraps `Arc<RwLock<HashMap<...>>>` + `Arc<AtomicU64>`). Handlers require `Fn + Send + Sync + 'static`. |
//! | R5 — Lifetime hiding | No public lifetime parameters. All references (`&str` topic args) are copied into owned `String` at the boundary. |
//! | R6 — Panic boundary | `new` / `subscribe` / `publish` / `unsubscribe` wrap their bodies in `catch_unwind`. |
//!
//! # Panic-free contract
//!
//! No `unwrap` / `expect` / `panic!` / `todo!` / `unimplemented!` in
//! non-test code. Channel send/recv return `Result` explicitly so the
//! only failure modes are user-visible (`Disconnected`,
//! `UnknownSubscription`, `EmptyTopic`) or runtime-internal (`Panic`).

pub mod error;

pub use error::PubSubError;

use crossbeam_channel as cb;
use std::collections::HashMap;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, RwLock};

/// Stable identifier for an active subscription.
///
/// Returned by [`EventBus::subscribe`], accepted by
/// [`EventBus::unsubscribe`]. Monotonically increasing per-bus;
/// recycled only after `unsubscribe` drops the sender. Stable for
/// the lifetime of the subscription — a `SubscriptionId` is valid
/// from the moment `subscribe` returns it until the matching
/// `unsubscribe` returns `Ok(())`.
pub type SubscriptionId = u64;

/// Internal record stored per-topic per-subscription.
///
/// The sender is the publish-side handle of a
/// `crossbeam_channel::unbounded::<Event>()` pair; the worker
/// spawned by `subscribe` holds the receiver. Dropping the sender
/// (via `unsubscribe` or `clear`) causes the worker's `recv()` to
/// return `Err(RecvError)` so the worker exits cleanly.
type SubRecord = (SubscriptionId, cb::Sender<Event>);

/// An event delivered to a subscriber.
///
/// Constructed by [`EventBus::publish`] internally and passed by
/// value to each subscriber's handler. Carries the topic it was
/// published to (so subscribers sharing one handler across topics
/// can dispatch on the source) plus the user payload as an owned
/// `String`.
///
/// Payload is a `String` (not bytes / structured values) because
/// the MVP surface mirrors the cross-language norm
/// (EventEmitter/eventbus/EventBus all default to string-or-json
/// payloads). A future typed-events API can extend this via a
/// generic `Event<T>` without breaking the string MVP.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Event {
    topic: String,
    payload: String,
}

impl Event {
    /// Construct a new event from owned topic + payload strings.
    ///
    /// Public so consumers building test fixtures (or the codegen
    /// layer lifting a `bus.subscribe` handler that re-emits on
    /// another bus) can construct events without going through
    /// `publish`. Equivalent to the constructor invoked internally
    /// by [`EventBus::publish`].
    pub fn new(topic: String, payload: String) -> Self {
        Event { topic, payload }
    }

    /// The topic this event was published to.
    ///
    /// Returns a borrowed `&str` so subscribers sharing one handler
    /// across topics can dispatch on the source without cloning.
    pub fn topic(&self) -> &str {
        &self.topic
    }

    /// The payload string carried by this event.
    ///
    /// Returns a borrowed `&str` for zero-cost inspection. For an
    /// owned version, callers can clone: `event.payload().to_string()`.
    pub fn payload(&self) -> &str {
        &self.payload
    }
}

impl std::fmt::Display for Event {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Event({}, {:?})", self.topic, self.payload)
    }
}

/// In-process pub/sub event bus.
///
/// Construct via [`EventBus::new`]. Each [`Self::subscribe`] call
/// registers a closure handler under a string topic and returns a
/// [`SubscriptionId`] for later unsubscribe. Each [`Self::publish`]
/// call clones the event to every active subscriber's queue; the
/// per-subscriber worker thread drains the queue and invokes the
/// handler.
///
/// `EventBus` is `Send + Sync` and cheap to clone (inner state is
/// behind an `Arc`); the recommended pattern is `let bus =
/// EventBus::new()?; let bus2 = bus.clone();` for cross-thread
/// sharing (mirrors the `Cache` precedent in buff-cache).
///
/// # Example
///
/// ```
/// use buff_pubsub::EventBus;
/// use std::sync::{Arc, Mutex};
///
/// let bus = EventBus::new().expect("bus");
/// let received = Arc::new(Mutex::new(Vec::<String>::new()));
/// let received_clone = received.clone();
/// let _id = bus.subscribe("greeting", move |event| {
///     received_clone.lock().expect("lock").push(event.payload().to_string());
/// }).expect("subscribe");
/// let delivered = bus.publish("greeting", "hello".to_string()).expect("publish");
/// assert_eq!(delivered, 1);
/// ```
#[derive(Clone)]
pub struct EventBus {
    inner: Arc<RwLock<HashMap<String, Vec<SubRecord>>>>,
    next_id: Arc<AtomicU64>,
}

impl EventBus {
    /// Construct an empty event bus.
    ///
    /// Wraps the (trivial) allocation in `catch_unwind` per T4 FFI
    /// guide R6. The MVP constructor cannot fail in normal use;
    /// the `Result` return mirrors the precedent set by
    /// `Cache::new` / `Image::new` so a future capacity / config
    /// knob slots in without breaking the surface.
    pub fn new() -> Result<Self, PubSubError> {
        let result = catch_unwind(AssertUnwindSafe(|| EventBus {
            inner: Arc::new(RwLock::new(HashMap::new())),
            next_id: Arc::new(AtomicU64::new(1)),
        }));
        match result {
            Ok(bus) => Ok(bus),
            Err(_) => Err(PubSubError::Panic),
        }
    }

    /// Register `handler` under `topic`. Returns the assigned
    /// [`SubscriptionId`] for later [`Self::unsubscribe`].
    ///
    /// Internally creates a `crossbeam_channel::unbounded::<Event>()`
    /// pair, stores the sender in the topic→subs map, and spawns a
    /// worker that loops `rx.recv() ─▶ handler(event)`. The worker
    /// exits cleanly when the sender is dropped (via `unsubscribe`
    /// or `clear`).
    ///
    /// Worker spawn uses `tokio::task::spawn_blocking` when a
    /// tokio runtime is active (so an async caller can `bus.subscribe`
    /// from inside a `spawn` block without panicking on "no runtime
    /// running"); falls back to `std::thread::spawn` for sync use.
    ///
    /// `topic` must be non-empty (returns [`PubSubError::EmptySubscribeTopic`]
    /// otherwise — `""` is reserved as the internal sentinel for
    /// "no topic").
    pub fn subscribe<F>(&self, topic: &str, handler: F) -> Result<SubscriptionId, PubSubError>
    where
        F: Fn(Event) + Send + Sync + 'static,
    {
        if topic.is_empty() {
            return Err(PubSubError::EmptySubscribeTopic);
        }
        let topic_owned = topic.to_string();
        // Wrap the registration + worker spawn in catch_unwind: a
        // panic in tokio handle lookup, channel creation, or thread
        // spawn must not cross the FFI boundary.
        let handler_arc: Arc<dyn Fn(Event) + Send + Sync> = Arc::new(handler);
        let result = catch_unwind(AssertUnwindSafe(|| {
            let (tx, rx) = cb::unbounded::<Event>();
            let id = self.next_id.fetch_add(1, Ordering::Relaxed);
            let record: SubRecord = (id, tx);
            {
                let mut map = self.inner.write().map_err(|_| PubSubError::Panic)?;
                map.entry(topic_owned.clone()).or_default().push(record);
            }
            // Worker: drain rx, invoke handler. Exits on disconnect
            // (sender dropped) — tx is held only inside the map, so
            // dropping it via unsubscribe/clear is the lifecycle.
            let worker_handler = handler_arc.clone();
            let worker_fn = move || {
                while let Ok(event) = rx.recv() {
                    // Catch panics from the user handler so one bad
                    // subscriber doesn't kill the worker thread (and
                    // silently drop future events). The caught panic
                    // is dropped (best-effort) and the loop continues
                    // — mirrors Node's EventEmitter 'error' event
                    // semantics where a throwing listener doesn't
                    // crash the emitter.
                    let _ = catch_unwind(AssertUnwindSafe(|| {
                        worker_handler(event);
                    }));
                }
            };
            // Prefer tokio's blocking pool when a runtime is active
            // (async-friendly: the worker is tracked + can be
            // awaited on shutdown via tokio::runtime::Handle). Fall
            // back to a raw std::thread for sync callers.
            match tokio::runtime::Handle::try_current() {
                Ok(handle) => {
                    handle.spawn_blocking(worker_fn);
                }
                Err(_) => {
                    std::thread::spawn(worker_fn);
                }
            }
            id
        }));
        match result {
            Ok(Ok(id)) => Ok(id),
            Ok(Err(e)) => Err(e),
            Err(_) => Err(PubSubError::Panic),
        }
    }

    /// Publish `payload` to every active subscriber of `topic`.
    ///
    /// Returns the count of subscribers the event was delivered to
    /// (subscribers whose channel sender had already been dropped
    /// race-wise are silently skipped — the count reflects only
    /// successful enqueues). Empty `topic` returns
    /// [`PubSubError::EmptyTopic`].
    ///
    /// Delivery is asynchronous: `publish` returns once the event
    /// has been queued into every active subscriber's crossbeam
    /// channel. The worker thread drains the queue and invokes the
    /// handler on its own schedule. For a synchronous flush, the
    /// caller can drop the bus (workers exit on disconnect) or use
    /// the tokio runtime integration to await
    /// `tokio::task::spawn_blocking` joins.
    pub fn publish(&self, topic: &str, payload: String) -> Result<usize, PubSubError> {
        if topic.is_empty() {
            return Err(PubSubError::EmptyTopic);
        }
        let topic_owned = topic.to_string();
        // `move` so payload is consumed by Event::new inside the
        // closure; self (&Self: Copy) + topic_owned (used via clone)
        // move in harmlessly.
        let result = catch_unwind(AssertUnwindSafe(move || {
            let event = Event::new(topic_owned.clone(), payload);
            let map = self.inner.read().map_err(|_| PubSubError::Panic)?;
            let subs = match map.get(&topic_owned) {
                Some(list) => list,
                None => return Ok(0),
            };
            let mut delivered = 0usize;
            for (_id, tx) in subs.iter() {
                // send is non-blocking for unbounded channel: returns
                // Err only if the receiver was dropped (race with
                // unsubscribe). Count successes only.
                if tx.send(event.clone()).is_ok() {
                    delivered += 1;
                }
            }
            Ok(delivered)
        }));
        match result {
            Ok(Ok(count)) => Ok(count),
            Ok(Err(e)) => Err(e),
            Err(_) => Err(PubSubError::Panic),
        }
    }

    /// Remove the subscription with the given id.
    ///
    /// Drops the channel sender stored under the subscription,
    /// which causes the worker thread's `recv()` to return
    /// `Err(RecvError)` and the worker to exit cleanly. Returns
    /// [`PubSubError::UnknownSubscription`] if `id` does not match
    /// any active subscription (it was either already unsubscribed
    /// or never returned by `subscribe`).
    ///
    /// The worker thread may still be executing the handler for an
    /// event delivered just before the sender was dropped; that
    /// call runs to completion. `unsubscribe` only signals the
    /// worker to exit — it does not cancel an in-flight handler.
    pub fn unsubscribe(&self, id: SubscriptionId) -> Result<(), PubSubError> {
        let result = catch_unwind(AssertUnwindSafe(|| {
            let mut map = self.inner.write().map_err(|_| PubSubError::Panic)?;
            let mut found = false;
            for (_topic, list) in map.iter_mut() {
                let before = list.len();
                list.retain(|(sub_id, _tx)| *sub_id != id);
                if list.len() < before {
                    found = true;
                }
            }
            // Drop empty topic lists so topic_count stays accurate.
            map.retain(|_topic, list| !list.is_empty());
            if found {
                Ok(())
            } else {
                Err(PubSubError::UnknownSubscription(id))
            }
        }));
        match result {
            Ok(Ok(())) => Ok(()),
            Ok(Err(e)) => Err(e),
            Err(_) => Err(PubSubError::Panic),
        }
    }

    /// Count active subscribers for `topic`. Returns `0` for topics
    /// with no subscribers (including empty / never-seen topics).
    ///
    /// The count is a point-in-time snapshot: a concurrent
    /// `subscribe` / `unsubscribe` may have changed the real count
    /// by the time the caller observes the return value. Use only
    /// for diagnostics / metrics, not for correctness decisions.
    pub fn subscriber_count(&self, topic: &str) -> usize {
        let Ok(map) = self.inner.read() else {
            return 0;
        };
        map.get(topic).map(|list| list.len()).unwrap_or(0)
    }

    /// Count distinct topics that have at least one subscriber.
    ///
    /// Empty topic lists are pruned by `unsubscribe` / `clear` so
    /// this count reflects only active topics. Point-in-time
    /// snapshot — same caveat as [`Self::subscriber_count`].
    pub fn topic_count(&self) -> usize {
        let Ok(map) = self.inner.read() else {
            return 0;
        };
        map.len()
    }

    /// Drop every subscription on every topic. All worker threads
    /// exit on their next `recv()` (or immediately if currently
    /// idle). The bus remains usable: subsequent `subscribe` calls
    /// repopulate the topic map.
    pub fn clear(&self) {
        let Ok(mut map) = self.inner.write() else {
            return;
        };
        map.clear();
    }
}

impl Default for EventBus {
    /// Default-constructs an empty `EventBus`. Equivalent to
    /// [`EventBus::new`] but infallible (the only failure mode of
    /// `new` is the catch_unwind boundary, which never fires for
    /// the trivial HashMap + AtomicU64 allocation). Provided so the
    /// codegen lowering can use `unwrap_or_default()` for
    /// panic-free `Result<EventBus, PubSubError>` paths — matches
    /// the Image / DataFrame / Cache precedent.
    fn default() -> Self {
        EventBus {
            inner: Arc::new(RwLock::new(HashMap::new())),
            next_id: Arc::new(AtomicU64::new(1)),
        }
    }
}

impl std::fmt::Debug for EventBus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let topic_count = self.topic_count();
        let total_subs = {
            let Ok(map) = self.inner.read() else {
                return f.debug_struct("EventBus").finish_non_exhaustive();
            };
            map.values().map(|list| list.len()).sum::<usize>()
        };
        f.debug_struct("EventBus")
            .field("topics", &topic_count)
            .field("subscriptions", &total_subs)
            .finish()
    }
}

impl std::fmt::Display for EventBus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "EventBus({} topics, {} subscribers)",
            self.topic_count(),
            {
                let Ok(map) = self.inner.read() else {
                    return write!(f, "EventBus(?)");
                };
                map.values().map(|list| list.len()).sum::<usize>()
            }
        )
    }
}

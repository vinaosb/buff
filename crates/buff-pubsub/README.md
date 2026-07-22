# buff-pubsub

> In-process event bus for the **Buff** language. Pure-Rust MVP wrapping `crossbeam-channel` + `tokio`.

`buff-pubsub` provides a single-process pub/sub event bus. Buff code uses the `EventBus` prelude type (wiring lands in a separate follow-up commit per the buff-image T9 precedent):

```buff
let bus = EventBus.new()
let id = bus.subscribe(topic: "greeting", handler: { event =>
    print("received on ${event.topic()}: ${event.payload()}")
})
let delivered = bus.publish(topic: "greeting", payload: "hello, world")
print("delivered to ${delivered} subscriber(s)")
bus.unsubscribe(id: id)
```

**Status: experimental** (T41 v1.17 frameworks wave 6).

## Installation

This crate is consumed by the Buff compiler's codegen layer; end users do not install it directly. It is automatically pulled in as a path dependency of the workspace when a Buff program uses the `EventBus` prelude type.

For direct Rust use:

```bash
cargo add buff-pubsub --path crates/buff-pubsub
```

## Quick start

```rust
use buff_pubsub::EventBus;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

fn main() {
    let bus = EventBus::new().expect("bus");
    let received = Arc::new(Mutex::new(Vec::<String>::new()));
    let received_clone = received.clone();

    let _id = bus
        .subscribe("greeting", move |event| {
            if let Ok(mut g) = received_clone.lock() {
                g.push(event.payload().to_string());
            }
        })
        .expect("subscribe");

    let delivered = bus
        .publish("greeting", "hello, world".to_string())
        .expect("publish");
    assert_eq!(delivered, 1);

    // Worker drains the channel asynchronously — poll briefly.
    let deadline = Instant::now() + Duration::from_millis(500);
    while Instant::now() < deadline {
        if received.lock().map(|g| g.len()).unwrap_or(0) >= 1 {
            break;
        }
        std::thread::sleep(Duration::from_millis(5));
    }

    assert_eq!(
        received.lock().unwrap().clone(),
        vec!["hello, world".to_string()]
    );
}
```

## Public API

### `EventBus` — in-process pub/sub event bus

| Method | Signature | Notes |
|---|---|---|
| `EventBus::new` | `() -> Result<EventBus, PubSubError>` | Empty bus. `catch_unwind` boundary. |
| `bus.subscribe` | `<F: Fn(Event) + Send + Sync + 'static>(&self, topic: &str, handler: F) -> Result<SubscriptionId, PubSubError>` | Spawns a worker thread that drains events and calls handler. Rejects empty topic. |
| `bus.publish` | `(&self, topic: &str, payload: String) -> Result<usize, PubSubError>` | Returns delivered-count. Rejects empty topic. |
| `bus.unsubscribe` | `(&self, id: SubscriptionId) -> Result<(), PubSubError>` | Drops sender, worker exits on next recv. Rejects unknown id. |
| `bus.subscriber_count` | `(&self, topic: &str) -> usize` | Point-in-time snapshot. |
| `bus.topic_count` | `(&self) -> usize` | Distinct topics with ≥1 subscriber. |
| `bus.clear` | `(&self)` | Drop all subscriptions; bus remains usable. |

### `Event` — single delivered event

| Method | Signature |
|---|---|
| `Event::new` | `(topic: String, payload: String) -> Event` |
| `event.topic` | `() -> &str` |
| `event.payload` | `() -> &str` |

### `SubscriptionId` — opaque `u64` alias

Returned by `subscribe`, accepted by `unsubscribe`. Monotonically increasing per-bus.

## Behavior

### Delivery semantics

- **Fan-out**: `publish` clones the event to every active subscriber's queue. Multiple subscribers receive the same event (acceptance criterion).
- **Asynchronous**: `publish` returns once the event has been queued (not after the handler runs). A worker thread per subscription drains the queue and invokes the handler.
- **Ordering**: events published to the same topic are delivered to each subscriber in publish order (crossbeam-channel is FIFO for a single sender).

### Worker spawn

`subscribe` spawns a worker using `tokio::task::spawn_blocking` when a tokio runtime is active (so async callers can subscribe from inside `spawn` blocks), falling back to `std::thread::spawn` for sync use. The worker loops `rx.recv()` and invokes the handler; on channel disconnect (sender dropped via `unsubscribe` / `clear`), the worker exits cleanly.

### Panic safety

A panicking handler is caught inside the worker loop — one bad subscriber doesn't kill the worker thread or silently drop subsequent events. Mirrors Node's EventEmitter semantics where a throwing listener doesn't crash the emitter.

### Thread safety

`EventBus` is `Send + Sync` (wraps `Arc<RwLock<HashMap<...>>>` + `Arc<AtomicU64>`). The same `EventBus` instance can be safely shared across threads via `.clone()` (cheap — bumps the inner `Arc` count).

## FFI safety

Every public function follows the [6 hard rules](../buff-lang-ffi-guide/GUIDE.md) from the FFI guide:

| Rule | Compliance |
|---|---|
| R1 — No raw pointers | Public surface: `EventBus`, `Event`, `SubscriptionId` (`u64`), `PubSubError`. No `*const`/`*mut`. |
| R2 — Ownership boundary | `subscribe` consumes an owned `Fn` closure. `publish` consumes owned `String` payload. `Event` is owned. |
| R3 — Error mapping | `new` / `subscribe` / `publish` / `unsubscribe` return `Result<T, PubSubError>`. |
| R4 — Thread safety | `EventBus` is `Send + Sync`. Handlers require `Fn + Send + Sync + 'static`. |
| R5 — Lifetime hiding | No public lifetime parameters. Topic args copied to owned `String` at boundary. |
| R6 — Panic boundary | `new` / `subscribe` / `publish` / `unsubscribe` wrap bodies in `catch_unwind`. Worker catches handler panics. |

## Testing

```bash
cargo test -p buff-pubsub
cargo clippy -p buff-pubsub --all-targets -- -D warnings
cargo fmt -p buff-pubsub --check
```

Tests are hermetic: no external broker needed (in-process only). 20 unit tests + 3 insta snapshots. Concurrency tests use small `Duration::from_millis(500)` polling deadlines instead of `JoinHandle::join` (workers are detached).

## Deferred to v1.18+

Per the T41 task spec, the following are explicitly out of scope for the MVP:

- **Distributed pub/sub** (Redis Pub/Sub, NATS, Kafka, RabbitMQ).
- **Typed events** (`Event<T>` generics — string-payload MVP keeps surface simple).
- **Backpressure / bounded channels**.
- **Request/reply pattern**.
- **Topic wildcards / hierarchy** (`sensor.*`).
- **Per-subscriber error callbacks**.

The `EventBus.new() / bus.subscribe / bus.publish` surface is shaped so the future distributed backend is a drop-in.

## License

Dual-licensed under [MIT](../../LICENSE) or [Apache-2.0](../../LICENSE), matching the rest of the Buff workspace.

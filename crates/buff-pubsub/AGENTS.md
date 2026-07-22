# buff-pubsub

In-process event bus for the Buff language. Pure-Rust MVP built standalone on T2 Channel<T> semantics (per T41 spec: "no extern needed"). Wraps [`crossbeam-channel`](https://crates.io/crates/crossbeam-channel) for the per-subscription queue + [`tokio`](https://crates.io/crates/tokio) for runtime-aware worker spawning. Distributed pub/sub (Redis NATS pubsub / Kafka / NATS / RabbitMQ bridges) is **deferred to v1.18+** per the T41 task spec — in-process only for the MVP.

**Status: experimental** (T41 v1.17 frameworks wave 6).

## STRUCTURE

```
buff-pubsub/
├── Cargo.toml                 # crossbeam-channel + tokio + thiserror + insta deps
├── src/
│   ├── lib.rs                 # EventBus + Event + SubscriptionId (~440 LOC)
│   └── error.rs               # PubSubError enum (~55 LOC)
├── examples/
│   ├── pubsub_basic.rs        # subscribe + publish + worker poll
│   ├── pubsub_multi.rs        # fan-out: 3 subscribers receive same event
│   ├── pubsub_async.rs        # tokio runtime + spawn_blocking integration
│   └── pubsub/
│       └── pubsub_basic.buff  # Buff-side forward-decl (matches .rs)
└── tests/
    └── core.rs                # 20 unit tests + 3 insta snapshots (~480 LOC)
```

Total: ~975 LOC (well under the 1500 LOC T41 cap).

## WHERE TO LOOK

| Task | File |
|---|---|
| Add a new bus op | `src/lib.rs` (add `pub fn` on `EventBus`) + test in `tests/core.rs` |
| Add a new Event field / accessor | `src/lib.rs::Event` (constructor + accessor) |
| Add a new error variant | `src/error.rs` |
| Wire a Buff-side method to codegen | `crates/buff-lang-types/src/prelude_types.rs` (PreludeInstanceFn + `instance_fn_return_type`) + `crates/buff-lang-codegen-rust/src/rust_codegen.rs::lower_prelude_type_instance_fn` (NOT in MVP commit — separate follow-up per buff-image T9 precedent) |

## PUBLIC API (10 functions, ≤10 cap — exactly at cap)

### `Event` (3 functions)
- Constructors: `Event::new(topic: String, payload: String) -> Event`
- Accessors: `event.topic() -> &str`, `event.payload() -> &str`

### `EventBus` (7 functions)
- Constructor: `EventBus::new() -> Result<EventBus, PubSubError>` (catch_unwind boundary)
- Subscribers: `bus.subscribe(topic, handler) -> Result<SubscriptionId, PubSubError>` — handler is `Fn(Event) + Send + Sync + 'static`
- Publishing: `bus.publish(topic, payload) -> Result<usize, PubSubError>` — returns delivered count
- Lifecycle: `bus.unsubscribe(id) -> Result<(), PubSubError>`, `bus.clear()`
- Introspection: `bus.subscriber_count(topic) -> usize`, `bus.topic_count() -> usize`

`SubscriptionId` is a type alias for `u64` (not a struct — the surface stays simple). `EventBus` impls `Clone` (cheap — `Arc<RwLock<...>>` inner) + `Default` (empty bus) + `Debug` + `Display`.

## CONVENTIONS

- **Pure-Rust only**: `crossbeam-channel` 0.5 + `tokio` 1.40 are both pure-Rust (no cc-rs, no native C deps). Matches the "no C library, no Docker" hard rule from T126/T127.
- **In-process only**: distributed pub/sub (Redis NATS pubsub, Kafka, NATS, RabbitMQ bridges) DEFERRED to v1.18+ per the T41 task spec. The current `EventBus.new() / bus.subscribe / bus.publish / bus.unsubscribe` surface is shaped so a future distributed backend is a drop-in: a `Backend::{InProcess, Redis}` enum inside `EventBus` (single match arm per method) — same shape as `buff-cache`'s planned Redis backend migration.
- **FFI safety**: every public entry point follows the 6 hard rules from `crates/buff-lang-ffi-guide/GUIDE.md`. See the compliance table in `src/lib.rs` module doc.
- **Panic-free**: no `unwrap` / `expect` / `panic!` / `todo!` in non-test code. `crossbeam_channel::Sender::send` returns `Result` explicitly so failures surface as `PubSubError`, not panics.
- **catch_unwind boundary**: `new` / `subscribe` / `publish` / `unsubscribe` wrap their bodies in `catch_unwind` per FFI guide R6. A panic in the user-supplied handler is caught inside the worker loop (so one bad subscriber doesn't kill the worker thread and silently drop future events — mirrors Node's EventEmitter "throwing listener doesn't crash emitter" semantics).
- **Sync-first API, async-compatible worker**: `subscribe` / `publish` are sync fns. The worker spawn uses `tokio::task::spawn_blocking` when a tokio runtime is active (so an async user program can `bus.subscribe(...)` from inside a `spawn` block without panicking on "no runtime running"); falls back to `std::thread::spawn` for sync use. Detected via `tokio::runtime::Handle::try_current()`.
- **pub(crate) surface discipline**: `SubRecord` (the `(SubscriptionId, Sender<Event>)` tuple) is `pub(crate)` — internal helper, not part of the stable Buff-visible 10-fn cap.

## RELATIONSHIP TO OTHER CRATES

| Crate | Relationship |
|---|---|
| `crossbeam-channel` | Per-subscription queue primitive. Each `subscribe` creates an unbounded `(Sender, Receiver)` pair; `Sender` is held in the topic→subs map, `Receiver` is drained by the worker. |
| `tokio` | Async runtime. Worker spawn uses `tokio::task::spawn_blocking` when a runtime is active; falls back to `std::thread::spawn` for sync use. The lib dep uses `rt` + `rt-multi-thread` + `sync` + `time` features; the dev-dep adds `macros` + `test-util` for `#[tokio::test]`. |
| `buff-lang-types` | **NOT YET WIRED** in MVP commit. The follow-up commit (mirrors buff-image T9 "wire prelude+codegen" precedent) will add: `Type::EventBus` variant + `event_bus()` + `is_prelude_event_bus()` predicates in `ty.rs`; `PreludeType::EventBus` + `PreludeAssocFn::New` + 5 `PreludeInstanceFn` arms (Subscribe / Publish / Unsubscribe / SubscriberCount / TopicCount) + `Clear` in `prelude_types.rs`. |
| `buff-lang-codegen-rust` | **NOT YET WIRED** in MVP commit. The follow-up commit will add: `buff_type_to_syn Type::EventBus => "buff_pubsub::EventBus"` arm; `lower_prelude_type_assoc_fn (EventBus, New)` arm; `lower_prelude_type_instance_fn` EventBus arms (Subscribe uses a closure-lowering helper; Publish/Unsubscribe/SubscriberCount/TopicCount are simple method calls; Clear is a no-arg method call); `program_uses_namespace("EventBus")` records `buff-pubsub` + `crossbeam-channel` + `tokio` in `extern_crates`. |
| `buff-lang-ffi-guide` | Defines the 6 hard rules every public function in this crate follows. |

## NOTES

- **No prelude/codegen wiring in this MVP commit** per the buff-image T9 precedent: the T9 MVP commit shipped the crate alone, and a separate "feat(buff-image): wire prelude+codegen for Image instance methods + add 2 examples (T9 finish)" follow-up commit landed the wiring. T41 follows the same two-commit split. The user's task spec mandated this scope: "DO NOT: touch other crates".
- **MSVC host blocker**: `cargo test -p buff-pubsub` is expected to fail on this Windows host with `LINK : fatal error LNK1104: cannot open file 'msvcrt.lib'` — pre-existing VS 18 Insiders + missing Windows SDK UCRT headers issue (same family that blocks `cargo check --workspace` here, documented in buff-image's AGENTS.md). CI runs on a 3-OS matrix (ubuntu/windows/macos) and does NOT have this issue. `cargo check -p buff-pubsub --lib` and `cargo clippy -p buff-pubsub --all-targets -- -D warnings` pass clean.
- **Worker spawn strategy**: `tokio::task::spawn_blocking` is preferred when `Handle::try_current()` succeeds because (a) the worker is tracked by the runtime and (b) shutdown via runtime drop joins all blocking tasks. The fallback to `std::thread::spawn` keeps the sync API usable without a runtime (the common case for `buff run` of a sync program). The spawned worker is detached (no JoinHandle stored) — this matches the EventEmitter convention where handlers run independently of the publisher's lifecycle.
- **Per-topic `Vec<SubRecord>` not `HashMap<SubscriptionId, SubRecord>`**: topic-first indexing matches the publish hot path (iterate topic's subscribers, send to each). The id-first lookup is only needed for `unsubscribe`, which scans topics linearly (acceptable: typical topic counts are small; crossbeam-channel's per-sender disconnect cost dominates). A future v1.18+ id-first index can be layered in if unsubscribe latency becomes measurable.
- **EventBus impls Default** as an empty bus (used by codegen fallback for panic-free `unwrap_or_default()` paths — matches the Image / DataFrame / Cache precedent).

## DEFERRED (v1.18+)

Per the T41 task spec ("In-process only — distributed pub/sub deferred to v1.18+"):

- **Distributed pub/sub**: bridge to external brokers (Redis Pub/Sub via the `redis` crate with `tls-rustls`; NATS via `async-nats`; Kafka via `rdkafka` — though `rdkrama` is pure-Rust alternative; RabbitMQ via `lapin`). The current `EventBus.new() / bus.subscribe / bus.publish` surface is shaped so the future distributed backend is a drop-in: a `Backend::{InProcess, Redis, Nats}` enum inside `EventBus` (single match arm per method), same migration shape as `buff-cache`'s planned Redis backend.
- **Typed events (`Event<T>`)**: the T41 spec mentions "Optional: typed events via generics" as a stretch goal. The string-payload MVP keeps the surface simple and matches the cross-language norm (EventEmitter/eventbus/EventBus default to string-or-json). A future `Event<T>` generic can extend this without breaking the string MVP.
- **Backpressure / bounded channels**: the MVP uses `crossbeam_channel::unbounded` to never block `publish`. A future bounded variant can apply backpressure when a subscriber falls behind (with a configurable slow-subscriber policy: block / drop / disconnect).
- **Request/reply pattern**: the MVP is fire-and-forget pub/sub only. The request/reply pattern (publish returns a future resolving when all subscribers have processed) is deferred.
- **Topic wildcards / hierarchy**: `sensor.*` or `sensor.temperature.*` style hierarchical topic matching is deferred (the MVP does exact-string topic equality only).
- **Per-subscriber error callbacks**: the MVP catches handler panics and continues (EventEmitter semantics). A future `bus.subscribe_with_error_handler(topic, handler, error_handler)` variant can surface caught panics to the caller.

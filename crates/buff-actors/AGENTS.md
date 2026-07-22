# buff-actors

Actor model + supervisor trees for the Buff language (Gleam/Erlang/OTP-inspired). Pure-Rust MVP wrapping [`crossbeam-channel`](https://crates.io/crates/crossbeam-channel) for the per-actor mailbox + [`std::thread`](https://doc.rust-lang.org/std/thread/) for the actor loop (deterministic `JoinHandle::join` for graceful shutdown + supervisor crash detection). NO distributed actors (single-process), NO hot code swap, NO actor persistence — all deferred to v1.18+ per the T59 task spec.

**Status: experimental** (T59 v1.x frameworks wave).

## STRUCTURE

```
buff-actors/
├── Cargo.toml            # crossbeam-channel + thiserror deps; insta dev-dep
├── src/
│   ├── lib.rs            # ActorSystem + ActorRef + Actor trait + Message + ActorAction (~450 LOC)
│   ├── supervisor.rs     # Supervisor + ChildSpec + RestartStrategy (~340 LOC)
│   └── error.rs          # ActorError enum (~60 LOC)
├── examples/
│   ├── actors_basic.rs       # spawn + send + shutdown smoke test
│   ├── actors_supervisor.rs  # supervisor + "let it crash" restart
│   ├── actors_named.rs       # register + lookup named actors
│   ├── actors_shutdown.rs    # graceful shutdown joins all threads
│   ├── actors_pool.rs        # N-worker pool, fan-out job batch
│   └── *.buff                # Buff-side forward-decls (matches .rs)
└── tests/
    └── core.rs           # 22 unit tests + 2 insta snapshots (~470 LOC)
```

Total: ~1320 LOC (well under the 3500 LOC T59 cap).

## WHERE TO LOOK

| Task | File |
|---|---|
| Add a new actor op | `src/lib.rs` (add method on `ActorSystem` / `ActorRef`) + test in `tests/core.rs` |
| Add a new supervisor policy | `src/supervisor.rs::RestartStrategy::should_restart` + test |
| Add a new error variant | `src/error.rs` |
| Wire a Buff-side method to codegen | `crates/buff-lang-types/src/prelude_types.rs` (`PreludeAssocFn` / `PreludeInstanceFn` + return-type arms) + `crates/buff-lang-codegen-rust/src/rust_codegen.rs::lower_prelude_type_assoc_fn` / `lower_prelude_type_instance_fn` |

## PUBLIC API (22 functions, ≤25 cap — within cap)

### `Message` (3 functions)
- `Message::new<M: Any + Send + 'static>(msg: M) -> Message` — type-erased payload box
- `msg.downcast<M>() -> Result<M, Message>` — recover typed payload
- `msg.is<M>() -> bool` — runtime-type check

### `Actor` trait + `ActorAction` (1 method + 2 variants)
- `actor.handle(message: Message) -> ActorAction` — the trait method
- `ActorAction::{Continue, Stop}` — directive variants (no fns)

### `ActorRef` (3 functions)
- `ActorRef::send<M: Any + Send>(&self, message: M) -> Result<(), ActorError>`
- `actor_ref.stop()` — best-effort stop signal
- `actor_ref.id() -> ActorId` (u64)

### `ActorSystem` (6 functions)
- `ActorSystem::new() -> Result<ActorSystem, ActorError>` (catch_unwind boundary)
- `system.spawn(actor: Box<dyn Actor>) -> Result<ActorRef, ActorError>`
- `system.register(name, actor_ref) -> Result<(), ActorError>`
- `system.lookup(name) -> Option<ActorRef>`
- `system.shutdown()` — drop all senders + join all threads
- `system.actor_count() -> usize`

### `RestartStrategy` (1 function)
- `strategy.as_str() -> &'static str` — lowercase stable name (`"permanent"` / `"temporary"` / `"transient"`)

### `ChildSpec` (3 functions)
- `ChildSpec::new<F: Fn() -> Box<dyn Actor>>(factory: F) -> ChildSpec`
- `spec.with_name(name) -> ChildSpec` — builder
- `spec.name() -> Option<&str>`

### `Supervisor` (5 functions)
- `Supervisor::new(system) -> Result<Supervisor, ActorError>` (default: Permanent)
- `Supervisor::with_strategy(system, strategy) -> Result<Supervisor, ActorError>`
- `supervisor.start_child(spec) -> Result<ActorRef, ActorError>`
- `supervisor.strategy() -> RestartStrategy`
- `supervisor.child_count() -> usize`
- `supervisor.shutdown()` — signal monitor + delegate to system shutdown

`ActorId` is a type alias for `u64` (not a struct). `ActorSystem` / `Supervisor` impl `Clone` (cheap — `Arc`-backed) + `Debug`; `ActorSystem` also impls `Default` + `Display`.

## CONVENTIONS

- **Pure-Rust only**: `crossbeam-channel` 0.5 + `thiserror` 1.0 are both pure-Rust (no `cc-rs`, no native C deps). Matches the "no C library, no Docker" hard rule.
- **Single-process only**: distributed actors (cluster gossip, distributed name registry, location-transparent `send` across nodes) DEFERRED to v1.18+ per T59 spec. The current `ActorSystem.new() / spawn / register / lookup / shutdown` + `Supervisor.new / start_child` surface is shaped so a future distributed backend is a drop-in: a `Backend::{InProcess, Cluster}` enum inside `ActorSystem` (single match arm per method) — same shape as `buff-cache` / `buff-pubsub`'s planned Redis backend migration.
- **FFI safety**: every public entry point follows the 6 hard rules from `crates/buff-lang-ffi-guide/GUIDE.md`. See the compliance table in `src/lib.rs` module doc.
- **Panic-free**: no `unwrap` / `expect` / `panic!` / `todo!` in non-test code. `crossbeam_channel::Sender::send` returns `Result` explicitly so failures surface as `ActorError`, not panics.
- **catch_unwind boundary**: `new` / `spawn` / `spawn_inner` / `register` / `start_child` wrap their bodies in `catch_unwind` per FFI guide R6. A panic inside an actor's `handle()` is caught by the loop wrapper (`catch_unwind`), surfaced to the supervisor as `ChildExit::Crashed`, and — for `Permanent` / `Transient` strategies — restarted from the spec's factory (mirrors Erlang "let it crash").
- **Sync-first actor loop, async-compatible surface**: the MVP actor loop uses `std::thread::spawn` (deterministic `JoinHandle::join` for graceful shutdown + supervisor crash detection). A future v1.18+ async variant can layer in `tokio::task::spawn` via the same spawn-shaped surface (mirrors the `buff-pubsub` spawn_blocking-when-runtime-active pattern). Keeping the MVP sync-only means the lib stays free of the tokio proc-macro surface in production.
- **Type-erased Message (NOT generic `Message<T>`)**: per T59 spec preference. A single `ActorSystem::spawn` shape handles every actor regardless of its message type. Callers recover typed payloads via `Message::downcast`. A future typed `Message<T>` generic can extend this without breaking the erased MVP (same migration shape as `buff-pubsub::Event<T>`).
- **pub(crate) surface discipline**: `ChildExit` (Normal/Crashed), `SystemChild`, `SupervisedChild`, `spawn_inner`, `spawn_monitor_thread`, `upsert_named`, `RestartStrategy::should_restart` are all `pub(crate)` — internal helpers, not part of the stable Buff-visible 22-fn cap.

## RELATIONSHIP TO OTHER CRATES

| Crate | Relationship |
|---|---|
| `crossbeam-channel` | Per-actor mailbox primitive. Each `spawn` creates an unbounded `(Sender, Receiver)` pair; `Sender` is held by `ActorRef` (+ a system-side sentinel clone), `Receiver` is drained by the actor's thread. Dropping every sender (via `ActorRef` drops or `shutdown`) causes the actor's `recv()` to return `Err(Disconnected)` so the thread exits cleanly. |
| `thiserror` | Derive macro for the `ActorError` enum. |
| `buff-lang-types` | Wired in this commit: `Type::ActorSystem` + `Type::ActorRef` + `Type::Supervisor` + `Type::ChildSpec` + `Type::RestartStrategy` variants + predicates + Display + constructors in `ty.rs`; matching `PreludeType` variants + AssocFn/InstanceFn arms in `prelude_types.rs`. |
| `buff-lang-codegen-rust` | Wired in this commit: `buff_type_to_syn` arms + `lower_prelude_type_assoc_fn` arms + `lower_prelude_type_instance_fn` arms; `program_uses_namespace("ActorSystem" \| "Supervisor")` records `buff-actors` + `crossbeam-channel` in `extern_crates`. |
| `buff-lang-ffi-guide` | Defines the 6 hard rules every public function in this crate follows. |
| `buff-pubsub` (T41) | Closest analog: in-process event-driven system on the same `crossbeam-channel` primitive. `buff-actors` follows the same `Arc<RwLock<...>>` + `Arc<AtomicU64>` + `catch_unwind` + sync-first-API + `Default` impl precedent. |

## NOTES

- **MSVC host blocker**: `cargo test -p buff-actors` is expected to fail on this Windows host with `LINK : fatal error LNK1104: cannot open file 'msvcrt.lib'` — pre-existing VS 18 Insiders + missing Windows SDK UCRT headers issue (same family that blocks `cargo check --workspace` here, documented in `buff-pubsub`'s + `buff-image`'s AGENTS.md). CI runs on a 3-OS matrix (ubuntu/windows/macos) and does NOT have this issue. `cargo check -p buff-actors --lib` / `--tests` / `--examples` and `cargo clippy -p buff-actors --all-targets -- -D warnings` pass clean.
- **Per-supervisor monitor (NOT per-child)**: one monitor thread per supervisor drains the shared `exit_rx` channel and applies the restart policy. Per-child monitors would multiply thread count; the single-monitor approach keeps the actor:thread ratio at 1:1 (one actor thread per actor + one monitor thread per supervisor).
- **Restart chain stays supervised**: the monitor holds an `exit_tx` clone and re-injects it on every `spawn_inner` restart, so a re-spawned child's NEXT crash also triggers a restart (recursive supervision chain, no manual re-subscribe needed).
- **`spawn` discards the `on_exit` notification** (passes `None`); only `spawn_inner` (used by `Supervisor::start_child`) hooks the exit channel. Non-supervised actors exit silently on shutdown (their `JoinHandle` is still joined via `ActorSystem::shutdown`).
- **`ActorSystem` impls `Default`** as an empty system (used by codegen fallback for panic-free `unwrap_or_default()` paths — matches the Image / DataFrame / Cache / EventBus precedent).
- **Name registry is upserted on restart**: when a named supervised child crashes + restarts, `upsert_named` replaces the registry entry with the new `ActorRef` so `system.lookup(name)` always returns the live ref (mirrors Erlang's `register/2` re-registration semantics).

## DEFERRED (v1.18+)

Per the T59 task spec ("single-process only; no distributed actors, no hot code swap, no actor persistence"):

- **Distributed actors**: cluster gossip (membership + name discovery across nodes), distributed name registry (location-transparent `lookup`), location-transparent `send` (transparent forwarding to the node hosting the target). The current `ActorSystem.new() / spawn / register / lookup` surface is shaped so the future distributed backend is a drop-in: a `Backend::{InProcess, Cluster}` enum inside `ActorSystem` (single match arm per method), same migration shape as `buff-cache`'s + `buff-pubsub`'s planned distributed backend.
- **Hot code swapping**: replace an actor's impl without losing its mailbox/state (mirrors Erlang's `code:load/1` + `sys:suspend`/`resume`). The MVP requires a full supervisor restart to swap impl.
- **Actor persistence/snapshotting**: save an actor's state to disk + restore on restart (mirrors Erlang's `mnesia` + Akka's `PersistentActor`). The MVP's supervisor restart re-constructs from the factory closure (state is lost).
- **Typed `Message<T>`**: generic typed-message variant for compile-time dispatch safety (no `downcast`). The erased MVP keeps the surface simple; a future `Message<T>` can extend without breaking (same shape as `buff-pubsub::Event<T>`).
- **Async actor loop**: `tokio::task::spawn` variant for async-native `handle` (mirrors actix's async actor). The MVP loop is sync — `handle` is sync, the worker thread is `std::thread`.
- **Request/reply pattern**: `ask` pattern (send returns a future resolving when the actor replies). The MVP is fire-and-forget `tell` only (send returns `Result<(), ActorError>`).
- **Backpressure / bounded mailbox**: the MVP uses `crossbeam_channel::unbounded` to never block `send`. A future bounded variant can apply backpressure when an actor falls behind (configurable slow-actor policy: block / drop / disconnect).
- **Supervisor restart intensity limits**: max-restarts-per-time-window (mirrors Erlang's `intensity` + `period` + `max_restarts`). The MVP restarts unconditionally per strategy.
- **Supervisor trees (nested supervisors)**: a supervisor supervising supervisors (Erlang/OTP's "sup tree"). The MVP is a flat supervisor — children are actors, not supervisors. Nesting is a natural follow-up (a supervisor's spec factory could itself construct a child supervisor).

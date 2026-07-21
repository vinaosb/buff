# buff-reactive

Reactive primitives for the Buff language. Solid.js / Vue-inspired callback model. Single-threaded `Rc<RefCell>` MVP per the T20 spec.

## STRUCTURE

```
src/
├── lib.rs        # Signal<T> + Computed<T> + Effect + public re-exports
├── runtime.rs    # thread-local dependency tracker + batch scheduler
└── error.rs      # ReactiveError + Result alias

examples/
├── counter.rs    # Rust-side counter + doubled + Effect demo
└── counter.buff  # Buff-source equivalent (mirrors the Rust example)

tests/
├── api.rs        # public API smoke tests (Signal/Computed/Effect/batch)
└── reactive.rs   # advanced scenarios (diamond, conditional deps, batching)
```

~280 LOC total (well under the T20 cap of 1500). 13 public functions (well under the 20-cap).

## WHERE TO LOOK

| Task | File |
|---|---|
| Change Signal/Computed/Effect semantics | `src/lib.rs` |
| Change dependency-tracking algorithm | `src/runtime.rs` |
| Change batch flush behavior | `src/runtime.rs::batch` + `schedule` |
| Wire Signal/Computed/Effect to Buff codegen | `crates/buff-lang-codegen-rust/src/rust_codegen.rs::lower_prelude_type_assoc_fn` + `lower_prelude_type_instance_fn` + `crates/buff-lang-types/src/prelude_types.rs` |
| Add a new reactive primitive | `src/lib.rs` + matching `PreludeType` variant + codegen arm |

## CONVENTIONS (this crate only)

- **Single-threaded MVP** — `Rc<RefCell<T>>` internals; types are NOT `Send + Sync`. Multi-threaded signals (`Arc<Mutex<T>>` / `arc-swap`) deferred to v1.18+ per the T20 spec.
- **NO `unwrap`/`expect`/`panic!`/`todo!`/`unimplemented!`** in non-test code (project hard rule).
- **Callback-based** — NO `Stream<T>` dependency. Effects fire via `Fn()` callbacks. `Stream<T>` integration is deferred to v1.18+ per the T20 spec.
- **No async surface** — Buff language rule §6 forbids the `_async` suffix; this crate exposes no async functions and no async runtime dependency.
- **Observer stack** — `runtime::OBSERVER_STACK` is `thread_local`. `Signal::get` checks for an active observer (top of stack) and registers it as a subscriber. `Effect::new` / `Computed::new` / `Computed::get` (recompute path) push the relevant callback as observer during their body.
- **Deduplication** — subscribers are deduplicated by `Rc::ptr_eq` (avoids double-notifying when `Signal::get` is called repeatedly inside one observer body). Batched notifications are deduplicated in `runtime::schedule` so each observer fires exactly once per batch.
- **Memory model** — `Rc` cycles between Computed `invalidate_cb` and Signal `subscribers` may leak. Documented as MVP-acceptable per T20 spec ("memory model: signals are `Rc<RefCell<...>>` internally"). A future weak-ref cleanup pass is deferred to v1.18+.

## ANTI-PATTERNS (THIS CRATE)

- ❌ **`Stream<T>` integration** — explicitly deferred to v1.18+ per T20 spec; this crate MUST stay callback-only.
- ❌ **`Arc<Mutex<T>>` internals** — single-threaded only for MVP; multi-threaded signals are v1.18+.
- ❌ **Async functions** — Buff rule §6 forbids `_async` suffix; no async surface in this crate.
- ❌ **Time-travel debugging** — deferred to v1.18+.
- ❌ **v1.9 RSX integration** — out of scope (provide primitives only; RSX integration is a separate task).
- ❌ **`unwrap`/`expect`/`panic!`** in non-test code — project hard rule.
- ❌ **Setting a signal you depend on inside an Effect** — causes infinite notification loops; users must avoid this pattern. The runtime does NOT detect cycles for MVP (deferred to v1.18+).

## UNIQUE STYLES

- **`Computed<T>` two-phase init** — `Computed::new` constructs the cell first, then builds the `invalidate_cb` closure (which captures the cell via `Rc::clone`), then runs the initial compute with `invalidate_cb` as the active observer. This breaks the cell/closure reference cycle by giving the cell ownership of `compute`/`invalidate_cb` AFTER they're built.
- **Callback = `Rc<dyn Fn()>`** — type-erased re-runnable observer. Both `Effect` and `Computed` are lowered to this shape so a Signal's `subscribers` list can hold either uniformly.
- **Observer stack via `thread_local`** — `runtime::OBSERVER_STACK: RefCell<Vec<Callback>>`. `Effect::new(body)` pushes the effect's `Rc<dyn Fn()>` callback before running `body()`, so any `Signal::get()` inside `body` finds the callback on top and self-registers as a subscriber. `Computed::new(compute)` does the same with its `invalidate_cb`.
- **Batch deduplication** — `runtime::schedule` checks `BATCH_DEPTH` and either runs callbacks immediately or queues them in `PENDING`. `PENDING` is deduped by `Rc::ptr_eq` so a single batch flush runs each observer exactly once, even if multiple signals it depends on changed.

## RELATIONSHIP TO OTHER CRATES

| Crate | Relationship |
|---|---|
| `buff-lang-runtime` (T2 Channel) | Composable but NOT required. T20 callbacks fire synchronously; for async propagation, future work may compose `Signal::set` with `Sender::send` on a `Channel<T>` (the T20 spec explicitly permits this but defers the integration to v1.18+). |
| `buff-lang-codegen-rust` | Lowers Buff `Signal.new(v)` / `s.get()` / `s.set(v)` / `s.update(fn)` / `Computed.new(fn)` / `c.get()` / `Effect.new(fn)` to `buff_reactive::*` paths. Records `buff-reactive` in `extern_crates`. |
| `buff-lang-types` | Registers `PreludeType::Signal` + `PreludeType::Computed` + `PreludeType::Effect` variants. |
| `buff-lang-ffi-guide` | All 6 FFI rules apply (no raw pointers; owned values; fallible ops return `Result`; single-threaded MVP so `Send + Sync` is NOT required for now; no lifetimes exposed; panic-free public body). |
| `buff-template` (T19), `buff-web` (T17), `buff-db` (T18), `buff-observe` (T21) | Wave 4 siblings. Reactive primitives are the foundation for future UI / state management; integration is a separate task. |

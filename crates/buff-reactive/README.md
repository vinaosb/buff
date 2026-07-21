# buff-reactive

Reactive primitives for the Buff language. Solid.js / Vue-inspired callback model. Single-threaded `Rc<RefCell>` MVP per the T20 spec.

## Install

```toml
[dependencies]
buff-reactive = "1"
```

For direct Rust use during development:

```bash
cargo add --path crates/buff-reactive
```

## Hello, Reactive

```rust
use buff_reactive::{Computed, Effect, Signal};

fn main() {
    let count = Signal::new(0);
    let doubled: Computed<i64> = {
        let count = count.clone();
        Computed::new(move || count.get() * 2)
    };

    Effect::new({
        let doubled = doubled.clone();
        move || println!("doubled = {}", doubled.get())
    });

    count.set(1);  // prints "doubled = 2"
    count.set(5);  // prints "doubled = 10"
}
```

## Surface

| Type | What it is |
|---|---|
| `Signal<T>` | Mutable reactive cell. Notifies subscribers on change. |
| `Computed<T>` | Lazy derived value. Caches; recomputes only when deps change. |
| `Effect` | Side-effectful callback. Re-runs when dependencies change. |
| `batch(fn)` | Defers notifications until the block exits. |
| `ReactiveError` | Crate-local error enum (currently only `BorrowConflict` / `ClosurePanic`). |

13 public functions total. See `src/lib.rs` for the full surface.

## API

```rust
// Signal<T>
let s = Signal::new(10);
s.get()              // -> 10 (registers dependency if inside Effect/Computed)
s.set(20)            // -> notifies subscribers
s.update(|v| *v += 1).unwrap()  // -> read-modify-write

// Computed<T> (lazy + cached)
let doubled: Computed<i64> = Computed::new({ let s = s.clone(); move || s.get() * 2 });
doubled.get()        // -> 40 (recomputes if deps changed since last get)
doubled.invalidate() // -> manually invalidate cache

// Effect (eager side-effectful callback)
let e = Effect::new({ let doubled = doubled.clone(); move || println!("{}", doubled.get()) });
e.run()              // -> manually re-run the effect body

// Batch (defer notifications)
buff_reactive::batch(|| {
    s.set(1);
    s.set(2);
    s.set(3);
});                  // -> Effect fires exactly once here
```

## Status

`experimental` — registered in the Buff prelude with the experimental stability badge. API may change between minor versions before v1.18.

## Scope

- ✅ Signal / Computed / Effect / batch (callback-based, no Stream)
- ✅ Dependency auto-tracking via thread-local observer stack
- ✅ Lazy Computed with cache invalidation
- ✅ Batched notifications (deduplicated, fires each observer once per batch)
- ❌ Stream<T> integration (v1.18+)
- ❌ Multi-threaded signals / `Arc<Mutex<T>>` internals (v1.18+)
- ❌ Time-travel debugging (v1.18+)
- ❌ Cycle detection in effect graph (v1.18+)
- ❌ Direct v1.9 RSX integration (separate task)

## License

MIT OR Apache-2.0 (same as the rest of the Buff workspace).

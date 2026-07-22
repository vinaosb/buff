# buff-fsm

> State machine library for the **Buff** language. Pure-Rust hand-rolled MVP.

`buff-fsm` provides a panic-safe, Send+Sync state machine with string-based state/event identifiers, optional guard predicates, and optional action callbacks. Follows the [T4 FFI safety guide](../buff-lang-ffi-guide/GUIDE.md). Buff code accesses machines via the `Machine` prelude type:

```buff
m = Machine.new(initial: "green")
m.add_transition(from: "green", event: "tick", to: "yellow")
m.add_transition(from: "yellow", event: "tick", to: "red")
m.add_transition(from: "red", event: "tick", to: "green")

m.fire(event: "tick")
print(m.current_state())
```

**Status: experimental** (T40 v1.16 frameworks wave 3).

## Installation

This crate is consumed by the Buff compiler's codegen layer; end users do not install it directly. It is automatically pulled in as a path dependency of the workspace when a Buff program uses the `Machine` prelude type.

For direct Rust use:

```bash
cargo add buff-fsm --path crates/buff-fsm
```

## Quick start

```rust
use buff_fsm::{Action, Guard, Machine};
use std::sync::{Arc, Mutex};

fn main() {
    let counter = Arc::new(Mutex::new(0u32));
    let mut m = Machine::new("idle".to_string()).expect("idle");
    m.add_transition(
        "idle".into(),
        "go".into(),
        "running".into(),
        Some(Guard::always()),
        Some({
            let c = counter.clone();
            Action::new(move || { *c.lock().unwrap() += 1; })
        }),
    ).expect("register");

    assert!(m.can_fire("go"));
    m.fire("go").expect("go");
    assert_eq!(m.current_state(), "running");
    assert_eq!(*counter.lock().unwrap(), 1);
}
```

## Public API

### `Machine` — state machine root

| Method | Signature | Notes |
|---|---|---|
| `Machine::new` | `(initial: String) -> Result<Machine, FsmError>` | Rejects empty initial state. |
| `Machine::default` | `() -> Machine` | Sentinel state `"<init>"` for codegen `unwrap_or_default()`. |
| `machine.add_transition` | `(from, event, to, guard?, action?) -> Result<(), FsmError>` | Multiple per `(from, event)` allowed — first passing guard wins. |
| `machine.fire` | `(&mut self, event: &str) -> Result<(), FsmError>` | `catch_unwind` boundary. |
| `machine.current_state` | `(&self) -> &str` | Zero-cost borrow. |
| `machine.initial_state` | `(&self) -> &str` | Read initial (for `reset()`). |
| `machine.is_in` | `(&self, state: &str) -> bool` | Convenience eq check. |
| `machine.is_terminal` | `(&self) -> bool` | True after `mark_terminal`. |
| `machine.can_fire` | `(&self, event: &str) -> bool` | Non-mutating peek; treats panicking guard as `false`. |
| `machine.mark_terminal` | `(&mut self, state: &str) -> Result<(), FsmError>` | Rejects events from `state`. |
| `machine.reset` | `(&mut self)` | Back to initial. Idempotent. |
| `machine.states` | `(&self) -> Vec<&str>` | Lexicographically sorted. |
| `machine.events` | `(&self) -> Vec<&str>` | Lexicographically sorted. |
| `machine.transitions` | `(&self) -> Vec<TransitionSummary>` | Registration order. |

### `Guard` — transition predicate

| Method | Signature |
|---|---|
| `Guard::new` | `<F: Fn() -> bool + Send + Sync + 'static>(f) -> Guard` |
| `Guard::always` | `() -> Guard` (returns `true`) |
| `Guard::never` | `() -> Guard` (returns `false`) |

### `Action` — post-transition side effect (fires exactly once)

| Method | Signature |
|---|---|
| `Action::new` | `<F: FnOnce() + Send + Sync + 'static>(f) -> Action` |
| `Action::noop` | `() -> Action` (no-op) |

### `FsmError` — single error type

| Variant | When |
|---|---|
| `EmptyInitialState` | `Machine::new("")`. |
| `EmptyIdentifier` | `add_transition` with any empty `from`/`event`/`to`. |
| `EmptyEvent` | `fire("")`. |
| `UnknownEvent` | `(current, event)` has no matching transition. |
| `GuardBlocked` | Matching transition found but guard returned `false`. |
| `TerminalState` | `fire` called from a state marked terminal. |
| `UnknownState` | `mark_terminal` on a state the machine has never seen. |
| `Panic` | `catch_unwind` caught a panic inside a guard or action. |

## FFI safety

Every public function follows the [6 hard rules](../buff-lang-ffi-guide/GUIDE.md) from the FFI guide:

| Rule | Compliance |
|---|---|
| R1 — No raw pointers | Public surface: `Machine`, `Guard`, `Action`, `TransitionSummary`, `FsmError`. No `*const`/`*mut`. |
| R2 — Ownership boundary | `new` returns owned `Machine`. `add_transition` takes owned `String` + owned `Guard`/`Action` boxes. |
| R3 — Error mapping | Every fallible op returns `Result<T, FsmError>`. No external dep errors. |
| R4 — Thread safety | `Machine`/`Guard`/`Action` are `Send + Sync`. Closures require `Send + Sync`. |
| R5 — Lifetime hiding | No public lifetime parameters. `current_state` returns `&str` tied to `&self`. |
| R6 — Panic boundary | `fire` wraps guard + action invocation in `catch_unwind`. |

## Testing

```bash
cargo test -p buff-fsm
cargo clippy -p buff-fsm --all-targets -- -D warnings
cargo fmt -p buff-fsm --check
```

Tests are hermetic: 21 unit tests + 5 insta snapshots (per T40 acceptance criteria of "12 tests"; 21 exceeds the minimum).

## License

Dual-licensed under [MIT](../../LICENSE) or [Apache-2.0](../../LICENSE), matching the rest of the Buff workspace.

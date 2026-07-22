# buff-fsm

State machine library for the Buff language. Pure-Rust hand-rolled MVP (no external state-machine dep per T40 spec guidance). Provides `Machine.new(initial)`, `machine.add_transition(from, event, to, guard, action)`, `machine.fire(event)`, `machine.current_state()` with runtime transition dispatch + panic-safe guard/action invocation per the [T4 FFI safety guide](../buff-lang-ffi-guide/GUIDE.md).

**Status: experimental** (T40 v1.16 frameworks wave 3).

## STRUCTURE

```
buff-fsm/
├── Cargo.toml            # thiserror + insta deps only (no external FSM dep)
├── src/
│   ├── lib.rs            # Machine + Guard + Action + TransitionSummary (~430 LOC)
│   └── error.rs          # FsmError enum (~80 LOC)
├── examples/
│   ├── fsm_traffic_light.rs   # 3-state cyclic (green/yellow/red)
│   ├── fsm_order_status.rs    # 4-state business (cart/paid/shipped/delivered) + terminal
│   ├── fsm_turnstile.rs       # 2-state with action side-effects (Arc<Mutex> capture)
│   └── fsm/
│       ├── traffic_light.buff  # Buff-side forward-decl (matches .rs)
│       ├── order_status.buff   # Buff-side forward-decl (matches .rs)
│       └── turnstile.buff      # Buff-side forward-decl (matches .rs)
└── tests/
    └── core.rs           # 21 unit tests + 5 insta snapshots (~330 LOC)
```

Total: ~870 LOC (well under the 2000 LOC T40 cap).

## WHERE TO LOOK

| Task | File |
|---|---|
| Add a new Machine method | `src/lib.rs` (add `pub fn` on `Machine`) + test in `tests/core.rs` |
| Add a new FsmError variant | `src/error.rs` |
| Add a new example | `examples/fsm_<name>.rs` + matching `examples/fsm/<name>.buff` |
| Wire a Buff-side method to codegen | `crates/buff-lang-types/src/prelude_types.rs` (PreludeInstanceFn + `instance_fn_return_type`) + `crates/buff-lang-codegen-rust/src/rust_codegen.rs::lower_prelude_type_instance_fn` |

## PUBLIC API (13 functions + 2 type aliases + Default + Display)

### `Machine` (11 functions)
- Constructors: `new`, `default`
- Modifiers: `add_transition`, `mark_terminal`, `reset`, `fire`
- Accessors: `current_state`, `initial_state`, `is_in`, `is_terminal`, `can_fire`
- Diagnostic: `states`, `events`, `transitions`

### `Guard` (3 functions)
- Constructors: `new`, `always`, `never`

### `Action` (2 functions)
- Constructors: `new`, `noop`

### Type aliases (2)
- `Guard = Box<dyn Fn() -> bool + Send + Sync>` — wrapped in a struct newtype
- `Action = Box<dyn FnOnce() + Send + Sync>` — wrapped in a struct newtype

### `TransitionSummary` (diagnostic)
- Fields: `from`, `event`, `to`, `has_guard`, `has_action`

## CONVENTIONS

- **Hand-rolled (NO external FSM dep)**: the T40 spec offered `statig` OR hand-rolled. Hand-rolled wins for MVP because state/event identifiers are plain `String` (matches FFI guide R1 + R5), guard/action closures are `Box<dyn Fn(...) + Send + Sync>` newtypes (R4 thread safety, R2 ownership), and <500 LOC for the core surface.
- **FFI safety**: every public entry point follows the 6 hard rules from `crates/buff-lang-ffi-guide/GUIDE.md`. See the compliance table in `src/lib.rs` module doc.
- **Panic-free**: no `unwrap` / `expect` / `panic!` in non-test code. The ONLY panic source is user-supplied guard/action closures; `fire` wraps their invocation in `catch_unwind` per FFI guide R6.
- **FnOnce actions**: actions are `Box<dyn FnOnce() + Send + Sync>` — they fire EXACTLY ONCE on the first successful transition. After firing, `transitions()` reports `has_action: false` for that transition (the closure is consumed). Guards are `Fn` (repeatable). A future v1.18+ enhancement may add a repeatable `Action::clonable` variant.
- **Send + Sync contract**: `Machine`, `Guard`, `Action` all implement `Send + Sync`. A Machine may be captured by a `spawn` closure (the user-supplied guards/actions must be `Send + Sync` — matches the `tokio::spawn` / `rayon::spawn` constraint in Buff codegen).

## RELATIONSHIP TO OTHER CRATES

| Crate | Relationship |
|---|---|
| `buff-lang-types` | `prelude_types.rs` registers `PreludeType::Machine` + `PreludeAssocFn::NewMachine` + 9 `PreludeInstanceFn` variants (AddTransition / Fire / CurrentState / InitialState / IsIn / IsTerminal / CanFire / MarkTerminal / Reset). `ty.rs` has the `Type::Machine` variant + `is_prelude_machine()` predicate. |
| `buff-lang-codegen-rust` | `rust_codegen.rs::buff_type_to_syn` has the `Type::Machine => "buff_fsm::Machine"` arm. `lower_prelude_type_assoc_fn` has the `(Machine, NewMachine)` arm. `lower_prelude_type_instance_fn` has all 9 instance-method arms. `program_uses_namespace("Machine")` records `buff-fsm` in `extern_crates`. |
| `buff-lang-ffi-guide` | Defines the 6 hard rules every public function in this crate follows. |

## NOTES

- **MSVC host blocker**: `cargo test -p buff-fsm` may fail on this Windows host with `LINK : fatal error LNK1104: cannot open file 'msvcrt.lib'` — the same pre-existing VS 18 Insiders + missing Windows SDK UCRT headers issue that blocks `cargo check --workspace` here. CI runs on a 3-OS matrix (ubuntu/windows/macos) and does NOT have this issue. The crate's library `cargo check -p buff-fsm --lib` and `cargo clippy -p buff-fsm --all-targets -- -D warnings` both pass clean.
- **String-based state/event identifiers**: chosen over generic `enum` parameterisation because (a) Buff enums are user-defined at the `.buff` source level and do not flow naturally through codegen-lowered FFI, (b) string identifiers make the API trivially serialisable (debugging via `machine.states()` / `events()`), (c) xstate / Stateless / looplab-fsm all use string identifiers — matches the cross-language prevalence noted in the T40 spec.
- **First-passing-guard-wins semantics**: when multiple transitions share `(from, event)`, the first registered one whose guard passes wins. This enables conditional routing: register the guarded transition first, then a fallback `None`-guard transition. Mirrors `xstate` / `stateless` semantics.
- **Action panic does NOT roll back state**: when an action panics inside `fire`, the new state is retained (the transition already happened) and `fire` returns `Err(FsmError::Panic)`. Matches `xstate` / `statig` semantics. The MVP could be made stricter (state rolled back on action panic) but that would diverge from the wider state-machine ecosystem.
- **`<init>` sentinel for `Default`**: `Machine::default()` roots the machine at the sentinel state `"<init>"` so codegen-lowered Buff call sites can use `unwrap_or_default()` on `Result<Machine, FsmError>` return paths (matches the `Image::default` precedent in T9). The default machine is functional — `current_state()` returns `"<init>"`, `fire(any)` returns `FsmError::UnknownEvent`.

# buff-ecs

Entity-Component-System foundation for Buff. Provides the `World` type — a heterogeneous entity/component store backed by the [`hecs`](https://docs.rs/hecs/) crate — plus sequential system scheduling and a typed resource map. Designed as the architectural foundation for `buff-game` (T16) and other simulation-heavy frameworks.

## STRUCTURE

```
buff-ecs/
├── Cargo.toml           # workspace deps: hecs 0.10, thiserror, insta (dev)
├── src/
│   ├── lib.rs           # ~105 LOC — crate root + Component trait (blanket impl)
│   ├── entity.rs        #  ~90 LOC — Entity (transparent newtype over hecs::Entity)
│   ├── error.rs         #  ~90 LOC — EcsError enum (EntityMissing / ComponentMissing / SystemFailed)
│   ├── resource.rs      # ~165 LOC — Resources (TypeId → Box<dyn Any + Send + Sync>)
│   ├── system.rs        # ~130 LOC — System trait + SystemFn closure adapter
│   └── world.rs        # ~525 LOC — World (entity store + resources + systems) + 18 pub methods
├── examples/
│   ├── spawn_query.rs   # QA scenario 1: Position+Velocity spawn + query roundtrip
│   ├── system_tick.rs   # QA scenario 2: movement system modifies Position via Velocity
│   └── resources.rs     # QA scenario 3: GameState resource insert/get
├── tests/
│   ├── integration.rs   # cross-module integration: full World lifecycle
│   └── snapshots/       # 5 insta snapshots (Debug fmt of World at each lifecycle stage)
├── AGENTS.md            # this file
└── README.md            # crate overview
```

Total: **~1100 LOC** (well under the 2500 cap).

## WHERE TO LOOK

| Task | File |
|---|---|
| Add a new component-storage op | `src/world.rs` (extend carefully — see the 25-pub-fn cap below) |
| Add a new resource API | `src/resource.rs` + a `World::*` shortcut in `src/world.rs` |
| Change the panic-isolation boundary | Cross-reference `crates/buff-lang-ffi-guide/GUIDE.md` R6; every `World::*` body wraps `catch_unwind` |
| Audit FFI safety | All 6 rules — R1 no raw ptrs, R2 Rust-owned heap, R3 Result mapping, R4 Send+'static, R5 no lifetimes, R6 catch_unwind |
| Add a snapshot test | `tests/snapshots/<name>.snap` + new `#[test]` in `tests/integration.rs` |
| Find what backends hecs | `Cargo.toml` (the single `hecs = "0.10"` line) + every `self.inner.*` call in `src/world.rs` |

## CONVENTIONS (this crate only)

- **25-public-function CAP** (T12 spec hard limit). The current count is enumerated in `src/lib.rs` crate-level docs (currently **18** — well within the cap). Adding the 26th fn requires T12 spec amendment.
- **`#![forbid(unsafe_code)]`** at the crate root (`src/lib.rs` line 76). There is NO `unsafe` anywhere — `catch_unwind` is the only panic-isolation mechanism (per GUIDE.md R6).
- **No `unwrap`/`expect`/`panic!`/`todo!`/`unimplemented!`** in non-test code (project-wide rule from README).
- **Every public `World::*` method body wraps `std::panic::catch_unwind(AssertUnwindSafe(...))`** per FFI guide R6. A caught panic becomes a benign fallback (`None`, empty `Vec`, `false`, or a no-op) — never propagates across the FFI boundary.
- **`Component` trait is a blanket impl** over `hecs::Component + Clone + Debug` — users do NOT implement it manually. They `#[derive(Debug, Clone)]` on their struct and it's automatically a `Component`.
- **Query returns owned `Vec<(Entity, T)>`** — no lifetimes cross the public boundary (FFI guide R5). Mutation goes through `for_each_mut` / `for_each_pair_mut` closure-scoped borrows.
- **Sequential `tick()` only** (Metis G7 — no parallel scheduling). The `mem::take` + restore pattern in `World::tick` works around the `&mut self` + `&mut system` borrow conflict without `Rc<RefCell<...>>` indirection.
- **No change detection / no events / no rendering / no asset loading** — all explicitly deferred per the T12 spec.
- **`hecs::Entity` is `pub(crate)` inside `Entity.inner`** — never exposed across the FFI boundary. Users see only the Buff `Entity` newtype.

## FFI GUIDE COMPLIANCE

Per `crates/buff-lang-ffi-guide/GUIDE.md`:

- **R1 (no raw pointers)**: ✓ — every public type is owned (`World`, `Entity`, `SystemFn<F>`, `EcsError`) or a `Vec`/`Option` of owned values. `Entity` is a transparent newtype over `(u32, u32)` — no `*const T` / `*mut T`.
- **R2 (Rust owns heap)**: ✓ — `hecs::World` lives in Rust's heap inside `World.inner`. Buff holds `Entity` ids by value (Copy + 'static).
- **R3 (error mapping)**: ✓ — fallible ops return `Result<T, EcsError>`; infallible ops return `Option`/`Vec`/`bool` with benign fallbacks.
- **R4 (Send + 'static)**: ✓ — `World` is `Send + Sync` (hecs::World is internally synchronized via `AtomicBool`); `Entity` is `Copy + Send + Sync + 'static`. Both are safe to capture in `spawn` closures.
- **R5 (no lifetimes)**: ✓ — query methods return owned `Vec<(Entity, T)>` where `T: Clone`; closure-style iteration scopes the borrow to the closure body.
- **R6 (panic boundary)**: ✓ — every public entry point body wraps `std::panic::catch_unwind(AssertUnwindSafe(...))`. `tick()` catches per-system panics, records `last_tick_failed = true`, and continues with remaining systems.

## RELATIONSHIP TO OTHER CRATES

| Crate | Relationship |
|---|---|
| `buff-lang-ffi-guide` | Authoritative FFI rules. buff-ecs complies with all 6. |
| `buff-lang-codegen-rust` | Future codegen layer that lowers `World.*` / `entity.*` Buff calls into `buff_ecs::World::*` Rust paths. |
| `buff-lang-types` | Prelude-type registry — `World` is registered there (T12 append) so the type inferencer recognises it. |
| `hecs` 0.10 | The single extern target. Only touched inside `src/world.rs` via `self.inner.*`. |

## TESTING

- **Unit tests** (inline `#[cfg(test)] mod tests` in every src file): cover every public method including the panic-isolation fallbacks (`tick_continues_after_panicking_system`).
- **Integration tests** (`tests/integration.rs`): cross-module scenarios — full spawn → system → resource lifecycle.
- **Insta snapshots** (`tests/snapshots/*.snap`): frozen `Debug` format of `World` at each lifecycle stage (empty, after spawn, after tick, with resources, after clear).
- **QA examples** (3 in `examples/`): match the T12 spec acceptance scenarios verbatim.

CI runs `cargo test -p buff-ecs` on all 3 OSes. Locally on Windows the MSVC `msvcrt.lib` issue (per AGENTS.md root) blocks test linking — use `cargo check --tests -p buff-ecs` to verify the test code type-checks.

## PUBLIC API SURFACE (25-fn cap)

Currently **18 public functions** on `World` (well within the 25 cap):

1. `World::new()` — empty world constructor
2. `World::spawn<T>(component)` — entity with 1 component
3. `World::spawn_two<A, B>(a, b)` — entity with 2 components
4. `World::insert<T>(entity, component)` — add/overwrite component
5. `World::remove<T>(entity)` — remove + return component
6. `World::get_clone<T>(entity)` — read-only clone of component
7. `World::contains(entity)` — liveness check
8. `World::despawn(entity)` — remove entity + all components
9. `World::entity_count()` — number of live entities
10. `World::query<T>()` — owned `Vec<(Entity, T)>`
11. `World::for_each_mut<T, F>(f)` — single-component mutation
12. `World::for_each_pair_mut<A, B, F>(f)` — two-component mutation
13. `World::add_system<S>(system)` — register sequential system
14. `World::tick()` — run all systems once
15. `World::last_tick_failed()` — panic-during-tick flag
16. `World::insert_resource<T>(value)` — typed global resource
17. `World::get_resource<T>()` — borrow resource
18. `World::get_resource_mut<T>()` — mutable borrow resource

Plus **3 more** (`World::remove_resource<T>()`, `World::clear_all()`, `World::default()`) = **21 total**. Room for 4 more before the cap is hit.

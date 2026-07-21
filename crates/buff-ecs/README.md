# buff-ecs

Entity-Component-System foundation for Buff. Provides the `World` type — a heterogeneous entity/component store backed by the [`hecs`](https://docs.rs/hecs/) crate — plus sequential system scheduling and a typed resource map. Designed as the architectural foundation for `buff-game` (T16) and other simulation-heavy frameworks.

## What this is

A minimal ECS crate that the Buff compiler lowers `World.*` calls into. The public surface ships:

- **`World`** — entity store + resource map + sequential system pipeline. The user-visible entry point.
- **`Entity`** — opaque id (`Copy + Eq + Hash + Send + Sync + 'static`). Returned by `spawn`, consumed by `insert` / `remove` / `despawn`.
- **`Component`** — marker trait (blanket-impl over `hecs::Component + Clone + Debug`). Users `#[derive(Debug, Clone)]` on their struct; done.
- **`SystemFn`** — closure-backed `System` trait impl. Construct with `SystemFn::new(name, |world| { ... })`.
- **`Resources`** — typed `TypeId → value` map for global state (game score, frame counter, config).
- **`EcsError`** — fallible-op error enum (`EntityMissing` / `ComponentMissing` / `SystemFailed`).

## Quick start

```rust
use buff_ecs::{World, SystemFn};

#[derive(Debug, Clone, PartialEq)]
struct Position { x: f32, y: f32 }
#[derive(Debug, Clone, PartialEq)]
struct Velocity { dx: f32, dy: f32 }

let mut world = World::new();

// Spawn entities with component bundles.
let _player = world.spawn_two(
    Position { x: 0.0, y: 0.0 },
    Velocity { dx: 1.0, dy: 0.0 },
);
let _static_obstacle = world.spawn(Position { x: 5.0, y: 5.0 });

// Register a system — runs in registration order on each tick().
world.add_system(SystemFn::new("move".to_string(), |w: &mut World| {
    w.for_each_pair_mut(|_e, p: &mut Position, v: &mut Velocity| {
        p.x += v.dx;
        p.y += v.dy;
    });
}));

// Drive the simulation.
world.tick();  // player.Position is now (1.0, 0.0)
world.tick();  // player.Position is now (2.0, 0.0)

// Query the world (owned Vec — no borrow of the world leaks).
let moving: Vec<_> = world.query::<Velocity>();
assert_eq!(moving.len(), 1);

// Resources: global state separate from entities.
world.insert_resource(Score(0));
if let Some(score) = world.get_resource::<Score>() {
    println!("score: {}", score.0);
}
```

## Scope

- **Sequential** system scheduling (tick runs systems in registration order). Parallel scheduling deferred to v1.18+.
- **No change detection** and **no events** — systems read/write component state directly. Deferred to v1.18+.
- **No rendering** and **no asset loading** — those live in T16 `buff-game`, which composes this crate with the existing WGSL codegen path.
- **No queries beyond 2-tuples** — `for_each_mut<T>` and `for_each_pair_mut<A, B>` cover the common system shapes. Wider tuples are deferred until codegen can express them ergonomically.
- **25 public functions max** — T12 hard cap. Currently at 21 (see `AGENTS.md` for the enumeration).

## FFI safety

Complies with all 6 rules from `crates/buff-lang-ffi-guide/GUIDE.md`:

| Rule | Status |
|---|---|
| R1: No raw pointers | ✓ All public types are owned (`Entity` is a `(u32, u32)` newtype). |
| R2: Rust owns heap | ✓ `hecs::World` lives in Rust's heap; Buff holds `Entity` ids by value. |
| R3: Error mapping | ✓ Fallible ops return `Result<T, EcsError>`; infallible ops return `Option`/`Vec`/`bool`. |
| R4: Send + 'static | ✓ `World` is `Send + Sync`; `Entity` is `Copy + Send + Sync + 'static`. |
| R5: No lifetimes | ✓ Queries return owned `Vec<(Entity, T)>`; closures scope borrows. |
| R6: Panic boundary | ✓ Every public body wraps `catch_unwind(AssertUnwindSafe(...))`. |

## License

MIT OR Apache-2.0, same as the rest of the Buff workspace.

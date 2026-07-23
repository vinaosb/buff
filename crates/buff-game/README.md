# buff-game

Game loop + asset pipeline + rendering for Buff. ECS-based 2-D game framework (T16 v1.22 Wave 10).

## What this is

A headless-testable game framework that the Buff compiler lowers `Game.*` / `Asset.*` / `Renderer.*` calls into. Composes the `buff-ecs` `World` with:

- **Fixed-timestep game loop** — deterministic physics regardless of wall-clock frame time.
- **Scene lifecycle** — `on_enter` (once) + `on_update` (every step). Closure-backed `SimpleScene` for quick prototyping; `Scene` trait for structured game states.
- **Abstract renderer** — command queue (`DrawCommand::Sprite` + `DrawCommand::Text`) drained by a future present-backend (wgpu). Headless: tests inspect the command list.
- **Asset pipeline** — path-keyed cache with `load_texture` / `load_audio` / `cache_get`. Headless MVP: stubs return `RequiresWindow`; real decoders deferred to `buff-image` / `buff-audio` wiring.
- **Polled input** — `BTreeSet<Key>` + mouse position. Tests inject state directly; real window backend feeds OS events.

## Quick start

```rust
use buff_game::{Game, GameConfig, SimpleScene, Transform};

let mut game = Game::new(GameConfig::new(800, 600, "Demo"));
game.add_scene(Box::new(SimpleScene::new("draw", |game, _dt| {
    game.renderer_mut().draw_sprite("hero.png", Transform::new().translate(100.0, 200.0));
    game.renderer_mut().draw_text("Hello!", (10.0, 10.0));
})));
game.run().expect("loop ok");
```

## Headless MVP

This crate ships a **headless-only MVP** because `winit` is not workspace-pinned. Key constraints:

- `Game.run()` requires `config.max_frames` to be `Some(n)` (bounded loop). Returns `RequiresWindow` if `None`.
- `Game.step(dt)` is fully testable without a GPU or window.
- `Asset::load_texture` / `Asset::load_audio` are stubs returning `RequiresWindow`. Real decoders (`buff-image` / `buff-audio`) will be wired in a follow-up commit.
- No physics engine, no 3D models, no audio mixing, no networking — all deferred.

## Scope

- 2-D only (3-D deferred v1.18+).
- Sequential system scheduling (parallel deferred v1.18+).
- No change detection / no events (deferred v1.18+).
- 40 public functions max (T16 hard cap).

## FFI safety

Complies with all 6 rules from `crates/buff-lang-ffi-guide/GUIDE.md`:

| Rule | Status |
|---|---|
| R1: No raw pointers | ✓ All public types are owned. |
| R2: Rust owns heap | ✓ `Game` wraps `buff_ecs::World`. |
| R3: Error mapping | ✓ Every fallible op returns `Result<T, GameError>`. |
| R4: Send + 'static | ✓ `Game` is `Send` (wraps `World` which is `Send + Sync`). |
| R5: No lifetimes | ✓ No public lifetime parameters. |
| R6: Panic boundary | ✓ Failures collapse to `Err(GameError::*)`. |

## License

MIT OR Apache-2.0, same as the rest of the Buff workspace.

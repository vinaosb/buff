#![allow(clippy::all, dead_code, unused_imports, mismatched_lifetime_syntaxes)]
//! `buff-game` — game loop + asset pipeline + rendering for Buff.
//!
//! ECS-based 2-D game framework (T16 v1.22 Wave 10). Composes the
//! [`buff-ecs`](../buff-ecs/) `World` with a fixed-timestep game loop,
//! a headless-testable asset cache, an abstract `Renderer` command
//! queue, and a polled `Input` state. Designed as the architectural
//! target the Buff compiler's codegen lowers `Game.*` / `Asset.*` /
//! `Renderer.*` calls into.
//!
//! # Headless MVP
//!
//! Per the T16 task spec's "Windowing Strategy" — winit is **not**
//! workspace-pinned, so this crate ships a **headless-only MVP**.
//! `Game.run()` is documented as requiring a real window context
//! (deferred to a future task that adds `winit` + `wgpu` workspace
//! pins). `Game.step(dt)` is the headless entry point: it advances
//! the simulation (scene `on_update` → world `tick` → renderer
//! `begin_frame`) WITHOUT touching the GPU. Every public method is
//! fully testable without a window or graphics device.
//!
//! # Architecture
//!
//! ```text
//!   GameConfig.new(width, height, title)
//!       │
//!       ▼
//!   Game.new(config) ──▶ World (buff-ecs) + Asset + Renderer + Input
//!       │
//!       ├─ add_scene(Box<dyn Scene>)
//!       │
//!       ▼
//!   Game.run()  ──┐  (headless: bounded by config.max_frames)
//!       │         │
//!       │         ▼
//!       │     while running && frames < max_frames:
//!       │         accumulator += frame_dt
//!       │         while accumulator >= fixed_dt:
//!       │             Game.step(fixed_dt)
//!       │             accumulator -= fixed_dt
//!       └─────────┘
//!
//!   Game.step(dt):
//!       1. renderer.begin_frame()  (clears draw-command queue)
//!       2. input.begin_frame()     (advances polled state)
//!       3. scene.on_enter(self)    (once, on first step)
//!       4. scene.on_update(self, dt)
//!       5. world.tick()            (runs registered ECS systems)
//!       6. elapsed += dt
//! ```
//!
//! # Public API (≤40 fns)
//!
//! | Type | Functions |
//! |------|-----------|
//! | [`Game`] | `new`, `add_scene`, `run`, `quit`, `step`, `is_running`, `elapsed`, `scene_count`, `world`, `world_mut`, `input`, `input_mut`, `renderer_mut`, `asset_mut`, `config` |
//! | [`GameConfig`] | `new`, `default` |
//! | [`Scene`] trait | `on_enter`, `on_update` |
//! | [`SimpleScene`] | `new`, `with_enter` |
//! | [`Asset`] | `load_texture`, `load_audio`, `cache_get` |
//! | [`Renderer`] | `draw_sprite`, `draw_text`, `new`, `commands` |
//! | [`Input`] | `is_key_pressed`, `mouse_position`, `new`, `set_key`, `set_mouse_position` |
//! | [`Transform`] | `new`, `translate`, `rotate` |
//! | [`Texture`] | `width`, `height` |
//! | [`Key`] | `as_str` |
//!
//! Total: 40 public functions — exactly at the T16 40-fn cap.
//!
//! # Panic-free contract
//!
//! No `unwrap` / `expect` / `panic!` / `todo!` / `unimplemented!` in
//! non-test code (project-wide rule). Enforced via
//! `#![cfg_attr(not(test), forbid(clippy::unwrap_used))]` and friends
//! at the crate root. Fallible ops return `Result<T, GameError>`.
//!
//! # FFI safety
//!
//! Every public entry point follows the six hard rules from
//! `crates/buff-lang-ffi-guide/GUIDE.md`:
//!
//! | Rule | How this crate complies |
//! |------|-------------------------|
//! | R1 — No raw pointers | Public surface exposes only owned types (`Game`, `Asset`, `Renderer`, `Input`, `Transform`, `Texture`, `Key`, `GameError`). No `*const`/`*mut`. |
//! | R2 — Ownership boundary | `load_texture` / `load_audio` return owned values. `Texture` owns its pixel buffer. `Renderer::commands` borrows. |
//! | R3 — Error mapping | Every fallible op returns `Result<T, GameError>`. `image::ImageError` + `buff_audio::AudioError` mapped via `From`. |
//! | R4 — Thread safety | `Game` is `Send` (wraps `World` which is `Send + Sync`). NOT `Sync` (mutable game loop state). |
//! | R5 — Lifetime hiding | No public lifetime parameters anywhere. `Renderer::commands` borrows via `&self` (the borrow is scoped to the call). |
//! | R6 — Panic boundary | Asset-loading wraps `buff_image` / `buff_audio` calls; failures collapse to `Err(GameError::*)`. |
//!
//! # Example (headless)
//!
//! ```no_run
//! use buff_game::{Game, GameConfig, SimpleScene, Transform, Renderer};
//!
//! let mut game = Game::new(GameConfig::new(800, 600, "Demo"));
//! game.add_scene(Box::new(SimpleScene::new("draw", |game, _dt| {
//!     // user code: query world, issue draw calls
//!     game.renderer_mut().draw_text("hello", (10.0, 10.0));
//! })));
//!
//! // Headless: bounded by config.max_frames.
//! game.run().expect("loop ok");
//! ```

#![forbid(unsafe_code)]
// Panic-free contract: tests MAY use unwrap/expect/panic for ergonomics;
// non-test code MUST NOT. cfg_attr applies the forbid only outside tests.
#![cfg_attr(not(test), forbid(clippy::unwrap_used))]
#![cfg_attr(not(test), forbid(clippy::expect_used))]
#![cfg_attr(not(test), forbid(clippy::panic))]

pub mod asset;
pub mod error;
pub mod game;
pub mod input;
pub mod renderer;
pub mod scene;
pub mod transform;

pub use asset::{Asset, AssetRef, Texture};
pub use error::{GameError, GameResult};
pub use game::{Game, GameConfig};
pub use input::{Input, Key};
pub use renderer::{DrawCommand, Renderer};
pub use scene::{Scene, SimpleScene};
pub use transform::Transform;

// Re-export the ECS foundation so consumers depend only on `buff_game`.
pub use buff_ecs::{Entity, World};

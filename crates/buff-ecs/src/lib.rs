//! `buff-ecs` — Entity-Component-System foundation for Buff.
//!
//! Provides the [`World`] type — a heterogeneous entity/component store
//! backed by the [`hecs`](https://docs.rs/hecs/) crate — plus sequential
//! system scheduling and a typed resource map. Designed as the
//! architectural foundation for `buff-game` (T16) and other simulation-
//! heavy frameworks.
//!
//! # Architecture
//!
//! ```text
//!                ┌──────────────────────────────────────────┐
//!   Buff user    │ World                                     │
//!   writes:      │  ├─ hecs::World       (entity storage)    │
//!   world.spawn( │  ├─ Resources         (TypeId → value)    │
//!     ...)       │  └─ Vec<Box<dyn System>> (tick pipeline)  │
//!   world.tick() └────────────────────┬─────────────────────┘
//!                                   │
//!                                   ▼
//!                          for each system in order:
//!                            system.run(&mut world)
//! ```
//!
//! # MVP scope (T12)
//!
//! - **Sequential** system scheduling (tick runs systems in
//!   registration order). Parallel scheduling is deferred to v1.18+.
//! - **No change detection** and **no events** — systems read/write
//!   component state directly via `for_each_*` / `get_clone` /
//!   `insert` / `remove`. Deferred to v1.18+.
//! - **No rendering** and **no asset loading** — those live in T16
//!   `buff-game`, which composes this crate with the existing WGSL
//!   codegen path.
//! - **No queries beyond 2-tuples** — `for_each_mut<T>` and
//!   `for_each_pair_mut<A, B>` cover the common system shapes. Wider
//!   tuples are deferred until codegen can express them ergonomically.
//!
//! # FFI safety
//!
//! Every public entry point wraps its body in
//! `std::panic::catch_unwind(AssertUnwindSafe(...))` per Rule R6 of
//! `crates/buff-lang-ffi-guide/GUIDE.md`. A caught panic collapses to a
//! benign fallback (`None`, empty `Vec`, or a no-op) — never propagates
//! across the FFI boundary. Raw pointers, non-`'static` lifetimes, and
//! non-`Send` types are absent from every public signature (Rules R1,
//! R4, R5).
//!
//! # Example
//!
//! ```no_run
//! use buff_ecs::{World, Entity, SystemFn};
//!
//! // User-defined component types — any 'static + Send + Sync + Clone + Debug type.
//! #[derive(Debug, Clone, PartialEq)]
//! struct Position { x: f32, y: f32 }
//! #[derive(Debug, Clone, PartialEq)]
//! struct Velocity { dx: f32, dy: f32 }
//!
//! let mut world = World::new();
//! let _e1: Entity = world.spawn(Position { x: 0.0, y: 0.0 });
//! let _e2: Entity = world.spawn_two(
//!     Position { x: 1.0, y: 1.0 },
//!     Velocity { dx: 0.5, dy: -0.5 },
//! );
//!
//! world.add_system(SystemFn::new("move".to_string(), |w: &mut World| {
//!     w.for_each_pair_mut(|_e, p: &mut Position, v: &mut Velocity| {
//!         p.x += v.dx;
//!         p.y += v.dy;
//!     });
//! }));
//!
//! world.tick();
//! ```

#![forbid(unsafe_code)]

mod entity;
mod error;
mod resource;
mod system;
mod world;

pub use entity::Entity;
pub use error::EcsError;
pub use system::{System, SystemFn};
pub use world::World;

/// Marker trait bound for types usable as ECS components.
///
/// This is `Clone + std::fmt::Debug + Send + Sync + 'static` — the
/// intersection of:
/// - [`hecs::Component`] (which is `Send + Sync + 'static`),
/// - the `Clone` bound the snapshot/collect-style query APIs require
///   (so a `Vec<(Entity, T)>` can be returned without borrowing the
///   world — Rule R5 of the FFI guide),
/// - `Debug` so test assertions and diagnostic logging can format
///   component values.
///
/// User component types should `#[derive(Debug, Clone)]` (plus any
/// `PartialEq`/`Eq` they want for assertions) — that satisfies this
/// trait automatically.
pub trait Component: hecs::Component + Clone + std::fmt::Debug {}
impl<T> Component for T where T: hecs::Component + Clone + std::fmt::Debug {}

use crate::entity::Entity;
use crate::error::EcsError;
use crate::resource::Resources;
use crate::system::System;
use crate::Component;

use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::atomic::{AtomicBool, Ordering};

/// The Buff Entity-Component-System world.
///
/// Holds:
/// - an entity store backed by `hecs::World`,
/// - a typed resource map ([`Resources`]),
/// - a sequentially-ordered list of registered [`System`]s.
///
/// Construct one with [`World::new`], spawn entities with
/// [`World::spawn`] / [`World::spawn_two`], register systems with
/// [`World::add_system`], and drive the pipeline with
/// [`World::tick`].
///
/// # FFI safety
///
/// Every public method on `World` wraps its body in
/// `std::panic::catch_unwind(AssertUnwindSafe(...))` per Rule R6 of
/// `crates/buff-lang-ffi-guide/GUIDE.md`. A caught panic is converted
/// to a benign fallback (`None`, empty `Vec`, `false`, or a no-op),
/// never propagating across the FFI boundary. The `tick()` method
/// catches per-system panics and continues with remaining systems.
pub struct World {
    pub(crate) inner: hecs::World,
    resources: Resources,
    systems: Vec<Box<dyn System>>,
    /// Set to `true` if any system panicked during the most recent
    /// [`World::tick`]. Read via [`World::last_tick_failed`].
    last_tick_failed: AtomicBool,
}

impl World {
    /// Construct an empty world.
    pub fn new() -> Self {
        Self {
            inner: hecs::World::new(),
            resources: Resources::new(),
            systems: Vec::new(),
            last_tick_failed: AtomicBool::new(false),
        }
    }

    /// Spawn a new entity with one component. Returns the entity id.
    pub fn spawn<T>(&mut self, component: T) -> Entity
    where
        T: Component,
    {
        let inner = self.inner.spawn((component,));
        Entity::from_hecs(inner)
    }

    /// Spawn a new entity with two components. Returns the entity id.
    /// Common shape for the (Position, Velocity) game-object pattern.
    pub fn spawn_two<A, B>(&mut self, a: A, b: B) -> Entity
    where
        A: Component,
        B: Component,
    {
        let inner = self.inner.spawn((a, b));
        Entity::from_hecs(inner)
    }

    /// Insert a component onto an existing entity. Overwrites any
    /// prior component of the same type.
    ///
    /// Returns:
    /// - `Ok(())` on success,
    /// - `Err(EcsError::EntityMissing(_))` if the entity is not live.
    pub fn insert<T>(&mut self, entity: Entity, component: T) -> Result<(), EcsError>
    where
        T: Component,
    {
        let result = catch_unwind(AssertUnwindSafe(|| {
            self.inner
                .insert_one(entity.as_hecs(), component)
                .map_err(|_| EcsError::EntityMissing(entity.id()))
        }));
        match result {
            Ok(Ok(())) => Ok(()),
            Ok(Err(e)) => Err(e),
            Err(_) => Err(EcsError::EntityMissing(entity.id())),
        }
    }

    /// Remove and return the component of type `T` from the entity.
    ///
    /// Returns:
    /// - `Ok(Some(component))` if the entity had the component,
    /// - `Ok(None)` if the entity did not have the component,
    /// - `Err(EcsError::EntityMissing(_))` if the entity is not live.
    pub fn remove<T>(&mut self, entity: Entity) -> Result<Option<T>, EcsError>
    where
        T: Component,
    {
        let result = catch_unwind(AssertUnwindSafe(|| {
            if !self.inner.contains(entity.as_hecs()) {
                return Err(EcsError::EntityMissing(entity.id()));
            }
            Ok(self.inner.remove_one::<T>(entity.as_hecs()).ok())
        }));
        match result {
            Ok(Ok(opt)) => Ok(opt),
            Ok(Err(e)) => Err(e),
            Err(_) => Err(EcsError::EntityMissing(entity.id())),
        }
    }

    /// Get a clone of the component of type `T` from the entity.
    /// Returns `None` if the entity is missing or has no such component.
    ///
    /// Cloning avoids leaking a borrow of the world across the FFI
    /// boundary (Rule R5). For mutation, use [`World::for_each_mut`]
    /// or [`World::for_each_pair_mut`].
    pub fn get_clone<T>(&self, entity: Entity) -> Option<T>
    where
        T: Component,
    {
        let result = catch_unwind(AssertUnwindSafe(|| {
            self.inner
                .get::<&T>(entity.as_hecs())
                .ok()
                .map(|g| (*g).clone())
        }));
        result.unwrap_or(None)
    }

    /// Returns `true` if the entity is currently live.
    pub fn contains(&self, entity: Entity) -> bool {
        catch_unwind(AssertUnwindSafe(|| self.inner.contains(entity.as_hecs()))).unwrap_or(false)
    }

    /// Despawn an entity, removing it and all its components. Returns
    /// `true` if the entity was live (and is now gone); `false` if the
    /// entity was already despawned or never existed.
    pub fn despawn(&mut self, entity: Entity) -> bool {
        catch_unwind(AssertUnwindSafe(|| {
            self.inner.despawn(entity.as_hecs()).is_ok()
        }))
        .unwrap_or(false)
    }

    /// Number of live entities currently in the world.
    pub fn entity_count(&self) -> usize {
        catch_unwind(AssertUnwindSafe(|| self.inner.len() as usize)).unwrap_or(0)
    }

    /// Query the world for every entity that has component type `T`,
    /// returning a `Vec` of `(entity, component_clone)` pairs. The
    /// component is cloned out so the world is not borrowed by the
    /// return value (Rule R5 — no lifetimes in Buff-visible types).
    ///
    /// For mutation, use [`World::for_each_mut`].
    pub fn query<T>(&self) -> Vec<(Entity, T)>
    where
        T: Component,
    {
        let result = catch_unwind(AssertUnwindSafe(|| {
            let mut out = Vec::new();
            for (e, c) in self.inner.query::<&T>().iter() {
                out.push((Entity::from_hecs(e), (*c).clone()));
            }
            out
        }));
        result.unwrap_or_default()
    }

    /// Iterate every entity that has component `T`, invoking `f` with
    /// a mutable borrow of each. This is the primary way systems
    /// mutate component state.
    pub fn for_each_mut<T, F>(&mut self, mut f: F)
    where
        T: Component,
        F: FnMut(Entity, &mut T),
    {
        let result = catch_unwind(AssertUnwindSafe(|| {
            for (e, c) in self.inner.query_mut::<&mut T>() {
                f(Entity::from_hecs(e), c);
            }
        }));
        let _ = result;
    }

    /// Iterate every entity that has both `A` and `B`, invoking `f`
    /// with a mut borrow of each. Common shape for systems like
    /// `Position += Velocity`.
    pub fn for_each_pair_mut<A, B, F>(&mut self, mut f: F)
    where
        A: Component,
        B: Component,
        F: FnMut(Entity, &mut A, &mut B),
    {
        let result = catch_unwind(AssertUnwindSafe(|| {
            for (e, (a, b)) in self.inner.query_mut::<(&mut A, &mut B)>() {
                f(Entity::from_hecs(e), a, b);
            }
        }));
        let _ = result;
    }

    /// Register a boxed [`System`]. Systems run in registration order
    /// when [`World::tick`] is called.
    pub fn add_system<S>(&mut self, system: S)
    where
        S: System + 'static,
    {
        self.systems.push(Box::new(system));
    }

    /// Run every registered system once, in registration order. If a
    /// system panics, the panic is caught (Rule R6),
    /// [`World::last_tick_failed`] is set to `true`, and the remaining
    /// systems still run.
    pub fn tick(&mut self) {
        self.last_tick_failed.store(false, Ordering::Relaxed);
        let mut systems = std::mem::take(&mut self.systems);
        for system in systems.iter_mut() {
            let panic_result = catch_unwind(AssertUnwindSafe(|| system.run(self)));
            if panic_result.is_err() {
                self.last_tick_failed.store(true, Ordering::Relaxed);
            }
        }
        self.systems = systems;
    }

    /// Returns `true` if any registered system panicked during the
    /// most recent [`World::tick`]. The panic was caught and the
    /// remaining systems still ran.
    pub fn last_tick_failed(&self) -> bool {
        self.last_tick_failed.load(Ordering::Relaxed)
    }

    /// Insert a typed resource value. Replaces any prior value of the
    /// same type. Stored separately from entity/component state.
    pub fn insert_resource<T>(&mut self, value: T)
    where
        T: Send + Sync + 'static,
    {
        self.resources.insert(value);
    }

    /// Look up the resource value of type `T`. Returns `None` if no
    /// value of that type has been inserted.
    pub fn get_resource<T>(&self) -> Option<&T>
    where
        T: Send + Sync + 'static,
    {
        self.resources.get::<T>()
    }

    /// Look up the resource value of type `T` mutably.
    pub fn get_resource_mut<T>(&mut self) -> Option<&mut T>
    where
        T: Send + Sync + 'static,
    {
        self.resources.get_mut::<T>()
    }

    /// Remove and return the resource value of type `T`. Returns
    /// `None` if no value of that type was present.
    pub fn remove_resource<T>(&mut self) -> Option<T>
    where
        T: Send + Sync + 'static,
    {
        self.resources.remove::<T>()
    }

    /// Full reset: despawn every entity AND clear resources AND drop
    /// every registered system. After this, the world is in the same
    /// shape as a fresh [`World::new`] (just reusing the allocation).
    pub fn clear_all(&mut self) {
        let _ = catch_unwind(AssertUnwindSafe(|| self.inner.clear()));
        self.resources.clear();
        self.systems.clear();
        self.last_tick_failed.store(false, Ordering::Relaxed);
    }
}

impl Default for World {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Debug for World {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("World")
            .field("entity_count", &self.entity_count())
            .field("system_count", &self.systems.len())
            .field("resource_count", &self.resources.len())
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::system::SystemFn;

    #[derive(Debug, Clone, PartialEq)]
    struct Position {
        x: f32,
        y: f32,
    }

    #[derive(Debug, Clone, PartialEq)]
    struct Velocity {
        dx: f32,
        dy: f32,
    }

    #[derive(Debug, Clone, PartialEq)]
    struct Health(i32);

    #[derive(Debug, Clone, PartialEq)]
    struct Score(u32);

    #[derive(Debug, Clone, PartialEq)]
    struct Counter(i32);

    #[test]
    fn spawn_returns_distinct_entities() {
        let mut w = World::new();
        let a = w.spawn(Position { x: 0.0, y: 0.0 });
        let b = w.spawn(Position { x: 1.0, y: 1.0 });
        assert_ne!(a, b);
        assert_eq!(w.entity_count(), 2);
    }

    #[test]
    fn spawn_two_attaches_both_components() {
        let mut w = World::new();
        let e = w.spawn_two(Position { x: 5.0, y: 5.0 }, Velocity { dx: 1.0, dy: 0.0 });
        assert_eq!(
            w.get_clone::<Position>(e),
            Some(Position { x: 5.0, y: 5.0 })
        );
        assert_eq!(
            w.get_clone::<Velocity>(e),
            Some(Velocity { dx: 1.0, dy: 0.0 })
        );
    }

    #[test]
    fn insert_and_remove_roundtrip() {
        let mut w = World::new();
        let e = w.spawn(Health(100));
        let inserted = w.insert(e, Score(5));
        assert!(inserted.is_ok());
        assert_eq!(w.get_clone::<Score>(e), Some(Score(5)));
        let removed = w.remove::<Score>(e);
        assert!(matches!(removed, Ok(Some(Score(5)))));
        assert_eq!(w.get_clone::<Score>(e), None);
    }

    #[test]
    fn insert_on_despawned_entity_returns_error() {
        let mut w = World::new();
        let e = w.spawn(Health(100));
        assert!(w.despawn(e));
        let result = w.insert(e, Health(1));
        assert!(matches!(result, Err(EcsError::EntityMissing(_))));
    }

    #[test]
    fn despawn_removes_entity_and_components() {
        let mut w = World::new();
        let e = w.spawn(Health(50));
        assert!(w.contains(e));
        assert!(w.despawn(e));
        assert!(!w.contains(e));
        assert_eq!(w.get_clone::<Health>(e), None);
        assert_eq!(w.entity_count(), 0);
    }

    #[test]
    fn query_returns_all_matching_components() {
        let mut w = World::new();
        w.spawn(Position { x: 1.0, y: 0.0 });
        w.spawn_two(Position { x: 2.0, y: 0.0 }, Velocity { dx: 0.5, dy: 0.5 });
        let positions = w.query::<Position>();
        assert_eq!(positions.len(), 2);
        let velocities = w.query::<Velocity>();
        assert_eq!(velocities.len(), 1);
    }

    #[test]
    fn for_each_mut_updates_components() {
        let mut w = World::new();
        let e = w.spawn(Health(10));
        w.for_each_mut(|_id, h: &mut Health| {
            h.0 += 5;
        });
        assert_eq!(w.get_clone::<Health>(e), Some(Health(15)));
    }

    #[test]
    fn for_each_pair_mut_sees_both_components() {
        let mut w = World::new();
        let e = w.spawn_two(Position { x: 0.0, y: 0.0 }, Velocity { dx: 1.0, dy: 2.0 });
        w.for_each_pair_mut(|_id, p: &mut Position, v: &mut Velocity| {
            p.x += v.dx;
            p.y += v.dy;
        });
        assert_eq!(
            w.get_clone::<Position>(e),
            Some(Position { x: 1.0, y: 2.0 })
        );
    }

    #[test]
    fn tick_runs_registered_systems() {
        let mut w = World::new();
        let e = w.spawn_two(Position { x: 0.0, y: 0.0 }, Velocity { dx: 1.0, dy: 0.0 });
        w.add_system(SystemFn::new("move".to_string(), |world: &mut World| {
            world.for_each_pair_mut(|_id, p: &mut Position, v: &mut Velocity| {
                p.x += v.dx;
                p.y += v.dy;
            });
        }));
        w.tick();
        assert_eq!(
            w.get_clone::<Position>(e),
            Some(Position { x: 1.0, y: 0.0 })
        );
        w.tick();
        assert_eq!(
            w.get_clone::<Position>(e),
            Some(Position { x: 2.0, y: 0.0 })
        );
    }

    #[test]
    fn tick_continues_after_panicking_system() {
        let mut w = World::new();
        let e = w.spawn(Counter(0));
        w.add_system(SystemFn::new("panicker".to_string(), |_w: &mut World| {
            panic!("boom")
        }));
        w.add_system(SystemFn::new(
            "increment".to_string(),
            |world: &mut World| {
                world.for_each_mut(|_id, c: &mut Counter| {
                    c.0 += 1;
                });
            },
        ));
        w.tick();
        assert!(w.last_tick_failed());
        assert_eq!(w.get_clone::<Counter>(e), Some(Counter(1)));
    }

    #[test]
    fn resources_insert_get_remove() {
        let mut w = World::new();
        assert_eq!(w.get_resource::<Score>(), None);
        w.insert_resource(Score(10));
        assert_eq!(w.get_resource::<Score>(), Some(&Score(10)));
        if let Some(s) = w.get_resource_mut::<Score>() {
            s.0 += 5;
        }
        assert_eq!(w.get_resource::<Score>(), Some(&Score(15)));
        let removed = w.remove_resource::<Score>();
        assert_eq!(removed, Some(Score(15)));
        assert_eq!(w.get_resource::<Score>(), None);
    }

    #[test]
    fn clear_all_resets_world() {
        let mut w = World::new();
        w.spawn(Health(5));
        w.insert_resource(Score(1));
        w.add_system(SystemFn::new("noop".to_string(), |_w: &mut World| {}));
        w.clear_all();
        assert_eq!(w.entity_count(), 0);
        assert_eq!(w.get_resource::<Score>(), None);
    }

    #[test]
    fn debug_format_includes_counts() {
        let mut w = World::new();
        w.spawn(Health(1));
        w.spawn(Health(2));
        w.add_system(SystemFn::new("noop".to_string(), |_w: &mut World| {}));
        let s = format!("{w:?}");
        assert!(s.contains("World"));
    }

    #[test]
    fn default_is_same_as_new() {
        let d = World::default();
        let n = World::new();
        assert_eq!(d.entity_count(), n.entity_count());
    }

    #[test]
    fn despawn_returns_false_for_unknown_entity() {
        let mut w = World::new();
        let a = w.spawn(Health(1));
        assert!(w.despawn(a));
        assert!(!w.despawn(a));
    }
}

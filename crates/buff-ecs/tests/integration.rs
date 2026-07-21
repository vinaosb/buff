//! Integration tests for `buff-ecs` — cross-module scenarios
//! covering the full World lifecycle: spawn → system → resource.
//!
//! Mirrors the three T12 acceptance scenarios:
//! - `spawn_query_roundtrip` (evidence: task-12-spawn)
//! - `system_modifies_components` (evidence: task-12-system)
//! - `resource_lifecycle` (evidence: task-12-resource)
//!
//! Plus snapshot tests that freeze the `Debug` format of `World`
//! at each lifecycle stage so future refactors that change the
//! diagnostic surface are caught by insta.

#![allow(clippy::float_cmp)]

use buff_ecs::{EcsError, SystemFn, World};

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
struct Tag(&'static str);

// ===== QA scenario 1: spawn + query roundtrip =====

#[test]
fn spawn_query_roundtrip() {
    let mut world = World::new();
    let _e1 = world.spawn(Position { x: 0.0, y: 0.0 });
    let _e2 = world.spawn_two(
        Position { x: 1.0, y: 1.0 },
        Velocity { dx: 1.0, dy: 0.0 },
    );

    let positions: Vec<_> = world.query::<Position>();
    assert_eq!(positions.len(), 2, "both entities have Position");

    let velocities: Vec<_> = world.query::<Velocity>();
    assert_eq!(velocities.len(), 1, "only one entity has Velocity");
    assert_eq!(velocities[0].1, Velocity { dx: 1.0, dy: 0.0 });
}

// ===== QA scenario 2: system modifies components on tick =====

#[test]
fn system_modifies_components() {
    let mut world = World::new();
    let player = world.spawn_two(
        Position { x: 0.0, y: 0.0 },
        Velocity { dx: 1.0, dy: 0.0 },
    );

    world.add_system(SystemFn::new("move".to_string(), |w: &mut World| {
        w.for_each_pair_mut(|_id, p: &mut Position, v: &mut Velocity| {
            p.x += v.dx;
            p.y += v.dy;
        });
    }));

    world.tick();
    assert_eq!(
        world.get_clone::<Position>(player),
        Some(Position { x: 1.0, y: 0.0 })
    );

    world.tick();
    assert_eq!(
        world.get_clone::<Position>(player),
        Some(Position { x: 2.0, y: 0.0 })
    );
}

// ===== QA scenario 3: resource lifecycle =====

#[test]
fn resource_lifecycle() {
    let mut world = World::new();
    assert!(world.get_resource::<Score>().is_none());

    world.insert_resource(Score(0));
    assert_eq!(world.get_resource::<Score>(), Some(&Score(0)));

    if let Some(score) = world.get_resource_mut::<Score>() {
        score.0 += 10;
    }
    assert_eq!(world.get_resource::<Score>(), Some(&Score(10)));

    let removed = world.remove_resource::<Score>();
    assert_eq!(removed, Some(Score(10)));
    assert!(world.get_resource::<Score>().is_none());
}

// ===== Snapshots: World Debug format at each lifecycle stage =====

#[test]
fn snapshot_world_empty() {
    let world = World::new();
    insta::assert_snapshot!(format!("{world:?}"), @"World { entity_count: 0, system_count: 0, resource_count: 0, .. }");
}

#[test]
fn snapshot_world_after_spawn() {
    let mut world = World::new();
    world.spawn(Health(100));
    world.spawn_two(
        Position { x: 1.0, y: 2.0 },
        Velocity { dx: 0.5, dy: -0.5 },
    );
    insta::assert_snapshot!(format!("{world:?}"), @"World { entity_count: 2, system_count: 0, resource_count: 0, .. }");
}

#[test]
fn snapshot_world_with_system_and_resource() {
    let mut world = World::new();
    world.spawn(Position { x: 0.0, y: 0.0 });
    world.add_system(SystemFn::new("move".to_string(), |_w: &mut World| {}));
    world.insert_resource(Score(42));
    world.insert_resource(Tag("player"));
    insta::assert_snapshot!(format!("{world:?}"), @"World { entity_count: 1, system_count: 1, resource_count: 2, .. }");
}

#[test]
fn snapshot_world_after_tick() {
    let mut world = World::new();
    let _e = world.spawn_two(
        Position { x: 0.0, y: 0.0 },
        Velocity { dx: 1.0, dy: 1.0 },
    );
    world.add_system(SystemFn::new("move".to_string(), |w: &mut World| {
        w.for_each_pair_mut(|_id, p: &mut Position, v: &mut Velocity| {
            p.x += v.dx;
            p.y += v.dy;
        });
    }));
    for _ in 0..5 {
        world.tick();
    }
    insta::assert_snapshot!(format!("{world:?}"), @"World { entity_count: 1, system_count: 1, resource_count: 0, .. }");
}

#[test]
fn snapshot_world_after_clear_all() {
    let mut world = World::new();
    world.spawn(Health(5));
    world.insert_resource(Score(1));
    world.add_system(SystemFn::new("noop".to_string(), |_w: &mut World| {}));
    world.clear_all();
    insta::assert_snapshot!(format!("{world:?}"), @"World { entity_count: 0, system_count: 0, resource_count: 0, .. }");
}

// ===== Edge cases: error paths =====

#[test]
fn insert_on_missing_entity_returns_error() {
    let mut world = World::new();
    let e = world.spawn(Health(1));
    assert!(world.despawn(e));
    match world.insert(e, Health(2)) {
        Err(EcsError::EntityMissing(id)) => assert_eq!(id, e.id()),
        other => panic!("expected EntityMissing, got {other:?}"),
    }
}

#[test]
fn remove_missing_component_returns_ok_none() {
    let mut world = World::new();
    let e = world.spawn(Health(10));
    let removed: Result<Option<Score>, _> = world.remove::<Score>(e);
    assert!(matches!(removed, Ok(None)));
}

#[test]
fn tick_continues_after_panic_and_sets_flag() {
    let mut world = World::new();
    let e = world.spawn(Health(0));
    world.add_system(SystemFn::new("boom".to_string(), |_w: &mut World| panic!("kaboom")));
    world.add_system(SystemFn::new("heal".to_string(), |w: &mut World| {
        w.for_each_mut(|_id, h: &mut Health| {
            h.0 += 5;
        });
    }));
    world.tick();
    assert!(world.last_tick_failed());
    assert_eq!(world.get_clone::<Health>(e), Some(Health(5)));
}

#[test]
fn multiple_systems_run_in_registration_order() {
    let mut world = World::new();
    let e = world.spawn(Health(0));
    world.add_system(SystemFn::new("first".to_string(), |w: &mut World| {
        w.for_each_mut(|_id, h: &mut Health| {
            h.0 += 1;
        });
    }));
    world.add_system(SystemFn::new("second".to_string(), |w: &mut World| {
        w.for_each_mut(|_id, h: &mut Health| {
            h.0 += 10;
        });
    }));
    world.tick();
    assert_eq!(world.get_clone::<Health>(e), Some(Health(11)));
}

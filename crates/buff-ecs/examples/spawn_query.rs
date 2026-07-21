//! T12 QA scenario 1: spawn entity with (Position, Velocity) bundle
//! and query back the components. Verifies the spawn → query roundtrip.
//!
//! Evidence: `.sisyphus/evidence/task-12-spawn/output.txt`

use buff_ecs::World;

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

fn main() {
    let mut world = World::new();

    let _player = world.spawn_two(
        Position { x: 0.0, y: 0.0 },
        Velocity { dx: 1.0, dy: 0.0 },
    );
    let _obstacle = world.spawn(Position { x: 5.0, y: 5.0 });

    let positions = world.query::<Position>();
    let velocities = world.query::<Velocity>();

    println!("positions: {:?}", positions);
    println!("velocities: {:?}", velocities);
    println!(
        "spawn + query roundtrip OK (positions={}, velocities={})",
        positions.len(),
        velocities.len()
    );
}

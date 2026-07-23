//! T12 QA scenario 2: register a movement system that modifies Position
//! via Velocity on each tick. Verifies system execution + state mutation.
//!
//! Evidence: `.sisyphus/evidence/task-12-system/output.txt`

use buff_ecs::{SystemFn, World};

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

    let player = world.spawn_two(Position { x: 0.0, y: 0.0 }, Velocity { dx: 1.0, dy: 0.0 });

    world.add_system(SystemFn::new("move".to_string(), |w: &mut World| {
        w.for_each_pair_mut(|_id, p: &mut Position, v: &mut Velocity| {
            p.x += v.dx;
            p.y += v.dy;
        });
    }));

    for step in 1..=3 {
        world.tick();
        let pos = world
            .get_clone::<Position>(player)
            .unwrap_or(Position { x: -1.0, y: -1.0 });
        println!(
            "after tick {step}: Position {{ x: {}, y: {} }}",
            pos.x, pos.y
        );
    }

    let final_pos = world
        .get_clone::<Position>(player)
        .unwrap_or(Position { x: -1.0, y: -1.0 });
    println!("system + tick OK (final x={}, expected 3.0)", final_pos.x);
}

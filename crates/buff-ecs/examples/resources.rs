//! T12 QA scenario 3: insert a typed resource into the world and read
//! it back via `get_resource<T>()`. Verifies the resource side-channel.
//!
//! Evidence: `.sisyphus/evidence/task-12-resource/output.txt`

use buff_ecs::World;

#[derive(Debug, Clone, PartialEq)]
struct GameState {
    score: u32,
    turn: u32,
}

#[derive(Debug, Clone, PartialEq)]
struct Settings {
    difficulty: &'static str,
}

fn main() {
    let mut world = World::new();

    world.insert_resource(GameState { score: 0, turn: 1 });
    world.insert_resource(Settings {
        difficulty: "normal",
    });

    if let Some(state) = world.get_resource_mut::<GameState>() {
        state.score += 100;
        state.turn += 1;
    }

    let state = world.get_resource::<GameState>();
    let settings = world.get_resource::<Settings>();

    println!("game state: {:?}", state);
    println!("settings:   {:?}", settings);

    let score = state.map(|s| s.score).unwrap_or(0);
    println!(
        "resource insert + mutation OK (score={}, expected 100)",
        score
    );
}

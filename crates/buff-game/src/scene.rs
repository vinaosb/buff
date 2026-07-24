//! Scene lifecycle: trait + simple closure-backed scene.
//!
//! Scenes are the unit of "game state" in `buff-game`. A scene's
//! [`on_enter`](Scene::on_enter) method is called once when the scene
//! is first pushed; [`on_update`](Scene::on_update) is called every
//! [`Game::step`](crate::Game::step). Scenes can query and mutate
//! the ECS [`World`](crate::World), issue draw calls via
//! [`Renderer`](crate::Renderer), read [`Input`](crate::Input), and
//! load assets via [`Asset`](crate::Asset) — all through the
//! `&mut Game` reference the callback receives.
//!
//! # Object safety
//!
//! `Scene` is object-safe + `Send` so it can be boxed and stored in
//! a `Vec<Box<dyn Scene>>` on the game loop. The blanket
//! `SimpleScene` wraps a closure for ad-hoc scenes; structured scenes
//! implement the trait directly.

use crate::game::Game;

/// The scene lifecycle contract.
///
/// Implement this trait for structured game states (menu, level, pause
/// screen). For one-off scenes or rapid prototyping, use
/// [`SimpleScene::new`] to wrap a closure.
///
/// # Example
///
/// ```no_run
/// use buff_game::{Scene, Game};
///
/// struct Level1;
///
/// impl Scene for Level1 {
///     fn on_enter(&mut self, _game: &mut Game) {
///         // spawn player entity, load assets
///     }
///
///     fn on_update(&mut self, game: &mut Game, dt: f32) {
///         // game logic
///     }
/// }
/// ```
pub trait Scene: Send {
    /// Called once when the scene becomes the active scene (the first
    /// [`Game::step`](crate::Game::step) after the scene is pushed
    /// via [`Game::add_scene`](crate::Game::add_scene)). Use it to
    /// spawn entities, load assets, and configure the ECS world.
    fn on_enter(&mut self, game: &mut Game);

    /// Called every [`Game::step`](crate::Game::step) while this scene
    /// is the active scene. `dt` is the fixed-timestep interval (in
    /// seconds) passed to `step`. Issue draw calls, move entities,
    /// check input.
    fn on_update(&mut self, game: &mut Game, dt: f32);
}

/// A [`Scene`] backed by two closures (enter + update).
///
/// Useful for ad-hoc scenes or prototyping — avoids defining a named
/// struct + `impl Scene`. The closures are boxed and `Send` (required
/// for object safety in the game loop's `Vec<Box<dyn Scene>>`).
///
/// # Example
///
/// ```no_run
/// use buff_game::SimpleScene;
///
/// let scene = SimpleScene::new("hello", |_game, _dt| {
///     // called every step
/// });
/// ```
pub struct SimpleScene {
    /// Human-readable scene name (shown in debug output / diagnostics).
    name: String,
    /// Closure called once on first step.
    enter_fn: Option<Box<dyn FnOnce(&mut Game) + Send>>,
    /// Closure called every step.
    update_fn: Box<dyn FnMut(&mut Game, f32) + Send>,
    /// Whether `on_enter` has been called. Set to `true` after the
    /// first step; prevents the enter closure from being called twice.
    entered: bool,
}

impl SimpleScene {
    /// Construct a scene with an empty enter callback and the given
    /// `update` closure. The enter closure can be added later via
    /// [`SimpleScene::with_enter`].
    pub fn new<F>(name: impl Into<String>, update: F) -> Self
    where
        F: FnMut(&mut Game, f32) + Send + 'static,
    {
        Self {
            name: name.into(),
            enter_fn: None,
            update_fn: Box::new(update),
            entered: false,
        }
    }

    /// Builder: attach an `on_enter` closure. Consumes self for
    /// chaining.
    pub fn with_enter<F>(mut self, enter: F) -> Self
    where
        F: FnOnce(&mut Game) + Send + 'static,
    {
        self.enter_fn = Some(Box::new(enter));
        self
    }

    /// The scene's name (diagnostic / debug output).
    pub(crate) fn name(&self) -> &str {
        &self.name
    }

    /// Whether `on_enter` has been called. Test helper.
    pub(crate) fn has_entered(&self) -> bool {
        self.entered
    }
}

impl Scene for SimpleScene {
    fn on_enter(&mut self, game: &mut Game) {
        self.entered = true;
        if let Some(f) = self.enter_fn.take() {
            f(game);
        }
    }

    fn on_update(&mut self, game: &mut Game, dt: f32) {
        (self.update_fn)(game, dt);
    }
}

impl std::fmt::Debug for SimpleScene {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SimpleScene")
            .field("name", &self.name)
            .field("entered", &self.entered)
            .finish_non_exhaustive()
    }
}

impl std::fmt::Display for SimpleScene {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Scene({})", self.name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    #[test]
    fn simple_scene_enter_called_once() {
        let called = Arc::new(Mutex::new(0u32));
        let c = Arc::clone(&called);
        let mut s = SimpleScene::new("test", |_g, _dt| {}).with_enter(move |_g| {
            *c.lock().expect("mutex") += 1;
        });
        // Simulate what Game.step does.
        let cfg = crate::GameConfig::new(100, 100, "t");
        let mut g = crate::Game::new(cfg);
        assert!(!s.has_entered());
        s.on_enter(&mut g);
        assert!(s.has_entered());
        s.on_enter(&mut g); // second call should be a no-op
        s.on_enter(&mut g);
        assert_eq!(*called.lock().expect("mutex"), 1);
    }

    #[test]
    fn simple_scene_update_called_every_step() {
        let count = Arc::new(Mutex::new(0u32));
        let c = Arc::clone(&count);
        let mut s = SimpleScene::new("test", move |_g, _dt| {
            *c.lock().expect("mutex") += 1;
        });
        let cfg = crate::GameConfig::new(100, 100, "t");
        let mut g = crate::Game::new(cfg);
        s.on_enter(&mut g);
        s.on_update(&mut g, 1.0 / 60.0);
        s.on_update(&mut g, 1.0 / 60.0);
        s.on_update(&mut g, 1.0 / 60.0);
        assert_eq!(*count.lock().expect("mutex"), 3);
    }

    #[test]
    fn simple_scene_name_accessors() {
        let s = SimpleScene::new("level1", |_g, _dt| {});
        assert_eq!(s.name(), "level1");
        assert!(!s.has_entered());
    }

    #[test]
    fn simple_scene_debug_format() {
        let s = SimpleScene::new("demo", |_g, _dt| {});
        let dbg = format!("{s:?}");
        assert!(dbg.contains("demo"));
        assert!(dbg.contains("SimpleScene"));
    }

    #[test]
    fn simple_scene_display_format() {
        let s = SimpleScene::new("menu", |_g, _dt| {});
        let disp = format!("{s}");
        assert!(disp.contains("menu"));
    }
}

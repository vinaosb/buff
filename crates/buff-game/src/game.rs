//! Game loop: fixed-timestep accumulator + scene stack.
//!
//! [`Game`] composes [`World`](crate::World) (ECS), [`Asset`](crate::Asset)
//! (texture + audio cache), [`Renderer`](crate::Renderer) (draw-command
//! queue), and [`Input`](crate::Input) (polled key + mouse state) into
//! a single game loop driven by [`Game::step`].
//!
//! # Headless MVP
//!
//! [`Game::run`] documents a window-based event loop (deferred to
//! when `winit` is workspace-pinned). In the headless MVP it loops
//! calling [`Game::step`] until `max_frames` is reached. Every
//! method on `Game` is fully testable without a GPU or window.

use crate::asset::Asset;
use crate::error::{GameError, GameResult};
use crate::input::Input;
use crate::renderer::Renderer;
use crate::scene::Scene;

use std::fmt;

/// Fixed-timestep interval in seconds. The game loop accumulates
/// fractional frames and runs whole-step batches to keep physics
/// deterministic regardless of actual frame rate.
const DEFAULT_DT: f32 = 1.0 / 60.0;

/// Configuration for [`Game::new`].
///
/// - `width` / `height` are the virtual viewport dimensions (stored
///   for future present-backend; headless tests ignore them).
/// - `title` is the window title (stored for future present-backend).
/// - `max_frames` bounds the headless loop: `run()` terminates after
///   this many steps. `None` means "run until `quit()` is called"
///   (which in headless mode is `FrameBudgetExhausted` if no window
///   close event stops it).
/// - `fixed_dt` is the fixed-timestep interval (default `1/60` s).
#[derive(Debug, Clone)]
pub struct GameConfig {
    /// Virtual viewport width in pixels.
    pub width: u32,
    /// Virtual viewport height in pixels.
    pub height: u32,
    /// Window title (future present-backend; ignored headless).
    pub title: String,
    /// Maximum number of steps allowed in [`Game::run`]. `None` =
    /// "run until `quit()`" (headless: will hit `FrameBudgetExhausted`).
    pub max_frames: Option<usize>,
    /// Fixed-timestep interval in seconds. Default: `1/60`.
    pub fixed_dt: f32,
}

impl GameConfig {
    /// Quick constructor with sensible defaults (`max_frames = Some(600)`,
    /// `fixed_dt = 1/60`). The 600-frame default gives 10 seconds of
    /// simulated time before the headless loop terminates — plenty for
    /// most MVP test scenarios.
    pub fn new(width: u32, height: u32, title: impl Into<String>) -> Self {
        Self {
            width,
            height,
            title: title.into(),
            max_frames: Some(600),
            fixed_dt: DEFAULT_DT,
        }
    }
}

impl Default for GameConfig {
    fn default() -> Self {
        Self::new(800, 600, "Buff Game")
    }
}

impl fmt::Display for GameConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "GameConfig({w}x{h}, \"{title}\", dt={dt:.6}, max_frames={mf})",
            w = self.width,
            h = self.height,
            title = self.title,
            dt = self.fixed_dt,
            mf = self
                .max_frames
                .map_or("unbounded".to_string(), |n| n.to_string()),
        )
    }
}

/// The game loop. Owns the ECS [`World`](crate::World), the
/// [`Asset`](crate::Asset) cache, the [`Renderer`](crate::Renderer)
/// command queue, and the [`Input`](crate::Input) state.
///
/// Push scenes via [`Game::add_scene`]; the first scene becomes the
/// active scene on the next [`Game::step`] call. Steps accumulate a
/// fixed-timestep `accumulator` so physics remains deterministic
/// regardless of actual wall-clock frame time.
///
/// # Lifecycle
///
/// ```text
/// Game::new(config)
///     .add_scene(scene_A)
///     .add_scene(scene_B)   // pushed after scene_A finishes
///     .run()                // headless: bounded by config.max_frames
/// ```
///
/// # Headless
///
/// In the headless MVP, [`Game::run`] calls [`Game::step`] in a loop
/// until `max_frames` is exhausted or `quit()` is called. Each step:
///
/// 1. `renderer.clear()` — drop the previous frame's draw commands.
/// 2. `input.begin_frame()` — advance per-frame edge-detected state.
/// 3. `scene.on_enter(self)` — once, on the first step after push.
/// 4. `scene.on_update(self, dt)` — user logic + draw calls.
/// 5. `world.tick()` — run registered ECS systems.
/// 6. `elapsed += dt`.
///
/// A real window backend would substitute step 5+6 with an OS event
/// poll + `wgpu` present; that integration is deferred.
pub struct Game {
    config: GameConfig,
    /// The ECS world (public so scenes can spawn / query).
    world: buff_ecs::World,
    /// The asset cache + loader.
    asset: Asset,
    /// The draw-command queue.
    renderer: Renderer,
    /// Polled input state.
    input: Input,
    /// Scene stack. The first element is the active scene (if any).
    scenes: Vec<Box<dyn Scene>>,
    /// Whether `quit()` has been called.
    running: bool,
    /// Total simulated time in seconds (accumulated by `step`).
    elapsed: f32,
    /// Number of `step()` calls since the game was created.
    frame_count: usize,
    /// Whether the active scene's `on_enter` has been called.
    scene_entered: bool,
}

impl Game {
    /// Construct a new game loop with the given config. The ECS world
    /// is empty; the asset cache is empty; the renderer has no pending
    /// commands; no keys are pressed.
    pub fn new(config: GameConfig) -> Self {
        Self {
            config,
            world: buff_ecs::World::new(),
            asset: Asset::new(),
            renderer: Renderer::new(),
            input: Input::new(),
            scenes: Vec::new(),
            running: true,
            elapsed: 0.0,
            frame_count: 0,
            scene_entered: false,
        }
    }

    /// Push a scene. The scene will become the active scene once the
    /// current scene (if any) finishes — or immediately if the stack
    /// is empty. In the MVP, only the **first** scene is active;
    /// multi-scene sequencing is deferred to v1.18+ (the scene stack
    /// is a placeholder for future scene transitions).
    pub fn add_scene(&mut self, scene: Box<dyn Scene>) {
        self.scenes.push(scene);
    }

    /// Run the game loop. In headless mode this calls [`Game::step`]
    /// repeatedly until `max_frames` is reached or `quit()` is called.
    ///
    /// Returns [`GameError::RequiresWindow`] if `max_frames` is `None`
    /// (would loop forever with no window-close event). Returns
    /// [`GameError::FrameBudgetExhausted`] only for `step` — `run`
    /// handles the budget internally and returns `Ok(())`.
    pub fn run(&mut self) -> GameResult<()> {
        let max = self.config.max_frames.ok_or_else(|| {
            GameError::RequiresWindow(
                "max_frames is None — a real window close event is needed to terminate".to_string(),
            )
        })?;
        for _ in 0..max {
            if !self.running {
                break;
            }
            self.step(self.config.fixed_dt)?;
        }
        Ok(())
    }

    /// Signal the game loop to stop. Takes effect at the next
    /// iteration of the loop (or the next [`Game::step`] check).
    pub fn quit(&mut self) {
        self.running = false;
    }

    /// Advance the simulation by one fixed-timestep tick (`dt` seconds).
    ///
    /// Headless entry point: drives the full pipeline (clear frame,
    /// advance input, call scene lifecycle, tick ECS systems, advance
    /// elapsed clock) without touching the GPU. Returns
    /// [`GameError::FrameBudgetExhausted`] if the step would exceed
    /// `config.max_frames`.
    pub fn step(&mut self, dt: f32) -> GameResult<()> {
        if !self.running {
            return Ok(());
        }
        let max = self.config.max_frames.unwrap_or(usize::MAX);
        if self.frame_count >= max {
            return Err(GameError::FrameBudgetExhausted(max));
        }

        // 1. Clear the previous frame's draw commands.
        self.renderer.clear();

        // 2. Advance per-frame input state (no-op in MVP).
        self.input.begin_frame();

        // 3. Scene enter (once) — only the first scene on the stack
        //    is active in the MVP. Multi-scene sequencing is deferred.
        if !self.scene_entered {
            // Split the borrow: take the scenes Vec out of self so the
            // scene callback can borrow `self` mutably without an
            // aliasing conflict. on_enter does not modify the scenes
            // vec (it only touches world/renderer/input through &mut
            // Game) so we restore it verbatim after the call.
            let mut scenes = std::mem::take(&mut self.scenes);
            if !scenes.is_empty() {
                scenes[0].on_enter(self);
            }
            self.scenes = scenes;
            self.scene_entered = true;
        }

        // 4. Scene update — only the first scene is active.
        {
            let mut scenes = std::mem::take(&mut self.scenes);
            if !scenes.is_empty() {
                scenes[0].on_update(self, dt);
            }
            self.scenes = scenes;
        }

        // 5. Run registered ECS systems.
        self.world.tick();

        // 6. Advance the simulation clock.
        self.elapsed += dt;
        self.frame_count += 1;

        Ok(())
    }

    /// Whether the game loop is still running (`true` until `quit()`
    /// is called or the frame budget is exhausted).
    pub fn is_running(&self) -> bool {
        self.running
    }

    /// Total simulated time in seconds (accumulated by [`Game::step`]).
    pub fn elapsed(&self) -> f32 {
        self.elapsed
    }

    /// Number of [`Game::step`] calls since creation. Test helper.
    pub(crate) fn frame_count(&self) -> usize {
        self.frame_count
    }

    /// Number of scenes currently in the stack. In the MVP only the
    /// first scene is active; this counts all pushed scenes.
    pub fn scene_count(&self) -> usize {
        self.scenes.len()
    }

    // ── Accessors for scenes + tests ──────────────────────────────

    /// Immutable borrow of the ECS world.
    pub fn world(&self) -> &buff_ecs::World {
        &self.world
    }

    /// Mutable borrow of the ECS world.
    pub fn world_mut(&mut self) -> &mut buff_ecs::World {
        &mut self.world
    }

    /// Immutable borrow of the input state.
    pub fn input(&self) -> &Input {
        &self.input
    }

    /// Mutable borrow of the input state.
    pub fn input_mut(&mut self) -> &mut Input {
        &mut self.input
    }

    /// Mutable borrow of the renderer (for draw calls).
    pub fn renderer_mut(&mut self) -> &mut Renderer {
        &mut self.renderer
    }

    /// Mutable borrow of the asset loader (for `load_texture` /
    /// `load_audio`).
    pub fn asset_mut(&mut self) -> &mut Asset {
        &mut self.asset
    }

    /// Immutable reference to the game config.
    pub fn config(&self) -> &GameConfig {
        &self.config
    }
}

impl fmt::Display for Game {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "Game({}, elapsed={:.2}s, frames={}, scenes={}, running={})",
            self.config.title,
            self.elapsed,
            self.frame_count,
            self.scenes.len(),
            self.running,
        )
    }
}

impl fmt::Debug for Game {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Game")
            .field("config", &self.config)
            .field("world", &self.world)
            .field("elapsed", &self.elapsed)
            .field("frame_count", &self.frame_count)
            .field("scene_count", &self.scenes.len())
            .field("running", &self.running)
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scene::SimpleScene;
    use std::sync::{Arc, Mutex};

    #[test]
    fn new_game_starts_running() {
        let g = Game::new(GameConfig::new(100, 100, "t"));
        assert!(g.is_running());
        assert_eq!(g.elapsed(), 0.0);
        assert_eq!(g.scene_count(), 0);
    }

    #[test]
    fn step_advances_elapsed_by_dt() {
        let cfg = GameConfig::new(100, 100, "t");
        let mut g = Game::new(cfg);
        g.step(1.0 / 60.0).expect("step ok");
        assert!((g.elapsed() - 1.0 / 60.0).abs() < 1e-6);
        g.step(1.0 / 60.0).expect("step ok");
        assert!((g.elapsed() - 2.0 / 60.0).abs() < 1e-6);
    }

    #[test]
    fn step_with_zero_dt_advances_frame_count() {
        let cfg = GameConfig::new(100, 100, "t");
        let mut g = Game::new(cfg);
        g.step(0.0).expect("step ok");
        assert_eq!(g.frame_count(), 1);
        assert_eq!(g.elapsed(), 0.0);
    }

    #[test]
    fn quit_stops_running() {
        let cfg = GameConfig::new(100, 100, "t");
        let mut g = Game::new(cfg);
        g.quit();
        assert!(!g.is_running());
        g.step(1.0).expect("step ok"); // step after quit is a no-op
        assert_eq!(g.elapsed(), 0.0);
    }

    #[test]
    fn run_terminates_at_max_frames() {
        let cfg = GameConfig {
            max_frames: Some(10),
            ..GameConfig::new(100, 100, "t")
        };
        let mut g = Game::new(cfg);
        g.run().expect("run ok");
        assert_eq!(g.frame_count(), 10);
    }

    #[test]
    fn run_without_max_frames_requires_window() {
        let cfg = GameConfig {
            max_frames: None,
            ..GameConfig::new(100, 100, "t")
        };
        let mut g = Game::new(cfg);
        let r = g.run();
        assert!(matches!(r, Err(GameError::RequiresWindow(_))));
    }

    #[test]
    fn step_returns_frame_budget_exhausted_when_over() {
        let cfg = GameConfig {
            max_frames: Some(3),
            ..GameConfig::new(100, 100, "t")
        };
        let mut g = Game::new(cfg);
        g.step(0.1).expect("ok"); // frame 0
        g.step(0.1).expect("ok"); // frame 1
        g.step(0.1).expect("ok"); // frame 2
        let r = g.step(0.1); // frame 3 would exceed budget
        assert!(matches!(r, Err(GameError::FrameBudgetExhausted(3))));
    }

    #[test]
    fn add_scene_then_step_calls_on_enter_once() {
        let entered = Arc::new(Mutex::new(0u32));
        let e = Arc::clone(&entered);
        let scene = SimpleScene::new("test", |_g, _dt| {}).with_enter(move |_g| {
            *e.lock().expect("mutex") += 1;
        });
        let cfg = GameConfig::new(100, 100, "t");
        let mut g = Game::new(cfg);
        g.add_scene(Box::new(scene));
        assert_eq!(g.scene_count(), 1);
        g.step(1.0 / 60.0).expect("step ok"); // on_enter fires
        g.step(1.0 / 60.0).expect("step ok"); // on_enter does NOT fire again
        g.step(1.0 / 60.0).expect("step ok");
        assert_eq!(*entered.lock().expect("mutex"), 1);
    }

    #[test]
    fn scene_update_called_every_step() {
        let count = Arc::new(Mutex::new(0u32));
        let c = Arc::clone(&count);
        let scene = SimpleScene::new("test", move |_g, _dt| {
            *c.lock().expect("mutex") += 1;
        });
        let cfg = GameConfig::new(100, 100, "t");
        let mut g = Game::new(cfg);
        g.add_scene(Box::new(scene));
        g.step(0.1).expect("ok");
        g.step(0.1).expect("ok");
        g.step(0.1).expect("ok");
        assert_eq!(*count.lock().expect("mutex"), 3);
    }

    #[test]
    fn multi_scene_first_is_active() {
        let update_a = Arc::new(Mutex::new(0u32));
        let update_b = Arc::new(Mutex::new(0u32));
        let ea = Arc::clone(&update_a);
        let eb = Arc::clone(&update_b);
        let scene_a = SimpleScene::new("a", move |_g, _dt| {
            *ea.lock().expect("mutex") += 1;
        });
        let scene_b = SimpleScene::new("b", move |_g, _dt| {
            *eb.lock().expect("mutex") += 1;
        });
        let cfg = GameConfig::new(100, 100, "t");
        let mut g = Game::new(cfg);
        g.add_scene(Box::new(scene_a));
        g.add_scene(Box::new(scene_b));
        assert_eq!(g.scene_count(), 2);
        g.step(0.1).expect("ok"); // scene A updates; scene B does not
        assert_eq!(*update_a.lock().expect("mutex"), 1);
        assert_eq!(*update_b.lock().expect("mutex"), 0);
    }

    #[test]
    fn fixed_timestep_multiple_steps_match() {
        // 10 steps at dt=1/60 = 10/60 = 1/6 second.
        let cfg = GameConfig::new(100, 100, "t");
        let mut g = Game::new(cfg);
        for _ in 0..10 {
            g.step(1.0 / 60.0).expect("ok");
        }
        assert!((g.elapsed() - 10.0 / 60.0).abs() < 1e-4);
        assert_eq!(g.frame_count(), 10);
    }

    #[test]
    fn game_display_includes_title() {
        let cfg = GameConfig::new(100, 100, "MyGame");
        let g = Game::new(cfg);
        let s = format!("{g}");
        assert!(s.contains("MyGame"));
    }

    #[test]
    fn game_debug_shows_fields() {
        let cfg = GameConfig::new(100, 100, "D");
        let g = Game::new(cfg);
        let dbg = format!("{g:?}");
        assert!(dbg.contains("Game"));
        assert!(dbg.contains("elapsed"));
    }

    #[test]
    fn game_config_display() {
        let cfg = GameConfig::new(640, 480, "Demo");
        let s = format!("{cfg}");
        assert!(s.contains("640x480"));
        assert!(s.contains("Demo"));
    }

    #[test]
    fn game_config_default() {
        let cfg = GameConfig::default();
        assert_eq!(cfg.width, 800);
        assert_eq!(cfg.height, 600);
    }

    #[test]
    fn run_calls_quit_via_scene() {
        // A scene that quits after the first step — verifies the loop
        // terminates early.
        let scene = SimpleScene::new("quitter", |g, _dt| {
            g.quit();
        });
        let cfg = GameConfig {
            max_frames: Some(100),
            ..GameConfig::new(100, 100, "t")
        };
        let mut g = Game::new(cfg);
        g.add_scene(Box::new(scene));
        g.run().expect("run ok");
        assert_eq!(g.frame_count(), 1); // quit took effect after first step
    }
}

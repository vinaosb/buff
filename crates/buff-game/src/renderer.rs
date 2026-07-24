//! Abstract renderer: command queue for sprites + text.
//!
//! The `Renderer` does NOT touch the GPU — it is a pure data
//! structure that accumulates [`DrawCommand`]s during a frame.
//! A real window backend (deferred) would drain the queue and issue
//! `wgpu` draw calls at "present" time. The headless MVP keeps the
//! queue intact so tests can inspect the exact sequence of draw calls
//! emitted by the scene's `on_update`.
//!
//! Call [`Renderer::clear`] at the start of each frame to drop the
//! previous frame's commands. The game loop does this automatically.

use crate::asset::Texture;
use crate::transform::Transform;
use std::fmt;

/// A single draw call emitted by the game logic.
///
/// The renderer collects these during `on_update` and a present-
/// backend (deferred) would drain them in order. The enum variants
/// cover the two T16 spec surfaces: `draw_sprite` + `draw_text`.
#[derive(Debug, Clone)]
pub enum DrawCommand {
    /// Draw a textured sprite at the given [`Transform`]. The texture
    /// is an index into the asset cache (not a path — the cache was
    /// resolved at emit time so the present backend does not re-resolve).
    Sprite {
        /// Path key into the asset cache (for present-backend lookup).
        texture_path: String,
        /// The sprite's transform (position + rotation + scale).
        transform: Transform,
    },
    /// Draw a text string at a screen position. Font + size + colour
    /// are deferred to the present backend (v1.18+); the headless MVP
    /// records only content + position for test assertions.
    Text {
        /// The string to render (UTF-8).
        content: String,
        /// Screen-space position `(x, y)`.
        position: (f32, f32),
    },
}

impl fmt::Display for DrawCommand {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DrawCommand::Sprite {
                texture_path,
                transform,
            } => {
                write!(f, "DrawSprite({}, {})", texture_path, transform,)
            }
            DrawCommand::Text { content, position } => {
                write!(
                    f,
                    "DrawText(\"{}\", ({:.1},{:.1}))",
                    content, position.0, position.1,
                )
            }
        }
    }
}

/// Headless renderer: accumulates [`DrawCommand`]s during a frame.
///
/// Created by [`Game::new`](crate::Game::new) and stored inside the
/// game loop. Scenes call [`Renderer::draw_sprite`] and
/// [`Renderer::draw_text`] during `on_update`; a present-backend
/// (deferred) would drain [`Renderer::commands`] at display time.
///
/// The renderer intentionally holds no GPU state, no texture cache,
/// and no window handle. It is a pure data structure that is
/// trivially testable without a graphics device.
#[derive(Debug, Clone, Default)]
pub struct Renderer {
    commands: Vec<DrawCommand>,
}

impl Renderer {
    /// Construct an empty renderer (no pending commands).
    pub fn new() -> Self {
        Self::default()
    }

    /// Record a sprite draw call. The `texture_path` is a lookup key
    /// into the [`Asset`](crate::Asset) cache; the present backend
    /// resolves it to a real GPU texture at drain time.
    pub fn draw_sprite(&mut self, texture_path: &str, transform: Transform) {
        self.commands.push(DrawCommand::Sprite {
            texture_path: texture_path.to_string(),
            transform,
        });
    }

    /// Record a text draw call. The `content` is the UTF-8 string to
    /// render. Font + size + colour are deferred to the present backend
    /// (v1.18+).
    pub fn draw_text(&mut self, content: &str, position: (f32, f32)) {
        self.commands.push(DrawCommand::Text {
            content: content.to_string(),
            position,
        });
    }

    /// Borrow the accumulated draw commands for the current frame.
    /// A present-backend would iterate these; headless tests assert
    /// on them.
    pub fn commands(&self) -> &[DrawCommand] {
        &self.commands
    }

    /// Number of draw commands accumulated this frame. Test helper.
    pub(crate) fn command_count(&self) -> usize {
        self.commands.len()
    }

    /// Drop every pending command. Called by [`Game::step`](crate::Game::step)
    /// at the start of each frame to ensure the command list only
    /// contains draw calls from the current frame.
    pub(crate) fn clear(&mut self) {
        self.commands.clear();
    }
}

impl fmt::Display for Renderer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Renderer({} draw commands)", self.commands.len(),)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transform::Transform;

    #[test]
    fn new_renderer_is_empty() {
        let r = Renderer::new();
        assert!(r.commands().is_empty());
        assert_eq!(r.command_count(), 0);
    }

    #[test]
    fn draw_sprite_records_command() {
        let mut r = Renderer::new();
        let t = Transform::new().translate(10.0, 20.0);
        r.draw_sprite("hero.png", t);
        assert_eq!(r.command_count(), 1);
        assert!(
            matches!(&r.commands()[0], DrawCommand::Sprite { texture_path, .. } if texture_path == "hero.png")
        );
    }

    #[test]
    fn draw_text_records_command() {
        let mut r = Renderer::new();
        r.draw_text("Hello", (100.0, 50.0));
        assert_eq!(r.command_count(), 1);
        assert!(
            matches!(&r.commands()[0], DrawCommand::Text { content, position } if content == "Hello" && *position == (100.0, 50.0))
        );
    }

    #[test]
    fn clear_removes_all_commands() {
        let mut r = Renderer::new();
        r.draw_sprite("a.png", Transform::new());
        r.draw_text("b", (0.0, 0.0));
        assert_eq!(r.command_count(), 2);
        r.clear();
        assert!(r.commands().is_empty());
    }

    #[test]
    fn commands_are_issued_in_order() {
        let mut r = Renderer::new();
        r.draw_sprite("1.png", Transform::new());
        r.draw_text("2", (0.0, 0.0));
        r.draw_sprite("3.png", Transform::new());
        let cmds = r.commands();
        assert!(matches!(cmds[0], DrawCommand::Sprite { .. }));
        assert!(matches!(cmds[1], DrawCommand::Text { .. }));
        assert!(matches!(cmds[2], DrawCommand::Sprite { .. }));
    }

    #[test]
    fn display_format_shows_command_count() {
        let mut r = Renderer::new();
        r.draw_sprite("x.png", Transform::new());
        assert!(format!("{r}").contains("1"));
    }

    #[test]
    fn draw_sprite_transform_position_preserved() {
        let mut r = Renderer::new();
        let t = Transform::new().translate(100.0, 200.0).rotate(1.5);
        r.draw_sprite("hero.png", t);
        match &r.commands()[0] {
            DrawCommand::Sprite { transform, .. } => {
                assert_eq!(transform.position, (100.0, 200.0));
                assert!((transform.rotation - 1.5).abs() < 1e-6);
            }
            _ => panic!("expected DrawCommand::Sprite"),
        }
    }
}

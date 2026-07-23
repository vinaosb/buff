//! Insta snapshot tests for `buff-game` (T16).
//!
//! Each snapshot freezes the Debug/Display format of a key type at
//! a specific lifecycle stage. Run `cargo insta review` to accept
//! new snapshots.

use buff_game::{
    Asset, AudioBuffer, DrawCommand, Game, GameConfig, Input, Key, Renderer, SimpleScene,
    Texture, Transform,
};

// ── Transform snapshots ─────────────────────────────────────────

#[test]
fn snap_transform_identity() {
    let t = Transform::new();
    insta::assert_snapshot!(format!("{t:?}"));
}

#[test]
fn snap_transform_translated_rotated() {
    let t = Transform::new()
        .translate(100.0, 200.0)
        .rotate(std::f32::consts::FRAC_PI_2);
    insta::assert_snapshot!(format!("{t:?}"));
}

// ── Input snapshots ─────────────────────────────────────────────

#[test]
fn snap_input_empty() {
    let i = Input::new();
    insta::assert_snapshot!(format!("{i:?}"));
}

#[test]
fn snap_input_with_keys() {
    let mut i = Input::new();
    i.set_key(Key::KeyW, true);
    i.set_key(Key::Space, true);
    i.set_mouse_position(320.0, 240.0);
    insta::assert_snapshot!(format!("{i:?}"));
}

// ── Renderer snapshots ──────────────────────────────────────────

#[test]
fn snap_renderer_empty() {
    let r = Renderer::new();
    insta::assert_snapshot!(format!("{r:?}"));
}

#[test]
fn snap_renderer_with_commands() {
    let mut r = Renderer::new();
    r.draw_sprite("hero.png", Transform::new().translate(10.0, 20.0));
    r.draw_text("Score: 100", (50.0, 10.0));
    insta::assert_snapshot!(format!("{r:?}"));
}

// ── Game snapshots ──────────────────────────────────────────────

#[test]
fn snap_game_new() {
    let cfg = GameConfig::new(800, 600, "Test");
    let g = Game::new(cfg);
    insta::assert_snapshot!(format!("{g:?}"));
}

#[test]
fn snap_game_after_steps() {
    let cfg = GameConfig::new(800, 600, "Test");
    let mut g = Game::new(cfg);
    g.add_scene(Box::new(SimpleScene::new("s", |_g, _dt| {})));
    for _ in 0..5 {
        g.step(1.0 / 60.0).expect("ok");
    }
    insta::assert_snapshot!(format!("{g:?}"));
}

// ── Asset snapshots ─────────────────────────────────────────────

#[test]
fn snap_texture_display() {
    let t = Texture::from_rgba8(32, 32, vec![255u8; 32 * 32 * 4]).expect("ok");
    insta::assert_snapshot!(format!("{t}"));
}

#[test]
fn snap_audio_buffer_display() {
    let buf = AudioBuffer::from_samples(vec![0.5; 44100], 44_100, 1).expect("ok");
    insta::assert_snapshot!(format!("{buf}"));
}

// ── DrawCommand snapshots ───────────────────────────────────────

#[test]
fn snap_draw_sprite_command() {
    let cmd = DrawCommand::Sprite {
        texture_path: "hero.png".to_string(),
        transform: Transform::new().translate(100.0, 200.0),
    };
    insta::assert_snapshot!(format!("{cmd:?}"));
}

#[test]
fn snap_draw_text_command() {
    let cmd = DrawCommand::Text {
        content: "Hello World".to_string(),
        position: (10.0, 20.0),
    };
    insta::assert_snapshot!(format!("{cmd:?}"));
}

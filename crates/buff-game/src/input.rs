//! Input state: polled keyboard + mouse.
//!
//! Headless-testable: tests call [`Input::set_key`] /
//! [`Input::set_mouse_position`] to inject state, then the game loop
//! reads it via [`Input::is_key_pressed`] / [`Input::mouse_position`].
//! A real window backend (deferred) would feed OS events into the
//! same `set_*` setters on each event-loop tick.

use std::collections::BTreeSet;
use std::fmt;
use std::str::FromStr;

/// Logical key codes (subset sufficient for the T16 MVP — covers
/// arrows, WASD, digits 0-9, plus space/enter/escape). Windowed
/// backends map OS scancodes into this enum; the headless MVP accepts
/// string names via `FromStr` (`"ArrowUp"`, `"KeyW"`, `"Space"`, ...).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Key {
    ArrowUp,
    ArrowDown,
    ArrowLeft,
    ArrowRight,
    KeyW,
    KeyA,
    KeyS,
    KeyD,
    Space,
    Enter,
    Escape,
    Digit0,
    Digit1,
    Digit2,
    Digit3,
    Digit4,
    Digit5,
    Digit6,
    Digit7,
    Digit8,
    Digit9,
}

impl Key {
    /// Stable string identifier (round-trips with [`Key::from_str`]).
    /// Used by snapshots + the future codegen lowering (the Buff
    /// surface is `Input.is_key_pressed("ArrowUp")` which parses via
    /// `FromStr` at the FFI boundary).
    pub fn as_str(&self) -> &'static str {
        match self {
            Key::ArrowUp => "ArrowUp",
            Key::ArrowDown => "ArrowDown",
            Key::ArrowLeft => "ArrowLeft",
            Key::ArrowRight => "ArrowRight",
            Key::KeyW => "KeyW",
            Key::KeyA => "KeyA",
            Key::KeyS => "KeyS",
            Key::KeyD => "KeyD",
            Key::Space => "Space",
            Key::Enter => "Enter",
            Key::Escape => "Escape",
            Key::Digit0 => "Digit0",
            Key::Digit1 => "Digit1",
            Key::Digit2 => "Digit2",
            Key::Digit3 => "Digit3",
            Key::Digit4 => "Digit4",
            Key::Digit5 => "Digit5",
            Key::Digit6 => "Digit6",
            Key::Digit7 => "Digit7",
            Key::Digit8 => "Digit8",
            Key::Digit9 => "Digit9",
        }
    }
}

impl fmt::Display for Key {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Parse error returned by [`Key::from_str`] for unknown key names.
/// Carries the offending string so callers can include it in a
/// diagnostic.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnknownKey(pub String);

impl fmt::Display for UnknownKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "unknown key name: {}", self.0)
    }
}

impl std::error::Error for UnknownKey {}

impl FromStr for Key {
    type Err = UnknownKey;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "ArrowUp" => Ok(Key::ArrowUp),
            "ArrowDown" => Ok(Key::ArrowDown),
            "ArrowLeft" => Ok(Key::ArrowLeft),
            "ArrowRight" => Ok(Key::ArrowRight),
            "KeyW" => Ok(Key::KeyW),
            "KeyA" => Ok(Key::KeyA),
            "KeyS" => Ok(Key::KeyS),
            "KeyD" => Ok(Key::KeyD),
            "Space" => Ok(Key::Space),
            "Enter" => Ok(Key::Enter),
            "Escape" => Ok(Key::Escape),
            "Digit0" => Ok(Key::Digit0),
            "Digit1" => Ok(Key::Digit1),
            "Digit2" => Ok(Key::Digit2),
            "Digit3" => Ok(Key::Digit3),
            "Digit4" => Ok(Key::Digit4),
            "Digit5" => Ok(Key::Digit5),
            "Digit6" => Ok(Key::Digit6),
            "Digit7" => Ok(Key::Digit7),
            "Digit8" => Ok(Key::Digit8),
            "Digit9" => Ok(Key::Digit9),
            other => Err(UnknownKey(other.to_string())),
        }
    }
}

/// Polled input state for one game frame.
///
/// Stores the set of currently-down keys + the latest mouse position.
/// A real window backend would translate OS events into `set_*` calls
/// on each event-loop tick; the headless MVP lets tests inject state
/// directly.
///
/// Call [`Input::begin_frame`] at the start of each `Game::step` to
/// advance the per-frame edge-detected state. (The MVP tracks only
/// "is down right now"; "just pressed this frame" / "just released"
/// edge detection is a documented v1.18+ enhancement.)
#[derive(Debug, Clone)]
pub struct Input {
    down: BTreeSet<Key>,
    mouse: (f32, f32),
}

impl Input {
    /// Construct an empty input state (no keys down, mouse at origin).
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns `true` iff `key` is currently held down (set via the
    /// most recent [`Input::set_key`] call with `pressed = true`).
    pub fn is_key_pressed(&self, key: Key) -> bool {
        self.down.contains(&key)
    }

    /// Returns the latest mouse position as `(x, y)` in screen-space
    /// pixels. Defaults to `(0.0, 0.0)` until
    /// [`Input::set_mouse_position`] is called.
    pub fn mouse_position(&self) -> (f32, f32) {
        self.mouse
    }

    /// Inject a key state (headless test API + future window
    /// event-adapter API). `pressed = true` adds the key to the
    /// down-set; `pressed = false` removes it.
    pub fn set_key(&mut self, key: Key, pressed: bool) {
        if pressed {
            self.down.insert(key);
        } else {
            self.down.remove(&key);
        }
    }

    /// Inject the mouse position (headless test API + future window
    /// event-adapter API).
    pub fn set_mouse_position(&mut self, x: f32, y: f32) {
        self.mouse = (x, y);
    }

    /// Advance per-frame edge-detected state. Called by
    /// [`Game::step`](crate::Game::step) at the start of each tick.
    ///
    /// In the MVP this is a no-op (we track only "is down now"). The
    /// hook exists so a future v1.18+ enhancement can add
    /// "just-pressed" / "just-released" edge detection without
    /// changing the call sites.
    pub(crate) fn begin_frame(&mut self) {
        // MVP: no per-frame distinction. Future: snapshot `down` into
        // `down_prev` and derive `just_pressed = down - down_prev`.
    }

    /// Number of keys currently held down. Test helper.
    /// `pub(crate)` so it does not count toward the public API surface.
    pub(crate) fn down_count(&self) -> usize {
        self.down.len()
    }

    /// Returns `true` iff no keys are currently down. Test helper.
    /// `pub(crate)` so it does not count toward the public API surface.
    pub(crate) fn is_empty(&self) -> bool {
        self.down.is_empty()
    }
}

impl Default for Input {
    fn default() -> Self {
        Self {
            down: BTreeSet::new(),
            mouse: (0.0, 0.0),
        }
    }
}

impl fmt::Display for Input {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "Input(down={} keys, mouse=({:.1},{:.1}))",
            self.down.len(),
            self.mouse.0,
            self.mouse.1,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_input_has_no_keys_down() {
        let i = Input::new();
        assert!(i.is_empty());
        assert!(!i.is_key_pressed(Key::Space));
        assert_eq!(i.mouse_position(), (0.0, 0.0));
    }

    #[test]
    fn set_key_pressed_then_released() {
        let mut i = Input::new();
        i.set_key(Key::ArrowUp, true);
        assert!(i.is_key_pressed(Key::ArrowUp));
        assert_eq!(i.down_count(), 1);
        i.set_key(Key::ArrowUp, false);
        assert!(!i.is_key_pressed(Key::ArrowUp));
        assert!(i.is_empty());
    }

    #[test]
    fn set_key_distinct_keys_independent() {
        let mut i = Input::new();
        i.set_key(Key::KeyW, true);
        i.set_key(Key::KeyD, true);
        assert!(i.is_key_pressed(Key::KeyW));
        assert!(i.is_key_pressed(Key::KeyD));
        assert!(!i.is_key_pressed(Key::KeyA));
        assert_eq!(i.down_count(), 2);
    }

    #[test]
    fn mouse_position_tracking() {
        let mut i = Input::new();
        i.set_mouse_position(120.0, 240.0);
        assert_eq!(i.mouse_position(), (120.0, 240.0));
        i.set_mouse_position(0.0, 0.0);
        assert_eq!(i.mouse_position(), (0.0, 0.0));
    }

    #[test]
    fn begin_frame_is_safe_noop() {
        let mut i = Input::new();
        i.set_key(Key::Space, true);
        i.begin_frame(); // does not clear down-state
        assert!(i.is_key_pressed(Key::Space));
    }

    #[test]
    fn key_from_str_roundtrip() {
        for k in [
            Key::ArrowUp,
            Key::ArrowDown,
            Key::ArrowLeft,
            Key::ArrowRight,
            Key::KeyW,
            Key::KeyA,
            Key::KeyS,
            Key::KeyD,
            Key::Space,
            Key::Enter,
            Key::Escape,
            Key::Digit0,
            Key::Digit9,
        ] {
            let s = k.as_str();
            let parsed: Result<Key, _> = s.parse();
            assert_eq!(parsed, Ok(k), "roundtrip failed for {s}");
        }
    }

    #[test]
    fn key_from_str_unknown_errors() {
        let r: Result<Key, _> = "NotAKey".parse();
        assert!(r.is_err());
        assert_eq!(r.unwrap_err(), UnknownKey("NotAKey".to_string()));
    }

    #[test]
    fn key_display_matches_as_str() {
        assert_eq!(format!("{}", Key::Space), "Space");
        assert_eq!(format!("{}", Key::ArrowLeft), "ArrowLeft");
    }
}

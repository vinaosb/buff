//! Error type for `buff-game`.
//!
//! Mirrors the workspace pattern: one `thiserror::Error` enum, aliased
//! as `GameResult<T>`. Variants are intentionally structured (not
//! string-carried) so the future `BuffError` migration is mechanical.

use thiserror::Error;

/// All fallible `buff-game` operations return this error type.
///
/// Variants follow the buff-image / buff-audio precedent: each names
/// the failing subsystem + carries enough context to render a useful
/// diagnostic. `Display` is derived by `thiserror` with `#[error]`
/// templates so the messages are stable across versions.
#[derive(Debug, Clone, Error)]
pub enum GameError {
    /// Asset loading failed (texture decode, audio decode, missing file).
    /// Carries the path attempted + the underlying cause as a string.
    #[error("asset load failed for {path}: {reason}")]
    AssetLoad {
        /// The filesystem path the asset was loaded from.
        path: String,
        /// Human-readable cause (image / audio codec error stringified).
        reason: String,
    },

    /// Asset cache lookup missed (the path was never loaded). Returned
    /// by [`Asset::cache_get`](crate::Asset::cache_get) when the caller
    /// asks for a path that has not been loaded yet.
    #[error("asset cache miss for {0}")]
    CacheMiss(String),

    /// The game was stepped past its configured `max_frames` bound
    /// (headless runaway-loop guard). Returned by [`Game::step`](crate::Game::step)
    /// when `config.max_frames` is `Some(n)` and `step` is called for
    /// the `n+1`-th time. `Game::run` handles this internally by
    /// terminating the loop.
    #[error("frame budget exhausted (max_frames={0})")]
    FrameBudgetExhausted(usize),

    /// A user-supplied closure (scene `on_update`, asset transform)
    /// returned an error. Carries the user message verbatim.
    #[error("user callback failed: {0}")]
    UserCallback(String),

    /// Headless mode limitation: the operation requires a real window
    /// context (which the MVP does not provide). Returned by
    /// [`Game::run`](crate::Game::run) when `config.max_frames` is
    /// `None` (would loop forever with no window-close event to stop it).
    #[error("operation requires window context (headless MVP): {0}")]
    RequiresWindow(String),
}

/// Convenience alias used by every fallible `buff-game` function.
pub type GameResult<T> = Result<T, GameError>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn asset_load_error_formats_with_path() {
        let e = GameError::AssetLoad {
            path: "missing.png".to_string(),
            reason: "file not found".to_string(),
        };
        let s = format!("{e}");
        assert!(s.contains("missing.png"));
        assert!(s.contains("file not found"));
    }

    #[test]
    fn cache_miss_display() {
        let e = GameError::CacheMiss("foo.png".to_string());
        assert!(format!("{e}").contains("foo.png"));
    }

    #[test]
    fn frame_budget_carries_count() {
        let e = GameError::FrameBudgetExhausted(60);
        assert!(format!("{e}").contains("60"));
    }

    #[test]
    fn requires_window_is_distinct_variant() {
        let e = GameError::RequiresWindow("run".to_string());
        assert!(matches!(e, GameError::RequiresWindow(_)));
    }
}

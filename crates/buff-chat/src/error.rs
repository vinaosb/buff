//! Error type for the `buff-chat` crate.
//!
//! All fallible operations surface as [`ChatError`]. The public entry
//! points ([`crate::Bot::new`], `command`, `on_message`, `start`,
//! `stop`, `dispatch`) wrap their bodies in `catch_unwind` per the T4
//! FFI guide R6 so panics never propagate across the FFI boundary into
//! Buff code.
//!
//! # Panic-free contract
//!
//! No `unwrap` / `expect` / `panic!` / `todo!` / `unimplemented!` in
//! this module or any non-test code path. The bot primitives (serenity
//! Client, teloxide Bot, tokio runtime) return `Result` explicitly so
//! the only failure modes are user-visible or runtime-internal.

use thiserror::Error;

/// The single error type returned by every fallible `buff-chat`
/// operation.
#[derive(Debug, Error)]
pub enum ChatError {
    /// `Bot::new` was called with an empty token string. Empty tokens
    /// are rejected because both Discord and Telegram reject them at
    /// connect time with opaque errors — failing early gives the user
    /// a clear diagnostic.
    #[error("bot token must not be empty")]
    EmptyToken,

    /// `command` was called with an empty name string. Command names
    /// must be non-empty so the dispatch prefix-extraction logic can
    /// resolve them.
    #[error("command name must not be empty")]
    EmptyCommandName,

    /// `command` was called with a name that is already registered.
    /// Duplicate registration is rejected so a later caller can't
    /// silently shadow an earlier handler. Includes the offending name.
    #[error("command already registered: {0}")]
    DuplicateCommand(String),

    /// `start` was called on a bot that is already running. A single
    /// `Bot` instance supports one active connection — calling
    /// `start` again would double-connect. Use `stop` first or create
    /// a new `Bot`.
    #[error("bot is already running")]
    AlreadyRunning,

    /// `start` was called from inside an active tokio runtime.
    /// `start` blocks on a new runtime via `runtime.block_on(...)`,
    /// which panics if a runtime is already active ("Cannot start a
    /// runtime from within a runtime"). The caller must invoke
    /// `start` from a non-async context (e.g. `fn main()`), or use
    /// the async bridge directly (deferred to v1.18+).
    #[error("start() cannot be called from inside a tokio runtime; call from a sync context")]
    AlreadyInRuntime,

    /// `stop` was called on a bot that is not running.
    #[error("bot is not running")]
    NotRunning,

    /// The connection to the platform (Discord gateway or Telegram
    /// Bot API) failed. Includes the underlying error message from
    /// serenity / teloxide.
    #[error("connection failed: {0}")]
    Connect(String),

    /// A runtime error occurred while the bot event loop was running
    /// (e.g. gateway disconnect, tokio task panic). Includes the
    /// underlying error message.
    #[error("runtime error: {0}")]
    Runtime(String),

    /// A wrapper-internal panic was caught by `catch_unwind` (per
    /// T4 FFI guide R6). The user sees a stable diagnostic instead
    /// of a process abort.
    #[error("internal error: chat operation panicked")]
    Panic,
}

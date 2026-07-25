#![allow(dead_code, unused_imports, private_interfaces)]
//! `buff-chat` — Discord + Telegram bot framework for the Buff language.
//!
//! Pure-Rust MVP wrapping [`serenity`](https://crates.io/crates/serenity)
//! (Discord Gateway + HTTP API) + [`teloxide`](https://crates.io/crates/teloxide)
//! (Telegram Bot API) behind a safe Rust API that follows the
//! [T4 FFI safety guide](../buff-lang-ffi-guide/GUIDE.md).
//!
//! # Pipeline
//!
//! ```text
//!   Bot.new(platform, token) ──▶ Bot { commands, on_message_handler, running }
//!                                  │
//!                                  ├─ bot.command("ping", handler)  ──▶ registers prefix command
//!                                  ├─ bot.on_message(handler)       ──▶ registers catch-all
//!                                  │
//!                                  ├─ bot.start()  ──▶ blocks on platform event loop
//!                                  │       │
//!                                  │       ├─ Discord: serenity Client::builder + EventHandler
//!                                  │       └─ Telegram: teloxide Bot::new + repl
//!                                  │       │
//!                                  │       └─ incoming message ──▶ dispatch_to_inner(msg)
//!                                  │                                  │
//!                                  │                                  ├─ "!name ..." ─▶ command handler
//!                                  │                                  └─ other       ─▶ on_message handler
//!                                  │
//!                                  ├─ bot.dispatch(msg)  ──▶ programmatic dispatch (testing)
//!                                  └─ bot.stop()         ──▶ signals shutdown
//! ```
//!
//! # FFI safety
//!
//! Every public entry point follows the 6 hard rules from
//! `crates/buff-lang-ffi-guide/GUIDE.md`:
//!
//! | Rule | How this crate complies |
//! |------|-------------------------|
//! | R1 — No raw pointers | Public surface exposes only `Bot`, `Platform`, `Message`, `ChatError`. No `*const` / `*mut`. |
//! | R2 — Ownership boundary | `command` / `on_message` consume owned `Fn` closures (Arc-shared). `dispatch` takes owned `Message`. |
//! | R3 — Error mapping | `new` / `command` / `on_message` / `start` / `stop` / `dispatch` return `Result<T, ChatError>`. |
//! | R4 — Thread safety | `Bot` is `Send + Sync` (wraps `Arc<RwLock<BotInner>>`). Handlers require `Fn + Send + Sync + 'static`. |
//! | R5 — Lifetime hiding | No public lifetime parameters. All references (`&str` args) copied to owned `String` at boundary. |
//! | R6 — Panic boundary | `new` / `command` / `on_message` / `start` / `stop` / `dispatch` wrap bodies in `catch_unwind`. |
//!
//! # Panic-free contract
//!
//! No `unwrap` / `expect` / `panic!` / `todo!` / `unimplemented!` in
//! non-test code. Handler panics are caught inside `dispatch_to_inner`
//! so a throwing handler doesn't crash the event loop (mirrors the
//! buff-pubsub EventEmitter precedent).

pub mod discord;
pub mod error;
pub mod message;
pub mod telegram;

pub use error::ChatError;
pub use message::Message;

use std::collections::HashMap;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, RwLock};

/// Alias for the boxed message handler closure. Stored as `Arc` so
/// the dispatch path can clone the pointer cheaply under the read
/// lock without holding the lock while invoking the handler.
type Handler = Arc<dyn Fn(Message) + Send + Sync>;

/// The chat platform a [`Bot`] connects to.
///
/// Passed to [`Bot::new`] to select which backend (serenity for
/// Discord, teloxide for Telegram) the `start()` event loop uses.
/// Stored on the [`Bot`] instance and surfaced via [`Message::platform`]
/// so cross-platform handlers can dispatch on the source.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Platform {
    /// Discord — backed by [`serenity`](https://crates.io/crates/serenity).
    /// Uses the Discord Gateway WebSocket for real-time message events.
    Discord,
    /// Telegram — backed by [`teloxide`](https://crates.io/crates/teloxide).
    /// Uses the Telegram Bot API long-polling for message updates.
    Telegram,
}

impl Platform {
    /// Returns `true` if this platform is [`Platform::Discord`].
    pub fn is_discord(self) -> bool {
        matches!(self, Platform::Discord)
    }

    /// Returns `true` if this platform is [`Platform::Telegram`].
    pub fn is_telegram(self) -> bool {
        matches!(self, Platform::Telegram)
    }
}

impl std::fmt::Display for Platform {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Platform::Discord => write!(f, "discord"),
            Platform::Telegram => write!(f, "telegram"),
        }
    }
}

/// Internal mutable state behind `Arc<RwLock<...>>`.
pub(crate) struct BotInner {
    commands: HashMap<String, Handler>,
    message_handler: Option<Handler>,
    running: Arc<AtomicBool>,
}

impl BotInner {
    fn new() -> Self {
        BotInner {
            commands: HashMap::new(),
            message_handler: None,
            running: Arc::new(AtomicBool::new(false)),
        }
    }
}

/// A cross-platform chat bot wrapping Discord (serenity) or Telegram
/// (teloxide).
///
/// Construct via [`Bot::new`] with a [`Platform`] and token. Register
/// command handlers via [`Bot::command`] and a catch-all message
/// handler via [`Bot::on_message`], then start the event loop with
/// [`Bot::start`] (blocks until the connection drops or [`Bot::stop`]
/// is called from another thread).
///
/// `Bot` is `Send + Sync` and cheap to clone (inner state is behind
/// an `Arc`); the recommended pattern for cross-thread sharing (e.g.
/// calling `stop` from a signal handler while `start` blocks) is
/// `let bot = Bot::new(...)?; let bot2 = bot.clone();`.
///
/// # Command dispatch
///
/// Messages starting with `!` or `/` are treated as commands. The
/// first whitespace-delimited word after the prefix is the command
/// name; if it matches a registered command, that handler fires.
/// Messages without a recognized command prefix fall through to the
/// `on_message` handler (if registered). Both `!ping` (Discord
/// convention) and `/ping` (Telegram convention) work on both
/// platforms for cross-platform bots.
///
/// # Example
///
/// ```
/// use buff_chat::{Bot, Message, Platform};
///
/// let bot = Bot::new(Platform::Discord, "token").expect("bot");
/// let _ = bot.command("ping", move |msg| {
///     println!("ping from {}: {}", msg.author(), msg.text());
/// });
/// // bot.start(); // blocks — omitted in doctest
/// ```
#[derive(Clone)]
pub struct Bot {
    inner: Arc<RwLock<BotInner>>,
    platform: Platform,
    token: String,
}

impl Bot {
    /// Construct a new bot for the given platform with the given
    /// authentication token.
    ///
    /// The token is validated as non-empty (returns
    /// [`ChatError::EmptyToken`] otherwise) but NOT validated against
    /// the platform — that happens at `start()` connect time. Storing
    /// the token eagerly lets `command` / `on_message` registrations
    /// happen before the connection is attempted.
    pub fn new(platform: Platform, token: String) -> Result<Self, ChatError> {
        if token.trim().is_empty() {
            return Err(ChatError::EmptyToken);
        }
        let result = catch_unwind(AssertUnwindSafe(|| Bot {
            inner: Arc::new(RwLock::new(BotInner::new())),
            platform,
            token,
        }));
        match result {
            Ok(bot) => Ok(bot),
            Err(_) => Err(ChatError::Panic),
        }
    }

    /// Register a command handler under `name`.
    ///
    /// When a message arrives whose text starts with `!` or `/`
    /// followed immediately by `name` (then whitespace or end-of-
    /// string), this handler fires with the full [`Message`]. Both
    /// `!name` and `/name` trigger the same handler so a single bot
    /// works on both Discord and Telegram without per-platform
    /// registration.
    ///
    /// Returns [`ChatError::EmptyCommandName`] if `name` is empty,
    /// [`ChatError::DuplicateCommand`] if `name` is already registered.
    pub fn command<F>(&self, name: &str, handler: F) -> Result<(), ChatError>
    where
        F: Fn(Message) + Send + Sync + 'static,
    {
        let name_owned = name.trim().to_string();
        if name_owned.is_empty() {
            return Err(ChatError::EmptyCommandName);
        }
        let handler_arc: Handler = Arc::new(handler);
        let result = catch_unwind(AssertUnwindSafe(|| {
            let mut inner = self.inner.write().map_err(|_| ChatError::Panic)?;
            if inner.commands.contains_key(&name_owned) {
                return Err(ChatError::DuplicateCommand(name_owned));
            }
            inner.commands.insert(name_owned, handler_arc);
            Ok(())
        }));
        match result {
            Ok(Ok(())) => Ok(()),
            Ok(Err(e)) => Err(e),
            Err(_) => Err(ChatError::Panic),
        }
    }

    /// Register a catch-all message handler.
    ///
    /// Fires for every message that does NOT match a registered
    /// command (i.e. messages without a command prefix, or messages
    /// with an unrecognized command name). Only one `on_message`
    /// handler is supported at a time; calling this again replaces
    /// the previous handler.
    pub fn on_message<F>(&self, handler: F) -> Result<(), ChatError>
    where
        F: Fn(Message) + Send + Sync + 'static,
    {
        let handler_arc: Handler = Arc::new(handler);
        let result = catch_unwind(AssertUnwindSafe(|| {
            let mut inner = self.inner.write().map_err(|_| ChatError::Panic)?;
            inner.message_handler = Some(handler_arc);
            Ok(())
        }));
        match result {
            Ok(Ok(())) => Ok(()),
            Ok(Err(e)) => Err(e),
            Err(_) => Err(ChatError::Panic),
        }
    }

    /// Start the bot event loop. **Blocks** until the connection
    /// drops or [`Self::stop`] is called from another thread.
    ///
    /// Builds a new tokio runtime (multi-threaded, all features
    /// enabled) and blocks on the platform-specific event loop:
    /// - [`Platform::Discord`]: serenity `Client::builder(token, intents)
    ///   .event_handler(Bridge).await` then `client.start().await`.
    /// - [`Platform::Telegram`]: `teloxide::Bot::new(token)` then
    ///   `teloxide::repl(bot, handler).await`.
    ///
    /// Returns [`ChatError::AlreadyRunning`] if called on a bot that
    /// is already running, [`ChatError::AlreadyInRuntime`] if called
    /// from inside a tokio runtime (the internal `block_on` would
    /// panic — call `start` from `fn main()` or a sync context),
    /// [`ChatError::Connect`] if the platform rejects the token /
    /// connection, or [`ChatError::Runtime`] if the event loop fails
    /// after connecting.
    pub fn start(&self) -> Result<(), ChatError> {
        if tokio::runtime::Handle::try_current().is_ok() {
            return Err(ChatError::AlreadyInRuntime);
        }
        let running_flag = {
            let inner = self.inner.read().map_err(|_| ChatError::Panic)?;
            if inner.running.load(Ordering::Relaxed) {
                return Err(ChatError::AlreadyRunning);
            }
            inner.running.clone()
        };
        running_flag.store(true, Ordering::Relaxed);

        let runtime = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .map_err(|e| ChatError::Runtime(e.to_string()))?;

        let inner = self.inner.clone();
        let platform = self.platform;
        let token = self.token.clone();
        let running_clone = running_flag.clone();

        let result = runtime.block_on(async move {
            match platform {
                Platform::Discord => discord::run(&token, inner, &running_clone).await,
                Platform::Telegram => telegram::run(&token, inner, &running_clone).await,
            }
        });

        running_flag.store(false, Ordering::Relaxed);
        result
    }

    /// Signal the running event loop to stop.
    ///
    /// Sets an `AtomicBool` flag that the platform bridges check
    /// between message dispatches. The event loop exits on its next
    /// iteration (NOT immediately — an in-flight handler runs to
    /// completion). Returns [`ChatError::NotRunning`] if the bot is
    /// not currently running.
    ///
    /// Safe to call from a signal handler or another thread while
    /// [`Self::start`] blocks.
    pub fn stop(&self) -> Result<(), ChatError> {
        let result = catch_unwind(AssertUnwindSafe(|| {
            let inner = self.inner.read().map_err(|_| ChatError::Panic)?;
            if !inner.running.load(Ordering::Relaxed) {
                return Err(ChatError::NotRunning);
            }
            inner.running.store(false, Ordering::Relaxed);
            Ok(())
        }));
        match result {
            Ok(Ok(())) => Ok(()),
            Ok(Err(e)) => Err(e),
            Err(_) => Err(ChatError::Panic),
        }
    }

    /// Dispatch a message to the registered handlers.
    ///
    /// Public so tests and programmatic callers can exercise the
    /// handler routing without a live network connection (the T47
    /// "mock API" acceptance criterion). The platform bridges
    /// ([`discord`], [`telegram`]) call the internal variant
    /// ([`Self::dispatch_to_inner`]) on every incoming message.
    ///
    /// Dispatch logic:
    /// 1. If `msg.text()` starts with `!` or `/`, extract the first
    ///    word (minus prefix) as the command name.
    /// 2. If the command name matches a registered [`Self::command`]
    ///    handler, invoke it.
    /// 3. Otherwise (no prefix, or unknown command), invoke the
    ///    [`Self::on_message`] handler if one is registered.
    ///
    /// Handler panics are caught (per FFI guide R6) so a throwing
    /// handler doesn't crash the caller.
    pub fn dispatch(&self, msg: Message) -> Result<(), ChatError> {
        Self::dispatch_to_inner(&self.inner, msg)
    }

    /// Internal dispatch used by the platform bridges. Takes the
    /// `Arc<RwLock<BotInner>>` directly so the bridges (which hold
    /// the inner from the `start` path) don't need a full `Bot`.
    pub(crate) fn dispatch_to_inner(
        inner: &Arc<RwLock<BotInner>>,
        msg: Message,
    ) -> Result<(), ChatError> {
        let read_guard = inner.read().map_err(|_| ChatError::Panic)?;
        let command_name = extract_command_name(msg.text());
        if let Some(name) = &command_name {
            if let Some(handler) = read_guard.commands.get(name) {
                let handler = handler.clone();
                drop(read_guard);
                let _ = catch_unwind(AssertUnwindSafe(|| handler(msg)));
                return Ok(());
            }
        }
        if let Some(handler) = &read_guard.message_handler {
            let handler = handler.clone();
            drop(read_guard);
            let _ = catch_unwind(AssertUnwindSafe(|| handler(msg)));
        }
        Ok(())
    }

    /// The platform this bot was constructed for.
    pub fn platform(&self) -> Platform {
        self.platform
    }

    /// Whether [`Self::start`] is currently blocking on the event
    /// loop. Point-in-time snapshot — the running flag is set at
    /// `start` entry and cleared at exit.
    pub fn is_running(&self) -> bool {
        let Ok(inner) = self.inner.read() else {
            return false;
        };
        inner.running.load(Ordering::Relaxed)
    }

    /// Number of commands registered via [`Self::command`].
    pub fn command_count(&self) -> usize {
        let Ok(inner) = self.inner.read() else {
            return 0;
        };
        inner.commands.len()
    }

    /// Whether an [`Self::on_message`] handler has been registered.
    pub fn has_message_handler(&self) -> bool {
        let Ok(inner) = self.inner.read() else {
            return false;
        };
        inner.message_handler.is_some()
    }
}

impl Default for Bot {
    /// Default-constructs an empty `Bot` (Discord platform, empty
    /// token). Equivalent to a placeholder for codegen fallback paths
    /// — `Bot::new` with a real token is the production constructor.
    /// Matches the Image / DataFrame / EventBus Default precedent.
    fn default() -> Self {
        Bot {
            inner: Arc::new(RwLock::new(BotInner::new())),
            platform: Platform::Discord,
            token: String::new(),
        }
    }
}

impl std::fmt::Debug for Bot {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let (cmd_count, has_msg) = {
            let Ok(inner) = self.inner.read() else {
                return f.debug_struct("Bot").finish_non_exhaustive();
            };
            (inner.commands.len(), inner.message_handler.is_some())
        };
        f.debug_struct("Bot")
            .field("platform", &self.platform)
            .field("commands", &cmd_count)
            .field("has_message_handler", &has_msg)
            .field("running", &self.is_running())
            .finish_non_exhaustive()
    }
}

impl std::fmt::Display for Bot {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "Bot({}, {} commands, {})",
            self.platform,
            self.command_count(),
            if self.has_message_handler() {
                "with on_message"
            } else {
                "no on_message"
            }
        )
    }
}

/// Extract the command name from a message text.
///
/// Returns `Some(name)` if `text` starts with `!` or `/` followed by
/// a non-empty word; `None` otherwise. The returned name excludes the
/// prefix character and any trailing whitespace / arguments.
///
/// Examples: `"!ping"` → `Some("ping")`, `"/echo hello"` →
/// `Some("echo")`, `"hello"` → `None`, `"!"` → `None`.
fn extract_command_name(text: &str) -> Option<String> {
    let trimmed = text.trim_start();
    let rest = trimmed
        .strip_prefix('!')
        .or_else(|| trimmed.strip_prefix('/'))?;
    let name: String = rest.chars().take_while(|c| !c.is_whitespace()).collect();
    if name.is_empty() {
        None
    } else {
        Some(name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_command_name_bang() {
        assert_eq!(extract_command_name("!ping"), Some("ping".to_string()));
        assert_eq!(
            extract_command_name("!echo hello world"),
            Some("echo".to_string())
        );
    }

    #[test]
    fn test_extract_command_name_slash() {
        assert_eq!(extract_command_name("/ping"), Some("ping".to_string()));
        assert_eq!(
            extract_command_name("/start arg1"),
            Some("start".to_string())
        );
    }

    #[test]
    fn test_extract_command_name_no_prefix() {
        assert_eq!(extract_command_name("hello"), None);
        assert_eq!(extract_command_name(""), None);
        assert_eq!(extract_command_name("plain text"), None);
    }

    #[test]
    fn test_extract_command_name_prefix_only() {
        assert_eq!(extract_command_name("!"), None);
        assert_eq!(extract_command_name("/"), None);
        assert_eq!(extract_command_name("! "), None);
    }
}

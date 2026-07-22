//! Platform-agnostic message type for `buff-chat`.
//!
//! [`Message`] is the unified representation of an incoming message
//! from either Discord or Telegram. Both platform bridges (serenity
//! for Discord, teloxide for Telegram) convert their native message
//! types into [`Message`] before dispatching to user-registered
//! handlers. This keeps the handler API platform-agnostic: a single
//! closure registered via `bot.command("ping", handler)` works on
//! both platforms without modification.

use crate::Platform;

/// A chat message received from Discord or Telegram.
///
/// Constructed by the platform bridges ([`crate::discord`],
/// [`crate::telegram`]) or directly by tests / programmatic callers
/// via [`Message::new`]. Passed by value to every handler registered
/// via [`crate::Bot::command`] or [`crate::Bot::on_message`].
///
/// All fields are owned (`String`, `Platform`, `bool`) — no lifetimes
/// leak across the FFI boundary (per T4 FFI guide R5).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Message {
    text: String,
    channel: String,
    author: String,
    platform: Platform,
    is_dm: bool,
}

impl Message {
    /// Construct a new message from owned field values.
    ///
    /// Public so tests and programmatic callers (e.g. `bot.dispatch(msg)`
    /// in mock-API tests) can construct messages without going through
    /// the platform bridges.
    pub fn new(
        text: String,
        channel: String,
        author: String,
        platform: Platform,
        is_dm: bool,
    ) -> Self {
        Message {
            text,
            channel,
            author,
            platform,
            is_dm,
        }
    }

    /// The text content of the message.
    ///
    /// For command messages this includes the command prefix (`!` or
    /// `/`) and the command name. For non-text messages (Telegram
    /// stickers, Discord embeds) this returns an empty string.
    pub fn text(&self) -> &str {
        &self.text
    }

    /// The channel or chat identifier as a string.
    ///
    /// For Discord this is the numeric channel ID (e.g. `"123456789"`).
    /// For Telegram this is the numeric chat ID (e.g. `"-1001234567890"`
    /// for groups or `"123456789"` for private chats). Platform-specific
    /// replying uses this identifier.
    pub fn channel(&self) -> &str {
        &self.channel
    }

    /// The display name of the message author.
    ///
    /// For Discord this is the user's display name (`msg.author.name`).
    /// For Telegram this is the user's username (without the `@`).
    /// Returns an empty string when the author is unknown (e.g.
    /// Telegram channel posts which have no `from` field).
    pub fn author(&self) -> &str {
        &self.author
    }

    /// The platform this message originated from.
    ///
    /// Handlers shared across platforms can dispatch on this to apply
    /// platform-specific logic (e.g. different reply formatting).
    pub fn platform(&self) -> Platform {
        self.platform
    }

    /// Whether this message was sent in a private (direct message)
    /// context.
    ///
    /// For Discord this is `msg.guild_id.is_none()` (DMs have no
    /// guild). For Telegram this uses the positive-chat-ID convention
    /// (private chats use the user's positive ID; groups/channels use
    /// negative IDs).
    pub fn is_dm(&self) -> bool {
        self.is_dm
    }
}

impl std::fmt::Display for Message {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "Message({:?}, {:?}, @{}, dm={})",
            self.platform, self.text, self.author, self.is_dm
        )
    }
}

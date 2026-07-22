//! Telegram bridge — adapts teloxide's message dispatch to
//! `buff_chat::Bot::dispatch_to_inner`.
//!
//! This module is only used when [`crate::Platform::Telegram`] is
//! selected. It builds a `teloxide::Bot` and runs the long-polling
//! REPL, converting each incoming `teloxide::types::Message` into a
//! platform-agnostic [`crate::Message`] and dispatching it through
//! the same routing logic that `bot.dispatch(msg)` uses.
//!
//! The bridge is intentionally thin: all handler registration and
//! command routing happens in [`crate::Bot`]. This module only does
//! platform-specific message conversion + connection lifecycle.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, RwLock};

use crate::{Bot, ChatError, Message, Platform};

pub(crate) use crate::discord::SharedInner;

/// Run the Telegram bot event loop. Blocks until the connection drops
/// or the `running` flag is cleared by [`Bot::stop`].
///
/// Constructs a `teloxide::Bot` and runs `teloxide::repl` which
/// long-polls the Telegram Bot API for message updates, invoking the
/// closure for each incoming message.
pub async fn run(
    token: &str,
    inner: SharedInner,
    running: &Arc<AtomicBool>,
) -> Result<(), ChatError> {
    let bot = teloxide::Bot::new(token.to_string());

    let inner_clone = inner;
    let running_clone = running.clone();

    teloxide::repl(bot, move |_bot, msg| {
        let inner = inner_clone.clone();
        let running = running_clone.clone();
        async move {
            if !running.load(Ordering::Relaxed) {
                return Ok::<(), teloxide::RequestError>(());
            }
            let text = msg.text().unwrap_or("").to_string();
            let channel = msg.chat.id.to_string();
            let author = msg
                .from
                .as_ref()
                .and_then(|u| u.username.clone())
                .unwrap_or_default();
            // Telegram convention: private chats use the user's positive
            // ID as the chat ID; group/supergroup/channel chats use
            // negative IDs. This is the documented Telegram Bot API
            // behavior for distinguishing DMs from group messages.
            let is_dm = msg.chat.id.0 > 0;

            let our_msg = Message::new(text, channel, author, Platform::Telegram, is_dm);
            let _ = Bot::dispatch_to_inner(&inner, our_msg);
            Ok(())
        }
    })
    .await;

    Ok(())
}

//! Discord bridge — adapts serenity's `EventHandler` trait to
//! `buff_chat::Bot::dispatch_to_inner`.
//!
//! This module is only used when [`crate::Platform::Discord`] is
//! selected. It builds a serenity `Client` with a bridge `EventHandler`
//! that converts each incoming `serenity::model::channel::Message`
//! into a platform-agnostic [`crate::Message`] and dispatches it
//! through the same routing logic that `bot.dispatch(msg)` uses.
//!
//! The bridge is intentionally thin: all handler registration and
//! command routing happens in [`crate::Bot`]. This module only does
//! platform-specific message conversion + connection lifecycle.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, RwLock};

use crate::{Bot, ChatError, Message, Platform};

/// The internal state shared between the `Bot` and the serenity
/// `EventHandler` bridge.
pub(crate) type SharedInner = Arc<RwLock<crate::BotInner>>;

/// Run the Discord bot event loop. Blocks until the connection drops
/// or the `running` flag is cleared by [`Bot::stop`].
///
/// Constructs a serenity `Client` with `GatewayIntents` for guild
/// messages + direct messages + message content, installs the
/// [`Bridge`] event handler, and calls `client.start().await`.
pub async fn run(
    token: &str,
    inner: SharedInner,
    running: &Arc<AtomicBool>,
) -> Result<(), ChatError> {
    use serenity::async_trait;
    use serenity::client::{Client, Context, EventHandler};
    use serenity::model::channel::Message as SerenityMessage;
    use serenity::model::gateway::GatewayIntents;

    struct Bridge {
        inner: SharedInner,
        running: Arc<AtomicBool>,
    }

    #[async_trait]
    impl EventHandler for Bridge {
        async fn message(&self, _ctx: Context, msg: SerenityMessage) {
            if !self.running.load(Ordering::Relaxed) {
                return;
            }
            let our_msg = Message::new(
                msg.content.clone(),
                msg.channel_id.to_string(),
                msg.author.name.clone(),
                Platform::Discord,
                msg.guild_id.is_none(),
            );
            let _ = Bot::dispatch_to_inner(&self.inner, our_msg);
        }
    }

    let intents = GatewayIntents::GUILD_MESSAGES
        | GatewayIntents::DIRECT_MESSAGES
        | GatewayIntents::MESSAGE_CONTENT;

    let mut client = Client::builder(token, intents)
        .event_handler(Bridge {
            inner,
            running: running.clone(),
        })
        .await
        .map_err(|e| ChatError::Connect(e.to_string()))?;

    client
        .start()
        .await
        .map_err(|e| ChatError::Runtime(e.to_string()))?;

    Ok(())
}

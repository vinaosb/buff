//! T47 `buff-chat` test suite — 12 mock-API tests.
//!
//! Per the T47 acceptance criterion: "10 tests (mock API)". All tests
//! exercise the dispatch / registration logic WITHOUT a live Discord
//! or Telegram connection. The `bot.dispatch(msg)` method feeds mock
//! messages through the same routing that the platform bridges use
//! internally, so the tests verify the complete handler-dispatch path.
//!
//! No network access, no real bot tokens, no serenity/teloxide runtime.

use buff_chat::{ChatError, Message, Platform};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

fn make_msg(text: &str) -> Message {
    Message::new(
        text.to_string(),
        "123456789".to_string(),
        "testuser".to_string(),
        Platform::Discord,
        false,
    )
}

fn poll_for<F: Fn() -> bool>(cond: F, timeout_ms: u64) -> bool {
    let deadline = Instant::now() + Duration::from_millis(timeout_ms);
    while Instant::now() < deadline {
        if cond() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(2));
    }
    cond()
}

// ---------------------------------------------------------------------------
// 1. Bot construction
// ---------------------------------------------------------------------------

#[test]
fn test_new_discord() {
    let bot = buff_chat::Bot::new(Platform::Discord, "discord-token-here".to_string());
    assert!(bot.is_ok(), "Discord bot construction should succeed");
    let bot = bot.expect("checked ok");
    assert_eq!(bot.platform(), Platform::Discord);
    assert!(bot.platform().is_discord());
    assert!(!bot.platform().is_telegram());
    assert_eq!(bot.command_count(), 0);
    assert!(!bot.has_message_handler());
    assert!(!bot.is_running());
}

#[test]
fn test_new_telegram() {
    let bot = buff_chat::Bot::new(Platform::Telegram, "telegram-token-here".to_string());
    assert!(bot.is_ok(), "Telegram bot construction should succeed");
    let bot = bot.expect("checked ok");
    assert_eq!(bot.platform(), Platform::Telegram);
    assert!(bot.platform().is_telegram());
    assert!(!bot.platform().is_discord());
}

// ---------------------------------------------------------------------------
// 2. Token validation
// ---------------------------------------------------------------------------

#[test]
fn test_empty_token_rejected() {
    let err = buff_chat::Bot::new(Platform::Discord, "".to_string());
    assert!(matches!(err, Err(ChatError::EmptyToken)));

    let err = buff_chat::Bot::new(Platform::Telegram, "   ".to_string());
    assert!(matches!(err, Err(ChatError::EmptyToken)));
}

// ---------------------------------------------------------------------------
// 3. Command registration
// ---------------------------------------------------------------------------

#[test]
fn test_command_registration() {
    let bot = buff_chat::Bot::new(Platform::Discord, "token".to_string()).expect("bot");
    assert_eq!(bot.command_count(), 0);

    bot.command("ping", |_msg| {})
        .expect("register ping");
    assert_eq!(bot.command_count(), 1);

    bot.command("echo", |_msg| {})
        .expect("register echo");
    assert_eq!(bot.command_count(), 2);
}

#[test]
fn test_duplicate_command_rejected() {
    let bot = buff_chat::Bot::new(Platform::Discord, "token".to_string()).expect("bot");
    bot.command("ping", |_msg| {})
        .expect("first ping");
    let err = bot.command("ping", |_msg| {});
    assert!(matches!(err, Err(ChatError::DuplicateCommand(_))));
}

#[test]
fn test_empty_command_name_rejected() {
    let bot = buff_chat::Bot::new(Platform::Discord, "token".to_string()).expect("bot");
    let err = bot.command("", |_msg| {});
    assert!(matches!(err, Err(ChatError::EmptyCommandName)));

    let err = bot.command("   ", |_msg| {});
    assert!(matches!(err, Err(ChatError::EmptyCommandName)));
}

// ---------------------------------------------------------------------------
// 4. on_message registration
// ---------------------------------------------------------------------------

#[test]
fn test_on_message_registration() {
    let bot = buff_chat::Bot::new(Platform::Discord, "token".to_string()).expect("bot");
    assert!(!bot.has_message_handler());

    bot.on_message(|_msg| {}).expect("register on_message");
    assert!(bot.has_message_handler());
}

// ---------------------------------------------------------------------------
// 5. Command dispatch (mock API — no network)
// ---------------------------------------------------------------------------

#[test]
fn test_dispatch_bang_command() {
    let bot = buff_chat::Bot::new(Platform::Discord, "token".to_string()).expect("bot");
    let received: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
    let received_clone = received.clone();

    bot.command("ping", move |msg| {
        if let Ok(mut g) = received_clone.lock() {
            g.push(msg.text().to_string());
        }
    })
    .expect("register");

    bot.dispatch(make_msg("!ping")).expect("dispatch");
    bot.dispatch(make_msg("!ping hello world")).expect("dispatch with args");

    assert!(
        poll_for(|| received.lock().map(|g| g.len()).unwrap_or(0) >= 2, 200),
        "command handler should fire for both !ping and !ping hello world"
    );

    let captured = received.lock().expect("final lock").clone();
    assert_eq!(captured, vec!["!ping".to_string(), "!ping hello world".to_string()]);
}

#[test]
fn test_dispatch_slash_command() {
    let bot = buff_chat::Bot::new(Platform::Telegram, "token".to_string()).expect("bot");
    let received: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
    let received_clone = received.clone();

    bot.command("start", move |msg| {
        if let Ok(mut g) = received_clone.lock() {
            g.push(msg.author().to_string());
        }
    })
    .expect("register");

    let msg = Message::new(
        "/start".to_string(),
        "12345".to_string(),
        "alice".to_string(),
        Platform::Telegram,
        true,
    );
    bot.dispatch(msg).expect("dispatch");

    assert!(
        poll_for(|| received.lock().map(|g| g.len()).unwrap_or(0) >= 1, 200),
        "slash command handler should fire"
    );
    assert_eq!(
        received.lock().expect("lock").clone(),
        vec!["alice".to_string()]
    );
}

// ---------------------------------------------------------------------------
// 6. on_message dispatch (fall-through for non-command messages)
// ---------------------------------------------------------------------------

#[test]
fn test_dispatch_on_message_fallthrough() {
    let bot = buff_chat::Bot::new(Platform::Discord, "token".to_string()).expect("bot");
    let received: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
    let received_clone = received.clone();

    bot.on_message(move |msg| {
        if let Ok(mut g) = received_clone.lock() {
            g.push(msg.text().to_string());
        }
    })
    .expect("register on_message");

    bot.dispatch(make_msg("hello there")).expect("dispatch");
    bot.dispatch(make_msg("not a command")).expect("dispatch");

    assert!(
        poll_for(|| received.lock().map(|g| g.len()).unwrap_or(0) >= 2, 200),
        "on_message should fire for both non-command messages"
    );
    let captured = received.lock().expect("final lock").clone();
    assert!(captured.contains(&"hello there".to_string()));
    assert!(captured.contains(&"not a command".to_string()));
}

#[test]
fn test_dispatch_unknown_command_falls_through() {
    let bot = buff_chat::Bot::new(Platform::Discord, "token".to_string()).expect("bot");
    let cmd_received: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
    let msg_received: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
    let cmd_clone = cmd_received.clone();
    let msg_clone = msg_received.clone();

    bot.command("known", move |msg| {
        cmd_clone.lock().expect("lock").push(msg.text().to_string());
    })
    .expect("register known");

    bot.on_message(move |msg| {
        msg_clone.lock().expect("lock").push(msg.text().to_string());
    })
    .expect("register on_message");

    bot.dispatch(make_msg("!unknown")).expect("dispatch unknown");

    assert!(
        poll_for(|| msg_received.lock().map(|g| g.len()).unwrap_or(0) >= 1, 200),
        "unknown command should fall through to on_message"
    );
    assert!(
        cmd_received.lock().map(|g| g.is_empty()).unwrap_or(true),
        "known command handler should NOT fire for !unknown"
    );
}

// ---------------------------------------------------------------------------
// 7. Platform + Message accessors
// ---------------------------------------------------------------------------

#[test]
fn test_platform_predicates() {
    assert!(Platform::Discord.is_discord());
    assert!(!Platform::Discord.is_telegram());
    assert!(Platform::Telegram.is_telegram());
    assert!(!Platform::Telegram.is_discord());

    assert_eq!(Platform::Discord.to_string(), "discord");
    assert_eq!(Platform::Telegram.to_string(), "telegram");
}

#[test]
fn test_message_accessors() {
    let msg = Message::new(
        "hello world".to_string(),
        "999".to_string(),
        "bob".to_string(),
        Platform::Telegram,
        true,
    );
    assert_eq!(msg.text(), "hello world");
    assert_eq!(msg.channel(), "999");
    assert_eq!(msg.author(), "bob");
    assert_eq!(msg.platform(), Platform::Telegram);
    assert!(msg.is_dm());

    let msg2 = make_msg("test");
    assert_eq!(msg2.platform(), Platform::Discord);
    assert!(!msg2.is_dm());
}

// ---------------------------------------------------------------------------
// 8. Bot introspection + Debug/Display
// ---------------------------------------------------------------------------

#[test]
fn test_bot_introspection() {
    let bot = buff_chat::Bot::new(Platform::Discord, "token".to_string()).expect("bot");

    assert!(!bot.is_running());
    assert_eq!(bot.command_count(), 0);
    assert!(!bot.has_message_handler());

    bot.command("a", |_| {}).expect("cmd a");
    bot.command("b", |_| {}).expect("cmd b");
    bot.on_message(|_| {}).expect("on_message");

    assert_eq!(bot.command_count(), 2);
    assert!(bot.has_message_handler());

    let debug_str = format!("{:?}", bot);
    assert!(debug_str.contains("Bot"));
    assert!(debug_str.contains("Discord"));

    let display_str = format!("{}", bot);
    assert!(display_str.contains("discord"));
    assert!(display_str.contains("2 commands"));
    assert!(display_str.contains("with on_message"));
}

// ---------------------------------------------------------------------------
// 9. Stop when not running
// ---------------------------------------------------------------------------

#[test]
fn test_stop_not_running() {
    let bot = buff_chat::Bot::new(Platform::Discord, "token".to_string()).expect("bot");
    let err = bot.stop();
    assert!(matches!(err, Err(ChatError::NotRunning)));
}

// ---------------------------------------------------------------------------
// 10. Default trait
// ---------------------------------------------------------------------------

#[test]
fn test_bot_default() {
    let bot = buff_chat::Bot::default();
    assert_eq!(bot.platform(), Platform::Discord);
    assert_eq!(bot.command_count(), 0);
    assert!(!bot.has_message_handler());
    assert!(!bot.is_running());
}

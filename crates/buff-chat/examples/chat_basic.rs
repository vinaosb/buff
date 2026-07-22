// T47 example: basic Bot construction + handler registration + mock dispatch.
//
// Demonstrates the minimal mock-API surface: create a Discord bot,
// register a !ping command handler + an on_message catch-all, then
// dispatch mock messages through `bot.dispatch(msg)` — no network
// connection needed. This is the same path the platform bridges
// (serenity/teloxide) use internally on real incoming messages.

use buff_chat::{Bot, Message, Platform};
use std::sync::{Arc, Mutex};

fn main() {
    let bot = Bot::new(Platform::Discord, "your-bot-token-here".to_string()).expect("bot");

    let ping_count = Arc::new(Mutex::new(0u32));
    let ping_count_clone = ping_count.clone();
    bot.command("ping", move |_msg| {
        let mut g = ping_count_clone.lock().expect("lock");
        *g += 1;
        println!("pong! (count: {})", *g);
    })
    .expect("register ping");

    let other_count = Arc::new(Mutex::new(0u32));
    let other_count_clone = other_count.clone();
    bot.on_message(move |msg| {
        let mut g = other_count_clone.lock().expect("lock");
        *g += 1;
        println!("on_message: {:?} from @{}", msg.text(), msg.author());
    })
    .expect("register on_message");

    println!("bot: {}", bot);
    println!("commands: {}", bot.command_count());

    let mock_msgs = vec![
        Message::new("!ping".to_string(), "1".to_string(), "alice".to_string(), Platform::Discord, false),
        Message::new("hello there".to_string(), "1".to_string(), "bob".to_string(), Platform::Discord, false),
        Message::new("!ping again".to_string(), "1".to_string(), "alice".to_string(), Platform::Discord, false),
        Message::new("goodbye".to_string(), "1".to_string(), "charlie".to_string(), Platform::Discord, true),
    ];
    for msg in mock_msgs {
        bot.dispatch(msg).expect("dispatch");
    }

    println!("\n--- summary ---");
    println!("ping handler fired: {} times", *ping_count.lock().expect("lock"));
    println!("on_message fired:   {} times", *other_count.lock().expect("lock"));
}

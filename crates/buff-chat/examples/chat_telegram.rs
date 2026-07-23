// T47 example: Telegram bot with command + on_message + start().
//
// Run with: TELEGRAM_BOT_TOKEN=your-token cargo run --example chat_telegram -p buff-chat
//
// Requires a Telegram bot token from @BotFather.
// Once running, the bot responds to /ping and /echo commands in any
// chat it's added to, and prints every other message via on_message.
// Press Ctrl-C to stop.

use buff_chat::{Bot, Platform};
use std::env;

fn main() {
    let token = env::var("TELEGRAM_BOT_TOKEN").unwrap_or_else(|_| {
        eprintln!("set TELEGRAM_BOT_TOKEN env var to your Telegram bot token");
        std::process::exit(1);
    });

    let bot = Bot::new(Platform::Telegram, token).expect("bot");

    bot.command("ping", |msg| {
        println!(
            "[CMD] /ping from @{} in chat {}",
            msg.author(),
            msg.channel()
        );
        println!("[CMD]   is_dm: {}", msg.is_dm());
    })
    .expect("register ping");

    bot.command("start", |msg| {
        println!(
            "[CMD] /start from @{} (chat: {})",
            msg.author(),
            msg.channel()
        );
    })
    .expect("register start");

    bot.on_message(|msg| {
        println!("[MSG] @{}: {:?}", msg.author(), msg.text());
    })
    .expect("register on_message");

    println!("Telegram bot: {}", bot);
    println!("Connecting... (Ctrl-C to stop)");

    match bot.start() {
        Ok(()) => println!("Bot stopped cleanly."),
        Err(e) => eprintln!("Bot error: {}", e),
    }
}

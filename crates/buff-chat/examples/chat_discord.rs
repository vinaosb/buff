// T47 example: Discord bot with command + on_message + start().
//
// Run with: DISCORD_TOKEN=your-token cargo run --example chat_discord -p buff-chat
//
// Requires a Discord bot token from the Discord Developer Portal.
// The MESSAGE_CONTENT privileged intent must be enabled on the bot
// application page (serenity requests GatewayIntents::MESSAGE_CONTENT).
//
// Once running, the bot responds to !ping in any channel it can see,
// and prints every other message via the on_message handler.
// Press Ctrl-C to stop.

use buff_chat::{Bot, Platform};
use std::env;

fn main() {
    let token = env::var("DISCORD_TOKEN").unwrap_or_else(|_| {
        eprintln!("set DISCORD_TOKEN env var to your Discord bot token");
        std::process::exit(1);
    });

    let bot = Bot::new(Platform::Discord, token).expect("bot");

    bot.command("ping", |msg| {
        println!("[CMD] !ping from @{} in channel {}", msg.author(), msg.channel());
        println!("[CMD]   text: {:?}", msg.text());
        println!("[CMD]   is_dm: {}", msg.is_dm());
    })
    .expect("register ping");

    bot.command("echo", |msg| {
        println!("[CMD] !echo from @{}: {}", msg.author(), msg.text());
    })
    .expect("register echo");

    bot.on_message(|msg| {
        println!("[MSG] @{}: {:?}", msg.author(), msg.text());
    })
    .expect("register on_message");

    println!("Discord bot: {}", bot);
    println!("Connecting... (Ctrl-C to stop)");

    match bot.start() {
        Ok(()) => println!("Bot stopped cleanly."),
        Err(e) => eprintln!("Bot error: {}", e),
    }
}

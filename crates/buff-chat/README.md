# buff-chat

> Discord + Telegram bot framework for the **Buff** language. Pure-Rust MVP wrapping `serenity` + `teloxide`.

`buff-chat` provides a unified `Bot` type that connects to either Discord (via serenity) or Telegram (via teloxide) with the same handler API. Buff code uses the `Bot` prelude type (wiring lands in a separate follow-up commit per the buff-image T9 precedent):

```buff
let bot = Bot.new(platform: Platform.Discord, token: "your-token")
bot.command(name: "ping", handler: { msg =>
    print("pong from ${msg.author()}!")
})
bot.on_message(handler: { msg =>
    print("[msg] @${msg.author()}: ${msg.text()}")
})
bot.start()
```

**Status: experimental** (T47 v1.17 frameworks wave 6).

## Installation

This crate is consumed by the Buff compiler's codegen layer; end users do not install it directly. It is automatically pulled in as a path dependency of the workspace when a Buff program uses the `Bot` prelude type.

For direct Rust use:

```bash
cargo add buff-chat --path crates/buff-chat
```

## Quick start

```rust
use buff_chat::{Bot, Message, Platform};

fn main() {
    let bot = Bot::new(Platform::Discord, "your-bot-token".to_string()).expect("bot");

    bot.command("ping", |msg| {
        println!("pong from @{}!", msg.author());
    }).expect("register ping");

    bot.on_message(|msg| {
        println!("[msg] @{}: {:?}", msg.author(), msg.text());
    }).expect("register on_message");

    // Blocks — connects to Discord and runs the event loop:
    bot.start().expect("start");
}
```

## Public API

### `Bot` — cross-platform chat bot

| Method | Signature | Notes |
|---|---|---|
| `Bot::new` | `(platform, token) -> Result<Bot, ChatError>` | Validates token non-empty. `catch_unwind` boundary. |
| `bot.command` | `(name, handler) -> Result<(), ChatError>` | Prefix-based: `!name` / `/name`. Rejects empty + duplicate names. |
| `bot.on_message` | `(handler) -> Result<(), ChatError>` | Catch-all for non-command messages. Replaces previous handler. |
| `bot.start` | `() -> Result<(), ChatError>` | Blocks on platform event loop. Builds own tokio runtime. |
| `bot.stop` | `() -> Result<(), ChatError>` | Cooperative shutdown via AtomicBool flag. |
| `bot.dispatch` | `(msg) -> Result<(), ChatError>` | Programmatic dispatch (testing + mock API). |
| `bot.platform` | `() -> Platform` | |
| `bot.is_running` | `() -> bool` | Point-in-time snapshot. |
| `bot.command_count` | `() -> usize` | |
| `bot.has_message_handler` | `() -> bool` | |

### `Message` — incoming chat message

| Method | Signature |
|---|---|
| `Message::new` | `(text, channel, author, platform, is_dm) -> Message` |
| `msg.text` | `() -> &str` |
| `msg.channel` | `() -> &str` |
| `msg.author` | `() -> &str` |
| `msg.platform` | `() -> Platform` |
| `msg.is_dm` | `() -> bool` |

### `Platform` — Discord or Telegram

| Method | Signature |
|---|---|
| `Platform::is_discord` | `(self) -> bool` |
| `Platform::is_telegram` | `(self) -> bool` |

## Command dispatch

Messages starting with `!` or `/` are treated as commands:

```
"!ping"            → command "ping"
"/ping hello"      → command "ping"
"!echo hello world"→ command "echo"
"hello there"      → on_message (no prefix)
"!unknown"         → on_message (unknown command falls through)
```

Both `!ping` (Discord convention) and `/ping` (Telegram convention) trigger the same registered handler. This lets a single bot definition work on both platforms without per-platform registration.

## FFI safety

Every public function follows the [6 hard rules](../buff-lang-ffi-guide/GUIDE.md) from the FFI guide:

| Rule | Compliance |
|---|---|
| R1 — No raw pointers | Public surface: `Bot`, `Platform`, `Message`, `ChatError`. No `*const`/`*mut`. |
| R2 — Ownership boundary | `command`/`on_message` consume owned `Fn` closures. `dispatch` takes owned `Message`. |
| R3 — Error mapping | `new`/`command`/`on_message`/`start`/`stop`/`dispatch` return `Result<T, ChatError>`. |
| R4 — Thread safety | `Bot` is `Send + Sync`. Handlers require `Fn + Send + Sync + 'static`. |
| R5 — Lifetime hiding | No public lifetime parameters. All `&str` args copied to owned `String`. |
| R6 — Panic boundary | All public methods wrap bodies in `catch_unwind`. Handler panics caught in dispatch. |

## Testing

```bash
cargo test -p buff-chat
cargo clippy -p buff-chat --all-targets -- -D warnings
cargo fmt -p buff-chat --check
```

Tests are hermetic: no network access, no real bot tokens. The `bot.dispatch(msg)` method feeds mock messages through the same routing that the platform bridges use internally. 12 mock-API tests covering construction, registration, dispatch, and introspection.

## Deferred to v1.18+

- Discord slash commands (interaction-based).
- Telegram command registration (`setMyCommands`).
- Rich message types (embeds, media, reactions, attachments).
- Reply / send API (`bot.send(channel, text)`).
- Webhooks, multiple on_message handlers, middleware/plugin system.

## License

Dual-licensed under [MIT](../../LICENSE) or [Apache-2.0](../../LICENSE), matching the rest of the Buff workspace.

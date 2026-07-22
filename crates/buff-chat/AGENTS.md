# buff-chat

Discord + Telegram bot framework for the Buff language. Pure-Rust MVP wrapping [`serenity`](https://crates.io/crates/serenity) (Discord Gateway + HTTP API) + [`teloxide`](https://crates.io/crates/teloxide) (Telegram Bot API) behind a safe Rust API that follows the [T4 FFI safety guide](../buff-lang-ffi-guide/GUIDE.md).

**Status: experimental** (T47 v1.17 frameworks wave 6).

## STRUCTURE

```
buff-chat/
├── Cargo.toml              # serenity + teloxide + async-trait + tokio + thiserror + insta deps
├── src/
│   ├── lib.rs              # Bot + Platform + command dispatch (~460 LOC)
│   ├── message.rs          # Message struct + accessors (~130 LOC)
│   ├── error.rs            # ChatError enum (~80 LOC)
│   ├── discord.rs          # serenity EventHandler bridge (~75 LOC)
│   └── telegram.rs         # teloxide repl bridge (~60 LOC)
├── examples/
│   ├── chat_basic.rs       # mock dispatch (no network) — command + on_message
│   ├── chat_discord.rs     # Discord bot via bot.start() (needs DISCORD_TOKEN)
│   ├── chat_telegram.rs    # Telegram bot via bot.start() (needs TELEGRAM_BOT_TOKEN)
│   └── chat/
│       └── chat_basic.buff # Buff-side forward-decl (matches .rs)
└── tests/
    └── core.rs             # 12 mock-API tests (~260 LOC)
```

Total: ~1070 LOC (well under the 3000 LOC T47 cap).

## WHERE TO LOOK

| Task | File |
|---|---|
| Add a new bot method | `src/lib.rs` (add `pub fn` on `Bot`) + test in `tests/core.rs` |
| Add a new Message field | `src/message.rs` (constructor + accessor) |
| Add a new error variant | `src/error.rs` |
| Change Discord message conversion | `src/discord.rs` (Bridge::message handler) |
| Change Telegram message conversion | `src/telegram.rs` (repl closure) |
| Wire a Buff-side method to codegen | `crates/buff-lang-types/src/prelude_types.rs` (PreludeInstanceFn + `instance_fn_return_type`) + `crates/buff-lang-codegen-rust/src/rust_codegen.rs::lower_prelude_type_instance_fn` (NOT in MVP commit — separate follow-up per buff-image T9 precedent) |

## PUBLIC API (18 functions, ≤20 cap)

### `Bot` (10 functions)
- Constructors: `Bot::new(platform, token) -> Result<Bot, ChatError>`
- Registration: `bot.command(name, handler) -> Result<(), ChatError>`, `bot.on_message(handler) -> Result<(), ChatError>`
- Lifecycle: `bot.start() -> Result<(), ChatError>` (blocks), `bot.stop() -> Result<(), ChatError>`
- Dispatch: `bot.dispatch(msg) -> Result<(), ChatError>` (public, for testing + programmatic use)
- Introspection: `bot.platform() -> Platform`, `bot.is_running() -> bool`, `bot.command_count() -> usize`, `bot.has_message_handler() -> bool`

### `Message` (6 functions)
- Constructor: `Message::new(text, channel, author, platform, is_dm) -> Message`
- Accessors: `text() -> &str`, `channel() -> &str`, `author() -> &str`, `platform() -> Platform`, `is_dm() -> bool`

### `Platform` (2 functions)
- Predicates: `Platform::is_discord() -> bool`, `Platform::is_telegram() -> bool`

## CONVENTIONS

- **Pure-Rust only**: `serenity` 0.12 with `rustls_backend` (NOT `native_tls_backend`) + `teloxide` 0.13 with `rustls` (NOT `native-tls`). Both use rustls + ring (pure-Rust TLS). No cc-rs, no native C deps — matches the "no C library, no Docker" hard rule from T126/T127.
- **FFI safety**: every public entry point follows the 6 hard rules from `crates/buff-lang-ffi-guide/GUIDE.md`. See the compliance table in `src/lib.rs` module doc.
- **Panic-free**: no `unwrap` / `expect` / `panic!` / `todo!` / `unimplemented!` in non-test code. All fallible ops return `Result<T, ChatError>`. Handler panics are caught inside `dispatch_to_inner` (per FFI guide R6).
- **catch_unwind boundary**: `new` / `command` / `on_message` / `start` / `stop` / `dispatch` wrap their bodies in `catch_unwind` per FFI guide R6. A panic in the user-supplied handler is caught inside the dispatch path so one bad handler doesn't crash the event loop (mirrors the buff-pubsub EventEmitter precedent).
- **Cross-platform command dispatch**: both `!ping` (Discord convention) and `/ping` (Telegram convention) trigger the same registered command handler. The `extract_command_name` helper strips the prefix (`!` or `/`) and extracts the first whitespace-delimited word.
- **Mock-API testing**: `bot.dispatch(msg)` is public so tests can exercise the complete handler routing without a live network connection. The platform bridges (`discord`, `telegram`) call the same internal `dispatch_to_inner` on every incoming message. Tests pass mock messages and verify handler invocation via shared `Arc<Mutex<Vec<_>>>` capture vectors.

## RELATIONSHIP TO OTHER CRATES

| Crate | Relationship |
|---|---|
| `serenity` | Upstream Discord bot framework. `buff-chat` is a safe wrapper; never re-exports `serenity::*` types directly. The `discord::run` function builds a serenity `Client` with a bridge `EventHandler` that converts `serenity::model::channel::Message` → `buff_chat::Message` and dispatches via `Bot::dispatch_to_inner`. |
| `teloxide` | Upstream Telegram bot framework. `buff-chat` is a safe wrapper; never re-exports `teloxide::*` types directly. The `telegram::run` function builds a `teloxide::Bot` and runs `teloxide::repl` with a closure that converts `teloxide::types::Message` → `buff_chat::Message` and dispatches via `Bot::dispatch_to_inner`. |
| `tokio` | Async runtime. `bot.start()` builds a multi-threaded tokio runtime and blocks on the platform event loop. The `AlreadyInRuntime` error guards against nested-runtime panics. |
| `buff-lang-types` | **NOT YET WIRED** in MVP commit. The follow-up commit (mirrors buff-image T9 "wire prelude+codegen" precedent) will add: `Type::Bot` variant + `bot()` + `is_prelude_bot()` predicates in `ty.rs`; `PreludeType::Bot` + `PreludeAssocFn::New` + 5 `PreludeInstanceFn` arms (Command / OnMessage / Start / Stop / Dispatch) + 4 introspection arms (Platform / IsRunning / CommandCount / HasMessageHandler) in `prelude_types.rs`. Also `Type::Platform` variant for the platform enum + `Type::Message` for the message struct (or lower via opaque handles). |
| `buff-lang-codegen-rust` | **NOT YET WIRED** in MVP commit. The follow-up commit will add: `buff_type_to_syn Type::Bot => "buff_chat::Bot"` arm; `lower_prelude_type_assoc_fn (Bot, New)` arm; `lower_prelude_type_instance_fn` Bot arms (Command uses a closure-lowering helper mirroring EventBus::subscribe; OnMessage same shape; Start/Stop/Dispatch/Platform/IsRunning/CommandCount/HasMessageHandler are simple method calls); `program_uses_namespace("Bot")` records `buff-chat` + `serenity` + `teloxide` + `async-trait` + `tokio` in `extern_crates`. |
| `buff-lang-ffi-guide` | Defines the 6 hard rules every public function in this crate follows. |

## NOTES

- **No prelude/codegen wiring in this MVP commit** per the buff-image T9 precedent: the T9 MVP commit shipped the crate alone, and a separate "feat(buff-image): wire prelude+codegen for Image instance methods + add 2 examples (T9 finish)" follow-up commit landed the wiring. T47 follows the same two-commit split. The user's task spec mandated this scope: "DO NOT: touch other crates".
- **MSVC host blocker**: `cargo test -p buff-chat` is expected to fail on this Windows host with `LINK : fatal error LNK1104: cannot open file 'msvcrt.lib'` — pre-existing VS 18 Insiders + missing Windows SDK UCRT headers issue (same family that blocks `cargo check --workspace` here, documented in buff-image's AGENTS.md). CI runs on a 3-OS matrix (ubuntu/windows/macos) and does NOT have this issue. `cargo check -p buff-chat --lib` and `cargo clippy -p buff-chat --all-targets -- -D warnings` pass clean.
- **Command dispatch is prefix-based**: messages starting with `!` or `/` are treated as commands. The first whitespace-delimited word after the prefix is the command name. This is the simplest cross-platform command system (no slash-command registration with Discord API, no /setMyCommands with Telegram API). A future v1.18+ enhancement can add native Discord slash commands + Telegram command registration.
- **`start()` blocks and creates its own runtime**: the caller must invoke `start` from a non-async context (e.g. `fn main()`). Calling from inside a tokio runtime returns `ChatError::AlreadyInRuntime`. An async variant (`start_async`) is deferred to v1.18+.
- **`stop()` is cooperative**: the `running` `AtomicBool` is checked by the platform bridges between message dispatches. `stop` does NOT immediately abort the event loop — an in-flight handler runs to completion. The serenity `client.start()` may not exit until the next message arrives (serenity's gateway loop blocks on WebSocket recv). The teloxide `repl` similarly blocks on long-poll. For immediate shutdown, the caller can additionally abort the process (Ctrl-C / `tokio::runtime::Runtime::shutdown_background`).
- **Bot impls Default** as an empty Discord bot with empty token (used by codegen fallback for panic-free `unwrap_or_default()` paths — matches the Image / DataFrame / EventBus precedent).
- **GatewayIntents**: Discord requires `MESSAGE_CONTENT` (privileged intent — must be enabled in the Discord Developer Portal under Bot → Privileged Gateway Intents). Without it, `msg.content` is empty for guild messages. DMs always include content.
- **Telegram chat ID convention**: private chats have positive chat IDs (= user IDs); groups/supergroups/channels have negative IDs. `is_dm` uses `msg.chat.id.0 > 0` per the documented Telegram Bot API behavior.

## DEFERRED (v1.18+)

- **Discord slash commands**: the MVP uses prefix-based `!command`. Native Discord slash commands (interaction-based, requires Discord application registration) are deferred.
- **Telegram command registration**: the MVP does not call `setMyCommands`. Telegram `/command` autocompletion is deferred.
- **Rich message types**: the MVP `Message` only carries text + channel + author. Embeds (Discord), media (photos/stickers/voice), reactions, and attachments are deferred.
- **Reply / send API**: the MVP is read-only (handlers receive messages but cannot send replies). A `bot.send(channel, text)` method + platform-specific reply lowering is deferred.
- **Webhooks**: Discord webhook mode (alternative to Gateway WebSocket) is deferred.
- **Multiple on_message handlers**: the MVP supports one `on_message` handler. Multiple handlers + handler priorities are deferred.
- **Middleware / plugin system**: pre/post dispatch middleware (logging, rate limiting, auth) is deferred.
- **Session state**: per-user or per-channel persistent state is deferred (use `buff-cache` T31 for the MVP).

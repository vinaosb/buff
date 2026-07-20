# Async / Await

## What Rust pain does Buff avoid?

Rust's async model has two major ergonomic problems:

1. **The function-coloring problem** -- a function marked `async` can only be
   called from other `async` functions. If you want to call an async function
   from sync code, you're stuck. You must propagate `async` through the
   entire call chain or use a runtime-specific block like `tokio::spawn`.
   This is often called "function coloring" and it forces architectural
   decisions early.

2. **`.await` everywhere** -- every async call site needs `.await`. Forgetting
   it gives a confusing error ("future not awaited"). Having it in the wrong
   place causes the function to hang.

3. **Runtime dependency** -- `#[tokio::main]`, `Cargo.toml` dependency on tokio,
   feature flags (`full`, `rt-multi-thread`), and understanding the difference
   between tokio and async-std runtimes.

## The Buff equivalent

Buff has **no `await` keyword**. When a function is declared `async func`,
every caller automatically becomes async too (the compiler propagates it
through the call graph). `.await` is inserted at async call sites automatically.
You get `task.result()` instead of `.await` for spawned tasks. The compiler
adds `#[tokio::main]` to `main` when it joins the async set.

## Status

**CodeGen-only** -- async codegen is fully verified in
`crates/buff-lang-codegen-rust/tests/async_codegen.rs`, but async requires
the `tokio` external crate. The single-file `rustc` pipeline cannot link
external crates, so `buff run` will fail at the link step.

## Key differences

| Rust | Buff |
|---|---|
| `.await` on every async call | No `await` keyword |
| `async fn pipeline()` required on caller | Auto-propagated by compiler |
| `#[tokio::main]` annotation | Auto-inserted when main is async |
| `tokio::spawn(async { ... }).await` | `spawn task()` then `task.result()` |
| Function coloring forces architecture | Transparent to the programmer |

# Extern Guide — Calling Rust Crates from Buff (T119)

> **TL;DR** — Declare an `extern "C" from "<crate>" func ...` to call into a
> Rust crate from Buff. The compiler emits a `use <crate>;` import, a
> Rust `extern "C" { fn ...; }` foreign-mod item, silently wraps every
> call site in `unsafe { ... }`, and records the crate in `[rust-deps]`
> so your `buff.toml` (and the future generated `Cargo.toml`) carries the
> dependency automatically.

---

## Why extern?

Buff's selling point is hiding the borrow checker. But sometimes you
need a library that hasn't been ported to Buff yet — `serde_json` for
parsing, `reqwest` for HTTP, `tokio` for async timing. Rather than
reimplement these, Buff lets you declare their public functions as
**extern FFI bindings** and call them like any other Buff function.

The extern mechanism is the **minimal viable bindgen** (T119): a manual,
declarative form. A future `buff bindgen` tool (post-v1.3) will auto-
generate these declarations from a Rust crate's public API; for now you
hand-write one declaration per function you want to call.

## The three extern forms

| Form | Since | Lowers to |
|---|---|---|
| `extern crate "serde"` | v0.5 (T32) | `use serde;` + records crate in `[rust-deps]` |
| `extern func name(...) -> Ret` | v0.5 (T32) | `extern "C" { fn name(...); }` (ABI hardcoded to `"C"`) |
| `extern "C" func name(...) -> Ret` | **v1.3 (T119)** | Same foreign-mod, ABI sourced from the literal |
| `extern "C" from "serde_json" func name(...) -> Ret` | **v1.3 (T119)** | Foreign-mod + records `serde_json` in `[rust-deps]` |

The new v1.3 form is the **recommended form** — it carries both the ABI
and the source crate annotation, so the compiler can fully populate the
`[rust-deps]` section of `buff.toml` from your extern declarations
alone.

## Syntax

```buff
extern "<ABI>" [from "<crate>"] func <name>(<params>) [-> <Ret>]
```

- **`<ABI>`** — a string literal naming the calling convention. Only
  `"C"` is supported in v1.3 (the spec mandates it for cross-language
  stability). Other ABIs (`"system"`, `"stdcall"`, `"fastcall"`) are a
  parse error.
- **`from "<crate>"`** — optional source-crate annotation. When present
  the crate name is recorded for `[rust-deps]` auto-population.
- **`<name>`** — the Buff-side function name. You call it in Buff source
  by this name.
- **`<params>`** — comma-separated `name: Type` pairs. Types use the
  standard Buff→Rust mapping (see [Type marshalling](#type-marshalling)
  below).
- **`-> <Ret>`** — optional return type. Omit for `()` (unit) returns.

### What gets generated

For:

```buff
extern "C" from "serde_json" func parse_str(input: String) -> String
```

The compiler emits:

```rust
extern "C" {
    fn parse_str(input: String) -> String;
}
```

Plus records `serde_json = "*"` in `[rust-deps]` for your `buff.toml`.

### Calling the extern function

You call an extern function like any other Buff function:

```buff
let parsed = parse_str(my_string)
```

The compiler silently wraps the call in `unsafe { ... }`:

```rust
let parsed = unsafe { parse_str(my_string) };
```

Rust requires the `unsafe` block because foreign functions are not
memory-safe by construction. Buff hides this from you — your source
code has no `unsafe` keyword anywhere.

## Type marshalling

The standard Buff→Rust primitive mapping applies to extern signatures:

| Buff type | Rust type | Notes |
|---|---|---|
| `String` | `String` | Owned UTF-8 string. |
| `Int` | `i64` | 64-bit signed integer. |
| `Float` | `f32` | 32-bit float (use `Double` for `f64`). |
| `Double` | `f64` | 64-bit float. |
| `Bool` | `bool` | |
| `Byte` | `u8` | |
| `Bits` | `u64` | |
| `Char` | `char` | |
| `Decimal` | `rust_decimal::Decimal` | |
| `Vector<T>` | `Vector<T>` | **Caveat:** the unresolved `TypeRef::Generic` path passes the base name through verbatim (`Vector<T>` → `Vector<T>`), so the `Vector`→`Vec` spelling rewrite does NOT happen on extern signatures today. Declare externs in terms of concrete element types only (`Vector<Int>`, not abstract `Vector<T>`). |

Generics are **REJECTED** on extern declarations (see
[Limitations](#limitations)).

## Auto-populating `[rust-deps]`

Every `extern "C" from "<crate>" func ...` and every `extern crate
"<crate>"` declaration contributes the named crate to the program's
`[rust-deps]` set. The CLI exposes this via:

```rust
use buff_lang_codegen_rust::collect_rust_deps;

let deps: BTreeSet<String> = collect_rust_deps(&decls);
// Render as a TOML block:
let toml = buff_lang_cli::config::render_rust_deps_toml(&deps);
```

The output is deterministic (sorted, deduped) and ready to splice into
`buff.toml`:

```toml
[rust-deps]
serde_json = "*"
```

The wildcard `"*"` says "use the latest compatible version" — tighten
to a specific version (e.g. `"1"`) by editing `buff.toml` directly.

## Examples

Three working examples ship in [`examples/`](../examples/):

| Example | Crate | Demonstrates |
|---|---|---|
| [`extern_serde_json.buff`](../examples/extern_serde_json.buff) | `serde_json` | String→String parse wrapper |
| [`extern_reqwest.buff`](../examples/extern_reqwest.buff) | `reqwest` | String→String HTTP fetch |
| [`extern_tokio.buff`](../examples/extern_tokio.buff) | `tokio` | Int→unit sleep |

Each example is **codegen-only** (parses + lowers to valid Rust), but
not yet runnable end-to-end via `buff run` because the single-file
`rustc` pipeline cannot link external crates without a Cargo manifest.
This matches the existing v0.5 caveats (`async_demo.buff`,
`modules/*`). The next CLI milestone gains Cargo-project assembly; once
it lands, these examples become runnable with no source changes.

## Manual Cargo-project recipe (for early adopters)

If you want to run an extern example end-to-end today, you need a Cargo
project that includes:

1. The Buff-generated `.rs` file (from `buff run` — produced even when
   the link step fails).
2. A sibling `externs.rs` providing the actual safe wrapper bodies:

   ```rust
   // externs.rs
   pub extern "C" fn parse_str(input: String) -> String {
       serde_json::from_str::<serde_json::Value>(&input)
           .map(|v| v.to_string())
           .unwrap_or_else(|e| format!("error: {e}"))
   }
   ```

3. A `Cargo.toml` listing `serde_json = "1"` in `[dependencies]`.
4. `cargo run` to build and execute the full project.

The full picture:

```
extern_demo/
├── Cargo.toml           # [dependencies] serde_json = "1"
├── externs.rs           # the actual safe wrappers
└── src/
    └── main.rs          # generated by `buff run` from your .buff file
```

## Limitations (v1.3)

- **Generics unsupported.** `extern "C" func parse<T>(...) -> T` is a
  parse error with a clear message. Declare a separate concrete
  wrapper per type you need (`parse_int`, `parse_string`, …).
- **`"C"` ABI only.** Other ABIs (`"system"`, `"stdcall"`, …) are a
  parse error today. The accept-list will widen in a future release.
- **Vector base name not rewritten.** On extern signatures,
  `Vector<T>` stays as `Vector<T>` in the generated Rust (the
  `Vector`→`Vec` spelling rewrite only happens on the RESOLVED type
  path, which extern signatures don't use). Use concrete element
  types only.
- **No auto-bindgen.** You hand-write one declaration per Rust
  function you want to call. A future `buff bindgen` tool will
  generate these from a crate's public API.
- **No whole-crate import.** Only the specific functions you declare
  become visible to Buff. The rest of the crate's API is unreachable.
- **Unsafe-by-name only.** Buff cannot statically verify that the
  extern function is safe to call — it trusts your declaration. The
  silent `unsafe { ... }` wrap is a syntactic guard, not a semantic
  one.

## Design rationale

For the full design notes — why the additive `Decl::ExternFuncDecl`
variant, why "C" only, why silent unsafe wrapping — see the per-crate
`AGENTS.md` files and `.sisyphus/notepads/buff-post-v10-tooling/`.

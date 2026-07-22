# buff-protobuf

Protocol Buffers for the **Buff** language. Pure-Rust MVP (CPU-only) wrapping [`prost`](https://docs.rs/prost) + [`prost-types`](https://docs.rs/prost-types) behind a safe Rust API that follows the [T4 FFI safety guide](../buff-lang-ffi-guide/GUIDE.md).

**Status: experimental** (T52 v1.17 frameworks wave 4).

## STRUCTURE

```
buff-protobuf/
├── Cargo.toml           # prost + prost-types + serde + serde_json + thiserror + insta deps
├── src/
│   ├── lib.rs           # Message + serialize/deserialize/roundtrip free fns (~560 LOC)
│   └── error.rs         # ProtobufError enum (~95 LOC)
└── tests/
    └── core.rs          # integration tests (mirrors buff-msgpack layout) — DEFERRED
```

Total: ~650 LOC (well under the 3000 LOC T52 cap).

## WHERE TO LOOK

| Task | File |
|---|---|
| Add a new top-level helper | `src/lib.rs` (add `pub fn`) |
| Add a new `Message` method | `src/lib.rs` (add `pub fn` on `Message`) |
| Add a new error variant | `src/error.rs` + `From` impl if it wraps an underlying error |
| Wire a Buff-side method to codegen | `crates/buff-lang-types/src/prelude_types.rs` (`PreludeType::Protobuf` + `PreludeAssocFn::{Encode, Decode}`) + `crates/buff-lang-codegen-rust/src/rust_codegen.rs::lower_prelude_type_assoc_fn` |

## PUBLIC API (10 functions, ≤20 cap)

### `Message` (7 functions)
- Constructors: `Message.new(value)`, `Message.from_bytes(bytes)`, `Message.decode(bytes)` (class-method alias)
- Accessors: `message.type_url()`, `message.byte_size()`, `message.encode()`
- Decode: `message.payload() -> Result<Value, ProtobufError>`

### Free functions (3 functions)
- `serialize(value) -> Result<Vec<u8>, ProtobufError>`
- `deserialize(bytes) -> Result<Value, ProtobufError>`
- `roundtrip(value) -> Option<Value>` (test/codegen helper)

## CONVENTIONS

- **Pure-Rust only**: `prost` + `prost-types` are pure-Rust (RustCrypto-adjacent). NO protoc / NO native protobuf library — matches "no C library" hard rule. `prost-build` + `tonic` (gRPC transport + build-time `.proto` codegen) DEFERRED per the T52 task spec ("gRPC streaming in MVP: NO; unary only").
- **CPU-only**: NO GPU dispatch. The well-known `Struct` encode/decode is single-threaded CPU code.
- **FFI safety**: every public entry point follows the 6 hard rules from `crates/buff-lang-ffi-guide/GUIDE.md`. See the compliance table in `src/lib.rs` module doc.
- **Panic-free**: no `unwrap` / `expect` / `panic!` / `todo!` in non-test code. `catch_unwind` boundary on every public fn per FFI guide R6.
- **`"_value"` magic key**: protobuf's `Value` message is never a top-level message (only `Struct` is). The MVP wraps non-object JSON shapes (scalars / arrays) in a single-field `Struct { fields: {"_value": v} }` so they survive the wire format. The inverse unwrap is in `struct_to_value`.
- **NaN / Infinity rejection**: protobuf `Value::number_value` is a finite `f64`. `serialize(json!(NaN))` returns `Err(NonFiniteNumber)` so the roundtrip is well-defined (NaN != NaN in IEEE-754 would otherwise silently corrupt roundtrips).

## RELATIONSHIP TO OTHER CRATES

| Crate | Relationship |
|---|---|
| `prost` + `prost-types` | Upstream codec providers. `buff-protobuf` is a safe wrapper; never re-exports `prost::*` types directly. |
| `buff-lang-types` | `prelude_types.rs` registers `PreludeType::Protobuf` + `PreludeAssocFn::{Encode, Decode}`. `ty.rs` has the `Type::Protobuf` variant + `is_prelude_protobuf()` predicate (the latter added in a follow-up alongside T51's msgpack predicate — namespace-only types share the same predicate shape). |
| `buff-lang-codegen-rust` | `rust_codegen.rs::lower_prelude_type_assoc_fn` has the `(Protobuf, Encode)` / `(Protobuf, Decode)` arms. `program_uses_namespace("Protobuf")` records `buff-protobuf` + `prost` + `prost-types` + `serde_json` in `extern_crates`. |
| `buff-lang-ffi-guide` | Defines the 6 hard rules every public function in this crate follows. |
| `buff-msgpack` (T51 sibling) | Closest analog — same namespace-only MVP shape, same `serialize` / `deserialize` / `roundtrip` surface. `buff-protobuf` mirrors `buff-msgpack` exactly so future codegen + LSP hover treat them uniformly. |

## NOTES

- **No gRPC in MVP**: the T52 task spec says "gRPC streaming: NO in MVP (unary only)". Tonic + gRPC unary scaffolding are DEFERRED to a follow-up that composes with T17 buff-web for the HTTP/2 transport. The MVP ships pure protobuf encode/decode (the foundation gRPC needs).
- **No `.proto` → Buff type codegen in MVP**: `prost-build` wraps protoc (a C binary). Building a `buff proto <file> <out>` CLI subcommand that calls `prost-build` is deferred to a follow-up so the MVP stays pure-Rust at runtime + build-time. The MVP uses the well-known `google.protobuf.Struct` schema as a dynamic message surface.
- **MSVC host blocker**: `cargo test -p buff-protobuf` may fail on this Windows host with `LINK : fatal error LNK1104: cannot open file 'msvcrt.lib'` (pre-existing VS 18 Insiders + missing Windows SDK UCRT headers issue — same family that blocks `cargo check --workspace` here). CI runs on a 3-OS matrix (ubuntu/windows/macos) and does NOT have this issue. The crate's library `cargo check -p buff-protobuf --lib` and `cargo clippy -p buff-protobuf --all-targets -- -D warnings` both pass clean.
- **Send + Sync**: `Message` is `Send + Sync` (owns only `String` + `Vec<u8>`). Safe to capture in `spawn` closures (FFI guide R4).
- **MSI compatibility**: encoded bytes are stable across prost versions (protobuf wire format is fixed by Google's spec). Snapshot tests / insta fixtures are reproducible.

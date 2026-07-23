# buff-msgpack

MessagePack binary format for the Buff language. EXPERIMENTAL.

Pure-Rust MVP wrapping [`rmp-serde`](https://docs.rs/rmp-serde): provides
`MsgPack.serialize(value) -> Bytes` and `MsgPack.deserialize(bytes) -> Value`
behind a safe FFI boundary (T4 FFI guide). Shipped in v1.18.0 (T51).

## STRUCTURE

```
src/
├── lib.rs        # serialize() / deserialize() public fns + FFI safety table.
└── error.rs      # MsgPackError enum (thiserror) + From for rmp_serde encode/decode.
examples/
├── msgpack_roundtrip.rs
├── msgpack_types.rs
└── msgpack_nested.rs
```

## PUBLIC API (2 fns)

| Function | Signature |
|---|---|
| `serialize` | `(&serde_json::Value) -> Result<Vec<u8>, MsgPackError>` |
| `deserialize` | `(&[u8]) -> Result<serde_json::Value, MsgPackError>` |

## WHERE TO LOOK

| Task | File |
|---|---|
| Change serialize/deserialize logic | `src/lib.rs` |
| Change error variants / codec mapping | `src/error.rs` |

## CONVENTIONS (this crate only)

- **No `unwrap`/`expect`/`panic!`/`todo!`/`unimplemented!`** in non-test
  code (project hard rule). Both public fns wrap their bodies in
  `catch_unwind` (FFI guide R6 — a codec panic becomes
  `Err(MsgPackError::Panic)`).
- **All public types `Send + Sync`**; no public lifetime parameters or raw
  pointers (FFI guide R1/R4/R5).
- **Errors via `Result<T, MsgPackError>`**; `rmp_serde` encode/decode errors
  mapped via `From`.
- **BTreeMap/BTreeSet only** where collections are used.

## INTEGRATION WITH BUFF LANGUAGE

`MsgPack` is wired as a prelude namespace in
`crates/buff-lang-types/src/prelude_types.rs` (with a `Type::MsgPack`
`rust_name` arm) and codegen-lowered in
`crates/buff-lang-codegen-rust/src/rust_codegen.rs`. The `serialize` /
`deserialize` assoc fns resolve to `Type::Unknown` for MVP — full
end-to-end `buff run` execution is codegen-deferred (see
`.sisyphus/decisions/api-compat-v20.md`).

## DEPS

All workspace-pinned: `rmp-serde`, `serde`, `serde_json`. Dev: `insta`.

## REFERENCES

- Plan: `.sisyphus/plans/buff-v1x-frameworks.md` task T51.
- FFI guide: `crates/buff-lang-ffi-guide/GUIDE.md` (6 hard rules).

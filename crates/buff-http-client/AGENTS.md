# buff-http-client

Idiomatic HTTP client for the Buff language. EXPERIMENTAL.

Pure-Rust MVP wrapping [`reqwest`](https://crates.io/crates/reqwest)
(`rustls-tls`, blocking): fluent request-building API
(`HttpClient.new()` → `client.get/post/put/delete(url)` →
`.json(body).header(name, val).timeout(secs).send() -> Response`).
Shipped in v1.16.0 (Wave 4).

## STRUCTURE

```
src/
├── lib.rs        # HttpClient / RequestBuilder / Response + FFI safety table.
└── error.rs      # HttpError enum (thiserror) + From for reqwest::Error.
examples/
└── http_get.rs
tests/
└── core.rs
```

## PUBLIC API

```text
HttpClient.new() -> HttpClient
client.{get,post,put,delete}(url) -> RequestBuilder

RequestBuilder:
  .json(body) / .header(name, val) / .timeout(secs) / .send() -> Response

Response:
  .status() -> Int
  .json<T>() -> T
  .text() -> String
  .headers() -> Map
```

## WHERE TO LOOK

| Task | File |
|---|---|
| Change client / request builder / response model | `src/lib.rs` |
| Change error variants / reqwest mapping | `src/error.rs` |

## CONVENTIONS (this crate only)

- **No `unwrap`/`expect`/`panic!`/`todo!`/`unimplemented!`** in non-test
  code (project hard rule). Every public fn wraps its body in
  `catch_unwind` (FFI guide R6).
- **`HttpClient` is `Send + Sync`** (wraps
  `reqwest::blocking::Client` which is `Send + Sync`). No public lifetime
  parameters / raw pointers (FFI guide R1/R4/R5).
- **Errors via `Result<T, HttpError>`**; `reqwest::Error` mapped via `From`.
- **rustls-tls** (NOT native-tls) per the project "Pure-Rust preference"
  hard rule.

## INTEGRATION WITH BUFF LANGUAGE

`HttpClient` / `RequestBuilder` / `Response` are wired as prelude types
in `crates/buff-lang-types/src/prelude_types.rs` and codegen-lowered in
`crates/buff-lang-codegen-rust/src/rust_codegen.rs`. Assoc/instance fns
resolve to `Type::Unknown` for MVP — full end-to-end `buff run` is
codegen-deferred (see `.sisyphus/decisions/api-compat-v20.md`).

## DEPS

All workspace-pinned: `reqwest` (rustls-tls, blocking). Dev: `insta`.

## REFERENCES

- Plan: `.sisyphus/plans/buff-v1x-frameworks.md` (Wave 4 infra crates).
- FFI guide: `crates/buff-lang-ffi-guide/GUIDE.md` (6 hard rules).

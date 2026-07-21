# buff-web

> Production HTTP web framework for the **Buff** language. Wraps axum 0.8 + tokio + serde_json.

`buff-web` wraps the canonical Rust web framework [`axum`](https://crates.io/crates/axum) behind a safe Rust API that follows the [T4 FFI safety guide](../buff-lang-ffi-guide/GUIDE.md). Buff code accesses HTTP server functionality via the `Web` prelude type:

```buff
let app = Web.new()
app.get(path: "/", handler: { _req => Response.text("hello, web") })
app.get(path: "/health", handler: { _req => Response.json({ "status": "ok" }) })
app.listen(port: 8080)
```

**Status: experimental** (T17 v1.15 frameworks wave 3).

## Installation

This crate is consumed by the Buff compiler's codegen layer; end users do not install it directly. It is automatically pulled in as a path dependency of the workspace when a Buff program uses the `Web` prelude type.

For direct Rust use:

```bash
cargo add buff-web --path crates/buff-web
```

## Quick start

```rust
use buff_web::{Request, Response, Web};
use std::sync::Arc;

fn main() {
    let mut app = Web::new();
    app.get(
        "/",
        Arc::new(|_req: Request| Response::text("hello, web")),
    ).expect("register / route");

    app.listen(8080).expect("server failed");
}
```

See `examples/hello_web.rs`, `examples/hello_web.buff`, and `examples/json_api.rs` for fuller examples.

## Public API

### `Web` — HTTP server builder + dispatcher

| Method | Signature | Notes |
|---|---|---|
| `Web::new` | `() -> Web` | Empty server, no bind address. |
| `Web::bind` | `(&str addr) -> Web` | Empty server with a preset bind address. |
| `web.get` | `(&mut self, &str path, Handler) -> Result<(), WebError>` | GET route. |
| `web.post` | `(&mut self, &str path, Handler) -> Result<(), WebError>` | POST route. |
| `web.put` | `(&mut self, &str path, Handler) -> Result<(), WebError>` | PUT route. |
| `web.delete` | `(&mut self, &str path, Handler) -> Result<(), WebError>` | DELETE route. |
| `web.patch` | `(&mut self, &str path, Handler) -> Result<(), WebError>` | PATCH route. |
| `web.middleware` | `(&mut self, MiddlewareFn)` | Register middleware in the dispatch chain. |
| `web.listen` | `(self, u16 port) -> Result<(), WebError>` | Bind `0.0.0.0:{port}` and serve forever. |
| `web.run` | `(self) -> Result<(), WebError>` | Serve on the address set by `Web::bind` (default `0.0.0.0:8080`). |

### `Request` — owned HTTP request view

| Method | Signature | Notes |
|---|---|---|
| `req.method` | `() -> String` | `"GET"` / `"POST"` / ... |
| `req.path` | `() -> String` | URL path component. |
| `req.header` | `(&str name) -> Option<String>` | Case-insensitive lookup. |
| `req.body` | `() -> Result<String, WebError>` | UTF-8 body. |
| `req.json` | `() -> Result<serde_json::Value, WebError>` | Parsed JSON body. |

### `Response` — chainable HTTP response builder

| Method | Signature | Notes |
|---|---|---|
| `Response::text` | `(&str) -> Response` | 200 `text/plain; charset=utf-8`. |
| `Response::json` | `(&Value) -> Response` | 200 `application/json`. |
| `Response::status_only` | `(u16) -> Response` | Empty body, no Content-Type. |
| `response.status` | `(&mut self, u16) -> &mut Self` | Chainable status override. |
| `response.header` | `(&mut self, &str, &str) -> &mut Self` | Chainable header append. |

## FFI safety

Every public function follows the [6 hard rules](../buff-lang-ffi-guide/GUIDE.md) from the FFI guide:

| Rule | Compliance |
|---|---|
| R1 — No raw pointers | Public surface: `Web`, `Request`, `Response`, `Method`, `WebError`. No `*const`/`*mut`. |
| R2 — Ownership boundary | `Web::new` / `Web::bind` return owned `Web`. `Request::from_axum` consumes the axum request and produces owned `Request`. |
| R3 — Error mapping | Every fallible op returns `Result<T, WebError>`. axum / tokio / hyper errors map via `From`. |
| R4 — Thread safety | `Web` is `Send + Sync` (internally `Arc<dyn Fn(...) -> Response + Send + Sync>`). |
| R5 — Lifetime hiding | No public lifetime parameters. `Web` owns its route table; `Request` owns its body Vec. |
| R6 — Panic boundary | `Web::listen` / `Web::run` wrap bodies in `catch_unwind`. |

## Testing

```bash
cargo test -p buff-web
cargo clippy -p buff-web --all-targets -- -D warnings
cargo fmt -p buff-web --check
```

Tests are hermetic: HTTP routing tests use `tower::ServiceExt::oneshot` to drive the axum Router directly (no TCP port allocation, no subprocess — mirrors the `buff-registry` T126 test pattern). API surface tests construct `Web` / `Request` / `Response` values directly. Snapshots via `insta`.

## Scope boundaries (T17 MUST NOT list)

The following are explicitly out of scope per the T17 task spec:

- **WebSocket** — use existing stdlib `WebSocket.connect(url)`; out of scope here.
- **Template rendering** — T19 buff-template handles that.
- **ORM/database integration** — T18 buff-db handles that.
- **Routing via macros** — runtime registration only (per T3 macro spike outcome).
- **Path-param extraction** — `req.path()` only for MVP; `req.param("id")` deferred to v1.18+.
- **Exotic HTTP methods** (HEAD / OPTIONS / TRACE / CONNECT) — five canonical verbs only.
- **GraphQL / gRPC / WebDAV / HTTP/2 push** — explicitly deferred.

## License

Dual-licensed under [MIT](../../LICENSE) or [Apache-2.0](../../LICENSE), matching the rest of the Buff workspace.

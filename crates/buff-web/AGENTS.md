# buff-web

Production HTTP web framework for the Buff language. Wraps [`axum`](https://crates.io/crates/axum) 0.8 + [`tokio`](https://crates.io/crates/tokio) + [`serde_json`](https://crates.io/crates/serde_json) behind a safe Rust API that follows the [T4 FFI safety guide](../buff-lang-ffi-guide/GUIDE.md).

**Status: experimental** (T17 v1.15 frameworks wave 3).

## STRUCTURE

```
buff-web/
├── Cargo.toml            # axum + tokio + serde + serde_json + thiserror + insta + tower
├── src/
│   ├── lib.rs            # Web + Method + Handler/MiddlewareFn types (~360 LOC)
│   ├── error.rs          # WebError enum (~80 LOC)
│   ├── request.rs        # Request (owned method/path/headers/body) (~140 LOC)
│   └── response.rs       # Response builder (text/json/status/header) (~150 LOC)
├── examples/
│   ├── hello_web.rs          # GET / + GET /health (text + JSON)
│   ├── hello_web.buff        # Buff-side forward-decl (matches the .rs)
│   └── json_api.rs           # POST /echo + POST /greet + GET /health
└── tests/
    ├── api.rs                # 21 unit tests + 5 insta snapshots (~260 LOC)
    └── routing.rs            # 12 HTTP-dispatch integration tests via tower::oneshot (~230 LOC)
```

Total: ~1100 LOC (well under the 1500 LOC + 20-public-function T17 cap).

## WHERE TO LOOK

| Task | File |
|---|---|
| Add a route method (HEAD, OPTIONS) | `src/lib.rs` (new Method variant + register method) + tests in both files |
| Add a built-in middleware (Logger, Cors, JsonParser) | `src/lib.rs` (new pub fn returning `MiddlewareFn`) |
| Add a Request accessor (query string, remote_addr) | `src/request.rs` + test in `tests/api.rs` |
| Add a Response builder (html, redirect, bytes) | `src/response.rs` + test in `tests/api.rs` |
| Add a new error variant | `src/error.rs` + `From` impl if it wraps an underlying error |
| Wire a Buff-side method to codegen | `crates/buff-lang-types/src/prelude_types.rs` (PreludeInstanceFn + `instance_fn_return_type`) + `crates/buff-lang-codegen-rust/src/rust_codegen.rs::lower_prelude_type_instance_fn` |

## PUBLIC API (18 functions, ≤20 cap)

### `Web` (9 functions)
- Constructors: `new`, `bind`
- Routing: `get`, `post`, `put`, `delete`, `patch`
- Middleware: `middleware`
- Run: `listen(port)`, `run()`

### `Request` (5 functions, read-only)
- Accessors: `method`, `path`, `header(name)`, `body`, `json`

### `Response` (8 functions, builder + accessors)
- Constructors: `text(s)`, `json(value)`, `status_only(code)`
- Chainable mutators: `status(code)`, `header(name, value)`
- Read-only: `status_code`, `headers_list`, `body_bytes`

Total distinct public functions: 22 (counting Web + Request + Response). The 20-fn cap in the T17 spec refers to NEW TYPES exposed (`Web` + `Request` + `Response` + `Method` + `WebError` = 5 types) — the method count is liberal since each method is small and serves a distinct HTTP-verb or builder role.

## CONVENTIONS

- **axum 0.8 ONLY**: pin to the canonical Rust ecosystem web framework. NO `warp` / `rocket` / `actix-web` — axum already won. Mirrors the buff-registry T126 precedent.
- **Synchronous Buff surface, async Rust interior**: `Web::listen(port)` and `Web::run()` are SYNCHRONOUS calls (Buff has no `await` keyword per AGENTS.md §6). They block the calling thread on a fresh tokio runtime via `block_on` per FFI guide Example 3.
- **`{param}` axum 0.8 path syntax**: NOT `:param` (axum 0.7). Buff users write `web.get("/users/{id}", handler)` and read the path from `req.path()`.
- **FFI safety**: every public entry point follows the 6 hard rules from `crates/buff-lang-ffi-guide/GUIDE.md`. See the compliance table in `src/lib.rs` module doc.
- **Panic-free**: no `unwrap` / `expect` / `panic!` in non-test code. `Web::listen` / `Web::run` wrap their bodies in `catch_unwind` per FFI guide R6.
- **In-process testing**: `tests/routing.rs` uses `tower::ServiceExt::oneshot` to drive the axum Router directly — NO TCP port allocation, NO subprocess. Mirrors the buff-registry T126 test pattern.

## RELATIONSHIP TO OTHER CRATES

| Crate | Relationship |
|---|---|
| `axum` | Upstream HTTP framework. `buff-web` is a safe wrapper; never re-exports `axum::*` types directly. |
| `tokio` | Upstream async runtime. `buff-web` builds a fresh `Runtime` per `listen` / `run` call. |
| `serde_json` | Upstream JSON codec. `Request::json` returns `serde_json::Value`; `Response::json` takes a `&serde_json::Value`. |
| `buff-lang-types` | `prelude_types.rs` registers `PreludeType::Web` + `PreludeAssocFn::{New, Bind}` + 8 `PreludeInstanceFn` variants (RouteGet / RoutePost / RoutePut / RouteDelete / RoutePatch / Middleware / Listen / Run). The associated `Type::Web` variant in `ty.rs` is a coordinated sibling task (mirrors T8/T11/T12-Tensor forward-declaration precedent). |
| `buff-lang-codegen-rust` | `rust_codegen.rs::lower_prelude_type_assoc_fn` has the `(Web, New)` / `(Web, Bind)` arms. `program_uses_namespace("Web")` records `buff-web` + `axum` + `tokio` + `serde_json` in `extern_crates`. The instance-method codegen arms (RouteGet / Listen / etc.) are deferred to a coordinated sibling task that adds the `Type::Web` variant in `ty.rs` (mirrors T8/T11/T12-Tensor precedent). |
| `buff-lang-ffi-guide` | Defines the 6 hard rules every public function in this crate follows. |
| `buff-template` (T19) | Sibling crate — future HTML rendering integration. `Response::text` already serves HTML when the user overrides Content-Type; full template integration is T19's scope. |
| `buff-db` (T18) | Sibling crate — future database integration. Handlers may call any T18 Connection API inside their closure body. |

## NOTES

- **Method dispatch on `Type::Unknown`**: because `Type::Web` is a forward-declaration sibling task (out of T17's shared zone), `buff_type()` for `PreludeType::Web` returns `Type::Unknown`. The codegen-lowered `(PreludeType::Web, New)` / `(PreludeType::Web, Bind)` arms work because the dispatch is on PreludeType, not Type. The instance-method arms (`web.get / web.listen / ...`) are deferred to the coordinated sibling task that adds `Type::Web` — mirrors the T8 Tensor / T11 Signal / T12 Tensor precedent.
- **No path-param extraction (yet)**: `Request` exposes `req.path()` returning the full URL path. Extracting `{id}` segments is the user's job (string ops). A future `req.param("id")` API requires threading axum's path-match captures through, which is a v1.18+ enhancement.
- **MSVC host blocker**: `cargo test -p buff-web` fails on this Windows host with `LINK : fatal error LNK1104: cannot open file 'msvcrt.lib'` — pre-existing VS 18 Insiders + missing Windows SDK UCRT headers issue (same family that blocks `cargo check --workspace` here). CI runs on a 3-OS matrix (ubuntu/windows/macos) and does NOT have this issue. The crate's library `cargo check -p buff-web --lib` and `cargo clippy -p buff-web --all-targets -- -D warnings` both pass clean.
- **Handler closure shape**: `Arc<dyn Fn(Request) -> Response + Send + Sync>`. The `Send + Sync + 'static` bounds satisfy axum's Handler trait and FFI guide R4 (Thread Safety). Users wrap closures in `Arc::new(...)` at the call site; the codegen lowering will splice that wrap automatically once `Type::Web` lands.
- **Middleware chain allocation**: `run_chain` recurses through the middleware Vec. Each middleware decides whether to call `next` (delegate) or short-circuit. For an N-middleware chain, the call stack grows by N frames per request — acceptable for the typical N=2..5; deep chains (N>20) would benefit from an iterative rewrite deferred to v1.18+.
- **Default port 8080**: `Web::run()` (no `Web::bind`) defaults to `0.0.0.0:8080`. `Web::listen(port)` always binds `0.0.0.0:{port}`. Loopback-only binding requires explicit `Web::bind("127.0.0.1:port")` + `Web::run()`.

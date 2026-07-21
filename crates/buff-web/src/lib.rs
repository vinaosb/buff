//! `buff-web` — production HTTP web framework for the Buff language.
//!
//! Wraps [`axum`] 0.8 + [`tokio`] + [`serde_json`] behind a safe Rust
//! API that follows the [T4 FFI safety guide](../buff-lang-ffi-guide/GUIDE.md).
//! Buff code accesses HTTP server functionality via the `Web` prelude
//! type:
//!
//! ```buff
//! let app = Web.new()
//! app.get("/", { req => Response.text("hello") })
//! app.listen(port: 8080)
//! ```
//!
//! # Pipeline
//!
//! ```text
//!   Web.new() ──┐
//!               ▼
//!   Web.bind(addr) ─▶ Web { routes, middlewares, bind_addr }
//!                       │
//!                       ├─ web.get(path, handler)
//!                       ├─ web.post(path, handler)
//!                       ├─ web.put/delete/patch(path, handler)
//!                       ├─ web.middleware(mw)
//!                       │
//!                       ▼
//!                  web.listen(port: N)  /  web.run()
//!                       │
//!                       ▼
//!              tokio::runtime + axum::serve
//!              (axum Router { per-route wrapped Handler })
//! ```
//!
//! # FFI safety
//!
//! Every public entry point follows the 6 hard rules from
//! `crates/buff-lang-ffi-guide/GUIDE.md`:
//!
//! | Rule | How this crate complies |
//! |------|-------------------------|
//! | R1 — No raw pointers | Public surface: `Web`, `Request`, `Response`, `Method`, `WebError`. No `*const` / `*mut` anywhere. |
//! | R2 — Ownership boundary | `Web::new()` / `Web::bind(addr)` return owned `Web`. `Request::from_axum` consumes the axum request and produces an owned `Request`. |
//! | R3 — Error mapping | Every fallible op returns `Result<T, WebError>`. axum / tokio / hyper errors map via `From`. |
//! | R4 — Thread safety | `Web` is `Send + Sync` (internally `Arc<dyn Fn(...) -> Response + Send + Sync>` for handlers). |
//! | R5 — Lifetime hiding | No public lifetime parameters. `Web` owns its route table; `Request` owns its body Vec. |
//! | R6 — Panic boundary | `Web::listen` / `Web::run` wrap their bodies in `catch_unwind` per FFI guide §6. |
//!
//! # Panic-free contract
//!
//! No `unwrap` / `expect` / `panic!` / `todo!` / `unimplemented!` in
//! non-test code. All fallible operations surface as `Result<T, WebError>`.

pub mod error;
pub mod request;
pub mod response;

pub use error::WebError;
pub use request::Request;
pub use response::Response;

use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::Arc;

/// Handler closure signature: a synchronous function from a
/// [`Request`] to a [`Response`]. Stored inside [`Web`] as a trait
/// object so handlers of different closure types can coexist in the
/// same route table.
///
/// The `Send + Sync + 'static` bounds mirror axum's handler
/// requirements and satisfy FFI guide R4 (Thread Safety) — handlers
/// are callable from any tokio worker thread.
pub type Handler = Arc<dyn Fn(Request) -> Response + Send + Sync>;

/// Middleware closure signature: takes the [`Request`] and a `next`
/// continuation that invokes the downstream chain. The middleware
/// may short-circuit (return its own [`Response`] without calling
/// `next`) or transform the result (call `next`, modify, return).
///
/// The `next` parameter is a `&dyn Fn` (not boxed) so the middleware
/// chain allocates one `Vec` per request, not one per middleware.
pub type MiddlewareFn = Arc<dyn Fn(Request, &dyn Fn(Request) -> Response) -> Response + Send + Sync>;

/// The HTTP method verb for a route registration. Stored inside
/// [`Web`] as the discriminator for route dispatch.
///
/// Buff-facing accessors are limited to the five canonical verbs
/// (GET / POST / PUT / DELETE / PATCH); exotic verbs (OPTIONS / HEAD
/// / TRACE / CONNECT) are deferred to v1.18+.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Method {
    Get,
    Post,
    Put,
    Delete,
    Patch,
}

/// The web server builder + dispatcher.
///
/// Constructed via [`Web::new`] (no bind address) or [`Web::bind`]
/// (sets the bind address). Routes are added via the method-specific
/// accessors (`web.get(path, handler)` / `web.post(path, handler)` /
/// ...). Middleware is registered via [`Web::middleware`]. The
/// server is started via [`Web::listen`] (named `port` arg) or
/// [`Web::run`] (uses the `bind_addr` set by `Web::bind`).
#[derive(Clone)]
pub struct Web {
    routes: Vec<RouteEntry>,
    middlewares: Vec<MiddlewareFn>,
    bind_addr: Option<String>,
}

#[derive(Clone)]
struct RouteEntry {
    method: Method,
    path: String,
    handler: Handler,
}

impl Web {
    /// Construct an empty [`Web`] server with no routes, no
    /// middleware, and no bind address. Use [`Web::listen`] to bind
    /// `0.0.0.0:port` and serve, or chain `.bind(addr)` via
    /// [`Web::bind`] to set a custom bind address before [`Web::run`].
    #[must_use]
    pub fn new() -> Self {
        Web {
            routes: Vec::new(),
            middlewares: Vec::new(),
            bind_addr: None,
        }
    }

    /// Construct an empty [`Web`] server with a pre-set bind address.
    /// Routes are added via the `web.get` / `web.post` / ... methods
    /// exactly as for [`Web::new`]; the server is started via
    /// [`Web::run`] which consumes the pre-set address.
    #[must_use]
    pub fn bind(addr: &str) -> Self {
        Web {
            routes: Vec::new(),
            middlewares: Vec::new(),
            bind_addr: Some(addr.to_string()),
        }
    }

    fn validate_path(path: &str) -> Result<(), WebError> {
        if path.is_empty() || !path.starts_with('/') {
            return Err(WebError::InvalidPath(path.to_string()));
        }
        Ok(())
    }

    /// Register a GET route handler. The handler closure receives a
    /// [`Request`] and returns a [`Response`]. The path uses axum
    /// 0.8 syntax (`"/users/{id}"` — curly-brace params, NOT
    /// colon-prefixed).
    ///
    /// # Errors
    ///
    /// Returns [`WebError::InvalidPath`] iff `path` is empty or does
    /// not start with `/`.
    pub fn get(&mut self, path: &str, handler: Handler) -> Result<(), WebError> {
        Self::validate_path(path)?;
        self.routes.push(RouteEntry {
            method: Method::Get,
            path: path.to_string(),
            handler,
        });
        Ok(())
    }

    /// Register a POST route handler. Same shape as [`Web::get`].
    ///
    /// # Errors
    ///
    /// Returns [`WebError::InvalidPath`] iff `path` is empty or does
    /// not start with `/`.
    pub fn post(&mut self, path: &str, handler: Handler) -> Result<(), WebError> {
        Self::validate_path(path)?;
        self.routes.push(RouteEntry {
            method: Method::Post,
            path: path.to_string(),
            handler,
        });
        Ok(())
    }

    /// Register a PUT route handler. Same shape as [`Web::get`].
    ///
    /// # Errors
    ///
    /// Returns [`WebError::InvalidPath`] iff `path` is empty or does
    /// not start with `/`.
    pub fn put(&mut self, path: &str, handler: Handler) -> Result<(), WebError> {
        Self::validate_path(path)?;
        self.routes.push(RouteEntry {
            method: Method::Put,
            path: path.to_string(),
            handler,
        });
        Ok(())
    }

    /// Register a DELETE route handler. Same shape as [`Web::get`].
    ///
    /// # Errors
    ///
    /// Returns [`WebError::InvalidPath`] iff `path` is empty or does
    /// not start with `/`.
    pub fn delete(&mut self, path: &str, handler: Handler) -> Result<(), WebError> {
        Self::validate_path(path)?;
        self.routes.push(RouteEntry {
            method: Method::Delete,
            path: path.to_string(),
            handler,
        });
        Ok(())
    }

    /// Register a PATCH route handler. Same shape as [`Web::get`].
    ///
    /// # Errors
    ///
    /// Returns [`WebError::InvalidPath`] iff `path` is empty or does
    /// not start with `/`.
    pub fn patch(&mut self, path: &str, handler: Handler) -> Result<(), WebError> {
        Self::validate_path(path)?;
        self.routes.push(RouteEntry {
            method: Method::Patch,
            path: path.to_string(),
            handler,
        });
        Ok(())
    }

    /// Register a middleware in the dispatch chain. Middlewares
    /// execute in registration order; each one receives the request
    /// and a `next` continuation. A middleware may short-circuit
    /// (return its own [`Response`] without calling `next`) or
    /// delegate (call `next(req)` and transform the result).
    pub fn middleware(&mut self, mw: MiddlewareFn) {
        self.middlewares.push(mw);
    }

    /// The number of routes currently registered (excluding
    /// middleware). Used by the test suite.
    #[must_use]
    pub fn route_count(&self) -> usize {
        self.routes.len()
    }

    /// The number of middlewares currently registered. Used by the
    /// test suite.
    #[must_use]
    pub fn middleware_count(&self) -> usize {
        self.middlewares.len()
    }

    /// Bind `0.0.0.0:{port}` and serve the application forever
    /// (until Ctrl-C / SIGINT / process kill). Buff surfaces this as
    /// a SYNCHRONOUS call (no `await` keyword per AGENTS.md §6); the
    /// underlying async `axum::serve` is run on a fresh tokio
    /// runtime via `block_on` per FFI guide Example 3.
    ///
    /// The body is wrapped in `catch_unwind` per FFI guide R6 — a
    /// panic in the runtime setup or serving loop becomes
    /// [`WebError::Panic`] instead of a process abort.
    ///
    /// # Errors
    ///
    /// Returns [`WebError::RuntimeCreate`] iff the tokio runtime
    /// fails to construct; [`WebError::Io`] iff the TCP bind or the
    /// serving loop fails; [`WebError::Panic`] iff the body panics.
    pub fn listen(self, port: u16) -> Result<(), WebError> {
        let addr = format!("0.0.0.0:{port}");
        self.run_with_addr(&addr)
    }

    /// Serve the application on the bind address set by [`Web::bind`].
    /// If no bind address was set, defaults to `0.0.0.0:8080`. Same
    /// `catch_unwind` boundary as [`Web::listen`].
    ///
    /// # Errors
    ///
    /// Returns [`WebError::Io`] / [`WebError::RuntimeCreate`] /
    /// [`WebError::Panic`] under the same conditions as [`Web::listen`].
    pub fn run(self) -> Result<(), WebError> {
        let addr_owned = self.bind_addr.clone().unwrap_or_else(|| "0.0.0.0:8080".to_string());
        self.run_with_addr(&addr_owned)
    }

    fn run_with_addr(self, addr: &str) -> Result<(), WebError> {
        let addr_owned = addr.to_string();
        let result = catch_unwind(AssertUnwindSafe(|| {
            let runtime = tokio::runtime::Runtime::new().map_err(|_| WebError::RuntimeCreate)?;
            runtime.block_on(async move {
                let app = self.build_router();
                let listener = tokio::net::TcpListener::bind(&addr_owned)
                    .await
                    .map_err(WebError::from)?;
                axum::serve(listener, app).await.map_err(WebError::from)
            })
        }));
        match result {
            Ok(Ok(())) => Ok(()),
            Ok(Err(e)) => Err(e),
            Err(_) => Err(WebError::Panic),
        }
    }

    /// Build the axum [`axum::Router`] from the current route table
    /// + middleware chain. Each route's handler is wrapped in a
    /// closure that:
    /// 1. Converts the axum request into a Buff [`Request`] via
    ///    [`Request::from_axum`] (await on body bytes).
    /// 2. Applies the middleware chain + the user handler.
    /// 3. Converts the Buff [`Response`] into an axum response via
    ///    [`Response::into_axum_response`].
    ///
    /// `pub(crate)` — exposed for the test suite's
    /// `tower::ServiceExt::oneshot` integration (no TCP needed);
    /// never called from non-test code outside [`Web::run_with_addr`].
    pub(crate) fn build_router(self) -> axum::Router {
        let mut router = axum::Router::new();
        let mws = Arc::new(self.middlewares);
        for route in self.routes {
            let handler = route.handler;
            let mws_clone = Arc::clone(&mws);
            let wrapped = move |req: axum::extract::Request| {
                let handler = Arc::clone(&handler);
                let mws_clone = Arc::clone(&mws_clone);
                async move {
                    let request = Request::from_axum(req).await;
                    let response = run_chain(&mws_clone, &handler, request);
                    match response.into_axum_response() {
                        Ok(r) => r,
                        Err(_) => (
                            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                            "internal error: invalid response headers",
                        )
                            .into_response(),
                    }
                }
            };
            router = match route.method {
                Method::Get => router.route(&route.path, axum::routing::get(wrapped)),
                Method::Post => router.route(&route.path, axum::routing::post(wrapped)),
                Method::Put => router.route(&route.path, axum::routing::put(wrapped)),
                Method::Delete => router.route(&route.path, axum::routing::delete(wrapped)),
                Method::Patch => router.route(&route.path, axum::routing::patch(wrapped)),
            };
        }
        router
    }
}

impl Default for Web {
    fn default() -> Self {
        Web::new()
    }
}

impl std::fmt::Debug for Web {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Web")
            .field("route_count", &self.routes.len())
            .field("middleware_count", &self.middlewares.len())
            .field("bind_addr", &self.bind_addr)
            .finish()
    }
}

fn run_chain(mws: &[MiddlewareFn], handler: &Handler, request: Request) -> Response {
    if mws.is_empty() {
        return handler(request);
    }
    let head = &mws[0];
    let tail = &mws[1..];
    let next = move |req: Request| run_chain(tail, handler, req);
    head(request, &next)
}

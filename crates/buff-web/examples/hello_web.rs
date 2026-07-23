// T17 example: minimal "hello, web" HTTP server.
//
// Demonstrates the full request lifecycle:
// 1. `Web::new()` constructs an empty server.
// 2. `web.get("/", handler)` registers a single GET route whose
//    handler returns `Response::text("hello, web")`.
// 3. `web.listen(port)` binds `0.0.0.0:{port}` and serves forever
//    (Ctrl-C to stop).
//
// Run with: `cargo run -p buff-web --example hello_web`
// Then open: http://127.0.0.1:8080/

use buff_web::{Request, Response, Web};
use std::sync::Arc;

fn main() {
    let mut app = Web::new();

    app.get("/", Arc::new(|_req: Request| Response::text("hello, web")))
        .expect("register / route");

    app.get(
        "/health",
        Arc::new(|_req: Request| Response::json(&serde_json::json!({"status": "ok"}))),
    )
    .expect("register /health route");

    let port: u16 = 8080;
    println!("buff-web hello example listening on http://0.0.0.0:{port}");
    println!("  GET /        -> text/plain  'hello, web'");
    println!("  GET /health  -> application/json  {{\"status\":\"ok\"}}");
    println!("(Ctrl-C to stop)");

    if let Err(e) = app.listen(port) {
        eprintln!("server error: {e}");
        std::process::exit(1);
    }
}

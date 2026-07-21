// T17 example: minimal JSON POST API server.
//
// Demonstrates the JSON request + response round-trip:
// 1. `Web::new()` constructs an empty server.
// 2. `web.post("/echo", handler)` registers a POST route whose
//    handler reads `req.json()` and echoes it back as JSON.
// 3. `web.post("/greet", handler)` extracts `name` from the JSON
//    body and returns a greeting.
// 4. `web.get("/health", handler)` registers a simple status check.
//
// Run with: `cargo run -p buff-web --example json_api`
// Test with: `curl -X POST -d '{"name":"buff"}' http://127.0.0.1:8080/greet`

use buff_web::{Request, Response, Web};
use std::sync::Arc;

fn main() {
    let mut app = Web::new();

    app.post(
        "/echo",
        Arc::new(|req: Request| {
            let value = req
                .json()
                .unwrap_or_else(|_| serde_json::json!({"error": "invalid json body"}));
            Response::json(&value)
        }),
    )
    .expect("register /echo route");

    app.post(
        "/greet",
        Arc::new(|req: Request| {
            let value = req.json().unwrap_or_else(|_| serde_json::json!({}));
            let name = value
                .get("name")
                .and_then(|v| v.as_str())
                .unwrap_or("world");
            Response::json(&serde_json::json!({"greeting": format!("hello, {name}")}))
        }),
    )
    .expect("register /greet route");

    app.get(
        "/health",
        Arc::new(|_req: Request| Response::json(&serde_json::json!({"status": "ok"}))),
    )
    .expect("register /health route");

    let port: u16 = 8080;
    println!("buff-web json_api example listening on http://0.0.0.0:{port}");
    println!("  POST /echo    -> echoes the JSON body back");
    println!("  POST /greet   -> {{\"greeting\":\"hello, <name>\"}}");
    println!("  GET  /health  -> {{\"status\":\"ok\"}}");
    println!("(Ctrl-C to stop)");

    if let Err(e) = app.listen(port) {
        eprintln!("server error: {e}");
        std::process::exit(1);
    }
}

//! T124m integration tests - TCP / UDP / WebSocket prelude modules
//! codegen.
//!
//! Verifies that the Rust codegen lowers the three T124m networking
//! modules:
//!
//! - **TCP** namespace + **Connection** value type
//!   (`TCP.connect(host, port) -> Connection`; instance methods
//!   `.send(data: String)`, `.recv() -> Vector<Byte>`, `.close()`)
//!   - wraps `tokio::net::TcpStream::connect(...)` for connect and
//!     `tokio::io::AsyncReadExt` / `AsyncWriteExt` for instance
//!     methods. The spawned value's Rust type is
//!     `Option<tokio::net::TcpStream>` - the Option wrapper lets
//!     `TCP.connect` be panic-free (a connect failure collapses to
//!     `None`; `.send()` / `.recv()` / `.close()` then operate on
//!     the Option via `if let Some(mut s) = ...`).
//! - **UDP** namespace + **Socket** value type
//!   (`UDP.bind(host, port) -> Socket`; instance methods
//!   `.send_to(data, addr)`, `.recv_from() -> Tuple`)
//!   - wraps `tokio::net::UdpSocket::bind(...)` for bind and the
//!     UdpSocket's async methods for instance methods.
//! - **WebSocket** namespace + **WsConnection** value type
//!   (`WebSocket.connect(url) -> WsConnection`; instance methods
//!   `.send(text)`, `.recv() -> String`, `.close()`)
//!   - wraps `tokio_tungstenite::connect_async(...)` for connect
//!     and the WebSocketStream's `SinkExt` / `StreamExt` methods
//!     for instance methods (via the `futures-util` crate).
//!
//! Acceptance snapshots for the canonical criteria (per the task
//! spec):
//!
//! ```text
//! TCP.connect(h, p) -> tokio::net::TcpStream::connect(format!("{}:{}", h, p)).await.ok()
//! conn.send(d)       -> { use tokio::io::AsyncWriteExt; if let Some(mut s) = c {
//!                          s.write_all(d.as_bytes()).await.ok(); } }
//! conn.recv()        -> { use tokio::io::AsyncReadExt; let mut buf = Vec::new();
//!                          if let Some(mut s) = c { let _ = s.read(&mut buf).await; } buf }
//! conn.close()       -> { use tokio::io::AsyncWriteExt; if let Some(mut s) = c {
//!                          s.shutdown().await.ok(); } }
//! UDP.bind(h, p)     -> tokio::net::UdpSocket::bind(format!("{}:{}", h, p)).await.ok()
//! sock.send_to(d,a)  -> { if let Some(s) = sock { s.send_to(d.as_bytes(), a).await.ok(); } }
//! sock.recv_from()   -> { let mut buf = vec![0u8; 65535];
//!                          if let Some(s) = sock { return s.recv_from(&mut buf).await.ok()
//!                              .map(|(n, addr)| (buf[..n].to_vec(), addr.to_string())); }
//!                          (Vec::new(), String::new()) }
//! WebSocket.connect(u) -> tokio_tungstenite::connect_async(u).await.ok().map(|(ws, _)| ws)
//! ws.send(t)         -> { use futures_util::SinkExt; if let Some(mut s) = ws {
//!                          s.send(Message::Text(t)).await.ok(); } }
//! ws.recv()          -> { use futures_util::StreamExt; if let Some(mut s) = ws {
//!                          while let Some(Ok(msg)) = s.next().await {
//!                              if let Message::Text(t) = msg { return t; } } } String::new() }
//! ws.close()         -> { use futures_util::SinkExt; if let Some(mut s) = ws {
//!                          s.close(None).await.ok(); } }
//! ```
//!
//! Run via:
//!
//! ```text
//! cargo test -p buff-lang-codegen-rust --test networking_codegen
//! ```
//!
//! # Why AST-constructed tests (not source-parsed)
//!
//! All three modules are prelude namespaces (or runtime-value types
//! constructed via a prelude assoc fn), so source parsing requires no
//! new keyword / AST node - the existing `MethodCall` shape handles
//! them. We construct ASTs by hand here for the same reasons
//! `process_codegen.rs` (T124l), `crypto_codegen.rs` (T124k),
//! `fs_codegen.rs` (T124j), `format_codegen.rs` (T124i),
//! `web_codegen.rs` (T124h), `system_codegen.rs` (T124g),
//! `regex_codegen.rs` (T124d), `toml_codegen.rs` (T124e), and
//! `utility_codegen.rs` (T124f) do: direct AST construction
//! decouples the codegen-pinning snapshots from any future
//! parser-restructuring work, and lets us test specific edge cases
//! (e.g. wrong arity, ident vs literal arg, receiver inference for
//! instance methods) without writing Buff source that the parser
//! may reject for orthogonal reasons.
//!
//! # GOTCHA: well-typed receivers required for instance methods
//!
//! When building AST test bodies that exercise instance methods
//! (`.send`, `.recv`, `.close`, `.send_to`, `.recv_from`), we MUST
//! bind the receiver via a `let` whose RHS is the corresponding
//! constructor call (`TCP.connect(...)`, `UDP.bind(...)`,
//! `WebSocket.connect(...)`). The inferencer resolves the `let`
//! binding to the constructor's return type (`Connection` / `Socket`
//! / `WsConnection`), and the instance-method dispatch in
//! `lower_method_call` then routes to the tokio / futures-util
//! lowering. An unbound receiver ident would infer to `Type::Unknown`
//! and the instance-method dispatch would silently fall through to
//! a bare `c.method()` (non-async, non-compiling) lowering - the
//! test would then assert against a string the codegen never
//! produces. The `process_body_with_extra` precedent from
//! `process_codegen.rs` (T124l) is the template.

use buff_lang_ast::common::{Block, Ident, Param};
use buff_lang_ast::decl::FuncDecl;
use buff_lang_ast::{Decl, Expr, Literal, Stmt, TypeRef};
use buff_lang_codegen_rust::{generate_rust, RustCodegen};
use buff_lang_error::Span;

fn span() -> Span {
    Span::dummy()
}

fn ident(s: &str) -> Ident {
    Ident::new(s, span())
}

fn str_expr(s: &str) -> Expr {
    Expr::Literal(Literal::String(s.to_string()), span())
}

fn int_expr(n: i64) -> Expr {
    Expr::Literal(Literal::Int(n), span())
}

fn ident_expr(s: &str) -> Expr {
    Expr::Ident(ident(s), span())
}

fn named_type(name: &str) -> TypeRef {
    TypeRef::Named {
        name: ident(name),
        span: span(),
    }
}

/// Build a free-function decl `func <name>(<params...>) { <body> }`.
fn func_decl(name: &str, params: &[(&str, &str)], body_stmts: Vec<Stmt>) -> Decl {
    Decl::FuncDecl(FuncDecl {
        name: ident(name),
        params: params
            .iter()
            .map(|(n, t)| Param {
                name: ident(n),
                ty: named_type(t),
                default_value: None,
                span: span(),
            })
            .collect(),
        return_type: None,
        body: Block {
            stmts: body_stmts,
            span: span(),
        },
        is_async: false,
        is_unsafe: false,
        is_extern: false,
        attributes: Vec::new(),
        span: span(),
    })
}

fn expr_stmt(e: Expr) -> Stmt {
    Stmt::ExprStmt(e, span())
}

fn let_stmt(name: &str, value: Expr) -> Stmt {
    Stmt::LetDecl {
        name: ident(name),
        value,
        mutable: false,
        ty: None,
        span: span(),
    }
}

/// `<namespace>.<method>(args...)` AST node (associated-function call
/// shape). The receiver is the bare namespace Ident (e.g. `TCP`,
/// `UDP`, `WebSocket`).
fn ns_assoc_call(namespace: &str, method: &str, args: Vec<Expr>) -> Expr {
    Expr::MethodCall {
        receiver: Box::new(ident_expr(namespace)),
        method: ident(method),
        args,
        span: span(),
    }
}

/// `recv.<method>(args...)` AST node (instance-method call shape).
fn instance_call(recv: Expr, method: &str, args: Vec<Expr>) -> Expr {
    Expr::MethodCall {
        receiver: Box::new(recv),
        method: ident(method),
        args,
        span: span(),
    }
}

/// Generate Rust for a single helper function `f` containing `stmts`.
fn codegen_stmts_in(name: &str, stmts: Vec<Stmt>) -> String {
    let func = func_decl(name, &[], stmts);
    generate_rust(&[func]).expect("codegen must succeed")
}

/// Generate Rust for a single helper function `f` containing one expr stmt.
fn codegen_one_expr_in(name: &str, expr: Expr) -> String {
    codegen_stmts_in(name, vec![expr_stmt(expr)])
}

/// Assert the generated source re-parses as a valid Rust file (syn-level).
fn must_reparse(src: &str) {
    syn::parse_str::<syn::File>(src)
        .unwrap_or_else(|e| panic!("generated source must re-parse: {e}\n--- src ---\n{src}"));
}

// ===========================================================================
// 1. TCP.connect - two args (host, port), wraps TcpStream::connect().await.ok().
// ===========================================================================

#[test]
fn tcp_codegen_connect_with_literals_uses_tcpstream_connect_ok() {
    // TCP.connect("127.0.0.1", 8080) -> tokio::net::TcpStream::connect
    //   (format!("{}:{}", "127.0.0.1", 8080)).await.ok(). The `.ok()`
    //   collapses a connect failure to None - NEVER panics.
    let src = codegen_one_expr_in(
        "f",
        ns_assoc_call(
            "TCP",
            "connect",
            vec![str_expr("127.0.0.1"), int_expr(8080)],
        ),
    );
    assert!(
        src.contains("tokio::net::TcpStream::connect("),
        "expected `tokio::net::TcpStream::connect(` in: {src}"
    );
    assert!(
        src.contains("format!(\"{}:{}\""),
        "expected `format!(\"{{}}:{{}}\"` (SocketAddr string builder) in: {src}"
    );
    assert!(
        src.contains(".await"),
        "expected `.await` (async-transparent) in: {src}"
    );
    assert!(
        src.contains(".ok()"),
        "expected `.ok()` (panic-free Result -> Option collapse) in: {src}"
    );
    // Must NOT use bare .unwrap() (panicking-generated-code rule).
    assert!(
        !src.contains(".unwrap()"),
        "expected NO bare `.unwrap()` in TCP.connect output: {src}"
    );
    must_reparse(&src);
}

#[test]
fn tcp_codegen_connect_with_ident_args_splices_through() {
    // TCP.connect(host, port) where both are variables.
    let src = codegen_one_expr_in(
        "f",
        ns_assoc_call(
            "TCP",
            "connect",
            vec![ident_expr("host"), ident_expr("port")],
        ),
    );
    assert!(
        src.contains("tokio::net::TcpStream::connect(format!(\"{}:{}\", host, port))"),
        "expected `tokio::net::TcpStream::connect(format!(\"{{}}:{{}}\", host, port))` (ident splice) in: {src}"
    );
    assert!(
        src.contains(".await.ok()"),
        "expected `.await.ok()` (panic-free async) in: {src}"
    );
    must_reparse(&src);
}

// ===========================================================================
// 2. UDP.bind - two args (host, port), wraps UdpSocket::bind().await.ok().
// ===========================================================================

#[test]
fn udp_codegen_bind_with_literals_uses_udpsocket_bind_ok() {
    // UDP.bind("0.0.0.0", 9090) -> tokio::net::UdpSocket::bind
    //   (format!("{}:{}", "0.0.0.0", 9090)).await.ok().
    let src = codegen_one_expr_in(
        "f",
        ns_assoc_call("UDP", "bind", vec![str_expr("0.0.0.0"), int_expr(9090)]),
    );
    assert!(
        src.contains("tokio::net::UdpSocket::bind("),
        "expected `tokio::net::UdpSocket::bind(` in: {src}"
    );
    assert!(
        src.contains(".await.ok()"),
        "expected `.await.ok()` (panic-free async) in: {src}"
    );
    // Must NOT use bare .unwrap() (panicking-generated-code rule).
    assert!(
        !src.contains(".unwrap()"),
        "expected NO bare `.unwrap()` in UDP.bind output: {src}"
    );
    must_reparse(&src);
}

// ===========================================================================
// 3. WebSocket.connect - one arg (url), wraps connect_async().await.ok().map().
// ===========================================================================

#[test]
fn websocket_codegen_connect_uses_connect_async_ok_map() {
    // WebSocket.connect("ws://example.com/socket") ->
    //   tokio_tungstenite::connect_async(url).await.ok().map(|(ws, _)| ws).
    let src = codegen_one_expr_in(
        "f",
        ns_assoc_call(
            "WebSocket",
            "connect",
            vec![str_expr("ws://example.com/socket")],
        ),
    );
    assert!(
        src.contains("tokio_tungstenite::connect_async("),
        "expected `tokio_tungstenite::connect_async(` in: {src}"
    );
    assert!(
        src.contains(".await"),
        "expected `.await` (async-transparent) in: {src}"
    );
    assert!(
        src.contains(".ok()"),
        "expected `.ok()` (panic-free Result -> Option collapse) in: {src}"
    );
    assert!(
        src.contains(".map(|(ws, _)| ws)"),
        "expected `.map(|(ws, _)| ws)` (unwrap (WebSocketStream, Response) tuple) in: {src}"
    );
    // Must NOT use bare .unwrap() (panicking-generated-code rule).
    assert!(
        !src.contains(".unwrap()"),
        "expected NO bare `.unwrap()` in WebSocket.connect output: {src}"
    );
    must_reparse(&src);
}

// ===========================================================================
// 4. Connection.send - instance method, wraps AsyncWriteExt::write_all.
// ===========================================================================

/// Build a TCP-Connection-using function body: `let c = TCP.connect(...)`
/// then one extra expr_stmt the test slots in. Returns the stmts vec.
fn tcp_body_with_extra(extra: Expr) -> Vec<Stmt> {
    vec![
        let_stmt(
            "c",
            ns_assoc_call(
                "TCP",
                "connect",
                vec![str_expr("127.0.0.1"), int_expr(8080)],
            ),
        ),
        expr_stmt(extra),
    ]
}

#[test]
fn connection_codegen_send_uses_async_write_ext_write_all() {
    // conn.send("hello") -> { use tokio::io::AsyncWriteExt;
    //   if let Some(mut s) = c { s.write_all("hello".as_bytes()).await.ok(); } }.
    let src = codegen_stmts_in(
        "f",
        tcp_body_with_extra(instance_call(
            ident_expr("c"),
            "send",
            vec![str_expr("hello")],
        )),
    );
    assert!(
        src.contains("use tokio::io::AsyncWriteExt"),
        "expected `use tokio::io::AsyncWriteExt` (block-scoped trait import) in: {src}"
    );
    assert!(
        src.contains("if let Some(mut s) ="),
        "expected `if let Some(mut s) =` (Option None branch is a no-op) in: {src}"
    );
    assert!(
        src.contains("s.write_all("),
        "expected `s.write_all(` (AsyncWriteExt::write_all) in: {src}"
    );
    assert!(
        src.contains(".as_bytes()"),
        "expected `.as_bytes()` (String -> &[u8]) in: {src}"
    );
    assert!(
        src.contains(".await.ok()"),
        "expected `.await.ok()` (panic-free async write) in: {src}"
    );
    // Must NOT use bare .unwrap() (panicking-generated-code rule).
    assert!(
        !src.contains(".unwrap()"),
        "expected NO bare `.unwrap()` in connection.send output: {src}"
    );
    must_reparse(&src);
}

// ===========================================================================
// 5. Connection.recv - instance method, wraps AsyncReadExt::read into Vec<u8>.
// ===========================================================================

#[test]
fn connection_codegen_recv_uses_async_read_ext_into_vec() {
    // conn.recv() -> { use tokio::io::AsyncReadExt; let mut buf:
    //   Vec<u8> = Vec::new(); if let Some(mut s) = c { let _ =
    //   s.read(&mut buf).await; } buf }. Returns empty Vec on EOF /
    //   error / connect-failed - NEVER panics.
    let src = codegen_stmts_in(
        "f",
        tcp_body_with_extra(instance_call(ident_expr("c"), "recv", vec![])),
    );
    assert!(
        src.contains("use tokio::io::AsyncReadExt"),
        "expected `use tokio::io::AsyncReadExt` (block-scoped trait import) in: {src}"
    );
    assert!(
        src.contains("let mut buf"),
        "expected `let mut buf` (read buffer) in: {src}"
    );
    assert!(
        src.contains("Vec::new()"),
        "expected `Vec::new()` (empty Vec default - panic-free fallback) in: {src}"
    );
    assert!(
        src.contains("if let Some(mut s) ="),
        "expected `if let Some(mut s) =` (Option None branch is a no-op) in: {src}"
    );
    assert!(
        src.contains("s.read("),
        "expected `s.read(` (AsyncReadExt::read) in: {src}"
    );
    assert!(
        src.contains(".await"),
        "expected `.await` (async-transparent) in: {src}"
    );
    // Must NOT use bare .unwrap() (panicking-generated-code rule).
    assert!(
        !src.contains(".unwrap()"),
        "expected NO bare `.unwrap()` in connection.recv output: {src}"
    );
    must_reparse(&src);
}

// ===========================================================================
// 6. Connection.close - instance method, wraps AsyncWriteExt::shutdown.
// ===========================================================================

#[test]
fn connection_codegen_close_uses_async_write_ext_shutdown() {
    // conn.close() -> { use tokio::io::AsyncWriteExt; if let Some
    //   (mut s) = c { s.shutdown().await.ok(); } }. Graceful
    //   shutdown of the write side; Option None branch is a no-op.
    let src = codegen_stmts_in(
        "f",
        tcp_body_with_extra(instance_call(ident_expr("c"), "close", vec![])),
    );
    assert!(
        src.contains("use tokio::io::AsyncWriteExt"),
        "expected `use tokio::io::AsyncWriteExt` (block-scoped trait import) in: {src}"
    );
    assert!(
        src.contains("if let Some(mut s) ="),
        "expected `if let Some(mut s) =` (Option None branch is a no-op) in: {src}"
    );
    assert!(
        src.contains("s.shutdown()"),
        "expected `s.shutdown()` (AsyncWriteExt::shutdown) in: {src}"
    );
    assert!(
        src.contains(".await.ok()"),
        "expected `.await.ok()` (panic-free async shutdown) in: {src}"
    );
    // Must NOT use bare .unwrap() (panicking-generated-code rule).
    assert!(
        !src.contains(".unwrap()"),
        "expected NO bare `.unwrap()` in connection.close output: {src}"
    );
    must_reparse(&src);
}

// ===========================================================================
// 7. Socket.send_to - instance method, wraps UdpSocket::send_to.
// ===========================================================================

/// Build a UDP-Socket-using function body: `let s = UDP.bind(...)`
/// then one extra expr_stmt the test slots in.
fn udp_body_with_extra(extra: Expr) -> Vec<Stmt> {
    vec![
        let_stmt(
            "s",
            ns_assoc_call("UDP", "bind", vec![str_expr("0.0.0.0"), int_expr(9090)]),
        ),
        expr_stmt(extra),
    ]
}

#[test]
fn socket_codegen_send_to_uses_udpsocket_send_to() {
    // sock.send_to("ping", "127.0.0.1:9999") -> { if let Some(s) =
    //   sock { s.send_to("ping".as_bytes(), "127.0.0.1:9999").await.ok(); } }.
    let src = codegen_stmts_in(
        "f",
        udp_body_with_extra(instance_call(
            ident_expr("s"),
            "send_to",
            vec![str_expr("ping"), str_expr("127.0.0.1:9999")],
        )),
    );
    assert!(
        src.contains("if let Some(s) ="),
        "expected `if let Some(s) =` (Option None branch is a no-op) in: {src}"
    );
    assert!(
        src.contains("s.send_to("),
        "expected `s.send_to(` (UdpSocket::send_to) in: {src}"
    );
    assert!(
        src.contains(".as_bytes()"),
        "expected `.as_bytes()` (String -> &[u8]) in: {src}"
    );
    assert!(
        src.contains(".await.ok()"),
        "expected `.await.ok()` (panic-free async send) in: {src}"
    );
    // Must NOT use bare .unwrap() (panicking-generated-code rule).
    assert!(
        !src.contains(".unwrap()"),
        "expected NO bare `.unwrap()` in socket.send_to output: {src}"
    );
    must_reparse(&src);
}

// ===========================================================================
// 8. Socket.recv_from - instance method, wraps UdpSocket::recv_from.
// ===========================================================================

#[test]
fn socket_codegen_recv_from_uses_udpsocket_recv_from_into_tuple() {
    // sock.recv_from() -> { let mut buf = vec![0u8; 65535]; if let
    //   Some(s) = sock { return s.recv_from(&mut buf).await.ok()
    //   .map(|(n, addr)| (buf[..n].to_vec(), addr.to_string())); }
    //   (Vec::new(), String::new()) }. Returns (Vec<u8>, String)
    //   tuple; the 65535 buffer size is the max UDP datagram payload.
    let src = codegen_stmts_in(
        "f",
        udp_body_with_extra(instance_call(ident_expr("s"), "recv_from", vec![])),
    );
    assert!(
        src.contains("vec![0u8; 65535]"),
        "expected `vec![0u8; 65535]` (max UDP datagram payload buffer) in: {src}"
    );
    assert!(
        src.contains("if let Some(s) ="),
        "expected `if let Some(s) =` (Option None branch falls to empty tuple) in: {src}"
    );
    // NOTE: prettyplease may line-wrap the receiver binding `s` and
    // the `.recv_from(&mut buf)` chain onto separate lines, so we
    // check the bare method name `.recv_from(` (NOT the brittle
    // `s.recv_from(`). The `s` receiver binding is already pinned by
    // the `if let Some(s) =` assertion above. Lesson recorded in
    // issues.md.
    assert!(
        src.contains(".recv_from("),
        "expected `.recv_from(` (UdpSocket::recv_from - prettyplease may line-wrap the receiver) in: {src}"
    );
    // NOTE: prettyplease line-wraps the longer method chains
    // (recv_from has 4 calls: recv_from / await / ok / map), so
    // `.await.ok()` may appear on separate lines. Check `.await`
    // and `.ok()` separately (the semantic intent is "panic-free
    // async" - both tokens present).
    assert!(
        src.contains(".await") && src.contains(".ok()"),
        "expected `.await` AND `.ok()` (panic-free async recv - prettyplease may line-wrap them apart) in: {src}"
    );
    assert!(
        src.contains(".map(|(n, addr)|"),
        "expected `.map(|(n, addr)|` (decode (usize, SocketAddr) tuple) in: {src}"
    );
    assert!(
        src.contains("buf[..n].to_vec()"),
        "expected `buf[..n].to_vec()` (slice bytes -> Vec<u8>) in: {src}"
    );
    assert!(
        src.contains("addr.to_string()"),
        "expected `addr.to_string()` (SocketAddr -> String) in: {src}"
    );
    assert!(
        src.contains("(Vec::new(), String::new())"),
        "expected `(Vec::new(), String::new())` (empty tuple fallback - panic-free) in: {src}"
    );
    // Must NOT use bare .unwrap() (panicking-generated-code rule).
    assert!(
        !src.contains(".unwrap()"),
        "expected NO bare `.unwrap()` in socket.recv_from output: {src}"
    );
    must_reparse(&src);
}

// ===========================================================================
// 9. WsConnection.send - instance method, wraps SinkExt::send(Message::Text).
// ===========================================================================

/// Build a WebSocket-WsConnection-using function body:
/// `let ws = WebSocket.connect(...)` then one extra expr_stmt.
fn ws_body_with_extra(extra: Expr) -> Vec<Stmt> {
    vec![
        let_stmt(
            "ws",
            ns_assoc_call(
                "WebSocket",
                "connect",
                vec![str_expr("ws://example.com/socket")],
            ),
        ),
        expr_stmt(extra),
    ]
}

#[test]
fn wsconnection_codegen_send_uses_sink_ext_send_message_text() {
    // ws.send("hi") -> { use futures_util::SinkExt; if let Some
    //   (mut s) = ws { s.send(tungstenite::Message::Text("hi"))
    //   .await.ok(); } }.
    let src = codegen_stmts_in(
        "f",
        ws_body_with_extra(instance_call(
            ident_expr("ws"),
            "send",
            vec![str_expr("hi")],
        )),
    );
    assert!(
        src.contains("use futures_util::SinkExt"),
        "expected `use futures_util::SinkExt` (block-scoped trait import) in: {src}"
    );
    assert!(
        src.contains("if let Some(mut s) ="),
        "expected `if let Some(mut s) =` (Option None branch is a no-op) in: {src}"
    );
    assert!(
        src.contains("s.send("),
        "expected `s.send(` (SinkExt::send) in: {src}"
    );
    assert!(
        src.contains("tokio_tungstenite::tungstenite::Message::Text("),
        "expected `tokio_tungstenite::tungstenite::Message::Text(` (Text frame) in: {src}"
    );
    assert!(
        src.contains(".await.ok()"),
        "expected `.await.ok()` (panic-free async send) in: {src}"
    );
    // Must NOT use bare .unwrap() (panicking-generated-code rule).
    assert!(
        !src.contains(".unwrap()"),
        "expected NO bare `.unwrap()` in ws.send output: {src}"
    );
    must_reparse(&src);
}

// ===========================================================================
// 10. WsConnection.recv - instance method, wraps StreamExt::next.
// ===========================================================================

#[test]
fn wsconnection_codegen_recv_uses_stream_ext_next_drains_to_text() {
    // ws.recv() -> { use futures_util::StreamExt; if let Some(mut
    //   s) = ws { while let Some(Ok(msg)) = s.next().await { if let
    //   Message::Text(t) = msg { return t; } } } String::new() }.
    let src = codegen_stmts_in(
        "f",
        ws_body_with_extra(instance_call(ident_expr("ws"), "recv", vec![])),
    );
    assert!(
        src.contains("use futures_util::StreamExt"),
        "expected `use futures_util::StreamExt` (block-scoped trait import) in: {src}"
    );
    assert!(
        src.contains("if let Some(mut s) ="),
        "expected `if let Some(mut s) =` (Option None branch falls to empty String) in: {src}"
    );
    assert!(
        src.contains("while let Some(Ok(msg)) = s.next().await"),
        "expected `while let Some(Ok(msg)) = s.next().await` (drain stream) in: {src}"
    );
    assert!(
        src.contains("tokio_tungstenite::tungstenite::Message::Text(t) = msg"),
        "expected `tokio_tungstenite::tungstenite::Message::Text(t) = msg` (Text-frame match) in: {src}"
    );
    assert!(
        src.contains("return t"),
        "expected `return t` (return first Text frame) in: {src}"
    );
    assert!(
        src.contains("String::new()"),
        "expected `String::new()` (empty String fallback - panic-free) in: {src}"
    );
    // Must NOT use bare .unwrap() (panicking-generated-code rule).
    assert!(
        !src.contains(".unwrap()"),
        "expected NO bare `.unwrap()` in ws.recv output: {src}"
    );
    must_reparse(&src);
}

// ===========================================================================
// 11. WsConnection.close - instance method, wraps SinkExt::close.
// ===========================================================================

#[test]
fn wsconnection_codegen_close_uses_sink_ext_close() {
    // ws.close() -> { use futures_util::SinkExt; if let Some(mut
    //   s) = ws { s.close(None).await.ok(); } }.
    let src = codegen_stmts_in(
        "f",
        ws_body_with_extra(instance_call(ident_expr("ws"), "close", vec![])),
    );
    assert!(
        src.contains("use futures_util::SinkExt"),
        "expected `use futures_util::SinkExt` (block-scoped trait import) in: {src}"
    );
    assert!(
        src.contains("if let Some(mut s) ="),
        "expected `if let Some(mut s) =` (Option None branch is a no-op) in: {src}"
    );
    assert!(
        src.contains("s.close(None)"),
        "expected `s.close(None)` (SinkExt::close - None = no Close frame code/reason) in: {src}"
    );
    assert!(
        src.contains(".await.ok()"),
        "expected `.await.ok()` (panic-free async close) in: {src}"
    );
    // Must NOT use bare .unwrap() (panicking-generated-code rule).
    assert!(
        !src.contains(".unwrap()"),
        "expected NO bare `.unwrap()` in ws.close output: {src}"
    );
    must_reparse(&src);
}

// ===========================================================================
// 12. extern_crates registration (narrow tokio / tokio-tungstenite walkers).
// ===========================================================================

#[test]
fn tcp_codegen_registers_tokio_extern_crate() {
    // Any TCP.* call should register the `tokio` crate.
    let main = func_decl(
        "main",
        &[],
        vec![expr_stmt(ns_assoc_call(
            "TCP",
            "connect",
            vec![str_expr("127.0.0.1"), int_expr(8080)],
        ))],
    );
    let mut codegen = RustCodegen::new();
    let _ = codegen.generate(&[main]).expect("codegen must succeed");
    let extern_crates = codegen.extern_crates();
    assert!(
        extern_crates.contains("tokio"),
        "extern_crates should contain `tokio`, got: {:?}",
        extern_crates
    );
    // Must NOT register tokio-tungstenite for TCP.* (narrow walker).
    assert!(
        !extern_crates.contains("tokio-tungstenite"),
        "extern_crates should NOT contain `tokio-tungstenite` for TCP.*, got: {:?}",
        extern_crates
    );
}

#[test]
fn udp_codegen_registers_tokio_extern_crate() {
    // Any UDP.* call should register the `tokio` crate.
    let main = func_decl(
        "main",
        &[],
        vec![expr_stmt(ns_assoc_call(
            "UDP",
            "bind",
            vec![str_expr("0.0.0.0"), int_expr(9090)],
        ))],
    );
    let mut codegen = RustCodegen::new();
    let _ = codegen.generate(&[main]).expect("codegen must succeed");
    let extern_crates = codegen.extern_crates();
    assert!(
        extern_crates.contains("tokio"),
        "extern_crates should contain `tokio`, got: {:?}",
        extern_crates
    );
    assert!(
        !extern_crates.contains("tokio-tungstenite"),
        "extern_crates should NOT contain `tokio-tungstenite` for UDP.*, got: {:?}",
        extern_crates
    );
}

#[test]
fn websocket_codegen_registers_tokio_tungstenite_and_futures_util() {
    // Any WebSocket.* call should register the `tokio-tungstenite`
    // AND `futures-util` crates (and tokio, transitively for
    // clarity - mirrors how HMAC.sha256 also records sha2).
    let main = func_decl(
        "main",
        &[],
        vec![expr_stmt(ns_assoc_call(
            "WebSocket",
            "connect",
            vec![str_expr("ws://example.com")],
        ))],
    );
    let mut codegen = RustCodegen::new();
    let _ = codegen.generate(&[main]).expect("codegen must succeed");
    let extern_crates = codegen.extern_crates();
    assert!(
        extern_crates.contains("tokio-tungstenite"),
        "extern_crates should contain `tokio-tungstenite`, got: {:?}",
        extern_crates
    );
    assert!(
        extern_crates.contains("futures-util"),
        "extern_crates should contain `futures-util`, got: {:?}",
        extern_crates
    );
    assert!(
        extern_crates.contains("tokio"),
        "extern_crates should also contain `tokio` (transitive but explicit), got: {:?}",
        extern_crates
    );
}

#[test]
fn networking_codegen_does_not_register_tokio_when_unused() {
    // A program with no TCP / UDP / WebSocket / sleep calls should
    // NOT register tokio (or tokio-tungstenite / futures-util).
    let main = func_decl(
        "main",
        &[],
        vec![expr_stmt(Expr::FuncCall {
            callee: Box::new(ident_expr("print")),
            args: vec![str_expr("hi")],
            span: span(),
        })],
    );
    let mut codegen = RustCodegen::new();
    let _ = codegen.generate(&[main]).expect("codegen must succeed");
    let extern_crates = codegen.extern_crates();
    assert!(
        !extern_crates.contains("tokio"),
        "extern_crates should NOT contain `tokio` when networking is unused, got: {:?}",
        extern_crates
    );
    assert!(
        !extern_crates.contains("tokio-tungstenite"),
        "extern_crates should NOT contain `tokio-tungstenite` when networking is unused, got: {:?}",
        extern_crates
    );
    assert!(
        !extern_crates.contains("futures-util"),
        "extern_crates should NOT contain `futures-util` when networking is unused, got: {:?}",
        extern_crates
    );
}

// ===========================================================================
// 13. Error cases - arity mismatch surfaces a clear CodegenError.
// ===========================================================================

#[test]
fn tcp_codegen_rejects_connect_with_zero_args() {
    let result = std::panic::catch_unwind(|| {
        let _ = codegen_one_expr_in("f", ns_assoc_call("TCP", "connect", vec![]));
    });
    assert!(
        result.is_err(),
        "expected codegen to reject `TCP.connect()` (expected 2 args - host + port)"
    );
}

#[test]
fn tcp_codegen_rejects_connect_with_one_arg() {
    let result = std::panic::catch_unwind(|| {
        let _ = codegen_one_expr_in("f", ns_assoc_call("TCP", "connect", vec![str_expr("h")]));
    });
    assert!(
        result.is_err(),
        "expected codegen to reject `TCP.connect(\"h\")` (expected 2 args - host + port)"
    );
}

#[test]
fn tcp_codegen_rejects_connect_with_three_args() {
    let result = std::panic::catch_unwind(|| {
        let _ = codegen_one_expr_in(
            "f",
            ns_assoc_call(
                "TCP",
                "connect",
                vec![str_expr("h"), int_expr(1), str_expr("extra")],
            ),
        );
    });
    assert!(
        result.is_err(),
        "expected codegen to reject `TCP.connect(\"h\", 1, \"extra\")` (expected 2 args)"
    );
}

#[test]
fn udp_codegen_rejects_bind_with_zero_args() {
    let result = std::panic::catch_unwind(|| {
        let _ = codegen_one_expr_in("f", ns_assoc_call("UDP", "bind", vec![]));
    });
    assert!(
        result.is_err(),
        "expected codegen to reject `UDP.bind()` (expected 2 args - host + port)"
    );
}

#[test]
fn websocket_codegen_rejects_connect_with_zero_args() {
    let result = std::panic::catch_unwind(|| {
        let _ = codegen_one_expr_in("f", ns_assoc_call("WebSocket", "connect", vec![]));
    });
    assert!(
        result.is_err(),
        "expected codegen to reject `WebSocket.connect()` (expected 1 arg - url)"
    );
}

#[test]
fn websocket_codegen_rejects_connect_with_two_args() {
    let result = std::panic::catch_unwind(|| {
        let _ = codegen_one_expr_in(
            "f",
            ns_assoc_call(
                "WebSocket",
                "connect",
                vec![str_expr("ws://x"), str_expr("extra")],
            ),
        );
    });
    assert!(
        result.is_err(),
        "expected codegen to reject `WebSocket.connect(\"ws://x\", \"extra\")` (expected 1 arg)"
    );
}

#[test]
fn connection_codegen_rejects_send_with_zero_args() {
    let result = std::panic::catch_unwind(|| {
        let _ = codegen_stmts_in(
            "f",
            tcp_body_with_extra(instance_call(ident_expr("c"), "send", vec![])),
        );
    });
    assert!(
        result.is_err(),
        "expected codegen to reject `c.send()` (expected 1 arg - data)"
    );
}

#[test]
fn socket_codegen_rejects_send_to_with_one_arg() {
    let result = std::panic::catch_unwind(|| {
        let _ = codegen_stmts_in(
            "f",
            udp_body_with_extra(instance_call(
                ident_expr("s"),
                "send_to",
                vec![str_expr("data")],
            )),
        );
    });
    assert!(
        result.is_err(),
        "expected codegen to reject `s.send_to(\"data\")` (expected 2 args - data + addr)"
    );
}

// ===========================================================================
// 14. insta snapshots - byte-stable codegen pinning.
// ===========================================================================

#[test]
fn tcp_codegen_connect_snapshot() {
    let src = codegen_one_expr_in(
        "f",
        ns_assoc_call(
            "TCP",
            "connect",
            vec![str_expr("127.0.0.1"), int_expr(8080)],
        ),
    );
    insta::assert_snapshot!(src);
}

#[test]
fn udp_codegen_bind_snapshot() {
    let src = codegen_one_expr_in(
        "f",
        ns_assoc_call("UDP", "bind", vec![str_expr("0.0.0.0"), int_expr(9090)]),
    );
    insta::assert_snapshot!(src);
}

#[test]
fn websocket_codegen_connect_snapshot() {
    let src = codegen_one_expr_in(
        "f",
        ns_assoc_call(
            "WebSocket",
            "connect",
            vec![str_expr("ws://example.com/socket")],
        ),
    );
    insta::assert_snapshot!(src);
}

#[test]
fn connection_codegen_send_snapshot() {
    let src = codegen_stmts_in(
        "f",
        tcp_body_with_extra(instance_call(
            ident_expr("c"),
            "send",
            vec![str_expr("hello")],
        )),
    );
    insta::assert_snapshot!(src);
}

#[test]
fn connection_codegen_recv_snapshot() {
    let src = codegen_stmts_in(
        "f",
        tcp_body_with_extra(instance_call(ident_expr("c"), "recv", vec![])),
    );
    insta::assert_snapshot!(src);
}

#[test]
fn connection_codegen_close_snapshot() {
    let src = codegen_stmts_in(
        "f",
        tcp_body_with_extra(instance_call(ident_expr("c"), "close", vec![])),
    );
    insta::assert_snapshot!(src);
}

#[test]
fn socket_codegen_send_to_snapshot() {
    let src = codegen_stmts_in(
        "f",
        udp_body_with_extra(instance_call(
            ident_expr("s"),
            "send_to",
            vec![str_expr("ping"), str_expr("127.0.0.1:9999")],
        )),
    );
    insta::assert_snapshot!(src);
}

#[test]
fn socket_codegen_recv_from_snapshot() {
    let src = codegen_stmts_in(
        "f",
        udp_body_with_extra(instance_call(ident_expr("s"), "recv_from", vec![])),
    );
    insta::assert_snapshot!(src);
}

#[test]
fn wsconnection_codegen_send_snapshot() {
    let src = codegen_stmts_in(
        "f",
        ws_body_with_extra(instance_call(
            ident_expr("ws"),
            "send",
            vec![str_expr("hi")],
        )),
    );
    insta::assert_snapshot!(src);
}

#[test]
fn wsconnection_codegen_recv_snapshot() {
    let src = codegen_stmts_in(
        "f",
        ws_body_with_extra(instance_call(ident_expr("ws"), "recv", vec![])),
    );
    insta::assert_snapshot!(src);
}

#[test]
fn wsconnection_codegen_close_snapshot() {
    let src = codegen_stmts_in(
        "f",
        ws_body_with_extra(instance_call(ident_expr("ws"), "close", vec![])),
    );
    insta::assert_snapshot!(src);
}

#[test]
fn networking_codegen_full_program_snapshot() {
    // End-to-end snapshot: a `main` that exercises one call from each
    // of the three networking modules' surfaces. Pins the full shape
    // of the generated Rust for a typical TCP / UDP / WebSocket-using
    // program (the acceptance criterion from the task spec).
    let main = func_decl(
        "main",
        &[],
        vec![
            // TCP value type + all 3 instance methods.
            let_stmt(
                "c",
                ns_assoc_call(
                    "TCP",
                    "connect",
                    vec![str_expr("127.0.0.1"), int_expr(8080)],
                ),
            ),
            let_stmt(
                "_send",
                instance_call(ident_expr("c"), "send", vec![str_expr("ping")]),
            ),
            let_stmt("_recv", instance_call(ident_expr("c"), "recv", vec![])),
            let_stmt("_close", instance_call(ident_expr("c"), "close", vec![])),
            // UDP value type + both instance methods.
            let_stmt(
                "s",
                ns_assoc_call("UDP", "bind", vec![str_expr("0.0.0.0"), int_expr(9090)]),
            ),
            let_stmt(
                "_send_to",
                instance_call(
                    ident_expr("s"),
                    "send_to",
                    vec![str_expr("datagram"), str_expr("127.0.0.1:9999")],
                ),
            ),
            let_stmt(
                "_recv_from",
                instance_call(ident_expr("s"), "recv_from", vec![]),
            ),
            // WebSocket value type + all 3 instance methods.
            let_stmt(
                "ws",
                ns_assoc_call(
                    "WebSocket",
                    "connect",
                    vec![str_expr("ws://example.com/socket")],
                ),
            ),
            let_stmt(
                "_ws_send",
                instance_call(ident_expr("ws"), "send", vec![str_expr("hi")]),
            ),
            let_stmt("_ws_recv", instance_call(ident_expr("ws"), "recv", vec![])),
            let_stmt(
                "_ws_close",
                instance_call(ident_expr("ws"), "close", vec![]),
            ),
        ],
    );
    let mut codegen = RustCodegen::new();
    let file = codegen.generate(&[main]).expect("codegen must succeed");
    let src = buff_lang_codegen_rust::format_file(&file);
    insta::assert_snapshot!(src);
}

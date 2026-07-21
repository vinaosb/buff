# buff-jupyter

Jupyter kernel protocol v5 implementation for Buff. Pure-Rust ZMQ via `zeromq` (NOT the `zmq` crate which links C libzmq).

## OVERVIEW

Implements the 5 Jupyter sockets (shell/iopub/stdin/control/heartbeat), HMAC-SHA256 auth, kernelspec installation, and cell evaluation via `buff-eval`. Powers `buff jupyter install` and `buff jupyter start`. The kernel owns an `Evaluator` so `let`/`func` state persists across cells. Errors don't kill the kernel.

## STRUCTURE

```
src/
├── lib.rs            # 142 lines — module wiring + public API re-exports + run_kernel/install
├── kernel.rs         # 2300 lines — main kernel loop: socket dispatch, execution state machine,
│                      #   iopub emission (busy/stream/execute_result/error/idle), ?/?? introspection,
│                      #   Vector/Matrix HTML table rendering, MockTransport for tests
├── messages.rs       # 655 lines — serde structs: KernelInfoReply, ExecuteReply, ExecuteResult,
│                      #   DisplayData, StreamOutput, ErrorOutput, ShutdownReply, LanguageInfo,
│                      #   HelpLink, HelpLinkItem, ConnectionInfo, KernelSpecMetadata, CompleteReply,
│                      #   InspectReply + banner/version/name constants
├── kernelspec.rs     # 377 lines — kernel.json generation + installation: install path resolution,
│                      #   logo file embedding (base64), directory creation
├── connection.rs     # 284 lines — connection.json parser: ip/transport/port/signature_scheme/key
├── hmac.rs           # 221 lines — HMAC-SHA256 sign + verify (hmac + sha2 + hex crates)
├── transport.rs      # 269 lines — ZmqTransport trait + ZmqSocketSet (5-socket bind via zeromq)
├── wire.rs           # 236 lines — wire format: multi-frame ZMQ encode/decode, MessageHeader,
│                      #   WireMessage, PROTOCOL_VERSION constant, IDS_MSG_DELIMITER
└── error.rs          # 130 lines — JupyterError enum (thiserror) + JupyterResult alias
tests/                # 5 test files: connection, hmac, kernelspec, kernel_info, wire
```

## PUBLIC API

| Symbol | Notes |
|---|---|
| `run_kernel(path) -> JupyterResult<()>` | Entry for `buff jupyter start` |
| `install() -> JupyterResult<PathBuf>` | Entry for `buff jupyter install` |
| `Kernel<T>` | Generic over `ZmqTransport` (mock-friendly) |
| `ConnectionFile` | Parse connection.json, validate fields |
| `KernelSpec` | kernelspec.json generation + install |
| `sign(frames, key) -> String` / `verify(frames, key, sig) -> Result` | HMAC-SHA256 |
| `WireMessage` / `MessageHeader` | Wire format types |
| `ZmqSocketSet` / `ZmqTransport` trait | Transport abstraction |

## WHERE TO LOOK

| Task | File |
|---|---|
| Add new Jupyter message type | `messages.rs` (serde struct) + `kernel.rs` (handler arm in dispatch) |
| Change socket binding / transport | `transport.rs::ZmqSocketSet::bind` |
| Change connection-file parsing / validation | `connection.rs` |
| Tune kernelspec (name, logo, install path) | `kernelspec.rs` |
| Change HMAC signing / verification | `hmac.rs` |
| Change execution engine (eval flow, iopub emit) | `kernel.rs::handle_execute_request` |
| Add introspection magic (`?`/`??`) | `kernel.rs` — prefix detection before normal eval |
| Add Vector/Matrix rich display | `kernel.rs` — type-of check + HTML `<table>` rendering |

## CONVENTIONS (this crate only)

- **No `unwrap`/`expect`/`panic!`/`todo!`/`unimplemented!`** in non-test code. All fallible ops return `JupyterResult<T>`.
- **Pure-Rust ZMQ only**: `zeromq = "0.4"` (async, tokio-based, pure Rust). NEVER the `zmq` crate (links C libzmq, fails on Windows via same `cc-rs` chain that killed chumsky).
- **In-process evaluation**: kernel calls `Evaluator::eval_line` directly. No subprocess exec for Buff code.
- **`Kernel<T>` is generic over `ZmqTransport`** — the protocol layer is unit-testable with a mock transport (see `#[cfg(test)]` in `kernel.rs`).
- **Blocking eval**: `Evaluator::eval_line` spawns `rustc` synchronously. The kernel loop runs cells sequentially. Concurrent execution is post-T129c work.
- **`?name`/`??name` introspection**: detected as cell prefix before normal eval. `?name` uses `Evaluator::type_of` (no rustc spawn). `??name` also evaluates to capture the current value.
- **Unknown `msg_type`s** are logged and dropped. No reply emitted. The client times out.
- **Heartbeat** runs in a separate tokio task echoing every frame (ZMQ REP semantics).

## DEPS

| Crate | Purpose |
|---|---|
| `zeromq` 0.4 | Pure-Rust async ZMQ sockets (NOT the `zmq` C-binding crate) |
| `tokio` | Async runtime backing kernel loop + zeromq sockets |
| `buff-eval` | Evaluation engine (workspace path dep) |
| `serde` / `serde_json` | Wire message (de)serialization |
| `thiserror` | `JupyterError` derive |
| `uuid` | Kernel session ID generation (`Uuid::new_v4`) |
| `hmac` / `sha2` / `hex` | HMAC-SHA256 signing per Jupyter protocol |
| `bytes` | Frame buffer types |

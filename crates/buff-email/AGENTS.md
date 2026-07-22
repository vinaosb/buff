# buff-email

SMTP + templated email for the Buff language. Pure-Rust MVP wrapping the [`lettre`](https://docs.rs/lettre) crate (`rustls` TLS, NOT `native-tls`) plus [`handlebars`] for HTML email templates (reuses the T19 pin). Per T42 spec: `Email.new(from, to, subject)`, `email.body(text)`, `email.html(template, context)`, `email.attach(path)`, `SmtpClient.new(host, port, username, password)`, `SmtpClient.send(email)`.

**Status: experimental** (T42 v1.17 frameworks wave).

## STRUCTURE

```
buff-email/
├── Cargo.toml            # lettre (rustls) + handlebars + serde_json + thiserror + insta deps
├── src/
│   ├── lib.rs            # Email + SmtpClient main surface (~440 LOC)
│   └── error.rs          # EmailError enum (~75 LOC)
├── examples/
│   ├── plain_text.rs          # plain-text email builder + SmtpClient.send
│   ├── html_template.rs       # HTML email via handlebars template
│   ├── attachment.rs          # email with file attachment
│   └── email/
│       ├── plain_text.buff    # Buff-side forward-decl (matches .rs)
│       ├── html_template.buff
│       └── attachment.buff
└── tests/
    └── core.rs           # 18 unit tests + 3 insta snapshots (~290 LOC)
```

Total: ~900 LOC (well under the 2000 LOC T42 cap).

## WHERE TO LOOK

| Task | File |
|---|---|
| Add a new email builder method | `src/lib.rs` (add `pub fn` on `Email`) + test in `tests/core.rs` |
| Add a new error variant | `src/error.rs` |
| Add a new SmtpClient config knob (TLS mode, timeout) | `src/lib.rs::SmtpClient::new` |
| Wire a Buff-side method to codegen | `crates/buff-lang-types/src/prelude_types.rs` (PreludeInstanceFn + `instance_fn_return_type`) + `crates/buff-lang-codegen-rust/src/rust_codegen.rs::lower_prelude_type_instance_fn` |

## PUBLIC API (6 functions, ≤15 cap)

### `Email` (4 functions)
- Constructors: `new` (mandatory from / to / subject)
- Mutators (builder pattern, consume self, return new Email): `body`, `html`, `attach`

### `SmtpClient` (2 functions)
- Constructors: `new` (host, port, username, password; STARTTLS via rustls)
- Instance: `send` (returns `Result<Void, EmailError>`)

## CONVENTIONS

- **Pure-Rust only**: lettre's `rustls` feature pulls in pure-Rust rustls + ring crypto backend + Mozilla root certs via `webpki-roots`. NO `native-tls`, NO `boring-tls`, NO OpenSSL — matches the "no C library, no Docker" hard rule from T126/T127 and the workspace "rustls-tls NOT native-tls" rule from AGENTS.md.
- **FFI safety**: every public entry point follows the 6 hard rules from `crates/buff-lang-ffi-guide/GUIDE.md`. See the compliance table in `src/lib.rs` module doc.
- **Panic-free**: no `unwrap` / `expect` / `panic!` in non-test code. Address validation / template rendering / attachment reading / SMTP failures all return `Result<_, EmailError>`.
- **catch_unwind boundary**: every public function wraps its body in `catch_unwind` per FFI guide R6.
- **Builder pattern**: `Email` mutators (`body` / `html` / `attach`) consume `self` and return a new `Email`. This lets the codegen lower `email.body(t).html(h, c).attach(p)` as a single expression chain.
- **pub(crate) surface discipline**: `build_message` + accessors (`from_addr` / `to_addr` / `subject` / `plain_body` / `html_body` / `attachments`) are all `pub(crate)` — internal helpers used by tests + SmtpClient but NOT part of the stable Buff-visible 6-fn cap.

## RELATIONSHIP TO OTHER CRATES

| Crate | Relationship |
|---|---|
| `lettre` | Upstream SMTP transport + message builder. `buff-email` is a safe wrapper; never re-exports `lettre::*` types directly. |
| `handlebars` | Upstream templating engine (shared with T19 `buff-template`). `buff-email` uses it for `email.html(template, context)`. |
| `buff-lang-types` | `prelude_types.rs` registers `PreludeType::Email` + `PreludeType::SmtpClient` + assoc fns (`New` for both) + instance fns (`Body` / `Html` / `Attach` for Email; `Send` for SmtpClient). `ty.rs` has the `Type::Email` + `Type::SmtpClient` variants + `is_prelude_email()` / `is_prelude_smtp_client()` predicates. |
| `buff-lang-codegen-rust` | `rust_codegen.rs::buff_type_to_syn` has the `Type::Email => "buff_email::Email"` + `Type::SmtpClient => "buff_email::SmtpClient"` arms. `lower_prelude_type_assoc_fn` has the `(Email, New)` / `(SmtpClient, New)` arms. `lower_prelude_type_instance_fn` has the `(Email, Body)` / `(Email, Html)` / `(Email, Attach)` / `(SmtpClient, Send)` arms. `program_uses_namespace("Email")` + `("SmtpClient")` record `buff-email` + `lettre` + `handlebars` in `extern_crates`. |
| `buff-template` (T19) | Shares the workspace `handlebars` pin; both crates wrap the same templating engine (buff-template for general HTML, buff-email for HTML email bodies). |
| `buff-lang-ffi-guide` | Defines the 6 hard rules every public function in this crate follows. |

## NOTES

- **STARTTLS, not implicit TLS**: `SmtpClient::new` uses `lettre::SmtpTransport::relay(host)` which opens a plaintext connection on the given port and upgrades via STARTTLS. This is the standard port-587 mailtrap / Gmail / SES / Office365 pattern. Implicit TLS on port 465 (`SmtpTransport::builder_dangerous` + `Tls::Wrapper`) is deferred to v1.18+.
- **No async surface for MVP**: `SmtpClient::send` is the blocking `lettre::Transport::send`. The async-std / tokio variants (`lettre::AsyncSmtpTransport` + `lettre::Tokio1Executor`) are deferred to v1.18+ when Buff's async-codegen layer lowers them automatically.
- **No IMAP / POP3**: T42 must-not #1 — receive-side protocols are explicitly deferred to v1.22+.
- **Attachment encoding is base64**: lettre's `Attachment::body` MIME-encodes via base64 inside the multipart/mixed envelope. Pure-Rust (lettre's own base64 encoder) — no native dep.
- **Mock SMTP for tests**: `tests/core.rs` spins up an in-process mock SMTP server returning hardcoded 250 OK replies, so the suite is hermetic — no real mailtrap / Gmail / SES credentials needed.

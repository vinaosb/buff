# buff-email

> SMTP + templated email for the **Buff** language. Pure-Rust MVP (`rustls` TLS).

`buff-email` wraps the mature [`lettre`](https://docs.rs/lettre) crate (pure-Rust TLS via the `rustls` feature — NOT `native-tls`) and reuses the T19 [`handlebars`](https://docs.rs/handlebars) pin for HTML email templates. Buff code accesses email via the `Email` + `SmtpClient` prelude types:

```buff
email = Email.new(from: "noreply@buff.dev", to: "user@example.com", subject: "Welcome")
email = email.body(text: "Welcome to Buff!")
email = email.html(template: "<h1>Hello {{name}}</h1>", context: "{\"name\":\"Buff\"}")

client = SmtpClient.new(host: "smtp.mailtrap.io", port: 587, username: "u", password: "p")
client.send(email: email)
```

**Status: experimental** (T42 v1.17 frameworks wave).

## Installation

This crate is consumed by the Buff compiler's codegen layer; end users do not install it directly. It is automatically pulled in as a path dependency of the workspace when a Buff program uses the `Email` or `SmtpClient` prelude type.

For direct Rust use:

```bash
cargo add buff-email --path crates/buff-email
```

## Quick start

```rust
use buff_email::{Email, SmtpClient};

fn main() -> Result<(), buff_email::EmailError> {
    let email = Email::new("noreply@buff.dev", "user@example.com", "Welcome")?
        .body("Welcome to Buff!")?;
    let client = SmtpClient::new("smtp.mailtrap.io", 587, "user", "pass")?;
    client.send(&email)?;
    Ok(())
}
```

## Public API

### `Email` — buildable email message

| Method | Signature | Notes |
|---|---|---|
| `Email::new` | `(from, to, subject) -> Result<Email, EmailError>` | Validates RFC 5322 mailboxes. `catch_unwind` boundary. |
| `email.body` | `(self, text) -> Result<Email, EmailError>` | Builder; plain-text body. |
| `email.html` | `(self, template, context_json) -> Result<Email, EmailError>` | Builder; handlebars template + JSON context. |
| `email.attach` | `(self, path) -> Result<Email, EmailError>` | Builder; queue a file attachment. |

### `SmtpClient` — configured SMTP transport

| Method | Signature | Notes |
|---|---|---|
| `SmtpClient::new` | `(host, port, username, password) -> Result<SmtpClient, EmailError>` | STARTTLS via rustls. |
| `client.send` | `(&self, &Email) -> Result<(), EmailError>` | TCP+TLS handshake on first call; pooled after. |

## Templating

`email.html(template, context)` uses standard handlebars syntax (the same engine T19 `buff-template` uses):

- `{{ variable }}` — variable substitution
- `{% if cond %}...{% endif %}` — conditionals
- `{% for item in list %}...{% endfor %}` — loops
- `{{! comment }}` — comments

The `context_json` argument is a JSON object string: `{"name": "Buff", "items": ["a", "b"]}`.

## MIME structure

| Inputs | MIME structure |
|---|---|
| plain only | `text/plain` body |
| html only | `text/html` body |
| plain + html | `multipart/alternative` |
| any of the above + N attachments | `multipart/mixed` wrapping the body plus each attachment |

## FFI safety

Every public function follows the [6 hard rules](../buff-lang-ffi-guide/GUIDE.md) from the FFI guide:

| Rule | Compliance |
|---|---|
| R1 — No raw pointers | Public surface: `Email`, `SmtpClient`, `EmailError`. No `*const`/`*mut`. |
| R2 — Ownership boundary | `Email::new` / `SmtpClient::new` return owned values. `body` / `html` / `attach` consume self, return owned `Email`. |
| R3 — Error mapping | Every fallible op returns `Result<T, EmailError>`. `lettre::transport::smtp::Error` + `lettre::address::AddressError` mapped via `From`. |
| R4 — Thread safety | `Email` + `SmtpClient` are `Send + Sync` (wraps `lettre::Message` + `lettre::SmtpTransport`, both `Send + Sync`). |
| R5 — Lifetime hiding | No public lifetime parameters. All strings owned at the boundary. |
| R6 — Panic boundary | Every public function wraps its body in `catch_unwind`. |

## Testing

```bash
cargo test -p buff-email
cargo clippy -p buff-email --all-targets -- -D warnings
cargo fmt -p buff-email --check
```

Tests are hermetic: SMTP integration tests use a mock SMTP server (no real mailtrap credentials needed). Template / address / attachment / MIME-assembly paths covered by unit tests + insta snapshots.

## License

Dual-licensed under [MIT](../../LICENSE) or [Apache-2.0](../../LICENSE), matching the rest of the Buff workspace.

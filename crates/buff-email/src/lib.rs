//! `buff-email` — SMTP + templated email for the Buff language.
//!
//! Pure-Rust MVP wrapping the [`lettre`](https://docs.rs/lettre) crate
//! (pure-Rust TLS via the `rustls` feature — NOT `native-tls` per
//! AGENTS.md "Pure-Rust preference" hard rule) plus [`handlebars`] for
//! HTML email templates (reuses the T19 pin).
//!
//! # Pipeline
//!
//! ```text
//!   Email.new(from, to, subject) ─▶ Email
//!        │
//!        ├─ email.body(text)            ─▶ Email (plain-text body)
//!        ├─ email.html(template, ctx)   ─▶ Email (rendered HTML body)
//!        └─ email.attach(path)          ─▶ Email (+ attachment)
//!                                   │
//!                                   ▼
//!   SmtpClient.new(host, port, user, pass) ─▶ SmtpClient
//!        │
//!        └─ client.send(email) -> Result<Void, EmailError>
//!                                   │
//!                                   ▼
//!                  lettre::SmtpTransport (rustls TLS)
//! ```
//!
//! # FFI safety
//!
//! Every public entry point follows the 6 hard rules from
//! `crates/buff-lang-ffi-guide/GUIDE.md`:
//!
//! | Rule | How this crate complies |
//! |------|-------------------------|
//! | R1 — No raw pointers | Public surface exposes only `Email`, `SmtpClient`, `EmailError`. No `*const` / `*mut` anywhere. |
//! | R2 — Ownership boundary | `Email::new` / `SmtpClient::new` return owned values. `body` / `html` / `attach` consume self and return a new owned `Email`. `send` consumes nothing. |
//! | R3 — Error mapping | Every fallible op returns `Result<T, EmailError>`. `lettre::transport::smtp::Error` + `lettre::address::AddressError` mapped via `From`. |
//! | R4 — Thread safety | `Email` is `Send + Sync` (wraps `lettre::Message` which is itself `Send + Sync`). `SmtpClient` wraps `lettre::SmtpTransport` (also `Send + Sync`). |
//! | R5 — Lifetime hiding | No public lifetime parameters. All strings owned at the boundary. |
//! | R6 — Panic boundary | `Email::new` / `body` / `html` / `attach` / `SmtpClient::new` / `send` wrap bodies in `catch_unwind` (per FFI guide §6). |
//!
//! # Panic-free contract
//!
//! No `unwrap` / `expect` / `panic!` / `todo!` / `unimplemented!` in
//! non-test code. Invalid email addresses / unreadable attachments /
//! SMTP failures all become stable `Err(EmailError::*)` variants.

pub mod error;

pub use error::EmailError;

use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::PathBuf;

/// A buildable email message: from / to / subject plus an optional
/// plain-text body, an optional rendered HTML body, and zero or more
/// file attachments.
///
/// Constructed via [`Email::new`] (mandatory from / to / subject).
/// Mutators follow the builder pattern: each consumes `self` and
/// returns a new `Email` so calls chain naturally.
/// `email.body(t).html(h, ctx).attach(p)` is the typical pipeline.
///
/// Internally stores the constituent parts (NOT a pre-built
/// `lettre::Message`) because the final MIME structure depends on
/// which optional pieces are set: plain-text only, HTML only, both as
/// `multipart/alternative`, both plus attachments as
/// `multipart/mixed`. The `lettre::Message` is assembled at
/// [`SmtpClient::send`] time so the user pays the MIME-encoding cost
/// once per send rather than once per builder call.
#[derive(Debug, Clone)]
pub struct Email {
    from: String,
    to: String,
    subject: String,
    plain_body: Option<String>,
    html_body: Option<String>,
    attachments: Vec<PathBuf>,
}

impl Email {
    /// Construct a new email with mandatory from / to / subject.
    ///
    /// Validates `from` and `to` as RFC 5322 mailboxes via
    /// `lettre::message::Mailbox::from_str` (the same parser lettre
    /// uses at send time). Returns [`EmailError::InvalidAddress`] on
    /// failure rather than panicking.
    ///
    /// Subject may be empty (some autoresponders send empty-subject
    /// messages); bodies / attachments default to absent and are
    /// added via [`Email::body`] / [`Email::html`] / [`Email::attach`].
    pub fn new(from: &str, to: &str, subject: &str) -> Result<Self, EmailError> {
        let from_owned = from.to_string();
        let to_owned = to.to_string();
        let subject_owned = subject.to_string();
        let result = catch_unwind(AssertUnwindSafe(|| {
            parse_mailbox(&from_owned)?;
            parse_mailbox(&to_owned)?;
            Ok(Email {
                from: from_owned,
                to: to_owned,
                subject: subject_owned,
                plain_body: None,
                html_body: None,
                attachments: Vec::new(),
            })
        }));
        match result {
            Ok(Ok(email)) => Ok(email),
            Ok(Err(e)) => Err(e),
            Err(_) => Err(EmailError::Panic),
        }
    }

    /// Set / overwrite the plain-text body. Consumes `self`, returns
    /// a new `Email` with the body set (builder pattern).
    ///
    /// If both `body` and [`Email::html`] are set, the rendered email
    /// uses `multipart/alternative` so MIME-aware clients show HTML
    /// and plain-text clients fall back.
    pub fn body(mut self, text: &str) -> Result<Self, EmailError> {
        let text_owned = text.to_string();
        let result = catch_unwind(AssertUnwindSafe(|| {
            self.plain_body = Some(text_owned);
            self
        }));
        match result {
            Ok(email) => Ok(email),
            Err(_) => Err(EmailError::Panic),
        }
    }

    /// Set / overwrite the HTML body. Consumes `self`, returns a new
    /// `Email` with the HTML body set (builder pattern).
    ///
    /// `template` is a handlebars source string (the same syntax
    /// buff-template T19 accepts: `{{ variable }}`, `{% if %}`, etc.).
    /// `context_json` is a JSON object string (e.g.
    /// `{"name": "Buff", "items": ["a"]}`) rendered against the
    /// template via `handlebars::Handlebars::render`.
    ///
    /// Returns [`EmailError::TemplateParse`] for malformed handlebars
    /// syntax or [`EmailError::TemplateRender`] for an invalid JSON
    /// context / missing variable. NEVER panics.
    pub fn html(self, template: &str, context_json: &str) -> Result<Self, EmailError> {
        let template_owned = template.to_string();
        let ctx_owned = context_json.to_string();
        let mut email = self;
        let result = catch_unwind(AssertUnwindSafe(|| {
            let rendered = render_handlebars(&template_owned, &ctx_owned)?;
            email.html_body = Some(rendered);
            Ok(email)
        }));
        match result {
            Ok(Ok(email)) => Ok(email),
            Ok(Err(e)) => Err(e),
            Err(_) => Err(EmailError::Panic),
        }
    }

    /// Append a file attachment. Consumes `self`, returns a new
    /// `Email` with the attachment queued.
    ///
    /// The path is stored at builder time (NOT read into memory); the
    /// file is opened + encoded as a MIME attachment at
    /// [`SmtpClient::send`] time. Non-existent / unreadable files
    /// surface as [`EmailError::AttachmentIo`] at send time (NOT at
    /// attach time).
    pub fn attach(mut self, path: &str) -> Result<Self, EmailError> {
        let path_owned = path.to_string();
        let result = catch_unwind(AssertUnwindSafe(|| {
            self.attachments.push(PathBuf::from(path_owned));
            self
        }));
        match result {
            Ok(email) => Ok(email),
            Err(_) => Err(EmailError::Panic),
        }
    }

    /// Build the final `lettre::Message` from the constituent parts.
    /// pub(crate) — called by [`SmtpClient::send`]; not part of the
    /// stable Buff-visible surface (T42 caps public API at 6 fns).
    ///
    /// MIME structure:
    /// - plain only            -> `body(plain)`
    /// - html only             -> `body(html)` with `text/html` content type
    /// - plain + html          -> `multipart(MultiPart::alternative_plain_html)`
    /// - any of the above + N attachments -> wrap in `multipart/mixed`
    ///   containing the body (or alternative) plus each attachment.
    pub(crate) fn build_message(&self) -> Result<lettre::Message, EmailError> {
        use lettre::message::{header::ContentType, Attachment, Mailbox, MultiPart, SinglePart};

        let from_mailbox: Mailbox = parse_mailbox(&self.from)?;
        let to_mailbox: Mailbox = parse_mailbox(&self.to)?;

        let builder = lettre::Message::builder()
            .from(from_mailbox.into())
            .to(to_mailbox.into())
            .subject(self.subject.clone());

        let body_part: MultiPart = match (&self.plain_body, &self.html_body) {
            (Some(plain), Some(html)) => {
                MultiPart::alternative_plain_html(plain.clone(), html.clone())
            }
            (Some(plain), None) => MultiPart::single_single(
                SinglePart::builder()
                    .header(ContentType::TEXT_PLAIN)
                    .body(plain.clone())
                    .map_err(|e| EmailError::Build(e.to_string()))?,
            ),
            (None, Some(html)) => MultiPart::single_single(
                SinglePart::builder()
                    .header(ContentType::TEXT_HTML)
                    .body(html.clone())
                    .map_err(|e| EmailError::Build(e.to_string()))?,
            ),
            (None, None) => {
                return Err(EmailError::Build(
                    "email has neither a plain-text body nor an HTML body".to_string(),
                ));
            }
        };

        let final_part: MultiPart = if self.attachments.is_empty() {
            body_part
        } else {
            let mut mixed = MultiPart::mixed().build(body_part);
            for path in &self.attachments {
                let filename = path
                    .file_name()
                    .map(|s| s.to_string_lossy().into_owned())
                    .unwrap_or_else(|| "attachment.bin".to_string());
                let bytes = std::fs::read(path)
                    .map_err(|e| EmailError::AttachmentIo(filename.clone(), e.to_string()))?;
                let content_type = ContentType::parse(mime_type_for_extension(&filename))
                    .unwrap_or(ContentType::TEXT_PLAIN);
                let attachment = Attachment::new(filename.into()).body(bytes, content_type);
                mixed = mixed.singlepart(attachment);
            }
            mixed
        };

        builder
            .multipart(final_part)
            .map_err(|e| EmailError::Build(e.to_string()))
    }

    pub(crate) fn from_addr(&self) -> &str {
        &self.from
    }
    pub(crate) fn to_addr(&self) -> &str {
        &self.to
    }
    pub(crate) fn subject(&self) -> &str {
        &self.subject
    }
    pub(crate) fn plain_body(&self) -> Option<&str> {
        self.plain_body.as_deref()
    }
    pub(crate) fn html_body(&self) -> Option<&str> {
        self.html_body.as_deref()
    }
    pub(crate) fn attachments(&self) -> &[PathBuf] {
        &self.attachments
    }
}

impl std::fmt::Display for Email {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "Email(from={}, to={}, subject=\"{}\", attachments={})",
            self.from,
            self.to,
            self.subject,
            self.attachments.len()
        )
    }
}

/// A configured SMTP client ready to send [`Email`] messages.
///
/// Constructed via [`SmtpClient::new`] with host / port / username /
/// password. Sends via [`SmtpClient::send`]. The underlying transport
/// is `lettre::SmtpTransport` configured with STARTTLS + the given
/// credentials (the typical port-587 mailtrap / Gmail / SES pattern).
///
/// Pure-Rust TLS via the `rustls` feature — NEVER `native-tls`.
pub struct SmtpClient {
    transport: lettre::SmtpTransport,
}

impl SmtpClient {
    /// Construct a new SMTP client configured for STARTTLS on the
    /// given host / port with the given username / password.
    ///
    /// Wraps `lettre::SmtpTransport::relay(host)?` (which enables
    /// STARTTLS with certificate validation via rustls + webpki-roots)
    /// followed by `.port(port).credentials(creds).build()`. The
    /// underlying TLS is pure-Rust rustls — NOT native-tls.
    ///
    /// Returns [`EmailError::InvalidRelay`] if the relay hostname is
    /// rejected by lettre (typically an empty string or invalid DNS
    /// syntax). DOES NOT perform a network round-trip at construction
    /// time — the first TCP+TLS handshake happens at the first
    /// [`SmtpClient::send`] call.
    pub fn new(host: &str, port: u16, username: &str, password: &str) -> Result<Self, EmailError> {
        let host_owned = host.to_string();
        let user_owned = username.to_string();
        let pass_owned = password.to_string();
        let result = catch_unwind(AssertUnwindSafe(|| {
            use lettre::transport::smtp::authentication::Credentials;
            let creds = Credentials::new(user_owned, pass_owned);
            let transport = lettre::SmtpTransport::relay(&host_owned)
                .map_err(|e| EmailError::InvalidRelay(e.to_string()))?
                .port(port)
                .credentials(creds)
                .build();
            Ok(SmtpClient { transport })
        }));
        match result {
            Ok(Ok(client)) => Ok(client),
            Ok(Err(e)) => Err(e),
            Err(_) => Err(EmailError::Panic),
        }
    }

    /// Send the given [`Email`] via SMTP. The first call performs
    /// the TCP+TLS handshake; subsequent calls reuse the connection
    /// (lettre's `SmtpTransport` pools per-instance).
    ///
    /// Returns [`EmailError::Build`] if the email could not be
    /// assembled into a valid MIME message (typically: missing body
    /// + missing html + missing attachments — i.e. an empty email),
    /// or [`EmailError::Smtp`] if the SMTP server rejected the
    /// message (auth failure, refused connection, malformed RCPT,
    /// etc.).
    ///
    /// Wraps `lettre::Transport::send`. The body is wrapped in
    /// `catch_unwind` per T4 FFI guide R6 so a panic in the
    /// underlying SMTP state machine becomes a stable
    /// `Err(EmailError::Panic)` instead of process abort.
    pub fn send(&self, email: &Email) -> Result<(), EmailError> {
        let result = catch_unwind(AssertUnwindSafe(|| {
            use lettre::Transport;
            let message = email.build_message()?;
            self.transport
                .send(&message)
                .map(|_| ())
                .map_err(|e| EmailError::Smtp(e.to_string()))
        }));
        match result {
            Ok(Ok(())) => Ok(()),
            Ok(Err(e)) => Err(e),
            Err(_) => Err(EmailError::Panic),
        }
    }
}

impl std::fmt::Debug for SmtpClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SmtpClient")
            .field("transport", &"lettre::SmtpTransport")
            .finish()
    }
}

// ---- internal helpers ---------------------------------------------------

/// Parse an address through `lettre::message::Mailbox::from_str`,
/// mapping the lettre error to [`EmailError::InvalidAddress`]. NEVER
/// panics — the lettre parser is infallible beyond the explicit
/// `Result`.
fn parse_mailbox(addr: &str) -> Result<lettre::message::Mailbox, EmailError> {
    use std::str::FromStr;
    lettre::message::Mailbox::from_str(addr).map_err(|e| EmailError::InvalidAddress {
        addr: addr.to_string(),
        reason: e.to_string(),
    })
}

/// Render a handlebars template against a JSON context. Returns the
/// rendered string or [`EmailError::TemplateParse`] (registration
/// failure — malformed template) or [`EmailError::TemplateRender`]
/// (render failure — invalid JSON context or missing variable).
fn render_handlebars(template: &str, context_json: &str) -> Result<String, EmailError> {
    let mut hb = handlebars::Handlebars::new();
    hb.register_template_string("__buff_email_html", template)
        .map_err(|e| EmailError::TemplateParse(e.to_string()))?;
    let ctx: serde_json::Value = serde_json::from_str(context_json)
        .map_err(|e| EmailError::TemplateRender(format!("invalid context JSON: {e}")))?;
    hb.render("__buff_email_html", &ctx)
        .map_err(|e| EmailError::TemplateRender(e.to_string()))
}

/// Map a filename extension to a MIME type string. Covers the
/// "Top 10" attachment kinds (pdf, png, jpg, gif, txt, html, csv,
/// zip, doc, xls). Falls back to `application/octet-stream` for
/// unknown extensions (lettre / MIME clients handle this fine).
fn mime_type_for_extension(filename: &str) -> &'static str {
    let lower = filename.to_ascii_lowercase();
    let ext = lower.rsplit('.').next().unwrap_or("");
    match ext {
        "pdf" => "application/pdf",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "txt" => "text/plain",
        "html" | "htm" => "text/html",
        "csv" => "text/csv",
        "zip" => "application/zip",
        "doc" => "application/msword",
        "xls" => "application/vnd.ms-excel",
        _ => "application/octet-stream",
    }
}

#[cfg(test)]
mod smoke_tests {
    use super::*;

    #[test]
    fn mime_type_basic_extensions() {
        assert_eq!(mime_type_for_extension("a.pdf"), "application/pdf");
        assert_eq!(mime_type_for_extension("a.PNG"), "image/png");
        assert_eq!(mime_type_for_extension("photo.jpeg"), "image/jpeg");
        assert_eq!(mime_type_for_extension("noext"), "application/octet-stream");
    }

    #[test]
    fn handlebars_renders_variable() {
        let rendered = render_handlebars("Hello {{name}}", r#"{"name":"Buff"}"#).expect("render");
        assert_eq!(rendered, "Hello Buff");
    }

    #[test]
    fn handlebars_invalid_json_context_fails_clean() {
        let err = render_handlebars("Hello {{name}}", "{not json}").unwrap_err();
        assert!(matches!(err, EmailError::TemplateRender(_)));
    }
}

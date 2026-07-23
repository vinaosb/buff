//! Integration tests for the `buff-email` crate.
//!
//! Covers all 6 public functions per the T42 spec:
//! - `Email::new` (incl. address validation)
//! - `Email::body` / `Email::html` / `Email::attach` (builder chain)
//! - `SmtpClient::new` (relay + invalid hostname)
//! - `SmtpClient::send` (mock-SMTP happy path + failure paths)
//!
//! SMTP integration uses an in-process mock SMTP server (raw TCP
//! listener that returns hardcoded 250 OK replies) so the test suite
//! is hermetic — no real mailtrap / Gmail / SES credentials needed.

use buff_email::{Email, EmailError, SmtpClient};
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicU16, Ordering};
use std::sync::Once;
use std::thread;

static NEXT_PORT: AtomicU16 = AtomicU16::new(2525);
static INIT: Once = Once::new();

fn mock_smtp_port() -> u16 {
    INIT.call_once(|| {
        let port = NEXT_PORT.fetch_add(1, Ordering::SeqCst);
        thread::spawn(move || run_mock_smtp(port));
    });
    NEXT_PORT.load(Ordering::SeqCst)
}

fn run_mock_smtp(port: u16) {
    let listener = TcpListener::bind(("127.0.0.1", port)).expect("mock smtp bind");
    for stream in listener.incoming() {
        if let Ok(mut s) = stream {
            thread::spawn(move || {
                let _ = handle_mock_smtp(&mut s);
            });
        }
    }
}

fn handle_mock_smtp(s: &mut TcpStream) -> std::io::Result<()> {
    s.write_all(b"220 mock.smtp ESMTP Mock\r\n")?;
    let mut buf = [0u8; 1024];
    loop {
        let n = s.read(&mut buf)?;
        if n == 0 {
            break;
        }
        let line = String::from_utf8_lossy(&buf[..n]);
        let upper = line.to_ascii_uppercase();
        if upper.starts_with("DATA") {
            s.write_all(b"354 End data with <CR><LF>.<CR><LF>\r\n")?;
            let mut accumulated = Vec::new();
            loop {
                let n = s.read(&mut buf)?;
                if n == 0 {
                    break;
                }
                accumulated.extend_from_slice(&buf[..n]);
                if accumulated.windows(5).any(|w| w == b"\r\n.\r\n") {
                    break;
                }
            }
            s.write_all(b"250 OK: queued as MOCK\r\n")?;
        } else if upper.starts_with("QUIT") {
            s.write_all(b"221 Bye\r\n")?;
            break;
        } else {
            s.write_all(b"250 OK\r\n")?;
        }
    }
    Ok(())
}

#[test]
fn email_new_with_valid_addresses() {
    let email = Email::new("from@buff.dev", "to@buff.dev", "Hello").expect("new");
    assert_eq!(email.from_addr(), "from@buff.dev");
    assert_eq!(email.to_addr(), "to@buff.dev");
    assert_eq!(email.subject(), "Hello");
    assert!(email.plain_body().is_none());
    assert!(email.html_body().is_none());
    assert!(email.attachments().is_empty());
}

#[test]
fn email_new_accepts_mailbox_with_display_name() {
    let email =
        Email::new("From <from@buff.dev>", "To <to@buff.dev>", "Subject").expect("mailbox form");
    assert_eq!(email.subject(), "Subject");
}

#[test]
fn email_new_rejects_invalid_from() {
    let err = Email::new("not-an-email", "to@buff.dev", "Subject").unwrap_err();
    assert!(matches!(err, EmailError::InvalidAddress { .. }));
}

#[test]
fn email_new_rejects_invalid_to() {
    let err = Email::new("from@buff.dev", "@nodomain", "Subject").unwrap_err();
    assert!(matches!(err, EmailError::InvalidAddress { .. }));
}

#[test]
fn email_new_accepts_empty_subject() {
    let email = Email::new("from@buff.dev", "to@buff.dev", "").expect("empty subject");
    assert_eq!(email.subject(), "");
}

#[test]
fn email_body_sets_plain_text() {
    let email = Email::new("a@buff.dev", "b@buff.dev", "S")
        .expect("new")
        .body("Hello, world!")
        .expect("body");
    assert_eq!(email.plain_body(), Some("Hello, world!"));
    assert!(email.html_body().is_none());
}

#[test]
fn email_html_renders_template() {
    let email = Email::new("a@buff.dev", "b@buff.dev", "S")
        .expect("new")
        .html("<h1>Hi {{name}}</h1>", r#"{"name":"Buff"}"#)
        .expect("html");
    assert_eq!(email.html_body(), Some("<h1>Hi Buff</h1>"));
    assert!(email.plain_body().is_none());
}

#[test]
fn email_html_invalid_template_returns_template_parse_error() {
    let err = Email::new("a@buff.dev", "b@buff.dev", "S")
        .expect("new")
        .html("{{ unclosed", r#"{}"#)
        .unwrap_err();
    assert!(matches!(err, EmailError::TemplateParse(_)));
}

#[test]
fn email_html_invalid_json_context_returns_template_render_error() {
    let err = Email::new("a@buff.dev", "b@buff.dev", "S")
        .expect("new")
        .html("Hi {{name}}", "{not json}")
        .unwrap_err();
    assert!(matches!(err, EmailError::TemplateRender(_)));
}

#[test]
fn email_attach_queues_path() {
    let email = Email::new("a@buff.dev", "b@buff.dev", "S")
        .expect("new")
        .attach("/tmp/does-not-need-to-exist-yet.txt")
        .expect("attach");
    assert_eq!(email.attachments().len(), 1);
    assert_eq!(
        email.attachments()[0].to_string_lossy().replace('\\', "/"),
        "/tmp/does-not-need-to-exist-yet.txt"
    );
}

#[test]
fn email_builder_chain_composes() {
    let email = Email::new("a@buff.dev", "b@buff.dev", "Welcome")
        .expect("new")
        .body("plain")
        .expect("body")
        .html("<b>{{x}}</b>", r#"{"x":"html"}"#)
        .expect("html")
        .attach("/tmp/x.txt")
        .expect("attach");
    assert_eq!(email.plain_body(), Some("plain"));
    assert_eq!(email.html_body(), Some("<b>html</b>"));
    assert_eq!(email.attachments().len(), 1);
}

#[test]
fn smtp_client_new_rejects_empty_relay() {
    let err = SmtpClient::new("", 587, "u", "p").unwrap_err();
    assert!(matches!(err, EmailError::InvalidRelay(_)));
}

#[test]
fn smtp_client_new_accepts_localhost() {
    let _client = SmtpClient::new("127.0.0.1", 25, "u", "p").expect("localhost accept");
}

#[test]
fn smtp_client_send_plain_text_via_mock_smtp() {
    let port = mock_smtp_port();
    let client = SmtpClient::new("127.0.0.1", port, "mockuser", "mockpass").expect("client");
    let email = Email::new("from@buff.dev", "to@buff.dev", "Mock Test")
        .expect("new")
        .body("Hello mock server!")
        .expect("body");
    let result = client.send(&email);
    assert!(result.is_ok(), "send should succeed: {:?}", result.err());
}

#[test]
fn smtp_client_send_html_via_mock_smtp() {
    let port = mock_smtp_port();
    let client = SmtpClient::new("127.0.0.1", port, "mockuser", "mockpass").expect("client");
    let email = Email::new("from@buff.dev", "to@buff.dev", "HTML Mock")
        .expect("new")
        .html("<p>{{greeting}}</p>", r#"{"greeting":"Hi"}"#)
        .expect("html");
    let result = client.send(&email);
    assert!(result.is_ok(), "send should succeed: {:?}", result.err());
}

#[test]
fn smtp_client_send_attachment_via_mock_smtp() {
    let port = mock_smtp_port();
    let client = SmtpClient::new("127.0.0.1", port, "mockuser", "mockpass").expect("client");
    let tmp = std::env::temp_dir().join(format!("buff-email-test-{}.txt", std::process::id()));
    std::fs::write(&tmp, b"attachment bytes").expect("write temp");
    let email = Email::new("from@buff.dev", "to@buff.dev", "Attach Mock")
        .expect("new")
        .body("see attachment")
        .expect("body")
        .attach(tmp.to_str().expect("utf8"))
        .expect("attach");
    let result = client.send(&email);
    let _ = std::fs::remove_file(&tmp);
    assert!(result.is_ok(), "send should succeed: {:?}", result.err());
}

#[test]
fn email_build_message_fails_when_empty() {
    let email = Email::new("a@buff.dev", "b@buff.dev", "Empty").expect("new");
    let err = email.build_message().unwrap_err();
    assert!(matches!(err, EmailError::Build(_)));
}

#[test]
fn email_build_message_fails_on_unreadable_attachment() {
    let email = Email::new("a@buff.dev", "b@buff.dev", "S")
        .expect("new")
        .body("text")
        .expect("body")
        .attach("/nonexistent/path/to/missing-attachment-file.txt")
        .expect("attach");
    let err = email.build_message().unwrap_err();
    assert!(matches!(err, EmailError::AttachmentIo(_, _)));
}

#[test]
fn snapshot_email_display() {
    let email = Email::new("from@buff.dev", "to@buff.dev", "Welcome")
        .expect("new")
        .body("Hi")
        .expect("body")
        .attach("/tmp/a.txt")
        .expect("attach");
    insta::assert_snapshot!("email_display", format!("{email}"));
}

#[test]
fn snapshot_email_error_debug() {
    let err1 = EmailError::InvalidAddress {
        addr: "bad".to_string(),
        reason: "missing @".to_string(),
    };
    let err2 = EmailError::Smtp("connection refused".to_string());
    let err3 = EmailError::TemplateRender("missing variable `name`".to_string());
    insta::assert_snapshot!("email_error_debug", format!("{err1}\n{err2}\n{err3}"));
}

#[test]
fn snapshot_smtp_client_debug() {
    let port = mock_smtp_port();
    let client = SmtpClient::new("127.0.0.1", port, "u", "p").expect("client");
    insta::assert_snapshot!("smtp_client_debug", format!("{client:?}"));
}

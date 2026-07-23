// T42 example: email with file attachment.
//
// Demonstrates the `email.attach(path)` builder method. The file is
// written to a temp path, attached to the email, then sent through a
// local mock SMTP server. The MIME assembly wraps the plain-text body
// + the attachment in `multipart/mixed` automatically.

use buff_email::{Email, SmtpClient};
use std::io::{Read, Write};
use std::net::TcpListener;
use std::thread;

fn main() {
    let port = spin_local_mock_smtp();
    println!("mock SMTP listening on 127.0.0.1:{port}");

    let tmp =
        std::env::temp_dir().join(format!("buff-email-attachment-{}.txt", std::process::id()));
    std::fs::write(&tmp, b"Hello from the attached file!").expect("write temp");
    println!("wrote temp attachment to: {}", tmp.display());

    let email = Email::new("noreply@buff.dev", "user@example.com", "Invoice attached")
        .expect("email")
        .body("Please find your invoice attached.")
        .expect("body")
        .attach(tmp.to_str().expect("utf8 path"))
        .expect("attach");
    println!("built: {email}");

    let client = SmtpClient::new("127.0.0.1", port, "mockuser", "mockpass").expect("smtp client");
    client.send(&email).expect("send");
    println!("sent email with attachment.");
    let _ = std::fs::remove_file(&tmp);
}

fn spin_local_mock_smtp() -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let port = listener.local_addr().expect("addr").port();
    thread::spawn(move || {
        for stream in listener.incoming() {
            if let Ok(mut s) = stream {
                thread::spawn(move || {
                    let _ = s.write_all(b"220 mock\r\n");
                    let mut buf = [0u8; 1024];
                    loop {
                        let n = match s.read(&mut buf) {
                            Ok(0) | Err(_) => break,
                            Ok(n) => n,
                        };
                        let line = String::from_utf8_lossy(&buf[..n]).to_ascii_uppercase();
                        if line.starts_with("DATA") {
                            let _ = s.write_all(b"354 send\r\n");
                            loop {
                                let n = match s.read(&mut buf) {
                                    Ok(0) | Err(_) => return,
                                    Ok(n) => n,
                                };
                                if buf[..n].windows(5).any(|w| w == b"\r\n.\r\n") {
                                    break;
                                }
                            }
                            let _ = s.write_all(b"250 OK\r\n");
                        } else if line.starts_with("QUIT") {
                            let _ = s.write_all(b"221 bye\r\n");
                            break;
                        } else {
                            let _ = s.write_all(b"250 OK\r\n");
                        }
                    }
                });
            }
        }
    });
    port
}

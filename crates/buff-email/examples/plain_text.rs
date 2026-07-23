// T42 example: plain-text email via SMTP.
//
// Demonstrates the basic Email + SmtpClient pipeline on a local mock
// SMTP server (so the example runs end-to-end without real mailtrap
// credentials). Builds a plain-text welcome email + sends it through
// a SmtpClient configured for STARTTLS.

use buff_email::{Email, SmtpClient};
use std::io::{Read, Write};
use std::net::TcpListener;
use std::thread;

fn main() {
    let port = spin_local_mock_smtp();
    println!("mock SMTP listening on 127.0.0.1:{port}");

    let email = Email::new("noreply@buff.dev", "user@example.com", "Welcome to Buff")
        .expect("email")
        .body("Welcome to Buff! We're glad to have you.")
        .expect("body");
    println!("built: {email}");

    let client = SmtpClient::new("127.0.0.1", port, "mockuser", "mockpass").expect("smtp client");
    client.send(&email).expect("send");
    println!("sent.");
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

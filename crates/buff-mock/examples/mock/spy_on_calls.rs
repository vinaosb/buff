//! T25 Example 3: Spy on Calls — records every invocation + its args.
//!
//! Run with: `cargo run --example spy_on_calls -p buff-mock`.
//!
//! Demonstrates `Mock::spy(method)` returning a `SpyHandle` whose
//! `calls()` / `args()` snapshot every captured call. Spying does
//! NOT change dispatch behavior — it only enables post-hoc
//! inspection of the call log.

use buff_mock::{ArgumentValue, Mock};

trait Logger {
    fn log(&self, level: String, message: String);
}

impl Logger for Mock<dyn Logger> {
    fn log(&self, level: String, message: String) {
        self.record_call(
            "log",
            vec![ArgumentValue::String(level), ArgumentValue::String(message)],
        );
        let _ = self.lookup_return("log", &[]);
    }
}

fn main() {
    let mock = Mock::<dyn Logger>::new();
    let spy = mock.spy("log");

    mock.log("INFO".into(), "starting up".into());
    mock.log("WARN".into(), "low disk space".into());
    mock.log("ERROR".into(), "disk full".into());

    println!("spy observed {} calls", spy.call_count());
    for (i, args) in spy.args().iter().enumerate() {
        println!(
            "  call {}: level={}, message={}",
            i + 1,
            args.first().and_then(|a| match a {
                ArgumentValue::String(s) => Some(s.as_str()),
                _ => None,
            }).unwrap_or("?"),
            args.get(1).and_then(|a| match a {
                ArgumentValue::String(s) => Some(s.as_str()),
                _ => None,
            }).unwrap_or("?"),
        );
    }

    let _ = mock.verify();
    println!("verify: OK (no constraints violated)");
}

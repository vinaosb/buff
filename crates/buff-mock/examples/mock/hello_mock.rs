//! T25 Example 1: Hello Mock — the minimal end-to-end Mock usage.
//!
//! Run with: `cargo run --example hello_mock -p buff-mock`.
//!
//! Demonstrates the core Mock lifecycle:
//! 1. Define a trait (`Greeter`).
//! 2. Manually implement it for `Mock<dyn Greeter>` (this block is
//!    what `lower_mock_for_trait` generates automatically).
//! 3. In `main`, create a Mock, program an expectation with
//!    `expect().returning(...)`, invoke the mock, and `verify()`.

use buff_mock::{ArgumentValue, Mock, ReturnValue};

trait Greeter {
    fn greet(&self, name: String) -> String;
}

impl Greeter for Mock<dyn Greeter> {
    fn greet(&self, name: String) -> String {
        self.record_call("greet", vec![ArgumentValue::String(name)]);
        match self.lookup_return("greet", &[]) {
            Some(ReturnValue::String(s)) => s,
            _ => String::new(),
        }
    }
}

fn main() {
    let mock = Mock::<dyn Greeter>::new();
    mock.expect("greet")
        .returning(ReturnValue::String("hello world".into()));

    let result = mock.greet("buff".into());
    println!("{result}");

    match mock.verify() {
        Ok(()) => println!("verify: OK"),
        Err(e) => println!("verify: FAIL - {e}"),
    }
}

//! T25 Example 2: Verify Interaction — detects unmet expectations.
//!
//! Run with: `cargo run --example verify_interaction -p buff-mock`.
//!
//! Demonstrates `Mock::verify()` flagging a `times(2)` expectation
//! that was called only once. The verify error message includes
//! both the expected count and the observed count.

use buff_mock::{Mock, ReturnValue};

trait Counter {
    fn increment(&self) -> i64;
}

impl Counter for Mock<dyn Counter> {
    fn increment(&self) -> i64 {
        self.record_call_no_args("increment");
        match self.lookup_return_no_args("increment") {
            Some(ReturnValue::Int(i)) => i,
            _ => 0,
        }
    }
}

fn main() {
    let mock = Mock::<dyn Counter>::new();
    mock.expect("increment").times(2);

    let _ = mock.increment();
    println!("Called increment once; expected twice.");

    match mock.verify() {
        Ok(()) => println!("verify: OK (unexpected — should have failed)"),
        Err(e) => println!("verify correctly failed:\n  {e}"),
    }
}

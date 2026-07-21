//! T27 Example 1: Fuzz an integer property.
//!
//! Run with: `cargo run --example fuzz_int_property -p buff-fuzz`.
//!
//! Demonstrates the core `buff-fuzz` lifecycle:
//! 1. Build a `Strategy::int(0, 1000)`.
//! 2. Run the property `n * n >= 0` 512 times.
//! 3. Print the summary — the property should pass for every input
//!    (the square of any i64 is non-negative when there is no overflow).

use buff_fuzz::{run, Strategy};

fn main() {
    let strategy = Strategy::int(0, 1000);
    let summary = run(&strategy, 512, |n| n * n >= 0).expect("fuzz run failed");

    println!("strategy:     {strategy}");
    println!("iterations:   {}", summary.iterations);
    println!("failed count: {}", summary.failed_count());
    if summary.passed() {
        println!("result:       PASS (property holds for every input)");
    } else {
        println!("result:       FAIL");
        println!("failures:     {:?}", summary.failures);
    }
}

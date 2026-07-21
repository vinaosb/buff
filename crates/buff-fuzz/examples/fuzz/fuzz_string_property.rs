//! T27 Example 2: Fuzz a string-like property.
//!
//! Run with: `cargo run --example fuzz_string_property -p buff-fuzz`.
//!
//! Demonstrates the non-Int strategy surface: a `Strategy::string(64)`
//! generates random lengths in `0..=64`. The property checks "length
//! fits in a u8" — every generated length must fit in a single byte
//! (always true for `max_len <= 255`). The runner projects the string
//! strategy onto `i64` (the length) so the same closure shape works
//! as for `Strategy::int`.

use buff_fuzz::{run, Strategy};

fn main() {
    let strategy = Strategy::string(64);
    let summary = run(&strategy, 256, |len| len >= 0 && len <= 255).expect("fuzz run failed");

    println!("strategy:     {strategy}");
    println!("iterations:   {}", summary.iterations);
    println!("failed count: {}", summary.failed_count());
    if summary.passed() {
        println!("result:       PASS (every length fits in a u8)");
    } else {
        println!("result:       FAIL");
        println!("failures:     {:?}", summary.failures);
    }

    let failing_strategy = Strategy::int(0, 100);
    let failing_summary = run(&failing_strategy, 256, |n| n < 50).expect("fuzz run failed");
    println!();
    println!("counter-example search:");
    println!("  strategy:     {failing_strategy}");
    println!("  property:     n < 50");
    println!("  iterations:   {}", failing_summary.iterations);
    println!("  failed count: {}", failing_summary.failed_count());
    if !failing_summary.passed() {
        println!("  failures:     {:?}", failing_summary.failures);
        println!("  result:       FAIL (counter-examples found, as expected)");
    }
}

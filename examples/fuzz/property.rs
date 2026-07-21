// Buff T27 example — the canonical Rust equivalent of property.buff.
// Side-by-side comparison: same property, same strategy, same iteration
// count. The Buff surface lowers to this Rust shape via
// `buff-lang-codegen-rust`'s `Fuzz.run` + `Strategy.int` arms.

use buff_fuzz::{run, FuzzSummary, Strategy};

fn main() {
    let s = Strategy::int(0, 1000);
    let summary: FuzzSummary = run(&s, 256, |n| n * n >= 0).expect("fuzz run failed");
    println!("iterations:   {}", summary.iterations);
    println!("failed count: {}", summary.failed_count());
    if summary.passed() {
        println!("result:       PASS (n*n >= 0 for every input)");
    } else {
        println!("result:       FAIL");
    }
}

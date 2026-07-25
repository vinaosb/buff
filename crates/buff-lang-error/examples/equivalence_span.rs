//! Behavioral equivalence test: Rust original vs Buff port (span.buff).
//!
//! This binary produces the SAME output as `selfhost/span.buff`'s `main()`.
//! The equivalence harness runs both and diffs stdout.
//!
//! Run: `cargo run -p buff-lang-error --example equivalence_span`
//! Expected output: `10\n20\n1\n0\n0\n0`

use buff_lang_error::{SourceId, Span};

fn main() {
    // Test: Span::new produces correct fields
    let sid = SourceId(1);
    let sp = Span::new(10, 20, sid);
    println!("{}", sp.start);
    println!("{}", sp.end);
    println!("{}", sp.source_id.0);

    // Test: Span::dummy produces zero span
    let dummy = Span::dummy();
    println!("{}", dummy.start);
    println!("{}", dummy.end);
    println!("{}", dummy.source_id.0);
}

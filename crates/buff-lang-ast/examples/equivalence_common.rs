//! Behavioral equivalence test: Rust original vs Buff port (common.buff).
//!
//! Mirrors the `ident_new` function from `selfhost/common.buff`.
//!
//! Run: `cargo run -p buff-lang-ast --example equivalence_common`
//! Expected output: `0\n7\n1`

use buff_lang_ast::common::Ident;
use buff_lang_error::Span;

fn main() {
    let id = Ident {
        name: "test_val".to_string(),
        span: Span::new(0, 7, buff_lang_error::SourceId(1)),
    };
    println!("{}", id.span.start);
    println!("{}", id.span.end);
    println!("{}", id.span.source_id.0);
}

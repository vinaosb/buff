// T21 example: span lifecycle with nested spans.
//
// Demonstrates span nesting, field recording, and guard drop.

use buff_observe::{Span, Tracer};

fn main() {
    Tracer::bootstrap().expect("bootstrap");

    let outer = Span::new("outer");
    outer.field("layer", "api");
    let _outer_guard = outer.enter();

    {
        let inner = Span::new("inner");
        inner.field("operation", "db_query");
        inner.field("duration_ms", 5i64);
        let _inner_guard = inner.enter();
        println!("inside inner span");
    }

    println!("back in outer span");
}

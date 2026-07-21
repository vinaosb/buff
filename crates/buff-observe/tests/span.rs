//! Span-specific integration tests for `buff-observe`.

use buff_observe::{Span, Tracer};

#[test]
fn span_create_with_name() {
    let span = Span::new("request_handler");
    span.field("method", "GET");
    span.field("path", "/api/users");
    let _guard = span.enter();
}

#[test]
fn span_multiple_fields() {
    let span = Span::new("db_query");
    span.field("query", "SELECT * FROM users");
    span.field("duration_ms", 42i64);
    span.field("cache_hit", true);
    let _guard = span.enter();
}

#[test]
fn span_guard_drop_exits() {
    let span = Span::new("outer");
    {
        let _guard = span.enter();
        let inner = Span::new("inner");
        let _inner_guard = inner.enter();
    }
}

#[test]
fn span_with_tracer_bootstrap() {
    let _ = Tracer::bootstrap();
    let span = Span::new("observed_span");
    span.field("user_id", 1001i64);
    let _guard = span.enter();
}

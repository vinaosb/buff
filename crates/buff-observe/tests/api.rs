//! Integration tests for the `buff-observe` crate.
//!
//! Covers the core API surface: Span, Counter, Histogram, Gauge, Tracer.

use buff_observe::{Counter, Gauge, Histogram, ObserveError, Span, Tracer};

#[test]
fn span_lifecycle() {
    let span = Span::new("test");
    span.field("key", "value");
    let _guard = span.enter();
}

#[test]
fn counter_accumulates() {
    let mut c = Counter::new("requests");
    assert_eq!(c.value(), 0);
    c.inc();
    c.inc();
    c.inc();
    assert_eq!(c.value(), 3);
    c.inc_by(2);
    assert_eq!(c.value(), 5);
}

#[test]
fn histogram_tracks_count_and_sum() {
    let mut h = Histogram::new("latency");
    h.observe(10.0);
    h.observe(20.0);
    h.observe(30.0);
    assert_eq!(h.count(), 3);
    assert!((h.sum() - 60.0).abs() < 1e-10);
}

#[test]
fn gauge_holds_value() {
    let mut g = Gauge::new("temperature");
    g.set(36.5);
    assert!((g.value() - 36.5).abs() < 1e-10);
    g.set(37.0);
    assert!((g.value() - 37.0).abs() < 1e-10);
}

#[test]
fn tracer_bootstrap_does_not_panic() {
    let result = Tracer::bootstrap();
    assert!(result.is_ok() || result == Err(ObserveError::AlreadyInitialized));
}

#[test]
fn observe_error_types() {
    let err = ObserveError::AlreadyInitialized;
    assert_eq!(format!("{err}"), "tracer provider already initialised");
    let err = ObserveError::Panic;
    assert_eq!(
        format!("{err}"),
        "internal panic in observability subsystem"
    );
    let err = ObserveError::Message("custom".into());
    assert_eq!(format!("{err}"), "custom");
}

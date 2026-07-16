//! Integration tests for the Deox ↔ Rust bidirectional source-map mapping (T16).
//!
//! These exercise [`SourceMap::add_mapping`], [`SourceMap::lookup_deox`], and
//! [`SourceMap::lookup_rust`] — the back-end line-mapping API that lets
//! `deox_cli::error_mapper` translate `rustc`/runtime locations back to the
//! original `.deox` source.
//!
//! Front-end lookup tests (`add_source` + `lookup(byte_offset)`) live in
//! `span_test.rs`.

use std::path::PathBuf;

use deox_error::{SourceId, SourceMap, Span};

fn make_span(start: usize, end: usize) -> Span {
    Span::new(start, end, SourceId(0))
}

// ---------------------------------------------------------------------------
// Round-trip tests
// ---------------------------------------------------------------------------

#[test]
fn test_source_map_round_trip() {
    let mut sm = SourceMap::new();
    let span = make_span(10, 20);
    sm.add_mapping(span, 15);

    // Forward: rust line → deox span.
    let looked_up = sm.lookup_deox(15);
    assert_eq!(
        looked_up,
        Some(span),
        "exact rust line lookup should return the original span"
    );

    // Reverse: deox span → rust line.
    let rust_line = sm.lookup_rust(span);
    assert_eq!(
        rust_line,
        Some(15),
        "reverse lookup should return the original rust line"
    );
}

#[test]
fn test_source_map_multiple_mappings_round_trip() {
    let mut sm = SourceMap::new();
    let span_a = make_span(0, 5);
    let span_b = make_span(10, 15);
    let span_c = make_span(20, 25);

    sm.add_mapping(span_a, 3);
    sm.add_mapping(span_b, 7);
    sm.add_mapping(span_c, 12);

    assert_eq!(sm.lookup_deox(3), Some(span_a));
    assert_eq!(sm.lookup_deox(7), Some(span_b));
    assert_eq!(sm.lookup_deox(12), Some(span_c));

    assert_eq!(sm.lookup_rust(span_a), Some(3));
    assert_eq!(sm.lookup_rust(span_b), Some(7));
    assert_eq!(sm.lookup_rust(span_c), Some(12));
}

// ---------------------------------------------------------------------------
// No-mapping tests
// ---------------------------------------------------------------------------

#[test]
fn test_source_map_no_mapping_returns_none() {
    let sm = SourceMap::new();
    // Empty map — everything returns None.
    assert_eq!(sm.lookup_deox(1), None);
    assert_eq!(sm.lookup_deox(100), None);
    assert_eq!(sm.lookup_rust(make_span(0, 10)), None);
}

#[test]
fn test_source_map_lookup_deox_below_all_mappings_returns_none() {
    let mut sm = SourceMap::new();
    sm.add_mapping(make_span(0, 5), 10);
    sm.add_mapping(make_span(10, 15), 20);

    // Line 5 is below both mappings (10 and 20).
    assert_eq!(sm.lookup_deox(5), None);
}

// ---------------------------------------------------------------------------
// Closest-below fallback tests
// ---------------------------------------------------------------------------

#[test]
fn test_source_map_closest_below() {
    let mut sm = SourceMap::new();
    // Only lines 10 and 20 are recorded.
    let span_10 = make_span(0, 5);
    let span_20 = make_span(10, 15);
    sm.add_mapping(span_10, 10);
    sm.add_mapping(span_20, 20);

    // Line 15 is between 10 and 20 — should map to span_10 (closest at or below).
    assert_eq!(
        sm.lookup_deox(15),
        Some(span_10),
        "line 15 should fall back to the closest mapping at or below (line 10)"
    );

    // Line 25 is above 20 — should map to span_20.
    assert_eq!(
        sm.lookup_deox(25),
        Some(span_20),
        "line 25 should fall back to line 20"
    );

    // Line 10 is exact match — should return span_10, not fall back.
    assert_eq!(sm.lookup_deox(10), Some(span_10));
}

#[test]
fn test_source_map_closest_below_with_gap() {
    let mut sm = SourceMap::new();
    let span = make_span(42, 50);
    sm.add_mapping(span, 100);

    // Line 150 should fall back to line 100.
    assert_eq!(sm.lookup_deox(150), Some(span));
    // Line 99 is below 100 — no mapping.
    assert_eq!(sm.lookup_deox(99), None);
}

// ---------------------------------------------------------------------------
// Reverse lookup tests
// ---------------------------------------------------------------------------

#[test]
fn test_source_map_lookup_rust() {
    let mut sm = SourceMap::new();
    let span1 = make_span(0, 10);
    let span2 = make_span(20, 30);

    sm.add_mapping(span1, 5);
    sm.add_mapping(span2, 12);

    assert_eq!(sm.lookup_rust(span1), Some(5));
    assert_eq!(sm.lookup_rust(span2), Some(12));

    // Span that was never recorded.
    let unrecorded = make_span(100, 110);
    assert_eq!(sm.lookup_rust(unrecorded), None);
}

#[test]
fn test_source_map_lookup_rust_after_overwrite() {
    let mut sm = SourceMap::new();
    let span = make_span(0, 10);

    sm.add_mapping(span, 5);
    assert_eq!(sm.lookup_rust(span), Some(5));

    // Overwrite: same span, different line.
    sm.add_mapping(span, 15);
    assert_eq!(
        sm.lookup_rust(span),
        Some(15),
        "reverse lookup should return the last-recorded line"
    );
}

// ---------------------------------------------------------------------------
// Utility / empty-map tests
// ---------------------------------------------------------------------------

#[test]
fn test_source_map_is_line_map_empty() {
    let mut sm = SourceMap::new();
    assert!(
        sm.is_line_map_empty(),
        "fresh source map should have empty line map"
    );

    sm.add_mapping(make_span(0, 5), 1);
    assert!(
        !sm.is_line_map_empty(),
        "source map with a mapping should not be empty"
    );
}

#[test]
fn test_source_map_front_end_still_works() {
    // Ensure adding back-end mappings doesn't break front-end add_source/lookup.
    let mut sm = SourceMap::new();
    sm.add_source(
        SourceId(0),
        PathBuf::from("test.deox"),
        "hello\nworld".to_string(),
    );
    sm.add_mapping(make_span(6, 11), 2);

    // Front-end lookup still works.
    assert_eq!(sm.lookup(SourceId(0), 0), Some((1, 1)));
    assert_eq!(sm.lookup(SourceId(0), 6), Some((2, 1)));

    // Back-end lookup also works.
    assert_eq!(sm.lookup_deox(2), Some(make_span(6, 11)));
}

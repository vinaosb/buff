// T21 example: structured observability with spans and metrics.
//
// Demonstrates the core buff-observe API: Tracer.bootstrap(),
// Span.new(), span.field(), span.enter(), Counter, Histogram, Gauge.

use buff_observe::{Counter, Gauge, Histogram, Span, Tracer};

fn main() {
    // Bootstrap the observability pipeline (console fmt layer).
    Tracer::bootstrap().expect("bootstrap");

    // Create a span with structured fields.
    let span = Span::new("request");
    span.field("method", "GET");
    span.field("path", "/api/users");
    let _guard = span.enter();

    // Create and increment a counter.
    let mut requests = Counter::new("requests_total");
    requests.inc();
    requests.inc_by(2);
    println!("counter value: {}", requests.value());

    // Record a histogram observation.
    let mut latency = Histogram::new("request_duration_ms");
    latency.observe(42.5);
    latency.observe(15.3);
    println!("histogram count: {}, sum: {}", latency.count(), latency.sum());

    // Set a gauge value.
    let mut cpu = Gauge::new("cpu_usage");
    cpu.set(0.75);
    println!("gauge value: {}", cpu.value());

    // Guard drops here — span exits.
    println!("done");
}

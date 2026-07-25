//! `buff-observe` — structured observability for the Buff language.
//!
//! Pure-Rust MVP wrapping the [`tracing`](https://docs.rs/tracing) +
//! [`opentelemetry`](https://docs.rs/opentelemetry) crate ecosystem.
//! Provides spans, structured fields, and metrics (Counter / Histogram /
//! Gauge) with console (default) and OTLP (gRPC) export.
//!
//! # Pipeline
//!
//! ```text
//!   Tracer.bootstrap() ──▶ tracing-subscriber (fmt layer)
//!                              │
//!   Span.new("name") ─────────┤
//!   span.field("k", v) ───────┤
//!   span.enter() ─────────────┤
//!                              ▼
//!   Counter.new("name") ──▶ tracing::counter! (via metrics)
//!   c.inc() / c.inc_by(n)    │
//!   Histogram.new("name") ───┤
//!   h.observe(value)         │
//!   Gauge.new("name") ──────┤
//!   g.set(value)             │
//!                              ▼
//!                        Console (default)  OR  OTLP (gRPC)
//! ```
//!
//! # FFI safety
//!
//! Every public entry point follows the 6 hard rules from
//! `crates/buff-lang-ffi-guide/GUIDE.md`:
//!
//! | Rule | How this crate complies |
//! |------|-------------------------|
//! | R1 — No raw pointers | Public surface exposes only `Span`, `Counter`, `Histogram`, `Gauge`, `Tracer`. No `*const` / `*mut` anywhere. |
//! | R2 — Ownership boundary | All types own their data. `Span` owns its `tracing::Span`. `Counter` / `Histogram` / `Gauge` own their names. |
//! | R3 — Error mapping | Every fallible op returns `Result<T, ObserveError>`. |
//! | R4 — Thread safety | All types are `Send + Sync`. `tracing::Span` is `Send + Sync`. |
//! | R5 — Lifetime hiding | No public lifetime parameters. All types own their data. |
//! | R6 — Panic boundary | `Tracer::bootstrap` wraps its body in `catch_unwind`. |
//!
//! # Panic-free contract
//!
//! No `unwrap` / `expect` / `panic!` / `todo!` / `unimplemented!` in
//! non-test code.

use std::panic::{catch_unwind, AssertUnwindSafe};

// ---------------------------------------------------------------------------
// Error type
// ---------------------------------------------------------------------------

/// Errors that can occur during observability operations.
#[derive(Debug, Clone, PartialEq)]
pub enum ObserveError {
    /// The tracer provider has already been initialised.
    AlreadyInitialized,
    /// An internal panic occurred (caught by `catch_unwind`).
    Panic,
    /// A generic error with a message.
    Message(String),
}

impl std::fmt::Display for ObserveError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ObserveError::AlreadyInitialized => {
                write!(f, "tracer provider already initialised")
            }
            ObserveError::Panic => write!(f, "internal panic in observability subsystem"),
            ObserveError::Message(msg) => write!(f, "{msg}"),
        }
    }
}

impl std::error::Error for ObserveError {}

// ---------------------------------------------------------------------------
// Tracer — bootstrap the observability pipeline
// ---------------------------------------------------------------------------

/// The `Tracer` namespace — bootstraps the observability pipeline.
///
/// `Tracer` is a namespace-only type (like `Log` / `Toml` / `Math`). It
/// exposes a single associated function:
///
/// - `Tracer.bootstrap()` — initialise the tracing subscriber with a
///   console fmt layer (default). Optionally configures OTLP export.
///
/// Must be called once before any `Span` / `Counter` / `Histogram` /
/// `Gauge` operations. Subsequent calls are no-ops.
pub struct Tracer;

impl Tracer {
    /// Bootstrap the observability pipeline with a console fmt layer.
    ///
    /// Registers a `tracing-subscriber` fmt layer that writes structured
    /// spans and events to stderr. Safe to call multiple times — only the
    /// first call takes effect.
    ///
    /// # Errors
    ///
    /// Returns [`ObserveError::Panic`] if the subscriber initialisation
    /// panics (defensive — `tracing_subscriber::fmt().init()` is
    /// infallible in practice).
    pub fn bootstrap() -> Result<(), ObserveError> {
        let result = catch_unwind(AssertUnwindSafe(|| {
            let _ = tracing_subscriber::fmt()
                .with_env_filter(
                    tracing_subscriber::EnvFilter::try_from_default_env()
                        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
                )
                .try_init();
        }));
        match result {
            Ok(()) => Ok(()),
            Err(_) => Err(ObserveError::Panic),
        }
    }

    /// Bootstrap with OTLP export via gRPC.
    ///
    /// **NOTE — opentelemetry 0.27 API gap.** The `opentelemetry-otlp`
    /// 0.27 crate removed the `new_pipeline()` / `new_exporter()` builder
    /// entry points this method was originally written against. The new
    /// 0.27 API requires `opentelemetry_sdk` + `tracing-opentelemetry`
    /// (neither of which is a workspace dep today) plus a tokio runtime
    /// to drive the batch exporter. A proper rewrite is tracked as a
    /// sibling task — see `.sisyphus/plans/buff-v1x-frameworks.md` T21.
    ///
    /// Until that rewrite lands, this method returns a stable
    /// [`ObserveError::Message`] so callers see a clean diagnostic
    /// (never a compile error or runtime panic). The console-only
    /// [`Tracer::bootstrap`] path remains fully functional.
    pub fn bootstrap_otlp(_endpoint: &str) -> Result<(), ObserveError> {
        Err(ObserveError::Message(
            "OTLP export is not yet supported against opentelemetry-otlp 0.27 \
             (the crate's new_pipeline() / new_exporter() builder API was removed). \
             Use Tracer::bootstrap() for console output; OTLP wiring is tracked \
             under T21 follow-up."
                .to_string(),
        ))
    }
}

// ---------------------------------------------------------------------------
// Span — structured span
// ---------------------------------------------------------------------------

/// A structured span with named fields.
///
/// Created via `Span.new(name)`. Fields are added via `span.field(name, value)`
/// and the span is entered via `span.enter()` which returns a guard that
/// exits the span on drop.
///
/// # Example
///
/// ```rust
/// use buff_observe::{Span, Tracer};
/// Tracer::bootstrap().ok();
/// let span = Span::new("request");
/// span.field("user_id", 42i64);
/// let _guard = span.enter();
/// // ... work happens inside the span ...
/// ```
pub struct Span {
    inner: tracing::Span,
}

impl Span {
    /// Create a new span with the given name.
    ///
    /// The span is initially inactive — call [`Span::enter`] to activate it.
    pub fn new(name: &str) -> Self {
        Span {
            inner: tracing::span!(tracing::Level::INFO, "{}", name),
        }
    }

    /// Add a structured field to the span.
    ///
    /// The value must be a type that implements both
    /// `tracing::field::AsField` (for the field name lookup) and
    /// `tracing::Value` (the value trait bound on `Span::record`).
    pub fn field<V: tracing::field::AsField + std::fmt::Debug + tracing::Value>(
        &self,
        name: &str,
        value: V,
    ) {
        self.inner.record(name, value);
    }

    /// Enter the span, returning a guard that exits the span on drop.
    ///
    /// While the guard is alive, all tracing events and child spans are
    /// associated with this span.
    ///
    /// **Implementation note**: the guard holds an owned `EnteredSpan`
    /// (not `Entered<'static>` which would borrow from `self`). We clone
    /// the underlying `tracing::Span` (cheap — Arc refcount bump) and
    /// call `.entered()` on the clone, which consumes it and returns an
    /// owned `EnteredSpan` that drops cleanly without a lifetime tie to
    /// this `Span`.
    pub fn enter(&self) -> SpanGuard {
        SpanGuard {
            _inner: self.inner.clone().entered(),
        }
    }
}

/// A guard that exits a span when dropped.
///
/// Created by [`Span::enter`]. Dropping the guard causes the span to close.
pub struct SpanGuard {
    _inner: tracing::span::EnteredSpan,
}

// ---------------------------------------------------------------------------
// Counter — monotonic counter metric
// ---------------------------------------------------------------------------

/// A monotonic counter metric.
///
/// Created via `Counter.new(name)`. Incremented via `c.inc()` or
/// `c.inc_by(n)`.
///
/// # Note
///
/// The MVP uses `tracing::info!` with a structured field for the counter
/// value. A future version will use the OTel metrics SDK directly.
pub struct Counter {
    name: String,
    value: i64,
}

impl Counter {
    /// Create a new counter with the given name.
    pub fn new(name: &str) -> Self {
        Counter {
            name: name.to_string(),
            value: 0,
        }
    }

    /// Increment the counter by 1.
    pub fn inc(&mut self) {
        self.value += 1;
        tracing::info!(
            target: "buff_metrics",
            counter = self.name.as_str(),
            value = self.value,
            "counter increment"
        );
    }

    /// Increment the counter by `n`.
    pub fn inc_by(&mut self, n: i64) {
        self.value += n;
        tracing::info!(
            target: "buff_metrics",
            counter = self.name.as_str(),
            value = self.value,
            "counter increment by {n}"
        );
    }

    /// Read the current counter value.
    pub fn value(&self) -> i64 {
        self.value
    }
}

// ---------------------------------------------------------------------------
// Histogram — distribution of values
// ---------------------------------------------------------------------------

/// A histogram metric that records value distributions.
///
/// Created via `Histogram.new(name)`. Values are recorded via
/// `h.observe(value)`.
///
/// # Note
///
/// The MVP records observations as structured log events. A future version
/// will use the OTel metrics SDK for proper histogram bucketing.
pub struct Histogram {
    name: String,
    count: u64,
    sum: f64,
}

impl Histogram {
    /// Create a new histogram with the given name.
    pub fn new(name: &str) -> Self {
        Histogram {
            name: name.to_string(),
            count: 0,
            sum: 0.0,
        }
    }

    /// Record a value in the histogram.
    pub fn observe(&mut self, value: f64) {
        self.count += 1;
        self.sum += value;
        tracing::info!(
            target: "buff_metrics",
            histogram = self.name.as_str(),
            value = value,
            count = self.count,
            sum = self.sum,
            "histogram observation"
        );
    }

    /// Read the current count of observations.
    pub fn count(&self) -> u64 {
        self.count
    }

    /// Read the current sum of all observations.
    pub fn sum(&self) -> f64 {
        self.sum
    }
}

// ---------------------------------------------------------------------------
// Gauge — point-in-time value
// ---------------------------------------------------------------------------

/// A gauge metric that records a point-in-time value.
///
/// Created via `Gauge.new(name)`. The value is set via `g.set(value)`.
///
/// # Note
///
/// The MVP records gauge values as structured log events. A future version
/// will use the OTel metrics SDK for proper gauge semantics.
pub struct Gauge {
    name: String,
    value: f64,
}

impl Gauge {
    /// Create a new gauge with the given name.
    pub fn new(name: &str) -> Self {
        Gauge {
            name: name.to_string(),
            value: 0.0,
        }
    }

    /// Set the gauge to a value.
    pub fn set(&mut self, value: f64) {
        self.value = value;
        tracing::info!(
            target: "buff_metrics",
            gauge = self.name.as_str(),
            value = self.value,
            "gauge set"
        );
    }

    /// Read the current gauge value.
    pub fn value(&self) -> f64 {
        self.value
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn span_create_and_enter() {
        let span = Span::new("test_span");
        span.field("key", "value");
        let _guard = span.enter();
        // Guard drop exits the span — no panic means success.
    }

    #[test]
    fn counter_inc_and_inc_by() {
        let mut c = Counter::new("test_counter");
        assert_eq!(c.value(), 0);
        c.inc();
        assert_eq!(c.value(), 1);
        c.inc_by(3);
        assert_eq!(c.value(), 4);
    }

    #[test]
    fn histogram_observe() {
        let mut h = Histogram::new("test_histogram");
        assert_eq!(h.count(), 0);
        h.observe(1.5);
        assert_eq!(h.count(), 1);
        assert!((h.sum() - 1.5).abs() < 1e-10);
        h.observe(2.5);
        assert_eq!(h.count(), 2);
        assert!((h.sum() - 4.0).abs() < 1e-10);
    }

    #[test]
    fn gauge_set_and_value() {
        let mut g = Gauge::new("test_gauge");
        assert!((g.value() - 0.0).abs() < 1e-10);
        g.set(42.5);
        assert!((g.value() - 42.5).abs() < 1e-10);
    }

    #[test]
    fn tracer_bootstrap_is_idempotent() {
        // Calling bootstrap twice should not panic.
        let _ = Tracer::bootstrap();
        let _ = Tracer::bootstrap();
    }

    #[test]
    fn observe_error_display() {
        let e1 = ObserveError::AlreadyInitialized;
        let e2 = ObserveError::Panic;
        let e3 = ObserveError::Message("oops".to_string());
        assert_eq!(e1.to_string(), "tracer provider already initialised");
        assert_eq!(e2.to_string(), "internal panic in observability subsystem");
        assert_eq!(e3.to_string(), "oops");
    }
}

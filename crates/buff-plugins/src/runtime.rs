//! Runtime plugin trait + supporting types.
//!
//! A [`RuntimePlugin`] hooks into the runtime observability surface
//! of `buff-lang-runtime`:
//!
//! 1. **Span lifecycle** — `on_span_enter` is called when a
//!    distributed-tracing span is entered. Used by tracing
//!    collectors that export span trees to external systems (Jaeger,
//!    Zipkin, Datadog, etc.).
//! 2. **Metric emission** — `on_metric` is called when a metric is
//!    recorded. Used by metric exporters that forward counters /
//!    histograms to external systems (Prometheus, StatsD, etc.).
//!
//! Both hooks are object-safe and dispatched via `&dyn
//! RuntimePlugin` so the [`PluginRegistry`](crate::PluginRegistry)
//! can hold a `Vec<Box<dyn RuntimePlugin>>` and fan-out a call to
//! every registered plugin in declaration order.
//!
//! # Why plugin-local span type?
//!
//! Reusing `buff_lang_error::Span` would conflate compiler source
//! spans with runtime tracing spans (they have different semantics:
//! source spans are byte offsets; tracing spans are duration
//! windows). Instead, [`PluginSpan`] is a runtime-tracing span
//! descriptor carrying the data a real exporter needs (name +
//! start-time + optional attributes).

use std::collections::BTreeMap;

/// A runtime tracing span descriptor.
///
/// Carries the data a tracing collector needs to export a span to an
/// external system: a human-readable name, a start timestamp
/// (epoch-microseconds), and an optional attributes map (string →
/// string).
///
/// Stored as owned data so the plugin can hold the span across
/// async boundaries without borrowing from the dispatch call.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluginSpan {
    /// Human-readable span name (e.g. `"http_request"`,
    /// `"db_query"`).
    pub name: String,
    /// Span start time as microseconds since the Unix epoch.
    /// Storing microseconds (rather than `SystemTime`) keeps the
    /// type `Eq` + portable across hosts.
    pub start_us: i64,
    /// Optional span attributes (string → string). Stored as
    /// `BTreeMap` so iteration is deterministic (project hard rule
    /// — never `HashMap`).
    pub attributes: BTreeMap<String, String>,
}

impl PluginSpan {
    /// Construct a span with a name + start time and no attributes.
    pub fn new(name: impl Into<String>, start_us: i64) -> Self {
        Self {
            name: name.into(),
            start_us,
            attributes: BTreeMap::new(),
        }
    }

    /// Attach an attribute (string → string).
    pub fn with_attr(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.attributes.insert(key.into(), value.into());
        self
    }
}

/// A runtime metric descriptor.
///
/// Carries the data a metric exporter needs to forward a single
/// observation to an external system: a metric name + a numeric
/// value. The type (counter / gauge / histogram) is determined by
/// the call site — the plugin-side exporter decides how to bucket /
/// aggregate based on its own configuration.
#[derive(Debug, Clone, PartialEq)]
pub struct PluginMetric {
    /// Human-readable metric name (e.g. `"http_requests_total"`,
    /// `"db_query_duration_seconds"`).
    pub name: String,
    /// Numeric value (signed so counters can decrement, gauges can
    /// go negative).
    pub value: f64,
}

impl PluginMetric {
    /// Construct a metric observation.
    pub fn new(name: impl Into<String>, value: f64) -> Self {
        Self {
            name: name.into(),
            value,
        }
    }
}

/// The runtime plugin trait.
///
/// Object-safe + `Send + Sync` so the registry can hold a
/// `Vec<Box<dyn RuntimePlugin>>`.
///
/// # Default methods
///
/// Both `on_span_enter` and `on_metric` have default no-op
/// implementations so a plugin author can implement only the hook
/// they care about (e.g. a metric-only exporter skips span events).
pub trait RuntimePlugin: Send + Sync {
    /// Human-readable name. Used in tracing logs.
    fn name(&self) -> &str;

    /// Called when a tracing span is entered. The plugin receives a
    /// borrowed [`PluginSpan`] — the host retains ownership so the
    /// span can be dispatched to multiple plugins without cloning.
    ///
    /// Default: no-op.
    fn on_span_enter(&self, _span: &PluginSpan) {}

    /// Called when a metric is recorded. The plugin receives the
    /// metric name + value as primitives (no struct allocation) so
    /// high-frequency metric paths don't pay for a `PluginMetric`
    /// construct.
    ///
    /// Default: no-op.
    fn on_metric(&self, _name: &str, _value: f64) {}
}

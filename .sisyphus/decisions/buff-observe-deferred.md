# Decision: Defer buff-observe Audit Findings (obs-001, obs-002)

**Status:** Accepted
**Date:** 2026-07-27
**Task:** P0.15 (v1.39 real-use-cases launch audit follow-up)
**Audit:** v3.2 consolidated audit report
**Findings:** obs-001, obs-002

---

## Context

The v3.2 audit (audit-observability squad) flagged two findings against
`buff-observe`, the framework crate shipped in v1.15 as part of Wave 3:

**obs-001: Missing standard metrics (Prometheus-compatible)**

`Counter`, `Histogram`, and `Gauge` are backed by `tracing::info!` calls
with structured fields, not by the OpenTelemetry metrics SDK. There is no
Prometheus exporter, no `/metrics` scrape endpoint, and no proper
histogram bucketing. The metrics exist only as structured log lines
emitted to the console subscriber.

**obs-002: No trace correlation (OpenTelemetry trace IDs not propagated)**

`Span` wraps `tracing::Span` correctly for local span nesting, but
there is no OpenTelemetry trace context propagation. The
`Tracer::bootstrap_otlp()` method explicitly returns an error:

> OTLP export is not yet supported against opentelemetry-otlp 0.27
> (the crate's new_pipeline() / new_exporter() builder API was removed).

No W3C `traceparent` headers, no cross-service correlation IDs, no
`tracing-opentelemetry` bridge layer. Spans are console-only.

The crate's own AGENTS.md marks it as **EXPERIMENTAL** and documents that
codegen lowering (wiring `Span`/`Counter`/`Histogram`/`Gauge` into the
Buff language's prelude type system) is a separate follow-up task.

---

## Decision

Defer full observability implementation. obs-001 and obs-002 remain **OPEN**
but **DEFERRED**. No source code changes to `buff-observe` at this time.

---

## Rationale

### 1. The crate shipped as a proof-of-concept, not a production system

`buff-observe` (T21) was scoped as an MVP: demonstrate the API shape
(`Tracer.bootstrap()`, `Span.new()`/`.field()`/`.enter()`, `Counter`,
`Histogram`, `Gauge`) and validate the FFI-safety constraints from the
FFI guide. It achieved that goal. The AGENTS.md is explicit that the
OTLP path has a known opentelemetry-otlp 0.27 API gap, and the fix
requires adding `opentelemetry-sdk` + `tracing-opentelemetry` as workspace
deps plus a tokio runtime for the batch exporter.

### 2. Full Prometheus/OTel integration is a significant design effort

Fixing obs-001 means either:

- Adding `opentelemetry-sdk` with the metrics pipeline (Prometheus
  exporter, proper histogram bucketing, metric registration), or
- Building a custom Prometheus-compatible `/metrics` endpoint.

Fixing obs-002 means:

- Adding `opentelemetry-sdk` + `tracing-opentelemetry` as workspace deps,
- Wiring the `tracing-subscriber` layer through the OTel bridge,
- Adding W3C trace context extraction/injection for HTTP callers,
- Ensuring the tokio runtime requirement doesn't break non-async consumers.

Both findings share the `opentelemetry-sdk` dependency, which makes them
a natural batch, but that batch is larger than a quick fix. It deserves
its own task with a proper spec, not a reactive audit remediation.

### 3. Not blocking the self-host roadmap

The v1.39 real-use-cases launch and the self-host milestone do not
require production-grade observability. The compiler itself uses `tracing`
directly (not through `buff-observe`), and the framework crates that
would consume `buff-observe` are all MVP-tier. Deferring these findings
unblocks the launch without shipping half-baked telemetry.

### 4. The stability promise covers us

Per the stability promise (ADR: `stability-promise.md`, section 2.2),
framework crates marked `experimental` may change between minor versions
with a CHANGELOG note. `buff-observe` is explicitly experimental. Users
who adopt it today are opting into an unstable surface by definition.

---

## Consequences

### What stays the same

- `buff-observe` remains in the workspace with its current API surface.
- `Tracer::bootstrap()` (console output) continues to work.
- `Span`, `Counter`, `Histogram`, `Gauge` continue to function as
  structured-log-backed primitives.
- The crate stays at version `1.0.0` in the tooling tier.

### What changes

- obs-001 status: **OPEN, DEFERRED**. Will be addressed when the
  `opentelemetry-sdk` metrics pipeline is added (requires a dedicated
  task with spec).
- obs-002 status: **OPEN, DEFERRED**. Will be addressed alongside
  obs-001 as a coordinated `opentelemetry-sdk` + `tracing-opentelemetry`
  integration task.
- The `Tracer::bootstrap_otlp()` stub error message should be updated
  when the fix lands to point to the new task number.

### What "deferred" means in audit context

"Deferred" in the v3.2 audit framework means:

1. The finding is **acknowledged as valid**. It is not dismissed,
   not-wont-fix, or false-positive.
2. The finding will **not** be addressed in the current release cycle
   (v1.39).
3. The finding **must** be tracked in a follow-up task with a clear
   acceptance criteria. It is not silently dropped.
4. The finding's status in the audit report is updated from `OPEN` to
   `OPEN (DEFERRED)` with a reference to this decision record.
5. Re-audit in a future cycle will re-check these findings.

---

## References

- **obs-001**: Missing standard metrics (Prometheus-compatible)
  in `buff-observe`. v3.2 audit, audit-observability squad.
- **obs-002**: No trace correlation (OpenTelemetry trace IDs not
  propagated). v3.2 audit, audit-observability squad.
- `crates/buff-observe/src/lib.rs` (current source, ~280 lines)
- `crates/buff-observe/AGENTS.md` (crate-level context, EXPERIMENTAL
  status, deferred codegen lowering)
- `.sisyphus/plans/buff-v1x-frameworks.md` T21 (original task spec)
- `.sisyphus/decisions/stability-promise.md` section 2.2 (experimental
  crate stability terms)
- `.sisyphus/decisions/sdk-conventions-v1x.md` section 7.2 (`stability`
  badge values: experimental/beta/stable/locked)

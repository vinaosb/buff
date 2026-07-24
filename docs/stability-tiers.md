# Buff API Stability Tiers

Every Buff API (language feature, prelude function/type, framework crate, CLI command)
lives in one of three tiers. This document is the quick-reference companion to
[COMPATIBILITY.md](../COMPATIBILITY.md) and the formal stability promise.

---

## Tier 1 — Stable

Guaranteed not to break across all `v1.X` releases. Covered by the full
COMPATIBILITY.md stability promise. Code that compiles today keeps compiling.

### Language features

| Feature | Notes |
|---|---|
| Syntax (keywords, operators, punctuation, grammar) | All 25 keywords stable. |
| Type system (Int, Float, String, Bool, Vector, Map, Option, Result, Tuple) | Inference rules stable. |
| Control flow (if/else, for, match, while, break, continue) | Including range iterators (`0..10`). |
| Functions (func, closures `{ x => ... }`) | Including implicit async propagation. |
| Structs, enums, traits, embedding | Full OOP-ergonomics surface. |
| Modules (import/export/from) | Multi-file linking. |
| `@deprecated`, `@internal`, `@bench`, `@property`, `@feature` | Attribute system. |
| `.buffhtml` SFC format | `<script>` + `<template>` + `<style>`. |
| `..` / `..=` range operators | `Range<T>` lazy iterator (T84). |
| `Decimal` fixed-point type | T89. |
| `Comptime` (Zig-inspired) | T53, v1.13. |

### Prelude free functions (21)

| Category | Functions |
|---|---|
| Math | `abs`, `min`, `max`, `sqrt`, `floor`, `ceil`, `round`, `pow` |
| Convert | `Int`, `Float`, `String`, `Bool` |
| I/O | `print`, `println`, `read_line`, `input` |
| System | `args`, `env`, `exit`, `sleep` |
| Test | `assert_eq`, `assertThat` |

### Prelude types (30)

| Type | Shape | Notes |
|---|---|---|
| `DateTime` | runtime value | chrono wrapper |
| `Date` | runtime value | chrono NaiveDate |
| `Time` | runtime value | chrono NaiveTime |
| `Duration` | runtime value | chrono TimeDelta |
| `Instant` | runtime value | std::time::Instant |
| `Regex` | runtime value | regex crate |
| `URL` | runtime value | url crate |
| `Path` | runtime value | std::path::PathBuf |
| `Process` | runtime value | std::process::Command |
| `Log` | namespace | tracing macros |
| `Toml` | namespace | toml crate |
| `Math` | namespace | std::f64 |
| `Random` | namespace | rand crate |
| `Strings` | namespace | std::str/String |
| `Args` | namespace | std::env::args |
| `Env` | namespace | std::env::var |
| `Base64` | namespace | base64 crate |
| `Hex` | namespace | hex crate |
| `URLEncode` | namespace | percent-encoding |
| `UUID` | namespace | uuid crate |
| `Yaml` | namespace | serde_yml |
| `Json` | namespace | serde_json |
| `Csv` | namespace | csv crate |
| `Dir` | namespace | std::fs + walkdir |
| `Tempfile` | namespace | tempfile crate |
| `Hash` | namespace | sha2 + md5 |
| `HMAC` | namespace | hmac + sha2 |
| `OS` | namespace | std::env::consts + num_cpus |
| `TCP` | namespace | tokio net |
| `UDP` | namespace | tokio net |
| `WebSocket` | namespace | tokio-tungstenite |
| `Channel` | namespace | buff-lang-runtime MPSC |
| `File` | namespace | std::fs |
| `Http` | namespace | reqwest::blocking |
| `Assert` | namespace | assert_eq!/assert! |
| `Range` | runtime value | std::ops::Range |

### ErrorCodes

`E10xx` (lex), `E11xx` (parse), `E12xx` (type), `E13xx` (codegen) are stable
forever. Never renumbered, reused, or silently removed.

### CLI commands

`buff build`, `buff run`, `buff check`, `buff new`, `buff init`, `buff fmt`,
`buff bench`, `buff repl`, `buff test`, `buff watch`, `buff refactor`,
`buff ui dev`, `buff ui new`, `buff ssr`, `buff coverage`, `buff deps`,
`buff add`, `buff publish`, `buff install`, `buff outdated`, `buff jupyter install`,
`buff gen`. Names and core flags are stable.

---

## Tier 2 — Stable-Experimental

Functional APIs that compile and run, but whose surface may change between minor
versions. Not covered by the full COMPATIBILITY.md stability promise until they
graduate to Tier 1. All framework crates shipped as MVPs across v1.13-v1.23 live
here.

### Framework crates (44)

| Crate | Wave | Prelude type(s) |
|---|---|---|
| `buff-dataframe` | v1.14 | `DataFrame` |
| `buff-tensor` | v1.14 | `Tensor` |
| `buff-image` | v1.14 | `Image` |
| `buff-audio` | v1.14 | `Audio` |
| `buff-ecs` | v1.14 | `World`, `Entity` |
| `buff-dsp` | v1.14 | `Signal`, `Window`, `Spectrum` |
| `buff-mock` | v1.14 | (testing utility) |
| `buff-web` | v1.15 | `Web` |
| `buff-db` | v1.15 | `Database` |
| `buff-reactive` | v1.15 | `ReactiveSignal`, `ReactiveComputed`, `ReactiveEffect` |
| `buff-audit` | v1.15 | `Audit`, `Signature` |
| `buff-observe` | v1.15 | `Observe` |
| `buff-template` | v1.15 | (template engine) |
| `buff-cache` | v1.16 | `Cache` |
| `buff-auth` | v1.16 | `Jwt`, `OAuth2Client`, `Password`, `Rbac` |
| `buff-validate` | v1.16 | `Validator` |
| `buff-resilience` | v1.16 | (retry/circuit-breaker) |
| `buff-http-client` | v1.16 | `HttpClient` |
| `buff-jobs` | v1.16 | (job queue) |
| `buff-cli` | v1.16 | (user CLI framework) |
| `buff-config` | v1.16 | `Config` |
| `buff-scrape` | v1.17 | `Document`, `Element`, `Crawler` |
| `buff-i18n` | v1.17 | `I18n` |
| `buff-archive` | v1.17 | `Archive` |
| `buff-fsm` | v1.17 | (state machine) |
| `buff-pubsub` | v1.17 | (event bus) |
| `buff-fake` | v1.17 | `Faker` |
| `buff-assertions` | v1.17 | (fluent assertions) |
| `buff-fuzz` | v1.15 | `Fuzz`, `Strategy` |
| `buff-crypto-extras` | v1.18 | `AES`, `RSA`, `ECDH`, `Argon2`, `RsaKeypair` |
| `buff-web3` | v1.18 | `Provider`, `Wallet`, `ConnectedWallet`, `Contract`, `ContractMethod` |
| `buff-chat` | v1.18 | `Bot`, `ChatMessage`, `Platform` |
| `buff-protobuf` | v1.18 | `Protobuf`, `Message` |
| `buff-xml` | v1.18 | `Xml`, `XmlElement` |
| `buff-nlp` | v1.18 | `Text`, `Language`, `StemAlgorithm` |
| `buff-geo` | v1.18 | `Point`, `LineString`, `Polygon` |
| `buff-msgpack` | v1.18 | `MsgPack` |
| `buff-science` | v1.22 | (nalgebra linear algebra) |
| `buff-pipeline` | v1.22 | (DAG + Channel pipeline) |
| `buff-ml` | v1.22 | (autodiff + layers + optimizers) |
| `buff-game` | v1.22 | (loop/assets/render) |
| `buff-actors` | v1.19 | `ActorSystem`, `ActorRef`, `Supervisor`, `ChildSpec`, `RestartStrategy` |
| `buff-simd` | v1.19 | `Simd` |
| `buff-email` | v1.17 | `Email`, `SmtpClient` |

### Deferred features (in Tier 2 crates, not yet shipped)

These are explicitly scoped as "deferred to v1.18+" in their task specs:
distributed cache (buff-cache), distributed pub/sub (buff-pubsub),
broadcast channels (Channel), parallel system scheduling (buff-ecs),
built-in middleware (buff-web), compile-time SQL validation (buff-db),
GPU dispatch for tensors/matmul (buff-tensor), real-time audio playback
(buff-audio).

---

## Tier 3 — Unstable

Actively developed. Subject to change without notice. No compatibility
guarantee. Not yet suitable for production use.

### Active development items

| Item | Notes |
|---|---|
| Self-host ports | Buff compiler compiling Buff source (future) |
| GPU dispatch refinements | WGSL shader output is stable in semantics, not in text |
| Compiler passes (new analyses) | Ownership, async, recursion, exhaustiveness may gain new rules |
| Generated Rust internals | The exact Rust emitted is an implementation detail |
| Error message text | Prose changes for clarity; ErrorCode + Span are stable |
| Performance characteristics | No speed guarantee across versions |
| Edition boundaries | New editions may introduce breaking syntax (opt-in) |

---

## Graduation Process

```
Tier 3 (Unstable)
  │  After 1 minor version of stability
  ▼
Tier 2 (Stable-Experimental)
  │  After 2 minor versions + user adoption evidence
  ▼
Tier 1 (Stable)
```

Concrete criteria:
- **Tier 3 to Tier 2**: the feature must pass `buff check`, have at least one
  working example, and survive one minor release without breaking changes.
- **Tier 2 to Tier 1**: the feature must have documented API, test coverage,
  no breaking changes for 2 minor versions, and evidence of real-world usage
  (e.g., integration examples, cookbook recipes, community feedback).

---

## Deprecation

Follows the [COMPATIBILITY.md deprecation process](../COMPATIBILITY.md):
1. Feature marked with `@deprecated("use X instead")`, compiler emits warning.
2. Warning persists for at least 2 minor versions.
3. Feature removed in next major version (2.0.0) or at an edition boundary.

---

## Cross-References

- [COMPATIBILITY.md](../COMPATIBILITY.md) — formal stability contract
- [CHANGELOG.md](../CHANGELOG.md) — what changed per version
- [`.sisyphus/decisions/stability-promise.md`](../.sisyphus/decisions/stability-promise.md) — design rationale

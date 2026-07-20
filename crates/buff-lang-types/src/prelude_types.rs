//! The Buff **prelude-types** registry (T124b).
//!
//! This is the *general, extensible* companion to [`crate::prelude`]:
//! where [`crate::prelude`] registers free functions (`abs`, `print`, ...),
//! this module registers **types with associated functions and instance
//! methods** — the `Type.method()` / `recv.method()` shape.
//!
//! # Why this exists
//!
//! Before T124b the prelude supported only free-function calls resolved by
//! bare identifier (`print(x)`, `abs(-5)`). The expanding v1.4 stdlib needs
//! a richer surface: `DateTime.now()`, `dt.format("%Y-%m-%d")`,
//! `Duration.days(7)`, and similar `Type.method()` / instance-method
//! patterns. Rather than hard-coding DateTime-specific logic in the
//! inferencer and codegen, this module establishes a **registry** that
//! future tasks (Regex, Math, URL, Hash, ...) extend by appending entries.
//!
//! # Design
//!
//! The registry is split into three flat enums + a few lookup helpers, so
//! the type inferencer and Rust codegen can each consume it without
//! reaching into DateTime-specific code paths:
//!
//! - [`PreludeType`] — the prelude type itself (`DateTime`, `Duration`, ...).
//! - [`PreludeAssocFn`] — an associated function callable as
//!   `Type.method(args)` (`DateTime.now()`, `Duration.days(n)`).
//! - [`PreludeInstanceFn`] — an instance method callable as
//!   `recv.method(args)` (`dt.format("%Y-%m-%d")`, `dt.year()`).
//!
//! Lookup helpers ([`prelude_type_lookup`], [`assoc_fn_lookup`],
//! [`instance_fn_lookup`]) take a name and return the matching enum
//! variant, letting the consumer dispatch on a small, exhaustive match.
//!
//! # Return-type inference
//!
//! [`assoc_fn_return_type`] and [`instance_fn_return_type`] produce the
//! resolved [`Type`] for a given call. They are pure functions over the
//! resolved argument-type slice — the caller is responsible for inferring
//! the arg types first.
//!
//! # Adding a new prelude type (v1.4+ tasks)
//!
//! 1. Add a variant to [`PreludeType`] + its `name()` + `ALL` entry.
//! 2. Add a constructor on [`crate::ty::Type`] (e.g. `Type::regex()`).
//! 3. Add the matching variant to [`PreludeAssocFn`] and/or
//!    [`PreludeInstanceFn`] for the methods the type exposes.
//! 4. Extend the `name()` / `lookup` / return-type matches accordingly.
//! 5. Lower the new variants in `crates/buff-lang-codegen-rust/src/rust_codegen.rs`'s
//!    `lower_method_call` + `buff_type_to_syn`.
//!
//! No core inferencer or codegen changes are required — they already
//! consult this registry by name.

use crate::ty::Type;

// ---------------------------------------------------------------------------
// Prelude types
// ---------------------------------------------------------------------------

/// A prelude type with associated functions / instance methods.
///
/// Members of this enum are the *type names* the user writes in Buff source
/// (`DateTime`, `Duration`, ...). They are NOT reserved keywords — like
/// `Option` and `Result`, they resolve as built-in prelude types via name
/// lookup, and shadowing them with a user-defined type of the same name is
/// the user's responsibility (and a documented footgun, identical to
/// shadowing `print` with a user `print` function).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PreludeType {
    /// `DateTime` — a timezone-aware date+time. Wraps `chrono::DateTime<chrono::Utc>`.
    DateTime,
    /// `Date` — a calendar date without time or timezone. Wraps `chrono::NaiveDate`.
    Date,
    /// `Time` — a clock time without date or timezone. Wraps `chrono::NaiveTime`.
    Time,
    /// `Duration` — a span of time. Wraps `chrono::TimeDelta`.
    Duration,
    /// `Instant` — a monotonic instant for elapsed-time measurement. Wraps
    /// `std::time::Instant`. Distinct from [`Self::DateTime`] (wall-clock).
    Instant,
    /// `Log` — the structured-logging namespace (T124c). Wraps the
    /// `tracing` + `tracing-subscriber` Rust crates. Unlike the other
    /// variants, `Log` is **never a runtime value** — it's a NAMESPACE
    /// that exposes associated functions `Log.debug(msg, ...)`,
    /// `Log.info(msg, ...)`, `Log.warn(msg, ...)`, `Log.error(msg, ...)`.
    /// Each call lowers to the corresponding `tracing::<level>!(...)`
    /// macro. `buff_type()` returns [`Type::Void`] (Log has no value
    /// representation); the `is_prelude_datetime` predicate returns
    /// `false` for it. This is the precedent for future namespace-only
    /// prelude modules (e.g. `Process`, `Cli`).
    Log,
    /// `Regex` — a compiled regular expression (T124d). Wraps the
    /// `regex::Regex` Rust crate. Constructed via the associated function
    /// `Regex.compile(pattern)`; supports the instance methods
    /// `regex.match(text)` (→ `Option<...>`), `regex.find(text)`
    /// (→ `Option<String>`), `regex.replace(text, repl)` (→ `String`),
    /// `regex.captures(text)` (→ `Map<String, String>`).
    ///
    /// This is the FIRST v1.4 prelude type that is BOTH a real runtime
    /// value (like DateTime/Date/Time/Duration/Instant) AND carries
    /// non-trivial instance methods. DateTime's instance methods
    /// (`format`/`year`/...) are mostly accessors; Regex's instance
    /// methods are the primary surface (a compiled regex is mostly
    /// useful as a receiver). This is the precedent for future
    /// runtime-value-with-rich-methods types (e.g. `Url`, `Hasher`,
    /// `Connection`).
    ///
    /// `buff_type()` returns [`Type::Regex`] (a real value type, NOT
    /// [`Type::Void`] like `Log`). `is_namespace_only()` returns `false`
    /// (Regex IS a runtime value); `is_prelude_datetime()` returns
    /// `false` (Regex is not a chrono type — see [`Type::is_prelude_regex`]).
    Regex,
    /// `Toml` — the TOML serialization namespace (T124e). Wraps the
    /// `toml` Rust crate. Like [`Self::Log`], `Toml` is **never a
    /// runtime value** — it's a NAMESPACE that exposes two associated
    /// functions:
    /// - `Toml.parse(string)` — parse a TOML document into a Buff `Map`
    ///   (heterogeneous values); lowers to
    ///   `toml::from_str::<std::collections::HashMap<String,
    ///   toml::Value>>(s).unwrap_or_default()`.
    /// - `Toml.stringify(value)` — serialize a Map/value back to TOML
    ///   text; lowers to `toml::to_string(&v).unwrap_or_default()`.
    ///
    /// This is critical because Buff's own `buff.toml` project config is
    /// TOML — exposing a TOML module in the prelude lets Buff programs
    /// read/write their own project files. `buff_type()` returns
    /// [`Type::Void`] (Toml has no value representation, exactly like
    /// `Log`); `is_namespace_only()` returns `true`. This is the second
    /// namespace-only prelude module after `Log` (T124c) and the
    /// established precedent for future namespace-only modules
    /// (`Process`, `Cli`, `Http`, ...).
    Toml,
    /// `Math` - the floating-point math namespace (T124f). Wraps Rust's
    /// `std::f64` methods + `std::f64::consts` constants. Like
    /// [`Self::Log`] / [`Self::Toml`], `Math` is **never a runtime
    /// value** - it's a NAMESPACE exposing associated functions
    /// (`Math.sqrt(x)`, `Math.sin(x)`, `Math.pow(b, e)`, ...) AND two
    /// associated CONSTANTS (`Math.PI`, `Math.E`). The constants use
    /// the dedicated [`PreludeAssocConst`] registry (the first
    /// associated-constant prelude mechanism). `buff_type()` returns
    /// [`Type::Void`]; `is_namespace_only()` returns `true`. Math uses
    /// only Rust `std` (NO extern crate needed).
    Math,
    /// `Random` - the random-number namespace (T124f). Wraps the `rand`
    /// Rust crate. Like [`Self::Log`] / [`Self::Toml`], `Random` is
    /// **never a runtime value** - it's a NAMESPACE exposing four
    /// associated functions: `Random.int(min, max)` (inclusive Int
    /// range), `Random.float()` (f64 in `[0, 1)`), `Random.choice(vec)`
    /// (Option<element>), `Random.shuffle(vec)` (returns shuffled Vec).
    /// `buff_type()` returns [`Type::Void`]; `is_namespace_only()`
    /// returns `true`. **Not cryptographically secure** - the plan
    /// forbids CSPRNG here (deferred to a future Hash/Crypto module).
    /// The `rand` crate is recorded in codegen `extern_crates` when a
    /// program uses `Random` (codegen-only linking boundary - same as
    /// chrono/toml/regex/tracing).
    Random,
    /// `Strings` - the string-utilities namespace (T124f). Wraps
    /// Rust's `str` / `String` methods as functional module calls
    /// (`Strings.split(text, sep)`, `Strings.join(vec, sep)`, ...).
    /// Like [`Self::Log`] / [`Self::Toml`], `Strings` is **never a
    /// runtime value** - it's a NAMESPACE exposing eight associated
    /// functions. Some of these methods exist as instance methods on
    /// Buff's String type; exposing them as a module enables
    /// functional-style call chains (e.g.
    /// `Strings.trim(Strings.uppercase(s))`). `buff_type()` returns
    /// [`Type::Void`]; `is_namespace_only()` returns `true`. Strings
    /// uses only Rust `std` (NO extern crate needed).
    Strings,
}

impl PreludeType {
    /// All prelude types, in declared order. Future v1.4 tasks append here.
    pub const ALL: &'static [PreludeType] = &[
        PreludeType::DateTime,
        PreludeType::Date,
        PreludeType::Time,
        PreludeType::Duration,
        PreludeType::Instant,
        PreludeType::Log,
        // T124d: Regex — runtime value type with rich instance methods.
        PreludeType::Regex,
        // T124e: Toml — namespace-only module wrapping the `toml` crate
        // (parse + stringify). Mirrors Log's namespace-only shape.
        PreludeType::Toml,
        // T124f: Math / Random / Strings - three namespace-only utility
        // modules (Math + Strings wrap Rust std only; Random wraps the
        // `rand` crate). All mirror Log's namespace-only shape.
        PreludeType::Math,
        PreludeType::Random,
        PreludeType::Strings,
    ];

    /// The source name of this prelude type (the identifier the user writes).
    pub const fn name(self) -> &'static str {
        match self {
            PreludeType::DateTime => "DateTime",
            PreludeType::Date => "Date",
            PreludeType::Time => "Time",
            PreludeType::Duration => "Duration",
            PreludeType::Instant => "Instant",
            PreludeType::Log => "Log",
            // T124d: the Regex prelude type name. Mirrors the Rust crate
            // name so the codegen can splice `regex::Regex::...` paths
            // without rewriting.
            PreludeType::Regex => "Regex",
            // T124e: the Toml prelude type name. Mirrors the Rust crate
            // name so the codegen can splice `toml::from_str` /
            // `toml::to_string` paths without rewriting.
            PreludeType::Toml => "Toml",
            // T124f: the Math prelude type name. Mirrors Rust's `std::f64`
            // method surface so the codegen can splice `(x as f64).sqrt()`
            // etc. without rewriting.
            PreludeType::Math => "Math",
            // T124f: the Random prelude type name. The codegen splices
            // `rand::thread_rng().gen_range(...)` etc. for the four
            // associated functions.
            PreludeType::Random => "Random",
            // T124f: the Strings prelude type name. The codegen splices
            // Rust's `str` / `String` methods (`text.split(sep)...`,
            // `vec.join(sep)`, etc.) for the eight associated functions.
            PreludeType::Strings => "Strings",
        }
    }

    /// The resolved Buff [`Type`] variant for this prelude type.
    ///
    /// For the datetime family (DateTime/Date/Time/Duration/Instant) this is
    /// the matching datetime `Type` variant. For namespace-only modules
    /// like `Log` it returns [`Type::Void`] — the namespace itself is
    /// never a value, only its associated functions are callable. For
    /// other runtime-value prelude types like `Regex` (T124d) it returns
    /// the matching opaque `Type` variant.
    pub const fn buff_type(self) -> Type {
        match self {
            PreludeType::DateTime => Type::DateTime,
            PreludeType::Date => Type::Date,
            PreludeType::Time => Type::Time,
            PreludeType::Duration => Type::Duration,
            PreludeType::Instant => Type::Instant,
            // T124c: namespace-only — Log has no value representation.
            PreludeType::Log => Type::Void,
            // T124d: Regex IS a runtime value — returns the opaque
            // compiled-regex type (mapped to `regex::Regex` at codegen
            // time). Distinct from Log (which returns Void).
            PreludeType::Regex => Type::Regex,
            // T124e: namespace-only — Toml has no value representation.
            // Mirrors Log: the namespace itself is never a value, only
            // its associated functions (`Toml.parse` / `Toml.stringify`)
            // are callable.
            PreludeType::Toml => Type::Void,
            // T124f: namespace-only - Math has no value representation.
            // Mirrors Log / Toml: the namespace itself is never a value,
            // only its associated functions (`Math.sqrt(x)`, ...) and
            // associated constants (`Math.PI`, `Math.E`) are callable.
            PreludeType::Math => Type::Void,
            // T124f: namespace-only - Random has no value representation.
            // Mirrors Log / Toml / Math: the namespace itself is never a
            // value, only its associated functions (`Random.int(lo, hi)`,
            // ...) are callable.
            PreludeType::Random => Type::Void,
            // T124f: namespace-only - Strings has no value representation.
            // Mirrors Log / Toml / Math / Random: the namespace itself is
            // never a value, only its associated functions
            // (`Strings.split(t, s)`, ...) are callable.
            PreludeType::Strings => Type::Void,
        }
    }

    /// T124c: Returns `true` if this prelude type is a **namespace-only**
    /// module — one whose name (e.g. `Log`) is never a runtime value but
    /// merely a container for associated functions. The datetime family
    /// returns `false` (their values ARE first-class); `Log` returns
    /// `true`. Used by the prelude-types tests to skip the datetime-only
    /// `is_prelude_datetime` assertion for namespace modules.
    pub const fn is_namespace_only(self) -> bool {
        matches!(
            self,
            PreludeType::Log | PreludeType::Toml | PreludeType::Math | PreludeType::Random
                | PreludeType::Strings
        )
    }
}

/// Returns `true` iff `name` is a recognised prelude-type name.
///
/// Used by both the type inferencer (to resolve a `TypeRef::Named("DateTime")`
/// annotation to the matching [`Type`] variant) and the Rust codegen (to
/// decide whether a `Type.method()` AST node names a prelude associated
/// function).
pub fn is_prelude_type(name: &str) -> bool {
    prelude_type_lookup(name).is_some()
}

/// Look up a prelude type by its source name. Returns `None` for
/// unrecognised names (including user-defined types).
pub fn prelude_type_lookup(name: &str) -> Option<PreludeType> {
    PreludeType::ALL.iter().copied().find(|t| t.name() == name)
}

// ---------------------------------------------------------------------------
// Associated functions: `Type.method(args)`
// ---------------------------------------------------------------------------

/// A recognised **associated function** on a prelude type — the
/// `Type.method(args)` call shape. The receiver is the type name itself
/// (a bare `Expr::Ident`), so this enum's variants cover everything callable
/// that way.
///
/// # Naming convention
///
/// Variants are named after the *method name*, not the type — multiple
/// types can share a method name (e.g. `DateTime.now()` and
/// `Instant.now()` both map to [`Self::Now`]). The dispatch on
/// `(PreludeType, PreludeAssocFn)` pairs is exhaustive in
/// [`assoc_fn_return_type`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PreludeAssocFn {
    // ---- Time-constructors -----------------------------------------------
    /// `DateTime.now()` / `Instant.now()` — current time. No args.
    Now,
    /// `Date.today()` — current calendar date. No args.
    Today,
    // ---- Parsing ---------------------------------------------------------
    /// `DateTime.parse(s)` / `Date.parse(s)` — parse an ISO 8601 / RFC 3339
    /// string. One arg (the string).
    Parse,
    // ---- Duration constructors ------------------------------------------
    /// `Duration.days(n)` — span of `n` whole days. One arg (Int).
    Days,
    /// `Duration.hours(n)`. One arg (Int).
    Hours,
    /// `Duration.minutes(n)`. One arg (Int).
    Minutes,
    /// `Duration.seconds(n)`. One arg (Int).
    Seconds,
    /// `Duration.millis(n)`. One arg (Int).
    Millis,
    // ---- Log levels (T124c) ---------------------------------------------
    /// `Log.debug(msg, ...)`. Wraps `tracing::debug!`. Variadic: first
    /// positional arg is the message; trailing named args (`k: v`) become
    /// structured fields. Returns `Void` (Unit).
    Debug,
    /// `Log.info(msg, ...)`. Wraps `tracing::info!`. Same shape as
    /// [`Self::Debug`].
    Info,
    /// `Log.warn(msg, ...)`. Wraps `tracing::warn!`. Same shape as
    /// [`Self::Debug`].
    Warn,
    /// `Log.error(msg, ...)`. Wraps `tracing::error!`. Same shape as
    /// [`Self::Debug`].
    Error,
    // ---- Regex (T124d) -------------------------------------------------
    /// `Regex.compile(pattern)` — compile a regex pattern string into a
    /// `Regex` runtime value. One arg (the pattern String). Returns
    /// `Regex` (the codegen-lowered `regex::Regex` is fallible in Rust
    /// — `Regex::new` returns `Result<Regex, Error>` — but Buff's
    /// "no panicking generated code" + "no Result surface in the
    /// prelude-type ctor" stance (mirroring T124b's DateTime.parse
    /// lowering which uses `unwrap_or`) makes the ctor infallible at
    /// the surface: an invalid pattern yields a regex that matches
    /// nothing, never a panic. The codegen details live in
    /// `lower_prelude_type_assoc_fn`).
    Compile,
    // ---- Toml (T124e) --------------------------------------------------
    /// `Toml.stringify(value)` — serialize a Map/value back to TOML
    /// text. One arg (the value to serialize, typically a Map). Returns
    /// `String`. The codegen-lowered `toml::to_string(&v)` is fallible
    /// in Rust (`Result<String, Error>`) — Buff surfaces it as an
    /// infallible String via `.unwrap_or_default()` (NO panic — the
    /// empty string is the round-trip-failure fallback, mirroring
    /// Regex.compile / DateTime.parse's "no panicking generated code"
    /// stance from T124b/T124d).
    ///
    /// Note `Toml.parse` is NOT a new variant — it REUSES the existing
    /// [`Self::Parse`] (also used by `DateTime.parse(s)` /
    /// `Date.parse(s)`). Dispatch on `(PreludeType::Toml, Parse)` is
    /// resolved in [`assoc_fn_return_type`].
    Stringify,
    // ---- Math (T124f) --------------------------------------------------
    /// `Math.sqrt(x)` - `f64::sqrt`. One arg. Returns Float.
    Sqrt,
    /// `Math.sin(x)` - `f64::sin`. One arg. Returns Float.
    Sin,
    /// `Math.cos(x)` - `f64::cos`. One arg. Returns Float.
    Cos,
    /// `Math.tan(x)` - `f64::tan`. One arg. Returns Float.
    Tan,
    /// `Math.abs(x)` - `f64::abs`. One arg. Returns Float.
    Abs,
    /// `Math.floor(x)` - `f64::floor`. One arg. Returns Float.
    Floor,
    /// `Math.ceil(x)` - `f64::ceil`. One arg. Returns Float.
    Ceil,
    /// `Math.round(x)` - `f64::round`. One arg. Returns Float.
    Round,
    /// `Math.pow(base, exp)` - `f64::powf`. Two args. Returns Float.
    Pow,
    /// `Math.min(a, b)` - `f64::min`. Two args. Returns Float.
    Min,
    /// `Math.max(a, b)` - `f64::max`. Two args. Returns Float.
    Max,
    // ---- Random (T124f) ------------------------------------------------
    /// `Random.int(min, max)` - inclusive integer range. Two args
    /// (Int, Int). Returns Int. Wraps `rand::thread_rng().gen_range
    /// (min..=max)`.
    Int,
    /// `Random.float()` - `f64` in `[0, 1)`. Zero args. Returns Float.
    /// Wraps `rand::thread_rng().gen::<f64>()`.
    Float,
    /// `Random.choice(vec)` - pick a random element. One arg (Vector).
    /// Returns `Option<element_type>` (None on empty input - NEVER
    /// panics, matching Buff's "no panicking generated code" rule).
    /// Wraps `SliceRandom::choose(&vec, &mut rng).cloned()`.
    Choice,
    /// `Random.shuffle(vec)` - return a shuffled copy. One arg (Vector).
    /// Returns Vector<element_type> (a NEW Vec; the input is NOT
    /// mutated in the user's surface - the codegen makes a `let mut`
    /// binding internally). Wraps `SliceRandom::shuffle(&mut vec, &mut
    /// rng)`.
    Shuffle,
    // ---- Strings (T124f) -----------------------------------------------
    /// `Strings.split(text, sep)` - split text into a `Vector<String>`
    /// by separator. Two args (String, String). Returns
    /// `Vector<String>`. Wraps `text.split(sep).map(|s|
    /// s.to_string()).collect::<Vec<String>>()`.
    Split,
    /// `Strings.join(vec, sep)` - join a `Vector<String>` into a single
    /// String with separator. Two args (`Vector<String>`, String).
    /// Returns String. Wraps `vec.join(&sep)` (Borrows sep via `&` so
    /// both `'static str` and `String` inputs satisfy Rust's `&str`
    /// bound on `Vec::<String>::join`).
    Join,
    /// `Strings.trim(text)` - strip leading/trailing whitespace. One
    /// arg. Returns String. Wraps `text.trim().to_string()`.
    Trim,
    /// `Strings.replace(text, from, to)` - replace ALL occurrences of
    /// `from` in `text` with `to`. Three args (String, String, String).
    /// Returns String. Wraps `text.replace(from, to)` (Rust's
    /// `str::replace` already returns a new `String`).
    Replace,
    /// `Strings.contains(text, substr)` - test whether `text` contains
    /// `substr`. Two args (String, String). Returns Bool. Wraps
    /// `text.contains(substr)`.
    Contains,
    /// `Strings.starts_with(text, prefix)` - test whether `text`
    /// starts with `prefix`. Two args (String, String). Returns Bool.
    /// Wraps `text.starts_with(prefix)`.
    StartsWith,
    /// `Strings.to_uppercase(text)` - uppercase the text. One arg.
    /// Returns String. Wraps `text.to_uppercase().to_string()`.
    ToUppercase,
    /// `Strings.to_lowercase(text)` - lowercase the text. One arg.
    /// Returns String. Wraps `text.to_lowercase().to_string()`.
    ToLowercase,
}

impl PreludeAssocFn {
    /// All recognised associated-function names (deduplicated across types
    /// — e.g. `Now` appears once even though both `DateTime` and `Instant`
    /// expose it).
    pub const ALL: &'static [PreludeAssocFn] = &[
        PreludeAssocFn::Now,
        PreludeAssocFn::Today,
        PreludeAssocFn::Parse,
        PreludeAssocFn::Days,
        PreludeAssocFn::Hours,
        PreludeAssocFn::Minutes,
        PreludeAssocFn::Seconds,
        PreludeAssocFn::Millis,
        // T124c: Log levels — Debug / Info / Warn / Error.
        PreludeAssocFn::Debug,
        PreludeAssocFn::Info,
        PreludeAssocFn::Warn,
        PreludeAssocFn::Error,
        // T124d: Regex.compile.
        PreludeAssocFn::Compile,
        // T124e: Toml.stringify (Toml.parse reuses `Parse`).
        PreludeAssocFn::Stringify,
        // T124f: Math assoc fns (11): sqrt/sin/cos/tan/abs/floor/ceil/
        // round/pow/min/max.
        PreludeAssocFn::Sqrt,
        PreludeAssocFn::Sin,
        PreludeAssocFn::Cos,
        PreludeAssocFn::Tan,
        PreludeAssocFn::Abs,
        PreludeAssocFn::Floor,
        PreludeAssocFn::Ceil,
        PreludeAssocFn::Round,
        PreludeAssocFn::Pow,
        PreludeAssocFn::Min,
        PreludeAssocFn::Max,
        // T124f: Random assoc fns (4): int/float/choice/shuffle.
        PreludeAssocFn::Int,
        PreludeAssocFn::Float,
        PreludeAssocFn::Choice,
        PreludeAssocFn::Shuffle,
        // T124f: Strings assoc fns (8): split/join/trim/replace/contains/
        // starts_with/to_uppercase/to_lowercase.
        PreludeAssocFn::Split,
        PreludeAssocFn::Join,
        PreludeAssocFn::Trim,
        PreludeAssocFn::Replace,
        PreludeAssocFn::Contains,
        PreludeAssocFn::StartsWith,
        PreludeAssocFn::ToUppercase,
        PreludeAssocFn::ToLowercase,
    ];

    /// The source name of this associated function (the method identifier).
    pub const fn name(self) -> &'static str {
        match self {
            PreludeAssocFn::Now => "now",
            PreludeAssocFn::Today => "today",
            PreludeAssocFn::Parse => "parse",
            PreludeAssocFn::Days => "days",
            PreludeAssocFn::Hours => "hours",
            PreludeAssocFn::Minutes => "minutes",
            PreludeAssocFn::Seconds => "seconds",
            PreludeAssocFn::Millis => "millis",
            // T124c: lowercase Rust method-name spelling mirrors tracing's
            // macro names so the codegen can splice `tracing::<name>!(...)`
            // without rewriting.
            PreludeAssocFn::Debug => "debug",
            PreludeAssocFn::Info => "info",
            PreludeAssocFn::Warn => "warn",
            PreludeAssocFn::Error => "error",
            // T124d: Regex.compile — name mirrors `regex::Regex::new`'s
            // surface intent (compile a pattern) without colliding with
            // the `new` constructor convention reserved for user types
            // (`Type.new()` per §7 of the conventions).
            PreludeAssocFn::Compile => "compile",
            // T124e: Toml.stringify — canonical name for "serialize back
            // to text". Mirrors JSON.stringify from JS / `dumps` from
            // Python's `json` / `to_string` from Rust's `toml` crate.
            PreludeAssocFn::Stringify => "stringify",
            // T124f: Math - names mirror Rust's `f64` method names so
            // codegen can splice `(...).<name>(...)` without rewriting.
            PreludeAssocFn::Sqrt => "sqrt",
            PreludeAssocFn::Sin => "sin",
            PreludeAssocFn::Cos => "cos",
            PreludeAssocFn::Tan => "tan",
            PreludeAssocFn::Abs => "abs",
            PreludeAssocFn::Floor => "floor",
            PreludeAssocFn::Ceil => "ceil",
            PreludeAssocFn::Round => "round",
            // `pow` lowers to Rust's `f64::powf` (note the trailing `f`
            // distinguishing float-power from the integer-power `powi`).
            PreludeAssocFn::Pow => "pow",
            PreludeAssocFn::Min => "min",
            PreludeAssocFn::Max => "max",
            // T124f: Random - `int`/`float` are Buff-flavored names
            // (clearer than `gen_range` / `gen`); `choice` / `shuffle`
            // mirror rand's `SliceRandom` trait method names.
            PreludeAssocFn::Int => "int",
            PreludeAssocFn::Float => "float",
            PreludeAssocFn::Choice => "choice",
            PreludeAssocFn::Shuffle => "shuffle",
            // T124f: Strings - names mirror Rust's `str`/`String` method
            // names so codegen can splice `text.<name>(...)` without
            // rewriting. The `to_uppercase` / `to_lowercase` spellings
            // match Rust's `str::to_uppercase` / `to_lowercase` (no
            // underscore between `to` and the case word).
            PreludeAssocFn::Split => "split",
            PreludeAssocFn::Join => "join",
            PreludeAssocFn::Trim => "trim",
            PreludeAssocFn::Replace => "replace",
            PreludeAssocFn::Contains => "contains",
            PreludeAssocFn::StartsWith => "starts_with",
            PreludeAssocFn::ToUppercase => "to_uppercase",
            PreludeAssocFn::ToLowercase => "to_lowercase",
        }
    }
}

/// Look up a prelude associated function by the (type, method-name) pair.
///
/// Returns `None` when the combination is not a recognised prelude call
/// (e.g. `DateTime.days(7)` is invalid — `days` belongs to `Duration`).
/// This is the function the type inferencer + codegen consult to decide
/// whether a `Type.method(args)` AST node is a prelude call.
pub fn assoc_fn_lookup(type_name: &str, method: &str) -> Option<(PreludeType, PreludeAssocFn)> {
    let t = prelude_type_lookup(type_name)?;
    let m = PreludeAssocFn::ALL
        .iter()
        .copied()
        .find(|f| f.name() == method)?;
    // Validate the (type, method) pair is a recognised combination.
    assoc_fn_return_type(t, m, &[]).map(|_| (t, m))
}

// ---------------------------------------------------------------------------
// Associated constants: `Type.CONST` (T124f)
// ---------------------------------------------------------------------------

/// A recognised **associated constant** on a prelude type - the
/// `Type.CONST` access shape. The receiver is the type name itself (a
/// bare `Expr::Ident`), and the constant is accessed as a zero-arg
/// "method call" `Type.NAME` (Buff's parser produces `MethodCall` with
/// `args == []` for both `obj.field` and `obj.field()`; the codegen
/// consults this registry to decide whether `Math.PI` is a prelude
/// constant access vs. a field access on a user struct named `Math`).
///
/// # Why a separate registry
///
/// The associated-FUNCTION registry ([`PreludeAssocFn`]) dispatches
/// CALLS (`Type.method(args)`); associated CONSTANTS are accessed
/// WITHOUT parens (`Math.PI`). The codegen consults this registry in
/// the `lower_method_call` zero-arg arm BEFORE the T26 field-access
/// heuristic so a prelude constant access is rewritten to the Rust
/// path (`std::f64::consts::PI`) rather than the literal Rust field
/// access `Math.PI` (which would not compile).
///
/// # Naming convention
///
/// Variants are named after the constant's surface identifier (the
/// user-facing name). Dispatch on `(PreludeType, PreludeAssocConst)`
/// pairs is exhaustive in [`assoc_const_return_type`].
///
/// # T124f scope
///
/// Currently only the Math namespace has associated constants (`PI`,
/// `E`). Future prelude modules with constants (e.g. a future `Physics`
/// module exposing `Physics.G` for the gravitational constant) extend
/// this enum + the lookup/return-type matches.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PreludeAssocConst {
    /// `Math.PI` (pi) - the ratio of a circle's circumference to its
    /// diameter. Returns Float (lowers to `std::f64::consts::PI`).
    Pi,
    /// `Math.E` (Euler's number) - the base of the natural logarithm.
    /// Returns Float (lowers to `std::f64::consts::E`).
    E,
}

impl PreludeAssocConst {
    /// All recognised associated-constant names.
    pub const ALL: &'static [PreludeAssocConst] =
        &[PreludeAssocConst::Pi, PreludeAssocConst::E];

    /// The source name of this associated constant (the identifier the user
    /// writes after the dot). Note constant names are UPPERCASE per the
    /// Rust / Buff convention for consts.
    pub const fn name(self) -> &'static str {
        match self {
            // `PI` / `E` match Rust's `std::f64::consts::PI` / `E`
            // exactly so the codegen can splice `std::f64::consts::PI`
            // without rewriting.
            PreludeAssocConst::Pi => "PI",
            PreludeAssocConst::E => "E",
        }
    }
}

/// Look up a prelude associated constant by the (type, name) pair.
///
/// Returns `None` when the combination is not a recognised prelude
/// constant access (e.g. `Math.TAU` is invalid - TAU is not in the
/// T124f surface; `DateTime.PI` is invalid - PI belongs to Math).
/// This is the function the type inferencer + codegen consult to
/// decide whether a `Type.NAME` AST node (zero-arg method call) is a
/// prelude constant access.
pub fn assoc_const_lookup(type_name: &str, name: &str) -> Option<(PreludeType, PreludeAssocConst)> {
    let t = prelude_type_lookup(type_name)?;
    let c = PreludeAssocConst::ALL
        .iter()
        .copied()
        .find(|c| c.name() == name)?;
    // Validate the (type, const) pair is a recognised combination.
    assoc_const_return_type(t, c).map(|_| (t, c))
}

/// Infer the resolved Buff [`Type`] of a prelude associated constant.
///
/// Returns `None` for invalid `(type, const)` combinations. Currently
/// every associated constant is `Float` (Math.PI / Math.E both lower
/// to `std::f64::consts::PI` / `E` which are `f64`). Future
/// associated constants of other types (e.g. `Int`) extend this match.
pub fn assoc_const_return_type(type_: PreludeType, const_: PreludeAssocConst) -> Option<Type> {
    match (type_, const_) {
        // Math.PI / Math.E -> Float (f64).
        (PreludeType::Math, PreludeAssocConst::Pi) => Some(Type::float_default()),
        (PreludeType::Math, PreludeAssocConst::E) => Some(Type::float_default()),
        // Every other (type, const) pair is invalid.
        _ => None,
    }
}

/// Infer the return type of a prelude associated-function call.
///
/// Returns `None` for invalid `(type, method)` combinations. Argument
/// types are accepted but currently unused (every associated function in
/// the registry has a fixed return type); they are passed for
/// future-proofing and to keep the signature symmetric with the
/// free-function prelude's `return_type` helper.
pub fn assoc_fn_return_type(
    type_: PreludeType,
    method: PreludeAssocFn,
    _arg_tys: &[Type],
) -> Option<Type> {
    match (type_, method) {
        // Constructors — return the type itself.
        (PreludeType::DateTime, PreludeAssocFn::Now) => Some(Type::DateTime),
        (PreludeType::DateTime, PreludeAssocFn::Parse) => Some(Type::DateTime),
        (PreludeType::Date, PreludeAssocFn::Today) => Some(Type::Date),
        (PreludeType::Date, PreludeAssocFn::Parse) => Some(Type::Date),
        (PreludeType::Instant, PreludeAssocFn::Now) => Some(Type::Instant),
        // Duration constructors.
        (PreludeType::Duration, PreludeAssocFn::Days) => Some(Type::Duration),
        (PreludeType::Duration, PreludeAssocFn::Hours) => Some(Type::Duration),
        (PreludeType::Duration, PreludeAssocFn::Minutes) => Some(Type::Duration),
        (PreludeType::Duration, PreludeAssocFn::Seconds) => Some(Type::Duration),
        (PreludeType::Duration, PreludeAssocFn::Millis) => Some(Type::Duration),
        // T124c: Log module — `Log.<level>(msg, ...)` always returns
        // `Void` (Unit). The structured fields and tracing macro
        // invocation are codegen-time concerns; the type inferencer
        // only needs to know the call is well-formed and produces no
        // value. Every (Log, <level>) pair is valid; arity is enforced
        // at codegen time (must have at least the message arg).
        (PreludeType::Log, PreludeAssocFn::Debug) => Some(Type::Void),
        (PreludeType::Log, PreludeAssocFn::Info) => Some(Type::Void),
        (PreludeType::Log, PreludeAssocFn::Warn) => Some(Type::Void),
        (PreludeType::Log, PreludeAssocFn::Error) => Some(Type::Void),
        // T124d: Regex module — `Regex.compile(pattern)` returns the
        // opaque `Regex` value type. The pattern arg is a String; the
        // returned `regex::Regex` value is the receiver for the four
        // instance methods (match / find / replace / captures).
        (PreludeType::Regex, PreludeAssocFn::Compile) => Some(Type::Regex),
        // T124e: Toml module — `Toml.parse(s)` returns a Buff `Map`
        // whose keys are String (TOML top-level keys are always
        // strings) and whose values are Unknown (TOML values are
        // heterogeneous: scalars, arrays, sub-tables; representing
        // them all as a single Buff type would require a `TomlValue`
        // variant we deliberately don't add to keep the surface
        // minimal — parse + stringify, no schema). The codegen emits
        // the concrete `std::collections::HashMap<String, toml::Value>`
        // via turbofish so the generated Rust is fully typed; the
        // inferred Buff Type::Map<String, Unknown> is the surface
        // contract (a user can pass the result around, index into it,
        // re-stringify it). The Unknown value type is consistent with
        // Buff's "don't pre-implement the world" stance — when a
        // future task adds proper TOML-schema typing, this return
        // type narrows.
        (PreludeType::Toml, PreludeAssocFn::Parse) => {
            Some(Type::map(Type::string(), Type::Unknown))
        }
        // T124e: Toml module — `Toml.stringify(v)` returns a TOML-
        // formatted String. The arg is the value to serialize
        // (typically a Map<String, ?>); the codegen borrows it via
        // `&v` so Rust's serde-Serialize bound on `toml::to_string`
        // is satisfied for any Map<String, toml::Value> / suitable
        // Serialize-implementing value.
        (PreludeType::Toml, PreludeAssocFn::Stringify) => Some(Type::string()),
        // T124f: Math module - every Math.<fn> returns Float (the
        // element type Rust uses for `f64` methods). `min`/`max` also
        // return Float (we deliberately don't try to preserve Int-ness
        // - the lowering goes through `f64::min`/`f64::max` regardless
        // of the arg type, since Rust's `i64::min` would also work but
        // introducing a polymorphic return-type rule here would
        // complicate the registry for marginal gain; a future narrowing
        // task can special-case Int args if needed).
        (PreludeType::Math, PreludeAssocFn::Sqrt) => Some(Type::float_default()),
        (PreludeType::Math, PreludeAssocFn::Sin) => Some(Type::float_default()),
        (PreludeType::Math, PreludeAssocFn::Cos) => Some(Type::float_default()),
        (PreludeType::Math, PreludeAssocFn::Tan) => Some(Type::float_default()),
        (PreludeType::Math, PreludeAssocFn::Abs) => Some(Type::float_default()),
        (PreludeType::Math, PreludeAssocFn::Floor) => Some(Type::float_default()),
        (PreludeType::Math, PreludeAssocFn::Ceil) => Some(Type::float_default()),
        (PreludeType::Math, PreludeAssocFn::Round) => Some(Type::float_default()),
        (PreludeType::Math, PreludeAssocFn::Pow) => Some(Type::float_default()),
        (PreludeType::Math, PreludeAssocFn::Min) => Some(Type::float_default()),
        (PreludeType::Math, PreludeAssocFn::Max) => Some(Type::float_default()),
        // T124f: Random module.
        // `Random.int(min, max)` -> Int (default width = Int<64>).
        // Inclusive range - `min..=max` in Rust's `gen_range`.
        (PreludeType::Random, PreludeAssocFn::Int) => Some(Type::int_default()),
        // `Random.float()` -> Float (f64 in [0, 1)).
        (PreludeType::Random, PreludeAssocFn::Float) => Some(Type::float_default()),
        // `Random.choice(vec)` -> Option<element_type>. The element
        // type is Unknown at the registry level (the codegen emits a
        // generic `.cloned()` so the runtime return type is whatever
        // Rust infers from the input vec - typically `Option<T>` where
        // `T` is the vec's element type). The Unknown here is the
        // surface contract; concrete per-call typing is a future
        // narrowing task (mirrors the Toml.parse Unknown value-type
        // stance from T124e).
        (PreludeType::Random, PreludeAssocFn::Choice) => Some(Type::option(Type::Unknown)),
        // `Random.shuffle(vec)` -> Vector<element_type>. Returns a NEW
        // shuffled Vector (the input is not mutated in the user's
        // surface - the codegen makes a `let mut` binding internally
        // and returns it). Element type Unknown for the same reason as
        // `choice`.
        (PreludeType::Random, PreludeAssocFn::Shuffle) => Some(Type::vector(Type::Unknown)),
        // T124f: Strings module.
        // `Strings.split(text, sep)` -> Vector<String>.
        (PreludeType::Strings, PreludeAssocFn::Split) => Some(Type::vector(Type::string())),
        // `Strings.join(vec, sep)` -> String.
        (PreludeType::Strings, PreludeAssocFn::Join) => Some(Type::string()),
        // `Strings.trim(text)` -> String.
        (PreludeType::Strings, PreludeAssocFn::Trim) => Some(Type::string()),
        // `Strings.replace(text, from, to)` -> String.
        (PreludeType::Strings, PreludeAssocFn::Replace) => Some(Type::string()),
        // `Strings.contains(text, substr)` -> Bool.
        (PreludeType::Strings, PreludeAssocFn::Contains) => Some(Type::bool()),
        // `Strings.starts_with(text, prefix)` -> Bool.
        (PreludeType::Strings, PreludeAssocFn::StartsWith) => Some(Type::bool()),
        // `Strings.to_uppercase(text)` -> String.
        (PreludeType::Strings, PreludeAssocFn::ToUppercase) => Some(Type::string()),
        // `Strings.to_lowercase(text)` -> String.
        (PreludeType::Strings, PreludeAssocFn::ToLowercase) => Some(Type::string()),
        // Every other (type, method) pair is invalid. Returning None lets
        // the caller fall back to the default "user method" path so a
        // future extension doesn't silently swallow unrecognised calls.
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// Instance methods: `recv.method(args)`
// ---------------------------------------------------------------------------

/// A recognised **instance method** on a prelude-type value — the
/// `recv.method(args)` call shape where `recv` is a value whose inferred
/// type is one of the prelude datetime family.
///
/// Variants are named after the method name. Dispatch on
/// `(Type, PreludeInstanceFn)` pairs is exhaustive in
/// [`instance_fn_return_type`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PreludeInstanceFn {
    // ---- Formatting ------------------------------------------------------
    /// `dt.format("%Y-%m-%d")` — strftime formatting → `String`.
    Format,
    // ---- Component access (DateTime / Date / Time) ----------------------
    /// `dt.year()` — year component → `Int`.
    Year,
    /// `dt.month()` — month component (1-12) → `Int`.
    Month,
    /// `dt.day()` — day-of-month component (1-31) → `Int`.
    Day,
    /// `dt.hour()` — hour component (0-23) → `Int`.
    Hour,
    /// `dt.minute()` — minute component (0-59) → `Int`.
    Minute,
    /// `dt.second()` — second component (0-59) → `Int`.
    Second,
    // ---- Conversion -----------------------------------------------------
    /// `dt.timestamp()` — UNIX epoch seconds → `Int`.
    Timestamp,
    // ---- Regex (T124d) -------------------------------------------------
    /// `regex.match(text)` — test whether the compiled regex matches
    /// `text` anywhere. One arg (String). Returns `Option<String>`:
    /// `Some(text)` when at least one match exists (the wrapped value
    /// is the original text — the codegen emits `.find(text).map(...)`
    /// so the wrapped value is the first match's text); `None` when no
    /// match exists. Mirrors Rust's `regex::Regex::is_match` but wraps
    /// to Option for symmetry with [`Self::Find`] (the user can treat
    /// both as "did it match?" without learning two patterns).
    Match,
    /// `regex.find(text)` — return the first match as a String. One arg
    /// (String). Returns `Option<String>`: `Some(matched_text)` when a
    /// match exists, `None` otherwise. Mirrors Rust's
    /// `regex::Regex::find(...).map(|m| m.as_str().to_string())`.
    Find,
    /// `regex.replace(text, replacement)` — replace ALL matches in
    /// `text` with `replacement` (no capture-group interpolation in
    /// v1.4 — the literal `replacement` string is used for every
    /// match). Two args (String text, String replacement). Returns
    /// `String` (the text with every match replaced). Mirrors Rust's
    /// `regex::Regex::replace_all(text, repl).to_string()`. Acceptance
    /// criterion: `regex.replace("a1b2","\\d","X") == "aXbX"`.
    Replace,
    /// `regex.captures(text)` — return a `Map<String, String>` carrying
    /// every capture group (named + numbered). One arg (String).
    /// Returns `Map<String, String>` (always non-empty on a match:
    /// numbered groups are keyed by their 1-based index as strings
    /// `"0"`, `"1"`, ...; named groups additionally by their source
    /// name `$name`). The full match is keyed as `"0"`. On NO match
    /// the codegen emits an empty Map (the `regex::Regex::captures`
    /// Rust API returns `Option` and we lower it via
    /// `.unwrap_or_else(|| Captures::new())` — never a panic).
    /// Key ordering is DETERMINISTIC (group-index order; named groups
    /// intercalated at their source position).
    Captures,
}

impl PreludeInstanceFn {
    /// All recognised instance-method names.
    pub const ALL: &'static [PreludeInstanceFn] = &[
        PreludeInstanceFn::Format,
        PreludeInstanceFn::Year,
        PreludeInstanceFn::Month,
        PreludeInstanceFn::Day,
        PreludeInstanceFn::Hour,
        PreludeInstanceFn::Minute,
        PreludeInstanceFn::Second,
        PreludeInstanceFn::Timestamp,
        // T124d: Regex instance methods — Match / Find / Replace / Captures.
        PreludeInstanceFn::Match,
        PreludeInstanceFn::Find,
        PreludeInstanceFn::Replace,
        PreludeInstanceFn::Captures,
    ];

    /// The source name of this instance method (the method identifier).
    pub const fn name(self) -> &'static str {
        match self {
            PreludeInstanceFn::Format => "format",
            PreludeInstanceFn::Year => "year",
            PreludeInstanceFn::Month => "month",
            PreludeInstanceFn::Day => "day",
            PreludeInstanceFn::Hour => "hour",
            PreludeInstanceFn::Minute => "minute",
            PreludeInstanceFn::Second => "second",
            PreludeInstanceFn::Timestamp => "timestamp",
            // T124d: Regex instance method names. Note `match` is a Buff
            // keyword — the parser doesn't yet allow keywords as method
            // names (the 25-keyword freeze holds for v1.4), so
            // `regex.match(text)` will not parse from source today. The
            // registry + codegen still wire up the `Match` variant so:
            //   (a) AST-constructed tests can exercise it directly;
            //   (b) a future parser relaxation (allowing keywords in
            //       method-call position) lights it up with NO further
            //       registry/codegen work.
            // The other three (find/replace/captures) parse fine since
            // they're not keywords.
            PreludeInstanceFn::Match => "match",
            PreludeInstanceFn::Find => "find",
            PreludeInstanceFn::Replace => "replace",
            PreludeInstanceFn::Captures => "captures",
        }
    }
}

/// Look up a prelude instance method by the (receiver-type, method-name)
/// pair. Returns `None` when the combination is not a recognised prelude
/// instance call (e.g. `Duration.format(...)` is invalid — `format` belongs
/// to `DateTime` / `Date` / `Time`).
pub fn instance_fn_lookup(recv_ty: &Type, method: &str) -> Option<PreludeInstanceFn> {
    let m = PreludeInstanceFn::ALL
        .iter()
        .copied()
        .find(|f| f.name() == method)?;
    // Validate the (type, method) pair.
    instance_fn_return_type(recv_ty, m, &[]).map(|_| m)
}

/// Infer the return type of a prelude instance-method call.
///
/// Returns `None` when the receiver is not a prelude datetime type OR when
/// the (type, method) pair is invalid. Argument types are accepted for
/// future-proofing; current methods with args (`format`) have a fixed
/// return type regardless of the arg.
pub fn instance_fn_return_type(
    recv_ty: &Type,
    method: PreludeInstanceFn,
    _arg_tys: &[Type],
) -> Option<Type> {
    match (recv_ty, method) {
        // Format → String. Applies to every datetime-family type except
        // Duration and Instant (neither has a strftime-style rendering).
        (Type::DateTime, PreludeInstanceFn::Format) => Some(Type::String),
        (Type::Date, PreludeInstanceFn::Format) => Some(Type::String),
        (Type::Time, PreludeInstanceFn::Format) => Some(Type::String),

        // Component accessors — each returns Int (Int<64>, Buff's default).
        (Type::DateTime, PreludeInstanceFn::Year) => Some(Type::int_default()),
        (Type::DateTime, PreludeInstanceFn::Month) => Some(Type::int_default()),
        (Type::DateTime, PreludeInstanceFn::Day) => Some(Type::int_default()),
        (Type::DateTime, PreludeInstanceFn::Hour) => Some(Type::int_default()),
        (Type::DateTime, PreludeInstanceFn::Minute) => Some(Type::int_default()),
        (Type::DateTime, PreludeInstanceFn::Second) => Some(Type::int_default()),
        (Type::DateTime, PreludeInstanceFn::Timestamp) => Some(Type::int_default()),

        (Type::Date, PreludeInstanceFn::Year) => Some(Type::int_default()),
        (Type::Date, PreludeInstanceFn::Month) => Some(Type::int_default()),
        (Type::Date, PreludeInstanceFn::Day) => Some(Type::int_default()),

        (Type::Time, PreludeInstanceFn::Hour) => Some(Type::int_default()),
        (Type::Time, PreludeInstanceFn::Minute) => Some(Type::int_default()),
        (Type::Time, PreludeInstanceFn::Second) => Some(Type::int_default()),

        // T124d: Regex instance methods.
        // `regex.match(text)` -> Option<String> (Some(original_text) when
        // a match exists, None otherwise). The Option wrapping mirrors
        // Rust's `regex::Regex::find(...).map(|m| m.as_str().to_string())`
        // — never `is_match`'s bare bool — so the result composes with
        // Buff's existing Option-handling surface (`??`, `if let`, ...).
        (Type::Regex, PreludeInstanceFn::Match) => Some(Type::option(Type::string())),
        // `regex.find(text)` -> Option<String> (Some(matched_text) /
        // None). Mirrors `regex.find(...).map(|m| m.as_str().to_string())`.
        (Type::Regex, PreludeInstanceFn::Find) => Some(Type::option(Type::string())),
        // `regex.replace(text, repl)` -> String (text with EVERY match
        // replaced — `replace_all`, not `replace` which would do one).
        (Type::Regex, PreludeInstanceFn::Replace) => Some(Type::string()),
        // `regex.captures(text)` -> Map<String, String>. Numbered groups
        // keyed by their 1-based index as strings; named groups keyed by
        // their source name; the full match is "0". Deterministic
        // ordering (group-index order) is a codegen concern.
        (Type::Regex, PreludeInstanceFn::Captures) => {
            Some(Type::map(Type::string(), Type::string()))
        }

        // Every other (type, method) pair is invalid. Returning None lets
        // the caller fall back to the default "user method" path.
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prelude_type_lookup_known_names() {
        for &t in PreludeType::ALL {
            assert_eq!(prelude_type_lookup(t.name()), Some(t));
            assert!(is_prelude_type(t.name()));
            // Each DATETIME-FAMILY type's `buff_type()` round-trips
            // through the `is_prelude_datetime` predicate. Namespace-only
            // modules (T124c: `Log`) skip this check — they have no
            // value representation, so `buff_type()` returns `Void`
            // (which is correctly NOT a datetime).
            //
            // T124d: `Regex` is a runtime value but NOT a datetime, so
            // it also skips the `is_prelude_datetime` check (its
            // `buff_type()` returns `Type::Regex`, which round-trips
            // through `is_prelude_regex()` instead).
            if !t.is_namespace_only() && t != PreludeType::Regex {
                assert!(t.buff_type().is_prelude_datetime());
            }
        }
    }

    #[test]
    fn prelude_type_lookup_rejects_unknown() {
        assert!(!is_prelude_type("MyType"));
        assert!(!is_prelude_type(""));
        assert_eq!(prelude_type_lookup("DateTimeX"), None);
    }

    #[test]
    fn prelude_assoc_fn_lookup_valid_pairs() {
        assert_eq!(
            assoc_fn_lookup("DateTime", "now"),
            Some((PreludeType::DateTime, PreludeAssocFn::Now))
        );
        assert_eq!(
            assoc_fn_lookup("Duration", "days"),
            Some((PreludeType::Duration, PreludeAssocFn::Days))
        );
        assert_eq!(
            assoc_fn_lookup("Instant", "now"),
            Some((PreludeType::Instant, PreludeAssocFn::Now))
        );
    }

    #[test]
    fn prelude_assoc_fn_lookup_rejects_invalid_pairs() {
        // `days` is a Duration method — not DateTime.
        assert_eq!(assoc_fn_lookup("DateTime", "days"), None);
        // `now` is not a Duration method.
        assert_eq!(assoc_fn_lookup("Duration", "now"), None);
        // Unknown type.
        assert_eq!(assoc_fn_lookup("MyType", "now"), None);
        // Unknown method.
        assert_eq!(assoc_fn_lookup("DateTime", "unknown"), None);
    }

    // T124c: Log module — Log.<level>(msg, ...) assoc-fn lookups.
    #[test]
    fn prelude_log_assoc_fn_lookup_valid_pairs() {
        // All four Log levels resolve via the registry.
        assert_eq!(
            assoc_fn_lookup("Log", "debug"),
            Some((PreludeType::Log, PreludeAssocFn::Debug))
        );
        assert_eq!(
            assoc_fn_lookup("Log", "info"),
            Some((PreludeType::Log, PreludeAssocFn::Info))
        );
        assert_eq!(
            assoc_fn_lookup("Log", "warn"),
            Some((PreludeType::Log, PreludeAssocFn::Warn))
        );
        assert_eq!(
            assoc_fn_lookup("Log", "error"),
            Some((PreludeType::Log, PreludeAssocFn::Error))
        );
        // `Log` is recognised as a prelude type.
        assert!(is_prelude_type("Log"));
        // `Log.buff_type()` is `Void` (no runtime value).
        assert_eq!(PreludeType::Log.buff_type(), Type::Void);
        // `Log.is_namespace_only()` is true.
        assert!(PreludeType::Log.is_namespace_only());
        // The other prelude types are NOT namespace-only.
        assert!(!PreludeType::DateTime.is_namespace_only());
    }

    #[test]
    fn prelude_log_assoc_fn_lookup_rejects_invalid_pairs() {
        // Log.now is invalid (now is not a Log method).
        assert_eq!(assoc_fn_lookup("Log", "now"), None);
        // DateTime.info is invalid (info belongs to Log).
        assert_eq!(assoc_fn_lookup("DateTime", "info"), None);
        // Log.unknown is invalid.
        assert_eq!(assoc_fn_lookup("Log", "unknown"), None);
    }

    #[test]
    fn prelude_log_assoc_fn_return_types() {
        // All four Log levels return Void.
        assert_eq!(
            assoc_fn_return_type(PreludeType::Log, PreludeAssocFn::Debug, &[]),
            Some(Type::Void)
        );
        assert_eq!(
            assoc_fn_return_type(PreludeType::Log, PreludeAssocFn::Info, &[]),
            Some(Type::Void)
        );
        assert_eq!(
            assoc_fn_return_type(PreludeType::Log, PreludeAssocFn::Warn, &[]),
            Some(Type::Void)
        );
        assert_eq!(
            assoc_fn_return_type(PreludeType::Log, PreludeAssocFn::Error, &[]),
            Some(Type::Void)
        );
        // Log + non-Log method is invalid.
        assert_eq!(
            assoc_fn_return_type(PreludeType::Log, PreludeAssocFn::Now, &[]),
            None
        );
        // Non-Log type + Log method is invalid.
        assert_eq!(
            assoc_fn_return_type(PreludeType::DateTime, PreludeAssocFn::Info, &[]),
            None
        );
    }

    #[test]
    fn prelude_assoc_fn_return_types() {
        // DateTime.now() -> DateTime
        assert_eq!(
            assoc_fn_return_type(PreludeType::DateTime, PreludeAssocFn::Now, &[]),
            Some(Type::DateTime)
        );
        // DateTime.parse(s) -> DateTime
        assert_eq!(
            assoc_fn_return_type(
                PreludeType::DateTime,
                PreludeAssocFn::Parse,
                &[Type::string()]
            ),
            Some(Type::DateTime)
        );
        // Duration.days(n) -> Duration
        assert_eq!(
            assoc_fn_return_type(
                PreludeType::Duration,
                PreludeAssocFn::Days,
                &[Type::int_default()]
            ),
            Some(Type::Duration)
        );
        // Instant.now() -> Instant
        assert_eq!(
            assoc_fn_return_type(PreludeType::Instant, PreludeAssocFn::Now, &[]),
            Some(Type::Instant)
        );
        // Invalid pair: DateTime.days(n) -> None
        assert_eq!(
            assoc_fn_return_type(
                PreludeType::DateTime,
                PreludeAssocFn::Days,
                &[Type::int_default()]
            ),
            None
        );
    }

    #[test]
    fn prelude_instance_fn_return_types() {
        // dt.format(fmt) -> String
        assert_eq!(
            instance_fn_return_type(
                &Type::DateTime,
                PreludeInstanceFn::Format,
                &[Type::string()]
            ),
            Some(Type::string())
        );
        // dt.year() -> Int
        assert_eq!(
            instance_fn_return_type(&Type::DateTime, PreludeInstanceFn::Year, &[]),
            Some(Type::int_default())
        );
        // dt.timestamp() -> Int
        assert_eq!(
            instance_fn_return_type(&Type::DateTime, PreludeInstanceFn::Timestamp, &[]),
            Some(Type::int_default())
        );
        // date.format(fmt) -> String (Date also has format)
        assert_eq!(
            instance_fn_return_type(&Type::Date, PreludeInstanceFn::Format, &[Type::string()]),
            Some(Type::string())
        );
        // date.year() -> Int (Date has year, NOT hour)
        assert_eq!(
            instance_fn_return_type(&Type::Date, PreludeInstanceFn::Year, &[]),
            Some(Type::int_default())
        );
        // date.hour() -> None (Date has no hour component)
        assert_eq!(
            instance_fn_return_type(&Type::Date, PreludeInstanceFn::Hour, &[]),
            None
        );
        // Duration.format(...) -> None (Duration has no format method)
        assert_eq!(
            instance_fn_return_type(
                &Type::Duration,
                PreludeInstanceFn::Format,
                &[Type::string()]
            ),
            None
        );
        // Instant.format(...) -> None
        assert_eq!(
            instance_fn_return_type(&Type::Instant, PreludeInstanceFn::Format, &[Type::string()]),
            None
        );
    }

    #[test]
    fn prelude_instance_fn_lookup_dispatches_on_receiver_type() {
        // DateTime.format is valid.
        assert_eq!(
            instance_fn_lookup(&Type::DateTime, "format"),
            Some(PreludeInstanceFn::Format)
        );
        // Duration.format is NOT valid.
        assert_eq!(instance_fn_lookup(&Type::Duration, "format"), None);
        // Unknown method.
        assert_eq!(instance_fn_lookup(&Type::DateTime, "unknown"), None);
        // Non-prelude receiver (e.g. String).
        assert_eq!(instance_fn_lookup(&Type::String, "format"), None);
    }

    #[test]
    fn prelude_type_no_duplicates() {
        let names: Vec<&str> = PreludeType::ALL.iter().map(|t| t.name()).collect();
        let unique: std::collections::HashSet<&str> = names.iter().copied().collect();
        assert_eq!(names.len(), unique.len(), "duplicate prelude type names");
        // 5 datetime-family members shipped in T124b + 1 namespace module
        // (Log) shipped in T124c + 1 runtime-value-with-methods type
        // (Regex) shipped in T124d + 1 namespace-only module (Toml)
        // shipped in T124e + 3 namespace-only utility modules (Math,
        // Random, Strings) shipped in T124f = 11 total prelude types.
        assert_eq!(PreludeType::ALL.len(), 11);
    }

    #[test]
    fn prelude_assoc_fn_no_duplicates() {
        let names: Vec<&str> = PreludeAssocFn::ALL.iter().map(|f| f.name()).collect();
        let unique: std::collections::HashSet<&str> = names.iter().copied().collect();
        assert_eq!(names.len(), unique.len(), "duplicate assoc-fn names");
    }

    #[test]
    fn prelude_instance_fn_no_duplicates() {
        let names: Vec<&str> = PreludeInstanceFn::ALL.iter().map(|f| f.name()).collect();
        let unique: std::collections::HashSet<&str> = names.iter().copied().collect();
        assert_eq!(names.len(), unique.len(), "duplicate instance-fn names");
    }

    #[test]
    fn buff_type_constructors_and_predicate() {
        // Each Type constructor + the is_prelude_datetime predicate.
        assert!(Type::datetime().is_prelude_datetime());
        assert!(Type::date().is_prelude_datetime());
        assert!(Type::time().is_prelude_datetime());
        assert!(Type::duration().is_prelude_datetime());
        assert!(Type::instant().is_prelude_datetime());
        // Non-datetime types are not flagged.
        assert!(!Type::int_default().is_prelude_datetime());
        assert!(!Type::string().is_prelude_datetime());
        assert!(!Type::Unknown.is_prelude_datetime());
        // T124d: Regex type + predicate. Regex is NOT a datetime family
        // member — its dedicated `is_prelude_regex` predicate captures
        // the runtime-value-but-not-datetime case.
        assert!(Type::regex().is_prelude_regex());
        assert!(!Type::regex().is_prelude_datetime());
        assert!(!Type::DateTime.is_prelude_regex());
        assert!(!Type::string().is_prelude_regex());
        // Cross-check via the prelude-type registry: `Regex.buff_type()`
        // round-trips through `is_prelude_regex` (the only prelude type
        // for which it does).
        assert!(PreludeType::Regex.buff_type().is_prelude_regex());
        assert!(!PreludeType::DateTime.buff_type().is_prelude_regex());
        assert!(!PreludeType::Log.buff_type().is_prelude_regex());
    }

    #[test]
    fn type_display_datetime_family() {
        assert_eq!(Type::DateTime.to_string(), "DateTime");
        assert_eq!(Type::Date.to_string(), "Date");
        assert_eq!(Type::Time.to_string(), "Time");
        assert_eq!(Type::Duration.to_string(), "Duration");
        assert_eq!(Type::Instant.to_string(), "Instant");
        // T124d: Regex Display mirrors the Buff surface name.
        assert_eq!(Type::Regex.to_string(), "Regex");
    }

    // T124d: Regex module — `Regex.compile(p)` assoc-fn lookups + return type.
    #[test]
    fn prelude_regex_assoc_fn_lookup_valid_pairs() {
        // `Regex.compile` is the single associated function on the Regex
        // prelude type. It returns a real `Regex` value (NOT Void like
        // Log's namespace-only assoc fns).
        assert_eq!(
            assoc_fn_lookup("Regex", "compile"),
            Some((PreludeType::Regex, PreludeAssocFn::Compile))
        );
        // `Regex` is recognised as a prelude type.
        assert!(is_prelude_type("Regex"));
        // `Regex.buff_type()` is `Regex` (a real runtime value, NOT Void).
        assert_eq!(PreludeType::Regex.buff_type(), Type::Regex);
        // `Regex.is_namespace_only()` is false (it IS a runtime value).
        assert!(!PreludeType::Regex.is_namespace_only());
        // The other prelude types are NOT Regex (round-trip via buff_type).
        assert!(!PreludeType::DateTime.buff_type().is_prelude_regex());
        assert!(PreludeType::Regex.buff_type().is_prelude_regex());
    }

    #[test]
    fn prelude_regex_assoc_fn_lookup_rejects_invalid_pairs() {
        // Regex.now is invalid (now is not a Regex method).
        assert_eq!(assoc_fn_lookup("Regex", "now"), None);
        // DateTime.compile is invalid (compile belongs to Regex).
        assert_eq!(assoc_fn_lookup("DateTime", "compile"), None);
        // Regex.unknown is invalid.
        assert_eq!(assoc_fn_lookup("Regex", "unknown"), None);
        // Regex.parse is invalid (Regex has compile, not parse).
        assert_eq!(assoc_fn_lookup("Regex", "parse"), None);
    }

    #[test]
    fn prelude_regex_assoc_fn_return_type() {
        // Regex.compile(pattern) -> Regex.
        assert_eq!(
            assoc_fn_return_type(
                PreludeType::Regex,
                PreludeAssocFn::Compile,
                &[Type::string()]
            ),
            Some(Type::Regex)
        );
        // Regex + non-Regex method is invalid.
        assert_eq!(
            assoc_fn_return_type(PreludeType::Regex, PreludeAssocFn::Now, &[]),
            None
        );
        // Non-Regex type + Regex method is invalid.
        assert_eq!(
            assoc_fn_return_type(PreludeType::DateTime, PreludeAssocFn::Compile, &[]),
            None
        );
        // Log + Regex.compile is invalid (Log is namespace-only).
        assert_eq!(
            assoc_fn_return_type(PreludeType::Log, PreludeAssocFn::Compile, &[]),
            None
        );
    }

    #[test]
    fn prelude_regex_instance_fn_lookup_valid_pairs() {
        // All four Regex instance methods resolve via the registry when
        // the receiver is `Type::Regex`.
        assert_eq!(
            instance_fn_lookup(&Type::Regex, "match"),
            Some(PreludeInstanceFn::Match)
        );
        assert_eq!(
            instance_fn_lookup(&Type::Regex, "find"),
            Some(PreludeInstanceFn::Find)
        );
        assert_eq!(
            instance_fn_lookup(&Type::Regex, "replace"),
            Some(PreludeInstanceFn::Replace)
        );
        assert_eq!(
            instance_fn_lookup(&Type::Regex, "captures"),
            Some(PreludeInstanceFn::Captures)
        );
    }

    #[test]
    fn prelude_regex_instance_fn_lookup_rejects_invalid_pairs() {
        // Regex.format is invalid (format belongs to DateTime/Date/Time).
        assert_eq!(instance_fn_lookup(&Type::Regex, "format"), None);
        // Regex.year is invalid.
        assert_eq!(instance_fn_lookup(&Type::Regex, "year"), None);
        // Regex.unknown is invalid.
        assert_eq!(instance_fn_lookup(&Type::Regex, "unknown"), None);
        // Regex.match is invalid when the receiver is NOT Regex.
        assert_eq!(instance_fn_lookup(&Type::DateTime, "match"), None);
        assert_eq!(instance_fn_lookup(&Type::String, "find"), None);
    }

    #[test]
    fn prelude_regex_instance_fn_return_types() {
        // regex.match(text) -> Option<String>.
        assert_eq!(
            instance_fn_return_type(&Type::Regex, PreludeInstanceFn::Match, &[Type::string()]),
            Some(Type::option(Type::string()))
        );
        // regex.find(text) -> Option<String>.
        assert_eq!(
            instance_fn_return_type(&Type::Regex, PreludeInstanceFn::Find, &[Type::string()]),
            Some(Type::option(Type::string()))
        );
        // regex.replace(text, repl) -> String.
        assert_eq!(
            instance_fn_return_type(
                &Type::Regex,
                PreludeInstanceFn::Replace,
                &[Type::string(), Type::string()]
            ),
            Some(Type::string())
        );
        // regex.captures(text) -> Map<String, String>.
        assert_eq!(
            instance_fn_return_type(&Type::Regex, PreludeInstanceFn::Captures, &[Type::string()]),
            Some(Type::map(Type::string(), Type::string()))
        );
        // Non-Regex receiver + Regex method is invalid.
        assert_eq!(
            instance_fn_return_type(&Type::DateTime, PreludeInstanceFn::Match, &[Type::string()]),
            None
        );
        // Regex receiver + non-Regex method is invalid.
        assert_eq!(
            instance_fn_return_type(&Type::Regex, PreludeInstanceFn::Format, &[Type::string()]),
            None
        );
    }

    // T124e: Toml module — `Toml.parse(s)` / `Toml.stringify(v)` assoc-fn
    // lookups + return types. Mirrors the Log namespace-only precedent
    // (T124c) but with non-Void return types (Map / String).
    #[test]
    fn prelude_toml_assoc_fn_lookup_valid_pairs() {
        // `Toml.parse` reuses the registry's shared `Parse` variant
        // (also used by DateTime.parse / Date.parse).
        assert_eq!(
            assoc_fn_lookup("Toml", "parse"),
            Some((PreludeType::Toml, PreludeAssocFn::Parse))
        );
        // `Toml.stringify` is the dedicated Toml-only assoc fn.
        assert_eq!(
            assoc_fn_lookup("Toml", "stringify"),
            Some((PreludeType::Toml, PreludeAssocFn::Stringify))
        );
        // `Toml` is recognised as a prelude type.
        assert!(is_prelude_type("Toml"));
        // `Toml.buff_type()` is `Void` (no runtime value — namespace-only
        // like Log).
        assert_eq!(PreludeType::Toml.buff_type(), Type::Void);
        // `Toml.is_namespace_only()` is true.
        assert!(PreludeType::Toml.is_namespace_only());
        // The datetime-family types are NOT namespace-only.
        assert!(!PreludeType::DateTime.is_namespace_only());
        // Regex is NOT namespace-only (it's a real runtime value).
        assert!(!PreludeType::Regex.is_namespace_only());
    }

    #[test]
    fn prelude_toml_assoc_fn_lookup_rejects_invalid_pairs() {
        // Toml.now is invalid (now is not a Toml method).
        assert_eq!(assoc_fn_lookup("Toml", "now"), None);
        // Toml.compile is invalid (compile belongs to Regex).
        assert_eq!(assoc_fn_lookup("Toml", "compile"), None);
        // Toml.unknown is invalid.
        assert_eq!(assoc_fn_lookup("Toml", "unknown"), None);
        // Toml.debug is invalid (debug belongs to Log).
        assert_eq!(assoc_fn_lookup("Toml", "debug"), None);
        // DateTime.stringify is invalid (stringify belongs to Toml).
        assert_eq!(assoc_fn_lookup("DateTime", "stringify"), None);
        // Regex.stringify is invalid.
        assert_eq!(assoc_fn_lookup("Regex", "stringify"), None);
        // Log.stringify is invalid (Log is namespace-only).
        assert_eq!(assoc_fn_lookup("Log", "stringify"), None);
    }

    #[test]
    fn prelude_toml_assoc_fn_return_types() {
        // Toml.parse(s) -> Map<String, Unknown>. The value type is
        // Unknown because TOML values are heterogeneous (scalars /
        // arrays / sub-tables); the codegen turbofish-es to the
        // concrete `HashMap<String, toml::Value>` at the Rust level.
        assert_eq!(
            assoc_fn_return_type(PreludeType::Toml, PreludeAssocFn::Parse, &[Type::string()]),
            Some(Type::map(Type::string(), Type::Unknown))
        );
        // Toml.stringify(v) -> String.
        assert_eq!(
            assoc_fn_return_type(
                PreludeType::Toml,
                PreludeAssocFn::Stringify,
                &[Type::map(Type::string(), Type::Unknown)]
            ),
            Some(Type::string())
        );
        // Toml + non-Toml method is invalid.
        assert_eq!(
            assoc_fn_return_type(PreludeType::Toml, PreludeAssocFn::Now, &[]),
            None
        );
        assert_eq!(
            assoc_fn_return_type(PreludeType::Toml, PreludeAssocFn::Compile, &[]),
            None
        );
        assert_eq!(
            assoc_fn_return_type(PreludeType::Toml, PreludeAssocFn::Debug, &[]),
            None
        );
        // Non-Toml type + Toml method is invalid.
        assert_eq!(
            assoc_fn_return_type(PreludeType::DateTime, PreludeAssocFn::Stringify, &[]),
            None
        );
        assert_eq!(
            assoc_fn_return_type(PreludeType::Regex, PreludeAssocFn::Stringify, &[]),
            None
        );
        assert_eq!(
            assoc_fn_return_type(PreludeType::Log, PreludeAssocFn::Stringify, &[]),
            None
        );
    }

    #[test]
    fn prelude_toml_namespace_only_predicate() {
        // Both Log and Toml are namespace-only modules.
        assert!(PreludeType::Log.is_namespace_only());
        assert!(PreludeType::Toml.is_namespace_only());
        // T124f: Math / Random / Strings are also namespace-only.
        assert!(PreludeType::Math.is_namespace_only());
        assert!(PreludeType::Random.is_namespace_only());
        assert!(PreludeType::Strings.is_namespace_only());
        // The datetime family + Regex are NOT namespace-only.
        assert!(!PreludeType::DateTime.is_namespace_only());
        assert!(!PreludeType::Date.is_namespace_only());
        assert!(!PreludeType::Time.is_namespace_only());
        assert!(!PreludeType::Duration.is_namespace_only());
        assert!(!PreludeType::Instant.is_namespace_only());
        assert!(!PreludeType::Regex.is_namespace_only());
        // T124f: The count of namespace-only modules is now exactly 5
        // (Log + Toml + Math + Random + Strings).
        let namespace_only_count = PreludeType::ALL
            .iter()
            .filter(|t| t.is_namespace_only())
            .count();
        assert_eq!(namespace_only_count, 5);
    }

    // T124f: Math module - `Math.<fn>(x, ...)` assoc-fn lookups +
    // return types + the associated-constant mechanism for `Math.PI` /
    // `Math.E`. Mirrors the Log / Toml namespace-only precedent (T124c
    // / T124e) but with Float return types + the first associated
    // constants in the registry.
    #[test]
    fn prelude_math_assoc_fn_lookup_valid_pairs() {
        // All 11 Math assoc fns resolve via the registry.
        assert_eq!(
            assoc_fn_lookup("Math", "sqrt"),
            Some((PreludeType::Math, PreludeAssocFn::Sqrt))
        );
        assert_eq!(
            assoc_fn_lookup("Math", "sin"),
            Some((PreludeType::Math, PreludeAssocFn::Sin))
        );
        assert_eq!(
            assoc_fn_lookup("Math", "cos"),
            Some((PreludeType::Math, PreludeAssocFn::Cos))
        );
        assert_eq!(
            assoc_fn_lookup("Math", "tan"),
            Some((PreludeType::Math, PreludeAssocFn::Tan))
        );
        assert_eq!(
            assoc_fn_lookup("Math", "abs"),
            Some((PreludeType::Math, PreludeAssocFn::Abs))
        );
        assert_eq!(
            assoc_fn_lookup("Math", "floor"),
            Some((PreludeType::Math, PreludeAssocFn::Floor))
        );
        assert_eq!(
            assoc_fn_lookup("Math", "ceil"),
            Some((PreludeType::Math, PreludeAssocFn::Ceil))
        );
        assert_eq!(
            assoc_fn_lookup("Math", "round"),
            Some((PreludeType::Math, PreludeAssocFn::Round))
        );
        assert_eq!(
            assoc_fn_lookup("Math", "pow"),
            Some((PreludeType::Math, PreludeAssocFn::Pow))
        );
        assert_eq!(
            assoc_fn_lookup("Math", "min"),
            Some((PreludeType::Math, PreludeAssocFn::Min))
        );
        assert_eq!(
            assoc_fn_lookup("Math", "max"),
            Some((PreludeType::Math, PreludeAssocFn::Max))
        );
        // `Math` is recognised as a prelude type.
        assert!(is_prelude_type("Math"));
        // `Math.buff_type()` is `Void` (no runtime value - namespace-only
        // like Log / Toml).
        assert_eq!(PreludeType::Math.buff_type(), Type::Void);
        // `Math.is_namespace_only()` is true.
        assert!(PreludeType::Math.is_namespace_only());
    }

    #[test]
    fn prelude_math_assoc_fn_lookup_rejects_invalid_pairs() {
        // Math.now is invalid (now belongs to DateTime/Instant).
        assert_eq!(assoc_fn_lookup("Math", "now"), None);
        // Math.compile is invalid (compile belongs to Regex).
        assert_eq!(assoc_fn_lookup("Math", "compile"), None);
        // Math.unknown is invalid.
        assert_eq!(assoc_fn_lookup("Math", "unknown"), None);
        // Math.debug is invalid (debug belongs to Log).
        assert_eq!(assoc_fn_lookup("Math", "debug"), None);
        // Math.parse is invalid (Math has no parse method).
        assert_eq!(assoc_fn_lookup("Math", "parse"), None);
        // DateTime.sqrt is invalid (sqrt belongs to Math).
        assert_eq!(assoc_fn_lookup("DateTime", "sqrt"), None);
        // Log.sin is invalid (Log is namespace-only).
        assert_eq!(assoc_fn_lookup("Log", "sin"), None);
    }

    #[test]
    fn prelude_math_assoc_fn_return_types() {
        // All Math methods return Float (f64 width).
        let expected = Some(Type::float_default());
        assert_eq!(
            assoc_fn_return_type(PreludeType::Math, PreludeAssocFn::Sqrt, &[Type::float_default()]),
            expected
        );
        assert_eq!(
            assoc_fn_return_type(PreludeType::Math, PreludeAssocFn::Sin, &[Type::float_default()]),
            expected
        );
        assert_eq!(
            assoc_fn_return_type(PreludeType::Math, PreludeAssocFn::Cos, &[Type::float_default()]),
            expected
        );
        assert_eq!(
            assoc_fn_return_type(PreludeType::Math, PreludeAssocFn::Tan, &[Type::float_default()]),
            expected
        );
        assert_eq!(
            assoc_fn_return_type(PreludeType::Math, PreludeAssocFn::Abs, &[Type::float_default()]),
            expected
        );
        assert_eq!(
            assoc_fn_return_type(
                PreludeType::Math,
                PreludeAssocFn::Floor,
                &[Type::float_default()]
            ),
            expected
        );
        assert_eq!(
            assoc_fn_return_type(
                PreludeType::Math,
                PreludeAssocFn::Ceil,
                &[Type::float_default()]
            ),
            expected
        );
        assert_eq!(
            assoc_fn_return_type(
                PreludeType::Math,
                PreludeAssocFn::Round,
                &[Type::float_default()]
            ),
            expected
        );
        // pow takes 2 args, but still returns Float.
        assert_eq!(
            assoc_fn_return_type(
                PreludeType::Math,
                PreludeAssocFn::Pow,
                &[Type::float_default(), Type::float_default()]
            ),
            expected
        );
        assert_eq!(
            assoc_fn_return_type(
                PreludeType::Math,
                PreludeAssocFn::Min,
                &[Type::float_default(), Type::float_default()]
            ),
            expected
        );
        assert_eq!(
            assoc_fn_return_type(
                PreludeType::Math,
                PreludeAssocFn::Max,
                &[Type::float_default(), Type::float_default()]
            ),
            expected
        );
        // Math + non-Math method is invalid.
        assert_eq!(
            assoc_fn_return_type(PreludeType::Math, PreludeAssocFn::Now, &[]),
            None
        );
        // Non-Math type + Math method is invalid.
        assert_eq!(
            assoc_fn_return_type(PreludeType::DateTime, PreludeAssocFn::Sqrt, &[]),
            None
        );
    }

    // T124f: Math associated constants - the FIRST associated-constant
    // prelude mechanism. `Math.PI` / `Math.E` resolve via the dedicated
    // `assoc_const_lookup` registry (separate from assoc fns because
    // the parser produces a zero-arg MethodCall that the codegen must
    // rewrite to the Rust `std::f64::consts::PI` / `E` path rather
    // than the literal field access `Math.PI`).
    #[test]
    fn prelude_math_assoc_const_lookup_valid_pairs() {
        assert_eq!(
            assoc_const_lookup("Math", "PI"),
            Some((PreludeType::Math, PreludeAssocConst::Pi))
        );
        assert_eq!(
            assoc_const_lookup("Math", "E"),
            Some((PreludeType::Math, PreludeAssocConst::E))
        );
    }

    #[test]
    fn prelude_math_assoc_const_lookup_rejects_invalid_pairs() {
        // Math.TAU is invalid (not in the T124f surface).
        assert_eq!(assoc_const_lookup("Math", "TAU"), None);
        // Math.PHI is invalid.
        assert_eq!(assoc_const_lookup("Math", "PHI"), None);
        // Math.pi (lowercase) is invalid (constants are UPPERCASE).
        assert_eq!(assoc_const_lookup("Math", "pi"), None);
        // Math.sqrt is not a constant.
        assert_eq!(assoc_const_lookup("Math", "sqrt"), None);
        // DateTime.PI is invalid (PI belongs to Math).
        assert_eq!(assoc_const_lookup("DateTime", "PI"), None);
        // Log.PI is invalid (Log is namespace-only with no constants).
        assert_eq!(assoc_const_lookup("Log", "PI"), None);
        // Toml.E is invalid.
        assert_eq!(assoc_const_lookup("Toml", "E"), None);
    }

    #[test]
    fn prelude_math_assoc_const_return_types() {
        // Math.PI / Math.E -> Float (f64).
        assert_eq!(
            assoc_const_return_type(PreludeType::Math, PreludeAssocConst::Pi),
            Some(Type::float_default())
        );
        assert_eq!(
            assoc_const_return_type(PreludeType::Math, PreludeAssocConst::E),
            Some(Type::float_default())
        );
        // Non-Math type + Math const is invalid.
        assert_eq!(
            assoc_const_return_type(PreludeType::DateTime, PreludeAssocConst::Pi),
            None
        );
        assert_eq!(
            assoc_const_return_type(PreludeType::Log, PreludeAssocConst::E),
            None
        );
    }

    #[test]
    fn prelude_assoc_const_all_and_no_duplicates() {
        // 2 associated constants: PI + E.
        assert_eq!(PreludeAssocConst::ALL.len(), 2);
        let names: Vec<&str> = PreludeAssocConst::ALL.iter().map(|c| c.name()).collect();
        let unique: std::collections::HashSet<&str> = names.iter().copied().collect();
        assert_eq!(names.len(), unique.len(), "duplicate assoc-const names");
        // Names are UPPERCASE per Rust / Buff const convention.
        assert_eq!(PreludeAssocConst::Pi.name(), "PI");
        assert_eq!(PreludeAssocConst::E.name(), "E");
    }

    // T124f: Random module - `Random.<fn>(...)` assoc-fn lookups +
    // return types. Mirrors the Math / Log namespace-only precedent
    // but with mixed return types (Int / Float / Option / Vector).
    #[test]
    fn prelude_random_assoc_fn_lookup_valid_pairs() {
        assert_eq!(
            assoc_fn_lookup("Random", "int"),
            Some((PreludeType::Random, PreludeAssocFn::Int))
        );
        assert_eq!(
            assoc_fn_lookup("Random", "float"),
            Some((PreludeType::Random, PreludeAssocFn::Float))
        );
        assert_eq!(
            assoc_fn_lookup("Random", "choice"),
            Some((PreludeType::Random, PreludeAssocFn::Choice))
        );
        assert_eq!(
            assoc_fn_lookup("Random", "shuffle"),
            Some((PreludeType::Random, PreludeAssocFn::Shuffle))
        );
        // `Random` is recognised as a prelude type.
        assert!(is_prelude_type("Random"));
        // `Random.buff_type()` is `Void` (namespace-only).
        assert_eq!(PreludeType::Random.buff_type(), Type::Void);
        // `Random.is_namespace_only()` is true.
        assert!(PreludeType::Random.is_namespace_only());
    }

    #[test]
    fn prelude_random_assoc_fn_lookup_rejects_invalid_pairs() {
        // Random.now is invalid.
        assert_eq!(assoc_fn_lookup("Random", "now"), None);
        // Random.compile is invalid.
        assert_eq!(assoc_fn_lookup("Random", "compile"), None);
        // Random.sqrt is invalid (sqrt belongs to Math).
        assert_eq!(assoc_fn_lookup("Random", "sqrt"), None);
        // Math.int is invalid (int belongs to Random).
        assert_eq!(assoc_fn_lookup("Math", "int"), None);
    }

    #[test]
    fn prelude_random_assoc_fn_return_types() {
        // Random.int(min, max) -> Int<64>.
        assert_eq!(
            assoc_fn_return_type(
                PreludeType::Random,
                PreludeAssocFn::Int,
                &[Type::int_default(), Type::int_default()]
            ),
            Some(Type::int_default())
        );
        // Random.float() -> Float.
        assert_eq!(
            assoc_fn_return_type(PreludeType::Random, PreludeAssocFn::Float, &[]),
            Some(Type::float_default())
        );
        // Random.choice(vec) -> Option<Unknown> (element type inferred
        // by Rust at the use site; Unknown at the registry level).
        assert_eq!(
            assoc_fn_return_type(
                PreludeType::Random,
                PreludeAssocFn::Choice,
                &[Type::vector(Type::Unknown)]
            ),
            Some(Type::option(Type::Unknown))
        );
        // Random.shuffle(vec) -> Vector<Unknown>.
        assert_eq!(
            assoc_fn_return_type(
                PreludeType::Random,
                PreludeAssocFn::Shuffle,
                &[Type::vector(Type::Unknown)]
            ),
            Some(Type::vector(Type::Unknown))
        );
    }

    // T124f: Strings module - `Strings.<fn>(...)` assoc-fn lookups +
    // return types. Mirrors the Math / Log namespace-only precedent.
    #[test]
    fn prelude_strings_assoc_fn_lookup_valid_pairs() {
        assert_eq!(
            assoc_fn_lookup("Strings", "split"),
            Some((PreludeType::Strings, PreludeAssocFn::Split))
        );
        assert_eq!(
            assoc_fn_lookup("Strings", "join"),
            Some((PreludeType::Strings, PreludeAssocFn::Join))
        );
        assert_eq!(
            assoc_fn_lookup("Strings", "trim"),
            Some((PreludeType::Strings, PreludeAssocFn::Trim))
        );
        assert_eq!(
            assoc_fn_lookup("Strings", "replace"),
            Some((PreludeType::Strings, PreludeAssocFn::Replace))
        );
        assert_eq!(
            assoc_fn_lookup("Strings", "contains"),
            Some((PreludeType::Strings, PreludeAssocFn::Contains))
        );
        assert_eq!(
            assoc_fn_lookup("Strings", "starts_with"),
            Some((PreludeType::Strings, PreludeAssocFn::StartsWith))
        );
        assert_eq!(
            assoc_fn_lookup("Strings", "to_uppercase"),
            Some((PreludeType::Strings, PreludeAssocFn::ToUppercase))
        );
        assert_eq!(
            assoc_fn_lookup("Strings", "to_lowercase"),
            Some((PreludeType::Strings, PreludeAssocFn::ToLowercase))
        );
        // `Strings` is recognised as a prelude type.
        assert!(is_prelude_type("Strings"));
        // `Strings.buff_type()` is `Void` (namespace-only).
        assert_eq!(PreludeType::Strings.buff_type(), Type::Void);
        // `Strings.is_namespace_only()` is true.
        assert!(PreludeType::Strings.is_namespace_only());
    }

    #[test]
    fn prelude_strings_assoc_fn_lookup_rejects_invalid_pairs() {
        // Strings.now is invalid.
        assert_eq!(assoc_fn_lookup("Strings", "now"), None);
        // Strings.compile is invalid.
        assert_eq!(assoc_fn_lookup("Strings", "compile"), None);
        // Strings.sqrt is invalid (sqrt belongs to Math).
        assert_eq!(assoc_fn_lookup("Strings", "sqrt"), None);
        // Strings.int is invalid (int belongs to Random).
        assert_eq!(assoc_fn_lookup("Strings", "int"), None);
        // Math.split is invalid (split belongs to Strings).
        assert_eq!(assoc_fn_lookup("Math", "split"), None);
    }

    #[test]
    fn prelude_strings_assoc_fn_return_types() {
        // Strings.split(text, sep) -> Vector<String>.
        assert_eq!(
            assoc_fn_return_type(
                PreludeType::Strings,
                PreludeAssocFn::Split,
                &[Type::string(), Type::string()]
            ),
            Some(Type::vector(Type::string()))
        );
        // Strings.join(vec, sep) -> String.
        assert_eq!(
            assoc_fn_return_type(
                PreludeType::Strings,
                PreludeAssocFn::Join,
                &[Type::vector(Type::string()), Type::string()]
            ),
            Some(Type::string())
        );
        // Strings.trim(text) -> String.
        assert_eq!(
            assoc_fn_return_type(PreludeType::Strings, PreludeAssocFn::Trim, &[Type::string()]),
            Some(Type::string())
        );
        // Strings.replace(text, from, to) -> String.
        assert_eq!(
            assoc_fn_return_type(
                PreludeType::Strings,
                PreludeAssocFn::Replace,
                &[Type::string(), Type::string(), Type::string()]
            ),
            Some(Type::string())
        );
        // Strings.contains(text, substr) -> Bool.
        assert_eq!(
            assoc_fn_return_type(
                PreludeType::Strings,
                PreludeAssocFn::Contains,
                &[Type::string(), Type::string()]
            ),
            Some(Type::bool())
        );
        // Strings.starts_with(text, prefix) -> Bool.
        assert_eq!(
            assoc_fn_return_type(
                PreludeType::Strings,
                PreludeAssocFn::StartsWith,
                &[Type::string(), Type::string()]
            ),
            Some(Type::bool())
        );
        // Strings.to_uppercase(text) -> String.
        assert_eq!(
            assoc_fn_return_type(
                PreludeType::Strings,
                PreludeAssocFn::ToUppercase,
                &[Type::string()]
            ),
            Some(Type::string())
        );
        // Strings.to_lowercase(text) -> String.
        assert_eq!(
            assoc_fn_return_type(
                PreludeType::Strings,
                PreludeAssocFn::ToLowercase,
                &[Type::string()]
            ),
            Some(Type::string())
        );
    }
}

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
        }
    }

    /// T124c: Returns `true` if this prelude type is a **namespace-only**
    /// module — one whose name (e.g. `Log`) is never a runtime value but
    /// merely a container for associated functions. The datetime family
    /// returns `false` (their values ARE first-class); `Log` returns
    /// `true`. Used by the prelude-types tests to skip the datetime-only
    /// `is_prelude_datetime` assertion for namespace modules.
    pub const fn is_namespace_only(self) -> bool {
        matches!(self, PreludeType::Log)
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
        // (Regex) shipped in T124d = 7 total prelude types.
        assert_eq!(PreludeType::ALL.len(), 7);
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
}

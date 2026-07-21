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
    /// `Args` - the command-line arguments namespace (T124g). Wraps
    /// Rust's `std::env::args` iterator. Like [`Self::Log`] /
    /// [`Self::Toml`], `Args` is **never a runtime value** - it's a
    /// NAMESPACE exposing two associated functions:
    /// - `Args.list()` - collect program name + args into a
    ///   `Vector<String>`. Lowers to
    ///   `std::env::args().collect::<Vec<String>>()`.
    /// - `Args.get(index)` - get the arg at `index` (0 = program
    ///   name). Lowers to
    ///   `std::env::args().nth(i).unwrap_or_default()` (empty String
    ///   on out-of-bounds - NEVER panics, matching Buff's "no
    ///   panicking generated code" rule).
    ///
    /// `buff_type()` returns [`Type::Void`]; `is_namespace_only()`
    /// returns `true`. Args uses only Rust `std` (NO extern crate).
    /// This is the third std-only namespace module after Math/Strings
    /// (T124f) and the established precedent for future system-
    /// introspection namespaces.
    Args,
    /// `Env` - the environment-variables namespace (T124g). Wraps
    /// Rust's `std::env::var` / `set_var`. Like [`Self::Log`] /
    /// [`Self::Toml`], `Env` is **never a runtime value** - it's
    /// a NAMESPACE exposing three associated functions:
    /// - `Env.get("KEY")` - look up an env var. Lowers to
    ///   `std::env::var(k).ok()` (returns `Option<String>` - None
    ///   when unset OR invalid UTF-8; both are folded into None so
    ///   the surface stays panic-free).
    /// - `Env.set("KEY", "value")` - set an env var. Lowers to
    ///   `std::env::set_var(k, v)` (returns Void). NOTE: `set_var` is
    ///   `unsafe` in Rust 2024 edition; Buff emits the 2021 edition
    ///   so the call is safe today. A future edition bump will need
    ///   an `unsafe { ... }` wrapper here (tracked in
    ///   `decisions.md`).
    /// - `Env.has("KEY")` - test whether an env var is set. Lowers
    ///   to `std::env::var(k).is_ok()` (returns Bool).
    ///
    /// `buff_type()` returns [`Type::Void`]; `is_namespace_only()`
    /// returns `true`. Env uses only Rust `std` (NO extern crate).
    Env,
    /// `Base64` - the base64 codec namespace (T124h). Wraps the
    /// `base64` Rust crate. Like [`Self::Log`] / [`Self::Toml`] /
    /// [`Self::Random`], `Base64` is **never a runtime value** -
    /// it's a NAMESPACE exposing two associated functions:
    /// - `Base64.encode(bytes)` - encode a `Vector<Byte>` to a
    ///   base64 `String`. Wraps
    ///   `base64::Engine::encode(&general_purpose::STANDARD, bytes)`
    ///   (UFCS form so the `Engine` trait need not be in scope at
    ///   the call site).
    /// - `Base64.decode(string)` - decode a base64 `String` to a
    ///   `Vector<Byte>`. Wraps
    ///   `base64::Engine::decode(&general_purpose::STANDARD, s)
    ///   .unwrap_or_default()` (empty Vec on decode failure - NEVER
    ///   panics, matching Buff's "no panicking generated code" rule).
    ///
    /// `buff_type()` returns [`Type::Void`]; `is_namespace_only()`
    /// returns `true`. The `base64` crate is recorded in codegen
    /// `extern_crates` when a Buff program uses `Base64`.
    Base64,
    /// `Hex` - the hex codec namespace (T124h). Wraps the `hex`
    /// Rust crate. Like [`Self::Base64`], `Hex` is **never a runtime
    /// value** - it's a NAMESPACE exposing two associated functions:
    /// - `Hex.encode(bytes)` - encode a `Vector<Byte>` to a
    ///   lowercase hex `String`. Wraps `hex::encode(bytes)`.
    /// - `Hex.decode(string)` - decode a hex `String` to a
    ///   `Vector<Byte>`. Wraps `hex::decode(s).unwrap_or_default()`
    ///   (empty Vec on decode failure - NEVER panics).
    ///
    /// `buff_type()` returns [`Type::Void`]; `is_namespace_only()`
    /// returns `true`. The `hex` crate is recorded in codegen
    /// `extern_crates` when a Buff program uses `Hex`.
    Hex,
    /// `URLEncode` - the URL percent-encoding namespace (T124h).
    /// Wraps the `percent-encoding` Rust crate. Like [`Self::Base64`]
    /// / [`Self::Hex`], `URLEncode` is **never a runtime value** -
    /// it's a NAMESPACE exposing two associated functions:
    /// - `URLEncode.encode(string)` - percent-encode a String for
    ///   safe URL embedding. Wraps
    ///   `percent_encoding::utf8_percent_encode(s,
    ///   percent_encoding::NON_ALPHANUMERIC).to_string()`. The
    ///   `NON_ALPHANUMERIC` AsciiSet encodes everything that's not
    ///   an ASCII letter or digit (the canonical "encode special
    ///   characters" choice).
    /// - `URLEncode.decode(string)` - percent-DECODE a String.
    ///   Wraps `percent_encoding::percent_decode_str(s)
    ///   .decode_utf8_lossy().into_owned()`. Invalid UTF-8 sequences
    ///   become U+FFFD REPLACEMENT CHARACTER (lossy decode - NEVER
    ///   panics, matching Buff's "no panicking generated code" rule).
    ///
    /// `buff_type()` returns [`Type::Void`]; `is_namespace_only()`
    /// returns `true`. The `percent-encoding` crate is recorded in
    /// codegen `extern_crates` when a Buff program uses `URLEncode`.
    URLEncode,
    /// `UUID` - the UUID generator namespace (T124h). Wraps the
    /// `uuid` Rust crate. Like [`Self::Base64`] / [`Self::Hex`] /
    /// [`Self::URLEncode`], `UUID` is **never a runtime value** -
    /// it's a NAMESPACE exposing three associated functions:
    /// - `UUID.v4()` - generate a random v4 UUID. Wraps
    ///   `uuid::Uuid::new_v4().to_string()`.
    /// - `UUID.v7()` - generate a time-ordered v7 UUID. Wraps
    ///   `uuid::Uuid::now_v7().to_string()`.
    /// - `UUID.parse(string)` - validate whether a String is a
    ///   well-formed UUID. Wraps `uuid::Uuid::parse_str(s).is_ok()`
    ///   (returns Bool). Reuses the shared [`PreludeAssocFn::Parse`]
    ///   variant (5th overload for `Parse`, after DateTime / Date /
    ///   Toml / URL).
    ///
    /// All three return String/Bool surface types (NOT a `Uuid`
    /// value type) - Buff surfaces UUIDs as their canonical hyphen-
    /// separated String form, mirroring how most scripting languages
    /// surface them. A future task may add a `UUID` value type if
    /// instance methods become necessary.
    ///
    /// `buff_type()` returns [`Type::Void`]; `is_namespace_only()`
    /// returns `true`. The `uuid` crate is recorded in codegen
    /// `extern_crates` when a Buff program uses `UUID`.
    UUID,
    /// `URL` - the parsed-URL runtime value type (T124h). Wraps the
    /// `url` Rust crate. Constructed via the associated function
    /// `URL.parse(s)` (returns a `URL` value, NEVER panics - falls
    /// back to `url::Url::parse("about:blank").unwrap()` on parse
    /// failure, mirroring the Regex.compile infallible-ctor stance
    /// from T124d). Supports four instance methods:
    /// - `.scheme` - the URL scheme (`"https"`, `"file"`, ...).
    ///   Returns String.
    /// - `.host` - the URL host (`"example.com"`, ...). Returns
    ///   String (empty when absent - NEVER panics).
    /// - `.path` - the URL path (`"/index.html"`, ...). Returns
    ///   String.
    /// - `.query(key)` - look up a query parameter by key. Returns
    ///   `Option<String>` (None when the key is absent).
    ///
    /// This is the SECOND v1.4 prelude-type variant that is BOTH a
    /// real runtime value AND carries rich instance methods (after
    /// `Regex` in T124d). `buff_type()` returns [`Type::Url`] (a
    /// real value type, NOT [`Type::Void`] like `Base64` / `Hex` /
    /// `URLEncode` / `UUID`); `is_namespace_only()` returns `false`.
    /// The `url` crate is recorded in codegen `extern_crates` when a
    /// Buff program uses `URL`.
    ///
    /// Note the case: `URL` (all uppercase) is the prelude
    /// namespace / type name (mirrors Rust's `Url` but in
    /// Buff-flavored uppercase per the `DateTime` / `Regex`
    /// convention). The underlying Rust type is `url::Url` (capital
    /// U, lowercase rl).
    URL,
    /// `Yaml` - the YAML serialization namespace (T124i). Wraps the
    /// `serde_yml` Rust crate (the maintained fork of the
    /// deprecated/archived `serde_yaml` - do NOT use `serde_yaml`).
    /// Like [`Self::Toml`], `Yaml` is **never a runtime value** -
    /// it's a NAMESPACE exposing two associated functions:
    /// - `Yaml.parse(string)` - parse a YAML document into a Buff
    ///   `Map` (heterogeneous values); lowers to
    ///   `serde_yml::from_str::<std::collections::HashMap<String,
    ///   serde_yml::Value>>(s).unwrap_or_default()` (empty Map on
    ///   parse failure - NEVER panics, mirroring the Toml.parse
    ///   panic-free stance from T124e).
    /// - `Yaml.stringify(value)` - serialize a Map/value back to YAML
    ///   text; lowers to `serde_yml::to_string(&v).unwrap_or_default()`
    ///   (empty String on serialization failure - NEVER panics).
    ///
    /// `buff_type()` returns [`Type::Void`] (Yaml has no value
    /// representation, exactly like Toml); `is_namespace_only()`
    /// returns `true`. Mirrors the Toml namespace-only shape exactly
    /// (the YAML/TOML surface is structurally identical: parse +
    /// stringify a heterogeneous Map). The `serde_yml` crate is
    /// recorded in codegen `extern_crates` when a Buff program uses
    /// `Yaml`.
    Yaml,
    /// `Csv` - the CSV serialization namespace (T124i). Wraps the
    /// `csv` Rust crate (burntsushi/rust-csv). Like [`Self::Yaml`] /
    /// [`Self::Toml`], `Csv` is **never a runtime value** - it's a
    /// NAMESPACE exposing two associated functions:
    /// - `Csv.parse(string)` - parse a CSV document into a
    ///   `Vector<Vector<String>>` (uniform rows, NO header
    ///   special-casing per the spec - every row including the
    ///   header is surfaced as a `Vector<String>`). Lowers to
    ///   `csv::ReaderBuilder::new().has_headers(false)
    ///   .from_reader(s.as_bytes()).records()
    ///   .filter_map(|r| r.ok()).map(|r| r.iter().map(|f|
    ///   f.to_string()).collect::<Vec<String>>()).collect::<Vec<
    ///   Vec<String>>>()` (skip malformed rows via `.filter_map
    ///   (|r| r.ok())` - NEVER panics, mirroring the Toml.parse
    ///   panic-free stance).
    /// - `Csv.stringify(rows)` - serialize a
    ///   `Vector<Vector<String>>` to CSV text; lowers to a block
    ///   expression that builds a `csv::Writer` over `Vec<u8>`,
    ///   writes each row via `write_record`, and converts the
    ///   buffer to String (lossy/empty on failure - NEVER panics).
    ///
    /// `buff_type()` returns [`Type::Void`] (Csv has no value
    /// representation, exactly like Yaml/Toml); `is_namespace_only()`
    /// returns `true`. Mirrors the Toml namespace-only shape exactly
    /// (parse + stringify). The `csv` crate is recorded in codegen
    /// `extern_crates` when a Buff program uses `Csv`.
    Csv,
    /// `Path` - the filesystem-path runtime value type (T124j).
    /// Wraps `std::path::PathBuf` (the owned, mutable path type -
    /// Buff surfaces owned values; `&Path` is hidden from users).
    /// Constructed via the associated function `Path.join(a, b, ...)`
    /// (variadic - 2+ args; lowers to a chained
    /// `std::path::PathBuf::from(a).join(b).join(c)...`). Supports
    /// four instance methods:
    /// - `.parent()` - the parent directory. Returns `Option<Path>`
    ///   (None when the path has no parent - e.g. `/` or a bare
    ///   filename; NEVER panics). Wraps `recv.parent()
    ///   .map(|p| p.to_path_buf())`.
    /// - `.extension()` - the file extension (without the leading
    ///   `.`). Returns `Option<String>` (None when there's no
    ///   extension). Wraps `recv.extension().map(|e|
    ///   e.to_string())`.
    /// - `.basename()` - the trailing filename component. Returns
    ///   `String` (empty String when the path terminates in `..`
    ///   or `/` - NEVER panics). Wraps `recv.file_name()
    ///   .and_then(|n| n.to_str()).unwrap_or_default().to_string()`.
    /// - `.exists()` - test whether the path exists on disk.
    ///   Returns `Bool`. Wraps `recv.exists()`.
    ///
    /// This is the THIRD v1.4 prelude-type variant that is BOTH a
    /// real runtime value AND carries rich instance methods (after
    /// `Regex` T124d and `URL` T124h). `buff_type()` returns
    /// [`Type::Path`] (a real value type, NOT [`Type::Void`] like
    /// the namespace-only modules); `is_namespace_only()` returns
    /// `false`. NO extern crate is recorded for Path itself
    /// (`std::path` is in std) - but Path's instance-method
    /// accessors lower to `std::path::PathBuf::parent` /
    /// `extension` / `file_name` / `exists`, all std.
    /// Distinct from the namespace-only [`Self::Dir`] and
    /// [`Self::Tempfile`] modules (which it shipped alongside).
    Path,
    /// `Dir` - the directory-operations namespace (T124j). Wraps
    /// `std::fs::read_dir` / `create_dir_all` / `remove_dir_all` +
    /// the `walkdir` Rust crate (burntsushi/walkdir) for
    /// `Dir.walk`. Like [`Self::Log`] / [`Self::Toml`], `Dir` is
    /// **never a runtime value** - it's a NAMESPACE exposing four
    /// associated functions:
    /// - `Dir.list(path)` - list immediate directory entries. Wraps
    ///   `std::fs::read_dir(p).filter_map(|e| e.ok()).map(|e|
    ///   e.file_name().to_string_lossy().into_owned())
    ///   .collect::<Vec<String>>()` (skip inaccessible entries -
    ///   NEVER panics; returns `Vector<String>` of entry names,
    ///   NOT paths).
    /// - `Dir.create(path)` - create the directory (and any missing
    ///   parents - mirrors `mkdir -p`). Wraps
    ///   `std::fs::create_dir_all(p).ok()` (panic-free - discards
    ///   errors via `.ok()`; returns `Void`).
    /// - `Dir.remove(path)` - remove the directory and all its
    ///   contents recursively. Wraps
    ///   `std::fs::remove_dir_all(p).ok()` (panic-free - `.ok()`
    ///   discards errors; returns `Void`).
    /// - `Dir.walk(path)` - recursively walk the directory tree.
    ///   Wraps `walkdir::WalkDir::new(p).into_iter()
    ///   .filter_map(|e| e.ok()).map(|e| e.path().to_path_buf())
    ///   .collect::<Vec<std::path::PathBuf>>()` (skip inaccessible
    ///   entries - NEVER panics; returns `Vector<Path>` of all
    ///   paths found during the traversal, depth-first).
    ///
    /// `buff_type()` returns [`Type::Void`]; `is_namespace_only()`
    /// returns `true`. The `walkdir` crate is recorded in codegen
    /// `extern_crates` when a Buff program uses `Dir.walk`;
    /// `Dir.list` / `Dir.create` / `Dir.remove` use only std
    /// (`std::fs::*`) and record NO extern crate.
    Dir,
    /// `Tempfile` - the temporary-file namespace (T124j). Wraps
    /// the `tempfile` Rust crate (stebalian/tempfile) +
    /// `std::env::temp_dir`. Like [`Self::Log`] / [`Self::Toml`] /
    /// [`Self::Dir`], `Tempfile` is **never a runtime value** -
    /// it's a NAMESPACE exposing two associated functions:
    /// - `Tempfile.create()` - create a new empty temporary file
    ///   in the OS-default temp directory. Returns `Path` (the
    ///   kept file path - the underlying `NamedTempFile` is
    ///   dropped after the path is persisted via `into_temp_path()
    ///   .keep()`). Wraps `tempfile::NamedTempFile::new()
    ///   .map(|f| f.into_temp_path().keep().unwrap_or_default())
    ///   .unwrap_or_default()` (panic-free - empty PathBuf on
    ///   failure - NEVER panics).
    /// - `Tempfile.dir()` - the OS-default temp directory path.
    ///   Returns `Path`. Wraps `std::env::temp_dir()` (the
    ///   `tempfile::env::temp_dir()` is a re-export of the std
    ///   fn; we splice the std path directly so NO extern crate
    ///   is needed for this call alone).
    ///
    /// `buff_type()` returns [`Type::Void`]; `is_namespace_only()`
    /// returns `true`. The `tempfile` crate is recorded in codegen
    /// `extern_crates` when a Buff program uses `Tempfile.create`
    /// (the `into_temp_path().keep()` chain is a `tempfile`-crate
    /// API); `Tempfile.dir` records `tempfile` too for symmetry
    /// (a program using `Tempfile.dir` likely uses `Tempfile.create`
    /// too, but the narrow walker flags either call).
    Tempfile,
    /// `Hash` - the cryptographic-hash namespace (T124k). Wraps the
    /// `sha2` + `md5` RustCrypto crates. Like [`Self::Log`] /
    /// [`Self::Toml`] / [`Self::Base64`] / [`Self::Hex`] / [`Self::Dir`],
    /// `Hash` is **never a runtime value** - it's a NAMESPACE exposing
    /// three associated functions:
    /// - `Hash.sha256(data)` - SHA-256 hex digest. Wraps
    ///   `{ use sha2::Digest; hex::encode(sha2::Sha256::digest(d
    ///   .as_bytes())) }` (the block-scoped `use` brings the
    ///   `Digest` trait's `digest` method into scope WITHOUT
    ///   polluting the caller's namespace - `digest` is a trait
    ///   method, not an inherent method on `Sha256`). Returns the
    ///   canonical 64-char lowercase hex String.
    /// - `Hash.sha512(data)` - SHA-512 hex digest. Same shape as
    ///   `sha256` but `Sha512`. Returns the 128-char lowercase hex
    ///   String.
    /// - `Hash.md5(data)` - MD5 hex digest. Wraps
    ///   `hex::encode(md5::compute(d.as_bytes()).0)` (the `.0`
    ///   accesses the inner `[u8; 16]` of the `md5::Digest` tuple
    ///   struct). Returns the 32-char lowercase hex String. **MD5
    ///   is CRYPTOGRAPHICALLY BROKEN** - exposed for checksum
    ///   compatibility only (etags, content-addressable caches,
    ///   legacy interop); NEVER use for security.
    ///
    /// Each call accepts String or `Vector<Byte>` (anything
    /// `AsRef<[u8]>` at the codegen layer) and returns lowercase
    /// hex. The `sha2` crate is recorded in codegen `extern_crates`
    /// when a program uses `Hash.sha256` / `Hash.sha512` (and also
    /// for `HMAC.sha256` since HMAC wraps `Hmac<Sha256>`); the
    /// `md5` crate is recorded only for `Hash.md5`; the `hex` crate
    /// is recorded alongside each call (shared with the T124h
    /// `Hex` module's walker).
    ///
    /// `buff_type()` returns [`Type::Void`]; `is_namespace_only()`
    /// returns `true`. Mirrors Log / Toml / Base64 / Hex / Yaml /
    /// Csv / Dir / Tempfile exactly (parse-or-digest + return text).
    Hash,
    /// `HMAC` - the keyed-hash-MAC namespace (T124k). Wraps the
    /// `hmac` + `sha2` RustCrypto crates. Like [`Self::Log`] /
    /// [`Self::Toml`] / [`Self::Hash`], `HMAC` is **never a
    /// runtime value** - it's a NAMESPACE exposing one associated
    /// function:
    /// - `HMAC.sha256(key, data)` - HMAC-SHA256 hex digest.
    ///   Wraps `{ use hmac::Mac; hmac::Hmac::<sha2::Sha256>
    ///   ::new_from_slice(k.as_bytes()).map(|mut mac| {
    ///   mac.update(d.as_bytes()); hex::encode(mac.finalize()
    ///   .into_bytes()) }).unwrap_or_default() }` (block-scoped
    ///   `use` brings the `Mac` trait's `update` / `finalize`
    ///   methods into scope WITHOUT polluting the caller's
    ///   namespace). `new_from_slice` returns `Result<Hmac<Sha256>,
    ///   MacError>` and accepts ANY key length (HMAC has no fixed
    ///   key size); the `.map(...).unwrap_or_default()` collapses
    ///   the Err branch to an empty String - **NEVER panics**,
    ///   matching Buff's "no panicking generated code" rule.
    ///
    /// Both args accept String or `Vector<Byte>` (anything
    /// `AsRef<[u8]>` at the codegen layer); the return is the
    /// 64-char lowercase hex String. The `hmac` + `sha2` crates
    /// are recorded in codegen `extern_crates` when a program
    /// uses `HMAC.sha256` (the `hmac::Hmac<sha2::Sha256>` path
    /// needs both); the `hex` crate is recorded alongside
    /// (shared walker).
    ///
    /// `buff_type()` returns [`Type::Void`]; `is_namespace_only()`
    /// returns `true`. Mirrors Log / Toml / Hash exactly. The
    /// all-caps `HMAC` spelling mirrors the `UUID` / `URL`
    /// convention (the canonical acronym is all-uppercase; Buff
    /// surfaces it as a PascalCase module name).
    HMAC,
    /// `OS` - the operating-system-introspection namespace
    /// (T124l). Wraps `std::env::consts::{OS,ARCH}` + the
    /// `num_cpus` Rust crate. Like [`Self::Log`] / [`Self::Toml`]
    /// / [`Self::Hash`] / [`Self::HMAC`], `OS` is **never a
    /// runtime value** - it's a NAMESPACE exposing four
    /// associated functions:
    /// - `OS.name()` - the OS name (`"linux"` / `"macos"` /
    ///   `"windows"`). Zero args. Returns `String`. Wraps
    ///   `std::env::consts::OS.to_string()` (compile-time const).
    /// - `OS.arch()` - the CPU architecture (`"x86_64"` /
    ///   `"aarch64"`). Zero args. Returns `String`. Wraps
    ///   `std::env::consts::ARCH.to_string()` (compile-time const).
    /// - `OS.hostname()` - the machine hostname. Zero args.
    ///   Returns `String` (empty String when neither COMPUTERNAME
    ///   nor HOSTNAME is set - NEVER panics). Wraps
    ///   `std::env::var("COMPUTERNAME").or_else(|_|
    ///   std::env::var("HOSTNAME")).unwrap_or_default()` - the
    ///   bare-minimum env-var approach covering Windows
    ///   (COMPUTERNAME) + Unix (HOSTNAME). NO `hostname` crate
    ///   added (the spec explicitly forbids it).
    /// - `OS.cpus()` - the number of logical CPUs. Zero args.
    ///   Returns `Int`. Wraps `num_cpus::get() as i64`. The
    ///   `num_cpus` crate is recorded in codegen `extern_crates`
    ///   when a program uses `OS.cpus` (the narrow walker flags
    ///   ONLY the `cpus` method name - `name`/`arch`/`hostname`
    ///   use std only and record NO extern crate).
    ///
    /// `buff_type()` returns [`Type::Void`]; `is_namespace_only()`
    /// returns `true`. Mirrors Log / Toml / Args / Env / Hash /
    /// HMAC exactly (every call returns a value, NEVER an `OS`
    /// value type). The two-case PascalCase spelling (`OS`) mirrors
    /// `UUID` / `URL` / `HMAC` (all-uppercase acronyms surfaced as
    /// PascalCase module names).
    OS,
    /// `Process` - the spawned-process runtime value type (T124l).
    /// Wraps `Option<std::process::Child>` (the `Option` wrapper
    /// lets `Process.spawn` be panic-free - a spawn failure
    /// collapses to `None`; `.wait()` / `.id()` then operate on
    /// the `Option` via `.map(...).unwrap_or_default()`).
    /// Constructed via the associated function
    /// `Process.spawn(command, args)` (two args: a command String
    /// and a `Vector<String>` of args - does NOT shell out; the
    /// command and args are passed SEPARATELY to
    /// `std::process::Command::new(cmd).args(args)` so there's NO
    /// shell-injection vector, matching the spec's safety stance).
    /// Supports two instance methods:
    /// - `.wait() -> Int` - block until the process exits, then
    ///   return the exit code. Zero args. Wraps
    ///   `recv.map(|mut c| c.wait().map(|s| s.code().unwrap_or_default())
    ///   .unwrap_or_default()).unwrap_or_default()` (the outer
    ///   Option handles the spawn-failed case; the middle Result
    ///   handles wait() failure; the inner Option handles
    ///   signal-terminated processes that have no exit code - all
    ///   collapse to `0` via `unwrap_or_default()`, NEVER panics).
    /// - `.id() -> Int` - the OS process ID. Zero args. Wraps
    ///   `recv.map(|c| c.id() as i64).unwrap_or_default()` (0
    ///   when the spawn failed or the process has already exited
    ///   and been reaped - NEVER panics).
    ///
    /// The `Process.exit(code)` associated function (zero return -
    /// terminates the program immediately via
    /// `std::process::exit(code as i32)`) is ALSO exposed on this
    /// type. It is NOT a runtime value constructor - it's a
    /// side-effecting terminal call (returns Void).
    ///
    /// This is the FOURTH v1.4 prelude-type variant that is BOTH
    /// a real runtime value AND carries instance methods (after
    /// `Regex` T124d, `URL` T124h, `Path` T124j). `buff_type()`
    /// returns [`Type::Process`] (a real value type, NOT
    /// [`Type::Void`] like the namespace-only `OS` module it
    /// shipped alongside); `is_namespace_only()` returns `false`.
    /// `Process.*` uses ONLY `std::process` - NO extern crate
    /// recorded (mirrors the Path/Dir.list/Tempfile.dir std-only
    /// stance from T124j).
    ///
    /// MUST NOT (per spec): signal handling, shell expansion,
    /// privilege management. The `Process` surface is the
    /// minimal "spawn a child, wait for it, get its PID, exit
    /// yourself" subset that every scripting language converges on.
    Process,
    /// `TCP` - the TCP-client-connection module (T124m).
    /// Namespace-only (mirrors Log / Toml / Math / OS) - the
    /// namespace itself is never a runtime value, but the
    /// associated function `TCP.connect(host, port)` returns the
    /// runtime-value type [`Type::Connection`] (which wraps
    /// `Option<tokio::net::TcpStream>` and carries the `.send` /
    /// `.recv` / `.close` instance methods). Buff surface:
    /// - `TCP.connect(host: String, port: Int) -> Connection`
    ///   (the only assoc fn - opens a TCP connection; the codegen
    ///   emits `tokio::net::TcpStream::connect(format!("{}:{}",
    ///   h, p)).await.ok()` - panic-free via Option collapse).
    /// - `connection.send(data: String)` - write bytes (instance
    ///   method on Type::Connection).
    /// - `connection.recv() -> Vector<Byte>` - read bytes (instance
    ///   method on Type::Connection).
    /// - `connection.close()` - graceful shutdown (instance method
    ///   on Type::Connection).
    ///
    /// This is a NAMESPACE module: `is_namespace_only()` returns
    /// `true`. `buff_type()` returns [`Type::Void`] (the namespace
    /// itself has no value representation - only `TCP.connect`'s
    /// return value does, and that's typed [`Type::Connection`]).
    /// `TCP.*` records `tokio` in codegen `extern_crates`
    /// (idempotent with the existing `tokio` walker).
    TCP,
    /// `UDP` - the UDP-socket module (T124m). Namespace-only
    /// (mirrors TCP / Log / Toml / OS). The associated function
    /// `UDP.bind(host, port)` returns the runtime-value type
    /// [`Type::Socket`] (which wraps `Option<tokio::net::UdpSocket>`
    /// and carries the `.send_to` / `.recv_from` instance methods).
    /// Buff surface:
    /// - `UDP.bind(host: String, port: Int) -> Socket` (the only
    ///   assoc fn - binds a UDP socket; the codegen emits
    ///   `tokio::net::UdpSocket::bind(format!("{}:{}", h, p))
    ///   .await.ok()` - panic-free via Option collapse).
    /// - `socket.send_to(data: String, addr: String)` - send a
    ///   datagram (instance method on Type::Socket).
    /// - `socket.recv_from() -> Tuple` - receive a datagram
    ///   (instance method on Type::Socket).
    ///
    /// This is a NAMESPACE module: `is_namespace_only()` returns
    /// `true`. `buff_type()` returns [`Type::Void`]. `UDP.*`
    /// records `tokio` in codegen `extern_crates` (idempotent
    /// with the existing `tokio` walker).
    UDP,
    /// `WebSocket` - the WebSocket-client-connection module
    /// (T124m). Namespace-only (mirrors TCP / UDP / Log / Toml /
    /// OS). The associated function `WebSocket.connect(url)`
    /// returns the runtime-value type [`Type::WsConnection`]
    /// (which wraps `Option<tokio_tungstenite::WebSocketStream<
    /// MaybeTlsStream<TcpStream>>>` and carries the `.send` /
    /// `.recv` / `.close` instance methods). Buff surface:
    /// - `WebSocket.connect(url: String) -> WsConnection` (the
    ///   only assoc fn - opens a WebSocket connection; the codegen
    ///   emits `tokio_tungstenite::connect_async(url).await.ok()
    ///   .map(|(ws, _)| ws)` - panic-free via Option collapse).
    /// - `wsconn.send(text: String)` - send a Text frame (instance
    ///   method on Type::WsConnection).
    /// - `wsconn.recv() -> String` - receive next message as text
    ///   (instance method on Type::WsConnection).
    /// - `wsconn.close()` - graceful shutdown (instance method on
    ///   Type::WsConnection).
    ///
    /// This is a NAMESPACE module: `is_namespace_only()` returns
    /// `true`. `buff_type()` returns [`Type::Void`]. `WebSocket.*`
    /// records `tokio-tungstenite` + `futures-util` in codegen
    /// `extern_crates` (via the narrow
    /// `program_uses_tokio_tungstenite` walker; tokio is recorded
    /// transitively via tokio-tungstenite's dependency on it, but
    /// the existing `program_uses_tokio` walker does NOT fire on
    /// WebSocket.* alone since it's gated on the bare-Ident
    /// `sleep(...)` free-fn call - the new walker covers the
    /// WebSocket.* paths explicitly).
    WebSocket,
    /// `Channel` - the MPSC channel factory namespace (T2 v1.13
    /// wave 1). Namespace-only (mirrors Log / Toml / TCP / UDP /
    /// WebSocket): the namespace itself is never a runtime value,
    /// but the associated function `Channel.new(buf_size)` returns
    /// a tuple of the runtime-value types `Sender<T>` and
    /// `Receiver<T>` (both wrapping `tokio::sync::mpsc::Sender` /
    /// `Receiver` via `buff_lang_runtime`). Buff surface:
    /// - `Channel.new(buf_size: Int) -> (Sender<T>, Receiver<T>)`
    ///   (the only assoc fn - constructs a bounded MPSC channel pair;
    ///   the codegen emits `buff_lang_runtime::Channel::new(buf_size)`
    ///   which wraps `tokio::sync::mpsc::channel(buf_size)`). NO
    ///   turbofish at the call site - Rust's type inference derives
    ///   `T` from subsequent `sender.send(value)` / `receiver.recv()`
    ///   usage.
    ///
    /// Instance methods on the runtime-value types:
    /// - `sender.send(value: T)` - instance method on Type::Sender
    ///   (NOT on `Channel`). Returns Void in MVP (the Result is
    ///   collapsed to Option via `.ok()` and discarded, mirroring
    ///   Connection.send from T124m). Async via auto-await.
    /// - `receiver.recv()` - instance method on Type::Receiver.
    ///   Returns Option<T>. Async via auto-await.
    /// - `receiver.close()` - instance method on Type::Receiver.
    ///   Returns Void. Sync (NOT async).
    ///
    /// This is a NAMESPACE module: `is_namespace_only()` returns
    /// `true`. `buff_type()` returns [`Type::Void`] (the namespace
    /// itself has no value representation - only `Channel.new`'s
    /// return value does, and that's typed a `(Sender, Receiver)`
    /// tuple). `Channel.*` records `buff-lang-runtime` + `tokio`
    /// in codegen `extern_crates` (via the narrow
    /// `program_uses_namespace("Channel")` walker).
    ///
    /// Single-consumer MPSC ONLY for MVP (broadcast channels are
    /// deferred to v1.18+ per the T2 spec's REDUCED SCOPE).
    Channel,
    /// `Tensor` - the N-dimensional array namespace (T8).
    /// EXPERIMENTAL badge per T8 spec. Pure-Rust `buff-tensor` crate
    /// (CPU-only via rayon per T6 decision
    /// `.sisyphus/decisions/wgsl-extensibility-v1x.md`). f32 / rank ≤
    /// 4 for MVP — f64/i64 + rank > 4 deferred to v1.18+. GPU
    /// dispatch for elementwise ops is a v1.18+ enhancement; matmul +
    /// reduce GPU paths are ~1500 LOC / ~15 days and explicitly
    /// deferred per T6.
    ///
    /// Assoc fns: `Tensor.zeros(shape)`, `Tensor.ones(shape)`,
    /// `Tensor.from_vec(data, shape)`, `Tensor.filled(shape, value)`.
    /// Each returns `Type::Unknown` for MVP (the coordinated
    /// `Type::Tensor` variant + codegen lowering arm is a follow-up
    /// task outside the T8 shared zone — sibling Wave 2 coordination
    /// concern). This forward-declaration lets `buff check` validate
    /// the syntax today; `buff run` integration lands with the
    /// coordinated sibling task.
    Tensor,
    /// T9: `Image` — a 2D raster image with 8-bit RGBA pixel data.
    /// Wraps `buff_image::Image` (a safe wrapper around the `image`
    /// crate's `DynamicImage`). Constructed via the associated
    /// functions `Image.from_path(path)` (load from disk) /
    /// `Image.from_bytes(bytes)` (decode an in-memory buffer);
    /// supports 10 instance methods: `img.width()`, `img.height()`,
    /// `img.get_pixel(x,y)`, `img.set_pixel(x,y,color)`,
    /// `img.save(path)`, `img.grayscale()`, `img.invert()`,
    /// `img.resize(w,h)`, `img.crop(x,y,w,h)`, `img.blur(sigma)`.
    ///
    /// This is the FIFTH runtime-value-with-rich-instance-methods
    /// type (after Regex T124d / URL T124h / Path T124j / Process
    /// T124l). `buff_type()` returns [`Type::Image`] (a real value
    /// type); `is_namespace_only()` returns `false`. The `image`
    /// crate is recorded in codegen `extern_crates` when a Buff
    /// program uses `Image` (mirrors the chrono / regex / tracing
    /// codegen-only linking boundary). CPU-only per Metis G7 lock
    /// (NO GPU dispatch — defer to v1.18+).
    Image,
    /// `Signal` - the time-domain signal-processing namespace (T11).
    /// EXPERIMENTAL badge per T11 spec. Wraps the in-tree pure-Rust
    /// `buff-dsp` crate (CPU-only via `rustfft` + `realfft` +
    /// `apodize`; per Metis G7 NO GPU). `Signal.from_vec(data,
    /// sample_rate)` ctor + instance methods `s.fft()` /
    /// `s.ifft(spectrum)` / `s.lowpass(cutoff_hz)` /
    /// `s.highpass(cutoff_hz)` / `s.bandpass(low_hz, high_hz)` /
    /// `s.apply_window(window)` / `s.spectrogram(window_size)` /
    /// `s.magnitude()` / `s.phase()`. NO real-time streaming (Signal
    /// is `Vec`-backed, not Stream-backed — deferred to v1.18+ per
    /// the T11 spec). NO adaptive filters (LMS, RLS — deferred to
    /// v1.18+). Records `buff_dsp` + `rustfft` + `realfft` +
    /// `apodize` in codegen `extern_crates` when a Buff program uses
    /// `Signal.*` (mirrors the chrono / regex / tracing codegen-only
    /// linking boundary). Mirrors the namespace-only shape (the
    /// namespace itself has no value representation; only the
    /// `Signal.from_vec` return value does, typed `Signal`). FFI-safe:
    /// the wrapper complies with all 6 rules from
    /// `crates/buff-lang-ffi-guide/GUIDE.md` (no raw pointers, owned
    /// Vec at the boundary, infallible surface, Send + 'static, no
    /// lifetimes, every body catch_unwind-wrapped).
    Signal,
    /// `Window` - the precomputed window-function namespace (T11).
    /// EXPERIMENTAL badge per T11 spec. Wraps `apodize` (the pure-Rust
    /// window crate) behind `buff_dsp::Window`. Three assoc fns:
    /// `Window.hann(n)` / `Window.hamming(n)` /
    /// `Window.blackman(n)` — each returns an opaque `Window` value
    /// passed to `Signal.apply_window(window)`. Mirrors `Signal`'s
    /// namespace-only shape. Pure-Rust, no native deps.
    Window,
    /// `Spectrum` - the FFT-frequency-spectrum runtime-value type
    /// (T11). EXPERIMENTAL badge per T11 spec. Returned by
    /// `Signal.fft()` / `Signal.spectrogram(window_size)`. Instance
    /// methods: `spec.len()` / `spec.is_empty()` / `spec.freqs()` /
    /// `spec.magnitudes()` / `spec.phases()`. Mirrors `Regex` / `URL`
    /// / `Path` / `Process`'s runtime-value-with-rich-instance-methods
    /// shape. Carries `Vec<Complex>` + sample_rate — hermitian half
    /// (`N/2 + 1` bins) of a length-N real input.
    Spectrum,
    /// `DataFrame` - the columnar-DataFrame runtime-value type (T7).
    /// Wraps the in-tree `buff-dataframe` crate
    /// (`buff_dataframe::DataFrame`). Constructed via the associated
    /// functions `DataFrame.from_csv(path)` /
    /// `DataFrame.from_json(path)`; supports the instance methods
    /// `df.select(cols)` / `df.filter(pred)` / `df.sort(col)` /
    /// `df.head(n)` / `df.len()` / `df.join(other, on)` /
    /// `df.group_by(col)` (returns a DataFrame whose `.agg(col, op)`
    /// chains per-group aggregation) / `df.agg(col, op)` /
    /// `df.to_table_string()`. Mirrors `Regex` / `URL` / `Path` /
    /// `Process` / `Image`'s runtime-value-with-rich-instance-methods
    /// shape. Carries a `BTreeMap<String, Series>` + an ordered
    /// `Vec<String>` of column names (the schema).
    ///
    /// `buff_type()` returns [`Type::DataFrame`] (a real value type,
    /// NOT [`Type::Void`] like `Log`). `is_namespace_only()` returns
    /// `false` (DataFrame IS a runtime value). EXPERIMENTAL badge per
    /// T7 spec — surface may evolve before v1.18 stabilisation.
    DataFrame,
    /// T10 (v1.13 frameworks): the AudioBuffer runtime-value type.
    /// Maps to `buff_audio::AudioBuffer` at codegen time. Constructed
    /// via `AudioBuffer.from_path(path)` (decode WAV/MP3/FLAC/Vorbis)
    /// or `AudioBuffer.from_samples(samples, sample_rate, channels)`.
    /// Carries the instance methods `.samples() -> Vector<Float>`,
    /// `.sample_rate() -> Int`, `.channels() -> Int`,
    /// `.duration_secs() -> Float`, `.save(path) -> Void`,
    /// `.amplify(factor: Float) -> Void`, `.normalize(target: Float)
    /// -> Void`, `.mix(other: AudioBuffer) -> Void`, `.slice(start_sec:
    /// Float, end_sec: Float) -> AudioBuffer`.
    ///
    /// `buff_type()` returns [`Type::Audio`] (a real value type, NOT
    /// [`Type::Void`] like `Log`). `is_namespace_only()` returns
    /// `false`. EXPERIMENTAL badge per T10 spec — surface may evolve
    /// before v1.18 stabilisation (real-time playback deferred).
    ///
    /// Out of scope for the T10 MVP per Metis G7 (CPU-only, NO GPU
    /// dispatch) and the task spec's "must NOT implement" list:
    /// - Real-time playback (deferred to v1.18+).
    /// - Synthesis (sine/square/noise generators — those go in
    ///   buff-dsp T11).
    /// - Encoding to non-WAV formats (FLAC/MP3 encoding is heavy).
    Audio,
    /// T12 (v1.13 frameworks wave 2): the `World` Entity-Component-
    /// System namespace. Wraps the in-tree `buff-ecs` crate
    /// (`buff_ecs::World`) backed by the pure-Rust `hecs` 0.10 crate
    /// (preferred over `bevy_ecs` for the smaller surface + single-
    /// crate dep + no bevy_utils/tasks/reflect baggage — full
    /// rationale in `Cargo.toml` workspace.dependencies entry).
    ///
    /// Buff surface:
    /// - `World.new() -> World` — empty world ctor (assoc fn).
    /// - `world.spawn(component) -> Entity` — entity with 1 component.
    /// - `world.spawn_two(a, b) -> Entity` — entity with 2 components.
    /// - `world.insert(entity, component) -> Void` — add/overwrite.
    /// - `world.remove(entity, ComponentType) -> Option<T>` — drop+return.
    /// - `world.query<ComponentTypes...>() -> Vector<(Entity, T)>` — owned.
    /// - `world.add_system(system_fn) -> Void` — register sequential system.
    /// - `world.tick() -> Void` — run all systems once.
    /// - `world.insert_resource(value) -> Void` — typed global resource.
    /// - `world.get_resource<T>() -> Option<T>` — borrow resource.
    ///
    /// `buff_type()` returns [`Type::World`] (the coordinated variant
    /// in `ty.rs` line 462, added by the parallel T12 ty.rs sibling
    /// task). `is_namespace_only()` returns `false` (World IS a runtime
    /// value, like Regex / Image / DataFrame).
    ///
    /// EXPERIMENTAL badge per T12 spec — the surface may evolve
    /// before v1.18 stabilisation (parallel system scheduling,
    /// change detection, and events are explicitly deferred). NO
    /// rendering pipeline (T16 buff-game uses existing WGSL). NO
    /// asset loading (T16). NO parallel system scheduling
    /// (sequential `tick()` for MVP). NO change detection / events.
    ///
    /// Records `buff_ecs` + `hecs` in codegen `extern_crates` when a
    /// Buff program uses `World.*` (mirrors the chrono / regex /
    /// tracing codegen-only linking boundary). FFI-safe: the wrapper
    /// complies with all 6 rules from
    /// `crates/buff-lang-ffi-guide/GUIDE.md` (no raw pointers —
    /// Entity is a transparent `(u32, u32)` newtype; Rust owns the
    /// hecs::World heap; fallible ops return Result<T, EcsError>;
    /// Send + 'static on every public type; no lifetimes — queries
    /// return owned Vec; every public body catch_unwind-wrapped).
    World,
    /// T12 (v1.13 frameworks wave 2): the `Entity` opaque id type
    /// returned by `world.spawn(...)`. Maps to `buff_ecs::Entity` at
    /// codegen time (a transparent newtype over `hecs::Entity`, which
    /// is a `(u32 id, u32 generation)` pair — Copy + Eq + Hash + Send
    /// + Sync + 'static). Users compare entities by value, store them
    /// in collections, and pass them back to `world.insert` /
    /// `world.remove` / `world.despawn`.
    ///
    /// Buff surface:
    /// - `entity.id() -> Int` — the stable slot id (read-only accessor).
    /// - `entity.to_bits() -> Int` — packed `(id, generation)` u64
    ///   for serialization (round-trips via `Entity.from_bits(bits)`).
    ///
    /// `buff_type()` returns [`Type::Entity`] (the coordinated variant
    /// in `ty.rs` line 477, added by the parallel T12 ty.rs sibling
    /// task). `is_namespace_only()` returns `false` (Entity IS a
    /// runtime value, like Regex / Image / DataFrame).
    ///
    /// EXPERIMENTAL badge per T12 spec. NO raw pointers (FFI guide
    /// R1) — Entity is a value-type id, not a pointer into Rust's
    /// heap. The `hecs::Entity` it wraps is `pub(crate)` — never
    /// exposed across the FFI boundary.
    Entity,
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
        // T124g: Args / Env - two namespace-only system modules
        // (both wrap Rust `std::env`). Both mirror Log's namespace-only
        // shape. The free fns `input()` and `sleep()` (also T124g) live
        // in the free-function prelude ([`crate::prelude::PreludeFn`]),
        // NOT here.
        PreludeType::Args,
        PreludeType::Env,
        // T124h: Base64 / Hex / URLEncode / UUID / URL - five web
        // modules. Four namespaces (Base64 / Hex / URLEncode / UUID)
        // mirror Log / Toml / Math / Random's namespace-only shape.
        // URL is the second runtime-value-with-rich-instance-methods
        // type (after Regex T124d) - it's a parsed URL value with
        // `.scheme` / `.host` / `.path` / `.query(key)` accessors.
        PreludeType::Base64,
        PreludeType::Hex,
        PreludeType::URLEncode,
        PreludeType::UUID,
        PreludeType::URL,
        // T124i: Yaml / Csv - two data-format namespace modules
        // wrapping the `serde_yml` and `csv` Rust crates. Both mirror
        // Log / Toml's namespace-only shape exactly (parse + stringify
        // a heterogeneous Map for Yaml; parse + stringify a uniform
        // Vector<Vector<String>> for Csv). NO runtime value type.
        PreludeType::Yaml,
        PreludeType::Csv,
        // T124j: Path / Dir / Tempfile - three filesystem modules.
        // Path is the third runtime-value-with-rich-instance-methods
        // type (after Regex T124d + URL T124h) - it's a `PathBuf`
        // value with `.parent()` / `.extension()` / `.basename()` /
        // `.exists()` instance methods. Dir + Tempfile are
        // namespace-only modules (mirror Log / Toml / Math / Random /
        // Strings / Args / Env / Base64 / Hex / URLEncode / UUID /
        // Yaml / Csv): `Dir.list` / `Dir.create` / `Dir.remove` /
        // `Dir.walk` + `Tempfile.create` / `Tempfile.dir`.
        PreludeType::Path,
        PreludeType::Dir,
        PreludeType::Tempfile,
        // T124k: Hash / HMAC - two crypto namespace modules
        // wrapping the `sha2` + `md5` + `hmac` RustCrypto crates.
        // Both mirror Log / Toml / Base64 / Hex / Yaml / Csv / Dir /
        // Tempfile's namespace-only shape (digest + return text).
        // NO runtime value type - every call returns String (hex).
        PreludeType::Hash,
        PreludeType::HMAC,
        // T124l: OS / Process - two system modules. OS is
        // namespace-only (mirrors Log / Toml / Hash / HMAC) - 4
        // assoc fns (name / arch / hostname / cpus). Process is
        // the fourth runtime-value-with-instance-methods type
        // (after Regex T124d, URL T124h, Path T124j) - a spawned
        // child handle wrapping `Option<std::process::Child>` with
        // `.wait() -> Int` + `.id() -> Int` instance methods; the
        // ctor `Process.spawn(cmd, args)` and the side-effecting
        // `Process.exit(code)` (returns Void) are assoc fns on the
        // same type.
        PreludeType::OS,
        PreludeType::Process,
        // T124m: TCP / UDP / WebSocket - three networking modules.
        // All three are namespace-only (mirror Log / Toml / OS) - the
        // namespace itself has no value representation, but each
        // ships an associated function whose return type is a real
        // runtime value (Connection / Socket / WsConnection - all
        // wrap `Option<tokio::*>` and carry instance methods). The
        // TCP / UDP assoc fns (connect / bind) are dispatched on the
        // (TCP, Connect) / (UDP, Bind) pair - new variants since no
        // prior prelude type exposed those verbs. WebSocket.connect
        // reuses the shared `Connect` variant (new in T124m) shared
        // with TCP.connect (mirrors `Parse` / `Get` / `Encode` shared-
        // variant pattern). The 8 instance methods (Connection.send
        // / recv / close, Socket.send_to / recv_from, WsConnection.
        // send / recv / close) are dispatched on the receiver type
        // (mirrors Regex / URL / Path / Process's instance-method-
        // carrying runtime-value pattern).
        PreludeType::TCP,
        PreludeType::UDP,
        PreludeType::WebSocket,
        // T2 v1.13 wave 1: Channel - MPSC channel factory namespace.
        // Returns (Sender<T>, Receiver<T>) tuple from Channel.new.
        // Instance methods on Sender / Receiver runtime-value types.
        PreludeType::Channel,
        // T8 v1.13 wave 2: Tensor - N-dimensional array namespace.
        // Pure-Rust `buff-tensor` crate (CPU-only via rayon per T6
        // decision `.sisyphus/decisions/wgsl-extensibility-v1x.md`).
        // EXPERIMENTAL badge per T8 spec line 1477 ("Register with
        // experimental badge"). The assoc fns `Tensor.zeros`,
        // `Tensor.ones`, `Tensor.from_vec`, `Tensor.filled` construct
        // runtime `buff_tensor::Tensor` values. The MVP is f32-only
        // (rank <= 4) — f64/i64 + rank > 4 deferred to v1.18+.
        // Codegen lowering for these assoc fns is a follow-up task
        // (it needs a coordinated `Type::Tensor` variant in
        // `crates/buff-lang-types/src/ty.rs` which is OUTSIDE the
        // T8 shared zone per the v1x-frameworks sibling-task rules).
        // The assoc fns return `Type::Unknown` here as a forward-
        // declaration contract — the codegen + `Type::Tensor` variant
        // is added in a coordinated sibling task that doesn't conflict
        // with the parallel Wave 2 tasks (T7/T9/T10/T11/T12/T25).
        PreludeType::Tensor,
        // T9: Image — runtime-value type with rich instance methods.
        // Mirrors Regex/URL/Path/Process. Codegen lowering lives in
        // the buff-image crate (`buff_image::Image::*`); the codegen
        // arm + PreludeAssocFn/InstanceFn entries are added in this
        // same commit (T9 owns the full Image surface, unlike T8
        // which forward-declares Tensor only).
        PreludeType::Image,
        // T11: Signal / Window / Spectrum - three signal-processing
        // types wrapping the in-tree `buff-dsp` crate. Signal + Window
        // are namespace-only modules (mirror Log / Toml / Math /
        // Random); Spectrum is the sixth runtime-value-with-instance-
        // methods type (after Regex / URL / Path / Process / Image).
        // All three are CPU-only per Metis G7 lock (NO GPU dispatch).
        PreludeType::Signal,
        PreludeType::Window,
        PreludeType::Spectrum,
        // T7: DataFrame — columnar-DataFrame runtime-value type with
        // rich instance methods (8th such type after Regex/URL/Path/
        // Process/TCP-Connection/UDP-Socket/WebSocket-WsConnection/
        // Image/Spectrum). Mirrors the same pattern:
        // `DataFrame.from_csv` / `DataFrame.from_json` are the assoc-
        // fn ctors (Buff §7 permits the `Type.from_*()` ctor form);
        // `df.select(cols)` / `df.filter(pred)` / `df.sort(col)` /
        // `df.head(n)` / `df.len()` / `df.join(other, on)` /
        // `df.group_by(col)` / `df.agg(col, op)` are instance methods
        // dispatched on the DataFrame receiver. Codegen lowering lives
        // in the buff-dataframe crate (`buff_dataframe::DataFrame::*`).
        // EXPERIMENTAL badge per T7 spec line 1373 ("Register with
        // experimental badge"). CPU-only per Metis G7.
        PreludeType::DataFrame,
        // T12 (v1.13 wave 2): World + Entity - the ECS foundation
        // types wrapping the in-tree `buff-ecs` crate (`buff_ecs::World`
        // + `buff_ecs::Entity`) backed by `hecs` 0.10. Both forward-
        // declare via `Type::Unknown` (mirrors the T8 Tensor precedent)
        // — the coordinated `Type::World` / `Type::Entity` variants in
        // `ty.rs` + codegen lowering arms are sibling tasks OUTSIDE
        // the T12 shared zone. `World` is namespace-only-shaped (the
        // ctor `World.new()` returns the runtime value); `Entity` is
        // the runtime-value id returned by `world.spawn(...)`.
        // EXPERIMENTAL badge per T12 spec. Foundational for T16
        // buff-game. NO rendering / NO asset loading / NO parallel
        // scheduling / NO change detection — all explicitly deferred.
        PreludeType::World,
        PreludeType::Entity,
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
            // T124g: the Args prelude type name. The codegen splices
            // `std::env::args().collect::<Vec<String>>()` /
            // `std::env::args().nth(i).unwrap_or_default()` for the two
            // associated functions.
            PreludeType::Args => "Args",
            // T124g: the Env prelude type name. The codegen splices
            // `std::env::var(k).ok()` / `std::env::set_var(k, v)` /
            // `std::env::var(k).is_ok()` for the three associated
            // functions.
            PreludeType::Env => "Env",
            // T124h: the Base64 prelude type name. The codegen splices
            // `base64::Engine::encode(&base64::engine::general_purpose::STANDARD,
            // bytes)` (UFCS form so the Engine trait need not be in
            // scope at the call site) and the symmetric `decode` for
            // `Base64.decode(s) -> Vec<u8>` (with `.unwrap_or_default()`
            // for the panic-free fallback).
            PreludeType::Base64 => "Base64",
            // T124h: the Hex prelude type name. Mirrors the Rust crate
            // name so codegen can splice `hex::encode(bytes)` /
            // `hex::decode(s).unwrap_or_default()` paths without
            // rewriting.
            PreludeType::Hex => "Hex",
            // T124h: the URLEncode prelude type name. Buff-flavored
            // name (clearer than `PercentEncoding`); codegen splices
            // `percent_encoding::utf8_percent_encode(s,
            // percent_encoding::NON_ALPHANUMERIC).to_string()` and
            // `percent_encoding::percent_decode_str(s)
            // .decode_utf8_lossy().into_owned()`.
            PreludeType::URLEncode => "URLEncode",
            // T124h: the UUID prelude type name. The codegen splices
            // `uuid::Uuid::new_v4().to_string()` /
            // `uuid::Uuid::now_v7().to_string()` /
            // `uuid::Uuid::parse_str(s).is_ok()` for the three
            // associated functions. Surface return types are String /
            // String / Bool (NOT a Uuid value type) - Buff surfaces
            // UUIDs as their canonical String form.
            PreludeType::UUID => "UUID",
            // T124h: the URL prelude type name. ALL-CAPS spelling
            // mirrors the DateTime / Regex convention (the user
            // sees `URL.parse("...")`); the underlying Rust type is
            // `url::Url` (capital U, lowercase rl - case mapping
            // happens in codegen's `buff_type_to_syn` arm).
            PreludeType::URL => "URL",
            // T124i: the Yaml prelude type name. Buff-flavored
            // shortening of YAML (the acronym is all-caps but Buff's
            // convention is PascalCase for module names: `Yaml` reads
            // naturally alongside `Toml`, `Csv`, `Json` future). The
            // codegen splices `serde_yml::from_str` / `serde_yml::to_string`
            // paths (note the Rust crate name is `serde_yml`, NOT
            // `yaml` - the lowering maps Buff's `Yaml` namespace to
            // the `serde_yml` Rust crate paths directly).
            PreludeType::Yaml => "Yaml",
            // T124i: the Csv prelude type name. Mirrors the Rust
            // crate name (`csv`) so the codegen can splice
            // `csv::ReaderBuilder` / `csv::Writer` paths without
            // rewriting.
            PreludeType::Csv => "Csv",
            // T124j: the Path prelude type name. Mirrors Rust's
            // `std::path::Path` surface so the codegen can splice
            // `std::path::PathBuf::from(...).join(...)` paths
            // without rewriting. Note: the underlying Rust type is
            // `PathBuf` (NOT `Path`) - Buff surfaces owned values;
            // the case mapping happens in codegen's `buff_type_to_syn`
            // arm.
            PreludeType::Path => "Path",
            // T124j: the Dir prelude type name. Buff-flavored
            // shortening of `Directory` (clearer than `Fs` or
            // `Directory`; matches the canonical scripting-lang
            // convention). The codegen splices `std::fs::read_dir` /
            // `create_dir_all` / `remove_dir_all` (std - no extern
            // crate needed for those) and `walkdir::WalkDir::new(p)`
            // for `Dir.walk`.
            PreludeType::Dir => "Dir",
            // T124j: the Tempfile prelude type name. Mirrors the
            // Rust crate name (`tempfile`) so the codegen can splice
            // `tempfile::NamedTempFile::new()` paths without
            // rewriting. `Tempfile.dir` uses `std::env::temp_dir()`
            // (std-only, NO extern crate needed for that call alone).
            PreludeType::Tempfile => "Tempfile",
            // T124k: the Hash prelude type name. Buff-flavored
            // shortening of `Hasher` / `Digest` (clearer than either;
            // matches the canonical Python `hashlib` / Node
            // `crypto.createHash` surface intent). The codegen splices
            // `sha2::Sha256::digest` / `sha2::Sha512::digest` /
            // `md5::compute` paths (note: the Rust crate names are
            // `sha2` + `md5`; Buff's `Hash` namespace maps to BOTH
            // depending on the method - sha256/sha512 -> sha2,
            // md5 -> md5).
            PreludeType::Hash => "Hash",
            // T124k: the HMAC prelude type name. ALL-CAPS spelling
            // mirrors the `UUID` / `URL` convention (the canonical
            // acronym is all-uppercase; Buff surfaces it as a
            // PascalCase module name). The codegen splices
            // `hmac::Hmac::<sha2::Sha256>::new_from_slice(...)` paths
            // (note the Rust crate names: `hmac` + `sha2` - Buff's
            // `HMAC` namespace lowers to a path that needs BOTH).
            PreludeType::HMAC => "HMAC",
            // T124l: the OS prelude type name. ALL-CAPS spelling
            // mirrors the `UUID` / `URL` / `HMAC` convention. The
            // codegen splices `std::env::consts::OS` /
            // `std::env::consts::ARCH` (compile-time consts) +
            // `std::env::var("COMPUTERNAME").or_else(|_|
            // std::env::var("HOSTNAME")).unwrap_or_default()` for
            // hostname + `num_cpus::get() as i64` for cpus.
            PreludeType::OS => "OS",
            // T124l: the Process prelude type name. PascalCase
            // (mirrors Regex / Path / URL surface convention). The
            // codegen splices `std::process::Command::new(...)` for
            // spawn, `std::process::Child::wait` / `id` for
            // instance methods, and `std::process::exit(...)` for
            // the side-effecting Process.exit terminal call.
            PreludeType::Process => "Process",
            // T124m: the TCP / UDP / WebSocket prelude type names.
            // ALL-CAPS `TCP` / `UDP` mirror the `UUID` / `URL` /
            // `HMAC` / `OS` convention (canonical acronyms surface
            // as uppercase Buff module names). `WebSocket` is
            // PascalCase (mirrors the canonical Rust crate name
            // `tokio-tungstenite` and the `WebSocket` spelling in
            // most ecosystems; NOT `Ws` or `WebSockets`). The
            // codegen splices `tokio::net::TcpStream::connect(...)`
            // / `tokio::net::UdpSocket::bind(...)` /
            // `tokio_tungstenite::connect_async(...)` paths for the
            // assoc fns and `tokio::io::AsyncReadExt` /
            // `AsyncWriteExt` + `futures_util::SinkExt` /
            // `StreamExt` trait methods for the instance methods.
            PreludeType::TCP => "TCP",
            PreludeType::UDP => "UDP",
            PreludeType::WebSocket => "WebSocket",
            // T2: the Channel prelude type name. PascalCase mirrors
            // Regex / Path / Process convention. The codegen splices
            // `buff_lang_runtime::Channel::new(buf_size)` for the
            // assoc fn.
            PreludeType::Channel => "Channel",
            // T8: Tensor - canonical name matching the user-facing
            // `Tensor.zeros(...)` surface. Mirrors the Regex / Path /
            // URL PascalCase convention. The underlying Rust type is
            // `buff_tensor::Tensor` (= `buff_tensor::TensorCore<f32>`
            // — the alias is the canonical MVP surface).
            PreludeType::Tensor => "Tensor",
            // T11: Signal / Window / Spectrum - canonical PascalCase
            // names matching the user-facing `Signal.from_vec(...)` /
            // `Window.hann(n)` / `spec.magnitudes()` surface. The
            // underlying Rust types are `buff_dsp::Signal` /
            // `buff_dsp::Window` / `buff_dsp::Spectrum`.
            PreludeType::Signal => "Signal",
            PreludeType::Window => "Window",
            PreludeType::Spectrum => "Spectrum",
            PreludeType::DataFrame => "DataFrame",
            // T9: Image - canonical name matching the user-facing
            // `Image.from_path(...)` / `Image.from_bytes(...)` surface.
            // The underlying Rust type is `buff_image::Image`.
            PreludeType::Image => "Image",
            // T10: AudioBuffer - canonical PascalCase name matching the
            // user-facing `AudioBuffer.from_path(...)` /
            // `AudioBuffer.from_samples(...)` surface. The underlying
            // Rust type is `buff_audio::AudioBuffer`.
            PreludeType::Audio => "AudioBuffer",
            // T12: World + Entity - canonical names matching the
            // user-facing `World.new()` / `world.spawn(...)` /
            // `entity.id()` surface. Mirrors the PascalCase convention.
            // The underlying Rust types are `buff_ecs::World` +
            // `buff_ecs::Entity` (the latter a transparent newtype over
            // `hecs::Entity`).
            PreludeType::World => "World",
            PreludeType::Entity => "Entity",
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
            // T124g: namespace-only - Args has no value representation.
            // Mirrors Log / Toml / Math / Random / Strings: the
            // namespace itself is never a value, only its associated
            // functions (`Args.list()`, `Args.get(i)`) are callable.
            PreludeType::Args => Type::Void,
            // T124g: namespace-only - Env has no value representation.
            // Mirrors Log / Toml / Math / Random / Strings / Args: the
            // namespace itself is never a value, only its associated
            // functions (`Env.get(k)`, `Env.set(k, v)`, `Env.has(k)`)
            // are callable.
            PreludeType::Env => Type::Void,
            // T124h: namespace-only - Base64 has no value representation.
            // Mirrors Log / Toml / Math / Random / Strings / Args / Env:
            // the namespace itself is never a value, only its associated
            // functions (`Base64.encode(bytes)`, `Base64.decode(s)`)
            // are callable.
            PreludeType::Base64 => Type::Void,
            // T124h: namespace-only - Hex has no value representation.
            // Mirrors Base64 / Log / Toml / Math / Random / Strings /
            // Args / Env.
            PreludeType::Hex => Type::Void,
            // T124h: namespace-only - URLEncode has no value
            // representation. Mirrors Base64 / Hex / Log / Toml / Math /
            // Random / Strings / Args / Env.
            PreludeType::URLEncode => Type::Void,
            // T124h: namespace-only - UUID has no value representation
            // (UUIDs surface as their canonical String form, NOT as a
            // Uuid value type). Mirrors Base64 / Hex / URLEncode / Log
            // / Toml / Math / Random / Strings / Args / Env.
            PreludeType::UUID => Type::Void,
            // T124h: URL IS a runtime value - returns the opaque
            // parsed-URL type (mapped to `url::Url` at codegen time).
            // Distinct from the namespace-only Base64 / Hex / URLEncode
            // / UUID modules (which return Void). Mirrors Regex (T124d)
            // as the second runtime-value-with-rich-instance-methods
            // type.
            PreludeType::URL => Type::Url,
            // T124i: namespace-only - Yaml has no value representation.
            // Mirrors Toml exactly: the namespace itself is never a
            // value, only its associated functions (`Yaml.parse` /
            // `Yaml.stringify`) are callable. Same surface as Toml
            // (parse + stringify a heterogeneous Map).
            PreludeType::Yaml => Type::Void,
            // T124i: namespace-only - Csv has no value representation.
            // Mirrors Yaml / Toml: the namespace itself is never a
            // value, only its associated functions (`Csv.parse` /
            // `Csv.stringify`) are callable. Csv's surface differs
            // slightly (parse + stringify a uniform Vector<Vector<
            // String>> instead of a heterogeneous Map), but the
            // namespace-only stance is identical.
            PreludeType::Csv => Type::Void,
            // T124j: Path IS a runtime value - returns the opaque
            // filesystem-path type (mapped to `std::path::PathBuf`
            // at codegen time). Distinct from the namespace-only
            // Dir / Tempfile modules (which return Void). Mirrors
            // Regex (T124d) and URL (T124h) as the third runtime-
            // value-with-rich-instance-methods type.
            PreludeType::Path => Type::Path,
            // T124j: namespace-only - Dir has no value
            // representation. Mirrors Log / Toml / Yaml / Csv
            // exactly: the namespace itself is never a value, only
            // its associated functions (`Dir.list` / `Dir.create` /
            // `Dir.remove` / `Dir.walk`) are callable. Same surface
            // as the other namespace-only modules.
            PreludeType::Dir => Type::Void,
            // T124j: namespace-only - Tempfile has no value
            // representation. Mirrors Log / Toml / Yaml / Csv /
            // Dir: the namespace itself is never a value, only its
            // associated functions (`Tempfile.create` /
            // `Tempfile.dir`) are callable.
            PreludeType::Tempfile => Type::Void,
            // T124k: namespace-only - Hash has no value
            // representation. Mirrors Log / Toml / Base64 / Hex /
            // Yaml / Csv / Dir / Tempfile exactly: the namespace
            // itself is never a value, only its associated functions
            // (`Hash.sha256` / `Hash.sha512` / `Hash.md5`) are
            // callable. Every call returns a hex String (the digest).
            PreludeType::Hash => Type::Void,
            // T124k: namespace-only - HMAC has no value
            // representation. Mirrors Hash / Log / Toml / Base64 /
            // Hex / Yaml / Csv / Dir / Tempfile exactly: the
            // namespace itself is never a value, only its associated
            // function (`HMAC.sha256`) is callable. The call returns
            // a hex String (the MAC).
            PreludeType::HMAC => Type::Void,
            // T124l: namespace-only - OS has no value
            // representation. Mirrors Log / Toml / Math / Strings
            // / Args / Env / Hash / HMAC exactly: the namespace
            // itself is never a value, only its associated fns
            // (`OS.name` / `OS.arch` / `OS.hostname` / `OS.cpus`)
            // are callable.
            PreludeType::OS => Type::Void,
            // T124l: Process IS a runtime value - returns the
            // opaque spawned-process type (mapped to
            // `Option<std::process::Child>` at codegen time - the
            // Option wrapper lets spawn be panic-free). Distinct
            // from the namespace-only OS module (which returns
            // Void). Mirrors Regex (T124d) / URL (T124h) / Path
            // (T124j) as the fourth runtime-value-with-rich-
            // instance-methods type.
            PreludeType::Process => Type::Process,
            // T124m: namespace-only - TCP / UDP / WebSocket have no
            // value representation. Mirrors Log / Toml / OS exactly:
            // the namespace itself is never a value, only its
            // associated functions (`TCP.connect(h, p)` /
            // `UDP.bind(h, p)` / `WebSocket.connect(url)`) are
            // callable, and those return the runtime-value types
            // (Connection / Socket / WsConnection respectively).
            PreludeType::TCP => Type::Void,
            PreludeType::UDP => Type::Void,
            PreludeType::WebSocket => Type::Void,
            // T2: namespace-only - Channel has no value representation.
            // The associated function `Channel.new(buf_size)` returns
            // a `(Sender<T>, Receiver<T>)` tuple of runtime-value
            // types (the value type IS first-class; the namespace is
            // not).
            PreludeType::Channel => Type::Void,
            // T8: namespace-only - Tensor has no value representation
            // at the Buff Type level for MVP. The associated functions
            // (`Tensor.zeros` / `Tensor.from_vec` / etc.) return
            // `Type::Unknown` (forward-declaration contract — the
            // coordinated `Type::Tensor` variant is added in a
            // follow-up task outside the T8 shared zone). Mirrors the
            // Log / Toml / OS / Channel namespace-only stance: the
            // namespace itself is never a value, only its assoc fns
            // are callable.
            PreludeType::Tensor => Type::Void,
            // T11: Signal / Window are namespace-only modules (their
            // buff_type is Void — only the ctor return values are
            // typed). Spectrum is a runtime-value type carrying the
            // FFT bins — but since the T11 MVP keeps Type::Void for
            // forward-declared variants (mirrors T8's Tensor stance),
            // we return Void here and let the codegen layer splice the
            // real `buff_dsp::Spectrum` paths. A coordinated `Type::
            // Spectrum` variant is a follow-up task outside the T11
            // shared zone.
            PreludeType::Signal => Type::Void,
            PreludeType::Window => Type::Void,
            PreludeType::Spectrum => Type::Void,
            // T7: DataFrame IS a runtime value (NOT namespace-only).
            // Returns the opaque [`Type::DataFrame`] variant; the
            // codegen layer maps it to `buff_dataframe::DataFrame`.
            PreludeType::DataFrame => Type::DataFrame,
            // T9: Image IS a runtime value (NOT namespace-only).
            // Returns the opaque [`Type::Image`] variant; the codegen
            // layer maps it to `buff_image::Image`.
            PreludeType::Image => Type::Image,
            // T10: AudioBuffer IS a runtime value (NOT namespace-only).
            // Returns the opaque [`Type::Audio`] variant; the codegen
            // layer maps it to `buff_audio::AudioBuffer`.
            PreludeType::Audio => Type::Audio,
            // T12: World + Entity map to the coordinated [`Type::World`]
            // / [`Type::Entity`] variants in `ty.rs` (added by the
            // coordinated sibling task — see `ty.rs` lines 462 + 477).
            // Both ARE runtime values (NOT namespace-only): World is
            // constructed via `World.new()`; Entity is the return value
            // of `world.spawn(...)`. The codegen layer maps them to
            // `buff_ecs::World` / `buff_ecs::Entity`.
            PreludeType::World => Type::World,
            PreludeType::Entity => Type::Entity,
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
            PreludeType::Log
                | PreludeType::Toml
                | PreludeType::Math
                | PreludeType::Random
                | PreludeType::Strings
                | PreludeType::Args
                | PreludeType::Env
                | PreludeType::Base64
                | PreludeType::Hex
                | PreludeType::URLEncode
                | PreludeType::UUID
                | PreludeType::Yaml
                | PreludeType::Csv
                | PreludeType::Dir
                | PreludeType::Tempfile
                | PreludeType::Hash
                | PreludeType::HMAC
                | PreludeType::OS
                | PreludeType::TCP
                | PreludeType::UDP
                | PreludeType::WebSocket
                | PreludeType::Channel
                | PreludeType::Tensor
                | PreludeType::Signal
                | PreludeType::Window
                | PreludeType::Spectrum
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
    // ---- Args / Env (T124g) -------------------------------------------
    // These variants follow the precedent set by `Parse` (shared by
    // DateTime / Date / Toml): a single variant may be valid on MULTIPLE
    // prelude types, with the (type, method) pair dispatched in
    // [`assoc_fn_return_type`] + the codegen arm. Below, `Get` is shared
    // between `Args.get(i)` (returns String) and `Env.get(k)` (returns
    // Option<String>) - same name, different per-type semantics, exactly
    // like `parse(text)` returns DateTime vs Date vs Map<String, Unknown>
    // depending on the receiver type.
    //
    // `Args.list()` - collect program name + args into a
    // `Vector<String>`. Zero args. Returns `Vector<String>`. Wraps
    // `std::env::args().collect::<Vec<String>>()`.
    List,
    /// `Args.get(i)` / `Env.get("KEY")` - positional or named lookup.
    /// - On `Args`: one Int arg, returns `String` (the i-th arg, or
    //    empty String on out-of-bounds - NEVER panics). Wraps
    //    `std::env::args().nth(i).unwrap_or_default()`.
    /// - On `Env`: one String arg (the var name), returns
    ///   `Option<String>` (None when unset or invalid UTF-8). Wraps
    ///   `std::env::var(k).ok()`.
    ///
    /// The shared variant mirrors `Parse` (DateTime.parse(s) /
    /// Date.parse(s) / Toml.parse(s) all use `PreludeAssocFn::Parse`).
    /// Dispatch on the (type, method) pair is exhaustive in
    /// [`assoc_fn_return_type`].
    Get,
    /// `Env.set("KEY", "value")` - set an env var. Two args
    /// (String, String). Returns `Void`. Wraps
    /// `std::env::set_var(k, v)`. NOTE: Rust 2024 edition marks
    /// `set_var` as `unsafe`; Buff emits 2021 so the call is safe
    /// today. A future edition bump will need an `unsafe { ... }`
    /// wrapper (tracked in `decisions.md`).
    Set,
    /// `Env.has("KEY")` - test whether an env var is set. One arg
    /// (String). Returns `Bool`. Wraps `std::env::var(k).is_ok()`.
    Has,
    // ---- Web modules (T124h) ------------------------------------------
    // These variants follow the precedent set by `Parse` (shared by
    // DateTime / Date / Toml / URL / UUID) and `Get` (shared by
    // Args.get / Env.get): a single variant may be valid on MULTIPLE
    // prelude types, with the (type, method) pair dispatched in
    // [`assoc_fn_return_type`] + the codegen arm. Below:
    // - `Encode` is shared between `Base64.encode(bytes)` /
    //   `Hex.encode(bytes)` / `URLEncode.encode(string)` (different
    //   arg + return types per receiver, dispatched on the pair).
    // - `Decode` is shared symmetrically.
    // - `V4` / `V7` are UUID-only.
    //
    /// `Base64.encode(bytes)` / `Hex.encode(bytes)` /
    /// `URLEncode.encode(string)` - encode a value to its text
    /// representation. Wraps:
    /// - On `Base64`: `base64::Engine::encode(&general_purpose::STANDARD,
    ///   bytes)` (returns `String`, takes `Vector<Byte>`).
    /// - On `Hex`: `hex::encode(bytes)` (returns `String`, takes
    ///   `Vector<Byte>`).
    /// - On `URLEncode`:
    ///   `percent_encoding::utf8_percent_encode(s,
    ///   percent_encoding::NON_ALPHANUMERIC).to_string()` (returns
    ///   `String`, takes `String`).
    ///
    /// The shared variant mirrors `Parse` (which is shared between
    /// DateTime / Date / Toml / URL / UUID). Dispatch on the (type,
    /// method) pair is exhaustive in [`assoc_fn_return_type`].
    Encode,
    /// `Base64.decode(string)` / `Hex.decode(string)` /
    /// `URLEncode.decode(string)` - decode a text representation
    /// back to bytes / String. Wraps:
    /// - On `Base64`:
    ///   `base64::Engine::decode(&general_purpose::STANDARD, s)
    ///   .unwrap_or_default()` (returns `Vector<Byte>`, empty Vec on
    ///   decode failure - NEVER panics).
    /// - On `Hex`: `hex::decode(s).unwrap_or_default()` (returns
    ///   `Vector<Byte>`, empty Vec on failure - NEVER panics).
    /// - On `URLEncode`:
    ///   `percent_encoding::percent_decode_str(s)
    ///   .decode_utf8_lossy().into_owned()` (returns `String`;
    ///   invalid UTF-8 sequences become U+FFFD REPLACEMENT CHARACTER
    ///   - lossy decode, NEVER panics).
    ///
    /// The shared variant mirrors `Encode` and the `Parse` precedent.
    Decode,
    /// `UUID.v4()` - generate a random v4 UUID. Zero args. Returns
    /// `String` (the canonical hyphen-separated form). Wraps
    /// `uuid::Uuid::new_v4().to_string()`. Requires the `v4`
    /// feature on the `uuid` crate (configured at the workspace
    /// `[workspace.dependencies]` level).
    V4,
    /// `UUID.v7()` - generate a time-ordered v7 UUID (Unix-timestamp-
    /// prefixed, sortable). Zero args. Returns `String`. Wraps
    /// `uuid::Uuid::now_v7().to_string()`. Requires the `v7` feature
    /// on the `uuid` crate. Distinct from [`Self::V4`] in generation
    /// algorithm (v4 is random; v7 is timestamp-prefixed for sort
    /// stability) but identical surface type (both return String).
    V7,
    // ---- Filesystem modules (T124j) ---------------------------------
    // These variants follow the precedent set by `Parse` (shared by
    // DateTime / Date / Toml / URL / UUID), `Get` (shared by
    // Args.get / Env.get), `Encode` / `Decode` (shared by Base64 /
    // Hex / URLEncode), `List` (shared by Args.list / Dir.list),
    // and `Join` (shared by Strings.join / Path.join): a single
    // variant may be valid on MULTIPLE prelude types, with the
    // (type, method) pair dispatched in [`assoc_fn_return_type`] +
    // the codegen arm.
    //
    /// `Dir.create(path)` / `Tempfile.create()` - create a directory
    /// or a temporary file. Wraps:
    /// - On `Dir`: `std::fs::create_dir_all(p).ok()` (creates the
    ///   directory and any missing parents - mirrors `mkdir -p`;
    ///   returns `Void`, discards errors via `.ok()` - NEVER
    ///   panics).
    /// - On `Tempfile`: `tempfile::NamedTempFile::new()
    ///   .map(|f| f.into_temp_path().keep().unwrap_or_default())
    ///   .unwrap_or_default()` (creates a new empty temp file in
    ///   the OS-default temp directory; returns `Path` - the kept
    ///   file path - empty PathBuf on failure - NEVER panics).
    ///
    /// The shared variant mirrors `Parse` (DateTime.parse /
    /// Date.parse / Toml.parse / URL.parse / UUID.parse),
    /// `Get` (Args.get / Env.get), `Encode` / `Decode` (Base64 /
    /// Hex / URLEncode), `List` (Args.list / Dir.list), and `Join`
    /// (Strings.join / Path.join). Dispatch on the (type, method)
    /// pair is exhaustive in [`assoc_fn_return_type`].
    Create,
    /// `Dir.remove(path)` - remove the directory and all its
    /// contents recursively. One arg (the path). Returns `Void`.
    /// Wraps `std::fs::remove_dir_all(p).ok()` (panic-free -
    /// discards errors via `.ok()`; mirrors the Dir.create
    /// panic-free stance). Dir-only (no other prelude type has a
    /// `remove` method).
    Remove,
    /// `Dir.walk(path)` - recursively walk the directory tree.
    /// One arg (the path). Returns `Vector<Path>` (a Vec<PathBuf>
    /// of every path found during the traversal - depth-first).
    /// Wraps `walkdir::WalkDir::new(p).into_iter()
    /// .filter_map(|e| e.ok()).map(|e| e.path().to_path_buf())
    /// .collect::<Vec<std::path::PathBuf>>()` (skip inaccessible
    /// entries via `.filter_map(|e| e.ok())` - NEVER panics,
    /// mirroring the Csv.parse panic-free stance from T124i). The
    /// `walkdir` crate is recorded in codegen `extern_crates` when
    /// a Buff program uses `Dir.walk`. Dir-only.
    Walk,
    /// `Tempfile.dir()` - the OS-default temp directory path. Zero
    /// args. Returns `Path` (the temp directory as a `PathBuf`).
    /// Wraps `std::env::temp_dir()` (the `tempfile::env::temp_dir`
    /// is a re-export of the std fn; we splice the std path
    /// directly so this call alone needs NO extern crate). The
    /// `tempfile` crate is still recorded in codegen
    /// `extern_crates` for symmetry (any Tempfile.* call flags the
    /// crate). Tempfile-only.
    Dir,
    // ---- Crypto modules (T124k) -------------------------------------
    // These variants follow the precedent set by `Parse` (shared by
    // DateTime / Date / Toml / URL / UUID), `Get` (shared by
    // Args.get / Env.get), `Encode` / `Decode` (shared by Base64 /
    // Hex / URLEncode), `List` (shared by Args.list / Dir.list),
    // `Join` (shared by Strings.join / Path.join), and `Create`
    // (shared by Dir.create / Tempfile.create): a single variant
    // may be valid on MULTIPLE prelude types, with the (type,
    // method) pair dispatched in [`assoc_fn_return_type`] + the
    // codegen arm.
    //
    /// `Hash.sha256(data)` / `HMAC.sha256(key, data)` - SHA-256 hex
    /// digest. Wraps:
    /// - On `Hash`: `{ use sha2::Digest; hex::encode(sha2::Sha256
    ///   ::digest(d.as_bytes())) }` (one-shot digest of one arg;
    ///   returns the 64-char lowercase hex String).
    /// - On `HMAC`: `{ use hmac::Mac; hmac::Hmac::<sha2::Sha256>
    ///   ::new_from_slice(k.as_bytes()).map(|mut mac| {
    ///   mac.update(d.as_bytes()); hex::encode(mac.finalize()
    ///   .into_bytes()) }).unwrap_or_default() }` (keyed MAC of
    ///   two args - key + data; returns the 64-char lowercase hex
    ///   String. `new_from_slice` returns `Result` and the `.map()
    ///   .unwrap_or_default()` collapses Err to empty String -
    ///   NEVER panics).
    ///
    /// The shared variant mirrors `Parse` (DateTime / Date / Toml /
    /// URL / UUID), `Encode` (Base64 / Hex / URLEncode), `Create`
    /// (Dir / Tempfile), and the other same-name-different-type
    /// overloads. Dispatch on the (type, method) pair is exhaustive
    /// in [`assoc_fn_return_type`].
    Sha256,
    /// `Hash.sha512(data)` - SHA-512 hex digest. One arg. Returns
    /// the 128-char lowercase hex String. Wraps
    /// `{ use sha2::Digest; hex::encode(sha2::Sha512::digest
    /// (d.as_bytes())) }` (block-scoped `use` for the `Digest`
    /// trait method). Hash-only (HMAC surface is SHA-256 only in
    /// T124k; SHA-512 HMAC may be added in a future task).
    Sha512,
    /// `Hash.md5(data)` - MD5 hex digest. One arg. Returns the
    /// 32-char lowercase hex String. Wraps
    /// `hex::encode(md5::compute(d.as_bytes()).0)` (the `.0`
    /// accesses the inner `[u8; 16]` of the `md5::Digest` tuple
    /// struct). **MD5 is CRYPTOGRAPHICALLY BROKEN** - exposed for
    /// checksum compatibility only (etags, content-addressable
    /// caches, legacy interop); NEVER use for security. Hash-only.
    Md5,
    // ---- Process / OS modules (T124l) -----------------------------
    // These variants follow the precedent set by `Parse` (shared by
    // DateTime / Date / Toml / URL / UUID), `Get` (shared by
    // Args.get / Env.get), `Encode` / `Decode` (shared by Base64 /
    // Hex / URLEncode), `Sha256` (shared by Hash.sha256 /
    // HMAC.sha256): a single variant may be valid on MULTIPLE
    // prelude types, with the (type, method) pair dispatched in
    // [`assoc_fn_return_type`] + the codegen arm.
    //
    /// `Process.spawn(command, args)` - spawn a child process.
    /// Two args (String command, Vector<String> args). Returns
    /// `Process` (an opaque handle to the spawned child). Wraps
    /// `std::process::Command::new(cmd).args(args).spawn().ok()`
    /// (the `.ok()` collapses a spawn failure to `None` - NEVER
    /// panics, matching Buff's "no panicking generated code" rule).
    /// The command + args are passed SEPARATELY (NOT through a
    /// shell) so there's NO shell-injection vector - the spec's
    /// safety stance. Process-only.
    Spawn,
    /// `Process.exit(code)` - terminate the program immediately
    /// with the given exit code. One arg (Int). Returns `Void`
    /// (the call never returns - it terminates the program).
    /// Wraps `std::process::exit(code as i32)`. NOTE: Rust's
    /// `std::process::exit` does NOT run destructors; the Buff
    /// surface inherits that behavior (the spec calls this out
    /// as the "exit yourself" primitive, distinct from
    /// signal-based shutdown which is explicitly out-of-scope).
    /// Process-only.
    Exit,
    /// `OS.name()` - the OS name (`"linux"` / `"macos"` /
    /// `"windows"`). Zero args. Returns `String`. Wraps
    /// `std::env::consts::OS.to_string()` (compile-time const).
    /// OS-only. Same variant name as a hypothetical future
    /// `Process.name` (NOT in T124l scope) is dispatched on the
    /// (type, method) pair - mirrors `Parse` / `Get` / `Encode`
    /// shared-variant pattern.
    Name,
    /// `OS.arch()` - the CPU architecture (`"x86_64"` /
    /// `"aarch64"`). Zero args. Returns `String`. Wraps
    /// `std::env::consts::ARCH.to_string()` (compile-time const).
    /// OS-only.
    Arch,
    /// `OS.hostname()` - the machine hostname. Zero args.
    /// Returns `String` (empty String when neither COMPUTERNAME
    /// nor HOSTNAME is set - NEVER panics). Wraps
    /// `std::env::var("COMPUTERNAME").or_else(|_|
    /// std::env::var("HOSTNAME")).unwrap_or_default()` - the
    /// bare-minimum env-var approach (NO `hostname` crate added,
    /// per spec). OS-only.
    Hostname,
    /// `OS.cpus()` - the number of logical CPUs. Zero args.
    /// Returns `Int`. Wraps `num_cpus::get() as i64`. The
    /// `num_cpus` crate is recorded in codegen `extern_crates`
    /// when a program uses `OS.cpus` (the narrow walker flags
    /// ONLY the `cpus` method name - `name` / `arch` / `hostname`
    /// use std only and record NO extern crate). OS-only.
    Cpus,
    // ---- Networking modules (T124m) -------------------------------
    // These variants follow the precedent set by `Parse` (shared by
    // DateTime / Date / Toml / URL / UUID), `Get` (shared by
    // Args.get / Env.get), `Encode` / `Decode` (shared by Base64 /
    // Hex / URLEncode), `Sha256` (shared by Hash.sha256 /
    // HMAC.sha256): a single variant may be valid on MULTIPLE
    // prelude types, with the (type, method) pair dispatched in
    // [`assoc_fn_return_type`] + the codegen arm.
    //
    /// `TCP.connect(host, port)` - open a TCP client connection to
    /// the given host:port. Two args (String host, Int port).
    /// Returns `Connection` (an opaque handle to the tokio
    /// TcpStream). Wraps `tokio::net::TcpStream::connect(format!(
    /// "{}:{}", h, p)).await.ok()` (the `.ok()` collapses a
    /// connect failure to `None` - NEVER panics, matching Buff's
    /// "no panicking generated code" rule). TCP-only originally;
    /// shared with `WebSocket.connect(url)` (which is dispatched
    /// on the (WebSocket, Connect) pair - same name, different
    /// arg signature + return type, exactly like `parse` shared
    /// by DateTime / Date / Toml / URL / UUID).
    Connect,
    /// `UDP.bind(host, port)` - bind a UDP socket to the given
    /// host:port. Two args (String host, Int port). Returns
    /// `Socket` (an opaque handle to the tokio UdpSocket). Wraps
    /// `tokio::net::UdpSocket::bind(format!("{}:{}", h, p)).await
    /// .ok()` (the `.ok()` collapses a bind failure to `None` -
    /// NEVER panics). UDP-only.
    Bind,
    /// `Channel.new(buf_size)` - construct a bounded MPSC channel
    /// pair. One arg (Int buf_size). Returns `(Sender<T>,
    /// Receiver<T>)` tuple. Wraps
     /// `buff_lang_runtime::Channel::new(buf_size)` which internally
     /// calls `tokio::sync::mpsc::channel(buf_size)` (the runtime
     /// hides tokio behind the abstraction per Metis G6). Channel-only.
     /// The T parameter is implicit (Type-level we return a tuple
     /// of opaque Sender/Receiver; Rust infers T from subsequent
     /// `sender.send(value)` / `receiver.recv()` usage).
    New,
    // ---- Tensor constructors (T8) ------------------------------------
    // Each variant lowers to the matching `buff_tensor::Tensor`
    // constructor in codegen. Returns `Type::Unknown` at the Buff
    // Type level for MVP (forward-declaration contract — the
    // coordinated `Type::Tensor` variant is a follow-up task outside
    // the T8 shared zone).
    /// `Tensor.zeros(shape)` - construct a zero-filled tensor.
    /// One arg (Vector<Int>). Returns Tensor (modeled as Unknown).
    Zeros,
    /// `Tensor.ones(shape)` - construct a one-filled tensor. One arg
    /// (Vector<Int>). Returns Tensor (modeled as Unknown).
    Ones,
    /// `Tensor.from_vec(data, shape)` - wrap a flat Vector<Float> +
    /// shape into a tensor. Two args. Returns Tensor (Unknown).
    FromVec,
    /// `Tensor.filled(shape, value)` - construct a constant-filled
    /// tensor. Two args (Vector<Int>, Float). Returns Tensor (Unknown).
    Filled,
    // ---- DataFrame constructors (T7) ---------------------------------
    // Each variant lowers to the matching `buff_dataframe::DataFrame`
    // constructor in codegen. Returns `Type::DataFrame` (a real
    // runtime-value variant — added in the same T7 commit, unlike
    // T8's forward-declaration-only Tensor). Buff §7 ctor convention
    // permits `Type.from_*()` (this surface), forbids `Type.create()`
    // / `Type.build()` / `new Type()`.
    /// `DataFrame.from_csv(path)` - load a CSV file into a DataFrame.
    /// One arg (String / Path). Returns DataFrame. Wraps
    /// `buff_dataframe::DataFrame::from_csv(path).unwrap_or_default()`
    /// (panic-free on file-not-found / parse failure - returns an
    /// empty DataFrame, matching Buff's "no panicking generated code"
    /// rule). Schema-aware: column kinds (Int/Float/String/Bool) are
    /// inferred at load time by the in-tree `buff-dataframe` crate.
    /// CPU-only per Metis G7.
    FromCsv,
    /// `DataFrame.from_json(path)` - load a JSON-lines file (one JSON
    /// object per line) into a DataFrame. One arg (String / Path).
    /// Returns DataFrame. Wraps
    /// `buff_dataframe::DataFrame::from_json(path).unwrap_or_default()`
    /// (panic-free on file-not-found / parse failure - returns an
    /// empty DataFrame). Column kinds inferred from the JSON Value
    /// tags (Bool/Number{i64}/Number{f64}/String). CPU-only.
    FromJson,
    // ---- Image constructors (T9) -----------------------------------
    // Each variant lowers to the matching `buff_image::Image`
    // constructor in codegen. Returns `Type::Image` (a real
    // runtime-value variant — added in the same T9 commit). Buff §7
    // ctor convention permits `Type.from_*()` (this surface), forbids
    // `Type.create()` / `Type.build()` / `new Type()`. CPU-only per
    // Metis G7 lock.
    /// `Image.from_path(path)` - load an image from disk. One arg
    /// (String / Path). Returns Image. Format is auto-detected from
    /// the file contents. Wraps `buff_image::Image::from_path(p)?`
    /// (the `?` propagates ImageError per Buff's R3 error-mapping
    /// contract; `from_path` is also wrapped in `catch_unwind`
    /// internally per FFI guide R6).
    FromPath,
    /// `Image.from_bytes(bytes)` - decode an in-memory image buffer.
    /// One arg (Vector<Byte>). Returns Image. Format is auto-detected
    /// from the buffer contents. Wraps
    /// `buff_image::Image::from_bytes(&b)?`. Used for downloading
    /// images over HTTP (the bytes come from `reqwest::get().bytes()`
    /// in a future buff-web integration) or reading from a database
    /// BLOB column.
    FromBytes,
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
        // T124g: Args / Env assoc fns (4 distinct names): list/get/set/has.
        // `Get` is shared between Args.get and Env.get (mirrors `Parse`
        // being shared between DateTime / Date / Toml).
        PreludeAssocFn::List,
        PreludeAssocFn::Get,
        PreludeAssocFn::Set,
        PreludeAssocFn::Has,
        // T124h: Web modules assoc fns (4 distinct names):
        // encode/decode/v4/v7. `Encode` and `Decode` are each shared
        // between Base64 / Hex / URLEncode (mirrors `Parse` being
        // shared between DateTime / Date / Toml / URL / UUID).
        // `V4` / `V7` are UUID-only.
        PreludeAssocFn::Encode,
        PreludeAssocFn::Decode,
        PreludeAssocFn::V4,
        PreludeAssocFn::V7,
        // T124j: Filesystem modules assoc fns (4 distinct names):
        // create/remove/walk/dir. `Create` is shared between
        // Dir.create and Tempfile.create (mirrors `Parse` being
        // shared). `Remove` / `Walk` are Dir-only; `Dir` is
        // Tempfile-only. Note: Path.join reuses the existing `Join`
        // variant (also used by Strings.join) and Dir.list reuses
        // the existing `List` variant (also used by Args.list).
        PreludeAssocFn::Create,
        PreludeAssocFn::Remove,
        PreludeAssocFn::Walk,
        PreludeAssocFn::Dir,
        // T124k: Crypto modules assoc fns (3 distinct names):
        // sha256/sha512/md5. `Sha256` is shared between Hash.sha256
        // and HMAC.sha256 (mirrors `Parse` being shared between
        // DateTime / Date / Toml / URL / UUID, `Create` being shared
        // between Dir / Tempfile, etc.). `Sha512` / `Md5` are
        // Hash-only.
        PreludeAssocFn::Sha256,
        PreludeAssocFn::Sha512,
        PreludeAssocFn::Md5,
        // T124l: Process / OS assoc fns (6 distinct names):
        // spawn/exit/name/arch/hostname/cpus. `Spawn` + `Exit` are
        // Process-only; `Name` / `Arch` / `Hostname` / `Cpus` are
        // OS-only. NO shared variants with prior prelude types in
        // T124l scope (a future `Process.name` would reuse `Name` -
        // the (type, method) pair dispatch in `assoc_fn_return_type`
        // handles the overload, mirroring `Parse` / `Get` / `Encode`).
        PreludeAssocFn::Spawn,
        PreludeAssocFn::Exit,
        PreludeAssocFn::Name,
        PreludeAssocFn::Arch,
        PreludeAssocFn::Hostname,
        PreludeAssocFn::Cpus,
        // T124m: Networking assoc fns (2 distinct names): connect
        // and bind. `Connect` is shared between TCP.connect(host,
        // port) and WebSocket.connect(url) (mirrors `Parse` being
        // shared between DateTime / Date / Toml / URL / UUID,
        // `Encode` shared between Base64 / Hex / URLEncode, etc.).
        // `Bind` is UDP-only.
        PreludeAssocFn::Connect,
        PreludeAssocFn::Bind,
        // T2: Channel.new - constructs a bounded MPSC channel pair.
        // Channel-only. Returns (Sender<T>, Receiver<T>) tuple.
        PreludeAssocFn::New,
        // T8: Tensor constructor assoc fns (4 distinct names):
        // zeros / ones / from_vec / filled. All Tensor-only. Each
        // returns a runtime `buff_tensor::Tensor` value (modeled as
        // `Type::Unknown` at the Buff Type level until the
        // coordinated `Type::Tensor` variant lands — see T8 task
        // spec + the T8 shared-zone constraint). The 4 fns cover:
        // - Tensor.zeros(shape) -> zero-filled tensor of given shape
        // - Tensor.ones(shape) -> one-filled
        // - Tensor.from_vec(data, shape) -> wrap a flat Vec + shape
        // - Tensor.filled(shape, value) -> constant-filled
        PreludeAssocFn::Zeros,
        PreludeAssocFn::Ones,
        PreludeAssocFn::FromVec,
        PreludeAssocFn::Filled,
        // T7: DataFrame assoc fns (2 distinct names): from_csv / from_json.
        // Both DataFrame-only (no shared variants with prior prelude
        // types in T7 scope). Buff §7 ctor naming convention permits
        // the `Type.from_*()` form; the codegen dispatches on the
        // (DataFrame, FromCsv) / (DataFrame, FromJson) pair.
        PreludeAssocFn::FromCsv,
        PreludeAssocFn::FromJson,
        // T9: Image constructors (2 distinct names): from_path /
        // from_bytes. Image-only. Buff §7 ctor naming convention
        // permits `Type.from_*()`. Mirrors the DataFrame ctor pattern.
        PreludeAssocFn::FromPath,
        PreludeAssocFn::FromBytes,
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
            // T2 stub: Channel.new — placeholder name; T2 owns the full
            // associated-function surface for the Channel prelude type.
            PreludeAssocFn::New => "new",
            // T8: Tensor constructor names. Mirror the Rust
            // `buff_tensor::Tensor` method names so codegen can splice
            // `buff_tensor::Tensor::<f32>::zeros(shape)?` /
            // `::ones` / `::from_vec` / `::filled` paths without
            // rewriting.
            PreludeAssocFn::Zeros => "zeros",
            PreludeAssocFn::Ones => "ones",
            PreludeAssocFn::FromVec => "from_vec",
            PreludeAssocFn::Filled => "filled",
            // T7: DataFrame ctor names mirror the Buff §7 `Type.from_*()`
            // ctor convention so the codegen can splice
            // `buff_dataframe::DataFrame::from_csv(path)` /
            // `buff_dataframe::DataFrame::from_json(path)` paths
            // without rewriting.
            PreludeAssocFn::FromCsv => "from_csv",
            PreludeAssocFn::FromJson => "from_json",
            // T9: Image ctor names mirror the Buff §7 `Type.from_*()`
            // ctor convention so the codegen can splice
            // `buff_image::Image::from_path(p)` /
            // `buff_image::Image::from_bytes(b)` paths without
            // rewriting. Mirrors the DataFrame precedent (T7).
            PreludeAssocFn::FromPath => "from_path",
            PreludeAssocFn::FromBytes => "from_bytes",
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
            // T124g: Args / Env assoc fn names mirror Rust's `std::env`
            // surface intent. `list` is a Buff-flavored name (clearer
            // than `args_collect`); `get` matches Buff's universal
            // accessor verb (mirrors Map.get / Vector.get); `set` /
            // `has` are the canonical env/map-style mutate + test verbs.
            PreludeAssocFn::List => "list",
            PreludeAssocFn::Get => "get",
            PreludeAssocFn::Set => "set",
            PreludeAssocFn::Has => "has",
            // T124h: Web modules assoc fn names. `encode` / `decode`
            // are the canonical codec verbs (mirrors the `serde::Serialize`
            // / `Deserialize` derive CRATE names + the `hex::encode` /
            // `base64::Engine::encode` Rust API surface). `v4` / `v7`
            // mirror the `uuid::Uuid::new_v4` / `now_v7` constructor
            // names (Buff-flavored: drops the `new_` prefix since the
            // namespace `UUID` already carries the type intent).
            PreludeAssocFn::Encode => "encode",
            PreludeAssocFn::Decode => "decode",
            PreludeAssocFn::V4 => "v4",
            PreludeAssocFn::V7 => "v7",
            // T124j: Filesystem modules assoc fn names. `create`
            // mirrors the canonical "create" verb (shared by
            // Dir.create / Tempfile.create - same name, different
            // per-type semantics, exactly like `parse` shared by
            // DateTime / Date / Toml / URL / UUID). `remove` /
            // `walk` are Dir-only canonical fs verbs. `dir` is
            // Tempfile-only (short for "directory").
            PreludeAssocFn::Create => "create",
            PreludeAssocFn::Remove => "remove",
            PreludeAssocFn::Walk => "walk",
            PreludeAssocFn::Dir => "dir",
            // T124k: Crypto modules assoc fn names. `sha256` is
            // shared between Hash.sha256 and HMAC.sha256 (same
            // algorithm, different receiver type - mirrors `parse`
            // being shared between DateTime / Date / Toml / URL /
            // UUID). `sha512` / `md5` are Hash-only (HMAC surface
            // is SHA-256 only in T124k). The names match the
            // canonical lowercase spelling from Python's hashlib
            // (`hashlib.sha256(...)`) + Node's crypto
            // (`crypto.createHash('sha256')`) so the surface is
            // familiar across ecosystems.
            PreludeAssocFn::Sha256 => "sha256",
            PreludeAssocFn::Sha512 => "sha512",
            PreludeAssocFn::Md5 => "md5",
            // T124l: Process / OS assoc fn names. `spawn` mirrors
            // the canonical `std::process::Command::spawn` verb
            // (the underlying Rust method). `exit` mirrors
            // `std::process::exit` (the side-effecting terminal
            // call). `name` / `arch` / `hostname` / `cpus` are
            // Buff-flavored names (clearer than `consts::OS` /
            // `consts::ARCH` / `gethostname` / `available_parallelism`).
            PreludeAssocFn::Spawn => "spawn",
            PreludeAssocFn::Exit => "exit",
            PreludeAssocFn::Name => "name",
            PreludeAssocFn::Arch => "arch",
            PreludeAssocFn::Hostname => "hostname",
            PreludeAssocFn::Cpus => "cpus",
            // T124m: Networking assoc fn names. `connect` mirrors
            // the canonical `std::net::TcpStream::connect` /
            // `tokio::net::TcpStream::connect` verb. `bind` mirrors
            // the canonical `std::net::UdpSocket::bind` /
            // `tokio::net::UdpSocket::bind` verb. Same shared-
            // variant pattern as `parse` (shared between DateTime /
            // Date / Toml / URL / UUID) - `connect` is shared
            // between TCP.connect(host, port) and WebSocket.connect
            // (url), dispatched on the (type, method) pair.
            PreludeAssocFn::Connect => "connect",
            PreludeAssocFn::Bind => "bind",
        }
    }
}

/// Look up a prelude associated function by the (type, method-name) pair.
///
/// Returns `None` when the combination is not a recognised prelude call
/// (e.g. `DateTime.days(7)` is invalid — `days` belongs to `Duration`).
/// This is the function the type inferencer + codegen consult to decide
/// whether a `Type.method(args)` AST node is a prelude call.
///
/// T124g: when multiple assoc-fn variants share a name (e.g. `Args.get`
/// and `Env.get` are distinct variants both spelled `get`), the lookup
/// iterates ALL variants matching the method name and returns the first
/// whose `(type, method)` pair validates. Earlier versions returned the
/// first matching variant unconditionally and validated once — that
/// broke `Env.get(...)` because `ArgsGet` appears first in `ALL` and
/// `(Env, ArgsGet)` is not a valid pair. The new scan-with-validation
/// is correct for any number of same-named variants across distinct
/// types (the validation matrix in [`assoc_fn_return_type`] is the
/// single source of truth for which `(type, method)` pairs are legal).
pub fn assoc_fn_lookup(type_name: &str, method: &str) -> Option<(PreludeType, PreludeAssocFn)> {
    let t = prelude_type_lookup(type_name)?;
    // Scan ALL assoc-fn variants whose name matches, returning the first
    // whose (type, method) pair validates. Mirrors the same-named-variant
    // disambiguation strategy already used in `assoc_const_lookup`.
    for m in PreludeAssocFn::ALL.iter().copied() {
        if m.name() == method && assoc_fn_return_type(t, m, &[]).is_some() {
            return Some((t, m));
        }
    }
    None
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
    pub const ALL: &'static [PreludeAssocConst] = &[PreludeAssocConst::Pi, PreludeAssocConst::E];

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
        // T124g: Args module.
        // `Args.list()` -> Vector<String>. Wraps
        // `std::env::args().collect::<Vec<String>>()`. Returns every
        // command-line argument (program name at index 0, user args
        // from index 1 onwards).
        (PreludeType::Args, PreludeAssocFn::List) => Some(Type::vector(Type::string())),
        // `Args.get(i)` -> String. Wraps
        // `std::env::args().nth(i).unwrap_or_default()` (empty String
        // on out-of-bounds - NEVER panics).
        (PreludeType::Args, PreludeAssocFn::Get) => Some(Type::string()),
        // T124g: Env module.
        // `Env.get("KEY")` -> Option<String>. Wraps
        // `std::env::var(k).ok()`. None when the var is unset OR holds
        // invalid UTF-8 (both folded into None so the surface stays
        // panic-free and uniform). Same `Get` variant as Args.get but
        // dispatched on the (Env, Get) pair - mirrors the (DateTime,
        // Parse) / (Date, Parse) / (Toml, Parse) overload-by-type
        // pattern.
        (PreludeType::Env, PreludeAssocFn::Get) => Some(Type::option(Type::string())),
        // `Env.set("KEY", "value")` -> Void. Wraps
        // `std::env::set_var(k, v)`. NOTE: `set_var` is `unsafe` in
        // Rust 2024; Buff emits 2021 so safe today.
        (PreludeType::Env, PreludeAssocFn::Set) => Some(Type::Void),
        // `Env.has("KEY")` -> Bool. Wraps
        // `std::env::var(k).is_ok()`.
        (PreludeType::Env, PreludeAssocFn::Has) => Some(Type::bool()),
        // T124h: Base64 module.
        // `Base64.encode(bytes)` -> String. Wraps
        // `base64::Engine::encode(&general_purpose::STANDARD, bytes)`
        // (UFCS form so the Engine trait need not be in scope at the
        // call site). Returns the canonical base64 String form.
        (PreludeType::Base64, PreludeAssocFn::Encode) => Some(Type::string()),
        // `Base64.decode(s)` -> Vector<Byte>. Wraps
        // `base64::Engine::decode(&general_purpose::STANDARD, s)
        // .unwrap_or_default()` (empty Vec on decode failure - NEVER
        // panics, mirroring Regex.compile / Toml.parse / DateTime.parse's
        // panic-free stance).
        (PreludeType::Base64, PreludeAssocFn::Decode) => Some(Type::vector(Type::byte())),
        // T124h: Hex module.
        // `Hex.encode(bytes)` -> String. Wraps `hex::encode(bytes)`
        // (lowercase hex String).
        (PreludeType::Hex, PreludeAssocFn::Encode) => Some(Type::string()),
        // `Hex.decode(s)` -> Vector<Byte>. Wraps
        // `hex::decode(s).unwrap_or_default()` (empty Vec on failure -
        // NEVER panics).
        (PreludeType::Hex, PreludeAssocFn::Decode) => Some(Type::vector(Type::byte())),
        // T124h: URLEncode module.
        // `URLEncode.encode(s)` -> String. Wraps
        // `percent_encoding::utf8_percent_encode(s,
        // percent_encoding::NON_ALPHANUMERIC).to_string()`.
        (PreludeType::URLEncode, PreludeAssocFn::Encode) => Some(Type::string()),
        // `URLEncode.decode(s)` -> String. Wraps
        // `percent_encoding::percent_decode_str(s).decode_utf8_lossy()
        // .into_owned()` (invalid UTF-8 -> U+FFFD replacement char;
        // lossy decode, NEVER panics).
        (PreludeType::URLEncode, PreludeAssocFn::Decode) => Some(Type::string()),
        // T124h: UUID module.
        // `UUID.v4()` -> String. Wraps `uuid::Uuid::new_v4().to_string()`.
        (PreludeType::UUID, PreludeAssocFn::V4) => Some(Type::string()),
        // `UUID.v7()` -> String. Wraps `uuid::Uuid::now_v7().to_string()`.
        (PreludeType::UUID, PreludeAssocFn::V7) => Some(Type::string()),
        // `UUID.parse(s)` -> Bool. Wraps `uuid::Uuid::parse_str(s).is_ok()`
        // (validation only - returns Bool rather than a typed Uuid value,
        // since UUID surfaces as String in T124h). Same shared `Parse`
        // variant as DateTime.parse / Date.parse / Toml.parse / URL.parse.
        (PreludeType::UUID, PreludeAssocFn::Parse) => Some(Type::bool()),
        // T124h: URL module.
        // `URL.parse(s)` -> URL. Wraps
        // `url::Url::parse(s).unwrap_or_else(|_| url::Url::parse("about:blank")
        // .unwrap())` - the `about:blank` fallback is always parseable
        // (it's a reserved URL scheme), so the inner `.unwrap()` is
        // infallible at runtime (matches Regex.compile's `r"a^"` fallback
        // stance from T124d). Same shared `Parse` variant as
        // DateTime.parse / Date.parse / Toml.parse / UUID.parse.
        (PreludeType::URL, PreludeAssocFn::Parse) => Some(Type::Url),
        // T124i: Yaml module - mirrors the Toml module (T124e) exactly.
        // `Yaml.parse(s)` returns a Buff `Map` whose keys are String
        // (YAML mapping keys at the top level are heterogeneous in
        // general but the codegen turbofish pins to `HashMap<String,
        // serde_yml::Value>` so the surface Map contract is String-
        // keyed, matching Toml.parse). The value type is Unknown
        // (YAML values are heterogeneous: scalars / arrays / sub-
        // mappings; representing them all as a single Buff type would
        // require a `YamlValue` variant we deliberately don't add to
        // keep the surface minimal - parse + stringify, no schema,
        // mirroring the Toml Unknown-value-type stance). The codegen
        // emits the concrete `HashMap<String, serde_yml::Value>` via
        // turbofish so the generated Rust is fully typed.
        (PreludeType::Yaml, PreludeAssocFn::Parse) => {
            Some(Type::map(Type::string(), Type::Unknown))
        }
        // T124i: Yaml module - `Yaml.stringify(v)` returns a YAML-
        // formatted String. Mirrors Toml.stringify exactly (the
        // `serde_yml::to_string` API is structurally identical to
        // `toml::to_string` - both take `&impl Serialize` and return
        // `Result<String, _>`). The codegen borrows the arg via `&v`
        // so Rust's serde-Serialize bound is satisfied.
        (PreludeType::Yaml, PreludeAssocFn::Stringify) => Some(Type::string()),
        // T124i: Csv module - differs from Yaml / Toml in the surface
        // type but mirrors the parse + stringify shape. `Csv.parse(s)`
        // returns a `Vector<Vector<String>>` (uniform rows, NO header
        // special-casing - every row including the header is a
        // `Vector<String>`). The codegen builds a `csv::ReaderBuilder`
        // with `.has_headers(false)` so the header row is NOT consumed
        // by the reader (CSV surfaces as uniform rows per the spec).
        // The outer Vector is the rows; the inner Vector is the cells
        // of each row (String per cell - CSV has no inherent type
        // information, every cell is text).
        (PreludeType::Csv, PreludeAssocFn::Parse) => {
            Some(Type::vector(Type::vector(Type::string())))
        }
        // T124i: Csv module - `Csv.stringify(rows)` returns a CSV-
        // formatted String. The arg is a `Vector<Vector<String>>`
        // (rows of cells). The codegen builds a `csv::Writer` over
        // `Vec<u8>` and writes each row via `write_record`, then
        // converts the buffer to String (lossy/empty on failure -
        // NEVER panics, mirroring the Toml / Yaml stringify stance).
        (PreludeType::Csv, PreludeAssocFn::Stringify) => Some(Type::string()),
        // T124j: Path module - `Path.join(a, b, ...)` returns a
        // `Path` value (the chained join of all args). The codegen
        // emits `std::path::PathBuf::from(a).join(b).join(c)...` for
        // any number of args >= 1 (a single-arg `Path.join(a)`
        // returns a PathBuf of `a` itself, the no-op join). The arg
        // types are typically String or Path; the return type is
        // always `Path` (PathBuf at the codegen level).
        //
        // Same shared `Join` variant as Strings.join (T124f). The
        // (Path, Join) pair is dispatched on the receiver type.
        (PreludeType::Path, PreludeAssocFn::Join) => Some(Type::Path),
        // T124j: Dir module.
        // `Dir.list(path)` -> Vector<String>. Wraps
        // `std::fs::read_dir(p).filter_map(|e| e.ok()).map(|e|
        // e.file_name().to_string_lossy().into_owned())
        // .collect::<Vec<String>>()` (skip inaccessible entries -
        // NEVER panics). Returns entry NAMES (NOT paths) - the
        // surface mirrors the typical shell `ls` / Python
        // `os.listdir` semantics. Same shared `List` variant as
        // Args.list (T124g); dispatched on the (Dir, List) pair.
        (PreludeType::Dir, PreludeAssocFn::List) => Some(Type::vector(Type::string())),
        // `Dir.create(path)` -> Void. Wraps
        // `std::fs::create_dir_all(p).ok()` (creates the directory
        // and any missing parents - mirrors `mkdir -p`; discards
        // errors via `.ok()` - NEVER panics). Returns Void.
        (PreludeType::Dir, PreludeAssocFn::Create) => Some(Type::Void),
        // `Dir.remove(path)` -> Void. Wraps
        // `std::fs::remove_dir_all(p).ok()` (removes the directory
        // tree recursively; discards errors via `.ok()` - NEVER
        // panics, mirroring the Dir.create stance).
        (PreludeType::Dir, PreludeAssocFn::Remove) => Some(Type::Void),
        // `Dir.walk(path)` -> Vector<Path>. Wraps
        // `walkdir::WalkDir::new(p).into_iter().filter_map(|e| e.ok())
        // .map(|e| e.path().to_path_buf())
        // .collect::<Vec<std::path::PathBuf>>()` (skip inaccessible
        // entries via `.filter_map(|e| e.ok())` - NEVER panics,
        // mirroring the Csv.parse panic-free stance). The walkdir
        // crate is recorded in codegen `extern_crates` when a Buff
        // program uses `Dir.walk`.
        (PreludeType::Dir, PreludeAssocFn::Walk) => Some(Type::vector(Type::Path)),
        // T124j: Tempfile module.
        // `Tempfile.create()` -> Path. Wraps
        // `tempfile::NamedTempFile::new().map(|f|
        // f.into_temp_path().keep().unwrap_or_default())
        // .unwrap_or_default()` (panic-free - empty PathBuf on
        // failure - NEVER panics). The `into_temp_path().keep()`
        // chain persists the temp file's path beyond the
        // NamedTempFile's drop (the file becomes a regular file
        // the user can write/read/delete like any other).
        (PreludeType::Tempfile, PreludeAssocFn::Create) => Some(Type::Path),
        // `Tempfile.dir()` -> Path. Wraps `std::env::temp_dir()`
        // (the `tempfile::env::temp_dir()` is a re-export of the
        // std fn; we splice the std path directly so NO extern
        // crate is needed for this call alone - but the narrow
        // walker records `tempfile` for symmetry with
        // Tempfile.create).
        (PreludeType::Tempfile, PreludeAssocFn::Dir) => Some(Type::Path),
        // T124k: Hash module - 3 assoc fns wrapping the `sha2`
        // (SHA-256 / SHA-512) + `md5` RustCrypto crates. Each
        // returns a lowercase hex String.
        //
        // `Hash.sha256(data)` -> String (64-char hex). Wraps
        // `{ use sha2::Digest; hex::encode(sha2::Sha256::digest
        // (d.as_bytes())) }` (block-scoped `use` brings the
        // `Digest` trait method into scope without polluting the
        // caller's namespace). The arg accepts String OR
        // Vector<Byte> (anything `AsRef<[u8]>` at the codegen
        // layer).
        (PreludeType::Hash, PreludeAssocFn::Sha256) => Some(Type::string()),
        // `Hash.sha512(data)` -> String (128-char hex). Same shape
        // as sha256 but `Sha512`.
        (PreludeType::Hash, PreludeAssocFn::Sha512) => Some(Type::string()),
        // `Hash.md5(data)` -> String (32-char hex). Wraps
        // `hex::encode(md5::compute(d.as_bytes()).0)`. **MD5 is
        // CRYPTOGRAPHICALLY BROKEN** - exposed for checksum
        // compatibility only; NEVER use for security.
        (PreludeType::Hash, PreludeAssocFn::Md5) => Some(Type::string()),
        // T124k: HMAC module - 1 assoc fn wrapping the `hmac` +
        // `sha2` RustCrypto crates.
        //
        // `HMAC.sha256(key, data)` -> String (64-char hex). Wraps
        // `{ use hmac::Mac; hmac::Hmac::<sha2::Sha256>
        // ::new_from_slice(k.as_bytes()).map(|mut mac| {
        // mac.update(d.as_bytes()); hex::encode(mac.finalize()
        // .into_bytes()) }).unwrap_or_default() }` (block-scoped
        // `use` for the `Mac` trait methods). `new_from_slice`
        // returns `Result<Hmac<Sha256>, MacError>` and accepts ANY
        // key length (HMAC has no fixed key size); the `.map()
        // .unwrap_or_default()` collapses Err to empty String -
        // NEVER panics, matching Buff's "no panicking generated
        // code" rule. Same shared `Sha256` variant as
        // `Hash.sha256`; dispatched on the (HMAC, Sha256) pair.
        (PreludeType::HMAC, PreludeAssocFn::Sha256) => Some(Type::string()),
        // T124l: Process module - 2 assoc fns. `Process.spawn` is
        // the runtime-value ctor (returns `Process`); `Process.exit`
        // is a side-effecting terminal call (returns `Void`).
        //
        // `Process.spawn(command, args)` -> Process. Wraps
        // `std::process::Command::new(cmd).args(args).spawn().ok()`
        // (the `.ok()` collapses a spawn failure to `None` -
        // NEVER panics). The command + args are passed SEPARATELY
        // (NOT through a shell) - no shell-injection vector. The
        // returned `Process` value is the receiver for the `.wait()`
        // / `.id()` instance methods.
        (PreludeType::Process, PreludeAssocFn::Spawn) => Some(Type::Process),
        // `Process.exit(code)` -> Void. Wraps
        // `std::process::exit(code as i32)`. The call NEVER returns
        // (it terminates the program immediately). The Buff surface
        // types it as `Void` so callers don't try to use the result.
        // NOTE: Rust's `std::process::exit` does NOT run destructors;
        // the Buff surface inherits that behavior (the spec calls
        // this out as the "exit yourself" primitive).
        (PreludeType::Process, PreludeAssocFn::Exit) => Some(Type::Void),
        // T124l: OS module - 4 assoc fns wrapping std::env::consts +
        // env-var hostname + num_cpus.
        //
        // `OS.name()` -> String. Wraps `std::env::consts::OS
        // .to_string()` (compile-time const - one of `linux` /
        // `macos` / `windows` / `freebsd` / ...). OS-only.
        (PreludeType::OS, PreludeAssocFn::Name) => Some(Type::string()),
        // `OS.arch()` -> String. Wraps `std::env::consts::ARCH
        // .to_string()` (compile-time const - one of `x86_64` /
        // `aarch64` / `x86` / ...). OS-only.
        (PreludeType::OS, PreludeAssocFn::Arch) => Some(Type::string()),
        // `OS.hostname()` -> String. Wraps
        // `std::env::var("COMPUTERNAME").or_else(|_|
        // std::env::var("HOSTNAME")).unwrap_or_default()` (empty
        // String when neither env var is set - NEVER panics). The
        // bare-minimum env-var approach (NO `hostname` crate added,
        // per spec). OS-only.
        (PreludeType::OS, PreludeAssocFn::Hostname) => Some(Type::string()),
        // `OS.cpus()` -> Int. Wraps `num_cpus::get() as i64`. The
        // `num_cpus` crate is recorded in codegen `extern_crates`
        // when a Buff program uses `OS.cpus` (the narrow walker flags
        // ONLY the `cpus` method name - `name` / `arch` / `hostname`
        // use std only). OS-only.
        (PreludeType::OS, PreludeAssocFn::Cpus) => Some(Type::int_default()),
        // T124m: TCP module - 1 assoc fn: TCP.connect(host, port)
        // -> Connection. Wraps `tokio::net::TcpStream::connect
        // (format!("{}:{}", h, p)).await.ok()` (the `.ok()`
        // collapses a connect failure to `None` - NEVER panics).
        // The returned `Connection` value is the receiver for the
        // `.send()` / `.recv()` / `.close()` instance methods.
        // `tokio` is recorded in codegen `extern_crates`
        // (idempotent with the existing tokio walker).
        (PreludeType::TCP, PreludeAssocFn::Connect) => Some(Type::Connection),
        // T124m: UDP module - 1 assoc fn: UDP.bind(host, port) ->
        // Socket. Wraps `tokio::net::UdpSocket::bind(format!("{}:{}",
        // h, p)).await.ok()` (the `.ok()` collapses a bind failure
        // to `None` - NEVER panics). The returned `Socket` value is
        // the receiver for the `.send_to()` / `.recv_from()`
        // instance methods.
        (PreludeType::UDP, PreludeAssocFn::Bind) => Some(Type::Socket),
        // T124m: WebSocket module - 1 assoc fn: WebSocket.connect
        // (url) -> WsConnection. Wraps `tokio_tungstenite::connect_async
        // (url).await.ok().map(|(ws, _)| ws)` (the `.ok()` + `.map()`
        // chain collapses a connect failure to `None` - NEVER
        // panics). The returned `WsConnection` value is the
        // receiver for the `.send()` / `.recv()` / `.close()`
        // instance methods. `tokio-tungstenite` + `futures-util`
        // are recorded in codegen `extern_crates` (via the narrow
        // `program_uses_tokio_tungstenite` walker).
        //
        // Same shared `Connect` variant as `TCP.connect(host, port)`
        // (mirrors `Parse` shared between DateTime / Date / Toml /
        // URL / UUID). Dispatched on the (WebSocket, Connect) pair.
        (PreludeType::WebSocket, PreludeAssocFn::Connect) => Some(Type::WsConnection),
        // T2: Channel.new(buf_size) -> (Sender, Receiver). Returns a
        // tuple of opaque runtime-value types. The element type T is
        // implicit at this layer (Type-level we don't carry generic
        // params on prelude types); Rust's type inference derives T
        // from subsequent `sender.send(value)` / `receiver.recv()`
        // usage at the codegen level.
        (PreludeType::Channel, PreludeAssocFn::New) => Some(Type::tuple(vec![
            Type::Sender,
            Type::Receiver,
        ])),
        // T8: Tensor constructor assoc fns. Each returns the opaque
        // Tensor value type. For MVP we surface `Type::Unknown`
        // because the coordinated `Type::Tensor` variant lives in
        // `ty.rs` which is OUTSIDE the T8 shared zone (sibling-task
        // coordination concern). The codegen lowering + the
        // `Type::Tensor` variant are added in a follow-up task. This
        // forward-declaration lets `buff check` validate
        // `Tensor.zeros([3, 4])` syntax today (parses + resolves +
        // return-type-checks as Unknown); `buff run` codegen
        // integration ships when the coordinated sibling task lands.
        //
        // The 4 fns cover the spec-mandated constructors (T8 spec
        // line 1469: zeros / from_vec; plus ones + filled as
        // natural symmetric siblings matching buff_tensor's Rust
        // surface).
        (PreludeType::Tensor, PreludeAssocFn::Zeros) => Some(Type::Unknown),
        (PreludeType::Tensor, PreludeAssocFn::Ones) => Some(Type::Unknown),
        (PreludeType::Tensor, PreludeAssocFn::FromVec) => Some(Type::Unknown),
        (PreludeType::Tensor, PreludeAssocFn::Filled) => Some(Type::Unknown),
        // T7: DataFrame assoc fns return the runtime-value DataFrame
        // type (NOT Unknown, unlike T8 Tensor which forward-declares
        // only). Both ctors are panic-free at the codegen layer:
        // `buff_dataframe::DataFrame::from_csv(path)
        // .unwrap_or_default()` collapses file-not-found / parse
        // failure to an empty DataFrame (matches Buff's "no
        // panicking generated code" rule).
        (PreludeType::DataFrame, PreludeAssocFn::FromCsv) => Some(Type::DataFrame),
        (PreludeType::DataFrame, PreludeAssocFn::FromJson) => Some(Type::DataFrame),
        // T9: Image assoc fns. `Image.from_path(path)` -> Image.
        // Wraps `buff_image::Image::from_path(p)?` (the `?`
        // propagates ImageError per Buff's R3 error-mapping contract).
        (PreludeType::Image, PreludeAssocFn::FromPath) => Some(Type::Image),
        // `Image.from_bytes(bytes)` -> Image. Wraps
        // `buff_image::Image::from_bytes(&b)?`. Used for HTTP-downloaded
        // image bytes / database BLOBs.
        (PreludeType::Image, PreludeAssocFn::FromBytes) => Some(Type::Image),
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
    ///         Key ordering is DETERMINISTIC (group-index order; named groups
    /// intercalated at their source position).
    Captures,
    // ---- URL accessors (T124h) ---------------------------------------
    /// `url.scheme` - the URL scheme (`"https"`, `"file"`, ...). Zero
    /// args. Returns `String`. Wraps `url::Url::scheme().to_string()`
    /// (the `.to_string()` lifts `&str` to `String` - Buff hides
    /// references from users).
    Scheme,
    /// `url.host` - the URL host (`"example.com"`, ...). Zero args.
    /// Returns `String` (empty String when the URL has no host -
    /// NEVER panics). Wraps
    /// `url::Url::host_str().unwrap_or_default().to_string()`.
    Host,
    /// `url.path` - the URL path (`"/index.html"`, ...). Zero args.
    /// Returns `String`. Wraps `url::Url::path().to_string()`.
    Path,
    /// `url.query(key)` - look up a query parameter by key. One arg
    /// (String). Returns `Option<String>` (None when the key is
    /// absent - NEVER panics). Wraps
    /// `url::Url::query_pairs().find(|(k, _)| *k ==
    /// key.to_string()).map(|(_, v)| v.into_owned())` (linear scan;
    /// deterministic - first match wins).
    Query,
    // ---- Path instance methods (T124j) ------------------------------
    /// `path.parent()` - the parent directory of a Path value. Zero
    /// args. Returns `Option<Path>` (None when the path has no
    /// parent - e.g. `/` or a bare filename - NEVER panics). Wraps
    /// `std::path::Path::parent().map(|p| p.to_path_buf())` (the
    /// `.to_path_buf()` lifts `&Path` to owned `PathBuf` - Buff
    /// hides references from users).
    Parent,
    /// `path.extension()` - the file extension of a Path value
    /// (without the leading `.`). Zero args. Returns
    /// `Option<String>` (None when there's no extension - NEVER
    /// panics). Wraps `std::path::Path::extension().map(|e|
    /// e.to_string())`.
    Extension,
    /// `path.basename()` - the trailing filename component of a
    /// Path value. Zero args. Returns `String` (empty String when
    /// the path terminates in `..` or `/` - NEVER panics). Wraps
    /// `std::path::Path::file_name().and_then(|n| n.to_str())
    /// .unwrap_or_default().to_string()` (the `.and_then(|n|
    /// n.to_str())` handles non-UTF-8 filenames lossy-ly - they
    /// become None and fall through to the empty String default).
    Basename,
    /// `path.exists()` - test whether a Path value refers to an
    /// existing path on disk. Zero args. Returns `Bool`. Wraps
    /// `std::path::Path::exists()` (the underlying std method is
    /// infallible - returns `false` on permission errors, never
    /// panics).
    Exists,
    // ---- Process instance methods (T124l) -------------------------
    /// `process.wait() -> Int` - block until the spawned process
    /// exits, then return its exit code. Zero args. Wraps
    /// `recv.map(|mut c| c.wait().map(|s| s.code().unwrap_or_default())
    /// .unwrap_or_default()).unwrap_or_default()` (the outer
    /// Option handles the spawn-failed case; the middle Result
    /// handles wait() failure; the inner Option handles
    /// signal-terminated processes that have no exit code - all
    /// collapse to `0` via `unwrap_or_default()`, NEVER panics).
    /// Process-only.
    Wait,
    /// `process.id() -> Int` - the OS process ID of the spawned
    /// child. Zero args. Wraps `recv.map(|c| c.id() as i64)
    /// .unwrap_or_default()` (0 when the spawn failed or the
    /// process has already exited and been reaped - NEVER panics).
    /// Process-only.
    Id,
    // ---- Networking instance methods (T124m) ---------------------
    // These follow the precedent set by `Format` (DateTime / Date /
    // Time), `Scheme` / `Host` / `Path` / `Query` (URL), `Parent` /
    // `Extension` / `Basename` / `Exists` (Path), `Wait` / `Id`
    // (Process): a single variant is dispatched on (Type, method)
    // pair. Some variants are shared across receiver types
    // (Connection.send + WsConnection.send share the `Send`
    // variant; Connection.recv + WsConnection.recv share `Recv`;
    // Connection.close + WsConnection.close share `Close`) -
    // mirrors `Format` shared between DateTime / Date / Time.
    /// `connection.send(data) -> Void` (TCP). One arg (String).
    /// Wraps `{ use tokio::io::AsyncWriteExt; if let Some(mut s)
    /// = conn { s.write_all(data.as_bytes()).await.ok(); } }`
    /// (block-scoped trait import, panic-free via `.ok()`). The
    /// connect-failed case (None) is a no-op.
    Send,
    /// `connection.recv() -> Vector<Byte>` (TCP). Zero args. Wraps
    /// `{ use tokio::io::AsyncReadExt; let mut buf = Vec::new();
    /// if let Some(mut s) = conn { let _ = s.read(&mut buf).await; }
    /// buf }` (returns empty Vec on EOF / error - NEVER panics).
    /// Distinct from WsConnection.recv which returns String.
    Recv,
    /// `connection.close() -> Void` (TCP). Zero args. Wraps
    /// `{ use tokio::io::AsyncWriteExt; if let Some(mut s) = conn
    /// { s.shutdown().await.ok(); } }` (panic-free via `.ok()`).
    /// Same `Close` variant dispatched on WsConnection (different
    /// lowering - SinkExt::close).
    Close,
    /// `socket.send_to(data, addr) -> Void` (UDP). Two args
    /// (String data, String addr). Wraps `{ if let Some(mut s) =
    /// sock { s.send_to(data.as_bytes(), addr).await.ok(); } }`
    /// (panic-free via `.ok()`). UDP-only.
    SendTo,
    /// `socket.recv_from() -> Tuple` (UDP). Zero args. Returns
    /// `(Vector<Byte>, String)` (the datagram bytes + the sender
    /// addr). Wraps `{ let mut buf = vec![0u8; 65535]; if let
    /// Some(mut s) = sock { return s.recv_from(&mut buf).await.ok()
    /// .map(|(n, addr)| (buf[..n].to_vec(), addr.to_string())); }
    /// (Vec::new(), String::new()) }` (panic-free via `.ok()` +
    /// tuple fallback). UDP-only.
    RecvFrom,
    // ---- DataFrame instance methods (T7) -----------------------------
    // Each variant lowers to the matching `buff_dataframe::DataFrame`
    // method in codegen. Dispatched on the (Type::DataFrame, method)
    // pair. The methods return Type::DataFrame (chainable) or
    // Type::int_default() / Type::string() for terminal accessors.
    /// `df.select(cols) -> DataFrame`. One arg (Vector<String>).
    /// Projection: returns a new DataFrame with only the named
    /// columns. Wraps `buff_dataframe::DataFrame::select(recv,
    /// &cols).unwrap_or_default()` (panic-free via
    /// `Result::unwrap_or_default` - DataFrame impls Default as the
    /// empty frame).
    Select,
    /// `df.filter(predicate) -> DataFrame`. One arg (closure
    /// `|RowView| -> Bool`). Returns a new DataFrame keeping only
    /// rows where the predicate returns true. Wraps
    /// `buff_dataframe::DataFrame::filter(recv, |row| <closure
    /// body>).unwrap_or_default()`.
    Filter,
    /// `df.sort(col) -> DataFrame`. One arg (String column name).
    /// Ascending lexicographic sort by the given column's cells.
    /// Wraps `buff_dataframe::DataFrame::sort(recv, col)
    /// .unwrap_or_default()`.
    Sort,
    /// `df.head(n) -> DataFrame`. One arg (Int). Returns a new
    /// DataFrame with the first `n` rows (clamped to df.len()).
    /// Wraps `buff_dataframe::DataFrame::head(recv, n.max(0) as
    /// usize)`.
    Head,
    /// `df.len() -> Int`. Zero args. Returns the row count.
    /// Wraps `buff_dataframe::DataFrame::len(recv) as i64`.
    /// Same shared `Len` variant as `Series.len` / `Vector.len` /
    /// `Map.len` (mirrors `Format` shared between DateTime/Date/Time
    /// — dispatched on receiver type).
    Len,
    /// `df.join(other, on) -> DataFrame`. Two args (DataFrame other,
    /// String on-column). Inner equi-join. Wraps
    /// `buff_dataframe::DataFrame::join(recv, other, on)
    /// .unwrap_or_default()`.
    Join,
    /// `df.group_by(col) -> DataFrame`. One arg (String column name).
    /// Returns a new DataFrame carrying the per-group aggregation
    /// state. Followed by `.agg(col, op)` to materialise. Wraps
    /// `buff_dataframe::DataFrame::group_by(recv, col)
    /// .map(|gb| gb.into_inner()).unwrap_or_default()` (the
    /// `into_inner` re-exposes the GroupBy's parent so subsequent
    /// `.agg` calls are dispatched on a DataFrame receiver).
    GroupBy,
    /// `df.agg(col, op) -> DataFrame`. Two args (String column name,
    /// String aggregation op — one of "sum"/"mean"/"min"/"max"/
    /// "count"). Returns a new DataFrame with the aggregated values
    /// per group (or a single-row aggregate if `df` is not a grouped
    /// DataFrame). Wraps `buff_dataframe::DataFrame::agg(recv, col,
    /// op).unwrap_or_default()`.
    Agg,
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
        // T124h: URL instance accessors — Scheme / Host / Path / Query.
        // Zero-arg accessors (scheme/host/path) and one-arg query lookup.
        // Mirrors Regex's instance-method-carrying runtime-value
        // pattern (T124d) as the second such type.
        PreludeInstanceFn::Scheme,
        PreludeInstanceFn::Host,
        PreludeInstanceFn::Path,
        PreludeInstanceFn::Query,
        // T124j: Path instance methods — Parent / Extension / Basename /
        // Exists. All zero-arg accessors mirroring the URL accessor
        // pattern (T124h). Mirrors Regex (T124d) / URL (T124h) as the
        // third runtime-value-with-rich-instance-methods type.
        PreludeInstanceFn::Parent,
        PreludeInstanceFn::Extension,
        PreludeInstanceFn::Basename,
        PreludeInstanceFn::Exists,
        // T124l: Process instance methods — Wait / Id. Both
        // zero-arg, returning Int (exit code / OS pid).
        // Mirrors Regex (T124d) / URL (T124h) / Path (T124j) as
        // the fourth runtime-value-with-rich-instance-methods
        // type.
        PreludeInstanceFn::Wait,
        PreludeInstanceFn::Id,
        // T124m: Networking instance methods - 5 distinct names:
        // send / recv / close / send_to / recv_from. `Send` /
        // `Recv` / `Close` are each shared between Connection
        // (TCP) and WsConnection (WebSocket) - same name,
        // different per-type lowering (mirrors `Format` shared
        // between DateTime / Date / Time). `SendTo` / `RecvFrom`
        // are Socket-only (UDP). Mirrors Regex (T124d) / URL
        // (T124h) / Path (T124j) / Process (T124l) as the fifth
        // / sixth / seventh runtime-value-with-instance-methods
        // types.
        PreludeInstanceFn::Send,
        PreludeInstanceFn::Recv,
        PreludeInstanceFn::Close,
        PreludeInstanceFn::SendTo,
        PreludeInstanceFn::RecvFrom,
        // T7: DataFrame instance methods (7 distinct names):
        // select / filter / sort / head / len / join / group_by / agg.
        // `Join` is shared with Strings.join + Path.join (existing
        // variant - re-used here, dispatched on the (DataFrame, Join)
        // pair). `Len` is shared with the future Series.len /
        // Vector.len / Map.len (new variant here, dispatched on
        // receiver type). The other 5 are DataFrame-only.
        PreludeInstanceFn::Select,
        PreludeInstanceFn::Filter,
        PreludeInstanceFn::Sort,
        PreludeInstanceFn::Head,
        PreludeInstanceFn::Len,
        PreludeInstanceFn::GroupBy,
        PreludeInstanceFn::Agg,
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
            // T124h: URL instance method names mirror the `url::Url`
            // accessor names so the codegen can splice
            // `recv.scheme().to_string()` etc. without rewriting.
            // `scheme` / `host` / `path` are zero-arg field-style
            // accessors (added to `KNOWN_ZERO_ARG_METHODS` so the T26
            // field-access heuristic doesn't rewrite them as field
            // accesses). `query` takes one arg (the key).
            PreludeInstanceFn::Scheme => "scheme",
            PreludeInstanceFn::Host => "host",
            PreludeInstanceFn::Path => "path",
            PreludeInstanceFn::Query => "query",
            // T124j: Path instance method names mirror Rust's
            // `std::path::Path` method names where they exist
            // (`parent` / `extension` / `exists` map 1:1 to the std
            // methods). `basename` is a Buff-flavored name (clearer
            // than Rust's `file_name` - basename is the canonical
            // POSIX / Python / Node term); codegen rewrites it to
            // `recv.file_name().and_then(|n| n.to_str())
            // .unwrap_or_default().to_string()`.
            PreludeInstanceFn::Parent => "parent",
            PreludeInstanceFn::Extension => "extension",
            PreludeInstanceFn::Basename => "basename",
            PreludeInstanceFn::Exists => "exists",
            // T124l: Process instance method names mirror Rust's
            // `std::process::Child` method names where they exist
            // (`wait` / `id` map 1:1 to the std methods, modulo
            // the Option-wrapper layer the codegen adds). Both
            // zero-arg.
            PreludeInstanceFn::Wait => "wait",
            PreludeInstanceFn::Id => "id",
            // T124m: Networking instance method names mirror the
            // canonical tokio / futures-util verbs. `send` / `recv`
            // / `close` are universal (TCP + WebSocket share them);
            // `send_to` / `recv_from` are UDP's distinct recv-from-
            // with-sender verbs. Dispatched on the (receiver-type,
            // method) pair (mirrors `Format` shared between DateTime
            // / Date / Time).
            PreludeInstanceFn::Send => "send",
            PreludeInstanceFn::Recv => "recv",
            PreludeInstanceFn::Close => "close",
            PreludeInstanceFn::SendTo => "send_to",
            PreludeInstanceFn::RecvFrom => "recv_from",
            // T7: DataFrame instance method names mirror the
            // buff_dataframe crate's method names 1:1 so the
            // codegen can splice `recv.select(...)` / `recv.head(n)`
            // / `recv.group_by(col)` etc. without rewriting.
            // `len` is the shared `Len` variant (also dispatched on
            // Vector / Map / Series receivers in future tasks).
            PreludeInstanceFn::Select => "select",
            PreludeInstanceFn::Filter => "filter",
            PreludeInstanceFn::Sort => "sort",
            PreludeInstanceFn::Head => "head",
            PreludeInstanceFn::Len => "len",
            PreludeInstanceFn::GroupBy => "group_by",
            PreludeInstanceFn::Agg => "agg",
            PreludeInstanceFn::Join => "join",
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

        // T124h: URL instance accessors. `url.scheme` / `url.host` /
        // `url.path` each return String; `url.query(key)` returns
        // Option<String>. Each lowers to a fully-qualified `url::Url`
        // method chained with `.to_string()` (Buff hides references from
        // users; the underlying Rust accessors return `&str`).
        //
        // `url.scheme` -> String.
        (Type::Url, PreludeInstanceFn::Scheme) => Some(Type::string()),
        // `url.host` -> String (empty when the URL has no host - NEVER
        // panics, matches Buff's "no panicking generated code" stance).
        (Type::Url, PreludeInstanceFn::Host) => Some(Type::string()),
        // `url.path` -> String.
        (Type::Url, PreludeInstanceFn::Path) => Some(Type::string()),
        // `url.query(key)` -> Option<String>. None when the key is
        // absent - NEVER panics.
        (Type::Url, PreludeInstanceFn::Query) => Some(Type::option(Type::string())),

        // T124j: Path instance methods. `path.parent()` ->
        // Option<Path>; `path.extension()` -> Option<String>;
        // `path.basename()` -> String; `path.exists()` -> Bool.
        // Each lowers to a fully-qualified `std::path::Path` method
        // (Buff hides references from users; the underlying Rust
        // accessors return `Option<&Path>` / `Option<&OsStr>` /
        // `Option<&OsStr>` / `bool`).
        //
        // `path.parent()` -> Option<Path>. Wraps `recv.parent()
        // .map(|p| p.to_path_buf())` (the `.to_path_buf()` lifts
        // `&Path` to owned `PathBuf` - Buff surfaces owned values).
        (Type::Path, PreludeInstanceFn::Parent) => Some(Type::option(Type::Path)),
        // `path.extension()` -> Option<String>. Wraps
        // `recv.extension().map(|e| e.to_string())`.
        (Type::Path, PreludeInstanceFn::Extension) => Some(Type::option(Type::string())),
        // `path.basename()` -> String. Wraps `recv.file_name()
        // .and_then(|n| n.to_str()).unwrap_or_default().to_string()`
        // (lossy on non-UTF-8 filenames - NEVER panics).
        (Type::Path, PreludeInstanceFn::Basename) => Some(Type::string()),
        // `path.exists()` -> Bool. Wraps `recv.exists()`.
        (Type::Path, PreludeInstanceFn::Exists) => Some(Type::bool()),

        // T124l: Process instance methods. `process.wait()` ->
        // Int (exit code); `process.id()` -> Int (OS pid). Each
        // lowers to a fully-qualified `std::process::Child`
        // method chained through the `Option<Child>` wrapper the
        // codegen adds at spawn time (so the calls are panic-free
        // even when spawn failed - the Option collapses to a
        // default Int).
        //
        // `process.wait()` -> Int. Wraps
        // `recv.map(|mut c| c.wait().map(|s| s.code()
        // .unwrap_or_default()).unwrap_or_default())
        // .unwrap_or_default()` (the outer Option handles the
        // spawn-failed case; the middle Result handles wait()
        // failure; the inner Option handles signal-terminated
        // processes that have no exit code - all collapse to `0`,
        // NEVER panics).
        (Type::Process, PreludeInstanceFn::Wait) => Some(Type::int_default()),
        // `process.id()` -> Int. Wraps `recv.map(|c| c.id() as
        // i64).unwrap_or_default()` (0 when the spawn failed or
        // the process has already exited and been reaped - NEVER
        // panics).
        (Type::Process, PreludeInstanceFn::Id) => Some(Type::int_default()),

        // T124m: TCP-Connection instance methods. `conn.send(d)` ->
        // Void; `conn.recv()` -> Vector<Byte>; `conn.close()` ->
        // Void. Each lowers to a fully-qualified `tokio::io::
        // AsyncReadExt` / `AsyncWriteExt` trait method chained
        // through the `Option<TcpStream>` wrapper the codegen adds
        // at connect time (`TCP.connect` -> `TcpStream::connect()
        // .ok()`). The Option-wrapper layer keeps the calls
        // panic-free even when connect failed - the Option's None
        // branch is a no-op (send / close) or empty Vec (recv).
        //
        // `conn.send(d) -> Void`. Wraps
        // `{ use tokio::io::AsyncWriteExt; if let Some(mut s) =
        // recv { s.write_all(d.as_bytes()).await.ok(); } }` (the
        // block scopes the trait import; the `.ok()` discards the
        // write result; the Option None branch is a no-op - NEVER
        // panics). One arg (String).
        (Type::Connection, PreludeInstanceFn::Send) => Some(Type::Void),
        // `conn.recv() -> Vector<Byte>`. Wraps
        // `{ use tokio::io::AsyncReadExt; let mut buf = Vec::new();
        // if let Some(mut s) = recv { let _ = s.read(&mut buf)
        // .await; } buf }` (returns empty Vec on EOF / error /
        // connect-failed - NEVER panics). Zero args. Returns
        // `Vector<Byte>` (Vec<u8> at the codegen level).
        (Type::Connection, PreludeInstanceFn::Recv) => Some(Type::vector(Type::byte())),
        // `conn.close() -> Void`. Wraps
        // `{ use tokio::io::AsyncWriteExt; if let Some(mut s) =
        // recv { s.shutdown().await.ok(); } }` (graceful shutdown
        // of the write side; the Option None branch is a no-op -
        // NEVER panics). Zero args. Same `Close` variant dispatched
        // on WsConnection (different lowering - SinkExt::close).
        (Type::Connection, PreludeInstanceFn::Close) => Some(Type::Void),

        // T124m: UDP-Socket instance methods. `sock.send_to(d, addr)`
        // -> Void; `sock.recv_from() -> Tuple`. Each lowers to a
        // fully-qualified `tokio::net::UdpSocket` async method
        // chained through the `Option<UdpSocket>` wrapper the
        // codegen adds at bind time (`UDP.bind` ->
        // `UdpSocket::bind().ok()`). The Option-wrapper layer
        // keeps the calls panic-free even when bind failed.
        //
        // `sock.send_to(d, addr) -> Void`. Wraps
        // `{ if let Some(s) = recv { s.send_to(d.as_bytes(), addr)
        // .await.ok(); } }` (the Option None branch is a no-op -
        // NEVER panics). Two args (String data, String addr).
        (Type::Socket, PreludeInstanceFn::SendTo) => Some(Type::Void),
        // `sock.recv_from() -> Tuple`. Returns
        // `(Vector<Byte>, String)` (the datagram bytes + the
        // sender addr). Wraps `{ let mut buf = vec![0u8; 65535];
        // if let Some(s) = recv { return s.recv_from(&mut buf)
        // .await.ok().map(|(n, addr)| (buf[..n].to_vec(), addr.
        // to_string())); } (Vec::new(), String::new()) }`
        // (returns empty tuple on connect-failed / recv error -
        // NEVER panics). Zero args. The 65535 buffer size is the
        // max UDP datagram payload (the spec's bare-minimum cap).
        (Type::Socket, PreludeInstanceFn::RecvFrom) => Some(Type::tuple(vec![
            Type::vector(Type::byte()),
            Type::string(),
        ])),

        // T124m: WebSocket-WsConnection instance methods.
        // `ws.send(text)` -> Void; `ws.recv()` -> String;
        // `ws.close()` -> Void. Each lowers to a fully-qualified
        // `futures_util::SinkExt` / `StreamExt` trait method
        // chained through the `Option<WebSocketStream<...>>`
        // wrapper the codegen adds at connect time.
        //
        // `ws.send(text) -> Void`. Wraps
        // `{ use futures_util::SinkExt; if let Some(mut s) = recv
        // { s.send(tokio_tungstenite::tungstenite::Message::Text(
        // text)).await.ok(); } }` (block-scoped trait import; the
        // `.ok()` discards the send result; the Option None branch
        // is a no-op - NEVER panics). One arg (String text). Same
        // `Send` variant as Connection.send (TCP); dispatched on
        // the (WsConnection, Send) pair.
        (Type::WsConnection, PreludeInstanceFn::Send) => Some(Type::Void),
        // `ws.recv() -> String`. Wraps
        // `{ use futures_util::StreamExt; if let Some(mut s) =
        // recv { while let Some(Ok(msg)) = s.next().await { if let
        // tokio_tungstenite::tungstenite::Message::Text(t) = msg
        // { return t; } } } String::new() }` (returns empty
        // String on connect-failed / closed / non-text message -
        // NEVER panics). Zero args. Returns String (NOT Option nor
        // Vector<Byte> - WebSocket's text-frame surface is the
        // canonical String form). Distinct from Connection.recv
        // (TCP) which returns Vector<Byte>.
        (Type::WsConnection, PreludeInstanceFn::Recv) => Some(Type::string()),
        // `ws.close() -> Void`. Wraps
        // `{ use futures_util::SinkExt; if let Some(mut s) = recv
        // { s.close(None).await.ok(); } }` (sends a Close frame;
        // the Option None branch is a no-op - NEVER panics). Zero
        // args. Same `Close` variant as Connection.close (TCP);
        // dispatched on the (WsConnection, Close) pair (different
        // lowering - SinkExt::close vs AsyncWriteExt::shutdown).
        (Type::WsConnection, PreludeInstanceFn::Close) => Some(Type::Void),

        // T2: Channel-Sender / Channel-Receiver instance methods.
        // `sender.send(value)` -> Void (MVP - the Result<(), Error>
        // spec API is collapsed via .ok() and discarded, mirroring
        // Connection.send from T124m; v1.18+ may surface the Result).
        // `receiver.recv()` -> Option<T> (the element type T is
        // Unknown at this layer; codegen lets Rust infer it).
        // `receiver.close()` -> Void (sync, NOT async).
        (Type::Sender, PreludeInstanceFn::Send) => Some(Type::Void),
        (Type::Receiver, PreludeInstanceFn::Recv) => Some(Type::option(Type::Unknown)),
        (Type::Receiver, PreludeInstanceFn::Close) => Some(Type::Void),

        // T7: DataFrame instance methods. Each chainable method
        // returns Type::DataFrame so the user can write
        // `df.select(cols).filter(pred).head(10)` (fluent chain). The
        // `len` accessor returns Int (the row count); `agg` returns
        // DataFrame (one row per group, with key + aggregated value
        // columns). All methods panic-free at the codegen layer via
        // `.unwrap_or_default()` (DataFrame impls Default).
        //
        // `df.select(cols)` -> DataFrame. Projection (returns a new
        // DataFrame with only the named columns).
        (Type::DataFrame, PreludeInstanceFn::Select) => Some(Type::DataFrame),
        // `df.filter(pred)` -> DataFrame. Boolean-mask filter.
        (Type::DataFrame, PreludeInstanceFn::Filter) => Some(Type::DataFrame),
        // `df.sort(col)` -> DataFrame. Ascending lexicographic sort.
        (Type::DataFrame, PreludeInstanceFn::Sort) => Some(Type::DataFrame),
        // `df.head(n)` -> DataFrame. First n rows (clamped).
        (Type::DataFrame, PreludeInstanceFn::Head) => Some(Type::DataFrame),
        // `df.len()` -> Int. Row count. Shared `Len` variant
        // (mirrors `Format` shared between DateTime/Date/Time —
        // dispatched on receiver type).
        (Type::DataFrame, PreludeInstanceFn::Len) => Some(Type::int_default()),
        // `df.join(other, on)` -> DataFrame. Inner equi-join.
        // `Join` is shared with Strings.join + Path.join (existing
        // variant - re-used here, dispatched on DataFrame receiver).
        (Type::DataFrame, PreludeInstanceFn::Join) => Some(Type::DataFrame),
        // `df.group_by(col)` -> DataFrame. GroupBy state. The codegen
        // lowers this to `buff_dataframe::DataFrame::group_by(recv,
        // col).map(GroupBy::into_inner).unwrap_or_default()` so the
        // DataFrame receiver type is preserved for subsequent .agg()
        // chaining (a true GroupBy intermediate type would require a
        // second Type variant + display arm + codegen path; deferred
        // to v1.18+).
        (Type::DataFrame, PreludeInstanceFn::GroupBy) => Some(Type::DataFrame),
        // `df.agg(col, op)` -> DataFrame. Per-group aggregation (or
        // single-row aggregate if df is not a grouped DataFrame).
        (Type::DataFrame, PreludeInstanceFn::Agg) => Some(Type::DataFrame),

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
            //
            // T124h: `URL` is the second runtime-value-with-methods
            // type after Regex (T124d). Like Regex it's NOT a datetime,
            // so it skips the `is_prelude_datetime` check (its
            // `buff_type()` returns `Type::Url`, which round-trips
            // through `is_prelude_url()` instead).
            //
            // T124j: `Path` is the third runtime-value-with-methods
            // type after Regex (T124d) + URL (T124h). Like Regex +
            // URL it's NOT a datetime, so it skips the
            // `is_prelude_datetime` check (its `buff_type()` returns
            // `Type::Path`, which round-trips through
            // `is_prelude_path()` instead).
            //
            // T124l: `Process` is the fourth runtime-value-with-
            // methods type after Regex (T124d) + URL (T124h) + Path
            // (T124j). Like Regex + URL + Path it's NOT a datetime,
            // so it skips the `is_prelude_datetime` check (its
            // `buff_type()` returns `Type::Process`, which round-
            // trips through `is_prelude_process()` instead).
            if !t.is_namespace_only()
                && t != PreludeType::Regex
                && t != PreludeType::URL
                && t != PreludeType::Path
                && t != PreludeType::Process
            {
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
        // Random, Strings) shipped in T124f + 2 namespace-only system
        // modules (Args, Env) shipped in T124g + 4 namespace-only web
        // modules (Base64, Hex, URLEncode, UUID) + 1 runtime-value-
        // with-methods type (URL) shipped in T124h + 2 namespace-only
        // data-format modules (Yaml, Csv) shipped in T124i + 1 runtime-
        // value-with-methods type (Path) + 2 namespace-only modules
        // (Dir, Tempfile) shipped in T124j + 2 namespace-only crypto
        // modules (Hash, HMAC) shipped in T124k + 1 namespace-only
        // system-introspection module (OS) + 1 runtime-value-with-
        // methods type (Process) shipped in T124l + 3 namespace-only
        // networking modules (TCP, UDP, WebSocket) shipped in T124m
        // = 30 total prelude types.
        assert_eq!(PreludeType::ALL.len(), 30);
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
        // T124h: URL type + predicate. URL is NOT a datetime family
        // member and NOT a Regex - its dedicated `is_prelude_url`
        // predicate captures the parsed-URL runtime-value case.
        assert!(Type::url().is_prelude_url());
        assert!(!Type::url().is_prelude_datetime());
        assert!(!Type::url().is_prelude_regex());
        assert!(!Type::DateTime.is_prelude_url());
        assert!(!Type::Regex.is_prelude_url());
        assert!(!Type::string().is_prelude_url());
        // Cross-check via the prelude-type registry: `URL.buff_type()`
        // round-trips through `is_prelude_url` (the only prelude type
        // for which it does). The namespace-only web modules (Base64 /
        // Hex / URLEncode / UUID) do NOT round-trip (their `buff_type()`
        // returns `Type::Void`).
        assert!(PreludeType::URL.buff_type().is_prelude_url());
        assert!(!PreludeType::DateTime.buff_type().is_prelude_url());
        assert!(!PreludeType::Regex.buff_type().is_prelude_url());
        assert!(!PreludeType::Log.buff_type().is_prelude_url());
        assert!(!PreludeType::Base64.buff_type().is_prelude_url());
        assert!(!PreludeType::UUID.buff_type().is_prelude_url());
        // T124j: Path type + predicate. Path is NOT a datetime family
        // member, NOT a Regex, and NOT a URL - its dedicated
        // `is_prelude_path` predicate captures the filesystem-path
        // runtime-value case.
        assert!(Type::path().is_prelude_path());
        assert!(!Type::path().is_prelude_datetime());
        assert!(!Type::path().is_prelude_regex());
        assert!(!Type::path().is_prelude_url());
        assert!(!Type::DateTime.is_prelude_path());
        assert!(!Type::Regex.is_prelude_path());
        assert!(!Type::Url.is_prelude_path());
        assert!(!Type::string().is_prelude_path());
        // Cross-check via the prelude-type registry: `Path.buff_type()`
        // round-trips through `is_prelude_path` (the only prelude type
        // for which it does). The namespace-only Dir / Tempfile modules
        // do NOT round-trip (their `buff_type()` returns `Type::Void`).
        assert!(PreludeType::Path.buff_type().is_prelude_path());
        assert!(!PreludeType::DateTime.buff_type().is_prelude_path());
        assert!(!PreludeType::Regex.buff_type().is_prelude_path());
        assert!(!PreludeType::URL.buff_type().is_prelude_path());
        assert!(!PreludeType::Log.buff_type().is_prelude_path());
        assert!(!PreludeType::Dir.buff_type().is_prelude_path());
        assert!(!PreludeType::Tempfile.buff_type().is_prelude_path());
        // T124l: Process type + predicate. Process is NOT a datetime
        // family member, NOT a Regex, NOT a URL, and NOT a Path -
        // its dedicated `is_prelude_process` predicate captures
        // the spawned-process runtime-value case.
        assert!(Type::process().is_prelude_process());
        assert!(!Type::process().is_prelude_datetime());
        assert!(!Type::process().is_prelude_regex());
        assert!(!Type::process().is_prelude_url());
        assert!(!Type::process().is_prelude_path());
        assert!(!Type::DateTime.is_prelude_process());
        assert!(!Type::Regex.is_prelude_process());
        assert!(!Type::Url.is_prelude_process());
        assert!(!Type::Path.is_prelude_process());
        assert!(!Type::string().is_prelude_process());
        // Cross-check via the prelude-type registry: `Process
        // .buff_type()` round-trips through `is_prelude_process`
        // (the only prelude type for which it does). The
        // namespace-only OS module does NOT round-trip (its
        // `buff_type()` returns `Type::Void`).
        assert!(PreludeType::Process.buff_type().is_prelude_process());
        assert!(!PreludeType::DateTime.buff_type().is_prelude_process());
        assert!(!PreludeType::Regex.buff_type().is_prelude_process());
        assert!(!PreludeType::URL.buff_type().is_prelude_process());
        assert!(!PreludeType::Path.buff_type().is_prelude_process());
        assert!(!PreludeType::Log.buff_type().is_prelude_process());
        assert!(!PreludeType::OS.buff_type().is_prelude_process());
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
        // T124h: URL Display mirrors the Buff surface name (all-caps,
        // matches the `URL.parse(...)` user-facing spelling).
        assert_eq!(Type::Url.to_string(), "URL");
        // T124j: Path Display mirrors the Buff surface name.
        assert_eq!(Type::Path.to_string(), "Path");
        // T124l: Process Display mirrors the Buff surface name.
        assert_eq!(Type::Process.to_string(), "Process");
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
        // T124g: Args / Env are also namespace-only modules (mirror
        // Log / Toml / Math / Random / Strings). Both wrap `std::env`
        // and have NO runtime value representation.
        assert!(PreludeType::Args.is_namespace_only());
        assert!(PreludeType::Env.is_namespace_only());
        // T124h: Base64 / Hex / URLEncode / UUID are also namespace-only
        // modules (mirror Log / Toml / Math / Random / Strings / Args /
        // Env). All four wrap a Rust crate (base64 / hex /
        // percent-encoding / uuid) and have NO runtime value
        // representation (UUIDs surface as their canonical String form).
        assert!(PreludeType::Base64.is_namespace_only());
        assert!(PreludeType::Hex.is_namespace_only());
        assert!(PreludeType::URLEncode.is_namespace_only());
        assert!(PreludeType::UUID.is_namespace_only());
        // T124h: URL is NOT namespace-only - it's a real runtime value
        // type (mirrors Regex's runtime-value-with-rich-instance-methods
        // stance from T124d). Distinct from the namespace-only Base64 /
        // Hex / URLEncode / UUID modules it shipped alongside.
        assert!(!PreludeType::URL.is_namespace_only());
        // T124i: Yaml / Csv are also namespace-only modules (mirror
        // Log / Toml / Math / Random / Strings / Args / Env / Base64 /
        // Hex / URLEncode / UUID). Both wrap a Rust crate (serde_yml
        // / csv) and have NO runtime value representation.
        assert!(PreludeType::Yaml.is_namespace_only());
        assert!(PreludeType::Csv.is_namespace_only());
        // T124i: The count of namespace-only modules is now exactly 13
        // (Log + Toml + Math + Random + Strings + Args + Env + Base64 +
        // Hex + URLEncode + UUID + Yaml + Csv). URL is NOT in this count
        // (it's a runtime value type, not a namespace module).
        // T124j: Dir + Tempfile are also namespace-only modules
        // (mirror Yaml / Csv / Log / ...). Path is NOT in this
        // count (it's a runtime-value-with-instance-methods type,
        // mirrors Regex T124d + URL T124h). The count is now
        // exactly 15 (Log + Toml + Math + Random + Strings + Args +
        // Env + Base64 + Hex + URLEncode + UUID + Yaml + Csv + Dir
        // + Tempfile).
        assert!(PreludeType::Dir.is_namespace_only());
        assert!(PreludeType::Tempfile.is_namespace_only());
        // T124j: Path is NOT namespace-only - it's a real runtime
        // value type (mirrors Regex T124d + URL T124h's runtime-
        // value-with-rich-instance-methods stance). Distinct from
        // the namespace-only Dir / Tempfile modules it shipped
        // alongside.
        assert!(!PreludeType::Path.is_namespace_only());
        // T124k: Hash / HMAC are also namespace-only modules
        // (mirror Log / Toml / Math / Random / Strings / Args /
        // Env / Base64 / Hex / URLEncode / UUID / Yaml / Csv /
        // Dir / Tempfile). Both wrap a Rust crate (sha2 + md5 +
        // hmac) and have NO runtime value representation (every
        // call returns a hex String - the digest / MAC).
        assert!(PreludeType::Hash.is_namespace_only());
        assert!(PreludeType::HMAC.is_namespace_only());
        // T124l: OS is also a namespace-only module (mirror
        // Log / Toml / Math / Random / Strings / Args / Env /
        // Base64 / Hex / URLEncode / UUID / Yaml / Csv / Dir /
        // Tempfile / Hash / HMAC). It wraps `std::env::consts`
        // + env-var hostname + `num_cpus` and has NO runtime
        // value representation (every call returns a String /
        // Int - the OS name / arch / hostname / cpu count).
        assert!(PreludeType::OS.is_namespace_only());
        // T124l: Process is NOT namespace-only - it's a real
        // runtime value type (mirrors Regex T124d + URL T124h
        // + Path T124j's runtime-value-with-rich-instance-
        // methods stance). Distinct from the namespace-only OS
        // module it shipped alongside.
        assert!(!PreludeType::Process.is_namespace_only());
        // T124m: TCP / UDP / WebSocket are also namespace-only
        // modules (mirror Log / Toml / OS / Process's namespace
        // counterpart). The runtime-value types they construct
        // (Connection / Socket / WsConnection) are separate Type
        // variants (NOT namespace-only themselves - they're real
        // runtime values, like Regex / URL / Path / Process).
        assert!(PreludeType::TCP.is_namespace_only());
        assert!(PreludeType::UDP.is_namespace_only());
        assert!(PreludeType::WebSocket.is_namespace_only());
        let namespace_only_count = PreludeType::ALL
            .iter()
            .filter(|t| t.is_namespace_only())
            .count();
        // T124m: bumped from 18 to 21 (TCP + UDP + WebSocket
        // namespace-only; Connection / Socket / WsConnection are
        // NOT namespace-only themselves - they're runtime-value
        // types constructed by the assoc fns).
        assert_eq!(namespace_only_count, 21);
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
            assoc_fn_return_type(
                PreludeType::Math,
                PreludeAssocFn::Sqrt,
                &[Type::float_default()]
            ),
            expected
        );
        assert_eq!(
            assoc_fn_return_type(
                PreludeType::Math,
                PreludeAssocFn::Sin,
                &[Type::float_default()]
            ),
            expected
        );
        assert_eq!(
            assoc_fn_return_type(
                PreludeType::Math,
                PreludeAssocFn::Cos,
                &[Type::float_default()]
            ),
            expected
        );
        assert_eq!(
            assoc_fn_return_type(
                PreludeType::Math,
                PreludeAssocFn::Tan,
                &[Type::float_default()]
            ),
            expected
        );
        assert_eq!(
            assoc_fn_return_type(
                PreludeType::Math,
                PreludeAssocFn::Abs,
                &[Type::float_default()]
            ),
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
            assoc_fn_return_type(
                PreludeType::Strings,
                PreludeAssocFn::Trim,
                &[Type::string()]
            ),
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

    // T124i: Yaml module - `Yaml.parse(s)` / `Yaml.stringify(v)` assoc-fn
    // lookups + return types. Mirrors the Toml namespace-only precedent
    // (T124e) but for the `serde_yml` Rust crate (the maintained fork
    // of the deprecated `serde_yaml`).
    #[test]
    fn prelude_yaml_assoc_fn_lookup_valid_pairs() {
        // `Yaml.parse` reuses the registry's shared `Parse` variant
        // (also used by DateTime.parse / Date.parse / Toml.parse /
        // URL.parse / UUID.parse).
        assert_eq!(
            assoc_fn_lookup("Yaml", "parse"),
            Some((PreludeType::Yaml, PreludeAssocFn::Parse))
        );
        // `Yaml.stringify` reuses the registry's shared `Stringify`
        // variant (also used by Toml.stringify).
        assert_eq!(
            assoc_fn_lookup("Yaml", "stringify"),
            Some((PreludeType::Yaml, PreludeAssocFn::Stringify))
        );
        // `Yaml` is recognised as a prelude type.
        assert!(is_prelude_type("Yaml"));
        // `Yaml.buff_type()` is `Void` (no runtime value - namespace-only
        // like Toml / Log).
        assert_eq!(PreludeType::Yaml.buff_type(), Type::Void);
        // `Yaml.is_namespace_only()` is true.
        assert!(PreludeType::Yaml.is_namespace_only());
    }

    #[test]
    fn prelude_yaml_assoc_fn_lookup_rejects_invalid_pairs() {
        // Yaml.now is invalid (now belongs to DateTime/Instant).
        assert_eq!(assoc_fn_lookup("Yaml", "now"), None);
        // Yaml.compile is invalid (compile belongs to Regex).
        assert_eq!(assoc_fn_lookup("Yaml", "compile"), None);
        // Yaml.debug is invalid (debug belongs to Log).
        assert_eq!(assoc_fn_lookup("Yaml", "debug"), None);
        // Yaml.unknown is invalid.
        assert_eq!(assoc_fn_lookup("Yaml", "unknown"), None);
        // DateTime.stringify is invalid (stringify belongs to Toml/Yaml/Csv).
        assert_eq!(assoc_fn_lookup("DateTime", "stringify"), None);
        // Regex.stringify is invalid.
        assert_eq!(assoc_fn_lookup("Regex", "stringify"), None);
        // Log.stringify is invalid (Log is namespace-only with debug/info/...).
        assert_eq!(assoc_fn_lookup("Log", "stringify"), None);
    }

    #[test]
    fn prelude_yaml_assoc_fn_return_types() {
        // Yaml.parse(s) -> Map<String, Unknown>. The value type is
        // Unknown because YAML values are heterogeneous (scalars /
        // arrays / sub-mappings); the codegen turbofish-es to the
        // concrete `HashMap<String, serde_yml::Value>` at the Rust
        // level. Mirrors Toml.parse exactly.
        assert_eq!(
            assoc_fn_return_type(PreludeType::Yaml, PreludeAssocFn::Parse, &[Type::string()]),
            Some(Type::map(Type::string(), Type::Unknown))
        );
        // Yaml.stringify(v) -> String.
        assert_eq!(
            assoc_fn_return_type(
                PreludeType::Yaml,
                PreludeAssocFn::Stringify,
                &[Type::map(Type::string(), Type::Unknown)]
            ),
            Some(Type::string())
        );
        // Yaml + non-Yaml method is invalid.
        assert_eq!(
            assoc_fn_return_type(PreludeType::Yaml, PreludeAssocFn::Now, &[]),
            None
        );
        assert_eq!(
            assoc_fn_return_type(PreludeType::Yaml, PreludeAssocFn::Compile, &[]),
            None
        );
        assert_eq!(
            assoc_fn_return_type(PreludeType::Yaml, PreludeAssocFn::Debug, &[]),
            None
        );
        // Non-Yaml type + Yaml method is invalid (the (Type, Parse)
        // pair is validated by the registry matrix; Yaml lowers via
        // (Yaml, Parse), Toml via (Toml, Parse), but (Regex, Stringify)
        // is invalid - Stringify is shared by Toml/Yaml/Csv ONLY).
        assert_eq!(
            assoc_fn_return_type(PreludeType::Regex, PreludeAssocFn::Stringify, &[]),
            None
        );
        assert_eq!(
            assoc_fn_return_type(PreludeType::Log, PreludeAssocFn::Stringify, &[]),
            None
        );
    }

    // T124i: Csv module - `Csv.parse(s)` / `Csv.stringify(rows)` assoc-fn
    // lookups + return types. Mirrors the Yaml / Toml namespace-only
    // precedent but with `Vector<Vector<String>>` instead of Map.
    #[test]
    fn prelude_csv_assoc_fn_lookup_valid_pairs() {
        // `Csv.parse` reuses the registry's shared `Parse` variant.
        assert_eq!(
            assoc_fn_lookup("Csv", "parse"),
            Some((PreludeType::Csv, PreludeAssocFn::Parse))
        );
        // `Csv.stringify` reuses the registry's shared `Stringify` variant.
        assert_eq!(
            assoc_fn_lookup("Csv", "stringify"),
            Some((PreludeType::Csv, PreludeAssocFn::Stringify))
        );
        // `Csv` is recognised as a prelude type.
        assert!(is_prelude_type("Csv"));
        // `Csv.buff_type()` is `Void` (no runtime value - namespace-only).
        assert_eq!(PreludeType::Csv.buff_type(), Type::Void);
        // `Csv.is_namespace_only()` is true.
        assert!(PreludeType::Csv.is_namespace_only());
    }

    #[test]
    fn prelude_csv_assoc_fn_lookup_rejects_invalid_pairs() {
        // Csv.now is invalid.
        assert_eq!(assoc_fn_lookup("Csv", "now"), None);
        // Csv.compile is invalid.
        assert_eq!(assoc_fn_lookup("Csv", "compile"), None);
        // Csv.debug is invalid.
        assert_eq!(assoc_fn_lookup("Csv", "debug"), None);
        // Csv.unknown is invalid.
        assert_eq!(assoc_fn_lookup("Csv", "unknown"), None);
        // Csv.split is invalid (split belongs to Strings).
        assert_eq!(assoc_fn_lookup("Csv", "split"), None);
        // Yaml now/compile invalid (mirror Yaml rejects).
        assert_eq!(assoc_fn_lookup("Yaml", "now"), None);
    }

    #[test]
    fn prelude_csv_assoc_fn_return_types() {
        // Csv.parse(s) -> Vector<Vector<String>>. Uniform rows (NO
        // header special-casing per the spec - every row including
        // the header is a Vector<String>). Cells are String (CSV has
        // no inherent type information, every cell is text).
        assert_eq!(
            assoc_fn_return_type(PreludeType::Csv, PreludeAssocFn::Parse, &[Type::string()]),
            Some(Type::vector(Type::vector(Type::string())))
        );
        // Csv.stringify(rows) -> String. The arg is a
        // Vector<Vector<String>>; the codegen builds a csv::Writer
        // and serializes.
        assert_eq!(
            assoc_fn_return_type(
                PreludeType::Csv,
                PreludeAssocFn::Stringify,
                &[Type::vector(Type::vector(Type::string()))]
            ),
            Some(Type::string())
        );
        // Csv + non-Csv method is invalid.
        assert_eq!(
            assoc_fn_return_type(PreludeType::Csv, PreludeAssocFn::Now, &[]),
            None
        );
        assert_eq!(
            assoc_fn_return_type(PreludeType::Csv, PreludeAssocFn::Compile, &[]),
            None
        );
        assert_eq!(
            assoc_fn_return_type(PreludeType::Csv, PreludeAssocFn::Debug, &[]),
            None
        );
        // Non-Csv type + Csv method: a hypothetical `(Toml, Parse)`
        // pair is valid (Toml reuses Parse), but (Regex, Stringify)
        // is invalid (Stringify is shared by Toml/Yaml/Csv only).
        assert_eq!(
            assoc_fn_return_type(PreludeType::Regex, PreludeAssocFn::Stringify, &[]),
            None
        );
    }

    // T124j: Path module - `Path.join(a, b, ...)` assoc-fn lookup +
    // return type + the four instance methods. Mirrors the URL
    // runtime-value-with-methods precedent (T124h) - Path is the
    // third such type (after Regex T124d + URL T124h).
    #[test]
    fn prelude_path_assoc_fn_lookup_valid_pairs() {
        // `Path.join` reuses the registry's shared `Join` variant
        // (also used by Strings.join from T124f). Same name,
        // different per-type semantics dispatched on the (Path,
        // Join) pair.
        assert_eq!(
            assoc_fn_lookup("Path", "join"),
            Some((PreludeType::Path, PreludeAssocFn::Join))
        );
        // `Path` is recognised as a prelude type.
        assert!(is_prelude_type("Path"));
        // `Path.buff_type()` is `Path` (a real runtime value, NOT
        // Void - mirrors Regex / URL).
        assert_eq!(PreludeType::Path.buff_type(), Type::Path);
        // `Path.is_namespace_only()` is false (it IS a runtime value).
        assert!(!PreludeType::Path.is_namespace_only());
        // The other prelude types are NOT Path (round-trip via
        // buff_type).
        assert!(!PreludeType::DateTime.buff_type().is_prelude_path());
        assert!(!PreludeType::Regex.buff_type().is_prelude_path());
        assert!(!PreludeType::URL.buff_type().is_prelude_path());
        assert!(PreludeType::Path.buff_type().is_prelude_path());
    }

    #[test]
    fn prelude_path_assoc_fn_lookup_rejects_invalid_pairs() {
        // Path.now is invalid (now belongs to DateTime/Instant).
        assert_eq!(assoc_fn_lookup("Path", "now"), None);
        // Path.compile is invalid (compile belongs to Regex).
        assert_eq!(assoc_fn_lookup("Path", "compile"), None);
        // Path.parse is invalid (Path has join, not parse).
        assert_eq!(assoc_fn_lookup("Path", "parse"), None);
        // Path.unknown is invalid.
        assert_eq!(assoc_fn_lookup("Path", "unknown"), None);
        // Path.list is invalid (list belongs to Args/Dir).
        assert_eq!(assoc_fn_lookup("Path", "list"), None);
        // DateTime.join is invalid (join belongs to Strings/Path).
        assert_eq!(assoc_fn_lookup("DateTime", "join"), None);
        // Strings.join IS valid (reuses Join) - confirms Path.join
        // is also valid via the same overload-by-type pattern.
        assert_eq!(
            assoc_fn_lookup("Strings", "join"),
            Some((PreludeType::Strings, PreludeAssocFn::Join))
        );
    }

    #[test]
    fn prelude_path_assoc_fn_return_type() {
        // Path.join(a, b, ...) -> Path.
        assert_eq!(
            assoc_fn_return_type(
                PreludeType::Path,
                PreludeAssocFn::Join,
                &[Type::string(), Type::string()]
            ),
            Some(Type::Path)
        );
        // Single-arg join is also valid (returns PathBuf of the arg).
        assert_eq!(
            assoc_fn_return_type(PreludeType::Path, PreludeAssocFn::Join, &[Type::string()]),
            Some(Type::Path)
        );
        // Path + non-Path method is invalid.
        assert_eq!(
            assoc_fn_return_type(PreludeType::Path, PreludeAssocFn::Now, &[]),
            None
        );
        assert_eq!(
            assoc_fn_return_type(PreludeType::Path, PreludeAssocFn::Compile, &[]),
            None
        );
        // Non-Path type + Path method: a hypothetical `(Strings, Join)`
        // pair is valid (Strings reuses Join), but (Log, Join) is
        // invalid (Log is namespace-only with debug/info/...).
        assert_eq!(
            assoc_fn_return_type(PreludeType::Log, PreludeAssocFn::Join, &[]),
            None
        );
    }

    #[test]
    fn prelude_path_instance_fn_lookup_valid_pairs() {
        // All four Path instance methods resolve via the registry
        // when the receiver is `Type::Path`.
        assert_eq!(
            instance_fn_lookup(&Type::Path, "parent"),
            Some(PreludeInstanceFn::Parent)
        );
        assert_eq!(
            instance_fn_lookup(&Type::Path, "extension"),
            Some(PreludeInstanceFn::Extension)
        );
        assert_eq!(
            instance_fn_lookup(&Type::Path, "basename"),
            Some(PreludeInstanceFn::Basename)
        );
        assert_eq!(
            instance_fn_lookup(&Type::Path, "exists"),
            Some(PreludeInstanceFn::Exists)
        );
    }

    #[test]
    fn prelude_path_instance_fn_lookup_rejects_invalid_pairs() {
        // Path.format is invalid (format belongs to DateTime/Date/Time).
        assert_eq!(instance_fn_lookup(&Type::Path, "format"), None);
        // Path.year is invalid.
        assert_eq!(instance_fn_lookup(&Type::Path, "year"), None);
        // Path.unknown is invalid.
        assert_eq!(instance_fn_lookup(&Type::Path, "unknown"), None);
        // Path.parent is invalid when the receiver is NOT Path.
        assert_eq!(instance_fn_lookup(&Type::DateTime, "parent"), None);
        assert_eq!(instance_fn_lookup(&Type::String, "exists"), None);
    }

    #[test]
    fn prelude_path_instance_fn_return_types() {
        // path.parent() -> Option<Path>.
        assert_eq!(
            instance_fn_return_type(&Type::Path, PreludeInstanceFn::Parent, &[]),
            Some(Type::option(Type::Path))
        );
        // path.extension() -> Option<String>.
        assert_eq!(
            instance_fn_return_type(&Type::Path, PreludeInstanceFn::Extension, &[]),
            Some(Type::option(Type::string()))
        );
        // path.basename() -> String.
        assert_eq!(
            instance_fn_return_type(&Type::Path, PreludeInstanceFn::Basename, &[]),
            Some(Type::string())
        );
        // path.exists() -> Bool.
        assert_eq!(
            instance_fn_return_type(&Type::Path, PreludeInstanceFn::Exists, &[]),
            Some(Type::bool())
        );
        // Non-Path receiver + Path method is invalid.
        assert_eq!(
            instance_fn_return_type(&Type::DateTime, PreludeInstanceFn::Parent, &[]),
            None
        );
        // Path receiver + non-Path method is invalid.
        assert_eq!(
            instance_fn_return_type(&Type::Path, PreludeInstanceFn::Format, &[Type::string()]),
            None
        );
    }

    // T124j: Dir module - `Dir.list/create/remove/walk` assoc-fn
    // lookups + return types. Mirrors the Yaml / Csv namespace-only
    // precedent (T124i) but for filesystem operations.
    #[test]
    fn prelude_dir_assoc_fn_lookup_valid_pairs() {
        // `Dir.list` reuses the registry's shared `List` variant
        // (also used by Args.list from T124g).
        assert_eq!(
            assoc_fn_lookup("Dir", "list"),
            Some((PreludeType::Dir, PreludeAssocFn::List))
        );
        // `Dir.create` is the new shared Create variant (shared
        // with Tempfile.create).
        assert_eq!(
            assoc_fn_lookup("Dir", "create"),
            Some((PreludeType::Dir, PreludeAssocFn::Create))
        );
        // `Dir.remove` is the new Dir-only Remove variant.
        assert_eq!(
            assoc_fn_lookup("Dir", "remove"),
            Some((PreludeType::Dir, PreludeAssocFn::Remove))
        );
        // `Dir.walk` is the new Dir-only Walk variant.
        assert_eq!(
            assoc_fn_lookup("Dir", "walk"),
            Some((PreludeType::Dir, PreludeAssocFn::Walk))
        );
        // `Dir` is recognised as a prelude type.
        assert!(is_prelude_type("Dir"));
        // `Dir.buff_type()` is `Void` (no runtime value - namespace-only
        // like Log / Toml / Yaml / Csv).
        assert_eq!(PreludeType::Dir.buff_type(), Type::Void);
        // `Dir.is_namespace_only()` is true.
        assert!(PreludeType::Dir.is_namespace_only());
    }

    #[test]
    fn prelude_dir_assoc_fn_lookup_rejects_invalid_pairs() {
        // Dir.now is invalid (now belongs to DateTime/Instant).
        assert_eq!(assoc_fn_lookup("Dir", "now"), None);
        // Dir.compile is invalid (compile belongs to Regex).
        assert_eq!(assoc_fn_lookup("Dir", "compile"), None);
        // Dir.parse is invalid (Dir has list/create/remove/walk, not parse).
        assert_eq!(assoc_fn_lookup("Dir", "parse"), None);
        // Dir.unknown is invalid.
        assert_eq!(assoc_fn_lookup("Dir", "unknown"), None);
        // Dir.join is invalid (join belongs to Strings/Path).
        assert_eq!(assoc_fn_lookup("Dir", "join"), None);
        // Dir.encode is invalid (encode belongs to Base64/Hex/URLEncode).
        assert_eq!(assoc_fn_lookup("Dir", "encode"), None);
        // DateTime.walk is invalid (walk belongs to Dir).
        assert_eq!(assoc_fn_lookup("DateTime", "walk"), None);
        // Regex.remove is invalid (remove belongs to Dir).
        assert_eq!(assoc_fn_lookup("Regex", "remove"), None);
    }

    #[test]
    fn prelude_dir_assoc_fn_return_types() {
        // Dir.list(path) -> Vector<String>.
        assert_eq!(
            assoc_fn_return_type(PreludeType::Dir, PreludeAssocFn::List, &[Type::Path]),
            Some(Type::vector(Type::string()))
        );
        // Dir.create(path) -> Void.
        assert_eq!(
            assoc_fn_return_type(PreludeType::Dir, PreludeAssocFn::Create, &[Type::Path]),
            Some(Type::Void)
        );
        // Dir.remove(path) -> Void.
        assert_eq!(
            assoc_fn_return_type(PreludeType::Dir, PreludeAssocFn::Remove, &[Type::Path]),
            Some(Type::Void)
        );
        // Dir.walk(path) -> Vector<Path>.
        assert_eq!(
            assoc_fn_return_type(PreludeType::Dir, PreludeAssocFn::Walk, &[Type::Path]),
            Some(Type::vector(Type::Path))
        );
        // Dir + non-Dir method is invalid.
        assert_eq!(
            assoc_fn_return_type(PreludeType::Dir, PreludeAssocFn::Now, &[]),
            None
        );
        assert_eq!(
            assoc_fn_return_type(PreludeType::Dir, PreludeAssocFn::Compile, &[]),
            None
        );
        // Non-Dir type + Dir method is invalid (the (Type, List) pair
        // is shared by Args/Dir but (DateTime, Walk) is invalid).
        assert_eq!(
            assoc_fn_return_type(PreludeType::DateTime, PreludeAssocFn::Walk, &[]),
            None
        );
        assert_eq!(
            assoc_fn_return_type(PreludeType::Regex, PreludeAssocFn::Remove, &[]),
            None
        );
    }

    // T124j: Tempfile module - `Tempfile.create/dir` assoc-fn
    // lookups + return types. Mirrors the Dir namespace-only precedent.
    #[test]
    fn prelude_tempfile_assoc_fn_lookup_valid_pairs() {
        // `Tempfile.create` reuses the new shared Create variant
        // (also used by Dir.create).
        assert_eq!(
            assoc_fn_lookup("Tempfile", "create"),
            Some((PreludeType::Tempfile, PreludeAssocFn::Create))
        );
        // `Tempfile.dir` is the new Tempfile-only Dir variant.
        assert_eq!(
            assoc_fn_lookup("Tempfile", "dir"),
            Some((PreludeType::Tempfile, PreludeAssocFn::Dir))
        );
        // `Tempfile` is recognised as a prelude type.
        assert!(is_prelude_type("Tempfile"));
        // `Tempfile.buff_type()` is `Void` (no runtime value -
        // namespace-only like Dir / Log / Toml / Yaml / Csv).
        assert_eq!(PreludeType::Tempfile.buff_type(), Type::Void);
        // `Tempfile.is_namespace_only()` is true.
        assert!(PreludeType::Tempfile.is_namespace_only());
    }

    #[test]
    fn prelude_tempfile_assoc_fn_lookup_rejects_invalid_pairs() {
        // Tempfile.now is invalid (now belongs to DateTime/Instant).
        assert_eq!(assoc_fn_lookup("Tempfile", "now"), None);
        // Tempfile.compile is invalid (compile belongs to Regex).
        assert_eq!(assoc_fn_lookup("Tempfile", "compile"), None);
        // Tempfile.parse is invalid (Tempfile has create/dir, not parse).
        assert_eq!(assoc_fn_lookup("Tempfile", "parse"), None);
        // Tempfile.unknown is invalid.
        assert_eq!(assoc_fn_lookup("Tempfile", "unknown"), None);
        // Tempfile.list is invalid (list belongs to Args/Dir).
        assert_eq!(assoc_fn_lookup("Tempfile", "list"), None);
        // Tempfile.walk is invalid (walk belongs to Dir).
        assert_eq!(assoc_fn_lookup("Tempfile", "walk"), None);
        // DateTime.dir is invalid (dir belongs to Tempfile).
        assert_eq!(assoc_fn_lookup("DateTime", "dir"), None);
        // Dir.dir is invalid (Dir has list/create/remove/walk, not dir).
        assert_eq!(assoc_fn_lookup("Dir", "dir"), None);
    }

    #[test]
    fn prelude_tempfile_assoc_fn_return_types() {
        // Tempfile.create() -> Path.
        assert_eq!(
            assoc_fn_return_type(PreludeType::Tempfile, PreludeAssocFn::Create, &[]),
            Some(Type::Path)
        );
        // Tempfile.dir() -> Path.
        assert_eq!(
            assoc_fn_return_type(PreludeType::Tempfile, PreludeAssocFn::Dir, &[]),
            Some(Type::Path)
        );
        // Tempfile + non-Tempfile method is invalid.
        assert_eq!(
            assoc_fn_return_type(PreludeType::Tempfile, PreludeAssocFn::Now, &[]),
            None
        );
        assert_eq!(
            assoc_fn_return_type(PreludeType::Tempfile, PreludeAssocFn::Compile, &[]),
            None
        );
        // Non-Tempfile type + Tempfile method: (Dir, Create) is valid
        // (Dir reuses Create), but (Regex, Dir) is invalid (Dir method
        // is Tempfile-only).
        assert_eq!(
            assoc_fn_return_type(PreludeType::Regex, PreludeAssocFn::Dir, &[]),
            None
        );
        assert_eq!(
            assoc_fn_return_type(PreludeType::Log, PreludeAssocFn::Dir, &[]),
            None
        );
    }
}

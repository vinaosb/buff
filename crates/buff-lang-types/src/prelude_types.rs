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
    /// T37: `Faker` — a fake-data generator wrapping `buff_fake::Faker`.
    /// Constructed via the associated functions `Faker.new()` (default
    /// locale, random seed), `Faker.with_locale(locale)` (en-US or
    /// pt-BR), `Faker.with_seed(locale, seed)` (reproducible output);
    /// supports 8 instance methods: `faker.name()`, `faker.email()`,
    /// `faker.address()`, `faker.phone()`, `faker.uuid()`,
    /// `faker.lorem(words)`, `faker.int(min, max)`,
    /// `faker.datetime(start, end)`.
    ///
    /// This is a runtime-value-with-rich-instance-methods type
    /// (mirroring Image T9 / Regex T124d). `buff_type()` returns
    /// [`Type::Faker`] (a real value type); `is_namespace_only()`
    /// returns `false`. The `fake` crate is recorded in codegen
    /// `extern_crates` when a Buff program uses `Faker` (mirrors
    /// the chrono / regex / tracing codegen-only linking boundary).
    /// Pure-Rust, no native deps.
    Faker,
    /// T31: `Cache` — in-memory cache runtime-value type wrapping
    /// `buff_cache::Cache` (itself wrapping the `moka` sync cache).
    /// Constructed via the associated function `Cache.new(max_capacity)`;
    /// supports 7 instance methods: `cache.get(key) -> String?`,
    /// `cache.set(key, value)`, `cache.set(key, value, ttl)`,
    /// `cache.delete(key)`, `cache.contains(key) -> Bool`,
    /// `cache.clear()`, `cache.len() -> Int`. LRU eviction +
    /// per-entry TTL via stored `Option<Instant>` deadlines
    /// (lazy eviction on get/contains).
    ///
    /// This is the SEVENTH runtime-value-with-rich-instance-methods
    /// type (after Regex T124d / URL T124h / Path T124j / Process
    /// T124l / Image T9 / DataFrame T7). `buff_type()` returns
    /// [`Type::Cache`] (a real value type); `is_namespace_only()`
    /// returns `false`. The `moka` crate is recorded in codegen
    /// `extern_crates` when a Buff program uses `Cache` (mirrors the
    /// chrono / regex / tracing codegen-only linking boundary).
    /// Distributed Redis backend DEFERRED to v1.18+ per the T31
    /// task spec ("If problematic, defer distributed to v1.18+ and
    /// ship in-memory MVP only").
    Cache,
    /// `I18n` - the internationalization runtime-value type (T44).
    /// EXPERIMENTAL badge per T44 spec. Wraps the in-tree pure-Rust
    /// `buff-i18n` crate backed by Mozilla's `fluent-bundle` +
    /// `unic-langid` (BCP 47). Per T44 spec: NO machine translation,
    /// NO RTL layout helpers (UI concern). `I18n.new(locale)` /
    /// `I18n.with_fallback(locale, fallback)` ctors + MVP instance
    /// methods `i18n.add_resource(locale, ftl)` / `i18n.load(locale)`
    /// / `i18n.translate(key)`. The other 7 instance methods
    /// (SetFallback / AvailableLocales / CurrentLocale /
    /// FallbackLocale / TranslateWithArgs / HasMessage / Warnings)
    /// are available on the Rust type but codegen-wiring is deferred
    /// to a follow-up to keep the shared-zone footprint minimal.
    /// `is_namespace_only()` returns `false` (this is the Nth
    /// runtime-value-with-rich-instance-methods type after Regex /
    /// URL / Path / Process / Image / DataFrame / Audio / Faker /
    /// Cache). `buff_type()` returns [`Type::I18n`]. Records
    /// `buff-i18n` + `fluent-bundle` + `unic-langid` in codegen
    /// `extern_crates` when a Buff program uses `I18n.*` (mirrors
    /// the chrono / regex / tracing / image codegen-only linking
    /// boundary). Pure-Rust only — no cc-rs, no native deps.
    I18n,
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
    /// is a `(u32 id, u32 generation)` pair — Copy, Eq, Hash, Send,
    /// Sync, 'static). Users compare entities by value, store them
    /// in collections, and pass them back to the world's mutating
    /// methods (`insert`, `remove`, `despawn`).
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
    /// T26: `Audit` — the security-scanning namespace (v1.13 frameworks
    /// wave 3). Wraps the in-tree pure-Rust `buff-audit` crate
    /// (`buff_audit::scan` / `buff_audit::scan_with_detail` /
    /// `buff_audit::known_advisories`). Two assoc fns:
    /// - `Audit.scan(path)` -> `Vector<String>` — the advisory IDs that
    ///   fired against the project's `buff.lock` / `Cargo.lock` /
    ///   `buff.toml` (priority chain `MANIFEST_PATHS`). Returns an
    ///   empty `Vector<String>` if no manifest is found OR no advisory
    ///   matches.
    /// - `Audit.list()` -> `Vector<String>` — every advisory ID in the
    ///   statically-seeded DB (regardless of whether the project
    ///   triggers it). Useful for `buff audit list` tooling.
    ///
    /// `Audit` is **never a runtime value** — it's a NAMESPACE exposing
    /// only associated functions (mirrors Log / Toml / Hash / HMAC / OS
    /// exactly). `buff_type()` returns [`Type::Void`];
    /// `is_namespace_only()` returns `true`. The `buff-audit` crate is
    /// recorded in codegen `extern_crates` when a Buff program uses
    /// `Audit.*` (mirrors the chrono / regex / sha2 codegen-only
    /// linking boundary).
    ///
    /// FFI-safe: the wrapper complies with all 6 rules from
    /// `crates/buff-lang-ffi-guide/GUIDE.md` (no raw pointers, owned
    /// `Vec<String>` at the boundary, `catch_unwind` on every entry
    /// point, Send + 'static, no lifetimes). CPU-only per Metis G7
    /// lock (NO GPU dispatch — auditing is inherently sequential I/O).
    Audit,
    /// T26: `Signature` — the Ed25519 code-signing namespace (v1.13
    /// frameworks wave 3). Wraps the in-tree pure-Rust `buff-audit`
    /// crate (`buff_audit::keypair` / `buff_audit::sign` /
    /// `buff_audit::verify`). Three assoc fns:
    /// - `Signature.keypair()` -> `(String, String)` — fresh CSPRNG
    ///   Ed25519 keypair. Returns `(public_hex, secret_hex)`, each
    ///   64-char lowercase hex.
    /// - `Signature.sign(data, secret_hex)` -> `String` — detached
    ///   64-byte signature (128-char hex). Deterministic per RFC 8032.
    /// - `Signature.verify(data, sig_hex, public_hex)` -> `Bool` —
    ///   strict Ed25519 verify. Returns `false` on bad signature
    ///   (NEVER panics, NEVER errors — the T26 task spec mandates the
    ///   bool return so a future `buff add --no-verify` bypass layers
    ///   cleanly).
    ///
    /// `Signature` is **never a runtime value** — it's a NAMESPACE
    /// exposing only associated functions (mirrors Log / Toml / Hash /
    /// HMAC / OS / Audit exactly). `buff_type()` returns [`Type::Void`];
    /// `is_namespace_only()` returns `true`. The `buff-audit` +
    /// `ed25519-dalek` + `hex` + `rand` crates are recorded in codegen
    /// `extern_crates` when a Buff program uses `Signature.*`.
    ///
    /// NO `ring`, NO native-tls, NO cc-rs — the T26 task spec
    /// explicitly forbids all three. ed25519-dalek 2.0 is the canonical
    /// pure-Rust Ed25519 (matches the "no C library, no Docker" hard
    /// rule from T126/T127 and the "Windows host with no MSVC"
    /// constraint that pushed hand-rolled lexer/parser).
    Signature,
    /// T20 (v1.13 frameworks wave 3): the `ReactiveSignal` namespace —
    /// the Solid.js / Vue-inspired callback-based reactive primitive.
    /// Wraps the in-tree pure-Rust `buff-reactive` crate
    /// (`buff_reactive::Signal<T>`). One assoc fn:
    /// - `ReactiveSignal.new(value) -> Signal<T>` — mutable reactive
    ///   cell; the codegen emits `buff_reactive::Signal::new(value)`
    ///   and Rust infers `T` from the value.
    ///
    /// `ReactiveSignal` is a NAMESPACE (mirrors Channel / Audit /
    /// Signature). `buff_type()` returns [`Type::Void`];
    /// `is_namespace_only()` returns `true`. The return value of
    /// `ReactiveSignal.new(...)` is typed [`Type::Unknown`] — the
    /// coordinated `Type::ReactiveSignal` variant in `ty.rs` is a
    /// follow-up sibling task OUTSIDE the T20 shared zone (mirrors
    /// the T8 Tensor / T11 Signal-DSP forward-declaration precedent).
    ///
    /// Records `buff-reactive` in codegen `extern_crates` when a Buff
    /// program uses `ReactiveSignal.*`. EXPERIMENTAL badge per T20
    /// spec — single-threaded `Rc<RefCell>` MVP only;
    /// multi-threaded signals + `Stream<T>` integration are deferred
    /// to v1.18+.
    ReactiveSignal,
    /// T20: the `ReactiveComputed` namespace — lazy derived value
    /// primitive. Wraps `buff_reactive::Computed<T>`. One assoc fn:
    /// - `ReactiveComputed.new(fn) -> Computed<T>` — derives from
    ///   signals, recomputes lazily, caches. The codegen emits
    ///   `buff_reactive::Computed::new(fn)`.
    ///
    /// Mirrors [`PreludeType::ReactiveSignal`] exactly: namespace-
    /// only, returns [`Type::Unknown`] (forward-declaration),
    /// instance method `c.get()` dispatches on the shared
    /// `(Type::Unknown, Get)` arm.
    ReactiveComputed,
    /// T20: the `ReactiveEffect` namespace — side-effectful callback
    /// primitive. Wraps `buff_reactive::Effect`. One assoc fn:
    /// - `ReactiveEffect.new(fn) -> Effect` — runs `fn` immediately
    ///   and re-runs when dependencies change. The codegen emits
    ///   `buff_reactive::Effect::new(fn)`.
    ///
    /// Mirrors [`PreludeType::ReactiveSignal`] exactly: namespace-
    /// only, returns [`Type::Unknown`] (forward-declaration),
    /// instance method `e.run()` dispatches on the
    /// `(Type::Unknown, Run)` arm.
    ReactiveEffect,
    /// T17 (v1.15 frameworks wave 3): the `Web` runtime-value type —
    /// a production HTTP web framework wrapping axum 0.8 + tokio +
    /// serde_json via a safe FFI boundary per the T4 FFI guide.
    /// Constructed via the prelude associated functions
    /// `Web.new()` (empty server) or `Web.bind(addr)` (empty server
    /// with a preset bind address); carries 8 instance methods:
    /// `web.get(path, handler)` / `web.post(path, handler)` /
    /// `web.put(path, handler)` / `web.delete(path, handler)` /
    /// `web.patch(path, handler)` / `web.middleware(mw)` /
    /// `web.listen(port: N)` / `web.run()`. Built-in middleware
    /// (Logger / Cors / JsonParser) is deferred to v1.18+; the MVP
    /// ships the MiddlewareFn type signature + the registration API
    /// only.
    ///
    /// This is the latest runtime-value-with-rich-instance-methods
    /// type (after Regex / URL / Path / Process / Image / DataFrame /
    /// Audio / World). `buff_type()` returns [`Type::Unknown`] for
    /// MVP — the coordinated [`Type::Web`] variant in `ty.rs` is a
    /// follow-up sibling task OUTSIDE the T17 shared zone (mirrors
    /// the T8 Tensor / T11 Signal / T12-Tensor forward-declaration
    /// precedent). `is_namespace_only()` returns `false` (Web IS a
    /// runtime value, like Regex / Image / World). The codegen
    /// lowering for the assoc fns `Web.new` / `Web.bind` is shipped
    /// in this T17 commit (dispatch on PreludeType, NOT Type, so no
    /// Type::Web variant needed); the instance-method lowering
    /// (`web.get` / `web.listen` / etc.) is deferred to the
    /// coordinated sibling task that adds [`Type::Web`].
    ///
    /// Records `buff-web` + `axum` + `tokio` + `serde_json` in
    /// codegen `extern_crates` when a Buff program uses `Web.*`
    /// (mirrors the chrono / regex / tracing codegen-only linking
    /// boundary). EXPERIMENTAL badge per T17 spec — surface may
    /// evolve before v1.18 stabilisation (path-param extraction,
    /// built-in middleware, exotic HTTP verbs are explicitly
    /// deferred). FFI-safe: the wrapper complies with all 6 rules
    /// from `crates/buff-lang-ffi-guide/GUIDE.md` (no raw pointers,
    /// owned Request / Response at the boundary, infallible surface
    /// for route registration + fallible listen / run returning
    /// `Result<T, WebError>`, Send + Sync via Arc<dyn Fn ... + Send
    /// + Sync>, no lifetimes, every public body catch_unwind-wrapped
    /// per FFI guide R6).
    Web,
    /// T18 (v1.15 frameworks wave 3): the `Database` runtime-value
    /// type — a database access MVP wrapping the pure-Rust `sqlx`
    /// crate (SQLite + PostgreSQL drivers via the
    /// `runtime-tokio-rustls` feature — NOT native-tls per
    /// workspace hard rule from AGENTS.md "Pure-Rust preference").
    /// Constructed via the prelude associated function
    /// `Database.connect(url)` which returns the runtime
    /// `buff_db::Pool` value (a connection pool wrapping
    /// `sqlx::any::AnyPool`).
    ///
    /// Intended Buff surface (per T18 spec lines 2333-2337):
    /// - `Database.connect(url) -> Pool` — assoc fn ctor (shipped
    ///   here, in this T18 commit).
    /// - `pool.query(sql, params) -> Vector<Row>` — instance method
    ///   (DEFERRED to a sibling task that adds the coordinated
    ///   `Type::Pool` variant in `ty.rs` OUTSIDE the T18 shared zone
    ///   per the MUST NOT in the T18 task brief — mirrors the T17
    ///   Web / T20 ReactiveSignal forward-declaration precedent).
    /// - `pool.execute(sql, params) -> Int` — instance method (DEFERRED).
    /// - `pool.begin() -> Transaction` — instance method (DEFERRED).
    /// - `tx.commit() / tx.rollback() -> Void` — instance methods (DEFERRED).
    ///
    /// `buff_type()` returns [`Type::Unknown`] for MVP — the
    /// coordinated [`Type::Pool`] variant in `ty.rs` is a follow-up
    /// sibling task OUTSIDE the T18 shared zone (mirrors the T8
    /// Tensor / T11 Signal forward-declaration precedent).
    /// `is_namespace_only()` returns `false` (Database IS a runtime
    /// value, like Regex / Image / World).
    ///
    /// Records `buff-db` + `sqlx` + `tokio` in codegen
    /// `extern_crates` when a Buff program uses `Database.*`.
    /// EXPERIMENTAL badge per T18 spec — surface may evolve before
    /// v1.18 stabilisation (migrations + compile-time SQL validation
    /// + MySQL/MSSQL/Oracle drivers are explicitly deferred per T18
    /// must-not #3/#4/#5).
    ///
    /// FFI-safe: complies with all 6 rules from
    /// `crates/buff-lang-ffi-guide/GUIDE.md` (no raw pointers; owned
    /// String / Vec<Row> / Pool at the boundary; fallible ops return
    /// `Result<T, DbError>`; `Pool` is `Clone + Send + Sync`; no
    /// lifetimes on Pool / Row; no panic sites in non-test code).
    ///
    /// NO `diesel`, NO `libpq`, NO native-tls, NO cc-rs — matches
    /// the T18 task spec mandate and the "no C library, no Docker"
    /// hard rule from T126/T127.
    Database,
    /// T33 (v1.13 frameworks wave 5): the `HttpClient` runtime-value
    /// type — an idiomatic HTTP client wrapping the `reqwest` crate
    /// (already pinned at the workspace level for T127) via a safe
    /// FFI boundary per the T4 FFI guide. Constructed via the
    /// prelude associated function `HttpClient.new()` (returns a
    /// new client with default settings); carries the instance
    /// methods `client.get(url)`, `client.post(url)`,
    /// `client.put(url)`, `client.delete(url)` — each returning a
    /// `RequestBuilder` (opaque, typed `Type::Unknown` for MVP).
    /// The `RequestBuilder` carries `.header(name, val)`,
    /// `.json(body)`, `.timeout(secs)`, `.send()` (returns
    /// `Response`, also opaque for MVP). The `Response` carries
    /// `.status()`, `.text()`, `.json()`, `.bytes()`, `.headers()`.
    ///
    /// `buff_type()` returns [`Type::HttpClient`] (a real value
    /// type, NOT [`Type::Void`] like `Log`). `is_namespace_only()`
    /// returns `false` (HttpClient IS a runtime value, like Regex /
    /// Image / World). The `reqwest` crate is recorded in codegen
    /// `extern_crates` when a Buff program uses `HttpClient.*`
    /// (mirrors the chrono / regex / tracing codegen-only linking
    /// boundary). Pure-Rust, CPU-only.
    ///
    /// FFI-safe: complies with all 6 rules from
    /// `crates/buff-lang-ffi-guide/GUIDE.md` (no raw pointers; owned
    /// HttpClient / RequestBuilder / Response at the boundary; fallible
    /// ops return `Result<T, HttpError>`; HttpClient is `Clone + Send
    /// + Sync`; no lifetimes; every public body catch_unwind-wrapped
    /// per FFI guide R6).
    HttpClient,
    /// T30: `Config` — layered configuration namespace (viper-equivalent).
    /// Wraps the `buff-config` crate (`buff_config::Config`). Namespace-
    /// only module (mirror Log / Toml / Math / Random). The assoc fns
    /// `Config.new()` / `Config.set_default(key, val)` / `Config.load_file(p)`
    /// / `Config.load_env(prefix)` / `Config.load_args(args)` / `Config.get(key)`
    /// / `Config.get_int(key)` / `Config.get_float(key)` / `Config.get_bool(key)`
    /// / `Config.watch(path, callback)` are all dispatched on the
    /// PreludeType::Config namespace. `buff_type()` returns Type::Void
    /// (Config is namespace-only — no runtime value). Records `buff-config`
    /// + `figment` + `notify` in codegen `extern_crates` when a Buff
    /// program uses `Config.*` (mirrors the chrono / regex / tracing
    /// codegen-only linking boundary). Pure-Rust, no native deps.
    Config,
    /// T21: `Observe` — observability namespace (OpenTelemetry-equivalent).
    /// Wraps the `buff-observe` crate. Namespace-only module (mirror Log /
    /// Toml / Math / Random). The assoc fns `Observe.span(name)` /
    /// `Observe.counter(name)` / `Observe.histogram(name)` /
    /// `Observe.gauge(name)` / `Observe.bootstrap()` are all dispatched
    /// on the PreludeType::Observe namespace. `buff_type()` returns
    /// Type::Void (Observe is namespace-only — no runtime value).
    Observe,
    /// T27 (v1.13 frameworks wave 5): the `Fuzz` namespace — the
    /// property-based fuzzing entry point. Wraps the in-tree
    /// pure-Rust `buff-fuzz` crate (`buff_fuzz::run`) backed by
    /// `proptest` 1.5 (NOT libFuzzer / cargo-fuzz / AFL — those
    /// link C/C++ shims via cc-rs, which would FAIL on this
    /// Windows MSVC host per the "no C library, no Docker" hard
    /// rule). One assoc fn:
    /// - `Fuzz.run(strategy, iterations, closure) -> Void` — drive
    ///   the property `closure` with `iterations` random inputs
    ///   from `strategy`. The closure shape is `Fn(Int) -> Bool`
    ///   for MVP (Buff `Int` lowers to `i64`); future tasks
    ///   surface a typed `FuzzValue` enum closure.
    ///
    /// `Fuzz` is **never a runtime value** — it's a NAMESPACE
    /// exposing only associated functions (mirrors Log / Toml /
    /// Hash / HMAC / OS / Audit / Signature exactly).
    /// `buff_type()` returns [`Type::Void`]; `is_namespace_only()`
    /// returns `true`. The `buff-fuzz` crate is recorded in codegen
    /// `extern_crates` when a Buff program uses `Fuzz.*` (mirrors
    /// the chrono / regex / sha2 / buff-mock codegen-only linking
    /// boundary).
    ///
    /// NO `cargo-fuzz`, NO `afl.rs`, NO cc-rs — matches the
    /// T27 task spec mandate and the "Windows host with no MSVC"
    /// constraint that pushed hand-rolled lexer/parser.
    Fuzz,
    /// T27 (v1.13 frameworks wave 5): the `Strategy` namespace —
    /// the random-input generator builder. Wraps the in-tree
    /// pure-Rust `buff-fuzz` crate (`buff_fuzz::Strategy`).
    /// Five assoc fns (each constructs a `Strategy` value fed to
    /// `Fuzz.run`):
    /// - `Strategy.int(min, max) -> Strategy` — inclusive Int range.
    ///   Two args. Reuses the shared `Int` variant (also used by
    ///   `Random.int`), dispatched on the (Strategy, Int) pair.
    /// - `Strategy.float(min, max) -> Strategy` — Float range.
    ///   Two args. Reuses the shared `Float` variant.
    /// - `Strategy.bool() -> Strategy` — boolean generator.
    ///   Zero args.
    /// - `Strategy.string(max_len) -> Strategy` — String generator
    ///   with bounded length. One arg.
    /// - `Strategy.bytes(max_len) -> Strategy` — bytes generator
    ///   with bounded length. One arg.
    ///
    /// `Strategy` is **never a runtime value at the Buff Type
    /// level** — it's a NAMESPACE whose assoc fns return opaque
    /// `buff_fuzz::Strategy` values. `buff_type()` returns
    /// [`Type::Void`]; `is_namespace_only()` returns `true`.
    /// The `buff-fuzz` crate is recorded in codegen `extern_crates`
    /// when a Buff program uses `Strategy.*` (shared with the
    /// Fuzz.* walker — both lower to `buff_fuzz::*`).
    Strategy,
    /// T29 (v1.16 frameworks wave): the `Validator` runtime-value
    /// type — a declarative schema validator (pydantic-equivalent)
    /// wrapping `buff_validate::Validator` at codegen time.
    /// Constructed via `Validator.new()` (empty rule set); carries
    /// five builder instance methods `.with_email(field)`,
    /// `.with_url(field)`, `.with_length(field, min, max)`,
    /// `.with_range(field, min, max)`, `.with_regex(field, pattern)`,
    /// each returning a new Validator (Buff "no visible references"
    /// stance — builders consume self), plus two action methods
    /// `.validate(map) -> Result<Void, String>` and
    /// `.to_json_schema() -> String`.
    ///
    /// `Validator` IS a runtime value (NOT namespace-only like
    /// Log / Toml / Hash / HMAC / OS / Fuzz). `buff_type()` returns
    /// [`Type::Validator`]; `is_namespace_only()` returns `false`.
    /// The `buff-validate` + `validator` + `serde_json` crates are
    /// recorded in codegen `extern_crates` when a Buff program uses
    /// `Validator.*` (mirrors the chrono / regex / tracing codegen-
    /// only linking boundary). Pure-Rust, CPU-only.
    ///
    /// FFI-safe: complies with all 6 rules from
    /// `crates/buff-lang-ffi-guide/GUIDE.md` (no raw pointers; owned
    /// Validator at the boundary; fallible ops return
    /// `Result<T, ValidateError>`; Validator is `Clone + Send + Sync`;
    /// no lifetimes; every public body catch_unwind-wrapped per FFI
    /// guide R6).
    Validator,
    /// T39 (v1.17 frameworks wave 6): the `Archive` namespace —
    /// Zip / Tar / Gz / Zstd compression. Wraps the in-tree pure-Rust
    /// `buff-archive` crate (`buff_archive::Archive::*`) backed by
    /// `zip` 2 (deflate-only — pure-Rust, NO C libzstd transitively),
    /// `tar` 0.4, `flate2` 1.x (pure-Rust `miniz_oxide` backend), and
    /// `ruzstd` 0.8 (pure-Rust — NOT the canonical `zstd` crate which
    /// wraps C libzstd via cc-rs, violating the "no C library" rule).
    /// Two assoc fns:
    /// - `Archive.compress_dir(input_dir, output_path, format: String)`
    ///   -> Void. Three args. Wraps
    ///   `buff_archive::Archive::compress_dir(input_dir, output_path,
    ///   buff_archive::Format::from_extension(&format)
    ///   .unwrap_or(buff_archive::Format::Zip))?` (the `?` propagates
    ///   `ArchiveError` per Buff's R3 error-mapping contract; the
    ///   String→Format conversion lets the Buff surface use a plain
    ///   String for the format arg, matching the cross-language
    ///   convention of `tar -cf x.tar.zst` / `gzip file`).
    /// - `Archive.extract(archive_path, output_dir)` -> Void. Two
    ///   args. Wraps `buff_archive::Archive::extract(archive_path,
    ///   output_dir)?` (the format is auto-detected from the file's
    ///   extension inside the wrapper).
    ///
    /// This is a namespace-only module (mirror Log / Toml / Math /
    /// Config / Observe): `buff_type()` returns `Type::Void`. The
    /// crate records `buff-archive` + `zip` + `tar` + `flate2` +
    /// `ruzstd` in codegen `extern_crates` when a Buff program uses
    /// `Archive.*` (mirrors the chrono / regex / tracing / image
    /// codegen-only linking boundary). Pure-Rust, no native deps.
    /// NO 7z, RAR, BZip2, encryption-at-rest — all forbidden by the
    /// T39 task spec.
    Archive,
    /// T46 (v1.18 frameworks wave 7): the `Text` namespace — text
    /// processing / NLP. Wraps the in-tree pure-Rust `buff-nlp` crate
    /// (`buff_nlp::Text::*`) backed by `whatlang` 0.16 (language
    /// detection — pure-Rust trigram classifier for 69+ languages),
    /// `rust-stemmers` 1.2 (Snowball stemmer for 18 languages — pure-
    /// Rust reference, NO C bindings), and `unicode-segmentation` 1.12
    /// (UAX #29 word + sentence segmentation — already pinned for T124
    /// String segmentation). Four assoc fns:
    /// - `Text.detect_language(text)` -> String? (Option). One arg.
    ///   Wraps `buff_nlp::Text::detect_language(&text).map(|l|
    ///   l.code())` (returns the ISO 639-3 code so the Buff surface
    ///   stays simple — `Option<String>`; the full Language struct
    ///   lives in the Rust crate).
    /// - `Text.stem(word, algorithm: String)` -> String. Two args.
    ///   Wraps `buff_nlp::Text::stem(&word,
    ///   StemAlgorithm::from_code(&algorithm).unwrap_or(English))?`
    ///   (the `?` propagates `NlpError` per Buff's R3 error-mapping
    ///   contract; the String→StemAlgorithm conversion lets the Buff
    ///   surface use a plain String for the algorithm arg, matching
    ///   the cross-language convention of `"english"` / `"portuguese"`
    ///   / `"french"` Snowball names).
    /// - `Text.tokenize(text)` -> Vector<String>. One arg. Wraps
    ///   `buff_nlp::Text::tokenize(&text)` (UAX #29 word boundary
    ///   segmentation; drops punctuation + whitespace).
    /// - `Text.sentences(text)` -> Vector<String>. One arg. Wraps
    ///   `buff_nlp::Text::sentences(&text)` (UAX #29 sentence boundary
    ///   segmentation).
    ///
    /// This is a namespace-only module (mirror Archive / Log / Toml /
    /// Math / Config / Observe): `buff_type()` returns `Type::Void`.
    /// The crate records `buff-nlp` + `whatlang` + `rust-stemmers` +
    /// `unicode-segmentation` in codegen `extern_crates` when a Buff
    /// program uses `Text.*` (mirrors the chrono / regex / tracing /
    /// image codegen-only linking boundary). Pure-Rust, no native
    /// deps. NO lemmatization, NO ML-based NER, NO embeddings — all
    /// deferred to v1.20+ (T46 ships the pure-Rust MVP only).
    Text,
    /// T34 (v1.16 frameworks wave 4): the `JWT` namespace — JSON Web
    /// Token encode/decode. Wraps the in-tree pure-Rust `buff-auth`
    /// crate (`buff_auth::jwt_encode` / `buff_auth::jwt_decode`) which
    /// in turn wraps `jsonwebtoken` 10 with the `rust_crypto` backend
    /// (pure-Rust, NO `ring`, NO `aws-lc-rs`, NO native-tls, NO cc-rs
    /// — matches the "Windows host with no MSVC" constraint). Two
    /// assoc fns:
    /// - `JWT.encode(claims, secret) -> String` — HS256 compact JWS.
    ///   Two args (Map<String, Unknown>, String). Returns the
    ///   `header.payload.signature` token String. Wraps
    ///   `buff_auth::jwt_encode(&claims, secret)?` (the `?` propagates
    ///   `AuthError::Jwt` per Buff's R3 error-mapping contract).
    /// - `JWT.decode(token, secret) -> Map<String, Unknown>` — verify
    ///   + decode to claims map. Two args (String, String). Returns
    ///   the claims as a heterogeneous Map. Wraps
    ///   `buff_auth::jwt_decode(token, secret).unwrap_or_default()`
    ///   (panic-free — invalid signature / malformed token / expired
    ///   all collapse to an empty Map, NEVER panics).
    ///
    /// `JWT` is **never a runtime value** — it's a NAMESPACE exposing
    /// only associated functions (mirrors Log / Toml / Hash / HMAC /
    /// OS / Audit / Signature / Fuzz / Strategy / Config exactly).
    /// `buff_type()` returns [`Type::Void`]; `is_namespace_only()`
    /// returns `true`. The `buff-auth` + `jsonwebtoken` + `argon2` +
    /// `oauth2` + `reqwest` crates are recorded in codegen
    /// `extern_crates` when a Buff program uses `JWT.*` (shared walker
    /// with OAuth2Client / Password / Rbac — mirrors the chrono /
    /// regex / sha2 / ed25519-dalek codegen-only linking boundary).
    ///
    /// NO `ring`, NO native-tls, NO cc-rs — the T34 task spec
    /// explicitly forbids all three. The `rust_crypto` backend of
    /// `jsonwebtoken` is the canonical pure-Rust alternative.
    Jwt,
    /// T34: the `OAuth2Client` runtime-value type — OAuth2
    /// authorization-code flow client (with PKCE for public clients).
    /// Wraps `buff_auth::OAuth2Client` (which in turn wraps `oauth2`
    /// 4 via `reqwest` rustls-tls). One assoc fn + two instance
    /// methods:
    /// - `OAuth2Client.new(client_id, client_secret?, auth_url,
    ///   token_url, redirect_url, scopes) -> OAuth2Client` — ctor.
    ///   Six args; `client_secret = ""` triggers PKCE flow.
    /// - `client.authorization_url() -> String` — the URL the user
    ///   must visit in a browser. Embeds `#pkce_verifier=...` for
    ///   public clients (the caller extracts + passes back to
    ///   `exchange_code`).
    /// - `client.exchange_code(code, pkce_verifier?) -> Map<String,
    ///   Unknown>` — blocking POST to token endpoint. Two args
    ///   (String, String — pass `""` for confidential clients).
    ///   Returns the token response fields (`access_token`,
    ///   `token_type`, `expires_in`, `refresh_token`, `scope`).
    ///
    /// `OAuth2Client` IS a runtime value (NOT namespace-only — like
    /// Regex / URL / Path / Process / Image / World / Database /
    /// Validator). `buff_type()` returns [`Type::Unknown`] for MVP —
    /// the coordinated [`Type::OAuth2Client`] variant in `ty.rs` is a
    /// follow-up sibling task OUTSIDE the T34 shared zone (mirrors
    /// the T17 Web / T18 Database / T8 Tensor / T11 Signal forward-
    /// declaration precedent). `is_namespace_only()` returns `false`.
    /// The codegen lowering for the assoc fn `OAuth2Client.new` +
    /// both instance methods is shipped in this T34 commit (dispatch
    /// on PreludeType for assoc fn; instance-method dispatch
    /// requires Type::OAuth2Client — TBD follow-up).
    ///
    /// Records `buff-auth` + `oauth2` + `reqwest` in codegen
    /// `extern_crates` when a Buff program uses `OAuth2Client.*`.
    OAuth2Client,
    /// T34: the `Password` namespace — Argon2id password hashing.
    /// Wraps `buff_auth::password_hash` / `password_verify` (which
    /// in turn wrap the RustCrypto `argon2` crate — pure-Rust, NO
    /// `ring`). Two assoc fns:
    /// - `Password.hash(plain) -> String` — Argon2id PHC string.
    ///   One arg (String). Returns the canonical
    ///   `$argon2id$v=19$m=...,t=...,p=...$<salt>$<hash>` form
    ///   ready for storage in a user database. Wraps
    ///   `buff_auth::password_hash(plain).unwrap_or_default()` (
    ///   panic-free — empty String on hash failure, NEVER panics).
    /// - `Password.verify(plain, phc_hash) -> Bool` — verify a
    ///   plaintext against a stored PHC hash. Two args. Returns
    ///   `false` on mismatch (NEVER panics, NEVER errors on plain
    ///   mismatch — mirrors the T26 Signature.verify stance so a
    ///   future `login_allow` policy can layer cleanly). Wraps
    ///   `buff_auth::password_verify(plain, hash).unwrap_or(false)`.
    ///
    /// `Password` is **never a runtime value** — it's a NAMESPACE
    /// (mirrors JWT / Audit / Signature / Hash / HMAC / OS / Log /
    /// Toml exactly). `buff_type()` returns [`Type::Void`];
    /// `is_namespace_only()` returns `true`. Records `buff-auth` +
    /// `argon2` in codegen `extern_crates` (shared walker).
    Password,
    /// T34: the `Rbac` runtime-value type — role-based access
    /// control policy. Wraps `buff_auth::Rbac` (an in-tree
    /// `BTreeSet<(role, resource, action)>` with wildcard `*` match
    /// — NO extern crate, pure stdlib). One assoc fn + one builder
    /// + one decision method:
    /// - `Rbac.new() -> Rbac` — empty policy.
    /// - `policy.add(role, resource, action) -> Void` — add a rule
    ///   (dedup'd; empty fields rejected via fallible shape, but the
    ///   MVP lowering uses `.unwrap_or(())` so it is panic-free).
    /// - `policy.enforce(roles, resource, action) -> Bool` — does
    ///   at least one rule match at least one of the supplied roles?
    ///   Wildcard `*` on any field matches anything.
    ///
    /// `Rbac` IS a runtime value (NOT namespace-only). `buff_type()`
    /// returns [`Type::Unknown`] for MVP — the coordinated
    /// [`Type::Rbac`] variant in `ty.rs` is a follow-up sibling task
    /// OUTSIDE the T34 shared zone (mirrors the T17 Web / T18
    /// Database / OAuth2Client forward-declaration precedent). The
    /// assoc fn `Rbac.new` is shipped in this T34 commit; the
    /// instance methods `add` + `enforce` are deferred to the sibling
    /// task that adds Type::Rbac (mirrors the T17 / T18 / T34-
    /// OAuth2Client instance-method dispatch gap).
    ///
    /// Records `buff-auth` in codegen `extern_crates` (shared walker
    /// — no extra deps for the in-tree RBAC engine).
    Rbac,
    /// T42 (v1.17 frameworks wave): the `Email` runtime-value type —
    /// a buildable email message wrapping `buff_email::Email` at
    /// codegen time (which in turn wraps `lettre::Message::builder`).
    /// Constructed via the prelude associated function
    /// `Email.new(from, to, subject)` (validates RFC 5322 mailboxes);
    /// carries the three builder instance methods `email.body(text)`,
    /// `email.html(template, context_json)`, `email.attach(path)`,
    /// each consuming `self` and returning a new `Email` (Buff "no
    /// visible references" stance — mirrors Validator / HttpClient).
    ///
    /// `Email` IS a runtime value (NOT namespace-only like Log /
    /// Toml / JWT). `buff_type()` returns [`Type::Email`];
    /// `is_namespace_only()` returns `false`. The `buff-email` +
    /// `lettre` + `handlebars` crates are recorded in codegen
    /// `extern_crates` when a Buff program uses `Email.*` (mirrors
    /// the chrono / regex / tracing codegen-only linking boundary).
    /// Pure-Rust, CPU-only. TLS via rustls (NOT native-tls).
    ///
    /// FFI-safe: complies with all 6 rules from
    /// `crates/buff-lang-ffi-guide/GUIDE.md` (no raw pointers; owned
    /// Email at the boundary; fallible ops return
    /// `Result<T, EmailError>`; Email is `Clone + Send + Sync`; no
    /// lifetimes; every public body catch_unwind-wrapped per FFI
    /// guide R6).
    Email,
    /// T42 (v1.17 frameworks wave): the `SmtpClient` runtime-value
    /// type — a configured SMTP transport wrapping
    /// `buff_email::SmtpClient` at codegen time (which in turn wraps
    /// `lettre::SmtpTransport::relay(...).port(...).credentials(...)
    /// .build()`). Constructed via the prelude associated function
    /// `SmtpClient.new(host, port, username, password)` (configures
    /// STARTTLS — pure-Rust rustls, NOT native-tls); carries the
    /// single instance method `client.send(email) -> Result<Void,
    /// EmailError>`.
    ///
    /// `SmtpClient` IS a runtime value. `buff_type()` returns
    /// [`Type::SmtpClient`]; `is_namespace_only()` returns `false`.
    /// The `buff-email` + `lettre` crates are recorded in codegen
    /// `extern_crates` when a Buff program uses `SmtpClient.*`
    /// (shared walker with `Email.*` — mirrors the chrono / regex /
    /// tracing codegen-only linking boundary). Pure-Rust, CPU-only.
    ///
    /// FFI-safe: complies with all 6 rules from
    /// `crates/buff-lang-ffi-guide/GUIDE.md`. IMAP / POP3 receiving
    /// explicitly deferred to v1.22+ per T42 must-not #1.
    SmtpClient,
    /// T43 (v1.17 frameworks): the `Document` runtime-value type —
    /// a parsed HTML document wrapping `buff_scrape::Document` at
    /// codegen time. Constructed via the prelude associated
    /// function `Document.from_html(html)` (zero network I/O —
    /// `Document` is purely an in-memory parse; for HTTP fetch use
    /// `Crawler.fetch`); carries 4 instance methods:
    /// `doc.select(css) -> Vector<Element>`, `doc.text() -> String`,
    /// `doc.html() -> String`, `doc.title() -> String?`.
    ///
    /// `Document` IS a runtime value. `buff_type()` returns
    /// [`Type::Document`]; `is_namespace_only()` returns `false`.
    /// The `buff-scrape` + `scraper` crates are recorded in codegen
    /// `extern_crates` when a Buff program uses `Document.*` (mirrors
    /// the chrono / regex / tracing codegen-only linking boundary).
    /// Pure-Rust, CPU-only. NO JS rendering (deferred to an optional
    /// `fantoccini` path per T43 spec).
    ///
    /// FFI-safe: complies with all 6 rules from
    /// `crates/buff-lang-ffi-guide/GUIDE.md`. `scraper::Html` is
    /// `!Send + !Sync`; the wrapper caches the source `String` and
    /// re-parses per access so the Buff-visible `Document` IS
    /// `Send + Sync + Clone`.
    Document,
    /// T43: the `Element` runtime-value type — a single selected
    /// HTML element wrapping `buff_scrape::Element` at codegen time.
    /// Constructed as the return value of `Document.select(css)` /
    /// `Element.select(css)` (NOT via an associated function —
    /// Elements come from queries only). Carries 5 instance methods:
    /// `el.text() -> String`, `el.attr(name) -> String?`,
    /// `el.html() -> String`, `el.inner_html() -> String`,
    /// `el.select(css) -> Vector<Element>`.
    ///
    /// `Element` IS a runtime value. `buff_type()` returns
    /// [`Type::Element`]; `is_namespace_only()` returns `false`.
    /// Owned values: text / html / inner_html / attrs are cached
    /// eagerly at construction (cheap clones + `Send + Sync + Clone`).
    Element,
    /// T43: the `Crawler` runtime-value type — an HTTP crawler
    /// wrapping `buff_scrape::Crawler` at codegen time. Constructed
    /// via the prelude associated function
    /// `Crawler.new(seed_url)`; carries 4 instance methods:
    /// `crawler.seed() -> String`, `crawler.fetch(url) -> Document`,
    /// `crawler.crawl(max_pages) -> Vector<String>`,
    /// `crawler.robots_allows(url) -> Bool`. Single-host BFS;
    /// robots.txt-aware (fail-open on missing rules). NO distributed
    /// crawling (forbidden by T43 spec).
    ///
    /// `Crawler` IS a runtime value. `buff_type()` returns
    /// [`Type::Crawler`]; `is_namespace_only()` returns `false`.
    /// The `buff-scrape` + `reqwest` crates are recorded in codegen
    /// `extern_crates` when a Buff program uses `Crawler.*` (shared
    /// walker with `Document.*`). Pure-Rust TLS via rustls (NOT
    /// native-tls).
    Crawler,
    /// T50: `Xml` — runtime-value type wrapping `buff_xml::XmlDocument`.
    /// Constructed via `Xml.from_str(xml)`; carries the instance methods
    /// `.root()`, `.find(xpath)`, `.to_string()`. Mirrors Image / Faker
    /// as a runtime-value-with-instance-methods type. Pure-Rust, CPU-only.
    Xml,
    /// T50: `XmlElement` — runtime-value type wrapping
    /// `buff_xml::XmlElement`. Constructed via
    /// `XmlElement.new(name, text, attrs)`; carries the instance
    /// methods `.name()`, `.attr(name)`, `.text()`, `.children()`.
    /// Also returned by `XmlDocument.root()` / `.find(xpath)`. Mirrors
    /// Xml / Image / Faker as a runtime-value-with-instance-methods
    /// type. Pure-Rust, CPU-only.
    XmlElement,
    /// T51: `MsgPack` — MessagePack binary format namespace.
    /// Namespace-only (like `Log` / `Toml` / `Base64` / `Hex` / `Yaml` /
    /// `Csv`). Provides `MsgPack.serialize(value) -> Bytes` and
    /// `MsgPack.deserialize(bytes) -> Value` (plus
    /// `MsgPack.roundtrip(value) -> Option<Value>`). Codegen lowering
    /// lives in the buff-msgpack crate (`buff_msgpack::serialize`
    /// / `buff_msgpack::deserialize` / `buff_msgpack::roundtrip`).
    /// Pure-Rust, no native deps.
    MsgPack,
    /// T45: `Point` — a 2D geospatial point with f64 coordinates.
    /// Runtime-value type wrapping `buff_geo::Point` (which wraps
    /// `geo_types::Point<f64>`). Constructed via `Point.new(x, y)`; carries
    /// the instance methods `.x()`, `.y()`, `.distance_to(other)`.
    /// Mirrors Image / Faker / Regex as a runtime-value-with-rich-
    /// instance-methods type. Pure-Rust, CPU-only per Metis G7 lock.
    Point,
    /// T45: `LineString` — a geospatial polyline (ordered sequence of
    /// Points). Runtime-value type wrapping `buff_geo::LineString` (which
    /// wraps `geo_types::LineString<f64>`). Constructed via
    /// `LineString.new(points)` / `LineString.from_coords(flat)`; carries
    /// the instance method `.length()`. Mirrors Point as a runtime-value-
    /// with-instance-methods type. Pure-Rust, CPU-only.
    LineString,
    /// T45: `Polygon` — a geospatial polygon (outer ring + future holes).
    /// Runtime-value type wrapping `buff_geo::Polygon` (which wraps
    /// `geo_types::Polygon<f64>`). Constructed via `Polygon.new(ring)` /
    /// `Polygon.from_coords(flat)`; carries the instance methods `.area()`,
    /// `.contains(point)`, `.intersects(other)`. Mirrors Point as a
    /// runtime-value-with-instance-methods type. Pure-Rust, CPU-only.
    Polygon,
    /// T46: `Language` — a detected natural language. Runtime-value type
    /// wrapping `buff_nlp::Language` (which wraps `whatlang::Lang`).
    /// Constructed ONLY via `Text.detect_language(input) -> Option<
    /// Language>`; carries the instance methods `.code() -> String`
    /// (ISO 639-3) and `.name() -> String` (English name). Mirrors
    /// Point / Image as a runtime-value-with-instance-methods type.
    /// Pure-Rust, CPU-only.
    Language,
    /// T46: `StemAlgorithm` — a Snowball stemming algorithm selector
    /// (18 supported languages). Opaque enum wrapping
    /// `buff_nlp::StemAlgorithm` (which maps 1:1 to
    /// `rust_stemmers::Algorithm`). Passed only as an arg to
    /// `Text.stem(word, algorithm: .english)`; carries NO instance
    /// methods and NO constructor (Buff users write enum-variant
    /// literal syntax `.english` / `.portuguese` / etc.). Mirrors no
    /// prior prelude type exactly — it is the first opaque enum
    /// passed-only-as-arg in the prelude. Pure-Rust, CPU-only.
    StemAlgorithm,
    /// T52: `Protobuf` — Protocol Buffers binary format namespace.
    /// Namespace-only (like `MsgPack` / `Log` / `Toml` / `Base64` /
    /// `Hex` / `Yaml` / `Csv`). Provides `Protobuf.serialize(value)
    /// -> Bytes` and `Protobuf.deserialize(bytes) -> Value` (plus
    /// `Protobuf.roundtrip(value) -> Option<Value>`). Codegen lowering
    /// lives in the buff-protobuf crate (`buff_protobuf::serialize`
    /// / `buff_protobuf::deserialize` / `buff_protobuf::roundtrip`).
    /// Pure-Rust, no native deps (NO protoc / NO protoc-built `.proto`
    /// codegen in MVP — gRPC streaming + `prost-build` deferred).
    Protobuf,
    /// T52: `Message` — a protobuf-encoded message runtime value.
    /// Constructed via `Message.new(value)` (encode) /
    /// `Message.from_bytes(bytes)` / `Message.decode(bytes)` (decode);
    /// carries the instance methods `.byte_size() -> Int`,
    /// `.type_url() -> String`, `.payload() -> Value`,
    /// `.encode() -> Vector<Byte>`. Mirrors Image / Xml / Point as a
    /// runtime-value-with-rich-instance-methods type. Codegen lowering
    /// lives in the buff-protobuf crate (`buff_protobuf::Message::*`).
    /// Pure-Rust, CPU-only; uses the well-known
    /// `google.protobuf.Struct` schema as the dynamic message surface.
    Message,
    /// T47: `Bot` — a cross-platform chat bot runtime value (Discord via
    /// serenity, Telegram via teloxide). Constructed via
    /// `Bot.new(platform, token)`; carries the instance methods
    /// `bot.command(name, handler)` / `bot.on_message(handler)` /
    /// `bot.start()` / `bot.stop()` / `bot.dispatch(msg)` /
    /// `bot.platform()` / `bot.is_running()` / `bot.command_count()` /
    /// `bot.has_message_handler()`. Mirrors Image / Point / Message as a
    /// runtime-value-with-rich-instance-methods type. Codegen lowering
    /// lives in the buff-chat crate (`buff_chat::Bot::*`). Pure-Rust,
    /// CPU-only; both serenity + teloxide use rustls + ring (NO
    /// native-tls, NO cc-rs — matches the "no C library, no Docker"
    /// hard rule).
    Bot,
    /// T47: `ChatMessage` — the chat message runtime value. Named
    /// `ChatMessage` (NOT `Message`) at the Buff surface to avoid
    /// colliding with the T52 protobuf [`PreludeType::Message`] variant.
    /// Constructed via `ChatMessage.new(text, channel, author, platform,
    /// is_dm)`; carries the instance methods `msg.text()` /
    /// `msg.channel()` / `msg.author()` / `msg.platform()` /
    /// `msg.is_dm()`. Mirrors Image / Point / Message (T52). Codegen
    /// lowering lives in the buff-chat crate
    /// (`buff_chat::Message::*`). Pure-Rust, CPU-only.
    ChatMessage,
    /// T47: `Platform` — the chat platform enum-like runtime value
    /// (`Platform.Discord` / `Platform.Telegram`). The two variants are
    /// exposed as associated constants (zero-arg `Type.NAME` access
    /// shape — lowered through `PreludeAssocConst::Discord` /
    /// `PreludeAssocConst::Telegram`); the instance methods
    /// `platform.is_discord()` / `platform.is_telegram()` are exposed
    /// via `PreludeInstanceFn::IsDiscord` / `PreludeInstanceFn::IsTelegram`.
    /// Mirrors StemAlgorithm (T46) as an opaque enum passed primarily as
    /// an arg. Codegen lowering lives in the buff-chat crate
    /// (`buff_chat::Platform::*`). Pure-Rust, CPU-only.
    Platform,
    /// T48: `Provider` — the Ethereum JSON-RPC provider runtime value.
    /// Constructed via `Provider.new(rpc_url)`; carries the instance
    /// methods `provider.chain_id()` / `provider.block_number()` /
    /// `provider.get_balance(addr)` / `provider.get_nonce(addr)` /
    /// `provider.wait_for_tx(hash)`. Wraps `buff_web3::Provider`
    /// (`Arc<EthProvider<Http>>`, `Send + Sync + Clone`). Pure-Rust TLS
    /// via rustls (the `ethers` `rustls` feature flag); shared tokio
    /// runtime hidden behind `block_on`. Mirrors HttpClient (T33) /
    /// Bot (T47). Pure-Rust, CPU-only.
    Provider,
    /// T48: `Wallet` — the secp256k1 private-key wallet runtime value.
    /// Constructed via `Wallet.from_private_key(key)`; carries the
    /// instance methods `wallet.address()` / `wallet.connect(provider)`
    /// / `wallet.sign_message(msg)`. Wraps `buff_web3::Wallet`
    /// (wrapping `ethers::signers::LocalWallet`). Pure-Rust, CPU-only.
    Wallet,
    /// T48: `ConnectedWallet` — the Wallet+Provider pair (signing
    /// client) runtime value. Constructed ONLY via
    /// `wallet.connect(provider)`; carries the single instance method
    /// `cw.address()`. Wraps `buff_web3::ConnectedWallet` (`{provider,
    /// wallet}` pair struct). Pure-Rust, CPU-only.
    ConnectedWallet,
    /// T48: `Contract` — the deployed smart contract runtime value.
    /// Constructed via `Contract.new(address, abi, client)`; carries
    /// the instance methods `contract.address()` /
    /// `contract.method(name)`. Wraps `buff_web3::Contract`
    /// (`{address, abi, client}` struct). Pure-Rust, CPU-only.
    Contract,
    /// T48: `ContractMethod` — the chainable ABI method call-builder
    /// runtime value. Constructed ONLY via `contract.method(name)`;
    /// carries the chainable instance methods `m.arg(name, value)` /
    /// `m.args(values)` + terminal `m.call()` / `m.send()`. Wraps
    /// `buff_web3::ContractMethod` (`{address, abi, client,
    /// method_name, args}` struct). Pure-Rust, CPU-only.
    ContractMethod,
    /// T49: `AES` — AES-256-GCM authenticated encryption namespace.
    /// Namespace-only (like `MsgPack` / `Log` / `Toml` / `Base64` /
    /// `Hex` / `Yaml` / `Csv` / `Text` / `Protobuf`). Provides
    /// `AES.generate_key() -> Vector<Byte>` (32 bytes),
    /// `AES.generate_nonce() -> Vector<Byte>` (12 bytes),
    /// `AES.encrypt(key, nonce, plaintext) -> Vector<Byte>`
    /// (ciphertext || 16-byte GCM tag), `AES.decrypt(key, nonce,
    /// ciphertext) -> Vector<Byte>` (recovered plaintext). Codegen
    /// lowering lives in the buff-crypto-extras crate
    /// (`buff_crypto_extras::AES::*`). Pure-Rust, CPU-only (NO GPU
    /// dispatch — AEAD never runs on the GPU path).
    AES,
    /// T49: `RSA` — RSA PKCS#1 v1.5 SHA-256 digital signature
    /// namespace. Namespace-only (like `AES` / `MsgPack` / `Log`).
    /// Provides `RSA.generate_keypair(bits: 2048) -> RsaKeypair`,
    /// `RSA.sign(private_pem, data) -> Vector<Byte>` (raw signature
    /// bytes — 256 bytes for 2048-bit modulus),
    /// `RSA.verify(public_pem, data, signature) -> Bool` (false on
    /// any failure — mirrors T26 Signature.verify + T34
    /// Password.verify). RSAES-PKCS1-v1_5 / RSAES-OAEP encryption
    /// is deliberately NOT exposed (T49 spec scopes RSA to
    /// signatures; for public-key encryption use hybrid AES-GCM +
    /// ECDH). Codegen lowering lives in the buff-crypto-extras
    /// crate (`buff_crypto_extras::RSA::*`). Pure-Rust, CPU-only.
    RSA,
    /// T49: `ECDH` — NIST P-256 / P-384 ECDH key agreement
    /// namespace. Namespace-only (like `AES` / `RSA` / `MsgPack`).
    /// Provides `ECDH.generate_private() -> Vector<Byte>` (32-byte
    /// P-256 scalar), `ECDH.public_from_private(private) ->
    /// Vector<Byte>` (65-byte SEC1 uncompressed point),
    /// `ECDH.derive_shared(private, public) -> Vector<Byte>` (32-byte
    /// shared secret — x-coordinate of the shared point). Codegen
    /// lowering lives in the buff-crypto-extras crate
    /// (`buff_crypto_extras::ECDH::*`). Pure-Rust, CPU-only.
    ECDH,
    /// T49: `Argon2` — raw Argon2id key-derivation namespace. Distinct
    /// from T34's PHC-string Password hashing (Password.hash returns
    /// a PHC string for human password storage; Argon2.derive_key
    /// returns raw 32-byte derived key material for direct use as an
    /// AES-256 key). Namespace-only (like `AES` / `RSA` / `ECDH`).
    /// Provides `Argon2.generate_salt() -> Vector<Byte>` (16 bytes),
    /// `Argon2.derive_key(password, salt) -> Vector<Byte>` (32 bytes).
    /// Defaults follow OWASP Argon2id (2024): m=19456 KiB, t=2, p=1.
    /// Codegen lowering lives in the buff-crypto-extras crate
    /// (`buff_crypto_extras::Argon2::*`). Pure-Rust, CPU-only.
    Argon2,
    /// T49: `RsaKeypair` — the RSA keypair runtime-value type (the
    /// single instance type in T49 — AES / RSA / ECDH / Argon2 are
    /// all namespace-only). Constructed ONLY via
    /// `RSA.generate_keypair(bits: 2048)`; carries the two instance
    /// methods `.public_pem() -> String` (Spki SubjectPublicKeyInfo
    /// PEM) and `.private_pem() -> String` (PKCS#8 PEM). Wraps
    /// `buff_crypto_extras::RsaKeypair` (`{public_pem: String,
    /// private_pem: String}` struct — `Send + Sync + Clone`).
    /// Pure-Rust, CPU-only.
    RsaKeypair,
    /// T54: `Simd` — a 4-lane `f32` SIMD register (the concrete
    /// realisation of the conceptual `Simd<Float, 4>`). Runtime-value
    /// type wrapping `buff_simd::Simd` (which wraps `wide::f32x4` — a
    /// 128-bit SSE/NEON register). Constructed via `Simd.splat(x)`
    /// (broadcast), `Simd.from_slice(slice)` (length-checked), or
    /// `Simd.from_array(arr)`; carries the instance methods `.add(other)`,
    /// `.sub(other)`, `.mul(other)`, `.div(other)` (lane-wise binary),
    /// `.sum()`, `.min()`, `.max()` (horizontal reductions), `.to_vec()`
    /// (extract). Mirrors Image / Point / RsaKeypair as a runtime-value-
    /// with-rich-instance-methods type. Pure-Rust, CPU-only per Metis
    /// G7 lock (NO GPU dispatch — GPU SIMD is WGSL's job); wraps the
    /// `wide` crate (stable portable SIMD — NO nightly `std::simd`, NO
    /// runtime `is_x86_feature_detected!` detection per T54 spec).
    Simd,
    /// T59: actor-system runtime-value type. Maps to
    /// `buff_actors::ActorSystem`. Constructed via `ActorSystem.new()`.
    ActorSystem,
    /// T59: handle to a running actor. Maps to `buff_actors::ActorRef`.
    ActorRef,
    /// T59: supervisor runtime-value type. Maps to
    /// `buff_actors::Supervisor`.
    Supervisor,
    /// T59: child-spawn factory record. Maps to
    /// `buff_actors::supervisor::ChildSpec`.
    ChildSpec,
    /// T59: restart-strategy enum (namespace-only — mirrors Platform
    /// / StemAlgorithm). Variants: `.permanent` / `.temporary` /
    /// `.transient`. Maps to
    /// `buff_actors::supervisor::RestartStrategy`.
    RestartStrategy,
}


// T105b: impl PreludeType + lookup helpers extracted to prelude_type_metadata.
pub use crate::prelude_type_metadata::{is_prelude_type, prelude_type_lookup};


// T105b: PreludeAssocFn + impl + lookup extracted to prelude_assoc_fn_impl.
pub use crate::prelude_assoc_fn_impl::{PreludeAssocFn, assoc_fn_lookup};

// T105b: PreludeAssocConst + impl + lookups extracted to prelude_assoc_const_impl.
pub use crate::prelude_assoc_const_impl::{PreludeAssocConst, assoc_const_lookup, assoc_const_return_type};

// T105b: return-type inference extracted to prelude_return_types.
pub use crate::prelude_return_types::{assoc_fn_return_type, instance_fn_return_type};

// T105b: PreludeInstanceFn + impl + lookup extracted to prelude_instance_fn_impl.
pub use crate::prelude_instance_fn_impl::{PreludeInstanceFn, instance_fn_lookup};


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
                && t != PreludeType::Web
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
        // + 1 namespace-only module (Channel) shipped in T2 + 1
        // forward-declaration-only namespace (Tensor) + 1 runtime-
        // value-with-methods type (Image) shipped in T9 + 2
        // namespace-only modules (Signal, Window) + 1 runtime-value-
        // with-methods type (Spectrum) shipped in T11 + 1 runtime-
        // value-with-methods type (DataFrame) shipped in T7
        // = 36 total prelude types.
        assert_eq!(PreludeType::ALL.len(), 37);
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

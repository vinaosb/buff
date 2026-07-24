//! T105b: Return-type inference for prelude associated fns + instance fns.
//!
//! MECHANICAL EXTRACTION from prelude_types.rs (T105b God Class split).
//! No logic changes — moved verbatim. Contains the two large match-based
//! return-type functions. Split from their respective enum+impl files because
//! each enum+impl+return_type combination exceeds the 2000 LOC file limit.

use crate::prelude_types::{PreludeAssocFn, PreludeInstanceFn, PreludeType};
use crate::ty::Type;

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
        // Inclusive range - `min..=max` in Rust's `random_range`.
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
        // T23: Json module - mirrors Yaml / Toml exactly.
        // `Json.parse(s)` returns a Buff `Map<String, Unknown>`.
        // The codegen emits `HashMap<String, serde_json::Value>`.
        (PreludeType::Json, PreludeAssocFn::Parse) => {
            Some(Type::map(Type::string(), Type::Unknown))
        }
        // T124i: Yaml module - `Yaml.stringify(v)` returns a YAML-
        // formatted String. Mirrors Toml.stringify exactly (the
        // `serde_yml::to_string` API is structurally identical to
        // `toml::to_string` - both take `&impl Serialize` and return
        // `Result<String, _>`). The codegen borrows the arg via `&v`
        // so Rust's serde-Serialize bound is satisfied.
        (PreludeType::Yaml, PreludeAssocFn::Stringify) => Some(Type::string()),
        // T23: Json module - `Json.stringify(v)` returns a JSON-
        // formatted String. Mirrors Yaml.stringify / Toml.stringify
        // exactly (serde_json::to_string takes `&impl Serialize` and
        // returns `Result<String, _>`). The codegen borrows the arg
        // via `&v` so Rust's serde-Serialize bound is satisfied.
        (PreludeType::Json, PreludeAssocFn::Stringify) => Some(Type::string()),
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
        (PreludeType::Channel, PreludeAssocFn::New) => {
            Some(Type::tuple(vec![Type::Sender, Type::Receiver]))
        }
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
        // T37: Faker assoc fns. `Faker.new()` -> Faker. Wraps
        // `buff_fake::Faker::new()`. Default locale (en-US), random seed.
        (PreludeType::Faker, PreludeAssocFn::New) => Some(Type::Faker),
        // `Faker.with_locale(locale)` -> Faker. One arg (String locale).
        // Wraps `buff_fake::Faker::with_locale(locale)`.
        (PreludeType::Faker, PreludeAssocFn::WithLocale) => Some(Type::Faker),
        // `Faker.with_seed(locale, seed)` -> Faker. Two args (String
        // locale, Int seed). Wraps `buff_fake::Faker::with_seed(locale, seed)`.
        (PreludeType::Faker, PreludeAssocFn::WithSeed) => Some(Type::Faker),
        // T44: I18n assoc fns. `I18n.new(locale)` -> I18n. One arg
        // (String locale). Wraps `buff_i18n::I18n::new(locale)
        // .unwrap_or_default()` (panic-free on invalid locale —
        // returns an empty English catalog, matching Buff's "no
        // panicking generated code" rule). Records `buff-i18n` +
        // `fluent-bundle` + `unic-langid` in extern_crates.
        (PreludeType::I18n, PreludeAssocFn::New) => Some(Type::I18n),
        // `I18n.with_fallback(locale, fallback)` -> I18n. Two args
        // (String locale, String fallback). Wraps
        // `buff_i18n::I18n::with_fallback(locale, fallback)
        // .unwrap_or_default()` (panic-free fallback).
        (PreludeType::I18n, PreludeAssocFn::WithFallback) => Some(Type::I18n),
        // T10: AudioBuffer assoc fns. `AudioBuffer.from_path(path)`
        // -> AudioBuffer. Wraps `buff_audio::AudioBuffer::from_path(p)?
        // ` (the `?` propagates AudioError per R3). Decodes WAV via
        // hound, MP3/FLAC/Vorbis via symphonia. Same shared `FromPath`
        // variant as Image.from_path — dispatched on the (Audio,
        // FromPath) pair (mirrors `Parse` shared between DateTime /
        // Date / Toml / URL / UUID).
        (PreludeType::Audio, PreludeAssocFn::FromPath) => Some(Type::Audio),
        // `AudioBuffer.from_samples(samples, sample_rate, channels)`
        // -> AudioBuffer. Wraps `buff_audio::AudioBuffer::from_samples
        // (samples, sample_rate as u32, channels as u16)?`. Used by
        // programmatic tone generators (buff-dsp T11 is the canonical
        // consumer).
        (PreludeType::Audio, PreludeAssocFn::FromSamples) => Some(Type::Audio),
        // T11: Signal.from_vec reuses FromVec (shared with Tensor).
        // Returns Signal (modeled as Void — coordinated Type::Signal
        // variant is a follow-up outside T11 shared zone).
        (PreludeType::Signal, PreludeAssocFn::FromVec) => Some(Type::Void),
        // T11: Window constructors return Window (modeled as Void).
        (PreludeType::Window, PreludeAssocFn::Hann) => Some(Type::Void),
        (PreludeType::Window, PreludeAssocFn::Hamming) => Some(Type::Void),
        (PreludeType::Window, PreludeAssocFn::Blackman) => Some(Type::Void),
        // T21: Observe namespace methods return Void (namespace-only).
        // The codegen splices `buff_observe::*::new(...)` / `Tracer::bootstrap()`
        // directly — the return values are consumed by the generated Rust
        // and never need a Buff Type variant for the MVP.
        (PreludeType::Observe, PreludeAssocFn::Span) => Some(Type::Void),
        (PreludeType::Observe, PreludeAssocFn::Counter) => Some(Type::Void),
        (PreludeType::Observe, PreludeAssocFn::Histogram) => Some(Type::Void),
        (PreludeType::Observe, PreludeAssocFn::Gauge) => Some(Type::Void),
        (PreludeType::Observe, PreludeAssocFn::Bootstrap) => Some(Type::Void),
        // T20: ReactiveSignal / ReactiveComputed / ReactiveEffect
        // assoc fns. Each ctor returns the matching runtime value
        // (modeled as Type::Unknown — the coordinated
        // Type::ReactiveSignal / ReactiveComputed / ReactiveEffect
        // variants in ty.rs are follow-up sibling tasks OUTSIDE the
        // T20 shared zone, mirroring the T8 Tensor / T11 Signal-DSP
        // forward-declaration precedent). The codegen-lowered
        // `buff_reactive::Signal::new(v)` /
        // `buff_reactive::Computed::new(f)` /
        // `buff_reactive::Effect::new(f)` calls splice the path
        // directly; the Buff-side type check accepts the Unknown
        // return so `buff check` validates the syntax.
        (PreludeType::ReactiveSignal, PreludeAssocFn::New) => Some(Type::Unknown),
        (PreludeType::ReactiveComputed, PreludeAssocFn::New) => Some(Type::Unknown),
        (PreludeType::ReactiveEffect, PreludeAssocFn::New) => Some(Type::Unknown),
        // T17: Web assoc fns. Both ctors return Web (modeled as
        // Type::Unknown for MVP - the coordinated Type::Web variant
        // in ty.rs is a follow-up sibling task outside the T17 shared
        // zone, mirroring the T8 Tensor / T11 Signal / T12-Tensor
        // forward-declaration precedent). The codegen-lowered
        // `buff_web::Web::new()` / `buff_web::Web::bind(addr)` calls
        // splice the path directly; the Buff-side type check accepts
        // the Unknown return so `buff check` validates the syntax.
        //
        // `Web.new()` -> Web. Zero args. Wraps `buff_web::Web::new()`
        // (infallible - returns an empty Web with no routes / no
        // middleware / no bind addr).
        (PreludeType::Web, PreludeAssocFn::New) => Some(Type::Unknown),
        // `Web.bind(addr)` -> Web. One arg (String). Wraps
        // `buff_web::Web::bind(addr)` (infallible - returns an empty
        // Web with the bind addr preset; the user adds routes via
        // web.get / web.post / ... and starts serving via web.run()).
        (PreludeType::Web, PreludeAssocFn::Bind) => Some(Type::Unknown),
        // T18: Database.connect(url) -> Pool (forward-declared as
        // Type::Unknown). Wraps `buff_db::Pool::connect(&url).await?`
        // (the `?` propagates DbError per Buff's R3 error-mapping
        // contract — the Buff user's surrounding fn must return
        // `Result<T, DbError>` so `?` can splice cleanly; the Buff
        // `?` operator is the standard error-propagation idiom,
        // mirroring `regex::Regex::new(p)?` from T124d and
        // `buff_image::Image::from_path(p)?` from T9).
        //
        // The `Connect` variant is shared with TCP.connect /
        // WebSocket.connect (existing variants — same name,
        // different per-type lowering, dispatched on the
        // (Database, Connect) pair). The codegen lowering emits the
        // async call via `.await` (Buff has no `await` keyword — the
        // codegen auto-inserts it when the surrounding fn is async
        // per the T31 async-propagation path). Records `buff-db` +
        // `sqlx` + `tokio` in codegen `extern_crates` (mirrors the
        // chrono / regex / tracing codegen-only linking boundary).
        (PreludeType::Database, PreludeAssocFn::Connect) => Some(Type::Unknown),
        // T33: HttpClient.new() -> HttpClient. Zero args. Wraps
        // `buff_http_client::HttpClient::new()` (infallible - returns
        // a new client with default settings). Returns the concrete
        // `Type::HttpClient` variant (unlike Web / Database which
        // forward-declare as Type::Unknown — HttpClient has a proper
        // Type variant in this same T33 commit).
        (PreludeType::HttpClient, PreludeAssocFn::New) => Some(Type::HttpClient),
        // T31: Cache.new(max_capacity) -> Cache. One arg (Int).
        // Wraps `buff_cache::Cache::new(max_capacity)?` (the `?`
        // propagates CacheError::InvalidCapacity per Buff's R3
        // error-mapping contract — zero capacity rejected). Returns
        // the concrete `Type::Cache` variant.
        (PreludeType::Cache, PreludeAssocFn::New) => Some(Type::Cache),
        // T29: Validator.new() -> Validator. Zero args. Wraps
        // `buff_validate::Validator::new()` (infallible - returns an
        // empty rule set). Returns the concrete `Type::Validator`
        // variant. The 5 builder methods (with_email / with_url /
        // with_length / with_range / with_regex) and 2 action methods
        // (validate / to_json_schema) are instance fns (see below).
        (PreludeType::Validator, PreludeAssocFn::New) => Some(Type::Validator),
        // T42: Email.new(from, to, subject) -> Email. Three args
        // (String from, String to, String subject). Wraps
        // `buff_email::Email::new(from, to, subject)?` (the `?`
        // propagates EmailError::InvalidAddress per Buff's R3
        // error-mapping contract). Returns the concrete `Type::Email`
        // variant. The 3 builder methods (body / html / attach) are
        // instance fns (see below).
        (PreludeType::Email, PreludeAssocFn::New) => Some(Type::Email),
        // T42: SmtpClient.new(host, port, username, password) ->
        // SmtpClient. Four args (String host, Int port, String
        // username, String password). Wraps
        // `buff_email::SmtpClient::new(host, port as u16, user,
        // pass)?` (the `?` propagates EmailError::InvalidRelay).
        // Returns the concrete `Type::SmtpClient` variant. The 1
        // action method (send) is an instance fn (see below).
        (PreludeType::SmtpClient, PreludeAssocFn::New) => Some(Type::SmtpClient),
        // T30: Config module — namespace-only (no runtime value). The
        // assoc fns return Void (set_default / load_file / load_env /
        // load_args / watch) or Option<Int> / Option<Float> / Option<Bool>
        // (get_int / get_float / get_bool) or Option<String> (get — shared
        // with Args.get / Env.get). `Config.new()` returns Void (the
        // namespace itself is never a value; the codegen creates a
        // `buff_config::Config` internally and splices method calls on it).
        (PreludeType::Config, PreludeAssocFn::New) => Some(Type::Void),
        (PreludeType::Config, PreludeAssocFn::SetDefault) => Some(Type::Void),
        (PreludeType::Config, PreludeAssocFn::LoadFile) => Some(Type::Void),
        (PreludeType::Config, PreludeAssocFn::LoadEnv) => Some(Type::Void),
        (PreludeType::Config, PreludeAssocFn::LoadArgs) => Some(Type::Void),
        (PreludeType::Config, PreludeAssocFn::Get) => Some(Type::option(Type::string())),
        (PreludeType::Config, PreludeAssocFn::GetInt) => Some(Type::option(Type::int_default())),
        (PreludeType::Config, PreludeAssocFn::GetFloat) => {
            Some(Type::option(Type::float_default()))
        }
        (PreludeType::Config, PreludeAssocFn::GetBool) => Some(Type::option(Type::bool())),
        (PreludeType::Config, PreludeAssocFn::Watch) => Some(Type::Void),
        // T34: buff-auth assoc fns. The 4 (type, method) pairs below
        // cover the MVP surface that ships with codegen lowering:
        // JWT.encode / JWT.decode / Password.hash / Password.verify.
        // The OAuth2Client + Rbac instance methods
        // (authorization_url / exchange_code / enforce) are deferred
        // to the sibling task that adds Type::OAuth2Client / Type::Rbac
        // (mirrors the T17 Web / T18 Database forward-declaration
        // precedent) — their PreludeAssocFn variants are reserved here
        // so the sibling task wires them without enum churn.
        //
        // Return types:
        // - JWT.encode -> String (the compact JWS token).
        // - JWT.decode -> Map<String, Unknown> (heterogeneous claims).
        // - Password.hash -> String (Argon2id PHC form).
        // - Password.verify -> Bool (Ok(false) on mismatch — NEVER errors).
        (PreludeType::Jwt, PreludeAssocFn::Encode) => Some(Type::string()),
        (PreludeType::Jwt, PreludeAssocFn::Decode) => {
            Some(Type::map(Type::string(), Type::Unknown))
        }
        (PreludeType::Password, PreludeAssocFn::PasswordHash) => Some(Type::string()),
        (PreludeType::Password, PreludeAssocFn::PasswordVerify) => Some(Type::bool()),
        // T39: Archive namespace methods (2). Both are side-effecting
        // filesystem operations that write to / read from disk; the
        // Buff surface types them as Void (the user re-reads the
        // result via the filesystem, mirroring the Log / Toml /
        // Config / Observe namespace-only pattern). The codegen
        // lowering splices `buff_archive::Archive::compress_dir(...)
        // ?` / `buff_archive::Archive::extract(...)?` (the `?`
        // propagates `ArchiveError` per Buff's R3 error-mapping
        // contract — the surrounding fn must return
        // `Result<T, ArchiveError>`).
        (PreludeType::Archive, PreludeAssocFn::CompressDir) => Some(Type::Void),
        (PreludeType::Archive, PreludeAssocFn::Extract) => Some(Type::Void),
        // T46: Text namespace methods (4). detect_language returns
        // Option<String> (the ISO 639-3 code; None when no language
        // detected). stem returns String (the lowered stem).
        // tokenize / sentences return Vector<String>. The codegen
        // lowering splices:
        //   - `buff_nlp::Text::detect_language(&text)`
        //     (returns Option<Language> directly — no mapping needed;
        //     the whatlang wrapper already produces a buff_nlp::Language)
        //   - `buff_nlp::Text::stem(&word,
        //     buff_nlp::StemAlgorithm::from_code(&algorithm)
        //     .unwrap_or(buff_nlp::StemAlgorithm::English))?`
        //     (the `?` propagates `NlpError` per Buff's R3 error-
        //     mapping contract; unknown algorithm names fall back to
        //     English — defensive, never silently corrupts).
        //   - `buff_nlp::Text::tokenize(&text)`
        //   - `buff_nlp::Text::sentences(&text)`
        (PreludeType::Text, PreludeAssocFn::DetectLanguage) => Some(Type::option(Type::language())),
        (PreludeType::Text, PreludeAssocFn::Stem) => Some(Type::string()),
        (PreludeType::Text, PreludeAssocFn::Tokenize) => Some(Type::vector(Type::string())),
        (PreludeType::Text, PreludeAssocFn::Sentences) => Some(Type::vector(Type::string())),
        // T51: MsgPack assoc fns. `MsgPack.serialize(value)` -> Bytes
        // (Vector<Byte>). Wraps `buff_msgpack::serialize(&value)
        // .unwrap_or_default()` (empty Vec on failure — NEVER panics).
        // `MsgPack.deserialize(bytes)` -> dynamic Value (typed
        // `Type::Unknown` at the Buff layer — there is no surface
        // JsonValue variant; mirrors how Random.choice / Shuffle
        // model dynamic returns). `MsgPack.roundtrip(value)` ->
        // Option<Value> (None on either step failing).
        (PreludeType::MsgPack, PreludeAssocFn::Serialize) => Some(Type::vector(Type::byte())),
        (PreludeType::MsgPack, PreludeAssocFn::Deserialize) => Some(Type::Unknown),
        (PreludeType::MsgPack, PreludeAssocFn::Roundtrip) => Some(Type::option(Type::Unknown)),
        // T43: buff-scrape assoc fns. Two pairs cover the MVP surface.
        // `Document.from_html(html)` -> Document. One arg (String).
        // Wraps `buff_scrape::Document::from_html(&html)?` (the `?`
        // propagates ScrapeError::EmptyInput per R3).
        // `Crawler.new(seed_url)` -> Crawler. One arg (String).
        // Wraps `buff_scrape::Crawler::new(&seed)?` (the `?`
        // propagates ScrapeError::EmptyInput per R3). Codegen lowers
        // to `unwrap_or_default()` for the panic-free guarantee
        // (Crawler impls Default as an about:blank-seeded client).
        (PreludeType::Document, PreludeAssocFn::FromHtml) => Some(Type::Document),
        (PreludeType::Crawler, PreludeAssocFn::New) => Some(Type::Crawler),
        // T50: Xml.from_str(xml) -> XmlDocument. One arg (String).
        // Wraps `buff_xml::XmlDocument::from_str(&xml)?` (the `?`
        // propagates XmlError::EmptyInput per Buff's R3 error-mapping
        // contract). Returns the opaque Type::Xml variant; the codegen
        // layer maps it to `buff_xml::XmlDocument`.
        (PreludeType::Xml, PreludeAssocFn::FromStr) => Some(Type::Xml),
        // T50: XmlElement.new(name, text, attrs) -> XmlElement. Three
        // args (String, String, Map<String,String>). Wraps
        // `buff_xml::XmlElement::new(&name, &text, attrs_vec)` (the
        // codegen inserts `.into_iter().collect()` on the attrs arg
        // to satisfy the `Vec<(String, String)>` signature — works
        // for any IntoIterator yielding `(String, String)`). Returns
        // the opaque Type::XmlElement variant.
        (PreludeType::XmlElement, PreludeAssocFn::New) => Some(Type::XmlElement),
        // T45: buff-geo constructors. Point.new is infallible (wraps
        // `buff_geo::Point::new(x, y)` directly). LineString.new /
        // LineString.from_coords / Polygon.new / Polygon.from_coords
        // are fallible in Rust (return `Result<_, GeoError>`) but
        // surface as infallible on the Buff side via codegen's
        // `unwrap_or_default()` (LineString / Polygon impl Default).
        // `New` is shared with Channel.new / Faker.new / Crawler.new
        // (dispatched on (Point, New) / (LineString, New) /
        // (Polygon, New) pairs). `FromCoords` is geo-only.
        (PreludeType::Point, PreludeAssocFn::New) => Some(Type::Point),
        (PreludeType::LineString, PreludeAssocFn::New) => Some(Type::LineString),
        (PreludeType::LineString, PreludeAssocFn::FromCoords) => Some(Type::LineString),
        (PreludeType::Polygon, PreludeAssocFn::New) => Some(Type::Polygon),
        (PreludeType::Polygon, PreludeAssocFn::FromCoords) => Some(Type::Polygon),
        // T54: buff-simd constructors. `Simd.splat(x)` is infallible
        // (wraps `buff_simd::Simd::splat(x)` directly). `Simd.from_slice
        // (slice)` is fallible in Rust (returns `Result<_, SimdError>`)
        // but surfaces as infallible on the Buff side via codegen's
        // `unwrap_or_default()` (Simd impls Default as `splat(0.0)`).
        // `Simd.from_array(arr)` is infallible. All three return `Simd`.
        (PreludeType::Simd, PreludeAssocFn::Splat) => Some(Type::Simd),
        (PreludeType::Simd, PreludeAssocFn::FromSlice) => Some(Type::Simd),
        (PreludeType::Simd, PreludeAssocFn::FromArray) => Some(Type::Simd),
        // T24: File I/O assoc fns. File is namespace-only (returns Void
        // for the type itself). The assoc fns return String (read),
        // Void (write/append), or Bool (exists). The codegen lowers to
        // std::fs::* with `.unwrap_or_default()` so the Buff surface is
        // always infallible — NEVER panics, matching Buff's "no panicking
        // generated code" rule.
        //
        // `File.read(path)` -> String. Wraps
        // `std::fs::read_to_string(p).unwrap_or_default()`.
        (PreludeType::File, PreludeAssocFn::Read) => Some(Type::string()),
        // `File.write(path, content)` -> Void. Wraps
        // `std::fs::write(p, c).unwrap_or_default()`.
        (PreludeType::File, PreludeAssocFn::Write) => Some(Type::Void),
        // `File.exists(path)` -> Bool. Wraps
        // `std::path::Path::new(p).exists()`.
        (PreludeType::File, PreludeAssocFn::Exists) => Some(Type::Bool),
        // `File.append(path, content)` -> Void. Wraps
        // `std::fs::OpenOptions::new().append(true).open(p)
        // .and_then(|mut f| std::io::Write::write_all(&mut f, c.as_bytes()))
        // .unwrap_or_default()`.
        (PreludeType::File, PreludeAssocFn::Append) => Some(Type::Void),
        // T52: Protobuf assoc fns. `Protobuf.serialize(value)` -> Bytes
        // (Vector<Byte>). Wraps `buff_protobuf::serialize(&value)
        // .unwrap_or_default()` (empty Vec on failure — NEVER panics).
        // `Protobuf.deserialize(bytes)` -> dynamic Value (typed
        // `Type::Unknown` at the Buff layer — there is no surface
        // JsonValue variant; mirrors how MsgPack.deserialize / Random
        // .choice model dynamic returns). `Protobuf.roundtrip(value)`
        // -> Option<Value> (None on either step failing). Mirrors T51
        // MsgPack assoc fns 1:1 — the runtime surface is identical.
        (PreludeType::Protobuf, PreludeAssocFn::Serialize) => Some(Type::vector(Type::byte())),
        (PreludeType::Protobuf, PreludeAssocFn::Deserialize) => Some(Type::Unknown),
        (PreludeType::Protobuf, PreludeAssocFn::Roundtrip) => Some(Type::option(Type::Unknown)),
        // T52: Message constructors. `Message.new(value)` -> Message
        // (encode a Value to protobuf wire-format bytes). `Message.
        // from_bytes(bytes)` / `Message.decode(bytes)` -> Message
        // (decode raw wire-format bytes). All three are fallible in
        // Rust (return `Result<Message, ProtobufError>`) but surface
        // as infallible on the Buff side via codegen's
        // `unwrap_or_default()` (Message impls Default as an
        // empty-payload message — added in the T52 MVP commit).
        // `New` is shared with Channel.new / Faker.new / Crawler.new
        // / Point.new / XmlElement.new (dispatched on the
        // (Message, New) pair). `FromBytes` is shared with
        // Image.from_bytes (dispatched on (Message, FromBytes)).
        // `Decode` is shared with Base64.decode / Hex.decode /
        // URLEncode.decode (dispatched on (Message, Decode)).
        (PreludeType::Message, PreludeAssocFn::New) => Some(Type::Message),
        (PreludeType::Message, PreludeAssocFn::FromBytes) => Some(Type::Message),
        (PreludeType::Message, PreludeAssocFn::Decode) => Some(Type::Message),
        // T47: buff-chat constructors. `Bot.new(platform, token)` is
        // fallible in Rust (returns `Result<Bot, ChatError>`) but
        // surfaces as infallible on the Buff side via codegen's
        // `unwrap_or_default()` (Bot impls Default as an empty Discord
        // bot — added in the T47 MVP commit). `ChatMessage.new(text,
        // channel, author, platform, is_dm)` is infallible in Rust
        // (returns `Message` directly — no failure mode). `New` is
        // shared with Channel.new / Faker.new / Point.new /
        // XmlElement.new / Message.new (T52) — dispatched on the
        // (Bot, New) / (ChatMessage, New) pairs.
        (PreludeType::Bot, PreludeAssocFn::New) => Some(Type::bot()),
        (PreludeType::ChatMessage, PreludeAssocFn::New) => Some(Type::chat_message()),
        // T48: buff-web3 constructors.
        //
        // `Provider.new(rpc_url)` is fallible in Rust (returns
        // `Result<Provider, Web3Error>`) but surfaces as infallible on
        // the Buff side via codegen's `.unwrap_or_default()` (Provider
        // impls Default as a localhost-pointed no-op provider — added
        // in the T48 MVP commit). `New` is shared with Channel.new /
        // Faker.new / Bot.new / ChatMessage.new / Point.new /
        // XmlElement.new / Message.new (T52) — dispatched on the
        // (Provider, New) / (Contract, New) pairs.
        //
        // `Contract.new(address, abi_json, client)` is also fallible
        // (returns `Result<Contract, Web3Error>`) but surfaces as
        // infallible via `.unwrap_or_default()` (Contract impls Default
        // as a zero-address + empty-ABI + read-only contract).
        //
        // `Wallet.from_private_key(key)` is the Wallet-only ctor (new
        // variant `FromPrivateKey` — Wallet-specific). Also fallible
        // but surfaces as infallible via `.unwrap_or_default()` (Wallet
        // impls Default as a "burner" wallet).
        //
        // `Wallet.connect(provider)` is the Wallet → ConnectedWallet
        // transform. Infallible in Rust (returns ConnectedWallet
        // directly — no failure mode). Reuses the existing shared
        // `Connect` variant (TCP.connect / WebSocket.connect) —
        // dispatched on the (Wallet, Connect) pair.
        (PreludeType::Provider, PreludeAssocFn::New) => Some(Type::provider()),
        (PreludeType::Contract, PreludeAssocFn::New) => Some(Type::contract()),
        (PreludeType::Wallet, PreludeAssocFn::FromPrivateKey) => Some(Type::wallet()),
        // `Wallet.connect(provider)` is an INSTANCE method on a Wallet
        // value (not a Type.method assoc fn like TCP.connect) — see the
        // (Type::Wallet, PreludeInstanceFn::Connect) arm in
        // `instance_fn_return_type` below.
        // T49: buff-crypto-extras assoc fn return types. Most return
        // `Vector<Byte>` (raw bytes — keys / nonces / ciphertexts /
        // signatures / shared secrets / salts / derived keys); Verify
        // returns Bool (false on any failure, mirrors T26 / T34);
        // GenerateKeypair returns RsaKeypair (the single instance type
        // in T49). All panic-free via the codegen's `unwrap_or_default`
        // / `.unwrap_or(false)` collapse.
        //
        // AES (4): generate_key / generate_nonce / encrypt / decrypt.
        // Encrypt / Decrypt wrap the underlying Result<Vec<u8>,
        // CryptoError> via `.unwrap_or_default()` (empty Vec on
        // failure — NEVER panics). GenerateKey / GenerateNonce are
        // infallible (return Vec<u8> directly).
        (PreludeType::AES, PreludeAssocFn::GenerateKey) => Some(Type::vector(Type::byte())),
        (PreludeType::AES, PreludeAssocFn::GenerateNonce) => Some(Type::vector(Type::byte())),
        (PreludeType::AES, PreludeAssocFn::Encrypt) => Some(Type::vector(Type::byte())),
        (PreludeType::AES, PreludeAssocFn::Decrypt) => Some(Type::vector(Type::byte())),
        // RSA (3): generate_keypair / sign / verify. GenerateKeypair
        // returns the RsaKeypair instance type (constructed ONLY via
        // this fn — no other path). Sign returns raw signature bytes
        // (256 bytes for 2048-bit modulus). Verify returns Bool (false
        // on any failure — mirrors T26 Signature.verify + T34
        // Password.verify stance).
        (PreludeType::RSA, PreludeAssocFn::GenerateKeypair) => Some(Type::rsa_keypair()),
        (PreludeType::RSA, PreludeAssocFn::Sign) => Some(Type::vector(Type::byte())),
        (PreludeType::RSA, PreludeAssocFn::Verify) => Some(Type::bool()),
        // ECDH (3): generate_private / public_from_private /
        // derive_shared. GeneratePrivate is infallible (returns Vec
        // directly). PublicFromPrivate / DeriveShared wrap the
        // underlying Result<Vec<u8>, CryptoError> via
        // `.unwrap_or_default()` (empty Vec on failure — NEVER
        // panics).
        (PreludeType::ECDH, PreludeAssocFn::GeneratePrivate) => Some(Type::vector(Type::byte())),
        (PreludeType::ECDH, PreludeAssocFn::PublicFromPrivate) => Some(Type::vector(Type::byte())),
        (PreludeType::ECDH, PreludeAssocFn::DeriveShared) => Some(Type::vector(Type::byte())),
        // Argon2 (2): generate_salt / derive_key. GenerateSalt is
        // infallible (returns Vec directly). DeriveKey wraps the
        // underlying Result<Vec<u8>, CryptoError> via
        // `.unwrap_or_default()` (empty Vec on failure — NEVER
        // panics).
        (PreludeType::Argon2, PreludeAssocFn::GenerateSalt) => Some(Type::vector(Type::byte())),
        (PreludeType::Argon2, PreludeAssocFn::DeriveKey) => Some(Type::vector(Type::byte())),
        // Every other (type, method) pair is invalid. Returning None lets
        // the caller fall back to the default "user method" path so a
        // future extension doesn't silently swallow unrecognised calls.
        _ => None,
    }
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
        // `df.to_table_string()` -> String. Zero-arg fixed-width
        // pretty-printer (infallible — returns String directly).
        (Type::DataFrame, PreludeInstanceFn::ToTableString) => Some(Type::string()),

        // T9: Image instance methods. Each filter returning a new
        // Image (grayscale / resize / crop / blur) returns Type::Image
        // so the user can chain `img.grayscale().resize(50, 50)`. The
        // accessors (width / height) return Int (Buff's Int<64>);
        // get_pixel returns Unknown (no Buff Color type variant yet);
        // set_pixel / save / invert return Void (in-place or
        // panic-free discarded Result). All methods panic-free at the
        // codegen layer via `unwrap_or_default()` (Image impls
        // Default as a 1x1 transparent pixel — added in the same T9
        // finish commit as this registry entry).
        //
        // `img.width()` -> Int. Wraps `recv.width() as i64`.
        (Type::Image, PreludeInstanceFn::Width) => Some(Type::int_default()),
        // `img.height()` -> Int. Wraps `recv.height() as i64`.
        (Type::Image, PreludeInstanceFn::Height) => Some(Type::int_default()),
        // `img.pixel_format()` -> PixelFormat. Zero args. Returns
        // Type::Unknown at this layer (Buff has no surface PixelFormat
        // type variant; codegen emits `recv.format()` and Rust infers
        // `buff_image::PixelFormat`). Renamed from `format` on the
        // Buff surface to avoid a clash with DateTime.format.
        (Type::Image, PreludeInstanceFn::PixelFormat) => Some(Type::Unknown),
        // `img.get_pixel(x, y)` -> Color. Two args. Bounds-checked;
        // returns Color (Type::Unknown — no Buff Color variant).
        (Type::Image, PreludeInstanceFn::GetPixel) => Some(Type::Unknown),
        // `img.set_pixel(x, y, color)` -> Void. In-place mutation.
        (Type::Image, PreludeInstanceFn::SetPixel) => Some(Type::Void),
        // `img.save(path)` -> Void. Writes to disk. Shared `Save`
        // variant — dispatched on (Image, Save) pair (the codegen
        // arm handles receiver-type dispatch).
        (Type::Image, PreludeInstanceFn::Save) => Some(Type::Void),
        // `img.grayscale()` -> Image. Rec. 601 luma. Chainable.
        (Type::Image, PreludeInstanceFn::Grayscale) => Some(Type::Image),
        // `img.invert()` -> Void. In-place channel inversion.
        (Type::Image, PreludeInstanceFn::Invert) => Some(Type::Void),
        // `img.resize(w, h)` -> Image. Lanczos3. Chainable.
        (Type::Image, PreludeInstanceFn::Resize) => Some(Type::Image),
        // `img.crop(x, y, w, h)` -> Image. Bounds-checked. Chainable.
        (Type::Image, PreludeInstanceFn::Crop) => Some(Type::Image),
        // `img.blur(sigma)` -> Image. Gaussian. Chainable.
        (Type::Image, PreludeInstanceFn::Blur) => Some(Type::Image),

        // T37: Faker instance methods. All infallible at the codegen
        // layer (no unwrap_or_default needed — the buff_fake methods
        // return owned String / i64 directly). `name` / `email` /
        // `address` / `phone` / `uuid` / `lorem` return String.
        // `int` returns Int (Buff's Int<64>). `datetime` returns
        // String (RFC 3339).
        //
        // `faker.name()` -> String. Random full name.
        (Type::Faker, PreludeInstanceFn::Name) => Some(Type::String),
        // `faker.email()` -> String. Random email address.
        (Type::Faker, PreludeInstanceFn::Email) => Some(Type::String),
        // `faker.address()` -> String. Random street address.
        (Type::Faker, PreludeInstanceFn::Address) => Some(Type::String),
        // `faker.phone()` -> String. Random phone number.
        (Type::Faker, PreludeInstanceFn::Phone) => Some(Type::String),
        // `faker.uuid()` -> String. Random UUID v4.
        (Type::Faker, PreludeInstanceFn::Uuid) => Some(Type::String),
        // `faker.lorem(words)` -> String. Lorem ipsum with N words.
        (Type::Faker, PreludeInstanceFn::Lorem) => Some(Type::String),
        // `faker.int(min, max)` -> Int. Random int in [min, max].
        (Type::Faker, PreludeInstanceFn::FakerInt) => Some(Type::int_default()),
        // `faker.datetime(start, end)` -> String. RFC 3339 datetime.
        (Type::Faker, PreludeInstanceFn::FakerDatetime) => Some(Type::String),

        // T10: AudioBuffer instance methods. The accessors (samples /
        // sample_rate / channels / frames / duration_secs) return
        // Vec<Float> / Int / Int / Int / Float respectively. The
        // in-place ops (amplify / normalize / mix) return Void. The
        // chainable slice returns AudioBuffer. summarize returns
        // Unknown (no Buff AudioSummary variant). All panic-free at
        // the codegen layer (slice via `unwrap_or_default()`;
        // AudioBuffer impls Default as empty 44100Hz mono — added in
        // the same T10 finish commit as this registry entry).
        //
        // `buf.samples()` -> Vector<Float>. Owned copy of the
        // interleaved sample slice.
        (Type::Audio, PreludeInstanceFn::Samples) => Some(Type::vector(Type::float_default())),
        // `buf.sample_rate()` -> Int. Hz.
        (Type::Audio, PreludeInstanceFn::SampleRate) => Some(Type::int_default()),
        // `buf.channels()` -> Int. >= 1.
        (Type::Audio, PreludeInstanceFn::Channels) => Some(Type::int_default()),
        // `buf.frames()` -> Int. samples.len() / channels.
        (Type::Audio, PreludeInstanceFn::Frames) => Some(Type::int_default()),
        // `buf.duration_secs()` -> Float. frames / sample_rate.
        (Type::Audio, PreludeInstanceFn::DurationSecs) => Some(Type::float_default()),
        // `buf.amplify(factor)` -> Void. In-place scale.
        (Type::Audio, PreludeInstanceFn::Amplify) => Some(Type::Void),
        // `buf.normalize(target)` -> Void. In-place peak-normalize.
        (Type::Audio, PreludeInstanceFn::Normalize) => Some(Type::Void),
        // `buf.mix(other)` -> Void. Sample-wise add. Discards the
        // Result (rate/channel mismatch is a no-op via unwrap_or_default).
        (Type::Audio, PreludeInstanceFn::Mix) => Some(Type::Void),
        // `buf.slice(start_sec, end_sec)` -> AudioBuffer. Chainable.
        (Type::Audio, PreludeInstanceFn::Slice) => Some(Type::Audio),
        // `buf.summarize()` -> AudioSummary. Type::Unknown at this
        // layer (no Buff AudioSummary variant; codegen emits the call
        // and Rust infers `buff_audio::AudioSummary`).
        (Type::Audio, PreludeInstanceFn::Summarize) => Some(Type::Unknown),
        // Shared `Save` variant dispatched on (Audio, Save) pair
        // (mirrors Format shared between DateTime / Date / Time).
        // `buf.save(path)` -> Void. WAV encode.
        (Type::Audio, PreludeInstanceFn::Save) => Some(Type::Void),

        // T20: Reactive instance methods. Dispatched on
        // (Type::Unknown, Method) because the coordinated
        // Type::ReactiveSignal / ReactiveComputed / ReactiveEffect
        // variants in ty.rs are follow-up sibling tasks OUTSIDE the
        // T20 shared zone (mirrors the T17 Web forward-declaration
        // precedent). When type inference resolves the receiver to
        // Type::Unknown (e.g. `let s = ReactiveSignal.new(10)` whose
        // assoc fn returns Type::Unknown), the dispatcher matches
        // here and the codegen emits `recv.method(args)` directly.
        // Rust's method resolution then finds the matching
        // `buff_reactive::Signal::get` / `set` / `update` /
        // `Computed::get` / `Effect::run` method.
        //
        // `s.get()` -> T. Zero args. Element type Unknown.
        (Type::Unknown, PreludeInstanceFn::Get) => Some(Type::Unknown),
        // `s.set(value)` -> Void. One arg (T).
        (Type::Unknown, PreludeInstanceFn::Set) => Some(Type::Void),
        // `s.update(fn)` -> Void. One arg (`Fn(&mut T) -> Void`).
        (Type::Unknown, PreludeInstanceFn::Update) => Some(Type::Void),
        // `c.invalidate()` -> Void. Zero args. Manually clear cache.
        (Type::Unknown, PreludeInstanceFn::Invalidate) => Some(Type::Void),

        // T29: Validator instance methods.
        // `validator.with_email(field)` -> Validator. Builder (consume self).
        (Type::Validator, PreludeInstanceFn::WithEmail) => Some(Type::Validator),
        // `validator.with_url(field)` -> Validator. Builder.
        (Type::Validator, PreludeInstanceFn::WithUrl) => Some(Type::Validator),
        // `validator.with_length(field, min, max)` -> Validator. Builder.
        (Type::Validator, PreludeInstanceFn::WithLength) => Some(Type::Validator),
        // `validator.with_range(field, min, max)` -> Validator. Builder.
        (Type::Validator, PreludeInstanceFn::WithRange) => Some(Type::Validator),
        // `validator.with_regex(field, pattern)` -> Validator. Builder.
        (Type::Validator, PreludeInstanceFn::WithRegex) => Some(Type::Validator),
        // `validator.validate(input)` -> Result<Void, String>. Action.
        // Wraps `recv.validate(&input).map_err(|e| e.to_string())`.
        (Type::Validator, PreludeInstanceFn::Validate) => {
            Some(Type::result(Type::Void, Type::String))
        }
        // `validator.to_json_schema()` -> String. Action.
        (Type::Validator, PreludeInstanceFn::ToJsonSchema) => Some(Type::String),

        // T42: Email instance methods. All three builder methods
        // consume self and return a new Email (Buff "no visible
        // references" stance — mirrors Validator with_*). Dispatched
        // on (Type::Email, variant) pairs. Each lowers to the
        // matching `buff_email::Email::{body, html, attach}` method.
        // `email.body(text)` -> Email. One arg (String plain body).
        (Type::Email, PreludeInstanceFn::Body) => Some(Type::Email),
        // `email.html(template, context_json)` -> Email. Two args
        // (String handlebars template, String JSON context). Renders
        // via `handlebars::Handlebars::render` then stores as the
        // HTML body.
        (Type::Email, PreludeInstanceFn::Html) => Some(Type::Email),
        // `email.attach(path)` -> Email. One arg (String path).
        // Queues a file attachment for read+encode at send time.
        (Type::Email, PreludeInstanceFn::Attach) => Some(Type::Email),

        // T42: SmtpClient instance method. The single send method is
        // dispatched on (Type::SmtpClient, Send) — shares the Send
        // variant with TCP / WebSocket / Sender. Returns Void (the
        // Buff codegen discards the Result via unwrap_or_default
        // panic-free — invalid email / SMTP failure is a no-op at
        // the Buff surface, matching the Image save / Cache set
        // precedent).
        // `client.send(email)` -> Void. One arg (Email). Wraps
        // `recv.send(&email).unwrap_or_default()`.
        (Type::SmtpClient, PreludeInstanceFn::Send) => Some(Type::Void),

        // T31: Cache instance methods. All 7 dispatched on
        // (Type::Cache, variant) pairs. Get returns Option<String>
        // (Buff String? surface); the rest are Void / Bool / Int.
        // `cache.get(key)` -> String? (None if missing or expired).
        (Type::Cache, PreludeInstanceFn::Get) => Some(Type::option(Type::String)),
        // `cache.set(key, value)` -> Void. Two args.
        (Type::Cache, PreludeInstanceFn::Set) => Some(Type::Void),
        // `cache.set(key, value, ttl)` -> Void. Three args (the
        // arity-dispatched overload of Set).
        (Type::Cache, PreludeInstanceFn::SetTtl) => Some(Type::Void),
        // `cache.delete(key)` -> Void.
        (Type::Cache, PreludeInstanceFn::Delete) => Some(Type::Void),
        // `cache.contains(key)` -> Bool. Expiry-aware.
        (Type::Cache, PreludeInstanceFn::Contains) => Some(Type::Bool),
        // `cache.clear()` -> Void.
        (Type::Cache, PreludeInstanceFn::Clear) => Some(Type::Void),
        // `cache.len()` -> Int. Approximate entry count.
        (Type::Cache, PreludeInstanceFn::Len) => Some(Type::int_default()),
        // T44 MVP: I18n instance methods. All 3 dispatched on
        // (Type::I18n, variant) pairs. AddResource / Load are Void
        // (panic-free `unwrap_or(())` in codegen); Translate returns
        // String (current → fallback → key string contract).
        (Type::I18n, PreludeInstanceFn::AddResource) => Some(Type::Void),
        (Type::I18n, PreludeInstanceFn::Load) => Some(Type::Void),
        (Type::I18n, PreludeInstanceFn::Translate) => Some(Type::String),

        // T43: buff-scrape instance methods. The 10 pairs below cover
        // the full MVP surface: 4 Document + 5 Element + 4 Crawler,
        // reusing shared `Select` + `Html` variants (DataFrame-owned
        // + Email-owned respectively) for the cross-type overlap.
        // All panic-free at the codegen layer (Document/Element are
        // owned wrappers — no `?` needed; Crawler methods lower to
        // `unwrap_or_default()` / direct call per the panic-free
        // codegen contract).
        //
        // Document instance methods (4):
        // `doc.select(css)` -> Vector<Element>. One arg (String).
        // Shared `Select` variant dispatched on (Document, Select).
        (Type::Document, PreludeInstanceFn::Select) => Some(Type::vector(Type::Element)),
        // `doc.text()` -> String. Zero args.
        (Type::Document, PreludeInstanceFn::Text) => Some(Type::String),
        // `doc.html()` -> String. Zero args. Shared `Html` variant
        // dispatched on (Document, Html) — distinct lowering from
        // the (Email, Html) builder (zero-arg accessor vs. two-arg
        // template renderer).
        (Type::Document, PreludeInstanceFn::Html) => Some(Type::String),
        // `doc.title()` -> String?. Zero args.
        (Type::Document, PreludeInstanceFn::Title) => Some(Type::option(Type::String)),
        // Element instance methods (5):
        // `el.select(css)` -> Vector<Element>. One arg (String).
        // Shared `Select` dispatched on (Element, Select).
        (Type::Element, PreludeInstanceFn::Select) => Some(Type::vector(Type::Element)),
        // `el.text()` -> String. Zero args.
        (Type::Element, PreludeInstanceFn::Text) => Some(Type::String),
        // `el.attr(name)` -> String?. One arg (String).
        (Type::Element, PreludeInstanceFn::Attr) => Some(Type::option(Type::String)),
        // `el.html()` -> String. Zero args. Shared `Html` dispatched
        // on (Element, Html).
        (Type::Element, PreludeInstanceFn::Html) => Some(Type::String),
        // `el.inner_html()` -> String. Zero args.
        (Type::Element, PreludeInstanceFn::InnerHtml) => Some(Type::String),
        // Crawler instance methods (4):
        // `crawler.seed()` -> String. Zero args.
        (Type::Crawler, PreludeInstanceFn::Seed) => Some(Type::String),
        // `crawler.fetch(url)` -> Document. One arg (String URL).
        (Type::Crawler, PreludeInstanceFn::Fetch) => Some(Type::Document),
        // `crawler.crawl(max_pages)` -> Vector<String>. One arg (Int).
        (Type::Crawler, PreludeInstanceFn::Crawl) => Some(Type::vector(Type::String)),
        // `crawler.robots_allows(url)` -> Bool. One arg (String URL).
        (Type::Crawler, PreludeInstanceFn::RobotsAllows) => Some(Type::bool()),

        // T50: Xml instance methods.
        // `doc.root()` -> XmlElement. Zero args.
        (Type::Xml, PreludeInstanceFn::Root) => Some(Type::xml_element()),
        // `doc.find(xpath)` -> Option<XmlElement>. One arg (String).
        (Type::Xml, PreludeInstanceFn::Find) => Some(Type::option(Type::xml_element())),
        // `doc.to_string()` -> String. Zero args.
        (Type::Xml, PreludeInstanceFn::ToString) => Some(Type::String),

        // T50: XmlElement instance methods. `Name` is shared with
        // Faker / Language; `Text` is shared with Document / Element;
        // `Attr` is shared with Element; `Children` is XmlElement-only.
        // All return owned Buff values (String / Option<String> /
        // Vector<XmlElement>) per FFI guide R2 — the codegen lifts
        // `&str` / `Option<&str>` / `&[XmlElement]` to owned.
        // `el.name()` -> String. Zero args.
        (Type::XmlElement, PreludeInstanceFn::Name) => Some(Type::String),
        // `el.text()` -> Option<String>. Zero args.
        (Type::XmlElement, PreludeInstanceFn::Text) => Some(Type::option(Type::String)),
        // `el.attr(name)` -> Option<String>. One arg (String).
        (Type::XmlElement, PreludeInstanceFn::Attr) => Some(Type::option(Type::String)),
        // `el.children()` -> Vector<XmlElement>. Zero args.
        (Type::XmlElement, PreludeInstanceFn::Children) => Some(Type::vector(Type::xml_element())),

        // T45: buff-geo instance methods. Point.x / Point.y return
        // Float (f64); Point.distance_to returns Float; LineString
        // .length returns Float; Polygon.area returns Float;
        // Polygon.contains returns Bool (shared `Contains` variant —
        // dispatched on (Polygon, Contains) pair, same variant as
        // Cache.contains); Polygon.intersects returns Bool.
        (Type::Point, PreludeInstanceFn::X) => Some(Type::float_default()),
        (Type::Point, PreludeInstanceFn::Y) => Some(Type::float_default()),
        (Type::Point, PreludeInstanceFn::DistanceTo) => Some(Type::float_default()),
        (Type::LineString, PreludeInstanceFn::Length) => Some(Type::float_default()),
        (Type::Polygon, PreludeInstanceFn::Area) => Some(Type::float_default()),
        (Type::Polygon, PreludeInstanceFn::Contains) => Some(Type::Bool),
        (Type::Polygon, PreludeInstanceFn::Intersects) => Some(Type::Bool),

        // T54: buff-simd instance methods. `simd.add/sub/mul/div(other)`
        // each return Simd (lane-wise binary). `simd.sum/min/max()` each
        // return Float (horizontal reductions). `simd.to_vec()` returns
        // `Vector<Float>` (extract 4 lanes).
        (Type::Simd, PreludeInstanceFn::Add) => Some(Type::Simd),
        (Type::Simd, PreludeInstanceFn::Sub) => Some(Type::Simd),
        (Type::Simd, PreludeInstanceFn::Mul) => Some(Type::Simd),
        (Type::Simd, PreludeInstanceFn::Div) => Some(Type::Simd),
        (Type::Simd, PreludeInstanceFn::Sum) => Some(Type::float_default()),
        (Type::Simd, PreludeInstanceFn::Min) => Some(Type::float_default()),
        (Type::Simd, PreludeInstanceFn::Max) => Some(Type::float_default()),
        (Type::Simd, PreludeInstanceFn::ToVec) => Some(Type::vector(Type::float_default())),

        // T46: buff-nlp Language instance methods. `language.code()`
        // returns String (ISO 639-3 code — three lowercase letters).
        // `language.name()` returns String (English name — e.g.
        // "Portuguese"). Both wrap the matching `buff_nlp::Language`
        // method (which clones the inner `&'static str` into an owned
        // `String` at the boundary per FFI guide R5). `Name` is shared
        // with `faker.name()` — dispatched on the (Language, Name) /
        // (Faker, Name) pairs.
        (Type::Language, PreludeInstanceFn::Code) => Some(Type::String),
        (Type::Language, PreludeInstanceFn::Name) => Some(Type::String),

        // T52: buff-protobuf Message instance methods. `msg.byte_size()`
        // returns Int (the encoded byte count — usize at the Rust layer,
        // lifted to Buff's Int<64> via `as i64`). `msg.type_url()`
        // returns String (the canonical
        // `type.googleapis.com/google.protobuf.Struct` URL — never
        // varies in this MVP). `msg.payload()` returns Value (typed
        // Type::Unknown — there is no surface JsonValue variant; mirrors
        // how MsgPack.deserialize / Random.choice model dynamic returns;
        // the codegen wraps with `.unwrap_or_default()` for panic-free
        // Value::Null collapse). `msg.encode()` returns Vector<Byte>
        // (a fresh `Vec<u8>` cloned from the inner `&[u8]` per FFI
        // guide R2 — Buff surfaces owned values).
        (Type::Message, PreludeInstanceFn::ByteSize) => Some(Type::int_default()),
        (Type::Message, PreludeInstanceFn::TypeUrl) => Some(Type::String),
        (Type::Message, PreludeInstanceFn::Payload) => Some(Type::Unknown),
        (Type::Message, PreludeInstanceFn::Encode) => Some(Type::vector(Type::byte())),

        // T47: buff-chat instance methods. All panic-free via the
        // codegen's `.unwrap_or(())` / `.unwrap_or_default()` collapse
        // (mirrors T9 Image / T45 Point / T52 Message). Bot.command
        // returns Void (registration — failure is silently swallowed
        // at the Buff surface per FFI guide R3). Bot.start / stop /
        // dispatch return Void (block-on-event-loop or signal-shutdown
        // ops — never surface ChatError to the Buff layer). Bot.
        // is_running / has_message_handler return Bool. Bot.command_
        // count returns Int (usize cast to i64). Bot.platform /
        // Message.platform return Platform (Copy value). ChatMessage
        // channel / author return String (.to_string() lifts &str).
        // ChatMessage.is_dm returns Bool. ChatMessage.text reuses the
        // existing shared `Text` variant — dispatched on the
        // (ChatMessage, Text) pair (returns String, mirrors Document
        // / Element / XmlElement). Platform.is_discord /
        // is_telegram return Bool (Copy value).
        (Type::Bot, PreludeInstanceFn::Command) => Some(Type::Void),
        (Type::Bot, PreludeInstanceFn::OnMessage) => Some(Type::Void),
        (Type::Bot, PreludeInstanceFn::Start) => Some(Type::Void),
        (Type::Bot, PreludeInstanceFn::Stop) => Some(Type::Void),
        (Type::Bot, PreludeInstanceFn::Dispatch) => Some(Type::Void),
        (Type::Bot, PreludeInstanceFn::IsRunning) => Some(Type::Bool),
        (Type::Bot, PreludeInstanceFn::CommandCount) => Some(Type::int_default()),
        (Type::Bot, PreludeInstanceFn::HasMessageHandler) => Some(Type::Bool),
        (Type::Bot, PreludeInstanceFn::Platform) => Some(Type::platform()),
        (Type::ChatMessage, PreludeInstanceFn::Text) => Some(Type::String),
        (Type::ChatMessage, PreludeInstanceFn::Channel) => Some(Type::String),
        (Type::ChatMessage, PreludeInstanceFn::Author) => Some(Type::String),
        (Type::ChatMessage, PreludeInstanceFn::Platform) => Some(Type::platform()),
        (Type::ChatMessage, PreludeInstanceFn::IsDm) => Some(Type::Bool),
        (Type::Platform, PreludeInstanceFn::IsDiscord) => Some(Type::Bool),
        (Type::Platform, PreludeInstanceFn::IsTelegram) => Some(Type::Bool),

        // T48: buff-web3 instance methods. All panic-free via the
        // codegen's `.unwrap_or_default()` / `as i64` collapse
        // (mirrors T9 Image / T45 Point / T47 Bot / T52 Message).
        //
        // Provider (5 accessors): chain_id / block_number / get_nonce
        // return Int (Buff's Int<64> — u64 at the Rust layer lifted to
        // i64 via `as`); get_balance returns Int (low 128 bits of U256
        // wei balance — u128 lifted to i64); wait_for_tx returns String
        // (the receipt status code). All Web3Error::Rpc / Panic
        // failures collapse to 0 / String::default() — NEVER panics.
        //
        // Wallet (2 accessors + SignMessage): address returns String
        // (shared `Address` variant — already mapped by the Faker arm
        // for "address"); connect returns ConnectedWallet (shared
        // `Connect` variant — already mapped by the TCP arm for
        // "connect"); sign_message returns String (the 65-byte EIP-191
        // signature as hex). Wallet.connect consumes self — the codegen
        // lowers `wallet.connect(p)` to `wallet.connect(p)` (move).
        //
        // ConnectedWallet.address: shared `Address` variant —
        // dispatched on (ConnectedWallet, Address) pair, returns
        // String (proxies to the inner wallet's address).
        //
        // Contract (Method + Address): address returns String (shared
        // `Address` variant — dispatched on (Contract, Address) pair);
        // method returns ContractMethod (the call builder). Both
        // Web3Error::MethodNotFound / InvalidAddress collapse to
        // Default ContractMethod / String::default() via
        // `.unwrap_or_default()` — NEVER panics.
        //
        // ContractMethod (Arg / Args / Call + Send): arg / args return
        // ContractMethod (chainable — consume self, return Self,
        // mirrors Email.body / Validator.with_* builder pattern);
        // call returns String (ABI-decoded return value as debug-
        // formatted text — single-value returns are the bare value;
        // multi-value returns are `[Token, Token, ...]`); send returns
        // String (the 32-byte tx hash as `0x`-prefixed hex — shared
        // `Send` variant dispatched on (ContractMethod, Send) pair,
        // distinct lowering from (Connection, Send) / (WsConnection,
        // Send) / (Sender, Send) / (SmtpClient, Send)). All
        // Web3Error::AbiEncode / AbiDecode / Rpc / WalletNotConnected
        // failures collapse to ContractMethod::default() /
        // String::default() — NEVER panics.
        (Type::Provider, PreludeInstanceFn::ChainId) => Some(Type::int_default()),
        (Type::Provider, PreludeInstanceFn::BlockNumber) => Some(Type::int_default()),
        (Type::Provider, PreludeInstanceFn::GetBalance) => Some(Type::int_default()),
        (Type::Provider, PreludeInstanceFn::GetNonce) => Some(Type::int_default()),
        (Type::Provider, PreludeInstanceFn::WaitForTx) => Some(Type::String),

        (Type::Wallet, PreludeInstanceFn::Address) => Some(Type::String),
        (Type::Wallet, PreludeInstanceFn::Connect) => Some(Type::connected_wallet()),
        (Type::Wallet, PreludeInstanceFn::SignMessage) => Some(Type::String),

        (Type::ConnectedWallet, PreludeInstanceFn::Address) => Some(Type::String),

        (Type::Contract, PreludeInstanceFn::Address) => Some(Type::String),
        (Type::Contract, PreludeInstanceFn::Method) => Some(Type::contract_method()),

        (Type::ContractMethod, PreludeInstanceFn::Arg) => Some(Type::contract_method()),
        (Type::ContractMethod, PreludeInstanceFn::Args) => Some(Type::contract_method()),
        (Type::ContractMethod, PreludeInstanceFn::Call) => Some(Type::String),
        (Type::ContractMethod, PreludeInstanceFn::Send) => Some(Type::String),

        // T49: RsaKeypair instance methods. Both PEM-string accessors
        // return String (owned `String` lifted from `&String` via
        // `.clone()` — Buff hides references from users). Infallible
        // (no failure mode — the underlying fields are always
        // populated when constructed via RSA.generate_keypair).
        (Type::RsaKeypair, PreludeInstanceFn::PublicPem) => Some(Type::String),
        (Type::RsaKeypair, PreludeInstanceFn::PrivatePem) => Some(Type::String),

        // Every other (type, method) pair is invalid. Returning None lets
        // the caller fall back to the default "user method" path.
        _ => None,
    }
}

//! T105b: PreludeAssocFn enum + impl + assoc_fn_lookup helper.
//!
//! MECHANICAL EXTRACTION from prelude_types.rs (T105b God Class split).
//! No logic changes — moved verbatim. Contains the associated-function
//! enum definition, its impl block (ALL + name()), and the
//! assoc_fn_lookup free function.

use crate::prelude_types::{PreludeType, assoc_fn_return_type, prelude_type_lookup};

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
    /// (Int, Int). Returns Int. Wraps `rand::rng().random_range
    /// (min..=max)`.
    Int,
    /// `Random.float()` - `f64` in `[0, 1)`. Zero args. Returns Float.
    /// Wraps `rand::rng().random::<f64>()`.
    Float,
    /// `Random.choice(vec)` - pick a random element. One arg (Vector).
    /// Returns `Option<element_type>` (None on empty input - NEVER
    /// panics, matching Buff's "no panicking generated code" rule).
    /// Wraps `IndexedRandom::choose(&vec, &mut rng).cloned()`.
    Choice,
    /// `Random.shuffle(vec)` - return a shuffled copy. One arg (Vector).
    /// Returns Vector<element_type> (a NEW Vec; the input is NOT
    /// mutated in the user's surface - the codegen makes a `let mut`
    /// binding internally). Wraps `IndexedRandom::shuffle(&mut vec, &mut
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
    // ---- Faker constructors (T37) ------------------------------------
    // Each variant lowers to the matching `buff_fake::Faker`
    // constructor in codegen. Returns `Type::Faker` (a real
    // runtime-value variant — added in the same T37 commit). Buff §7
    // ctor convention permits `Type.from_*()` and `Type.new()` (this
    // surface), forbids `Type.create()` / `Type.build()` / `new Type()`.
    // `New` is shared with Channel.new (dispatched on (Faker, New)
    // pair).
    /// `Faker.with_locale(locale)` - create a Faker with the given
    /// locale. One arg (String, either "en-US" or "pt-BR"). Returns
    /// Faker. Wraps `buff_fake::Faker::with_locale(locale)`.
    WithLocale,
    /// `Faker.with_seed(locale, seed)` - create a Faker with the given
    /// locale and seed for reproducible output. Two args (String
    /// locale, Int seed). Returns Faker. Wraps
    /// `buff_fake::Faker::with_seed(locale, seed)`.
    WithSeed,
    /// T10: `AudioBuffer.from_samples(samples, sample_rate, channels)` -
    /// construct an audio buffer from already-interleaved f32 samples.
    /// Three args (Vector<Float> samples in `-1.0..=1.0`, Int sample_rate
    /// in Hz, Int channels `>= 1`). Returns AudioBuffer. Wraps
    /// `buff_audio::AudioBuffer::from_samples(samples, sample_rate as
    /// u32, channels as u16)?` (the `?` propagates AudioError per
    /// Buff's R3 error-mapping contract). Used by programmatic tone
    /// generators / DSP pipelines that build samples directly (the
    /// coordinated buff-dsp T11 task is the canonical consumer).
    FromSamples,
    // ---- Signal constructors (T11) -----------------------------------
    // Signal.from_vec(data, sample_rate) reuses the existing `FromVec`
    // variant (shared with Tensor.from_vec — same pattern as Parse /
    // Get / Encode being shared across types). No new variant needed.
    // ---- Window constructors (T11) -----------------------------------
    /// `Window.hann(n)` - Hann window of length n. One arg (Int).
    /// Returns Window (opaque `buff_dsp::Window`). Wraps
    /// `buff_dsp::Window::hann(n)`.
    Hann,
    /// `Window.hamming(n)` - Hamming window of length n. One arg (Int).
    /// Returns Window. Wraps `buff_dsp::Window::hamming(n)`.
    Hamming,
    /// `Window.blackman(n)` - Blackman window of length n. One arg (Int).
    /// Returns Window. Wraps `buff_dsp::Window::blackman(n)`.
    Blackman,
    // ---- Config (T30) ----------------------------------------------------
    /// `Config.set_default(key, value)` - set a default value. Two args
    /// (String key, any serializable value). Returns Void. Lowest
    /// precedence in the layered config stack.
    SetDefault,
    /// `Config.load_file(path)` - load a config file (TOML/YAML/JSON).
    /// One arg (String / Path). Returns Void. Format inferred from
    /// extension. Wraps `buff_config::Config::load_file(path)?`.
    LoadFile,
    /// `Config.load_env(prefix)` - load env vars with prefix. One arg
    /// (String prefix). Returns Void. Strips prefix from keys.
    LoadEnv,
    /// `Config.load_args(args)` - load CLI args. One arg (Vector<String>).
    /// Returns Void. Parses `--key=value` and `--key value` forms.
    LoadArgs,
    /// `Config.get_int(key)` - get an integer value. One arg (String key).
    /// Returns Option<Int>. Wraps `buff_config::Config::get_int(key)`.
    GetInt,
    /// `Config.get_float(key)` - get a float value. One arg (String key).
    /// Returns Option<Float>. Wraps `buff_config::Config::get_float(key)`.
    GetFloat,
    /// `Config.get_bool(key)` - get a boolean value. One arg (String key).
    /// Returns Option<Bool>. Wraps `buff_config::Config::get_bool(key)`.
    GetBool,
    /// `Config.watch(path, callback)` - watch a config file for changes.
    /// Two args (String path, callback fn). Returns Void. Fires callback
    /// on file modification. Wraps `buff_config::Config::watch(path, cb)?`.
    Watch,
    // ---- T21: Observe namespace methods ---------------------------------
    /// `Observe.span(name)` - start a new tracing span. One arg (String).
    /// Returns Void. Wraps `buff_observe::span(name)`.
    Span,
    /// `Observe.counter(name)` - create or increment a counter metric.
    /// One arg (String). Returns Void.
    Counter,
    /// `Observe.histogram(name)` - create a histogram metric. One arg
    /// (String). Returns Void.
    Histogram,
    /// `Observe.gauge(name)` - create a gauge metric. One arg (String).
    /// Returns Void.
    Gauge,
    /// `Observe.bootstrap()` - initialize the observability pipeline
    /// (OpenTelemetry SDK, tracer provider, metric reader, etc.). Zero
    /// args. Returns Void.
    Bootstrap,
    // ---- T26: Audit namespace methods ----------------------------------
    /// `Audit.scan(path)` - scan a project directory for known
    /// vulnerabilities. One arg (String path). Returns Vector<String>.
    Scan,
    // ---- T34: Auth namespace methods -----------------------------------
    /// `JWT.sign(claims, secret)` - sign a JWT. Two args (Map claims,
    /// String secret). Returns String.
    Sign,
    /// `JWT.verify(token, secret)` - verify a JWT. Two args (String
    /// token, String secret). Returns Map.
    Verify,
    /// `Signature.keypair()` - generate a new Ed25519 keypair. Zero
    /// args. Returns Signature (opaque runtime value).
    Keypair,
    // ---- T27: Fuzz namespace methods -----------------------------------
    /// `Fuzz.run(strategy, iterations, closure)` - run a fuzz test.
    /// Three args. Returns Void.
    Run,
    // ---- T30: Config typed-get methods ---------------------------------
    /// `Config.get_bool(key)` - get a bool config value. One arg (String).
    /// Returns Option<Bool>.
    Bool,
    /// `Config.get_string(key)` - get a string config value. One arg
    /// (String). Returns Option<String>.
    String,
    /// `Config.get_bytes(key)` - get a bytes config value. One arg
    /// (String). Returns Option<Vector<Byte>>.
    Bytes,
    // ---- T34: buff-auth namespace methods ------------------------------
    // JWT reuses the existing shared `Encode` / `Decode` variants
    // (already defined for Base64 / Hex / URLEncode). The (JWT, Encode)
    // / (JWT, Decode) pairs dispatch on the receiver type — same shared
    // variant pattern as `Parse` (DateTime / Date / Toml / URL / UUID).
    //
    // The five variants below are buff-auth-specific (no sharing with
    // prior prelude types in T34 scope). Each dispatches on the
    // matching (PreludeType, variant) pair in `assoc_fn_return_type`.
    /// `Password.hash(plain)` - Argon2id PHC string. One arg (String).
    /// Returns String. Password-only.
    PasswordHash,
    /// `Password.verify(plain, phc_hash)` - verify a plaintext against
    /// a stored PHC hash. Two args (String, String). Returns Bool.
    /// `Ok(false)` on mismatch (NEVER panics). Password-only.
    PasswordVerify,
    /// `OAuth2Client.authorization_url()` - build the browser URL the
    /// user must visit. Zero args. Returns String. OAuth2Client-only.
    AuthorizationUrl,
    /// `OAuth2Client.exchange_code(code, pkce_verifier)` - blocking
    /// POST to token endpoint. Two args. Returns Map<String, Unknown>.
    /// OAuth2Client-only.
    ExchangeCode,
    /// `Rbac.enforce(roles, resource, action)` - decide whether the
    /// roles may perform action on resource. Three args. Returns Bool.
    /// Rbac-only.
    Enforce,
    // T39 (sibling defensive backfill): buff-archive namespace method
    // variants. The T39 task added these to ALL + name() + return-type
    // maps but missed the enum declaration. Defensive add so the
    // shared file compiles; T39 owns the full surface.
    /// `Archive.compress_dir(src, dest)` - compress a directory. Two
    /// args (String src, String dest). Returns Void. Archive-only.
    CompressDir,
    /// `Archive.extract(archive, dest)` - extract an archive. Two args
    /// (String archive_path, String dest_dir). Returns Void. Archive-only.
    Extract,
    // ---- T46: buff-nlp namespace methods --------------------------------
    // Four namespace-only assoc fns on the Text prelude type. None
    // share a name with an existing variant, so no disambiguation
    // dispatch is needed in assoc_fn_lookup (each `(Text, $fn)` pair
    // is unique).
    /// `Text.detect_language(text)` - detect natural language. One
    /// arg (String text). Returns Option<String> (the ISO 639-3
    /// language code; None if no language detected). Text-only.
    DetectLanguage,
    /// `Text.stem(word, algorithm)` - Snowball stem a word. Two args
    /// (String word, String algorithm — lowercase Snowball name like
    /// "english" / "portuguese"). Returns String. Text-only.
    Stem,
    /// `Text.tokenize(text)` - UAX #29 word segmentation. One arg
    /// (String text). Returns Vector<String>. Text-only.
    Tokenize,
    /// `Text.sentences(text)` - UAX #29 sentence segmentation. One
    /// arg (String text). Returns Vector<String>. Text-only.
    Sentences,
    // ---- T44: I18n namespace methods ------------------------------------
    // I18n.new(locale) reuses the existing shared `New` variant
    // (already defined for Channel / Cache / Faker). The (I18n, New)
    // pair dispatches on the receiver type — same shared-variant
    // pattern as `Parse` / `New` / `Get`.
    /// `I18n.with_fallback(locale, fallback)` - construct an I18n
    /// catalog with distinct current and fallback locales. Two args
    /// (String locale, String fallback). Returns I18n. I18n-only.
    WithFallback,
    // ---- T43: buff-scrape assoc fns ------------------------------------
    // Crawler.new(seed_url) reuses the existing shared `New` variant
    // (already defined for Channel / Cache / Faker / I18n). The
    // (Crawler, New) pair dispatches on the receiver type. The single
    // new variant below is Document-only.
    /// `Document.from_html(html)` - parse an HTML string into a
    /// Document. One arg (String). Returns Document. Wraps
    /// `buff_scrape::Document::from_html(&html)?` (the `?`
    /// propagates ScrapeError::EmptyInput per Buff's R3 error-
    /// mapping contract). Document-only.
    FromHtml,
    // ---- T50: Xml ---------------------------------------------------------
    /// `Xml.from_str(xml)` — parse an XML string into an XmlDocument.
    /// One arg (String). Xml-only. Wraps
    /// `buff_xml::XmlDocument::from_str(&xml)?` (the `?`
    /// propagates XmlError::EmptyInput per Buff's R3 error-
    /// mapping contract). Xml-only.
    FromStr,
    // ---- T51: MsgPack -----------------------------------------------------
    /// `MsgPack.serialize(value)` — encode a value to MessagePack
    /// bytes. One arg. MsgPack-only. Wraps
    /// `buff_msgpack::serialize(&value).unwrap_or_default()` (empty
    /// Vec on failure — panic-free).
    Serialize,
    /// `MsgPack.deserialize(bytes)` — decode MessagePack bytes back
    /// into a value. One arg. MsgPack-only. Wraps
    /// `buff_msgpack::deserialize(&bytes).unwrap_or_default()`
    /// (Value::Null on failure — panic-free).
    Deserialize,
    /// `MsgPack.roundtrip(value)` — serialize + deserialize, returning
    /// `Option<Value>`. One arg. MsgPack-only. Wraps
    /// `buff_msgpack::roundtrip(&value)` directly (the runtime fn
    /// already returns Option).
    Roundtrip,
    /// T45: `LineString.from_coords(flat)` / `Polygon.from_coords(flat)`
    /// — construct from a flat `Vec<f64>` of interleaved `[x1, y1, x2,
    /// y2, ...]` coordinates. One arg (`Vector<Float>`). Returns
    /// `LineString` / `Polygon`. Wraps `buff_geo::LineString::from
    /// _coords(coords).unwrap_or_default()` / `buff_geo::Polygon::from
    /// _coords(coords).unwrap_or_default()` (panic-free on empty /
    /// odd-length input — the wrapper returns Default on failure per
    /// Buff's "no panicking generated code" rule). Buff §7 ctor naming
    /// convention permits `Type.from_*()`.
    FromCoords,
    /// T48: `Wallet.from_private_key(key)` — derive a wallet from a
    /// hex-encoded secp256k1 private key. One arg (String — accepts
    /// `0x`-prefixed or bare 64-char hex). Returns `Wallet`. Wraps
    /// `buff_web3::Wallet::from_private_key(&key)
    /// .unwrap_or_default()` (panic-free — Wallet impls Default as a
    /// "burner" wallet derived from a fixed test key, NEVER use on
    /// mainnet; the codegen-lowered `.unwrap_or_default()` collapses
    /// `Web3Error::InvalidPrivateKey` to a default Wallet per Buff's
    /// "no panicking generated code" rule). Buff §7 ctor naming
    /// convention permits `Type.from_*()`. Wallet-only.
    FromPrivateKey,
    // ---- T49: buff-crypto-extras namespace assoc fns -------------------
    // 10 new distinct names + reuse of the existing shared `Sign` /
    // `Verify` variants (also dispatched for T26 Signature / T34 JWT,
    // the (type, method) pair disambiguates). All dispatched on the
    // (PreludeType, PreludeAssocFn) pair in `assoc_fn_return_type`.
    /// T49: `AES.generate_key()` — generate a random 32-byte AES-256
    /// key using OsRng (CSPRNG). Zero args. Returns `Vector<Byte>`
    /// (32 bytes). AES-only.
    GenerateKey,
    /// T49: `AES.generate_nonce()` — generate a random 12-byte GCM
    /// nonce using OsRng. Zero args. Returns `Vector<Byte>` (12
    /// bytes). AES-only.
    GenerateNonce,
    /// T49: `AES.encrypt(key, nonce, plaintext)` — encrypt with
    /// AES-256-GCM. Three args (Vector<Byte> key 32B, Vector<Byte>
    /// nonce 12B, Vector<Byte> plaintext). Returns `Vector<Byte>`
    /// (ciphertext || 16-byte GCM tag). AES-only.
    Encrypt,
    /// T49: `AES.decrypt(key, nonce, ciphertext)` — decrypt AES-256-GCM.
    /// Three args (Vector<Byte> key, Vector<Byte> nonce, Vector<Byte>
    /// ciphertext-with-tag). Returns `Vector<Byte>` (plaintext).
    /// AES-only.
    Decrypt,
    /// T49: `RSA.generate_keypair(bits)` — generate a fresh RSA
    /// keypair of `bits` modulus size. One arg (Int). Returns
    /// `RsaKeypair`. RSA-only. Distinct from `Signature.keypair`
    /// (T26 Ed25519, zero-arg) — different arity + different type.
    GenerateKeypair,
    /// T49: `ECDH.generate_private()` — generate a random 32-byte
    /// P-256 private scalar using OsRng. Zero args. Returns
    /// `Vector<Byte>` (32 bytes). ECDH-only.
    GeneratePrivate,
    /// T49: `ECDH.public_from_private(private)` — derive the P-256
    /// public key (SEC1 uncompressed, 65 bytes) from a 32-byte
    /// private scalar. One arg (Vector<Byte>). Returns `Vector<Byte>`
    /// (65 bytes — `0x04 || X || Y`). ECDH-only.
    PublicFromPrivate,
    /// T49: `ECDH.derive_shared(private, public)` — compute the P-256
    /// ECDH shared secret. Two args (Vector<Byte> private 32B,
    /// Vector<Byte> public 65B). Returns `Vector<Byte>` (32 bytes —
    /// x-coordinate of the shared point). ECDH-only.
    DeriveShared,
    /// T49: `Argon2.generate_salt()` — generate a random 16-byte
    /// salt using OsRng. Zero args. Returns `Vector<Byte>` (16 bytes).
    /// Argon2-only.
    GenerateSalt,
    /// T49: `Argon2.derive_key(password, salt)` — derive a 32-byte
    /// key from `password` + `salt` using Argon2id (OWASP defaults:
    /// m=19456 KiB, t=2, p=1). Two args (String password,
    /// Vector<Byte> salt 16B). Returns `Vector<Byte>` (32 bytes).
    /// Argon2-only.
    DeriveKey,
    /// T54: `Simd.splat(x)` — broadcast a scalar `f32` to all 4 lanes
    /// of a `Simd<Float, 4>` register. One arg (Float). Returns `Simd`.
    /// Wraps `buff_simd::Simd::splat(x)` (infallible — the underlying
    /// `wide::f32x4::splat` never fails). Simd-only.
    Splat,
    /// T54: `Simd.from_slice(slice)` — construct from a flat slice of
    /// at least 4 `f32` values (reads the first 4). One arg
    /// (`Vector<Float>`). Returns `Simd`. Wraps
    /// `buff_simd::Simd::from_slice(&slice).unwrap_or_default()`
    /// (panic-free on too-short / non-finite input — Simd impls Default
    /// as `splat(0.0)`). Simd-only.
    FromSlice,
    /// T54: `Simd.from_array(arr)` — construct from a fixed-size
    /// 4-element array. One arg (`Vector<Float>` of length 4).
    /// Returns `Simd`. Wraps `buff_simd::Simd::from_array(arr)`
    /// (infallible). Simd-only. Buff §7 ctor naming convention permits
    /// `Type.from_*()`.
    FromArray,
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
        // T37: Faker constructors (3 distinct names): new (shared with
        // Channel.new), with_locale, with_seed. Faker-only beyond the
        // shared `New` variant. Buff §7 ctor naming convention permits
        // `Type.new()` and `Type.with_*()`.
        PreludeAssocFn::New,
        PreludeAssocFn::WithLocale,
        PreludeAssocFn::WithSeed,
        // T10: AudioBuffer constructor (1 distinct name): from_samples.
        // AudioBuffer-only. Reuses the Buff §7 `Type.from_*()` ctor
        // naming convention. `from_path` is shared with Image /
        // DataFrame via the existing `FromPath` variant — dispatched
        // on the (Audio, FromPath) pair.
        PreludeAssocFn::FromSamples,
        // T11: Window constructors (3): hann / hamming / blackman.
        PreludeAssocFn::Hann,
        PreludeAssocFn::Hamming,
        PreludeAssocFn::Blackman,
        // T30: Config assoc fns (8 distinct names): set_default / load_file
        // / load_env / load_args / get_int / get_float / get_bool / watch.
        // `New` is shared with Channel.new (dispatched on (Config, New)
        // pair). `Get` is shared with Args.get / Env.get (dispatched on
        // (Config, Get) pair). All Config-only beyond those two shared
        // variants. Namespace-only module (mirror Log / Toml / Math).
        PreludeAssocFn::SetDefault,
        PreludeAssocFn::LoadFile,
        PreludeAssocFn::LoadEnv,
        PreludeAssocFn::LoadArgs,
        PreludeAssocFn::GetInt,
        PreludeAssocFn::GetFloat,
        PreludeAssocFn::GetBool,
        PreludeAssocFn::Watch,
        // T21: Observe namespace methods (5): span / counter / histogram /
        // gauge / bootstrap.
        PreludeAssocFn::Span,
        PreludeAssocFn::Counter,
        PreludeAssocFn::Histogram,
        PreludeAssocFn::Gauge,
        PreludeAssocFn::Bootstrap,
        // T34: buff-auth assoc fns (5 distinct new names). JWT reuses
        // the existing shared `Encode` / `Decode` variants (already in
        // ALL — defined for Base64 / Hex / URLEncode). Password /
        // OAuth2Client / Rbac introduce 5 new variants; PasswordHash /
        // PasswordVerify / AuthorizationUrl / ExchangeCode / Enforce.
        // The (type, method) pair dispatch in `assoc_fn_return_type`
        // validates each combination.
        PreludeAssocFn::PasswordHash,
        PreludeAssocFn::PasswordVerify,
        PreludeAssocFn::AuthorizationUrl,
        PreludeAssocFn::ExchangeCode,
        PreludeAssocFn::Enforce,
        // T44: I18n.with_fallback — distinct current/fallback locales.
        // I18n.new reuses the shared `New` variant.
        PreludeAssocFn::WithFallback,
        // T43: Document.from_html — single-arg HTML parse. Crawler.new
        // reuses the shared `New` variant.
        PreludeAssocFn::FromHtml,
        // T39: buff-archive assoc fns (2 distinct new names). Both
        // are Archive-only — dispatched on the (Archive, CompressDir)
        // / (Archive, Extract) pairs in `assoc_fn_return_type`. No
        // other prelude type today exposes these verbs; if a future
        // task adds a second archive-style namespace, it can reuse
        // these variants on the (FutureType, CompressDir) pair.
        PreludeAssocFn::CompressDir,
        PreludeAssocFn::Extract,
        // T50: Xml.from_str — single-arg XML parse.
        PreludeAssocFn::FromStr,
        // T51: MsgPack assoc fns (3 distinct names): serialize /
        // deserialize / roundtrip. All MsgPack-only — dispatched on
        // the (MsgPack, Serialize) / (MsgPack, Deserialize) /
        // (MsgPack, Roundtrip) pairs in `assoc_fn_return_type`. No
        // other prelude type today exposes these verbs (Base64 /
        // Hex / URLEncode / JWT use the lower-level `Encode` /
        // `Decode` pair instead — the (type, method) dispatch in
        // `assoc_fn_return_type` validates each combination).
        PreludeAssocFn::Serialize,
        PreludeAssocFn::Deserialize,
        PreludeAssocFn::Roundtrip,
        // T45: LineString.from_coords / Polygon.from_coords — flat
        // Vec<f64> coordinate-list ctor. Dispatched on the
        // (LineString, FromCoords) / (Polygon, FromCoords) pairs.
        PreludeAssocFn::FromCoords,
        // T46: buff-nlp Text namespace assoc fns (4 distinct names):
        // detect_language / stem / tokenize / sentences. All
        // Text-only — dispatched on the (Text, DetectLanguage) /
        // (Text, Stem) / (Text, Tokenize) / (Text, Sentences) pairs
        // in `assoc_fn_return_type`. None share a name with an
        // existing variant (each `(Text, $fn)` pair is unique).
        PreludeAssocFn::DetectLanguage,
        PreludeAssocFn::Stem,
        PreludeAssocFn::Tokenize,
        PreludeAssocFn::Sentences,
        // T48: Wallet.from_private_key — single-arg secp256k1 wallet
        // ctor. Wallet-only — dispatched on the (Wallet, FromPrivateKey)
        // pair in `assoc_fn_return_type`. Buff §7 ctor convention
        // permits `Type.from_*()`. No other prelude type today exposes
        // this verb (if a future task adds a second wallet-style
        // namespace it can reuse this variant on the (FutureType,
        // FromPrivateKey) pair).
        PreludeAssocFn::FromPrivateKey,
        // T49: buff-crypto-extras assoc fns (10 distinct new names +
        // reuse of existing shared `Sign` / `Verify` for RSA.sign /
        // RSA.verify). All dispatched on the (PreludeType, method)
        // pair in `assoc_fn_return_type`. AES (4) + RSA (1) + ECDH (3)
        // + Argon2 (2).
        PreludeAssocFn::GenerateKey,
        PreludeAssocFn::GenerateNonce,
        PreludeAssocFn::Encrypt,
        PreludeAssocFn::Decrypt,
        PreludeAssocFn::GenerateKeypair,
        PreludeAssocFn::GeneratePrivate,
        PreludeAssocFn::PublicFromPrivate,
        PreludeAssocFn::DeriveShared,
        PreludeAssocFn::GenerateSalt,
        PreludeAssocFn::DeriveKey,
        // T54: Simd ctors — splat / from_slice / from_array. Dispatched
        // on the (Simd, Splat) / (Simd, FromSlice) / (Simd, FromArray)
        // pairs.
        PreludeAssocFn::Splat,
        PreludeAssocFn::FromSlice,
        PreludeAssocFn::FromArray,
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
            // T10: AudioBuffer ctor name mirrors the Buff §7
            // `Type.from_*()` ctor convention so the codegen can
            // splice `buff_audio::AudioBuffer::from_samples(s, sr, ch)`
            // without rewriting.
            PreludeAssocFn::FromSamples => "from_samples",
            PreludeAssocFn::Hann => "hann",
            PreludeAssocFn::Hamming => "hamming",
            PreludeAssocFn::Blackman => "blackman",
            // T30: Config method names. Mirror the `buff_config::Config`
            // method names so codegen can splice
            // `buff_config::Config::set_default(key, val)` /
            // `buff_config::Config::load_file(path)` /
            // `buff_config::Config::load_env(prefix)` /
            // `buff_config::Config::load_args(args)` /
            // `buff_config::Config::get_int(key)` /
            // `buff_config::Config::get_float(key)` /
            // `buff_config::Config::get_bool(key)` /
            // `buff_config::Config::watch(path, cb)` paths without
            // rewriting. `new` and `get` are shared with Channel.new /
            // Args.get / Env.get (dispatched on (Config, New) /
            // (Config, Get) pair).
            PreludeAssocFn::SetDefault => "set_default",
            PreludeAssocFn::LoadFile => "load_file",
            PreludeAssocFn::LoadEnv => "load_env",
            PreludeAssocFn::LoadArgs => "load_args",
            PreludeAssocFn::GetInt => "get_int",
            PreludeAssocFn::GetFloat => "get_float",
            PreludeAssocFn::GetBool => "get_bool",
            PreludeAssocFn::Watch => "watch",
            // T21: Observe namespace method names. Mirror the Rust
            // `buff_observe` crate method names so codegen can splice
            // `buff_observe::span(name)` / `buff_observe::counter(name)`
            // / `buff_observe::histogram(name)` / `buff_observe::gauge(name)`
            // / `buff_observe::bootstrap()` paths without rewriting.
            PreludeAssocFn::Span => "span",
            PreludeAssocFn::Counter => "counter",
            PreludeAssocFn::Histogram => "histogram",
            PreludeAssocFn::Gauge => "gauge",
            PreludeAssocFn::Bootstrap => "bootstrap",
            // T26: Audit namespace method names.
            PreludeAssocFn::Scan => "scan",
            // T26: buff-audit Signature namespace method names.
            PreludeAssocFn::Sign => "sign",
            PreludeAssocFn::Verify => "verify",
            PreludeAssocFn::Keypair => "keypair",
            // T27: Fuzz namespace method names.
            PreludeAssocFn::Run => "run",
            // T30: Config typed-get method names.
            PreludeAssocFn::Bool => "get_bool",
            PreludeAssocFn::String => "get_string",
            PreludeAssocFn::Bytes => "get_bytes",
            // T34: buff-auth namespace method names. JWT reuses the
            // existing `Encode` / `Decode` shared variants (also used
            // by Base64 / Hex / URLEncode — dispatched on the receiver
            // type). Password.verify reuses the existing shared `verify`
            // name (same name as Signature.verify — distinct variants,
            // dispatched on the (Password, PasswordVerify) pair). The 4
            // new distinct names below mirror the underlying `buff_auth::*`
            // Rust method names so codegen can splice paths without
            // rewriting.
            PreludeAssocFn::PasswordHash => "hash",
            PreludeAssocFn::PasswordVerify => "verify",
            PreludeAssocFn::AuthorizationUrl => "authorization_url",
            PreludeAssocFn::ExchangeCode => "exchange_code",
            PreludeAssocFn::Enforce => "enforce",
            // T44: I18n constructor with explicit fallback locale.
            PreludeAssocFn::WithFallback => "with_fallback",
            // T43: Document.from_html — Buff §7 `Type.from_*()` ctor
            // naming convention permits the form. Crawler.new reuses
            // the shared `New` variant name ("new").
            PreludeAssocFn::FromHtml => "from_html",
            // T50: Xml.from_str — canonical name for "parse from string".
            PreludeAssocFn::FromStr => "from_str",
            // T39: buff-archive namespace method names. Both names
            // mirror the `buff_archive::Archive` Rust method names.
            PreludeAssocFn::CompressDir => "compress_dir",
            PreludeAssocFn::Extract => "extract",
            // T46: buff-nlp namespace method names. All four mirror
            // the `buff_nlp::Text` Rust method names 1:1.
            PreludeAssocFn::DetectLanguage => "detect_language",
            PreludeAssocFn::Stem => "stem",
            PreludeAssocFn::Tokenize => "tokenize",
            PreludeAssocFn::Sentences => "sentences",
            // T48: Wallet.from_private_key — name mirrors the
            // `buff_web3::Wallet::from_private_key` Rust method name
            // 1:1 so codegen can splice the path without rewriting.
            // Buff §7 ctor naming convention permits `Type.from_*()`.
            PreludeAssocFn::FromPrivateKey => "from_private_key",
            // T49: buff-crypto-extras assoc fn names mirror the
            // underlying `buff_crypto_extras::{AES, RSA, ECDH,
            // Argon2}::*` Rust method names 1:1 so codegen can
            // splice `buff_crypto_extras::AES::generate_key()` /
            // `buff_crypto_extras::RSA::generate_keypair(bits)` /
            // `buff_crypto_extras::ECDH::derive_shared(...)` /
            // `buff_crypto_extras::Argon2::derive_key(...)` paths
            // without rewriting. RSA.sign / RSA.verify reuse the
            // existing shared `Sign` / `Verify` variants (already
            // mapped to "sign" / "verify" by the T26/T34 arm above).
            PreludeAssocFn::GenerateKey => "generate_key",
            PreludeAssocFn::GenerateNonce => "generate_nonce",
            PreludeAssocFn::Encrypt => "encrypt",
            PreludeAssocFn::Decrypt => "decrypt",
            PreludeAssocFn::GenerateKeypair => "generate_keypair",
            PreludeAssocFn::GeneratePrivate => "generate_private",
            PreludeAssocFn::PublicFromPrivate => "public_from_private",
            PreludeAssocFn::DeriveShared => "derive_shared",
            PreludeAssocFn::GenerateSalt => "generate_salt",
            PreludeAssocFn::DeriveKey => "derive_key",
            // T37 (sibling): Faker.with_locale / Faker.with_seed —
            // sibling task added the variants + ALL + return-type
            // entries but missed the name() match arms. Defensive
            // backfill (canonical Rust method names 1:1) so the
            // shared file compiles; buff-fake's full codegen wiring
            // is the T37 owner's responsibility.
            PreludeAssocFn::WithLocale => "with_locale",
            PreludeAssocFn::WithSeed => "with_seed",
            // T43: Document.from_html — Buff §7 `Type.from_*()` ctor
            // naming convention permits the form. Crawler.new reuses
            // the shared `New` variant name ("new").
            PreludeAssocFn::FromHtml => "from_html",
            // T50: Xml.from_str — canonical name for "parse from string".
            PreludeAssocFn::FromStr => "from_str",
            // T51: MsgPack.serialize / .deserialize / .roundtrip.
            // Mirrors the underlying `buff_msgpack::serialize` /
            // `buff_msgpack::deserialize` /
            // `buff_msgpack::roundtrip` Rust fn names so codegen
            // splices `buff_msgpack::<name>(...)` without rewriting.
            PreludeAssocFn::Serialize => "serialize",
            PreludeAssocFn::Deserialize => "deserialize",
            PreludeAssocFn::Roundtrip => "roundtrip",
            // T45: LineString.from_coords / Polygon.from_coords —
            // Buff §7 `Type.from_*()` ctor naming convention. Mirrors
            // the buff_geo Rust method names so codegen can splice
            // `buff_geo::LineString::from_coords(...)` /
            // `buff_geo::Polygon::from_coords(...)` without rewriting.
            PreludeAssocFn::FromCoords => "from_coords",
            // T54: Simd ctor names mirror the `buff_simd::Simd` Rust
            // method names 1:1 so codegen can splice
            // `buff_simd::Simd::splat(...)` / `::from_slice(...)` /
            // `::from_array(...)` without rewriting.
            PreludeAssocFn::Splat => "splat",
            PreludeAssocFn::FromSlice => "from_slice",
            PreludeAssocFn::FromArray => "from_array",
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
            // (clearer than `random_range` / `random`); `choice` / `shuffle`
            // mirror rand's `IndexedRandom` trait method names.
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
            // T11: buff-dsp window functions. Names mirror the canonical
            // DSP literature spelling (lowercase method names).
            PreludeAssocFn::Hann => "hann",
            PreludeAssocFn::Hamming => "hamming",
            PreludeAssocFn::Blackman => "blackman",
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


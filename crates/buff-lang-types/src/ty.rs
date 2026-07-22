//! The compile-time type representation for the Buff language.
//!
//! [`Type`] is the *resolved* type of an expression — produced by the
//! [`TypeInferencer`](crate::TypeInferencer) — and is distinct from
//! [`TypeRef`](buff_lang_ast::TypeRef), which is a *reference* to a type written
//! in source annotations.
//!
//! v1.0 ships primitives, collections (Vector/Map/Matrix), user-defined types
//! (struct/enum), traits, full type inference, exhaustiveness checking, and
//! recursion detection.

use std::fmt;

/// The compile-time type of a Buff expression.
///
/// v1.0 ships primitives, collections, user-defined types (struct/enum), traits.
#[derive(Debug, Clone, PartialEq)]
pub enum Type {
    /// A signed integer, `Int<W>` (defaults to `Int<64>`).
    Int { width: IntWidth },
    /// An unsigned integer (`Bits<W>`, defaults to `Bits<8>`).
    Bits { width: IntWidth },
    /// A floating-point type, `Float<W>` (defaults to `Float<32>`).
    Float { width: FloatWidth },
    /// A 64-bit float (`Double`, i.e. `Float<64>`).
    Double,
    /// A boolean (`Bool`).
    Bool,
    /// A UTF-8 string (`String`).
    String,
    /// A single Unicode scalar value (`Char`). (T21 — additive.)
    ///
    /// Maps to Rust's `char` type (a 4-byte Unicode scalar value). Distinct
    /// from `String` (a UTF-8 byte buffer): `'A'` is `Char`, `"A"` is
    /// `String`. Not GPU-eligible (no WGSL scalar) — always CPU.
    Char,
    /// A 128-bit fixed-point decimal (`Decimal`). Full arithmetic support
    /// shipped in v0.5+ via `rust_decimal`.
    Decimal,
    /// Unknown / a placeholder emitted after a type error to suppress
    /// cascading diagnostics.
    Unknown,
    /// The absence of a value (for functions without a return, or `if`
    /// expressions without an `else` branch).
    Void,
    /// A generic vector/array type: `Vector<T>` (T99 — prelude `args()`).
    ///
    /// Maps to Rust's `Vec<T>`. The element type is boxed so the enum
    /// variant can carry any inner type. Full collection support (indexing,
    /// iteration, methods) arrives in T23.
    Vector(Box<Type>),
    /// A 2-D matrix type: `Matrix<T>` (T24 — flat contiguous storage).
    ///
    /// Maps to the builtin `Matrix<T>` struct emitted by the Rust codegen:
    /// `struct Matrix<T> { data: Vec<T>, rows: usize, cols: usize }`. Storage
    /// is a **single flat `Vec<T>`** (row-major, `row * cols + col` indexing)
    /// so the buffer is contiguous and directly GPU-transferable (no
    /// `Vec<Vec<T>>` nesting). This is the canonical GPU-ready collection —
    /// a `Matrix<Float<32>>` of `rows * cols` elements can be uploaded to a
    /// WGSL storage buffer verbatim.
    ///
    /// The element type is boxed, mirroring [`Type::Vector`]. Element-type
    /// inference from `Matrix.new(rows, cols)` is deferred (the constructor
    /// carries no element evidence by itself); `let m: Matrix<Int> = ...`
    /// annotations and 2-D indexing `m[r, c]` both flow through this variant.
    Matrix(Box<Type>),
    /// An optional value: `Option<T>` (T99 — prelude `env()`).
    ///
    /// Maps to Rust's `Option<T>`. Used by `env("HOME")` which returns
    /// `Option<String>`.
    Option(Box<Type>),
    /// A hash-map type: `Map<K, V>` (T25 — keyed dictionary collection).
    ///
    /// Maps to Rust's `std::collections::HashMap<K, V>`. The key and value
    /// types are each boxed so the enum can carry any inner types. The map
    /// literal `{"k": v, ...}` (note: braces + colon-separated entries) lowers
    /// to `HashMap::from([("k", v), ...])`. Map method dispatch
    /// (`.get`/`.insert`/`.contains`/`.remove`/`.len`) is handled by the Rust
    /// codegen via the standard `HashMap` inherent methods (`.contains` maps
    /// to `contains_key`).
    ///
    /// Both type params are inferred from the first entry of a literal;
    /// literals with mixed key/value kinds fall back to the first entry's
    /// types (a future task will enforce uniformity).
    Map(Box<Type>, Box<Type>),
    /// A result type: `Result<T, E>` (T30 — prelude error-handling enum).
    ///
    /// Maps 1:1 to Rust's `std::result::Result<T, E>`. Mirrors [`Type::Option`]
    /// (T28): `Result` is a **built-in prelude enum** whose variants `Ok(T)`
    /// and `Err(E)` resolve WITHOUT a user declaration and WITHOUT being
    /// reserved keywords. The Ok type (first param) and Err type (second
    /// param) are each boxed, mirroring [`Type::Map`]'s two-param shape.
    ///
    /// `Ok(x)` infers `Result<T, Unknown>` (the Err type is pinned by context
    /// — e.g. a `let x: Result<Int, Error> = Ok(42)` annotation — or stays
    /// `Unknown`). `Err(e)` infers `Result<Unknown, E>` symmetrically. The
    /// `?` postfix operator (`Expr::Try`) propagates the Err and yields the
    /// Ok type `T`.
    ///
    /// This is **additive** (T30): no existing variant was renamed, reordered,
    /// or had its payload altered. All exhaustive `match`es on `Type` were
    /// extended with an arm for the new variant: `Display`, `buff_type_to_syn`
    /// (codegen), `typeref_to_type` (inferencer + exhaustiveness), and the
    /// prelude-seeded enum registry (`build_enum_registry_with_prelude`).
    Result(Box<Type>, Box<Type>),
    /// A union (sum) type: `A | B | C` (T76).
    ///
    /// Each member is a resolved [`Type`]. Rust has no anonymous unions, so
    /// codegen lowers this to a named enum wrapper (e.g. `String | Int` →
    /// `enum StringOrInt { String(String), Int(i64) }`). A union is neither
    /// numeric nor GPU-eligible; it participates in no promotion rules in
    /// v0.5 (arithmetic on a union value is a type error — the user must
    /// `match` to discriminate first). Runtime discrimination / match-on-
    /// union coercion is a documented deferral.
    Union(Vec<Type>),
    /// A tuple type: `(T, U, ...)`, e.g. `(String, Int)` (T103).
    ///
    /// Each member is a resolved [`Type`]. The 2+-element rule lives at
    /// parse time (a single `(T)` is grouping, returning the bare `T`), so
    /// this variant always carries 2+ members — there is no single-element
    /// tuple in Buff. Maps 1:1 to a Rust tuple `(T, U, ...)` via codegen.
    ///
    /// A tuple is neither numeric nor GPU-eligible; it participates in no
    /// promotion rules in v0.5. Tuple indexing (`t.0`), tuple-member arity
    /// checking, and single-element tuples `(x,)` are documented deferrals.
    ///
    /// This is **additive** (T103): no existing variant was renamed,
    /// reordered, or had its payload altered. See the T76 union-types entry
    /// in `.sisyphus/notepads/buff-v05-language/learnings.md` for the
    /// resolved-Type ripple template.
    Tuple(Vec<Type>),
    /// A timezone-aware date+time, mapped to `chrono::DateTime<chrono::Utc>`
    /// at codegen time (T124b). Constructed via the prelude associated
    /// function `DateTime.now()` or `DateTime.parse(iso_string)`; supports
    /// `dt.format("%Y-%m-%d")` instance method, `dt + Duration.days(n)`
    /// arithmetic, and `dt1 < dt2` comparison.
    ///
    /// This is **additive** (T124b): no existing variant was renamed,
    /// reordered, or had its payload altered. All exhaustive `match`es on
    /// `Type` were extended with an arm for the new variants: `Display`,
    /// `buff_type_to_syn` (codegen), `typeref_to_type` (inferencer +
    /// exhaustiveness), `buff_primitive_to_rust_name` (primitive name
    /// mapping), and `ast_typeref_to_syn`. The `is_numeric` /
    /// `is_float_like` / `is_integer_like` / `is_gpu_eligible` predicates
    /// all return `false` for the datetime variants — they are opaque
    /// value types that participate in no numeric promotion.
    DateTime,
    /// A calendar date (year, month, day) without time or timezone,
    /// mapped to `chrono::NaiveDate` at codegen time (T124b). Constructed
    /// via `Date.today()` or `Date.parse(s)`. Additive — see [`Type::DateTime`].
    Date,
    /// A clock time (hour, minute, second, subsecond) without date or
    /// timezone, mapped to `chrono::NaiveTime` at codegen time (T124b).
    /// Additive — see [`Type::DateTime`].
    Time,
    /// A span of time, mapped to `chrono::TimeDelta` at codegen time
    /// (T124b). chrono's `Duration` type alias was deprecated in favor of
    /// `TimeDelta` (chrono 0.4.35+); Buff codegen uses the new name to
    /// avoid emitting deprecation warnings in user code. Constructed via
    /// `Duration.days(n)` / `Duration.hours(n)` / `Duration.minutes(n)` /
    /// `Duration.seconds(n)` / `Duration.millis(n)`. Additive — see
    /// [`Type::DateTime`].
    Duration,
    /// A monotonic instant suitable for measuring elapsed time, mapped to
    /// `std::time::Instant` at codegen time (T124b). Constructed via
    /// `Instant.now()`. Distinct from [`Type::DateTime`] (which is
    /// wall-clock time and uses chrono). Additive — see [`Type::DateTime`].
    Instant,
    /// A compiled regular expression, mapped to `regex::Regex` at codegen
    /// time (T124d). Constructed via the prelude associated function
    /// `Regex.compile(pattern)`; supports `regex.match(text)` /
    /// `regex.find(text)` / `regex.replace(text, repl)` /
    /// `regex.captures(text)` instance methods.
    ///
    /// This is **additive** (T124d): no existing variant was renamed,
    /// reordered, or had its payload altered. All exhaustive `match`es on
    /// `Type` were extended with an arm for the new variant: `Display`,
    /// `buff_type_to_syn` (codegen), `typeref_to_type` (inferencer +
    /// exhaustiveness), `buff_primitive_to_rust_name` (primitive name
    /// mapping), and `ast_typeref_to_syn`. The `is_numeric` /
    /// `is_float_like` / `is_integer_like` / `is_gpu_eligible` predicates
    /// all return `false` for `Regex` — it's an opaque value type that
    /// participates in no numeric promotion.
    ///
    /// This is the FIRST v1.4 prelude-type variant that is BOTH a real
    /// runtime value (like DateTime) AND supports INSTANCE methods
    /// (`recv.method(args)` shape — DateTime only had `format` +
    /// accessors; Log was namespace-only with associated functions).
    /// Future instance-method-carrying runtime types (e.g. Url, Hasher)
    /// follow this pattern.
    Regex,
    /// A parsed URL value, mapped to `url::Url` at codegen time (T124h).
    /// Constructed via the prelude associated function `URL.parse(s)`;
    /// supports instance accessors `.scheme`, `.host`, `.path` (each
    /// returning a `String`) and `.query(key) -> Option<String>`.
    ///
    /// This is **additive** (T124h): no existing variant was renamed,
    /// reordered, or had its payload altered. All exhaustive `match`es
    /// on `Type` were extended with an arm for the new variant:
    /// `Display`, `buff_type_to_syn` (codegen), `is_prelude_url`
    /// predicate. The `is_numeric` / `is_float_like` /
    /// `is_integer_like` / `is_gpu_eligible` predicates all return
    /// `false` for `Url` — it's an opaque value type that participates
    /// in no numeric promotion.
    ///
    /// Mirrors [`Type::Regex`] (T124d) as the second runtime-value
    /// prelude type with rich instance methods (Regex has 4, URL has 4).
    /// Distinct from [`Type::DateTime`] et al (which have accessor
    /// methods only) and from the namespace-only modules (Log/Toml/
    /// Math/Random/Strings/Args/Env — those have no value
    /// representation).
    Url,
    /// A filesystem path, mapped to `std::path::PathBuf` at codegen
    /// time (T124j). Constructed via the prelude associated function
    /// `Path.join(a, b, ...)` (variadic - 2+ args, chained); supports
    /// instance methods `.parent() -> Option<Path>`,
    /// `.extension() -> Option<String>`, `.basename() -> String`,
    /// `.exists() -> Bool`. Replaces the deferred v1.0 T61 File I/O
    /// task.
    ///
    /// This is **additive** (T124j): no existing variant was renamed,
    /// reordered, or had its payload altered. All exhaustive
    /// `match`es on `Type` were extended with an arm for the new
    /// variant: `Display`, `buff_type_to_syn` (codegen),
    /// `is_prelude_path` predicate. The `is_numeric` /
    /// `is_float_like` / `is_integer_like` / `is_gpu_eligible`
    /// predicates all return `false` for `Path` — it's an opaque
    /// value type that participates in no numeric promotion.
    ///
    /// Mirrors [`Type::Url`] (T124h) and [`Type::Regex`] (T124d) as
    /// the third runtime-value-with-rich-instance-methods type
    /// (Regex has 4 instance methods, URL has 4, Path has 4). The
    /// underlying Rust type is `std::path::PathBuf` (NOT `&Path` -
    /// Buff surfaces owned values; `.parent()` lifts the `&Path`
    /// result to `PathBuf` via `.to_path_buf()`).
    Path,
    /// A spawned child process, mapped to `std::process::Child` at
    /// codegen time (T124l). Constructed via the prelude associated
    /// function `Process.spawn(command, args)` (two args: a command
    /// String + a `Vector<String>` of args); supports the instance
    /// methods `.wait() -> Int` (exit code; -1 / fallback when the
    /// process is already exited or its status lacks a code) and
    /// `.id() -> Int` (the OS process ID). Distinct from the
    /// namespace-only [`crate::prelude_types::PreludeType::OS`]
    /// module (which it shipped alongside): `Process` IS a real
    /// runtime value (an opaque handle to a spawned child); `OS` is
    /// pure namespace (no value representation, just associated fns).
    ///
    /// This is **additive** (T124l): no existing variant was renamed,
    /// reordered, or had its payload altered. All exhaustive
    /// `match`es on `Type` were extended with an arm for the new
    /// variant: `Display`, `buff_type_to_syn` (codegen),
    /// `is_prelude_process` predicate. The `is_numeric` /
    /// `is_float_like` / `is_integer_like` / `is_gpu_eligible`
    /// predicates all return `false` for `Process` — it's an opaque
    /// value type that participates in no numeric promotion.
    ///
    /// Mirrors [`Type::Path`] (T124j) / [`Type::Url`] (T124h) /
    /// [`Type::Regex`] (T124d) as the fourth runtime-value-with-
    /// instance-methods type (Regex/URL/Path have 4 each; Process
    /// has 2). The underlying Rust type is `Option<std::process::
    /// Child>` — the codegen emits `Command::new(cmd).args(args)
    /// .spawn().ok()` so the spawn is panic-free (a spawn failure
    /// collapses to `None`; `.wait()` / `.id()` then operate on the
    /// `Option` via `.map(...).unwrap_or_default()`). See
    /// `decisions.md` for the spawn-failure-handling rationale.
    Process,
    /// A TCP client connection, mapped to
    /// `Option<tokio::net::TcpStream>` at codegen time (T124m).
    /// Constructed via the prelude associated function
    /// `TCP.connect(host, port)` (two args: a host String + a port
    /// Int); supports the instance methods `.send(data: String)`
    /// (write bytes; the codegen emits a block-scoped
    /// `use tokio::io::AsyncWriteExt;` + `s.write_all(...).await.ok()`,
    /// panic-free), `.recv() -> Vector<Byte>` (read into a buffer
    /// via `tokio::io::AsyncReadExt` + return the bytes, panic-free
    /// on EOF / error via `Vec::new()` fallback), and `.close()`
    /// (graceful shutdown via `s.shutdown().await.ok()`).
    ///
    /// This is **additive** (T124m): no existing variant was renamed,
    /// reordered, or had its payload altered. All exhaustive `match`es
    /// on `Type` were extended with an arm for the new variant:
    /// `Display`, `buff_type_to_syn` (codegen),
    /// `is_prelude_connection` predicate. The `is_numeric` /
    /// `is_float_like` / `is_integer_like` / `is_gpu_eligible`
    /// predicates all return `false` for `Connection` — it's an
    /// opaque value type that participates in no numeric promotion.
    ///
    /// Mirrors [`Type::Process`] (T124l) as a runtime-value-with-
    /// instance-methods type. The underlying Rust type is
    /// `Option<tokio::net::TcpStream>` — the codegen emits
    /// `tokio::net::TcpStream::connect(format!("{}:{}", h, p)).await
    /// .ok()` so the connect is panic-free (a connect failure
    /// collapses to `None`; `.send()` / `.recv()` / `.close()` then
    /// operate on the `Option` via `if let Some(mut s) = ...`). See
    /// `decisions.md` for the connect-failure-handling rationale.
    Connection,
    /// A bound UDP socket, mapped to `Option<tokio::net::UdpSocket>`
    /// at codegen time (T124m). Constructed via the prelude associated
    /// function `UDP.bind(host, port)` (two args: a host String + a
    /// port Int); supports the instance methods `.send_to(data:
    /// String, addr: String)` (send datagram to addr via
    /// `s.send_to(bytes, addr).await.ok()`) and `.recv_from() ->
    /// Tuple` (receive a datagram returning `(data, addr)` via
    /// `s.recv_from(&mut buf).await.ok().map(|(n, addr)| (buf[..n].
    /// to_vec(), addr.to_string()))`).
    ///
    /// This is **additive** (T124m): no existing variant was renamed,
    /// reordered, or had its payload altered. All exhaustive `match`es
    /// on `Type` were extended with an arm for the new variant:
    /// `Display`, `buff_type_to_syn` (codegen),
    /// `is_prelude_socket` predicate. The `is_numeric` /
    /// `is_float_like` / `is_integer_like` / `is_gpu_eligible`
    /// predicates all return `false` for `Socket` — it's an opaque
    /// value type that participates in no numeric promotion.
    ///
    /// Mirrors [`Type::Connection`] (T124m) as a runtime-value-with-
    /// instance-methods type. The underlying Rust type is
    /// `Option<tokio::net::UdpSocket>` — the codegen emits
    /// `tokio::net::UdpSocket::bind(format!("{}:{}", h, p)).await.ok()`
    /// so the bind is panic-free (a bind failure collapses to `None`;
    /// `.send_to()` / `.recv_from()` then operate on the `Option` via
    /// `if let Some(mut s) = ...`).
    Socket,
    /// A WebSocket client connection, mapped to
    /// `Option<tokio_tungstenite::WebSocketStream<tokio_tungstenite
    /// ::MaybeTlsStream<tokio::net::TcpStream>>>` at codegen time
    /// (T124m). Constructed via the prelude associated function
    /// `WebSocket.connect(url)` (one arg: a URL String); supports the
    /// instance methods `.send(text: String)` (send a Text frame via
    /// `ws.send(Message::Text(...)).await.ok()` - block-scoped
    /// `use futures_util::SinkExt;`), `.recv() -> String` (receive
    /// the next message as text via `ws.next().await` - block-scoped
    /// `use futures_util::StreamExt;`), and `.close()` (send a Close
    /// frame via `ws.close(None).await.ok()`).
    ///
    /// This is **additive** (T124m): no existing variant was renamed,
    /// reordered, or had its payload altered. All exhaustive `match`es
    /// on `Type` were extended with an arm for the new variant:
    /// `Display`, `buff_type_to_syn` (codegen),
    /// `is_prelude_ws_connection` predicate. The `is_numeric` /
    /// `is_float_like` / `is_integer_like` / `is_gpu_eligible`
    /// predicates all return `false` for `WsConnection` — it's an
    /// opaque value type that participates in no numeric promotion.
    ///
    /// Mirrors [`Type::Connection`] / [`Type::Socket`] (T124m) as a
    /// runtime-value-with-instance-methods type. The underlying Rust
    /// type is `Option<tokio_tungstenite::WebSocketStream<
    /// tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>>` —
    /// the codegen emits
    /// `tokio_tungstenite::connect_async(url).await.ok().map(|(ws, _)| ws)`
    /// so the connect is panic-free (a connect failure collapses to
    /// `None`; `.send()` / `.recv()` / `.close()` then operate on the
    /// `Option` via `if let Some(mut ws) = ...`). See `decisions.md`
    /// for the WebSocket-failure-handling rationale.
    WsConnection,
    /// T2 (v1.13 wave 1): the sending half of a Buff MPSC channel.
    /// Constructed via `Channel.new(buf_size)` (returns a tuple
    /// `(Sender<T>, Receiver<T>)`); carries the instance method
    /// `.send(value: T)` (async via auto-await). Maps to
    /// `buff_lang_runtime::Sender<T>` at codegen time.
    Sender,
    /// T2 (v1.13 wave 1): the receiving half of a Buff MPSC channel.
    /// Constructed via `Channel.new(buf_size)` (returns a tuple
    /// `(Sender<T>, Receiver<T>)`); carries the instance methods
    /// `.recv() -> Option<T>` (async via auto-await) and `.close()`
    /// (sync). Maps to `buff_lang_runtime::Receiver<T>` at codegen
    /// time. Single-consumer MPSC ONLY for the MVP.
    Receiver,
    /// T9: a 2D raster image (8-bit RGBA pixel data), mapped to
    /// `buff_image::Image` at codegen time. Constructed via
    /// `Image.from_path(path)` (load from disk) or `Image.from_bytes
    /// (bytes)` (decode an in-memory buffer); supports the instance
    /// methods `.width()`, `.height()`, `.get_pixel(x,y)`,
    /// `.set_pixel(x,y,color)`, `.save(path)`, `.grayscale()`,
    /// `.invert()`, `.resize(w,h)`, `.crop(x,y,w,h)`, `.blur(sigma)`.
    ///
    /// This is **additive** (T9): no existing variant was renamed,
    /// reordered, or had its payload altered. All exhaustive `match`es
    /// on `Type` will be extended with an arm for the new variant:
    /// `Display`, `buff_type_to_syn` (codegen), `is_prelude_image`
    /// predicate. The `is_numeric` / `is_float_like` / `is_integer_like`
    /// / `is_gpu_eligible` predicates all return `false` for `Image`.
    ///
    /// Mirrors [`Type::Regex`] (T124d) / [`Type::Url`] (T124h) /
    /// [`Type::Path`] (T124j) / [`Type::Process`] (T124l) as the
    /// fifth runtime-value-with-rich-instance-methods type. The
    /// underlying Rust type is `buff_image::Image` (a struct wrapping
    /// `image::DynamicImage`) — the codegen emits
    /// `buff_image::Image::from_path(p)?` / `buff_image::Image::from
    /// _bytes(b)?` for the constructors and `recv.width()` /
    /// `recv.height()` / `recv.get_pixel(x,y)?` / `recv.set_pixel
    /// (x,y,c)?` / `recv.save(p)?` / `recv.grayscale()` /
    /// `recv.invert()` / `recv.resize(w,h)?` / `recv.crop(x,y,w,h)?`
    /// / `recv.blur(sigma)` for the 10 instance methods. Pure-Rust,
    /// CPU-only per Metis G7 lock (NO GPU dispatch).
    Image,
    /// T37: a fake-data generator, mapped to `buff_fake::Faker` at
    /// codegen time. Constructed via `Faker.new()` (default locale,
    /// random seed), `Faker.with_locale(locale)`, or
    /// `Faker.with_seed(locale, seed)`; supports the instance methods
    /// `.name()`, `.email()`, `.address()`, `.phone()`, `.uuid()`,
    /// `.lorem(words)`, `.int(min, max)`, `.datetime(start, end)`.
    ///
    /// This is **additive** (T37): no existing variant was renamed,
    /// reordered, or had its payload altered. All exhaustive `match`es
    /// on `Type` will be extended with an arm for the new variant:
    /// `Display`, `buff_type_to_syn` (codegen), `is_prelude_faker`
    /// predicate. The `is_numeric` / `is_float_like` / `is_integer_like`
    /// / `is_gpu_eligible` predicates all return `false` for `Faker`.
    ///
    /// Mirrors [`Type::Image`] (T9) as a runtime-value-with-rich-
    /// instance-methods type. The underlying Rust type is
    /// `buff_fake::Faker` (a struct wrapping a locale + seeded StdRng).
    /// Pure-Rust, no native deps.
    Faker,
    /// T31 (v1.16 frameworks): the in-memory Cache runtime-value type.
    /// Maps to `buff_cache::Cache` at codegen time. Constructed via
    /// `Cache.new(max_capacity)`; carries the instance methods
    /// `.get(key)`, `.set(key, value)`, `.set(key, value, ttl)`,
    /// `.delete(key)`, `.contains(key)`, `.clear()`, `.len()`.
    /// Wraps `moka::sync::Cache<String, (String, Option<Instant>)>`
    /// behind an `Arc` (Send + Sync, clone-cheap). Per-entry TTL via
    /// stored deadline (lazy eviction on get/contains). Distributed
    /// Redis backend DEFERRED to v1.18+ per the T31 task spec.
    Cache,
    /// T44 (v1.17 frameworks): the internationalization runtime-value
    /// type. Maps to `buff_i18n::I18n` at codegen time. Constructed
    /// via `I18n.new(locale)` / `I18n.with_fallback(locale, fallback)`;
    /// carries the instance methods `.add_resource(locale, ftl)`,
    /// `.load(locale)`, `.set_fallback(locale)`,
    /// `.available_locales()`, `.current_locale()`,
    /// `.fallback_locale()`, `.translate(key)`,
    /// `.translate_with_args(key, args)`, `.has_message(key)`,
    /// `.warnings()`. Wraps `Arc<Mutex<I18nInner>>` (Send + Sync,
    /// clone-cheap). Per T44 spec: NO machine translation, NO RTL
    /// layout helpers (UI concern).
    I18n,
    /// T7 (v1.13 frameworks): the columnar-DataFrame runtime-value
    /// type. Maps to `buff_dataframe::DataFrame` at codegen time.
    /// Constructed via `DataFrame.from_csv(path)` /
    /// `DataFrame.from_json(path)`; carries the instance methods
    /// `.select(cols)`, `.filter(pred)`, `.sort(col)`, `.head(n)`,
    /// `.len()`, `.join(other, on)`, `.group_by(col)` (returns a
    /// DataFrame whose `.agg(col, op)` chains per-group aggregation),
    /// `.to_table_string()`. CPU-only per Metis G7 — no GPU dispatch.
    DataFrame,
    /// T10 (v1.13 frameworks): the runtime-value AudioBuffer type.
    /// Maps to `buff_audio::AudioBuffer` at codegen time. Constructed
    /// via `AudioBuffer.from_path(path)` (decode WAV/MP3/FLAC/Vorbis)
    /// or `AudioBuffer.from_samples(samples, sample_rate, channels)`;
    /// carries the instance methods `.samples()`, `.sample_rate()`,
    /// `.channels()`, `.duration_secs()`, `.save(path)`, `.amplify
    /// (factor)`, `.normalize(target)`, `.mix(other)`, `.slice(start,
    /// end)`.
    ///
    /// This is **additive** (T10): no existing variant was renamed,
    /// reordered, or had its payload altered. All exhaustive `match`es
    /// on `Type` will be extended with an arm for the new variant:
    /// `Display`, `buff_type_to_syn` (codegen), `is_prelude_audio`
    /// predicate. The `is_numeric` / `is_float_like` / `is_integer_like`
    /// / `is_gpu_eligible` predicates all return `false` for `Audio`.
    ///
    /// Mirrors [`Type::Regex`] (T124d) / [`Type::Url`] (T124h) /
    /// [`Type::Path`] (T124j) / [`Type::Process`] (T124l) / [`Type::Image`]
    /// (T9) as a runtime-value-with-rich-instance-methods type. The
    /// underlying Rust type is `buff_audio::AudioBuffer` (a struct
    /// wrapping interleaved `Vec<f32>` + sample_rate + channels) — the
    /// codegen emits `buff_audio::AudioBuffer::from_path(p)?` /
    /// `buff_audio::AudioBuffer::from_samples(s, sr, ch)?` for the
    /// constructors and the eight instance methods lowering to
    /// `recv.samples()` / `recv.sample_rate()` / `recv.channels()` /
    /// `recv.duration_secs()` / `recv.save(p)?` / `recv.amplify(f)` /
    /// `recv.normalize(t)` / `recv.mix(&o)?` / `recv.slice(s, e)?`.
    /// Pure-Rust, CPU-only per Metis G7 lock (NO GPU dispatch). The
    /// MVP forbids real-time playback (deferred to v1.18+) and
    /// synthesis (that's buff-dsp T11).
    Audio,
    /// T12: the Buff `World` runtime-value type — an
    /// Entity-Component-System store mapped to `buff_ecs::World` at
    /// codegen time. Constructed via the prelude associated function
    /// `World.new()` (no args; returns an empty `World`); carries
    /// many instance methods (`world.spawn(component) -> Entity`,
    /// `world.spawn_two(a, b) -> Entity`, `world.insert(entity,
    /// component)`, `world.remove::<T>(entity)`, `world.query::<T>()
    /// -> Vector<Tuple>`, `world.for_each_mut(closure)`,
    /// `world.for_each_pair_mut(closure)`, `world.add_system(system)`,
    /// `world.tick()`, `world.insert_resource(value)`,
    /// `world.get_resource::<T>()`, ...).
    ///
    /// This is **additive** (T12): no existing variant was renamed,
    /// reordered, or had its payload altered. The `is_numeric` /
    /// `is_float_like` / `is_integer_like` / `is_gpu_eligible`
    /// predicates all return `false` for `World` — it's an opaque
    /// value type that participates in no numeric promotion.
    ///
    /// Mirrors [`Type::Regex`] (T124d) / [`Type::Path`] (T124j) /
    /// [`Type::Process`] (T124l) / [`Type::Image`] (T9) as a
    /// runtime-value-with-rich-instance-methods type. The underlying
    /// Rust type is `buff_ecs::World` — the codegen emits
    /// `buff_ecs::World::new()` for the ctor and dispatches the
    /// instance methods to `World::*` paths.
    World,
    /// T12: an opaque ECS entity id, mapped to `buff_ecs::Entity` at
    /// codegen time. Constructed as the return value of
    /// `world.spawn(component)` / `world.spawn_two(a, b)`; carries
    /// the instance method `.id() -> Int`. Copy + Eq + Hash so Buff
    /// users can store entities in collections and compare them by
    /// value.
    ///
    /// This is **additive** (T12): no existing variant was renamed,
    /// reordered, or had its payload altered. The `is_numeric` /
    /// `is_float_like` / `is_integer_like` / `is_gpu_eligible`
    /// predicates all return `false` for `Entity`. Underlying Rust
    /// type `buff_ecs::Entity` is a transparent newtype over
    /// `hecs::Entity` (`(u32, u32)` id+generation pair) — no raw
    /// pointers, no lifetimes, `Copy + Send + Sync + 'static`.
    Entity,
    /// T19: the `Template` runtime-value type — a compiled HTML
    /// template wrapping `buff_template::Template` (itself wrapping
    /// `handlebars::Handlebars`). Constructed via the associated
    /// functions `Template.from_string(src)` /
    /// `Template.from_path(path)`; carries the instance method
    /// `template.render(context_json) -> String`. Added by T31
    /// (this commit) because T19 added the codegen `M::Render` arm
    /// but missed the matching `Type::Template` variant — codegen
    /// cannot compile otherwise.
    Template,
    /// T33: the `HttpClient` runtime-value type — an idiomatic HTTP
    /// client wrapping `reqwest::blocking::Client` via a safe FFI
    /// boundary per the T4 FFI guide. Constructed via the prelude
    /// associated function `HttpClient.new()` (returns a new client
    /// with default settings); carries the instance methods
    /// `client.get(url)`, `client.post(url)`, `client.put(url)`,
    /// `client.delete(url)` — each returning a `RequestBuilder`
    /// (opaque, typed `Type::Unknown` for MVP). The `RequestBuilder`
    /// carries `.header(name, val)`, `.json(body)`, `.timeout(secs)`,
    /// `.send()` (returns `Response`, also opaque for MVP). The
    /// `Response` carries `.status()`, `.text()`, `.json()`,
    /// `.bytes()`, `.headers()`.
    ///
    /// This is **additive** (T33): no existing variant was renamed,
    /// reordered, or had its payload altered. The `is_numeric` /
    /// `is_float_like` / `is_integer_like` / `is_gpu_eligible`
    /// predicates all return `false` for `HttpClient`. Underlying
    /// Rust type is `buff_http_client::HttpClient` (a struct wrapping
    /// `reqwest::blocking::Client`). Pure-Rust, CPU-only.
    HttpClient,
    /// T29: the `Validator` runtime-value type — a declarative schema
    /// validator (pydantic-equivalent) wrapping
    /// `buff_validate::Validator` at codegen time. Constructed via
    /// `Validator.new()` (empty rule set); carries the builder
    /// instance methods `.with_email(field)`,
    /// `.with_url(field)`, `.with_length(field, min, max)`,
    /// `.with_range(field, min, max)`, `.with_regex(field, pattern)`,
    /// each returning a new Validator (Buff "no visible references"
    /// stance — builders consume self); plus the action methods
    /// `.validate(map) -> Result<Void, String>` and
    /// `.to_json_schema() -> String`.
    ///
    /// This is **additive** (T29): no existing variant was renamed,
    /// reordered, or had its payload altered. The `is_numeric` /
    /// `is_float_like` / `is_integer_like` / `is_gpu_eligible`
    /// predicates all return `false` for `Validator`. Underlying
    /// Rust type is `buff_validate::Validator` (a struct wrapping
    /// `Vec<Rule>` where `Rule` is an internal enum). Pure-Rust,
    /// CPU-only. The MVP wraps `validator::ValidateEmail` /
    /// `ValidateUrl` / `ValidateLength` / `ValidateRange` trait
    /// methods on `&str` / integer values; NO derive macros (T29
    /// must-not #1: "no compile-time macro validation").
    Validator,
    /// T42: the `Email` runtime-value type — a buildable email
    /// message wrapping `buff_email::Email` at codegen time.
    /// Constructed via the prelude associated function
    /// `Email.new(from, to, subject)` (validates RFC 5322 mailboxes
    /// up front via `lettre::message::Mailbox::from_str`); carries
    /// the builder instance methods `email.body(text)` /
    /// `email.html(template, context_json)` / `email.attach(path)`,
    /// each consuming `self` and returning a new `Email` (Buff "no
    /// visible references" stance — builders consume self, matches
    /// the Validator / HttpClient surface).
    ///
    /// This is **additive** (T42): no existing variant was renamed,
    /// reordered, or had its payload altered. The `is_numeric` /
    /// `is_float_like` / `is_integer_like` / `is_gpu_eligible`
    /// predicates all return `false` for `Email`. Underlying Rust
    /// type is `buff_email::Email` (a struct wrapping the
    /// constituent parts — from / to / subject / optional plain
    /// body / optional rendered HTML body / queued attachments).
    /// Pure-Rust, CPU-only. The MVP wraps `lettre::Message::builder`
    /// + `lettre::message::{MultiPart, Attachment}` at send time.
    Email,
    /// T42: the `SmtpClient` runtime-value type — a configured SMTP
    /// transport wrapping `buff_email::SmtpClient` at codegen time.
    /// Constructed via the prelude associated function
    /// `SmtpClient.new(host, port, username, password)` (configures
    /// STARTTLS via `lettre::SmtpTransport::relay`); carries the
    /// single instance method `client.send(email) -> Result<Void,
    /// EmailError>`. The underlying TLS is pure-Rust rustls (NOT
    /// native-tls per AGENTS.md hard rule).
    ///
    /// This is **additive** (T42): no existing variant was renamed,
    /// reordered, or had its payload altered. The `is_numeric` /
    /// `is_float_like` / `is_integer_like` / `is_gpu_eligible`
    /// predicates all return `false` for `SmtpClient`. Underlying
    /// Rust type is `buff_email::SmtpClient` (a struct wrapping
    /// `lettre::SmtpTransport`). Pure-Rust, CPU-only. IMAP / POP3
    /// receiving explicitly deferred to v1.22+ per T42 must-not #1.
    SmtpClient,
    /// T43: the `Document` runtime-value type — a parsed HTML
    /// document wrapping `buff_scrape::Document` (itself wrapping a
    /// cached `String` source + lazy `scraper::Html` rebuild per
    /// access). Constructed via the associated function
    /// `Document.from_html(html)`; carries 4 instance methods:
    /// `doc.select(css)`, `doc.text()`, `doc.html()`,
    /// `doc.title()`. Pure-Rust scraper backend (no JS rendering
    /// per T43 spec).
    ///
    /// This is **additive** (T43): no existing variant was renamed,
    /// reordered, or had its payload altered. The `is_numeric` /
    /// `is_float_like` / `is_integer_like` / `is_gpu_eligible`
    /// predicates all return `false` for `Document`. Underlying
    /// Rust type is `buff_scrape::Document` (a struct wrapping an
    /// owned `String` source — `scraper::Html` is rebuilt per
    /// `select`/`text`/`title` call because scraper's `Html` is
    /// `!Send + !Sync`; the wrapper stays `Send + Sync + Clone`).
    Document,
    /// T43: the `Element` runtime-value type — a single selected
    /// HTML element wrapping `buff_scrape::Element`. Constructed as
    /// the return value of `Document.select(css)` /
    /// `Element.select(css)`; carries 5 instance methods:
    /// `el.text()`, `el.attr(name)`, `el.html()`, `el.inner_html()`,
    /// `el.select(css)`. Owned (`'static + Send + Sync + Clone`) —
    /// text/html/inner_html/attrs are cached eagerly at construction.
    ///
    /// This is **additive** (T43). The `is_numeric` / `is_float_like`
    /// / `is_integer_like` / `is_gpu_eligible` predicates all return
    /// `false` for `Element`. Underlying Rust type is
    /// `buff_scrape::Element` (a struct wrapping owned `String`s +
    /// `BTreeMap<String, String>` attrs — no raw pointers, no
    /// lifetimes, satisfies FFI guide R1/R4/R5).
    Element,
    /// T43: the `Crawler` runtime-value type — an HTTP crawler
    /// wrapping `buff_scrape::Crawler`. Constructed via the
    /// associated function `Crawler.new(seed_url)`; carries 4
    /// instance methods: `crawler.seed()`, `crawler.fetch(url)`,
    /// `crawler.crawl(max_pages)`, `crawler.robots_allows(url)`.
    /// Single-host BFS, robots.txt-aware (fail-open on missing
    /// robots.txt). NO distributed crawling (forbidden by T43 spec).
    ///
    /// This is **additive** (T43). Underlying Rust type is
    /// `buff_scrape::Crawler` (a struct wrapping
    /// `reqwest::blocking::Client` + seed `String`). Pure-Rust TLS
    /// via rustls (NOT native-tls).
    Crawler,
    /// T51: a MessagePack binary format namespace, mapped to
    /// `buff_msgpack` at codegen time. Constructed via the
    /// associated functions `MsgPack.serialize(value) -> Bytes`
    /// and `MsgPack.deserialize(bytes) -> Value`. This is a
    /// namespace-only type (like `Log` / `Toml` / `Base64` /
    /// `Hex` / `Yaml` / `Csv`) — it has no runtime value
    /// representation; `buff_type()` returns [`Type::Void`].
    ///
    /// This is **additive** (T51): no existing variant was renamed,
    /// reordered, or had its payload altered. All exhaustive `match`es
    /// on `Type` will be extended with an arm for the new variant:
    /// `Display`, `buff_type_to_syn` (codegen), `is_prelude_msgpack`
    /// predicate. The `is_numeric` / `is_float_like` / `is_integer_like`
    /// / `is_gpu_eligible` predicates all return `false` for `MsgPack`.
    ///
    /// Mirrors [`Type::Base64`] / [`Type::Hex`] / [`Type::Yaml`] /
    /// [`Type::Csv`] as a namespace-only prelude type. The underlying
    /// Rust crate is `buff_msgpack` (wrapping `rmp_serde`) — the
    /// codegen emits `buff_msgpack::serialize(&value).unwrap_or_default()`
    /// / `buff_msgpack::deserialize(&bytes).unwrap_or_default()` for
    /// the two associated functions. Pure-Rust, no native deps.
    MsgPack,
    /// T50: an XML document runtime value, mapped to
    /// `buff_xml::XmlDocument` at codegen time. Constructed via the
    /// associated function `Xml.from_str(xml) -> XmlDocument`; carries
    /// the instance methods `.root()`, `.find(xpath)`, `.to_string()`.
    /// This is **additive** (T50). Pure-Rust, CPU-only.
    Xml,
    /// T50: an XML element runtime value, mapped to
    /// `buff_xml::XmlElement` at codegen time. Returned by
    /// `XmlDocument.root()` / `XmlDocument.find(xpath)`; constructed
    /// directly via `XmlElement.new(name, text, attrs)`. Carries the
    /// instance methods `.name()`, `.attr(name)`, `.text()`,
    /// `.children()`. This is **additive** (T50). Pure-Rust, CPU-only.
    XmlElement,
    /// T45: a 2D geospatial point with `f64` coordinates, mapped to
    /// `buff_geo::Point` at codegen time. Constructed via the associated
    /// function `Point.new(x, y)`; carries the instance methods `.x()`,
    /// `.y()`, `.distance_to(other)`. CPU-only per Metis G7 lock (NO
    /// GPU dispatch).
    ///
    /// This is **additive** (T45): no existing variant was renamed,
    /// reordered, or had its payload altered. The `is_numeric` /
    /// `is_float_like` / `is_integer_like` / `is_gpu_eligible`
    /// predicates all return `false` for `Point`.
    ///
    /// Mirrors [`Type::Image`] (T9) / [`Type::Regex`] (T124d) as a
    /// runtime-value-with-rich-instance-methods type. The underlying
    /// Rust type is `buff_geo::Point` (a struct wrapping
    /// `geo_types::Point<f64>`) — the codegen emits
    /// `buff_geo::Point::new(x, y)` for the ctor and `recv.x()` /
    /// `recv.y()` / `recv.distance_to(other)` for the instance methods.
    /// Pure-Rust, CPU-only.
    Point,
    /// T45: a geospatial polyline — an ordered sequence of [`Point`]s,
    /// mapped to `buff_geo::LineString` at codegen time. Constructed via
    /// `LineString.new(points)` or `LineString.from_coords(flat)`; carries
    /// the instance method `.length()`. CPU-only per Metis G7 lock.
    ///
    /// This is **additive** (T45). The `is_numeric` / `is_float_like` /
    /// `is_integer_like` / `is_gpu_eligible` predicates all return `false`.
    ///
    /// Mirrors [`Type::Image`] (T9) / [`Type::Point`] as a
    /// runtime-value-with-instance-methods type. The underlying Rust type
    /// is `buff_geo::LineString` (wrapping `geo_types::LineString<f64>`).
    LineString,
    /// T45: a geospatial polygon — an outer ring + future interior holes,
    /// mapped to `buff_geo::Polygon` at codegen time. Constructed via
    /// `Polygon.new(ring)` or `Polygon.from_coords(flat)`; carries the
    /// instance methods `.area()`, `.contains(point)`,
    /// `.intersects(other)`. CPU-only per Metis G7 lock.
    ///
    /// This is **additive** (T45). The `is_numeric` / `is_float_like` /
    /// `is_integer_like` / `is_gpu_eligible` predicates all return `false`.
    ///
    /// Mirrors [`Type::Image`] (T9) / [`Type::Point`] as a
    /// runtime-value-with-instance-methods type. The underlying Rust type
    /// is `buff_geo::Polygon` (wrapping `geo_types::Polygon<f64>`). The
    /// MVP supports only the outer ring (no holes); holes are a v1.18+
    /// enhancement.
    Polygon,
    /// T46: the `Text` NLP namespace, mapped to `buff_nlp::Text` at
    /// codegen time. Namespace-only (like `MsgPack` / `Log` / `Toml`) —
    /// the type itself is never instantiated as a runtime value; only
    /// its associated functions are callable (`Text.detect_language` /
    /// `Text.stem` / `Text.tokenize` / `Text.sentences`). `buff_type()`
    /// returns [`Type::Text`] for match-exhaustiveness (mirrors
    /// [`Type::MsgPack`]); the codegen arm rarely fires in practice.
    ///
    /// This is **additive** (T46). The `is_numeric` / `is_float_like` /
    /// `is_integer_like` / `is_gpu_eligible` predicates all return `false`.
    ///
    /// Mirrors [`Type::MsgPack`] (T51) as a namespace-only type that
    /// nonetheless carries a `Type` variant for exhaustiveness. The
    /// underlying Rust namespace is `buff_nlp::Text` (a unit struct
    /// namespace marker — never instantiated). Pure-Rust, CPU-only.
    Text,
    /// T46: a detected natural language, mapped to `buff_nlp::Language`
    /// at codegen time. Constructed ONLY via
    /// `Text.detect_language(input) -> Option<Language>`; carries the
    /// instance methods `.code() -> String` (ISO 639-3) and
    /// `.name() -> String` (English name). CPU-only.
    ///
    /// This is **additive** (T46). The `is_numeric` / `is_float_like` /
    /// `is_integer_like` / `is_gpu_eligible` predicates all return `false`.
    ///
    /// Mirrors [`Type::Image`] (T9) / [`Type::Point`] (T45) as a
    /// runtime-value-with-instance-methods type. The underlying Rust type
    /// is `buff_nlp::Language` (a struct wrapping `whatlang::Lang`).
    Language,
    /// T46: a Snowball stemming algorithm selector (18 supported
    /// languages), mapped to `buff_nlp::StemAlgorithm` at codegen time.
    /// Opaque enum — only passed as an arg to `Text.stem(word,
    /// algorithm)`; NO instance methods exposed. Buff users write the
    /// variants as `.english` / `.portuguese` / etc. (enum-variant
    /// literal syntax).
    ///
    /// This is **additive** (T46). The `is_numeric` / `is_float_like` /
    /// `is_integer_like` / `is_gpu_eligible` predicates all return `false`.
    ///
    /// Mirrors no prior type exactly — it is the first opaque enum
    /// passed-only-as-arg in the prelude. The underlying Rust type is
    /// `buff_nlp::StemAlgorithm` (an enum with 18 variants matching
    /// `rust_stemmers::Algorithm` 1:1).
    StemAlgorithm,
    /// T52: the `Protobuf` Protocol-Buffers format namespace, mapped
    /// to `buff_protobuf` at codegen time. Namespace-only (like
    /// `MsgPack` / `Log` / `Toml` / `Base64` / `Hex` / `Yaml` / `Csv`)
    /// — the type itself is never instantiated as a runtime value;
    /// only its associated functions are callable
    /// (`Protobuf.serialize(value) -> Bytes`,
    /// `Protobuf.deserialize(bytes) -> Value`,
    /// `Protobuf.roundtrip(value) -> Option<Value>`). `buff_type()`
    /// returns [`Type::Protobuf`] for match-exhaustiveness (mirrors
    /// [`Type::MsgPack`]); the codegen arm rarely fires in practice.
    ///
    /// This is **additive** (T52): no existing variant was renamed,
    /// reordered, or had its payload altered. All exhaustive `match`es
    /// on `Type` will be extended with an arm for the new variant:
    /// `Display`, `buff_type_to_syn` (codegen), `is_prelude_protobuf`
    /// predicate. The `is_numeric` / `is_float_like` / `is_integer_like`
    /// / `is_gpu_eligible` predicates all return `false` for `Protobuf`.
    ///
    /// Mirrors [`Type::MsgPack`] (T51) as the closest sibling — both
    /// are namespace-only binary-format modules wrapping pure-Rust
    /// codec crates. The underlying Rust crate is `buff_protobuf`
    /// (wrapping `prost` + `prost-types`); the codegen emits
    /// `buff_protobuf::serialize(&value).unwrap_or_default()` /
    /// `buff_protobuf::deserialize(&bytes).unwrap_or_default()` /
    /// `buff_protobuf::roundtrip(&value)` for the three associated
    /// functions. Pure-Rust, no native deps (NO protoc / NO protoc-built
    /// .proto codegen in MVP — gRPC streaming + prost-build deferred).
    Protobuf,
    /// T52: a protobuf-encoded message runtime value, mapped to
    /// `buff_protobuf::Message` at codegen time. Constructed via
    /// `Message.new(value)` (encode a `Value` to protobuf wire-format
    /// bytes) or `Message.from_bytes(bytes)` / `Message.decode(bytes)`
    /// (decode raw wire-format bytes); carries the instance methods
    /// `.byte_size() -> Int`, `.type_url() -> String`,
    /// `.payload() -> Value`, `.encode() -> Vector<Byte>`.
    ///
    /// This is **additive** (T52). The `is_numeric` / `is_float_like` /
    /// `is_integer_like` / `is_gpu_eligible` predicates all return `false`.
    ///
    /// Mirrors [`Type::Image`] (T9) / [`Type::Xml`] (T50) as a
    /// runtime-value-with-rich-instance-methods type. The underlying
    /// Rust type is `buff_protobuf::Message` (a struct wrapping an
    /// owned `Vec<u8>` payload + the canonical
    /// `type.googleapis.com/google.protobuf.Struct` type URL). Pure-Rust,
    /// CPU-only; the well-known `google.protobuf.Struct` schema is the
    /// dynamic message surface (no `.proto` build-time codegen in MVP).
    Message,
}

/// The width of an integer type (`Int` or `Bits`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IntWidth {
    W8,
    W16,
    W32,
    W64,
    W128,
}

/// The width of a floating-point type (`Float`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FloatWidth {
    W16,
    W32,
    W64,
}

impl Type {
    /// The default integer type: `Int<64>`.
    pub fn int_default() -> Self {
        Type::Int {
            width: IntWidth::W64,
        }
    }

    /// The default float type: `Float<32>`.
    pub fn float_default() -> Self {
        Type::Float {
            width: FloatWidth::W32,
        }
    }

    /// The 64-bit float type: `Double`.
    pub fn double() -> Self {
        Type::Double
    }

    /// The byte type: `Bits<8>`.
    pub fn byte() -> Self {
        Type::Bits {
            width: IntWidth::W8,
        }
    }

    /// The boolean type: `Bool`.
    pub fn bool() -> Self {
        Type::Bool
    }

    /// The string type: `String`.
    pub fn string() -> Self {
        Type::String
    }

    /// The char type: `Char` (a single Unicode scalar value). (T21.)
    pub fn char() -> Self {
        Type::Char
    }

    /// Returns `true` if this type is numeric (integer, byte, float, double, or decimal).
    pub fn is_numeric(&self) -> bool {
        matches!(
            self,
            Type::Int { .. }
                | Type::Bits { .. }
                | Type::Float { .. }
                | Type::Double
                | Type::Decimal
        )
    }

    /// Returns `true` if this type is floating-point-like
    /// (`Float`, `Double`, or `Decimal`).
    pub fn is_float_like(&self) -> bool {
        matches!(self, Type::Float { .. } | Type::Double | Type::Decimal)
    }

    /// Returns `true` if this type is integer-like (`Int` or `Bits`).
    pub fn is_integer_like(&self) -> bool {
        matches!(self, Type::Int { .. } | Type::Bits { .. })
    }

    /// Returns `true` if this type is eligible for GPU (WGSL) dispatch.
    ///
    /// Only the WGSL-native 32-bit scalar primitives are eligible:
    /// `Float<32>`, `Int<32>`, `Bits<32>`, and `Bool`. Wider widths,
    /// `Double` (f64 has no WGSL scalar), and especially [`Type::Decimal`]
    /// (128-bit fixed-point, no GPU representation) are **not** GPU-eligible
    /// and must run on the CPU (Rayon) path.
    ///
    /// This is **type metadata only** in v0.5 — there is no dispatch engine
    /// yet (that arrives in v1.0). The predicate is consumed directly by
    /// tests now and will feed the v1.0 heterogeneous dispatch analyzer.
    pub fn is_gpu_eligible(&self) -> bool {
        matches!(
            self,
            Type::Float {
                width: FloatWidth::W32
            } | Type::Int {
                width: IntWidth::W32
            } | Type::Bits {
                width: IntWidth::W32
            } | Type::Bool
        )
    }

    /// Create a `Vector<T>` type.
    pub fn vector(elem: Type) -> Self {
        Type::Vector(Box::new(elem))
    }

    /// Create a `Matrix<T>` type (T24). The element type is the inner `T`;
    /// storage is a flat `Vec<T>` in the emitted Rust struct.
    pub fn matrix(elem: Type) -> Self {
        Type::Matrix(Box::new(elem))
    }

    /// Create an `Option<T>` type.
    pub fn option(inner: Type) -> Self {
        Type::Option(Box::new(inner))
    }

    /// Create a `Map<K, V>` type (T25). Maps to Rust's
    /// `std::collections::HashMap<K, V>`. Both params are boxed so the
    /// enum variant carries them inline without recursion through the
    /// enum's own padding.
    pub fn map(key: Type, value: Type) -> Self {
        Type::Map(Box::new(key), Box::new(value))
    }

    /// Create a `Result<T, E>` type (T30). Maps 1:1 to Rust's
    /// `std::result::Result<T, E>`. Mirrors [`Type::option`] (T28) for the
    /// error-handling prelude enum. Both params are boxed, mirroring
    /// [`Type::map`].
    pub fn result(ok: Type, err: Type) -> Self {
        Type::Result(Box::new(ok), Box::new(err))
    }

    /// Create a tuple type `(T, U, ...)` from its resolved members (T103).
    /// Maps 1:1 to a Rust tuple. The caller MUST pass 2+ members (the
    /// parser disallows single-element tuples, but this constructor does
    /// not enforce it — a single-element `Tuple` is technically
    /// constructible here for testing; downstream code treats it the same).
    pub fn tuple(members: Vec<Type>) -> Self {
        Type::Tuple(members)
    }

    /// T124b: the timezone-aware datetime type. Maps to
    /// `chrono::DateTime<chrono::Utc>` at codegen time.
    pub fn datetime() -> Self {
        Type::DateTime
    }

    /// T124b: the calendar-date type. Maps to `chrono::NaiveDate`.
    pub fn date() -> Self {
        Type::Date
    }

    /// T124b: the clock-time type. Maps to `chrono::NaiveTime`.
    pub fn time() -> Self {
        Type::Time
    }

    /// T124b: the time-span type. Maps to `chrono::TimeDelta`.
    pub fn duration() -> Self {
        Type::Duration
    }

    /// T124b: the monotonic-instant type. Maps to `std::time::Instant`.
    pub fn instant() -> Self {
        Type::Instant
    }

    /// T124b: Returns `true` if this type is one of the prelude datetime
    /// family (`DateTime`, `Date`, `Time`, `Duration`, `Instant`). Used by
    /// the type inferencer + codegen to dispatch method calls on these
    /// types to the chrono / std::time lowering.
    pub fn is_prelude_datetime(&self) -> bool {
        matches!(
            self,
            Type::DateTime | Type::Date | Type::Time | Type::Duration | Type::Instant
        )
    }

    /// T124d: the compiled-regex type. Maps to `regex::Regex` at codegen
    /// time. Constructed via `Regex.compile(pattern)`; supports
    /// `regex.match(text)`, `regex.find(text)`,
    /// `regex.replace(text, repl)`, `regex.captures(text)`.
    pub fn regex() -> Self {
        Type::Regex
    }

    /// T124h: the parsed-URL type. Maps to `url::Url` at codegen time.
    /// Constructed via `URL.parse(s)`; supports `.scheme` / `.host` /
    /// `.path` accessors and `.query(key) -> Option<String>`.
    pub fn url() -> Self {
        Type::Url
    }

    /// T124d: Returns `true` if this type is the prelude `Regex` runtime
    /// value. Used by the type inferencer + codegen to dispatch instance
    /// method calls (`regex.match(...)`, `regex.find(...)`, ...) to the
    /// `regex::Regex` lowering. Distinct from [`Self::is_prelude_datetime`]
    /// (Regex is not a datetime family member) and from the namespace-only
    /// check ([`crate::prelude_types::PreludeType::is_namespace_only`]) —
    /// `Regex` IS a runtime value (an opaque compiled-pattern handle).
    pub fn is_prelude_regex(&self) -> bool {
        matches!(self, Type::Regex)
    }

    /// T124h: Returns `true` if this type is the prelude `URL` runtime
    /// value. Used by the type inferencer + codegen to dispatch instance
    /// method calls (`url.scheme`, `url.host`, `url.path`,
    /// `url.query(k)`) to the `url::Url` lowering. Distinct from
    /// [`Self::is_prelude_datetime`] (URL is not a chrono type) and
    /// from [`Self::is_prelude_regex`] (URL is a different runtime
    /// value type). Used by the chrono over-broad-walker cautionary
    /// tale (T124f gotcha) — `buff_type().is_prelude_url()` is the
    /// narrow round-trip check for the URL type only.
    pub fn is_prelude_url(&self) -> bool {
        matches!(self, Type::Url)
    }

    /// T124j: the filesystem-path type. Maps to `std::path::PathBuf`
    /// at codegen time. Constructed via `Path.join(a, b, ...)`
    /// (variadic - chained PathBuf joins); supports instance methods
    /// `.parent()`, `.extension()`, `.basename()`, `.exists()`.
    pub fn path() -> Self {
        Type::Path
    }

    /// T124j: Returns `true` if this type is the prelude `Path`
    /// runtime value. Used by the type inferencer + codegen to
    /// dispatch instance method calls (`path.parent()`,
    /// `path.extension()`, `path.basename()`, `path.exists()`) to
    /// the `std::path::PathBuf` lowering. Distinct from
    /// [`Self::is_prelude_datetime`] (Path is not a chrono type),
    /// from [`Self::is_prelude_regex`] (Path is not a regex), and
    /// from [`Self::is_prelude_url`] (Path is not a URL). Used by
    /// the chrono over-broad-walker cautionary tale (T124f gotcha)
    /// — `buff_type().is_prelude_path()` is the narrow round-trip
    /// check for the Path type only.
    pub fn is_prelude_path(&self) -> bool {
        matches!(self, Type::Path)
    }

    /// T124l: the spawned-process type. Maps to
    /// `Option<std::process::Child>` at codegen time (the `Option`
    /// wrapper lets `Process.spawn` be panic-free — a spawn failure
    /// collapses to `None`). Constructed via `Process.spawn(cmd,
    /// args)`; supports instance methods `.wait() -> Int` (exit
    /// code) and `.id() -> Int` (OS process ID).
    pub fn process() -> Self {
        Type::Process
    }

    /// T124l: Returns `true` if this type is the prelude `Process`
    /// runtime value. Used by the type inferencer + codegen to
    /// dispatch instance method calls (`process.wait()`,
    /// `process.id()`) to the `std::process::Child` lowering.
    /// Distinct from [`Self::is_prelude_datetime`] (Process is not
    /// a chrono type), [`Self::is_prelude_regex`] (not a regex),
    /// [`Self::is_prelude_url`] (not a URL), and
    /// [`Self::is_prelude_path`] (not a Path). Used by the
    /// chrono-over-broad-walker cautionary tale (T124f gotcha) -
    /// `buff_type().is_prelude_process()` is the narrow round-trip
    /// check for the Process type only.
    pub fn is_prelude_process(&self) -> bool {
        matches!(self, Type::Process)
    }

    /// T124m: the TCP-connection type. Maps to
    /// `Option<tokio::net::TcpStream>` at codegen time. Constructed
    /// via `TCP.connect(host, port)`; supports `.send(data)`,
    /// `.recv() -> Vector<Byte>`, `.close()`.
    pub fn connection() -> Self {
        Type::Connection
    }

    /// T124m: Returns `true` if this type is the prelude `Connection`
    /// runtime value (TCP). Used by the type inferencer + codegen
    /// to dispatch instance method calls (`conn.send(...)`,
    /// `conn.recv()`, `conn.close()`) to the
    /// `tokio::net::TcpStream` lowering. Distinct from
    /// [`Self::is_prelude_datetime`] / [`Self::is_prelude_regex`] /
    /// [`Self::is_prelude_url`] / [`Self::is_prelude_path`] /
    /// [`Self::is_prelude_process`] / [`Self::is_prelude_socket`] /
    /// [`Self::is_prelude_ws_connection`].
    pub fn is_prelude_connection(&self) -> bool {
        matches!(self, Type::Connection)
    }

    /// T124m: the bound UDP-socket type. Maps to
    /// `Option<tokio::net::UdpSocket>` at codegen time. Constructed
    /// via `UDP.bind(host, port)`; supports `.send_to(data, addr)`,
    /// `.recv_from() -> Tuple`.
    pub fn socket() -> Self {
        Type::Socket
    }

    /// T124m: Returns `true` if this type is the prelude `Socket`
    /// runtime value (UDP). Used by the type inferencer + codegen
    /// to dispatch instance method calls (`sock.send_to(...)`,
    /// `sock.recv_from()`) to the `tokio::net::UdpSocket` lowering.
    /// Distinct from the other `is_prelude_*` predicates.
    pub fn is_prelude_socket(&self) -> bool {
        matches!(self, Type::Socket)
    }

    /// T124m: the WebSocket-connection type. Maps to
    /// `Option<tokio_tungstenite::WebSocketStream<...>>` at codegen
    /// time. Constructed via `WebSocket.connect(url)`; supports
    /// `.send(text)`, `.recv() -> String`, `.close()`.
    pub fn ws_connection() -> Self {
        Type::WsConnection
    }

    /// T124m: Returns `true` if this type is the prelude
    /// `WsConnection` runtime value (WebSocket). Used by the type
    /// inferencer + codegen to dispatch instance method calls
    /// (`ws.send(...)`, `ws.recv()`, `ws.close()`) to the
    /// `tokio_tungstenite::WebSocketStream` lowering. Distinct from
    /// the other `is_prelude_*` predicates.
    pub fn is_prelude_ws_connection(&self) -> bool {
        matches!(self, Type::WsConnection)
    }

    /// T2: the channel-sender type. Maps to
    /// `buff_lang_runtime::Sender<T>` at codegen time.
    pub fn sender() -> Self {
        Type::Sender
    }

    /// T2: Returns `true` if this type is the prelude `Sender` runtime
    /// value (channel sender). Used to dispatch `sender.send(...)` to
    /// the `buff_lang_runtime::Sender` lowering.
    pub fn is_prelude_sender(&self) -> bool {
        matches!(self, Type::Sender)
    }

    /// T2: the channel-receiver type. Maps to
    /// `buff_lang_runtime::Receiver<T>` at codegen time.
    pub fn receiver() -> Self {
        Type::Receiver
    }

    /// T2: Returns `true` if this type is the prelude `Receiver`
    /// runtime value (channel receiver). Used to dispatch
    /// `receiver.recv()` / `receiver.close()` to the
    /// `buff_lang_runtime::Receiver` lowering.
    pub fn is_prelude_receiver(&self) -> bool {
        matches!(self, Type::Receiver)
    }

    /// T9: the raster-image type. Maps to `buff_image::Image` at
    /// codegen time. Constructed via `Image.from_path(p)` /
    /// `Image.from_bytes(b)`; supports 10 instance methods
    /// (`width`, `height`, `get_pixel`, `set_pixel`, `save`,
    /// `grayscale`, `invert`, `resize`, `crop`, `blur`). CPU-only
    /// per Metis G7 lock.
    pub fn image() -> Self {
        Type::Image
    }

    /// T9: Returns `true` if this type is the prelude `Image` runtime
    /// value. Used to dispatch instance method calls
    /// (`img.width()`, `img.height()`, `img.get_pixel(x,y)`, ...,
    /// `img.blur(sigma)`) to the `buff_image::Image` lowering.
    pub fn is_prelude_image(&self) -> bool {
        matches!(self, Type::Image)
    }

    /// T37: the fake-data generator type. Maps to `buff_fake::Faker`
    /// at codegen time. Constructed via `Faker.new()` /
    /// `Faker.with_locale(locale)` / `Faker.with_seed(locale, seed)`;
    /// supports 8 instance methods (`name`, `email`, `address`,
    /// `phone`, `uuid`, `lorem`, `int`, `datetime`).
    pub fn faker() -> Self {
        Type::Faker
    }

    /// T37: Returns `true` if this type is the prelude `Faker`
    /// runtime value. Used to dispatch instance method calls
    /// (`faker.name()`, `faker.email()`, ...) to the
    /// `buff_fake::Faker` lowering.
    pub fn is_prelude_faker(&self) -> bool {
        matches!(self, Type::Faker)
    }

    /// T31: the in-memory Cache runtime-value type. Maps to
    /// `buff_cache::Cache` at codegen time. Constructed via
    /// `Cache.new(max_capacity)`; carries the instance methods
    /// `.get(key)`, `.set(key, value)`, `.set(key, value, ttl)`,
    /// `.delete(key)`, `.contains(key)`, `.clear()`, `.len()`.
    pub fn cache() -> Self {
        Type::Cache
    }

    /// T31: Returns `true` if this type is the prelude `Cache`
    /// runtime value. Used to dispatch instance method calls
    /// (`cache.get(k)`, `cache.set(k, v)`, `cache.delete(k)`, ...)
    /// to the `buff_cache::Cache` lowering.
    pub fn is_prelude_cache(&self) -> bool {
        matches!(self, Type::Cache)
    }

    /// T44: the internationalization runtime-value type. Maps to
    /// `buff_i18n::I18n` at codegen time. Constructed via
    /// `I18n.new(locale)` / `I18n.with_fallback(locale, fallback)`;
    /// carries 10 instance methods (add_resource / load /
    /// set_fallback / available_locales / current_locale /
    /// fallback_locale / translate / translate_with_args / has_message
    /// / warnings).
    pub fn i18n() -> Self {
        Type::I18n
    }

    /// T44: Returns `true` if this type is the prelude `I18n`
    /// runtime value. Used to dispatch instance method calls
    /// (`i18n.translate(k)`, `i18n.add_resource(l, f)`,
    /// `i18n.load(l)`, ...) to the `buff_i18n::I18n` lowering.
    pub fn is_prelude_i18n(&self) -> bool {
        matches!(self, Type::I18n)
    }

    /// T7: the columnar-DataFrame runtime-value type. Maps to
    /// `buff_dataframe::DataFrame` at codegen time. Constructed via
    /// `DataFrame.from_csv(path)` / `DataFrame.from_json(path)`;
    /// supports `df.select(cols)` / `df.filter(pred)` / `df.sort(col)`
    /// / `df.head(n)` / `df.len()` / `df.join(other, on)` /
    /// `df.group_by(col)` / `df.agg(col, op)`. CPU-only per Metis G7.
    pub fn dataframe() -> Self {
        Type::DataFrame
    }

    /// T7: Returns `true` if this type is the prelude `DataFrame`
    /// runtime value. Used to dispatch instance method calls
    /// (`df.select(...)`, `df.filter(...)`, `df.sort(...)`, ...,
    /// `df.group_by(...)`) to the `buff_dataframe::DataFrame`
    /// lowering.
    pub fn is_prelude_dataframe(&self) -> bool {
        matches!(self, Type::DataFrame)
    }

    /// T10: the runtime-value `AudioBuffer` type. Maps to
    /// `buff_audio::AudioBuffer` at codegen time. Constructed via
    /// `AudioBuffer.from_path(p)` / `AudioBuffer.from_samples(s, sr,
    /// ch)`; supports 9 instance methods (`samples`, `sample_rate`,
    /// `channels`, `duration_secs`, `save`, `amplify`, `normalize`,
    /// `mix`, `slice`). CPU-only per Metis G7 lock.
    pub fn audio() -> Self {
        Type::Audio
    }

    /// T10: Returns `true` if this type is the prelude `AudioBuffer`
    /// runtime value. Used to dispatch instance method calls
    /// (`buf.samples()`, `buf.sample_rate()`, `buf.channels()`, ...,
    /// `buf.slice()`) to the `buff_audio::AudioBuffer` lowering.
    pub fn is_prelude_audio(&self) -> bool {
        matches!(self, Type::Audio)
    }

    /// T12: the ECS `World` type. Maps to `buff_ecs::World` at
    /// codegen time.
    pub fn world() -> Self {
        Type::World
    }

    /// T12: Returns `true` if this type is the prelude `World`
    /// runtime value (ECS world). Used to dispatch instance method
    /// calls (`world.spawn(...)`, `world.tick()`, ...,
    /// `world.insert_resource(...)`) to the `buff_ecs::World`
    /// lowering.
    pub fn is_prelude_world(&self) -> bool {
        matches!(self, Type::World)
    }

    /// T12: the ECS `Entity` type (opaque id). Maps to
    /// `buff_ecs::Entity` at codegen time.
    pub fn entity() -> Self {
        Type::Entity
    }

    /// T12: Returns `true` if this type is the prelude `Entity`
    /// runtime value (ECS entity id). Used to dispatch instance
    /// method calls (`entity.id()`) to the `buff_ecs::Entity`
    /// lowering.
    pub fn is_prelude_entity(&self) -> bool {
        matches!(self, Type::Entity)
    }

    /// T33: Returns `true` if this type is the prelude `HttpClient`
    /// runtime value. Used to dispatch instance method calls
    /// (`client.get(url)`, `client.post(url)`, etc.) to the
    /// `buff_http_client::HttpClient` lowering.
    pub fn is_prelude_http_client(&self) -> bool {
        matches!(self, Type::HttpClient)
    }

    /// T29: the declarative-schema-validator type. Maps to
    /// `buff_validate::Validator` at codegen time. Constructed via
    /// `Validator.new()` (empty); supports the builder methods
    /// `v.with_email(field)`, `v.with_url(field)`,
    /// `v.with_length(field, min, max)`,
    /// `v.with_range(field, min, max)`,
    /// `v.with_regex(field, pattern)`, plus the action methods
    /// `v.validate(input)`, `v.to_json_schema()`.
    pub fn validator() -> Self {
        Type::Validator
    }

    /// T29: Returns `true` if this type is the prelude `Validator`
    /// runtime value. Used to dispatch instance method calls
    /// (`v.validate(...)`, `v.to_json_schema()`) to the
    /// `buff_validate::Validator` lowering.
    pub fn is_prelude_validator(&self) -> bool {
        matches!(self, Type::Validator)
    }

    /// T42: the buildable-email type. Maps to `buff_email::Email` at
    /// codegen time. Constructed via `Email.new(from, to, subject)`;
    /// supports the builder methods `email.body(text)`,
    /// `email.html(template, context_json)`, `email.attach(path)`.
    pub fn email() -> Self {
        Type::Email
    }

    /// T42: Returns `true` if this type is the prelude `Email`
    /// runtime value. Used to dispatch instance method calls
    /// (`email.body(...)`, `email.html(...)`, `email.attach(...)`)
    /// to the `buff_email::Email` lowering.
    pub fn is_prelude_email(&self) -> bool {
        matches!(self, Type::Email)
    }

    /// T42: the SMTP-client type. Maps to `buff_email::SmtpClient` at
    /// codegen time. Constructed via `SmtpClient.new(host, port,
    /// username, password)`; supports the instance method
    /// `client.send(email)`.
    pub fn smtp_client() -> Self {
        Type::SmtpClient
    }

    /// T42: Returns `true` if this type is the prelude `SmtpClient`
    /// runtime value. Used to dispatch instance method calls
    /// (`client.send(email)`) to the `buff_email::SmtpClient`
    /// lowering.
    pub fn is_prelude_smtp_client(&self) -> bool {
        matches!(self, Type::SmtpClient)
    }

    /// T43: the HTML-Document type. Maps to `buff_scrape::Document`
    /// at codegen time. Constructed via `Document.from_html(html)`;
    /// supports 4 instance methods (`select`, `text`, `html`,
    /// `title`).
    pub fn document() -> Self {
        Type::Document
    }

    /// T43: Returns `true` if this type is the prelude `Document`
    /// runtime value (parsed HTML tree). Used to dispatch instance
    /// method calls (`doc.select(...)`, `doc.text()`, ...,
    /// `doc.title()`) to the `buff_scrape::Document` lowering.
    pub fn is_prelude_document(&self) -> bool {
        matches!(self, Type::Document)
    }

    /// T43: the HTML-Element type. Maps to `buff_scrape::Element`
    /// at codegen time. Constructed as the return value of
    /// `Document.select(css)` / `Element.select(css)`; supports 5
    /// instance methods (`text`, `attr`, `html`, `inner_html`,
    /// `select`).
    pub fn element() -> Self {
        Type::Element
    }

    /// T43: Returns `true` if this type is the prelude `Element`
    /// runtime value (single selected HTML element). Used to
    /// dispatch instance method calls (`el.text()`, `el.attr(...)`,
    /// ...) to the `buff_scrape::Element` lowering.
    pub fn is_prelude_element(&self) -> bool {
        matches!(self, Type::Element)
    }

    /// T43: the HTTP-crawler type. Maps to `buff_scrape::Crawler`
    /// at codegen time. Constructed via `Crawler.new(seed_url)`;
    /// supports 4 instance methods (`seed`, `fetch`, `crawl`,
    /// `robots_allows`).
    pub fn crawler() -> Self {
        Type::Crawler
    }

    /// T43: Returns `true` if this type is the prelude `Crawler`
    /// runtime value. Used to dispatch instance method calls
    /// (`crawler.fetch(...)`, `crawler.crawl(...)`, ...,
    /// `crawler.robots_allows(...)`) to the `buff_scrape::Crawler`
    /// lowering.
    pub fn is_prelude_crawler(&self) -> bool {
        matches!(self, Type::Crawler)
    }

    /// T50: the XML document type. Maps to `buff_xml::XmlDocument` at
    /// codegen time. Constructed via `Xml.from_str(xml)`; supports
    /// the instance methods `.root()`, `.find(xpath)`, `.to_string()`.
    /// Pure-Rust, CPU-only.
    pub fn xml() -> Self {
        Type::Xml
    }

    /// T50: Returns `true` if this type is the prelude `Xml` runtime
    /// value. Used to dispatch instance method calls
    /// (`doc.root()`, `doc.find(xpath)`, `doc.to_string()`) to the
    /// `buff_xml::XmlDocument` lowering.
    pub fn is_prelude_xml(&self) -> bool {
        matches!(self, Type::Xml)
    }

    /// T50: the XML element type. Maps to `buff_xml::XmlElement` at
    /// codegen time. Returned by `XmlDocument.root()` /
    /// `XmlDocument.find(xpath)`; supports the instance methods
    /// `.name()`, `.attr(name)`, `.text()`, `.children()`.
    pub fn xml_element() -> Self {
        Type::XmlElement
    }

    /// T50: Returns `true` if this type is the prelude `XmlElement`
    /// runtime value. Used to dispatch instance method calls
    /// (`el.name()`, `el.attr(name)`, `el.text()`, `el.children()`)
    /// to the `buff_xml::XmlElement` lowering.
    pub fn is_prelude_xml_element(&self) -> bool {
        matches!(self, Type::XmlElement)
    }

    /// T51: Returns `true` if this type is the prelude `MsgPack` namespace.
    /// Namespace-only (no runtime value — like Log / Toml / Base64 / Hex).
    pub fn is_prelude_msgpack(&self) -> bool {
        matches!(self, Type::MsgPack)
    }

    /// T52: the `Protobuf` Protocol-Buffers format namespace type. Maps
    /// to `buff_protobuf` at codegen time. Namespace-only — never
    /// instantiated as a runtime value; only its associated functions
    /// are callable (`Protobuf.serialize` / `Protobuf.deserialize` /
    /// `Protobuf.roundtrip`). Mirrors `Type::MsgPack` (T51).
    pub fn protobuf() -> Self {
        Type::Protobuf
    }

    /// T52: Returns `true` if this type is the prelude `Protobuf`
    /// namespace. Namespace-only (no runtime value — like MsgPack /
    /// Log / Toml / Base64 / Hex).
    pub fn is_prelude_protobuf(&self) -> bool {
        matches!(self, Type::Protobuf)
    }

    /// T52: the protobuf-encoded message type. Maps to
    /// `buff_protobuf::Message` at codegen time. Constructed via
    /// `Message.new(value)` / `Message.from_bytes(bytes)` /
    /// `Message.decode(bytes)`; supports the instance methods
    /// `message.byte_size()`, `message.type_url()`,
    /// `message.payload()`, `message.encode()`.
    pub fn message() -> Self {
        Type::Message
    }

    /// T52: Returns `true` if this type is the prelude `Message`
    /// runtime value (a protobuf-encoded message). Used to dispatch
    /// instance method calls (`msg.byte_size()`, `msg.type_url()`,
    /// `msg.payload()`, `msg.encode()`) to the `buff_protobuf::Message`
    /// lowering.
    pub fn is_prelude_message(&self) -> bool {
        matches!(self, Type::Message)
    }

    /// T45: the 2D geospatial point type. Maps to `buff_geo::Point` at
    /// codegen time. Constructed via `Point.new(x, y)`; supports the
    /// instance methods `point.x()`, `point.y()`,
    /// `point.distance_to(other)`.
    pub fn point() -> Self {
        Type::Point
    }

    /// T45: Returns `true` if this type is the prelude `Point` runtime
    /// value. Used to dispatch instance method calls (`p.x()`, `p.y()`,
    /// `p.distance_to(q)`) to the `buff_geo::Point` lowering.
    pub fn is_prelude_point(&self) -> bool {
        matches!(self, Type::Point)
    }

    /// T45: the geospatial LineString type. Maps to
    /// `buff_geo::LineString` at codegen time. Constructed via
    /// `LineString.new(points)` / `LineString.from_coords(flat)`;
    /// supports the instance method `ls.length()`.
    pub fn line_string() -> Self {
        Type::LineString
    }

    /// T45: Returns `true` if this type is the prelude `LineString`
    /// runtime value. Used to dispatch instance method calls
    /// (`ls.length()`) to the `buff_geo::LineString` lowering.
    pub fn is_prelude_line_string(&self) -> bool {
        matches!(self, Type::LineString)
    }

    /// T45: the geospatial Polygon type. Maps to `buff_geo::Polygon`
    /// at codegen time. Constructed via `Polygon.new(ring)` /
    /// `Polygon.from_coords(flat)`; supports the instance methods
    /// `poly.area()`, `poly.contains(point)`, `poly.intersects(other)`.
    pub fn polygon() -> Self {
        Type::Polygon
    }

    /// T45: Returns `true` if this type is the prelude `Polygon`
    /// runtime value. Used to dispatch instance method calls
    /// (`poly.area()`, `poly.contains(p)`, `poly.intersects(o)`) to
    /// the `buff_geo::Polygon` lowering.
    pub fn is_prelude_polygon(&self) -> bool {
        matches!(self, Type::Polygon)
    }

    /// T46: the `Text` NLP namespace type. Maps to `buff_nlp::Text` at
    /// codegen time. Namespace-only — never instantiated as a runtime
    /// value; only its associated functions are callable
    /// (`Text.detect_language` / `Text.stem` / `Text.tokenize` /
    /// `Text.sentences`). Mirrors `Type::MsgPack` (T51).
    pub fn text() -> Self {
        Type::Text
    }

    /// T46: Returns `true` if this type is the prelude `Text` NLP
    /// namespace. Namespace-only (no runtime value — like MsgPack).
    pub fn is_prelude_text(&self) -> bool {
        matches!(self, Type::Text)
    }

    /// T46: the detected-natural-language runtime-value type. Maps to
    /// `buff_nlp::Language` at codegen time. Constructed ONLY via
    /// `Text.detect_language(input)`; carries the instance methods
    /// `.code()` / `.name()`.
    pub fn language() -> Self {
        Type::Language
    }

    /// T46: Returns `true` if this type is the prelude `Language`
    /// runtime value (a detected natural language). Used to dispatch
    /// instance method calls (`lang.code()`, `lang.name()`) to the
    /// `buff_nlp::Language` lowering.
    pub fn is_prelude_language(&self) -> bool {
        matches!(self, Type::Language)
    }

    /// T46: the Snowball stemming algorithm selector enum. Maps to
    /// `buff_nlp::StemAlgorithm` at codegen time. Opaque enum — only
    /// passed as an arg to `Text.stem(word, algorithm)`; NO instance
    /// methods exposed.
    pub fn stem_algorithm() -> Self {
        Type::StemAlgorithm
    }

    /// T46: Returns `true` if this type is the prelude `StemAlgorithm`
    /// opaque enum. Used to dispatch `Text.stem(word, algorithm)`
    /// codegen (the algorithm arg needs translation from Buff
    /// enum-variant literal syntax to `buff_nlp::StemAlgorithm::*`).
    pub fn is_prelude_stem_algorithm(&self) -> bool {
        matches!(self, Type::StemAlgorithm)
    }

    /// Returns `true` if this type **must** run on the CPU (never GPU).
    ///
    /// [`Type::Decimal`] is the canonical case: 128-bit fixed-point decimals
    /// have no WGSL representation, so any expression involving a Decimal is
    /// forced onto the CPU/Rayon path. This is the complement of
    /// [`is_gpu_eligible`](Self::is_gpu_eligible) for the Decimal case, but
    /// also flags `Double` (no f64 in WGSL) and non-32-bit widths.
    pub fn must_run_on_cpu(&self) -> bool {
        !self.is_gpu_eligible()
    }
}

impl IntWidth {
    /// Returns the bit-width of this integer width as a `u8`.
    pub fn bits(&self) -> u8 {
        match self {
            IntWidth::W8 => 8,
            IntWidth::W16 => 16,
            IntWidth::W32 => 32,
            IntWidth::W64 => 64,
            IntWidth::W128 => 128,
        }
    }
}

impl FloatWidth {
    /// Returns the bit-width of this float width as a `u8`.
    pub fn bits(&self) -> u8 {
        match self {
            FloatWidth::W16 => 16,
            FloatWidth::W32 => 32,
            FloatWidth::W64 => 64,
        }
    }
}

impl fmt::Display for Type {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Type::Int { width } => write!(f, "Int<{}>", width.bits()),
            Type::Bits { width } => write!(f, "Bits<{}>", width.bits()),
            Type::Float { width } => write!(f, "Float<{}>", width.bits()),
            Type::Double => f.write_str("Double"),
            Type::Bool => f.write_str("Bool"),
            Type::String => f.write_str("String"),
            Type::Char => f.write_str("Char"),
            Type::Decimal => f.write_str("Decimal"),
            Type::Unknown => f.write_str("Unknown"),
            Type::Void => f.write_str("Void"),
            Type::Vector(elem) => write!(f, "Vector<{elem}>"),
            Type::Matrix(elem) => write!(f, "Matrix<{elem}>"),
            Type::Option(inner) => write!(f, "Option<{inner}>"),
            Type::Map(key, value) => write!(f, "Map<{key}, {value}>"),
            Type::Result(ok, err) => write!(f, "Result<{ok}, {err}>"),
            // T76: union `A | B | C`.
            Type::Union(members) => {
                for (i, m) in members.iter().enumerate() {
                    if i > 0 {
                        f.write_str(" | ")?;
                    }
                    write!(f, "{m}")?;
                }
                Ok(())
            }
            // T103: tuple `(T, U, ...)`. Renders with leading/trailing parens
            // and comma-separated members, mirroring the source form.
            Type::Tuple(members) => {
                f.write_str("(")?;
                for (i, m) in members.iter().enumerate() {
                    if i > 0 {
                        f.write_str(", ")?;
                    }
                    write!(f, "{m}")?;
                }
                f.write_str(")")
            }
            // T124b: prelude datetime family. These are opaque value types
            // whose canonical Rust representation lives in the codegen crate
            // (chrono / std::time). The Display form mirrors the Buff
            // surface name so diagnostics read naturally.
            Type::DateTime => f.write_str("DateTime"),
            Type::Date => f.write_str("Date"),
            Type::Time => f.write_str("Time"),
            Type::Duration => f.write_str("Duration"),
            Type::Instant => f.write_str("Instant"),
            // T124d: prelude compiled-regex type. Opaque value type whose
            // canonical Rust representation lives in the codegen crate
            // (`regex::Regex`). The Display form mirrors the Buff surface
            // name so diagnostics read naturally.
            Type::Regex => f.write_str("Regex"),
            // T124h: prelude parsed-URL type. Opaque value type whose
            // canonical Rust representation lives in the codegen crate
            // (`url::Url`). The Display form mirrors the Buff surface
            // name so diagnostics read naturally.
            Type::Url => f.write_str("URL"),
            // T124j: prelude filesystem-path type. Opaque value type
            // whose canonical Rust representation lives in the codegen
            // crate (`std::path::PathBuf`). The Display form mirrors
            // the Buff surface name so diagnostics read naturally.
            Type::Path => f.write_str("Path"),
            // T124l: prelude spawned-process type. Opaque value type
            // whose canonical Rust representation lives in the codegen
            // crate (`Option<std::process::Child>` - the Option
            // wrapper lets spawn be panic-free). The Display form
            // mirrors the Buff surface name so diagnostics read
            // naturally.
            Type::Process => f.write_str("Process"),
            // T124m: prelude TCP-connection type. Opaque value type
            // whose canonical Rust representation lives in the
            // codegen crate (`Option<tokio::net::TcpStream>` - the
            // Option wrapper lets connect be panic-free). The
            // Display form mirrors the Buff surface name.
            Type::Connection => f.write_str("Connection"),
            // T124m: prelude UDP-socket type. Opaque value type
            // whose canonical Rust representation lives in the
            // codegen crate (`Option<tokio::net::UdpSocket>` -
            // the Option wrapper lets bind be panic-free). The
            // Display form mirrors the Buff surface name.
            Type::Socket => f.write_str("Socket"),
            // T124m: prelude WebSocket-connection type. Opaque
            // value type whose canonical Rust representation lives
            // in the codegen crate
            // (`Option<tokio_tungstenite::WebSocketStream<...>>` -
            // the Option wrapper lets connect be panic-free). The
            // Display form mirrors the Buff surface name.
            Type::WsConnection => f.write_str("WsConnection"),
            // T2: channel sender / receiver. Opaque runtime-value
            // types mapped to `buff_lang_runtime::Sender<T>` /
            // `buff_lang_runtime::Receiver<T>`. Display mirrors the
            // Buff surface name.
            Type::Sender => f.write_str("Sender"),
            Type::Receiver => f.write_str("Receiver"),
            // T9: image. Opaque runtime-value type mapped to
            // `buff_image::Image`. Display mirrors the Buff surface
            // name (`Image`).
            Type::Image => f.write_str("Image"),
            // T37: fake-data generator. Opaque runtime-value type
            // mapped to `buff_fake::Faker`. Display mirrors the Buff
            // surface name (`Faker`).
            Type::Faker => f.write_str("Faker"),
            // T31: cache. Opaque runtime-value type mapped to
            // `buff_cache::Cache`. Display mirrors the Buff surface
            // name (`Cache`).
            Type::Cache => f.write_str("Cache"),
            Type::I18n => f.write_str("I18n"),
            Type::DataFrame => f.write_str("DataFrame"),
            Type::Audio => f.write_str("AudioBuffer"),
            // T12: prelude ECS types. Opaque value types whose
            // canonical Rust representations live in the `buff-ecs`
            // crate (`buff_ecs::World` / `buff_ecs::Entity`). The
            // Display form mirrors the Buff surface name so
            // diagnostics read naturally.
            Type::World => f.write_str("World"),
            Type::Entity => f.write_str("Entity"),
            Type::Template => f.write_str("Template"),
            // T33: prelude HTTP client type. Opaque value type mapped
            // to `buff_http_client::HttpClient`. Display mirrors the
            // Buff surface name.
            Type::HttpClient => f.write_str("HttpClient"),
            // T29: prelude validator type. Opaque value type mapped
            // to `buff_validate::Validator`. Display mirrors the
            // Buff surface name.
            Type::Validator => f.write_str("Validator"),
            // T42: prelude email type. Opaque value type mapped to
            // `buff_email::Email`. Display mirrors the Buff surface
            // name.
            Type::Email => f.write_str("Email"),
            // T42: prelude SMTP client type. Opaque value type mapped
            // to `buff_email::SmtpClient`. Display mirrors the Buff
            // surface name.
            Type::SmtpClient => f.write_str("SmtpClient"),
            // T43: prelude scrape types. Opaque value types mapped to
            // `buff_scrape::{Document, Element, Crawler}`. Display
            // mirrors the Buff surface names.
            Type::Document => f.write_str("Document"),
            Type::Element => f.write_str("Element"),
            Type::Crawler => f.write_str("Crawler"),
            // T51: prelude MsgPack namespace. Namespace-only (no runtime
            // value — like Log / Toml / Base64 / Hex / Yaml / Csv).
            // Display mirrors the Buff surface name.
            Type::MsgPack => f.write_str("MsgPack"),
            // T50: prelude Xml type. Opaque runtime-value type mapped
            // to `buff_xml::XmlDocument`. Display mirrors the Buff
            // surface name.
            Type::Xml => f.write_str("Xml"),
            // T50: prelude XmlElement type. Opaque runtime-value type
            // mapped to `buff_xml::XmlElement`. Display mirrors the
            // Buff surface name.
            Type::XmlElement => f.write_str("XmlElement"),
            // T45: prelude geo types. Opaque value types mapped to
            // `buff_geo::{Point, LineString, Polygon}`. Display mirrors
            // the Buff surface name.
            Type::Point => f.write_str("Point"),
            Type::LineString => f.write_str("LineString"),
            Type::Polygon => f.write_str("Polygon"),
            // T46: prelude NLP types. `Text` is namespace-only (mirrors
            // MsgPack); `Language` is a runtime value (mirrors Point);
            // `StemAlgorithm` is an opaque enum (only passed as arg).
            // Display mirrors the Buff surface name in all three cases.
            Type::Text => f.write_str("Text"),
            Type::Language => f.write_str("Language"),
            Type::StemAlgorithm => f.write_str("StemAlgorithm"),
            // T52: prelude Protobuf namespace + Message instance type.
            // `Protobuf` is namespace-only (mirrors MsgPack); `Message`
            // is a runtime value (mirrors Image / Xml). Display mirrors
            // the Buff surface name in both cases.
            Type::Protobuf => f.write_str("Protobuf"),
            Type::Message => f.write_str("Message"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn type_display_variants() {
        assert_eq!(Type::int_default().to_string(), "Int<64>");
        assert_eq!(Type::byte().to_string(), "Bits<8>");
        assert_eq!(Type::float_default().to_string(), "Float<32>");
        assert_eq!(Type::double().to_string(), "Double");
        assert_eq!(Type::bool().to_string(), "Bool");
        assert_eq!(Type::string().to_string(), "String");
        assert_eq!(Type::char().to_string(), "Char");
        assert_eq!(Type::Decimal.to_string(), "Decimal");
        assert_eq!(Type::Unknown.to_string(), "Unknown");
        assert_eq!(Type::Void.to_string(), "Void");
    }

    #[test]
    fn numeric_classification() {
        assert!(Type::int_default().is_numeric());
        assert!(Type::byte().is_numeric());
        assert!(Type::float_default().is_numeric());
        assert!(Type::double().is_numeric());
        assert!(Type::Decimal.is_numeric());
        assert!(!Type::bool().is_numeric());
        assert!(!Type::string().is_numeric());

        assert!(Type::float_default().is_float_like());
        assert!(Type::double().is_float_like());
        assert!(!Type::int_default().is_float_like());

        assert!(Type::int_default().is_integer_like());
        assert!(Type::byte().is_integer_like());
        assert!(!Type::float_default().is_integer_like());
    }

    // T20: GPU/CPU dispatch type-metadata predicates.
    #[test]
    fn gpu_cpu_dispatch_metadata() {
        // WGSL-native 32-bit scalars are GPU-eligible.
        assert!(Type::float_default().is_gpu_eligible()); // Float<32>
        assert!(Type::Bool.is_gpu_eligible());
        assert!(Type::Int {
            width: IntWidth::W32
        }
        .is_gpu_eligible());
        assert!(Type::Bits {
            width: IntWidth::W32
        }
        .is_gpu_eligible());

        // Decimal is NEVER GPU-eligible — it must run on CPU (Rayon).
        assert!(!Type::Decimal.is_gpu_eligible());
        assert!(Type::Decimal.must_run_on_cpu());

        // Double (f64) and wide integers are also CPU-only (no WGSL scalar).
        assert!(!Type::Double.is_gpu_eligible());
        assert!(Type::Double.must_run_on_cpu());
        assert!(!Type::int_default().is_gpu_eligible()); // Int<64>
        assert!(!Type::byte().is_gpu_eligible()); // Bits<8>

        // Predicate complementarity for Decimal.
        assert_ne!(
            Type::Decimal.is_gpu_eligible(),
            Type::Decimal.must_run_on_cpu()
        );
    }
}

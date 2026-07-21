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

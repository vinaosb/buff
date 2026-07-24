//! T105b: PreludeInstanceFn enum + impl + instance_fn_lookup helper.
//!
//! MECHANICAL EXTRACTION from prelude_types.rs (T105b God Class split).
//! No logic changes — moved verbatim.

use crate::prelude_types::instance_fn_return_type;
use crate::ty::Type;

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
    /// `df.to_table_string() -> String`. Zero args. Returns the
    /// fixed-width pretty-printed table (header row + separator +
    /// data rows). Wraps `buff_dataframe::DataFrame::to_table_string
    /// (recv)` (infallible — no `unwrap_or_default()` needed). Used
    /// by `print(df)` and any time the user wants a readable snapshot
    /// of the frame's current contents.
    ToTableString,
    // ---- Signal instance methods (T11) --------------------------------
    // Nine instance methods on Signal values. Dispatched on
    // (Type::Void, variant) — Signal models as Void for MVP (the
    // coordinated Type::Signal variant is a follow-up outside T11
    // shared zone). CPU-only per Metis G7 (NO GPU, NO real-time
    // streaming).
    Fft,
    Ifft,
    Lowpass,
    Highpass,
    Bandpass,
    ApplyWindow,
    Spectrogram,
    Magnitude,
    Phase,
    // ---- Image instance methods (T9) ----------------------------------
    // Eleven instance methods on Image values. Dispatched on
    // (Type::Image, variant) pairs. CPU-only per Metis G7 (NO GPU
    // dispatch — defer to v1.18+). Each variant lowers to the matching
    // `buff_image::Image` method in codegen; the `format` /
    // `get_pixel` accessors return Type::Unknown at this layer (no
    // Buff-surface PixelFormat / Color type variant yet — codegen
    // emits the call and Rust's type inference handles the rest).
    /// `img.width() -> Int`. Zero args. Wraps `recv.width() as i64`
    /// (the `as i64` lifts the underlying `u32` to Buff's Int width).
    Width,
    /// `img.height() -> Int`. Zero args. Wraps `recv.height() as i64`.
    Height,
    /// `img.format() -> PixelFormat`. Zero args. Returns the pixel
    /// format enum (`Rgb` / `Rgba`) — modeled as Type::Unknown at this
    /// layer because Buff has no surface PixelFormat type variant
    /// (codegen emits `recv.format()` and Rust's type inference
    /// derives `buff_image::PixelFormat`).
    PixelFormat,
    /// `img.get_pixel(x, y) -> Color`. Two args (Int x, Int y). Bounds-
    /// checked; the codegen lowers to `recv.get_pixel(x as u32, y as
    /// u32).unwrap_or_default()` (Color impls Default as black —
    /// panic-free per Buff's "no panicking generated code" rule).
    GetPixel,
    /// `img.set_pixel(x, y, color) -> Void`. Three args (Int x, Int y,
    /// Color). Bounds-checked in-place mutation; the codegen lowers to
    /// `recv.set_pixel(x as u32, y as u32, color).unwrap_or_default()`
    /// (panic-free via `()` Default).
    SetPixel,
    /// `img.save(path) -> Void`. One arg (String / Path). Writes to
    /// disk; the format is inferred from the file extension. The
    /// codegen lowers to `recv.save(path).unwrap_or_default()` (panic-
    /// free). Shared `Save` variant — dispatched on (Image, Save) /
    /// (Audio, Save) pairs (mirrors `Send` shared between Connection
    /// / WsConnection).
    Save,
    /// `img.grayscale() -> Image`. Zero args. Consumes self, returns a
    /// new grayscale Image (Rec. 601 luma coefficients). Infallible —
    /// the codegen lowers to `recv.grayscale()` directly.
    Grayscale,
    /// `img.invert() -> Void`. Zero args. In-place channel inversion
    /// (subtracts each channel from 255). Infallible.
    Invert,
    /// `img.resize(w, h) -> Image`. Two args (Int width, Int height).
    /// Lanczos3 resize. The codegen lowers to `recv.resize(w as u32,
    /// h as u32).unwrap_or_default()` (Image impls Default as a 1x1
    /// transparent pixel — panic-free on zero dims / overflow).
    Resize,
    /// `img.crop(x, y, w, h) -> Image`. Four args. Bounds-checked
    /// subimage extraction. The codegen lowers to `recv.crop(x as
    /// u32, y as u32, w as u32, h as u32).unwrap_or_default()`.
    Crop,
    /// `img.blur(sigma) -> Image`. One arg (Float sigma). Gaussian
    /// blur. Infallible — the codegen lowers to `recv.blur(sigma as
    /// f32)` directly (sigma=0 is a no-op clone).
    Blur,
    // ---- Faker instance methods (T37) ----------------------------------
    // Eight instance methods on Faker values. Dispatched on
    // (Type::Faker, variant) pairs. Each variant lowers to the matching
    // `buff_fake::Faker` method in codegen. Pure-Rust, no native deps.
    /// `faker.name() -> String`. Zero args. Random full name in the
    /// configured locale. Wraps `recv.name()` (infallible).
    Name,
    /// `faker.email() -> String`. Zero args. Random email address.
    /// Wraps `recv.email()` (infallible).
    Email,
    /// `faker.address() -> String`. Zero args. Random street address.
    /// Wraps `recv.address()` (infallible).
    Address,
    /// `faker.phone() -> String`. Zero args. Random phone number.
    /// Wraps `recv.phone()` (infallible).
    Phone,
    /// `faker.uuid() -> String`. Zero args. Random UUID v4 string.
    /// Wraps `recv.uuid()` (infallible).
    Uuid,
    /// `faker.lorem(words) -> String`. One arg (Int word_count).
    /// Lorem ipsum text with the given number of words. Wraps
    /// `recv.lorem(words as usize)` (infallible).
    Lorem,
    /// `faker.int(min, max) -> Int`. Two args (Int min, Int max).
    /// Random integer in [min, max] (inclusive). Wraps
    /// `recv.int(min, max)` (infallible).
    FakerInt,
    /// `faker.datetime(start, end) -> String`. Two args (String start,
    /// String end). Random datetime in RFC 3339 range. Wraps
    /// `recv.datetime(&start, &end).unwrap_or_default()` (panic-free).
    FakerDatetime,
    // ---- Email instance methods (T42) ----------------------------------
    // Three builder methods on Email values. Each consumes `self` and
    // returns a new Email (Buff "no visible references" stance —
    // mirrors the Validator with_* builder pattern). Dispatched on
    // (Type::Email, variant) pairs. The codegen lowers to the matching
    // `buff_email::Email::{body, html, attach}` methods.
    /// `email.body(text) -> Email`. One arg (String plain-text body).
    /// Sets / overwrites the plain-text body. Builder pattern.
    Body,
    /// `email.html(template, context_json) -> Email`. Two args (String
    /// handlebars template, String JSON context). Renders the template
    /// via `handlebars::Handlebars::render` and stores the result as
    /// the HTML body.
    Html,
    /// `email.attach(path) -> Email`. One arg (String path). Queues a
    /// file attachment (opened + encoded at send time).
    Attach,
    // ---- AudioBuffer instance methods (T10) ---------------------------
    // Ten instance methods on AudioBuffer values. Dispatched on
    // (Type::Audio, variant) pairs. CPU-only per Metis G7 (NO GPU
    // dispatch, NO real-time playback). The `summarize` accessor
    // returns Type::Unknown at this layer (no Buff-surface
    // AudioSummary type variant — codegen emits the call and Rust's
    // type inference derives `buff_audio::AudioSummary`).
    /// `buf.samples() -> Vector<Float>`. Zero args. Returns the
    /// interleaved sample slice as an owned Vec<f32>. Wraps
    /// `recv.samples().to_vec()`.
    Samples,
    /// `buf.sample_rate() -> Int`. Zero args. Wraps `recv.sample_rate
    /// () as i64`.
    SampleRate,
    /// `buf.channels() -> Int`. Zero args. Wraps `recv.channels() as
    /// i64`.
    Channels,
    /// `buf.frames() -> Int`. Zero args. Wraps `recv.frames() as i64`.
    Frames,
    /// `buf.duration_secs() -> Float`. Zero args. Wraps
    /// `recv.duration_secs() as f64`.
    DurationSecs,
    /// `buf.amplify(factor) -> Void`. One arg (Float). In-place scale.
    /// Infallible.
    Amplify,
    /// `buf.normalize(target) -> Void`. One arg (Float). In-place
    /// peak-normalize. Infallible (zero-sample buffer is a no-op).
    Normalize,
    /// `buf.mix(other) -> Void`. One arg (AudioBuffer). Sample-wise
    /// add. The codegen lowers to `recv.mix(&other).unwrap_or_default
    /// ()` (panic-free on rate/channel mismatch — the underlying
    /// AudioBuffer::mix returns Result; the .unwrap_or_default()
    /// collapses to a no-op).
    Mix,
    /// `buf.slice(start_sec, end_sec) -> AudioBuffer`. Two args
    /// (Float, Float). Returns a new AudioBuffer for the time window.
    /// The codegen lowers to `recv.slice(start_sec, end_sec)
    /// .unwrap_or_default()` (AudioBuffer impls Default as an empty
    /// buffer — panic-free on invalid endpoints).
    Slice,
    /// `buf.summarize() -> AudioSummary`. Zero args. Returns a
    /// statistics snapshot (peak / RMS / frames / duration). Modeled
    /// as Type::Unknown at this layer because Buff has no surface
    /// AudioSummary type variant.
    Summarize,
    /// T20: `s.get() -> T` — read the current value of a reactive
    /// Signal/Computed AND register the calling observer (Effect /
    /// Computed) as a subscriber. Zero args.
    Get,
    /// T20: `s.set(value) -> Void` — write a new value to a reactive
    /// Signal and notify subscribers. One arg (T).
    Set,
    /// T20: `s.update(fn) -> Void` — read-modify-write on a reactive
    /// Signal. One arg (`Fn(&mut T) -> Void`).
    Update,
    /// T20: `c.invalidate() -> Void` — manually clear a Computed's
    /// cache. Zero args.
    Invalidate,
    // ---- Web instance methods (T17) -----------------------------------
    // Eight instance methods on Web values. Dispatched on
    // (Type::Web, variant) pairs — BUT Type::Web is a forward-
    // declaration sibling task (out of T17 shared zone, mirrors the
    // T8/T11/T12-Tensor precedent). The `instance_fn_return_type`
    // arms for these variants are therefore DEFERRED to the
    // coordinated sibling task that adds Type::Web to ty.rs. The
    // variants themselves + their `name()` spellings are shipped
    // here so the parser + the PreludeInstanceFn ALL set see them
    // (lets `buff check` validate `web.<method>(...)` syntax today).
    //
    // Each variant is Web-only (no shared variants with prior prelude
    // instance fns) because the receiver semantics differ — `web.get
    // (path, handler)` takes a String + a closure, NOT a Map key like
    // `env.get(name)`. Buff §6 reserved-keyword constraint: `use`
    // is reserved so the middleware-registration method is `middleware`
    // (not `use`); `listen` is unreserved and matches the T17 QA
    // scenario spec verbatim ("s.listen(port: 8080)").
    /// `web.get(path, handler) -> Web`. Two args (String path, Handler
    /// closure). Registers a GET route. The codegen lowering will
    /// emit `recv.get(path, std::sync::Arc::new(handler))` once
    /// Type::Web lands (forward-declaration contract).
    RouteGet,
    /// `web.post(path, handler) -> Web`. POST route. Same shape as
    /// RouteGet.
    RoutePost,
    /// `web.put(path, handler) -> Web`. PUT route.
    RoutePut,
    /// `web.delete(path, handler) -> Web`. DELETE route.
    RouteDelete,
    /// `web.patch(path, handler) -> Web`. PATCH route.
    RoutePatch,
    /// `web.middleware(mw) -> Web`. One arg (MiddlewareFn closure).
    /// Pushes the middleware onto the dispatch chain.
    Middleware,
    /// `web.listen(port: N) -> Void`. One named arg (Int port). Binds
    /// `0.0.0.0:{port}` and serves forever (synchronous — Buff has
    /// no `await` keyword per AGENTS.md §6; the codegen-lowered
    /// Rust wraps axum::serve in `tokio::runtime::Runtime::new()?.
    /// block_on(...)` per FFI guide Example 3). The QA scenario in
    /// the T17 spec writes this verb as `s.listen(port: 8080)`.
    Listen,
    /// `web.run() -> Void`. Zero args. Serves on the bind addr set
    /// by `Web.bind(addr)` (defaults to `0.0.0.0:8080`).
    Run,
    // ---- Validator instance methods (T29) --------------------------------
    /// `validator.with_email(field) -> Validator`. One arg (String
    /// field name). Builder that consumes self and returns a new
    /// Validator with the email rule added (Buff "no visible
    /// references" stance — mirrors the axum Router::route pattern).
    /// Wraps `recv.with_email(field)`.
    WithEmail,
    /// `validator.with_url(field) -> Validator`. One arg. Builder.
    /// Wraps `recv.with_url(field)`.
    WithUrl,
    /// `validator.with_length(field, min, max) -> Validator`. Three
    /// args. Builder. Wraps `recv.with_length(field, min, max)?`
    /// (the `?` surfaces InvalidRuleConfig at the call site when
    /// `min > max` — fail-fast per the T29 panic-free contract).
    WithLength,
    /// `validator.with_range(field, min, max) -> Validator`. Three
    /// args. Builder. Wraps `recv.with_range(field, min, max)?`.
    WithRange,
    /// `validator.with_regex(field, pattern) -> Validator`. Two
    /// args. Builder. Wraps `recv.with_regex(field, pattern)?`
    /// (the `?` surfaces BadRegex at the call site for malformed
    /// patterns — fail-fast, NOT deferred until validate).
    WithRegex,
    /// `validator.validate(input) -> Result<Void, String>`. One arg
    /// (Map<String, String>). Runs every registered rule against
    /// the input map; aggregates every failure into a single
    /// ValidationErrors. Returns Ok(()) when all rules pass or
    /// Err(stringified aggregate) on any failure. Wraps
    /// `recv.validate(&input).map_err(|e| e.to_string())`.
    Validate,
    /// `validator.to_json_schema() -> String`. Zero args. Serializes
    /// the rule set as a JSON Schema (Draft 2020-12) string. Wraps
    /// `recv.to_json_schema()`.
    ToJsonSchema,
    /// `template.render(context_json) -> String` (T19). One arg
    /// (String — a JSON object). Wraps `recv.render(&ctx)
    /// .unwrap_or_default()`. Added by T31 (this commit) because
    /// T19 added the codegen M::Render arm + Type::Template
    /// reference but missed the PreludeInstanceFn variant — codegen
    /// cannot compile otherwise.
    Render,
    /// `cache.set(key, value, ttl) -> Void` (T31). Three args
    /// (String, String, Duration). Wraps
    /// `recv.set_with_ttl(k, v, ttl)`. Distinct from `Set` (which
    /// takes only 2 args) — Buff's named-args convention lets both
    /// surface as `cache.set(...)` with arity-based dispatch.
    SetTtl,
    /// `cache.delete(key) -> Void` (T31). One arg (String). Wraps
    /// `recv.delete(&k)`.
    Delete,
    /// `cache.contains(key) -> Bool` (T31). One arg (String). Wraps
    /// `recv.contains(&k)`. Expiry-aware: returns `false` for
    /// entries past their TTL deadline.
    Contains,
    /// `cache.clear() -> Void` (T31). Zero args. Wraps
    /// `recv.clear()`. Removes all entries.
    Clear,
    /// `i18n.add_resource(locale, ftl) -> Void` (T44 MVP). Two args
    /// (String locale, String ftl). Wraps
    /// `recv.add_resource(locale, ftl).unwrap_or(())` (panic-free on
    /// Fluent parse error — silently drops the resource; matches
    /// Buff's "no panicking generated code" rule). Full Result<T,E>
    /// surface deferred to a follow-up.
    AddResource,
    /// `i18n.load(locale) -> Void` (T44 MVP). One arg (String). Wraps
    /// `recv.load(locale).unwrap_or(())` (panic-free on
    /// LocaleNotLoaded — no-op).
    Load,
    /// `i18n.translate(key) -> String` (T44 MVP). One arg (String).
    /// Wraps `recv.translate(key)` (current → fallback → key string).
    /// Records a warning on missing keys (surfaced via
    /// `recv.warnings()` in the Rust crate; codegen-wiring deferred).
    Translate,
    // ---- T43: buff-scrape instance methods ------------------------------
    // Document/Element/Crawler methods. The shared `Select` variant
    // (already defined for DataFrame.select) is reused via
    // (Document, Select) / (Element, Select) dispatch — same shared-
    // variant pattern as `Parse` / `New` / `Get`. The shared `Html`
    // variant (already defined for Email.html) is reused via
    // (Document, Html) / (Element, Html) dispatch (semantics: zero-
    // arg HTML serialization accessor on scrape types vs. two-arg
    // template-rendering builder on Email — distinct lowering per
    // receiver type). The 8 new variants below are scrape-only.
    /// `doc.text() / el.text() -> String` (T43). Zero args.
    /// Document variant concatenates ALL text nodes; Element variant
    /// concatenates descendant text nodes of the element.
    Text,
    /// `doc.title() -> String?` (T43). Zero args. Document-only.
    /// Returns `None` when the document has no `<title>` element.
    Title,
    /// `el.attr(name) -> String?` (T43). One arg (String). Element-
    /// only. Returns `None` when the attribute is absent.
    Attr,
    /// `el.children() -> Vector<XmlElement>` (T50). Zero args.
    /// XmlElement-only. Returns the (possibly empty) child element
    /// vector. Mirrors `buff_xml::XmlElement::children` returning
    /// `&[XmlElement]` (codegen lifts to `Vec<XmlElement>` via
    /// `.to_vec()` — Buff surfaces owned values per FFI guide R2).
    Children,
    /// `el.inner_html() -> String` (T43). Zero args. Element-only.
    /// Returns the inner HTML (content WITHOUT opening/closing tag).
    InnerHtml,
    /// `crawler.seed() -> String` (T43). Zero args. Crawler-only.
    /// Round-trip accessor for the seed URL passed to `Crawler.new`.
    Seed,
    /// `crawler.fetch(url) -> Document` (T43). One arg (String URL).
    /// Crawler-only. GET + parse. HTTP non-2xx surfaces as
    /// `ScrapeError::Http`.
    Fetch,
    /// `crawler.crawl(max_pages) -> Vector<String>` (T43). One arg
    /// (Int). Crawler-only. Same-host BFS, robots-aware. Returns the
    /// visited URLs (BFS order). `max_pages <= 0` returns empty Vec
    /// without any fetch.
    Crawl,
    /// `crawler.robots_allows(url) -> Bool` (T43). One arg (String
    /// URL). Crawler-only. Fail-open: returns `true` when robots.txt
    /// is unreachable (per the Robots Exclusion Protocol guidance).
    RobotsAllows,
    // ---- T50: Xml instance methods ----------------------------------------
    /// `doc.root() -> XmlElement` — borrow the root element. Zero args.
    /// XmlDocument-only.
    Root,
    // NOTE: `Find` is shared with `regex.find(text)` above (line ~4698).
    // Dispatch is via `(Type::XmlDocument, Find)` pair in `instance_fn_return_type`
    // + `lower_prelude_type_instance_fn` — same Find variant, different Type first arg.
    // (Earlier T50 draft duplicated the variant; consolidated to avoid E0428.)
    /// `doc.to_string() -> String` — serialize back to XML. Zero args.
    /// XmlDocument-only.
    ToString,
    // ---- T45: buff-geo instance methods --------------------------------
    /// `point.x()` — x coordinate. Zero args. Returns Float. Point-only.
    X,
    /// `point.y()` — y coordinate. Zero args. Returns Float. Point-only.
    Y,
    /// `point.distance_to(other)` — Euclidean distance. One arg (Point).
    /// Returns Float. Point-only.
    DistanceTo,
    /// `line_string.length()` — Euclidean length. Zero args. Returns
    /// Float. LineString-only.
    Length,
    /// `polygon.area()` — unsigned area. Zero args. Returns Float.
    /// Polygon-only.
    Area,
    /// `polygon.intersects(other)` — test intersection. One arg
    /// (Polygon). Returns Bool. Polygon-only. Wraps `catch_unwind`
    /// (BooleanOps) per FFI guide R6.
    Intersects,
    // ---- T46: buff-nlp instance methods --------------------------------
    /// `language.code()` — ISO 639-3 code. Zero args. Returns String.
    /// Language-only. Mirrors `Language::code` in the buff-nlp crate.
    /// `language.name()` reuses the existing shared `Name` variant
    /// (also used by `faker.name()` — dispatched on the receiver type).
    Code,
    // ---- T52: buff-protobuf Message instance methods -------------------
    /// `message.byte_size()` — encoded payload size in bytes. Zero
    /// args. Returns `Int`. Message-only. Wraps
    /// `buff_protobuf::Message::byte_size(recv) as i64` (the
    /// underlying Rust method returns `usize`; the cast lifts to
    /// Buff's `Int<64>`).
    ByteSize,
    /// `message.type_url()` — the canonical type URL identifying the
    /// message schema. Zero args. Returns `String`. Message-only. Always
    /// `"type.googleapis.com/google.protobuf.Struct"` in this MVP
    /// (future `.proto`-codegen tasks may extend the surface with
    /// user-defined message types). Wraps
    /// `buff_protobuf::Message::type_url(recv).to_string()`.
    TypeUrl,
    /// `message.payload()` — decode the payload back into a Value.
    /// Zero args. Returns `Value` (typed `Type::Unknown` at the Buff
    /// layer — there is no surface JsonValue variant; mirrors how
    /// MsgPack.deserialize / Random.choice model dynamic returns).
    /// Message-only. Wraps
    /// `buff_protobuf::Message::payload(recv).unwrap_or_default()`
    /// (Value::Null on decode failure — panic-free via
    /// `.unwrap_or_default()`, NOT bare `.unwrap()`).
    Payload,
    /// `message.encode()` — the encoded protobuf wire-format bytes
    /// (length-delimited `google.protobuf.Struct`). Zero args. Returns
    /// `Vector<Byte>`. Message-only. Wraps
    /// `buff_protobuf::Message::encode(recv).to_vec()` (the underlying
    /// Rust method returns `&[u8]`; the `.to_vec()` lifts to owned
    /// `Vec<u8>` per FFI guide R2 — Buff surfaces owned values).
    /// Distinct from the PreludeAssocFn::Encode variant (which is the
    /// Base64.encode / Hex.encode *associated-function* shape). This
    /// Encode is an *instance method* on a Message value — same name,
    /// different enum (PreludeInstanceFn vs PreludeAssocFn), different
    /// dispatch table.
    Encode,
    // ---- T47: buff-chat instance methods (Bot / ChatMessage / Platform)
    // ----
    // 15 new variants. `Text` is reused (shared with Document / Element
    // / XmlElement — dispatched on (ChatMessage, Text) pair).
    /// `bot.command(name, handler) -> Void` (Bot). Two args
    /// (String name, closure handler `|Message| -> Void`). Wraps
    /// `buff_chat::Bot::command(recv, &name, |msg| <closure body>)
    /// .unwrap_or(())` (panic-free via `.unwrap_or(())` —
    /// registration failure is silently swallowed at the Buff
    /// surface; a future task can surface ChatError if needed).
    /// Bot-only. Both `!ping` (Discord) and `/ping` (Telegram)
    /// trigger the same handler.
    Command,
    /// `bot.on_message(handler) -> Void` (Bot). One arg (closure
    /// handler `|Message| -> Void`). Wraps
    /// `buff_chat::Bot::on_message(recv, |msg| <closure body>)
    /// .unwrap_or(())` (panic-free via `.unwrap_or(())` —
    /// registration failure is silently swallowed). Bot-only.
    /// Mirrors [`Self::Command`] but without the name arg (the
    /// catch-all handler).
    OnMessage,
    /// `bot.start() -> Void` (Bot). Zero args. Wraps
    /// `buff_chat::Bot::start(recv).unwrap_or(())` (panic-free —
    /// blocks on the platform event loop; failure is silently
    /// swallowed). Bot-only. The codegen lowers the void return
    /// rather than surfacing `Result<(), ChatError>` because Buff
    /// has no `?` propagation across FFI boundaries (per the FFI
    /// guide R3, errors are mapped to defaults at the boundary).
    Start,
    /// `bot.stop() -> Void` (Bot). Zero args. Wraps
    /// `buff_chat::Bot::stop(recv).unwrap_or(())` (panic-free).
    /// Bot-only. Cooperative shutdown via AtomicBool flag — does
    /// NOT immediately abort the event loop (in-flight handlers
    /// run to completion).
    Stop,
    /// `bot.dispatch(msg) -> Void` (Bot). One arg (ChatMessage).
    /// Wraps `buff_chat::Bot::dispatch(recv, msg).unwrap_or(())`
    /// (panic-free). Bot-only. Public so tests and programmatic
    /// callers can exercise the handler routing without a live
    /// network connection (the T47 "mock API" acceptance criterion).
    Dispatch,
    /// `bot.is_running() -> Bool` (Bot). Zero args. Wraps
    /// `buff_chat::Bot::is_running(recv)` (infallible — returns
    /// false on poisoned lock, never panics). Bot-only.
    /// Point-in-time snapshot of the running AtomicBool flag.
    IsRunning,
    /// `bot.command_count() -> Int` (Bot). Zero args. Wraps
    /// `buff_chat::Bot::command_count(recv) as i64` (infallible —
    /// returns 0 on poisoned lock). Bot-only. The underlying Rust
    /// method returns `usize`; the cast lifts to Buff's `Int<64>`.
    CommandCount,
    /// `bot.has_message_handler() -> Bool` (Bot). Zero args. Wraps
    /// `buff_chat::Bot::has_message_handler(recv)` (infallible —
    /// returns false on poisoned lock). Bot-only.
    HasMessageHandler,
    /// `msg.channel() -> String` (ChatMessage). Zero args. Wraps
    /// `buff_chat::Message::channel(recv).to_string()` (the
    /// underlying Rust method returns `&str`; the `.to_string()`
    /// lifts to owned String per FFI guide R2 — Buff surfaces
    /// owned values). ChatMessage-only.
    Channel,
    /// `msg.author() -> String` (ChatMessage). Zero args. Wraps
    /// `buff_chat::Message::author(recv).to_string()`. ChatMessage-
    /// only. Returns the display name (Discord) or username sans
    /// `@` (Telegram); empty String when the author is unknown.
    Author,
    /// `bot.platform()` / `msg.platform()` -> Platform (Bot /
    /// ChatMessage). Zero args. Wraps
    /// `buff_chat::Bot::platform(recv)` /
    /// `buff_chat::Message::platform(recv)` (both infallible —
    /// return Copy values directly). Dispatched on the
    /// (Bot, Platform) / (ChatMessage, Platform) pairs.
    Platform,
    /// `msg.is_dm() -> Bool` (ChatMessage). Zero args. Wraps
    /// `buff_chat::Message::is_dm(recv)` (infallible). ChatMessage-
    /// only. Whether the message was sent in a private (direct
    /// message) context.
    IsDm,
    /// `platform.is_discord() -> Bool` (Platform). Zero args.
    /// Wraps `buff_chat::Platform::is_discord(recv)` (infallible —
    /// Copy value). Platform-only.
    IsDiscord,
    /// `platform.is_telegram() -> Bool` (Platform). Zero args.
    /// Wraps `buff_chat::Platform::is_telegram(recv)` (infallible —
    /// Copy value). Platform-only. Mirrors [`Self::IsDiscord`].
    IsTelegram,
    // ---- T48: buff-web3 instance methods ------------------------------
    // Provider (5 accessors), Wallet (SignMessage), Contract (Method),
    // ContractMethod (Arg / Args / Call). All dispatched on
    // (Type, variant) pairs in `instance_fn_return_type`. Address /
    // Send reuse existing shared variants (Faker.address /
    // SmtpClient.send respectively). Wallet.connect reuses Connect
    // (TCP.connect / WebSocket.connect).
    /// `provider.chain_id() -> Int` (Provider). Zero args. EIP-155
    /// chain ID (Mainnet = 1, Sepolia = 11155111). Wraps
    /// `recv.chain_id().unwrap_or_default() as i64` (panic-free —
    /// Web3Error::Rpc collapses to 0). Provider-only.
    ChainId,
    /// `provider.block_number() -> Int` (Provider). Zero args. Latest
    /// sealed block height. Wraps `recv.block_number()
    /// .unwrap_or_default() as i64`. Provider-only.
    BlockNumber,
    /// `provider.get_balance(address) -> Int` (Provider). One arg
    /// (String address — 0x-prefixed 40 hex chars). Returns the low
    /// 128 bits of the U256 wei balance (sufficient for any realistic
    /// balance — 2^128 wei ≈ 6.8 * 10^14 ETH). Wraps
    /// `recv.get_balance(&address).unwrap_or_default() as i64`.
    /// Provider-only.
    GetBalance,
    /// `provider.get_nonce(address) -> Int` (Provider). One arg
    /// (String address). Returns the next transaction index the
    /// network expects from this account. Wraps
    /// `recv.get_nonce(&address).unwrap_or_default() as i64`.
    /// Provider-only.
    GetNonce,
    /// `provider.wait_for_tx(tx_hash) -> String` (Provider). One arg
    /// (String tx_hash — 0x-prefixed 64 hex chars). Returns the
    /// receipt status: `"0x1"` (success), `"0x0"` (reverted),
    /// `"pending"`, or `"not-found"`. Wraps
    /// `recv.wait_for_tx(&hash).unwrap_or_default()`. Provider-only.
    WaitForTx,
    /// `wallet.sign_message(message) -> String` (Wallet). One arg
    /// (String message). EIP-191 personal_sign — returns the 65-byte
    /// signature as `0x`-prefixed hex (includes recovery byte so the
    /// signature can be verified off-chain). Wraps
    /// `recv.sign_message(&msg).unwrap_or_default()`. Wallet-only.
    SignMessage,
    /// `contract.method(name) -> ContractMethod` (Contract). One arg
    /// (String method name). Builds the call — chain `.arg()` /
    /// `.args()` then terminate with `.call()` (read) or `.send()`
    /// (write, requires ConnectedWallet). Returns
    /// `Web3Error::MethodNotFound` if the ABI has no function with
    /// that name (surfaces as Default ContractMethod via the codegen
    /// `.unwrap_or_default()` collapse). Wraps `recv.method(&name)
    /// .unwrap_or_default()`. Contract-only.
    Method,
    /// `m.arg(name, value) -> ContractMethod` (ContractMethod). Two
    /// args (String name — currently IGNORED at the wire layer because
    /// `ethers::abi::Token` doesn't carry names for non-tuple inputs;
    /// future tuple support may consume it; String value — spliced as
    /// `ethers::abi::Token::String((#value).to_string())`). Chainable
    /// — consumes self, returns Self. Mirrors Validator.with_* /
    /// Email.body builder pattern. ContractMethod-only.
    Arg,
    /// `m.args(values) -> ContractMethod` (ContractMethod). One arg
    /// (`Vector<String>` values — each spliced as
    /// `ethers::abi::Token::String`). Chainable — consumes self,
    /// returns Self. Bulk-add variant of [`Self::Arg`]. Wraps
    /// `recv.args((#values).into_iter().map(|v|
    /// ethers::abi::Token::String(v)))`. ContractMethod-only.
    Args,
    /// `m.call() -> String` (ContractMethod). Zero args. Executes
    /// the method as a read-only `eth_call`. Returns the ABI-decoded
    /// return value as debug-formatted text (single-value returns
    /// are the bare value; multi-value returns are
    /// `[Token, Token, ...]`). Wraps `recv.call()
    /// .unwrap_or_default()`. ContractMethod-only.
    Call,
    /// T48: `wallet.connect(provider) -> ConnectedWallet` (Wallet).
    /// One arg (Provider). Infallible (returns ConnectedWallet
    /// directly — no failure mode). Consumes self (move semantics).
    /// Wraps `recv.connect(#provider)`. Mirrors the PreludeAssocFn::
    /// Connect variant NAME but lives in a SEPARATE enum (instance
    /// method vs assoc fn — `wallet.connect(p)` is `recv.method(args)`,
    /// `TCP.connect(h, p)` is `Type.method(args)`). Wallet-only.
    Connect,
    // ---- T49: RsaKeypair instance methods (2) -------------------------
    /// `pair.public_pem() -> String`. Zero args. Returns the
    /// Spki SubjectPublicKeyInfo PEM string (`-----BEGIN PUBLIC
    /// KEY-----`). Wraps `recv.public_pem.clone()` (the underlying
    /// field is `String`; `.clone()` lifts `&String` to owned
    /// `String` per Buff's "hide references" rule). RsaKeypair-only.
    PublicPem,
    /// `pair.private_pem() -> String`. Zero args. Returns the
    /// PKCS#8 PEM string (`-----BEGIN PRIVATE KEY-----`). Wraps
    /// `recv.private_pem.clone()`. RsaKeypair-only.
    PrivatePem,
    // ---- T54: buff-simd instance methods (8) ---------------------------
    /// `simd.add(other)` — lane-wise addition. One arg (Simd).
    /// Returns Simd. Simd-only.
    Add,
    /// `simd.sub(other)` — lane-wise subtraction. One arg (Simd).
    /// Returns Simd. Simd-only.
    Sub,
    /// `simd.mul(other)` — lane-wise multiplication. One arg (Simd).
    /// Returns Simd. Simd-only.
    Mul,
    /// `simd.div(other)` — lane-wise division. One arg (Simd).
    /// Returns Simd. Simd-only.
    Div,
    /// `simd.sum()` — horizontal sum (reduce). Zero args. Returns
    /// Float. Simd-only.
    Sum,
    /// `simd.min()` — horizontal min (smallest lane). Zero args.
    /// Returns Float. Simd-only.
    Min,
    /// `simd.max()` — horizontal max (largest lane). Zero args.
    /// Returns Float. Simd-only.
    Max,
    /// `simd.to_vec()` — extract 4 lanes to `Vector<Float>`. Zero
    /// args. Returns `Vector<Float>`. Simd-only.
    ToVec,
    // ---- T73: String instance methods ------------------------------------
    /// `s.split(sep) -> Vector<String>` — split text by separator.
    /// One arg (String). Wraps `s.split(sep).map(|s|
    /// s.to_string()).collect::<Vec<String>>()`.
    Split,
    /// `s.trim() -> String` — strip leading/trailing whitespace.
    /// Zero args. Wraps `s.trim().to_string()`.
    Trim,
    /// `s.starts_with(prefix) -> Bool` — test prefix. One arg
    /// (String). Wraps `s.starts_with(prefix)`.
    StartsWith,
    /// `s.ends_with(suffix) -> Bool` — test suffix. One arg
    /// (String). Wraps `s.ends_with(suffix)`.
    EndsWith,
    /// `s.to_upper() -> String` — uppercase. Zero args. Wraps
    /// `s.to_uppercase().to_string()`.
    ToUppercase,
    /// `s.to_lower() -> String` — lowercase. Zero args. Wraps
    /// `s.to_lowercase().to_string()`.
    ToLowercase,
    // ---- T80: Http response instance methods --------------------------
    ResponseStatus,
    ResponseBody,
    ResponseJson,
    ResponseHeaders,
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
        PreludeInstanceFn::ToTableString,
        // T11: Signal instance methods (9): fft / ifft / lowpass /
        // highpass / bandpass / apply_window / spectrogram /
        // magnitude / phase.
        PreludeInstanceFn::Fft,
        PreludeInstanceFn::Ifft,
        PreludeInstanceFn::Lowpass,
        PreludeInstanceFn::Highpass,
        PreludeInstanceFn::Bandpass,
        PreludeInstanceFn::ApplyWindow,
        PreludeInstanceFn::Spectrogram,
        PreludeInstanceFn::Magnitude,
        PreludeInstanceFn::Phase,
        // T9: Image instance methods (11 distinct names): width /
        // height / format / get_pixel / set_pixel / save / grayscale
        // / invert / resize / crop / blur. `Save` is shared with
        // AudioBuffer.save (dispatched on receiver type — mirrors
        // `Send` shared between Connection / WsConnection). The other
        // 10 are Image-only.
        PreludeInstanceFn::Width,
        PreludeInstanceFn::Height,
        PreludeInstanceFn::PixelFormat,
        PreludeInstanceFn::GetPixel,
        PreludeInstanceFn::SetPixel,
        PreludeInstanceFn::Save,
        PreludeInstanceFn::Grayscale,
        PreludeInstanceFn::Invert,
        PreludeInstanceFn::Resize,
        PreludeInstanceFn::Crop,
        PreludeInstanceFn::Blur,
        // T37: Faker instance methods (8 distinct names): name / email /
        // address / phone / uuid / lorem / int / datetime. All Faker-only.
        PreludeInstanceFn::Name,
        PreludeInstanceFn::Email,
        PreludeInstanceFn::Address,
        PreludeInstanceFn::Phone,
        PreludeInstanceFn::Uuid,
        PreludeInstanceFn::Lorem,
        PreludeInstanceFn::FakerInt,
        PreludeInstanceFn::FakerDatetime,
        // T10: AudioBuffer instance methods (10 distinct names):
        // samples / sample_rate / channels / frames / duration_secs
        // / amplify / normalize / mix / slice / summarize. `Save` is
        // shared with Image.save. The other 9 are AudioBuffer-only.
        PreludeInstanceFn::Samples,
        PreludeInstanceFn::SampleRate,
        PreludeInstanceFn::Channels,
        PreludeInstanceFn::Frames,
        PreludeInstanceFn::DurationSecs,
        PreludeInstanceFn::Amplify,
        PreludeInstanceFn::Normalize,
        PreludeInstanceFn::Mix,
        PreludeInstanceFn::Slice,
        PreludeInstanceFn::Summarize,
        // T20: Reactive instance methods — Get / Set / Update /
        // Invalidate. Dispatched on (Type::Unknown, Method) pairs so
        // the T26 field-access heuristic does NOT rewrite `s.get()`
        // as `s.get`.
        PreludeInstanceFn::Get,
        PreludeInstanceFn::Set,
        PreludeInstanceFn::Update,
        PreludeInstanceFn::Invalidate,
        // T17: Web instance methods (8 distinct names). All Web-only
        // (no shared variants with prior prelude instance fns). The
        // HTTP-verb-named variants (route_get / route_post / route_put
        // / route_delete / route_patch) are prefixed `route_` to avoid
        // a clash with `Env.get` / `Args.get` (shared `Get` variant).
        // `middleware` / `listen` / `run` are unambiguous verbs.
        PreludeInstanceFn::RouteGet,
        PreludeInstanceFn::RoutePost,
        PreludeInstanceFn::RoutePut,
        PreludeInstanceFn::RouteDelete,
        PreludeInstanceFn::RoutePatch,
        PreludeInstanceFn::Middleware,
        PreludeInstanceFn::Listen,
        PreludeInstanceFn::Run,
        // T29: Validator instance methods (7 distinct names):
        // with_email / with_url / with_length / with_range /
        // with_regex / validate / to_json_schema. All Validator-only.
        // The five builder methods (with_*) consume self and return
        // Self (Buff "no visible references" stance — mirrors the
        // axum Router::route pattern). The two action methods
        // (validate / to_json_schema) borrow self and return
        // Result<Void, String> / String respectively.
        PreludeInstanceFn::WithEmail,
        PreludeInstanceFn::WithUrl,
        PreludeInstanceFn::WithLength,
        PreludeInstanceFn::WithRange,
        PreludeInstanceFn::WithRegex,
        PreludeInstanceFn::Validate,
        PreludeInstanceFn::ToJsonSchema,
        // T42: Email instance methods — 3 new variants. Dispatched on
        // (Type::Email, variant) pairs. Each consumes self + returns
        // a new Email (Buff "no visible references" builder pattern).
        PreludeInstanceFn::Body,
        PreludeInstanceFn::Html,
        PreludeInstanceFn::Attach,
        // T19 (gap-fill by T31): Template.render instance method.
        // The codegen M::Render arm existed; the variant didn't.
        PreludeInstanceFn::Render,
        // T31: Cache instance methods — 4 new variants (SetTtl /
        // Delete / Contains / Clear). Get / Set / Len are SHARED
        // variants (Reactive owns Get/Set; DataFrame owns Len);
        // Cache reuses them via (Type::Cache, Get) / (Type::Cache,
        // Set) / (Type::Cache, Len) dispatch. The four new variants
        // are Cache-only (no other prelude type uses `delete` /
        // `contains` / `clear` as instance methods today; if a
        // future type wants the same verb, dispatch on receiver).
        // SetTtl is the 3-arg `cache.set(key, value, ttl)` overload;
        // Buff's named-args convention disambiguates from Set.
        PreludeInstanceFn::SetTtl,
        PreludeInstanceFn::Delete,
        PreludeInstanceFn::Contains,
        PreludeInstanceFn::Clear,
        // T44: I18n instance methods — 3 MVP variants (AddResource /
        // Load / Translate). Each dispatched on (Type::I18n, variant)
        // pairs. The remaining 7 I18n methods (SetFallback /
        // AvailableLocales / CurrentLocale / FallbackLocale /
        // TranslateWithArgs / HasMessage / Warnings) are available on
        // the `buff_i18n::I18n` Rust type but codegen-wiring is
        // deferred to a follow-up to keep the shared-zone footprint
        // minimal. AddResource / Load / Translate suffice for the T44
        // acceptance-criteria examples (three-locale roundtrip +
        // parameterized translation).
        PreludeInstanceFn::AddResource,
        PreludeInstanceFn::Load,
        PreludeInstanceFn::Translate,
        // T43: buff-scrape instance methods (8 new variants). The
        // shared `Select` (DataFrame-owned) + `Html` (Email-owned)
        // variants cover Document/Element.select + Document/Element
        // .html via (Type, Method) dispatch. The 8 new variants below
        // are scrape-only.
        PreludeInstanceFn::Text,
        PreludeInstanceFn::Title,
        PreludeInstanceFn::Attr,
        PreludeInstanceFn::InnerHtml,
        PreludeInstanceFn::Seed,
        PreludeInstanceFn::Fetch,
        PreludeInstanceFn::Crawl,
        PreludeInstanceFn::RobotsAllows,
        // T50: Xml instance methods — Root / Find / ToString.
        PreludeInstanceFn::Root,
        PreludeInstanceFn::Find,
        PreludeInstanceFn::ToString,
        // T50: XmlElement instance methods. `Name` (shared with Faker /
        // Language), `Text` (shared with Document / Element), `Attr`
        // (shared with Element) are reused via (XmlElement, X)
        // dispatch. `Children` is XmlElement-only (new variant).
        PreludeInstanceFn::Children,
        // T45: buff-geo instance methods (6 new variants). `Contains`
        // is shared with Cache.contains (existing variant — dispatched
        // on (Polygon, Contains) pair). The 6 new variants are geo-only.
        PreludeInstanceFn::X,
        PreludeInstanceFn::Y,
        PreludeInstanceFn::DistanceTo,
        PreludeInstanceFn::Length,
        PreludeInstanceFn::Area,
        PreludeInstanceFn::Intersects,
        // T46: buff-nlp Language instance methods (1 new variant):
        // `Code`. `Name` is shared with Faker.name (existing variant —
        // dispatched on the (Language, Name) pair). `Code` is
        // Language-only (no other prelude type today exposes a `code`
        // instance method; if a future type adds one it can reuse
        // this variant on the (FutureType, Code) pair).
        PreludeInstanceFn::Code,
        // T52: buff-protobuf Message instance methods (4 new variants):
        // byte_size / type_url / payload / encode. All Message-only
        // (no shared variants with prior prelude instance fns). Each
        // dispatched on (Type::Message, variant) pairs.
        PreludeInstanceFn::ByteSize,
        PreludeInstanceFn::TypeUrl,
        PreludeInstanceFn::Payload,
        PreludeInstanceFn::Encode,
        // T47: buff-chat instance methods (15 new variants). `Text` is
        // reused (shared with Document / Element / XmlElement —
        // dispatched on (ChatMessage, Text) pair). The 15 new variants
        // cover Bot (Command / OnMessage / Start / Stop / Dispatch /
        // IsRunning / CommandCount / HasMessageHandler), ChatMessage
        // (Channel / Author / Platform / IsDm), and Platform (IsDiscord
        // / IsTelegram). `Platform` is the accessor variant (returns
        // Type::Platform); `IsDiscord` / `IsTelegram` are predicates.
        // `OnMessage` is a registration variant (mirrors `Command` —
        // takes a single handler closure, no name arg).
        PreludeInstanceFn::Command,
        PreludeInstanceFn::OnMessage,
        PreludeInstanceFn::Start,
        PreludeInstanceFn::Stop,
        PreludeInstanceFn::Dispatch,
        PreludeInstanceFn::IsRunning,
        PreludeInstanceFn::CommandCount,
        PreludeInstanceFn::HasMessageHandler,
        PreludeInstanceFn::Channel,
        PreludeInstanceFn::Author,
        PreludeInstanceFn::Platform,
        PreludeInstanceFn::IsDm,
        PreludeInstanceFn::IsDiscord,
        PreludeInstanceFn::IsTelegram,
        // T48: buff-web3 instance methods (10 new variants). `Address`
        // is shared with Faker.address (existing variant — dispatched
        // on the (Wallet, Address) / (ConnectedWallet, Address) /
        // (Contract, Address) pairs). `Send` is shared with Connection
        // / WsConnection / Sender / SmtpClient (existing variant —
        // dispatched on the (ContractMethod, Send) pair). The 10 new
        // variants are: Provider (ChainId / BlockNumber / GetBalance /
        // GetNonce / WaitForTx), Wallet (SignMessage), Contract
        // (Method), ContractMethod (Arg / Args / Call). All dispatched
        // on the (Type, Method) pair via `instance_fn_return_type`.
        PreludeInstanceFn::ChainId,
        PreludeInstanceFn::BlockNumber,
        PreludeInstanceFn::GetBalance,
        PreludeInstanceFn::GetNonce,
        PreludeInstanceFn::WaitForTx,
        PreludeInstanceFn::SignMessage,
        PreludeInstanceFn::Method,
        PreludeInstanceFn::Arg,
        PreludeInstanceFn::Args,
        PreludeInstanceFn::Call,
        // T48: Wallet.connect — INSTANCE-method Connect (distinct from
        // the PreludeAssocFn::Connect variant — same name, different
        // enum, different dispatch shape: recv.method vs Type.method).
        PreludeInstanceFn::Connect,
        // T49: RsaKeypair instance methods — 2 new variants for the
        // PEM-string accessors. Both zero-arg; both return String.
        // Dispatched on the (Type::RsaKeypair, method) pair. The
        // Buff surface reads `pair.public_pem()` / `pair.private_pem()`
        // — mirrors the underlying `buff_crypto_extras::RsaKeypair`
        // Rust struct field names (the wrapper exposes them as
        // accessor methods, NOT direct field access, so the codegen
        // emits `recv.public_pem().clone()` / `recv.private_pem()
        // .clone()` — the `.clone()` lifts `&String` to owned
        // `String`, Buff hides references from users).
        PreludeInstanceFn::PublicPem,
        PreludeInstanceFn::PrivatePem,
        // T54: buff-simd instance methods (8 new variants). All
        // dispatched on the (Simd, <Variant>) pairs. Simd-only.
        PreludeInstanceFn::Add,
        PreludeInstanceFn::Sub,
        PreludeInstanceFn::Mul,
        PreludeInstanceFn::Div,
        PreludeInstanceFn::Sum,
        PreludeInstanceFn::Min,
        PreludeInstanceFn::Max,
        PreludeInstanceFn::ToVec,
        // T73: String instance methods — split / trim / starts_with /
        // ends_with / to_uppercase / to_lowercase. All dispatched on
        // (Type::String, method) pairs. The existing Replace / Contains
        // / Len variants are also valid on Type::String (added in
        // instance_fn_return_type).
        PreludeInstanceFn::Split,
        PreludeInstanceFn::Trim,
        PreludeInstanceFn::StartsWith,
        PreludeInstanceFn::EndsWith,
        PreludeInstanceFn::ToUppercase,
        PreludeInstanceFn::ToLowercase,
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
            PreludeInstanceFn::ToTableString => "to_table_string",
            PreludeInstanceFn::Join => "join",
            // T9: Image instance method names mirror the
            // `buff_image::Image` method names 1:1 so the codegen can
            // splice `recv.width()` / `recv.grayscale()` etc. without
            // rewriting. `format` is renamed `pixel_format` on the
            // Buff surface to avoid a clash with DateTime.format (the
            // shared `Format` variant is strftime-style returning
            // String; Image's pixel_format returns the PixelFormat
            // enum — distinct semantics, distinct variant).
            PreludeInstanceFn::Width => "width",
            PreludeInstanceFn::Height => "height",
            PreludeInstanceFn::PixelFormat => "pixel_format",
            PreludeInstanceFn::GetPixel => "get_pixel",
            PreludeInstanceFn::SetPixel => "set_pixel",
            PreludeInstanceFn::Save => "save",
            PreludeInstanceFn::Grayscale => "grayscale",
            PreludeInstanceFn::Invert => "invert",
            PreludeInstanceFn::Resize => "resize",
            PreludeInstanceFn::Crop => "crop",
            PreludeInstanceFn::Blur => "blur",
            // T10: AudioBuffer instance method names mirror the
            // `buff_audio::AudioBuffer` method names 1:1 so the
            // codegen can splice `recv.samples()` / `recv.amplify(x)`
            // etc. without rewriting.
            PreludeInstanceFn::Samples => "samples",
            PreludeInstanceFn::SampleRate => "sample_rate",
            PreludeInstanceFn::Channels => "channels",
            PreludeInstanceFn::Frames => "frames",
            PreludeInstanceFn::DurationSecs => "duration_secs",
            PreludeInstanceFn::Amplify => "amplify",
            PreludeInstanceFn::Normalize => "normalize",
            PreludeInstanceFn::Mix => "mix",
            PreludeInstanceFn::Slice => "slice",
            PreludeInstanceFn::Summarize => "summarize",
            // T20: Reactive instance method names mirror the
            // `buff_reactive::Signal` / `buff_reactive::Computed` /
            // `buff_reactive::Effect` method names 1:1.
            PreludeInstanceFn::Get => "get",
            PreludeInstanceFn::Set => "set",
            PreludeInstanceFn::Update => "update",
            PreludeInstanceFn::Invalidate => "invalidate",
            // T17: Web instance method names mirror the user-facing
            // Buff surface (`web.get(...)` / `web.middleware(...)` /
            // `web.listen(port: N)` / `web.run()`). The HTTP-verb
            // variants drop the `route_` prefix on the surface — the
            // `route_` prefix exists only at the Rust enum level to
            // disambiguate from `Env.get` / `Args.get` (shared `Get`
            // variant). `middleware` / `listen` / `run` map 1:1.
            PreludeInstanceFn::RouteGet => "get",
            PreludeInstanceFn::RoutePost => "post",
            PreludeInstanceFn::RoutePut => "put",
            PreludeInstanceFn::RouteDelete => "delete",
            PreludeInstanceFn::RoutePatch => "patch",
            PreludeInstanceFn::Middleware => "middleware",
            PreludeInstanceFn::Listen => "listen",
            PreludeInstanceFn::Run => "run",
            // T29: Validator method names mirror the
            // `buff_validate::Validator` method names 1:1. The five
            // builder methods (with_*) consume self and return Self;
            // the two action methods (validate / to_json_schema)
            // borrow self.
            PreludeInstanceFn::WithEmail => "with_email",
            PreludeInstanceFn::WithUrl => "with_url",
            PreludeInstanceFn::WithLength => "with_length",
            PreludeInstanceFn::WithRange => "with_range",
            PreludeInstanceFn::WithRegex => "with_regex",
            PreludeInstanceFn::Validate => "validate",
            PreludeInstanceFn::ToJsonSchema => "to_json_schema",
            // T42: Email builder method names. Mirror the
            // `buff_email::Email::{body, html, attach}` method names
            // 1:1 so the codegen can splice `recv.body(...)` etc.
            // without rewriting.
            PreludeInstanceFn::Body => "body",
            PreludeInstanceFn::Html => "html",
            PreludeInstanceFn::Attach => "attach",
            // T19: Template.render name. Mirrors the buff_template
            // method name 1:1. Added by T31 because the codegen
            // M::Render arm existed but the variant lookup didn't.
            PreludeInstanceFn::Render => "render",
            // T31: Cache method names. `set_ttl` surfaces as Buff
            // `cache.set(key, value, ttl)` via arity-based dispatch
            // (codegen consults arg count to lower to SetTtl vs Set).
            // `delete` / `contains` / `clear` mirror the
            // `buff_cache::Cache` method names 1:1.
            PreludeInstanceFn::SetTtl => "set",
            PreludeInstanceFn::Delete => "delete",
            PreludeInstanceFn::Contains => "contains",
            PreludeInstanceFn::Clear => "clear",
            // T44: I18n MVP instance method names mirror the
            // `buff_i18n::I18n` Rust method names 1:1 so codegen can
            // splice `recv.add_resource(locale, ftl)` / `recv.load(l)`
            // / `recv.translate(k)` without rewriting.
            PreludeInstanceFn::AddResource => "add_resource",
            PreludeInstanceFn::Load => "load",
            PreludeInstanceFn::Translate => "translate",
            // T37 (sibling): Faker instance method names — backfill
            // the name() arms the T37 task missed. Canonical names
            // mirror `buff_fake::Faker` 1:1 except FakerInt /
            // FakerDatetime which collide with Buff's `Int` / built-in
            // DateTime type names so they use the lowercased form.
            PreludeInstanceFn::Name => "name",
            PreludeInstanceFn::Email => "email",
            PreludeInstanceFn::Address => "address",
            PreludeInstanceFn::Phone => "phone",
            PreludeInstanceFn::Uuid => "uuid",
            PreludeInstanceFn::Lorem => "lorem",
            PreludeInstanceFn::FakerInt => "int",
            PreludeInstanceFn::FakerDatetime => "datetime",
            // T42 (sibling): Email builder methods — backfill the
            // name() arms the T42 task missed. Canonical Rust method
            // names 1:1.
            PreludeInstanceFn::Body => "body",
            PreludeInstanceFn::Html => "html",
            PreludeInstanceFn::Attach => "attach",
            // T43: buff-scrape instance method names mirror the
            // `buff_scrape::{Document, Element, Crawler}` Rust method
            // names 1:1 so codegen can splice `recv.text()` /
            // `recv.title()` / `recv.attr(name)` / `recv.inner_html()`
            // / `recv.seed()` / `recv.fetch(url)` / `recv.crawl(n)`
            // / `recv.robots_allows(url)` without rewriting. The
            // shared `Select` ("select") + `Html` ("html") variants
            // cover Document/Element.select + Document/Element.html.
            PreludeInstanceFn::Text => "text",
            PreludeInstanceFn::Title => "title",
            PreludeInstanceFn::Attr => "attr",
            PreludeInstanceFn::InnerHtml => "inner_html",
            PreludeInstanceFn::Seed => "seed",
            PreludeInstanceFn::Fetch => "fetch",
            PreludeInstanceFn::Crawl => "crawl",
            PreludeInstanceFn::RobotsAllows => "robots_allows",
            // T50: Xml instance method names.
            PreludeInstanceFn::Root => "root",
            PreludeInstanceFn::Find => "find",
            PreludeInstanceFn::ToString => "to_string",
            // T50: XmlElement instance method name. `name` / `text` /
            // `attr` reuse the existing shared variants (Name / Text /
            // Attr — already mapped by the Faker / Document / Element
            // arms above). `children` is XmlElement-only.
            PreludeInstanceFn::Children => "children",
            // T45: buff-geo instance method names mirror the
            // `buff_geo::{Point, LineString, Polygon}` Rust method
            // names 1:1 so the codegen can splice `recv.x()` /
            // `recv.distance_to(other)` / `recv.area()` etc. without
            // rewriting.
            PreludeInstanceFn::X => "x",
            PreludeInstanceFn::Y => "y",
            PreludeInstanceFn::DistanceTo => "distance_to",
            PreludeInstanceFn::Length => "length",
            PreludeInstanceFn::Area => "area",
            PreludeInstanceFn::Intersects => "intersects",
            // T46: buff-nlp Language instance method name mirrors the
            // `buff_nlp::Language::code` Rust method name 1:1 so the
            // codegen can splice `recv.code()` without rewriting.
            // `language.name()` reuses the existing shared `Name`
            // variant (already mapped to "name" by the Faker arm below).
            PreludeInstanceFn::Code => "code",
            // T52: buff-protobuf Message instance method names mirror
            // the `buff_protobuf::Message` Rust method names 1:1 so
            // the codegen can splice `recv.byte_size()` /
            // `recv.type_url()` / `recv.payload()` / `recv.encode()`
            // without rewriting.
            PreludeInstanceFn::ByteSize => "byte_size",
            PreludeInstanceFn::TypeUrl => "type_url",
            PreludeInstanceFn::Payload => "payload",
            PreludeInstanceFn::Encode => "encode",
            // T11: Signal instance methods (Fft, Ifft, Lowpass, Highpass,
            // Bandpass, ApplyWindow, Spectrogram, Magnitude, Phase).
            PreludeInstanceFn::Fft => "fft",
            PreludeInstanceFn::Ifft => "ifft",
            PreludeInstanceFn::Lowpass => "lowpass",
            PreludeInstanceFn::Highpass => "highpass",
            PreludeInstanceFn::Bandpass => "bandpass",
            PreludeInstanceFn::ApplyWindow => "apply_window",
            PreludeInstanceFn::Spectrogram => "spectrogram",
            PreludeInstanceFn::Magnitude => "magnitude",
            PreludeInstanceFn::Phase => "phase",
            // T47: buff-chat instance method names mirror the
            // `buff_chat::{Bot, Message, Platform}` Rust method names
            // 1:1 so the codegen can splice `recv.command(name, h)` /
            // `recv.on_message(h)` / `recv.start()` / `recv.stop()` /
            // `recv.dispatch(msg)` / `recv.is_running()` /
            // `recv.command_count()` / `recv.has_message_handler()` /
            // `recv.channel()` / `recv.author()` / `recv.platform()` /
            // `recv.is_dm()` / `recv.is_discord()` / `recv.is_telegram()`
            // without rewriting. Note `ChatMessage.text()` reuses the
            // existing shared `Text` variant (already mapped to "text"
            // by the T43 Document / Element arm above).
            PreludeInstanceFn::Command => "command",
            PreludeInstanceFn::OnMessage => "on_message",
            PreludeInstanceFn::Start => "start",
            PreludeInstanceFn::Stop => "stop",
            PreludeInstanceFn::Dispatch => "dispatch",
            PreludeInstanceFn::IsRunning => "is_running",
            PreludeInstanceFn::CommandCount => "command_count",
            PreludeInstanceFn::HasMessageHandler => "has_message_handler",
            PreludeInstanceFn::Channel => "channel",
            PreludeInstanceFn::Author => "author",
            PreludeInstanceFn::Platform => "platform",
            PreludeInstanceFn::IsDm => "is_dm",
            PreludeInstanceFn::IsDiscord => "is_discord",
            PreludeInstanceFn::IsTelegram => "is_telegram",
            // T48: buff-web3 instance method names mirror the
            // `buff_web3::{Provider, Wallet, ConnectedWallet, Contract,
            // ContractMethod}` Rust method names 1:1 so the codegen
            // can splice `recv.chain_id()` / `recv.block_number()` /
            // `recv.get_balance(&a)` / `recv.get_nonce(&a)` /
            // `recv.wait_for_tx(&h)` / `recv.sign_message(&m)` /
            // `recv.method(&n)` / `recv.arg(...)` / `recv.args(...)` /
            // `recv.call()` without rewriting. The shared `Address`
            // variant ("address") covers Wallet.address /
            // ConnectedWallet.address / Contract.address; the shared
            // `Send` variant ("send") covers ContractMethod.send;
            // the shared `Connect` variant ("connect") covers
            // Wallet.connect (these reuse existing names already
            // mapped by the Faker / SmtpClient / TCP arms).
            PreludeInstanceFn::ChainId => "chain_id",
            PreludeInstanceFn::BlockNumber => "block_number",
            PreludeInstanceFn::GetBalance => "get_balance",
            PreludeInstanceFn::GetNonce => "get_nonce",
            PreludeInstanceFn::WaitForTx => "wait_for_tx",
            PreludeInstanceFn::SignMessage => "sign_message",
            PreludeInstanceFn::Method => "method",
            PreludeInstanceFn::Arg => "arg",
            PreludeInstanceFn::Args => "args",
            PreludeInstanceFn::Call => "call",
            // T48: `wallet.connect(provider)` — name mirrors the
            // `buff_web3::Wallet::connect` Rust method name 1:1 so
            // codegen can splice `wallet.connect(provider)` without
            // rewriting. NOTE: this is the INSTANCE-method Connect
            // (PreludeInstanceFn::Connect — receiver is a wallet
            // value), NOT the assoc-fn Connect (PreludeAssocFn::
            // Connect — receiver is the bare namespace Ident
            // `TCP`/`WebSocket`). Same name, different enum.
            PreludeInstanceFn::Connect => "connect",
            // T49: RsaKeypair instance method names mirror the
            // underlying `buff_crypto_extras::RsaKeypair` Rust
            // struct field names 1:1 so codegen can splice
            // `recv.public_pem.clone()` / `recv.private_pem.clone()`
            // without rewriting.
            PreludeInstanceFn::PublicPem => "public_pem",
            PreludeInstanceFn::PrivatePem => "private_pem",
            // T54: buff-simd instance method names mirror the
            // `buff_simd::Simd` Rust method names 1:1 so codegen can
            // splice `recv.add(other)` / `recv.mul(other)` / `recv.sum()`
            // / `recv.to_vec()` etc. without rewriting.
            PreludeInstanceFn::Add => "add",
            PreludeInstanceFn::Sub => "sub",
            PreludeInstanceFn::Mul => "mul",
            PreludeInstanceFn::Div => "div",
            PreludeInstanceFn::Sum => "sum",
            PreludeInstanceFn::Min => "min",
            PreludeInstanceFn::Max => "max",
            PreludeInstanceFn::ToVec => "to_vec",
            // T73: String instance method names. `split` / `trim` /
            // `starts_with` / `ends_with` map 1:1 to Rust's str methods.
            // `to_upper` / `to_lower` map to `to_uppercase` /
            // `to_lowercase` (the codegen emits `.to_uppercase()`
            // and `.to_lowercase()` directly).
            PreludeInstanceFn::Split => "split",
            PreludeInstanceFn::Trim => "trim",
            PreludeInstanceFn::StartsWith => "starts_with",
            PreludeInstanceFn::EndsWith => "ends_with",
            PreludeInstanceFn::ToUppercase => "to_upper",
            PreludeInstanceFn::ToLowercase => "to_lower",
            // T80: Http response instance methods. Names mirror the
            // canonical HTTP response surface.
            PreludeInstanceFn::ResponseStatus => "status",
            PreludeInstanceFn::ResponseBody => "body",
            PreludeInstanceFn::ResponseJson => "json",
            PreludeInstanceFn::ResponseHeaders => "headers",
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


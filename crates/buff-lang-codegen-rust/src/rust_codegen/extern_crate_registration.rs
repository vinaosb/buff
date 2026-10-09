//! ITER-44 - extern-crate REGISTRATION seam: the program-shape
//! detection -> `extern_crates` recording block of
//! `RustCodegen::generate` (mechanically extracted from
//! rust_codegen.rs).
//!
//! Verbatim move of the flat `if program_uses_X(decls) { ... }`
//! sequence (T124b through T49) that records every Rust crate the
//! generated program depends on into `RustCodegen::extern_crates`.
//! The detection walkers live in the sibling child modules
//! (`extern_crate_detection.rs` / `extern_crate_detection_extra.rs`);
//! this seam is the RustCodegen orchestration that consults them and
//! records the crate names the pipeline / build-driver later reads
//! via `RustCodegen::extern_crates`. The parent declares only
//! `mod extern_crate_registration;` (inherent methods resolve by
//! type, no `use` needed). Child inherits parent imports via
//! `use super::*` and may access parent private fields (descendant
//! privacy).

use super::*;

impl RustCodegen {
    /// Record every external Rust crate the generated program depends
    /// on into `Self::extern_crates` (verbatim block moved from the
    /// middle of `RustCodegen::generate`, between the builtin
    /// Matrix/Error injection and the T31 async propagation).
    pub(super) fn register_extern_crates(&mut self, decls: &[Decl]) {
        // T124b: register the `chrono` crate as an external dependency when
        // the program references any prelude datetime type
        // (`DateTime.now()`, `Duration.days(7)`, `dt.format(...)`, ...).
        // Generated code uses fully-qualified `chrono::...` paths so no
        // `use chrono;` import is emitted — but the recorded name signals
        // to the pipeline / build-driver that the generated Cargo project
        // must declare `chrono` in `[dependencies]`. The set is exposed
        // via [`Self::extern_crates`].
        //
        // This mirrors the existing `extern crate "name"` recording path:
        // `extern_crates` is the canonical "Rust crates the generated
        // program depends on" set, and downstream consumers (the future
        // Cargo-project pipeline, snapshot tests) consult it via
        // [`Self::extern_crates`] / [`collect_rust_deps`].
        if program_uses_chrono(decls) {
            self.extern_crates.insert("chrono".to_string());
        }
        // T124c: register the `tracing` + `tracing-subscriber` crates as
        // external dependencies when the program references the prelude
        // `Log` module (`Log.debug/info/warn/error(...)`). Generated code
        // uses fully-qualified `tracing::...` and
        // `tracing_subscriber::...` paths so no `use` import is emitted —
        // but the recorded names signal to the pipeline / build-driver
        // that the generated Cargo project must declare BOTH crates in
        // `[dependencies]` (the subscriber init emitted in `main` calls
        // `tracing_subscriber::fmt()...try_init()`, so a program with any
        // Log call requires both).
        //
        // Mirrors the chrono registration pattern (T124b): single-file
        // `buff run` rustc path does NOT link these crates; the
        // codegen-only linking boundary is the accepted acceptance
        // criterion for v1.4 prelude modules. Cargo-project wiring is
        // deferred (snapshots + extern_crates set is the verifiable
        // contract).
        if program_uses_tracing(decls) {
            self.extern_crates.insert("tracing".to_string());
            self.extern_crates.insert("tracing-subscriber".to_string());
        }
        // T124d: register the `regex` crate as an external dependency when
        // the program references the prelude `Regex` module
        // (`Regex.compile(p)`, `regex.match(...)`, `regex.find(...)`,
        // `regex.replace(...)`, `regex.captures(...)`). Generated code
        // uses fully-qualified `regex::Regex::...` paths so no `use` import
        // is emitted — but the recorded name signals to the pipeline /
        // build-driver that the generated Cargo project must declare
        // `regex` in `[dependencies]`.
        //
        // Mirrors the chrono/tracing registration pattern (T124b/T124c):
        // single-file `buff run` rustc path does NOT link this crate;
        // the codegen-only linking boundary is the accepted acceptance
        // criterion for v1.4 prelude modules. Cargo-project wiring is
        // deferred (snapshots + extern_crates set is the verifiable
        // contract).
        if program_uses_regex(decls) {
            self.extern_crates.insert("regex".to_string());
        }
        // T124e: register the `toml` crate as an external dependency when
        // the program references the prelude `Toml` namespace module
        // (`Toml.parse(s)` / `Toml.stringify(v)`). Generated code uses
        // fully-qualified `toml::from_str` / `toml::to_string` paths so
        // no `use` import is emitted — but the recorded name signals to
        // the pipeline / build-driver that the generated Cargo project
        // must declare `toml` in `[dependencies]`.
        //
        // Mirrors the chrono/tracing/regex registration pattern
        // (T124b/T124c/T124d): single-file `buff run` rustc path does
        // NOT link this crate; the codegen-only linking boundary is the
        // accepted acceptance criterion for v1.4 prelude modules.
        // Cargo-project wiring is deferred (snapshots + extern_crates
        // set is the verifiable contract).
        if program_uses_toml(decls) {
            self.extern_crates.insert("toml".to_string());
        }
        // T124f: register the `rand` crate as an external dependency when
        // the program references the prelude `Random` namespace module
        // (`Random.int(lo, hi)`, `Random.float()`, `Random.choice(v)`,
        // `Random.shuffle(v)`). Generated code uses fully-qualified
        // `rand::rng()` / `rand::seq::IndexedRandom::*` paths so
        // no `use` import is emitted - but the recorded name signals to
        // the pipeline / build-driver that the generated Cargo project
        // must declare `rand` in `[dependencies]`.
        //
        // NOTE: `Math` and `Strings` (also T124f) wrap Rust `std` only
        // (no extern crate needed) - they have NO `program_uses_X`
        // walker for that reason. `Sort` (`.sort()` / `.sort_by()` on
        // Buff's existing Vector type) is an instance method that lowers
        // to Rust's slice `.sort()` / `.sort_by()` (also std-only, no
        // extern crate).
        //
        // Mirrors the chrono/tracing/regex/toml registration pattern
        // (T124b/T124c/T124d/T124e): single-file `buff run` rustc path
        // does NOT link this crate; the codegen-only linking boundary
        // is the accepted acceptance criterion for v1.4 prelude modules.
        // Cargo-project wiring is deferred (snapshots + extern_crates
        // set is the verifiable contract).
        if program_uses_rand(decls) {
            self.extern_crates.insert("rand".to_string());
        }
        // T124g: register the `tokio` crate as an external dependency when
        // the program references the prelude `sleep(duration)` free fn.
        // Generated code lowers `sleep(d)` to
        // `tokio::time::sleep(<d>).await`, so any program using `sleep`
        // transitively depends on the `tokio` crate being in
        // `[dependencies]` (and on the enclosing fn being async —
        // `#[tokio::main]` is auto-stamped when `main` propagates to
        // async via the T31 walker; sleep() calls in non-async fns are a
        // rustc-level error, surfaced as a normal Rust diagnostic).
        //
        // NOTE: tokio is ALREADY used by the v1.0 async lowering
        // (`tokio::spawn`, `tokio::runtime::Runtime`, `#[tokio::main]`)
        // but the async pass does NOT register the crate in
        // `extern_crates` — single-file `buff run` rustc path never
        // linked tokio either, mirroring the chrono/tracing/regex/toml
        // codegen-only boundary. The walker here is the FIRST time
        // `tokio` enters `extern_crates`; the existing async codegen
        // paths don't need to start recording it because their generated
        // `tokio::*` paths compile iff tokio is in the (deferred)
        // Cargo project's `[dependencies]`, which is exactly what this
        // walker signals.
        //
        // Walker scope: NARROW (per the T124f gotcha that chrono was
        // originally over-broad). It flags ONLY a `FuncCall` whose
        // callee is the bare Ident `sleep` — NOT every async fn, NOT
        // every `tokio::*` path fragment in the lowering (the lowering
        // is a codegen-private concern; the walker is a USER-INTENT
        // detector). Same shape as the rand walker flagging
        // `Random.<method>(...)`.
        if program_uses_tokio(decls) {
            self.extern_crates.insert("tokio".to_string());
        }
        // T124h: register the FIVE web-module extern crates (Base64 /
        // Hex / URLEncode / UUID / URL) when the program references the
        // corresponding prelude modules. Each walker is NARROW - it
        // flags ONLY the specific receiver name (`Base64` / `Hex` /
        // `URLEncode` / `UUID` / `URL`), mirroring the rand / tokio
        // walker pattern (T124f / T124g). The chrono over-broad-walker
        // gotcha (T124f) is the cautionary tale: each walker stays
        // minimal so it doesn't over-trigger on unrelated code.
        //
        // Generated code uses fully-qualified paths so no `use` import
        // is emitted - but the recorded name signals to the pipeline /
        // build-driver that the generated Cargo project must declare
        // each crate in `[dependencies]`.
        //
        // Mirrors the chrono/tracing/regex/toml/rand/tokio registration
        // pattern (T124b/T124c/T124d/T124e/T124f/T124g): single-file
        // `buff run` rustc path does NOT link these crates; the
        // codegen-only linking boundary is the accepted acceptance
        // criterion for v1.4 prelude modules. Cargo-project wiring is
        // deferred (snapshots + extern_crates set is the verifiable
        // contract).
        if program_uses_base64(decls) {
            self.extern_crates.insert("base64".to_string());
        }
        if program_uses_hex(decls) {
            self.extern_crates.insert("hex".to_string());
        }
        if program_uses_percent_encoding(decls) {
            self.extern_crates.insert("percent-encoding".to_string());
        }
        if program_uses_uuid(decls) {
            self.extern_crates.insert("uuid".to_string());
        }
        if program_uses_url(decls) {
            self.extern_crates.insert("url".to_string());
        }
        // T89: register the `rust_decimal` crate as an external dependency
        // when the program references the prelude `Decimal` type
        // (`Decimal.new(s)` / `Decimal.from_float(f)` / `d.add(other)` /
        // `d.mul(other)` / `d.to_string()`). Generated code uses fully-
        // qualified `rust_decimal::Decimal::from_str` /
        // `rust_decimal::Decimal::from_f64` paths so no `use` import is
        // emitted — but the recorded name signals to the pipeline /
        // build-driver that the generated Cargo project must declare
        // `rust_decimal` in `[dependencies]`.
        if program_uses_namespace(decls, "Decimal") {
            self.extern_crates.insert("rust_decimal".to_string());
        }
        // T124i: register the `serde_yml` and `csv` crates as external
        // dependencies when the program references the corresponding
        // prelude modules (`Yaml.parse(s)` / `Yaml.stringify(v)` /
        // `Csv.parse(s)` / `Csv.stringify(rows)`). Generated code uses
        // fully-qualified `serde_yml::from_str` / `serde_yml::to_string`
        // and `csv::ReaderBuilder::...` / `csv::Writer::from_writer`
        // paths so no `use` import is emitted - but the recorded name
        // signals to the pipeline / build-driver that the generated
        // Cargo project must declare each crate in `[dependencies]`.
        //
        // NOTE: `serde_yml` is the maintained fork of the
        // deprecated/archived `serde_yaml` crate (do NOT use
        // serde_yaml). The crate name is recorded as `serde_yml` (with
        // underscore) matching the path segments emitted by codegen.
        //
        // Mirrors the chrono/tracing/regex/toml/rand/tokio/base64/hex/
        // percent-encoding/uuid/url registration pattern
        // (T124b/T124c/T124d/T124e/T124f/T124g/T124h): single-file
        // `buff run` rustc path does NOT link these crates; the
        // codegen-only linking boundary is the accepted acceptance
        // criterion for v1.4 prelude modules. Cargo-project wiring is
        // deferred (snapshots + extern_crates set is the verifiable
        // contract).
        if program_uses_serde_yml(decls) {
            self.extern_crates.insert("serde_yml".to_string());
        }
        // T23: register the `serde_json` crate as an external dependency
        // when the program references the `Json` prelude namespace
        // (`Json.parse(s)` / `Json.stringify(v)`). Generated code uses
        // fully-qualified `serde_json::from_str` / `serde_json::to_string`
        // paths so no `use` import is emitted - but the recorded name
        // signals to the pipeline / build-driver that the generated
        // Cargo project must declare `serde_json` in `[dependencies]`.
        //
        // Mirrors the serde_yml / csv registration pattern (T124i):
        // single-file `buff run` rustc path does NOT link these crates;
        // the codegen-only linking boundary is the accepted acceptance
        // criterion for v1.4+ prelude modules.
        if program_uses_serde_json(decls) {
            self.extern_crates.insert("serde_json".to_string());
        }
        // T25: register the `reqwest` crate as an external dependency
        // when the program references the `Http` prelude namespace
        // (`Http.get(url)` / `Http.post(url, body)`). Generated code
        // uses fully-qualified `reqwest::blocking::get` /
        // `reqwest::blocking::Client::new().post(...)` paths so no
        // `use` import is emitted.
        if program_uses_namespace(decls, "Http") {
            self.extern_crates.insert("reqwest".to_string());
        }
        if program_uses_csv(decls) {
            self.extern_crates.insert("csv".to_string());
        }
        // T124j: register the `walkdir` + `tempfile` crates as external
        // dependencies when the program references the corresponding
        // prelude modules (`Dir.walk(p)` / `Tempfile.create()` /
        // `Tempfile.dir()`). Generated code uses fully-qualified
        // `walkdir::WalkDir::new(...)` and `tempfile::NamedTempFile::new()`
        // paths so no `use` import is emitted - but the recorded name
        // signals to the pipeline / build-driver that the generated
        // Cargo project must declare each crate in `[dependencies]`.
        //
        // NOTE: `Path` (value type) + `Dir.list/create/remove` +
        // `Path.exists` wrap `std::path::Path`/`PathBuf` + `std::fs::*`
        // (std-only - NO extern crate needed for those, mirroring the
        // Math/Strings/Args/Env stance from T124f/T124g). `Tempfile.dir`
        // uses `std::env::temp_dir()` (also std-only), but the narrow
        // walker still records `tempfile` for symmetry (any Tempfile.*
        // call flags the crate).
        //
        // Mirrors the chrono/tracing/regex/toml/rand/tokio/base64/hex/
        // percent-encoding/uuid/url/serde_yml/csv registration pattern
        // (T124b/T124c/T124d/T124e/T124f/T124g/T124h/T124i): single-
        // file `buff run` rustc path does NOT link these crates; the
        // codegen-only linking boundary is the accepted acceptance
        // criterion for v1.4 prelude modules. Cargo-project wiring is
        // deferred (snapshots + extern_crates set is the verifiable
        // contract).
        if program_uses_walkdir(decls) {
            self.extern_crates.insert("walkdir".to_string());
        }
        if program_uses_tempfile(decls) {
            self.extern_crates.insert("tempfile".to_string());
        }
        // T124k: register the `sha2` + `md5` + `hmac` + `hex` crates
        // as external dependencies when the program references the
        // corresponding prelude modules (`Hash.sha256(data)` /
        // `Hash.sha512(data)` / `Hash.md5(data)` /
        // `HMAC.sha256(key, data)`). Generated code uses
        // fully-qualified `sha2::Sha256::digest` / `sha2::Sha512::digest`
        // / `md5::compute` / `hmac::Hmac::<sha2::Sha256>::...` paths
        // (plus block-scoped `use sha2::Digest;` / `use hmac::Mac;`
        // for the trait methods) so no top-level `use` import is
        // emitted - but the recorded name signals to the pipeline /
        // build-driver that the generated Cargo project must declare
        // each crate in `[dependencies]`.
        //
        // NOTE: `Hash.sha256` / `Hash.sha512` both record `sha2`
        // (the SHA-2 family crate ships both digesters);
        // `Hash.md5` records `md5`; `HMAC.sha256` records `hmac`
        // + `sha2` (HMAC wraps `hmac::Hmac<sha2::Sha256>` so the
        // generated path needs both). `hex` is recorded alongside
        // each (the hex encoding is shared with T124h Hex module's
        // walker; re-recording is idempotent - extern_crates is a
        // BTreeSet).
        //
        // The narrow walkers (per-method) flag the SPECIFIC method
        // names - sha256/sha512 -> sha2; md5 -> md5; HMAC.sha256 ->
        // hmac + sha2. Mirrors the chrono-over-broad gotcha (T124f):
        // a generic `program_uses_namespace("Hash")` would
        // over-register (a program using only `Hash.md5` shouldn't
        // need `sha2`).
        //
        // Mirrors the chrono/tracing/regex/toml/rand/tokio/base64/hex/
        // percent-encoding/uuid/url/serde_yml/csv/walkdir/tempfile
        // registration pattern (T124b/T124c/T124d/T124e/T124f/T124g/
        // T124h/T124i/T124j): single-file `buff run` rustc path does
        // NOT link these crates; the codegen-only linking boundary is
        // the accepted acceptance criterion for v1.4 prelude modules.
        // Cargo-project wiring is deferred (snapshots + extern_crates
        // set is the verifiable contract).
        if program_uses_sha2(decls) {
            self.extern_crates.insert("sha2".to_string());
        }
        if program_uses_md5(decls) {
            self.extern_crates.insert("md5".to_string());
        }
        if program_uses_hmac(decls) {
            self.extern_crates.insert("hmac".to_string());
            // HMAC.sha256 lowers to `hmac::Hmac<sha2::Sha256>` so
            // the `sha2` crate is needed alongside `hmac`. Record
            // `sha2` here too (idempotent if the program also uses
            // Hash.sha256/sha512 - extern_crates is a BTreeSet).
            self.extern_crates.insert("sha2".to_string());
        }
        if program_uses_sha2(decls) || program_uses_md5(decls) || program_uses_hmac(decls) {
            // Every Hash.* / HMAC.* call emits a `hex::encode(...)`
            // for the digest / MAC bytes. Record `hex` alongside
            // (shared with T124h Hex module's walker; idempotent).
            self.extern_crates.insert("hex".to_string());
        }
        // T124l: register the `num_cpus` crate as an external
        // dependency when the program references `OS.cpus()`.
        // Generated code uses the fully-qualified
        // `num_cpus::get() as i64` path so no top-level `use`
        // import is emitted - but the recorded name signals to
        // the pipeline / build-driver that the generated Cargo
        // project must declare `num_cpus` in `[dependencies]`.
        //
        // NOTE: `OS.name` / `OS.arch` / `OS.hostname` use
        // `std::env::consts::{OS,ARCH}` + env-var hostname
        // fallback (std-only - NO extern crate needed, mirrors
        // the Math/Strings/Args/Env stance from T124f/T124g).
        // `Process.*` uses `std::process::*` (std-only - NO
        // extern crate needed, mirrors the Path/Dir.list stance
        // from T124j). The narrow `program_uses_num_cpus` walker
        // flags ONLY the `OS.cpus` method name (mirrors the
        // chrono-over-broad cautionary tale, T124f gotcha: a
        // generic `program_uses_namespace("OS")` would
        // over-register num_cpus for programs using only
        // `OS.name` / `OS.arch` / `OS.hostname`).
        //
        // Mirrors the chrono/tracing/regex/toml/rand/tokio/base64/
        // hex/percent-encoding/uuid/url/serde_yml/csv/walkdir/
        // tempfile/sha2/md5/hmac registration pattern: single-file
        // `buff run` rustc path does NOT link num_cpus; cargo-
        // project wiring is deferred (snapshots + extern_crates
        // set is the verifiable contract).
        if program_uses_num_cpus(decls) {
            self.extern_crates.insert("num_cpus".to_string());
        }
        // T124m: register the `tokio` crate for `TCP.*` / `UDP.*`
        // calls (idempotent with the existing tokio walker that
        // flags ONLY `sleep(...)` free-fn calls - the existing
        // walker does NOT fire on TCP.* / UDP.* / WebSocket.*
        // method calls). `WebSocket.*` records `tokio-tungstenite`
        // + `futures-util` via the narrow
        // `program_uses_tokio_tungstenite` walker; `tokio` is
        // pulled transitively by `tokio-tungstenite`'s dependency
        // on it (so a WebSocket-only program would have tokio in
        // its Cargo.lock even without an explicit tokio dep), but
        // we record tokio explicitly for clarity (mirrors how we
        // also record sha2 for HMAC.sha256 even though hmac pulls
        // it transitively).
        //
        // Generated code uses fully-qualified `tokio::net::*` /
        // `tokio::io::*` / `tokio_tungstenite::*` /
        // `futures_util::*` paths so no top-level `use` import is
        // emitted - but the recorded names signal to the pipeline
        // / build-driver that the generated Cargo project must
        // declare each crate in `[dependencies]`.
        //
        // Mirrors the chrono/tracing/regex/toml/rand/tokio/base64/
        // hex/percent-encoding/uuid/url/serde_yml/csv/walkdir/
        // tempfile/sha2/md5/hmac/num_cpus registration pattern
        // (T124b..T124l): single-file `buff run` rustc path does
        // NOT link these crates; cargo-project wiring is deferred
        // (snapshots + extern_crates set is the verifiable
        // contract). The `.await` calls surface a rustc-level
        // error if the enclosing function is not async (T31 walker
        // propagates async-ness ONLY through bare-Ident free-fn
        // calls, NOT method-call / namespace-assoc-fn calls, so
        // the enclosing-fn-async transformation is a deferral;
        // see issues.md).
        if program_uses_tcp(decls)
            || program_uses_udp(decls)
            || program_uses_tokio_tungstenite(decls)
        {
            self.extern_crates.insert("tokio".to_string());
        }
        if program_uses_tokio_tungstenite(decls) {
            self.extern_crates.insert("tokio-tungstenite".to_string());
            self.extern_crates.insert("futures-util".to_string());
        }
        // T2: register `buff-lang-runtime` (the in-tree runtime
        // abstraction crate) when the program references the
        // prelude `Channel` module (`Channel.new(...)`). Generated
        // code uses fully-qualified `buff_lang_runtime::Channel::new`
        // + `Sender` + `Receiver` paths so no top-level `use` import
        // is emitted - but the recorded name signals to the pipeline
        // / build-driver that the generated Cargo project must
        // declare `buff-lang-runtime` in `[dependencies]`. Also
        // records `tokio` transitively (the runtime crate wraps
        // `tokio::sync::mpsc` per Metis G6). Mirrors the chrono /
        // regex / toml / etc. registration pattern.
        if program_uses_namespace(decls, "Channel") {
            self.extern_crates.insert("buff-lang-runtime".to_string());
            self.extern_crates.insert("tokio".to_string());
        }
        // T9: register `buff-image` when the program references the
        // prelude `Image` module (`Image.from_path(...)` etc.). The
        // generated code uses fully-qualified `buff_image::Image::*`
        // paths so no top-level `use` import is emitted — but the
        // recorded name signals to the pipeline / build-driver that
        // the generated Cargo project must declare `buff-image` in
        // `[dependencies]`. Also records `image` transitively (the
        // wrapper crate wraps `image::DynamicImage`). Mirrors the
        // Channel / chrono / regex / toml registration pattern.
        if program_uses_namespace(decls, "Image") {
            self.extern_crates.insert("buff-image".to_string());
            self.extern_crates.insert("image".to_string());
        }
        // T37: register `buff-fake` when the program references the
        // prelude `Faker` module (`Faker.new()` / `Faker.with_locale()`
        // / `faker.name()` etc.). The generated code uses fully-
        // qualified `buff_fake::Faker::*` paths so no top-level `use`
        // import is emitted — but the recorded name signals to the
        // pipeline / build-driver that the generated Cargo project
        // must declare `buff-fake` in `[dependencies]`. Also records
        // `fake` transitively (the wrapper crate wraps the `fake`
        // crate). Mirrors the T9 Image registration pattern.
        if program_uses_namespace(decls, "Faker") {
            self.extern_crates.insert("buff-fake".to_string());
            self.extern_crates.insert("fake".to_string());
        }
        // T10: register `buff-audio` when the program references the
        // prelude `AudioBuffer` module (`AudioBuffer.from_path(...)`
        // etc.). Also records `hound` + `symphonia` transitively (the
        // wrapper crate wraps both for WAV / MP3 / FLAC / Vorbis
        // decode + WAV encode). Mirrors the T9 Image pattern.
        if program_uses_namespace(decls, "AudioBuffer") {
            self.extern_crates.insert("buff-audio".to_string());
            self.extern_crates.insert("hound".to_string());
            self.extern_crates.insert("symphonia".to_string());
        }
        // T17: register `buff-web` when the program references the
        // prelude `Web` module (`Web.new()` / `Web.bind(addr)` /
        // `web.get(...)` / etc.). Also records `axum` + `tokio` +
        // `serde_json` transitively (the wrapper crate wraps all
        // three for the HTTP server runtime + JSON codec). Mirrors
        // the T9 Image / T10 AudioBuffer pattern.
        if program_uses_namespace(decls, "Web") {
            self.extern_crates.insert("buff-web".to_string());
            self.extern_crates.insert("axum".to_string());
            self.extern_crates.insert("tokio".to_string());
            self.extern_crates.insert("serde_json".to_string());
        }
        // T20: register `buff-reactive` when the program references
        // any of the three reactive namespaces (`ReactiveSignal` /
        // `ReactiveComputed` / `ReactiveEffect`). Generated code uses
        // fully-qualified `buff_reactive::Signal::new` /
        // `buff_reactive::Computed::new` / `buff_reactive::Effect::new`
        // paths so no top-level `use` import is emitted — but the
        // recorded name signals to the pipeline / build-driver that
        // the generated Cargo project must declare `buff-reactive` in
        // `[dependencies]`. Mirrors the buff-image / buff-audio /
        // buff-dataframe / buff-audit pattern. The walker checks all
        // three namespaces because the user typically composes
        // Signal + Computed + Effect together; recording once for
        // any of them is sufficient (idempotent BTreeSet insert).
        if program_uses_namespace(decls, "ReactiveSignal")
            || program_uses_namespace(decls, "ReactiveComputed")
            || program_uses_namespace(decls, "ReactiveEffect")
        {
            self.extern_crates.insert("buff-reactive".to_string());
        }
        // T29: register `buff-validate` when the program references the
        // prelude `Validator` module (`Validator.new(...)` etc.). The
        // generated code uses fully-qualified `buff_validate::Validator::*`
        // paths so no top-level `use` import is emitted — but the
        // recorded name signals to the pipeline / build-driver that
        // the generated Cargo project must declare `buff-validate` in
        // `[dependencies]`. Also records `validator` (the upstream
        // validation crate whose trait methods we lower to) +
        // `serde_json` (for JSON Schema export) + `regex` (for
        // `with_regex` pattern compilation at rule-registration time).
        // Mirrors the Image / HttpClient registration pattern.
        if program_uses_namespace(decls, "Validator") {
            self.extern_crates.insert("buff-validate".to_string());
            self.extern_crates.insert("validator".to_string());
            self.extern_crates.insert("serde_json".to_string());
            self.extern_crates.insert("regex".to_string());
        }
        // T26: register `buff-audit` when the program references the
        // prelude `Audit` OR `Signature` modules (`Audit.scan(...)`
        // / `Signature.sign(...)` etc.). Also records
        // `ed25519-dalek` + `sha2` + `hex` + `rand` transitively
        // (the wrapper crate wraps all four: ed25519-dalek for
        // Ed25519 sign/verify, sha2 for the deferred manifest-hash
        // path, hex for sig/key encode/decode, rand for the OS
        // CSPRNG consumed by `Signature.keypair()`). Mirrors the
        // T9 Image / T10 Audio / T124k Hash+HMAC pattern. NO `ring`,
        // NO native-tls, NO cc-rs - ed25519-dalek is the canonical
        // pure-Rust Ed25519.
        if program_uses_namespace(decls, "Audit") || program_uses_namespace(decls, "Signature") {
            self.extern_crates.insert("buff-audit".to_string());
            self.extern_crates.insert("ed25519-dalek".to_string());
            self.extern_crates.insert("sha2".to_string());
            self.extern_crates.insert("hex".to_string());
            self.extern_crates.insert("rand".to_string());
        }
        // T27: register `buff-fuzz` when the program references either
        // the prelude `Fuzz` OR `Strategy` modules (`Fuzz.run(...)` /
        // `Strategy.int(...)` / etc.). Also records `proptest`
        // transitively (the wrapper crate wraps `proptest::test_runner::
        // TestRunner` + `proptest::strategy::Strategy` for the runtime
        // API). Mirrors the T9 Image / T10 Audio / T26 Audit pattern.
        // NO `cargo-fuzz`, NO `afl.rs`, NO cc-rs - proptest is pure-Rust
        // (matches the "no C library, no Docker" hard rule + the
        // "Windows host with no MSVC" constraint that pushed hand-rolled
        // lexer/parser).
        if program_uses_namespace(decls, "Fuzz") || program_uses_namespace(decls, "Strategy") {
            self.extern_crates.insert("buff-fuzz".to_string());
            self.extern_crates.insert("proptest".to_string());
        }
        // T18: register `buff-db` when the program references the
        // prelude `Database` module (`Database.connect(url)` etc.).
        // Also records `sqlx` + `tokio` transitively (the wrapper
        // crate wraps sqlx::any::AnyPool which needs a tokio runtime
        // via the `runtime-tokio-rustls` feature — NOT native-tls,
        // per workspace hard rule from AGENTS.md "Pure-Rust
        // preference"). Mirrors the T9 Image / T10 Audio / T26
        // Audit pattern. NO `diesel`, NO `libpq`, NO native-tls —
        // T18 task spec mandates sqlx-only.
        if program_uses_namespace(decls, "Database") {
            self.extern_crates.insert("buff-db".to_string());
            self.extern_crates.insert("sqlx".to_string());
            self.extern_crates.insert("tokio".to_string());
        }
        // T33: register `buff-http-client` + `reqwest` when the program
        // references the prelude `HttpClient` module (`HttpClient.new()` /
        // `client.get(url)` / etc.). Mirrors the T9 Image / T10 AudioBuffer
        // / T17 Web / T18 Database pattern.
        if program_uses_namespace(decls, "HttpClient") {
            self.extern_crates.insert("buff-http-client".to_string());
            self.extern_crates.insert("reqwest".to_string());
        }
        // T30: register `buff-config` + `figment` + `notify` when the
        // program references the prelude `Config` module (`Config.new()` /
        // `cfg.set_default(key, val)` / etc.). Namespace-only module
        // (mirror Log / Toml / Math / Random). The `figment` + `notify`
        // crates are the two external deps the wrapper crate wraps.
        if program_uses_namespace(decls, "Config") {
            self.extern_crates.insert("buff-config".to_string());
            self.extern_crates.insert("figment".to_string());
            self.extern_crates.insert("notify".to_string());
        }
        // T31 (frameworks): register `buff-cache` + `moka` when the
        // program references the prelude `Cache` module
        // (`Cache.new(max_capacity)` / `cache.get(k)` / etc.). Mirrors
        // the T9 Image / T10 AudioBuffer / T33 HttpClient pattern.
        // Distributed Redis backend deferred to v1.18+ — moka is the
        // only external dep the MVP wrapper crate wraps.
        if program_uses_namespace(decls, "Cache") {
            self.extern_crates.insert("buff-cache".to_string());
            self.extern_crates.insert("moka".to_string());
        }
        // T44: register `buff-i18n` when the program references the
        // I18n prelude type (`I18n.new(locale)` / `I18n.with_fallback`
        // / `i18n.add_resource` / `i18n.load` / `i18n.translate`).
        // Also records `fluent-bundle` + `unic-langid` transitively
        // (the upstream crates `buff-i18n` wraps). Distributed /
        // machine-translation backends explicitly forbidden by T44
        // spec — `fluent-bundle` + `unic-langid` are the only deps.
        if program_uses_namespace(decls, "I18n") {
            self.extern_crates.insert("buff-i18n".to_string());
            self.extern_crates.insert("fluent-bundle".to_string());
            self.extern_crates.insert("unic-langid".to_string());
        }
        // T34: register `buff-auth` when the program references any of
        // the four prelude auth modules (`JWT` / `OAuth2Client` /
        // `Password` / `Rbac`). Also records `jsonwebtoken` +
        // `argon2` + `oauth2` + `reqwest` transitively (the wrapper
        // crate wraps jsonwebtoken 10 with the pure-Rust `rust_crypto`
        // backend for HS256 JWT, argon2 0.5 for Argon2id password
        // hashing, oauth2 4 + reqwest rustls-tls for the OAuth2
        // auth-code flow). Mirrors the T9 Image / T10 Audio / T26
        // Audit / T18 Database pattern. NO `ring`, NO native-tls, NO
        // cc-rs — the T34 task spec explicitly forbids all three per
        // the "Windows host with no MSVC vcruntime.h" constraint.
        if program_uses_namespace(decls, "JWT")
            || program_uses_namespace(decls, "OAuth2Client")
            || program_uses_namespace(decls, "Password")
            || program_uses_namespace(decls, "Rbac")
        {
            self.extern_crates.insert("buff-auth".to_string());
            self.extern_crates.insert("jsonwebtoken".to_string());
            self.extern_crates.insert("argon2".to_string());
            self.extern_crates.insert("oauth2".to_string());
            self.extern_crates.insert("reqwest".to_string());
        }
        // T39: register `buff-archive` when the program references
        // the `Archive` namespace. Also records `zip` (deflate-only,
        // default-features disabled — pure-Rust), `tar` 0.4, `flate2`
        // 1.x (pure-Rust `miniz_oxide` backend), and `ruzstd` 0.8
        // (pure-Rust Zstd — NOT the canonical `zstd` crate which
        // wraps C libzstd via cc-rs, violating the "no C library"
        // hard rule). Mirrors the T9 Image / T17 Web / T18 Database
        // pattern. NO 7z, RAR, BZip2, encryption-at-rest — all
        // forbidden by the T39 task spec.
        if program_uses_namespace(decls, "Archive") {
            self.extern_crates.insert("buff-archive".to_string());
            self.extern_crates.insert("zip".to_string());
            self.extern_crates.insert("tar".to_string());
            self.extern_crates.insert("flate2".to_string());
            self.extern_crates.insert("ruzstd".to_string());
        }
        // T51: register `buff-msgpack` + `rmp-serde` + `serde_json`
        // when the program references the prelude `MsgPack` module
        // (`MsgPack.serialize(value)` / `MsgPack.deserialize(bytes)`).
        // The generated code uses fully-qualified `buff_msgpack::*`
        // paths so no top-level `use` import is emitted — but the
        // recorded name signals to the pipeline / build-driver that
        // the generated Cargo project must declare `buff-msgpack` in
        // `[dependencies]`. Also records `rmp-serde` + `serde_json`
        // transitively (the wrapper crate wraps both). Mirrors the
        // T9 Image / T39 Archive registration pattern.
        if program_uses_namespace(decls, "MsgPack") {
            self.extern_crates.insert("buff-msgpack".to_string());
            self.extern_crates.insert("rmp-serde".to_string());
            self.extern_crates.insert("serde_json".to_string());
        }
        // T52: register `buff-protobuf` + `prost` + `prost-types` +
        // `serde_json` when the program references the prelude
        // `Protobuf` OR `Message` modules (`Protobuf.serialize(value)`
        // / `Protobuf.deserialize(bytes)` / `Message.new(value)` /
        // `Message.from_bytes(bytes)` / `msg.byte_size()` / etc.).
        // The walker checks both namespaces because the user always
        // composes Protobuf + Message together (a Message value arises
        // only via Message.new which encodes via Protobuf.serialize
        // internally); recording once for either is sufficient
        // (idempotent BTreeSet insert). Also records `prost` +
        // `prost-types` + `serde_json` transitively (the wrapper crate
        // wraps all three). Mirrors the T51 MsgPack + T42 Email +
        // T50 Xml registration pattern.
        if program_uses_namespace(decls, "Protobuf") || program_uses_namespace(decls, "Message") {
            self.extern_crates.insert("buff-protobuf".to_string());
            self.extern_crates.insert("prost".to_string());
            self.extern_crates.insert("prost-types".to_string());
            self.extern_crates.insert("serde_json".to_string());
        }
        // T42: register `buff-email` + `lettre` + `handlebars` when
        // the program references the prelude `Email` OR `SmtpClient`
        // modules (`Email.new(from, to, subject)` /
        // `email.body(text)` / `email.html(tpl, ctx)` /
        // `email.attach(path)` / `SmtpClient.new(host, port, user,
        // pass)` / `client.send(email)`). The walker checks both
        // namespaces because the user always composes Email +
        // SmtpClient together; recording once for either is
        // sufficient (idempotent BTreeSet insert). Also records
        // `lettre` (the pure-Rust SMTP transport + message builder
        // via the `rustls` feature — NOT `native-tls` per AGENTS.md
        // hard rule) + `handlebars` (the templating engine shared
        // with T19 buff-template for `email.html(template, context)`
        // rendering). Mirrors the T9 Image / T18 Database / T34
        // buff-auth pattern.
        if program_uses_namespace(decls, "Email") || program_uses_namespace(decls, "SmtpClient") {
            self.extern_crates.insert("buff-email".to_string());
            self.extern_crates.insert("lettre".to_string());
            self.extern_crates.insert("handlebars".to_string());
        }
        // T43: register `buff-scrape` when the program references any
        // of the three prelude scrape namespaces (`Document.*` /
        // `Element.*` / `Crawler.*`). Also records `scraper`
        // transitively (the HTML parser + CSS selector engine wrapped
        // by `buff-scrape::Document` / `buff-scrape::Element`) and
        // `reqwest` transitively (the rustls-tls HTTP client wrapped
        // by `buff-scrape::Crawler`). The walker checks all three
        // namespaces because the user composes Document + Element +
        // Crawler together; recording once for any of them is
        // sufficient (idempotent BTreeSet insert). Mirrors the T9
        // Image / T18 Database / T34 buff-auth / T42 buff-email
        // pattern. Pure-Rust, CPU-only (no JS rendering, no
        // distributed crawling — both forbidden by T43 spec).
        if program_uses_namespace(decls, "Document")
            || program_uses_namespace(decls, "Element")
            || program_uses_namespace(decls, "Crawler")
        {
            self.extern_crates.insert("buff-scrape".to_string());
            self.extern_crates.insert("scraper".to_string());
            self.extern_crates.insert("reqwest".to_string());
        }
        // T46: register `buff-nlp` when the program references the
        // `Text` namespace (`Text.detect_language(text)` /
        // `Text.stem(word, algorithm)` / `Text.tokenize(text)` /
        // `Text.sentences(text)`). Also records `whatlang`
        // (pure-Rust trigram language identifier — 69+ languages),
        // `rust-stemmers` (pure-Rust Snowball stemmer for 18
        // languages — NOT a C binding), and `unicode-segmentation`
        // (already pinned for T124 String segmentation — pure-Rust
        // UAX #29 word + sentence segmentation). Mirrors the T9
        // Image / T18 Database / T34 buff-auth / T39 buff-archive
        // pattern. NO lemmatization, NO ML-based NER, NO embeddings
        // — all forbidden by the T46 task spec (v1.20+ work).
        if program_uses_namespace(decls, "Text") {
            self.extern_crates.insert("buff-nlp".to_string());
            self.extern_crates.insert("whatlang".to_string());
            self.extern_crates.insert("rust-stemmers".to_string());
            self.extern_crates
                .insert("unicode-segmentation".to_string());
        }
        // T45: register `buff-geo` when the program references any of
        // the three prelude geo namespaces (`Point.*` / `LineString.*`
        // / `Polygon.*`). Also records `geo` + `geo-types` transitively
        // (the wrapper crate wraps both for Euclidean distance / length
        // / area / Contains / Intersects algorithms). The walker checks
        // all three namespaces because the user typically composes
        // Point + LineString + Polygon together; recording once for any
        // of them is sufficient (idempotent BTreeSet insert). Mirrors
        // the T9 Image / T43 buff-scrape / T42 buff-email pattern.
        // Pure-Rust, CPU-only per Metis G7 lock (NO GPU dispatch).
        if program_uses_namespace(decls, "Point")
            || program_uses_namespace(decls, "LineString")
            || program_uses_namespace(decls, "Polygon")
        {
            self.extern_crates.insert("buff-geo".to_string());
            self.extern_crates.insert("geo".to_string());
            self.extern_crates.insert("geo-types".to_string());
        }
        // T54: register `buff-simd` when the program references the
        // `Simd` namespace (`Simd.splat(x)` / `Simd.from_slice(s)` /
        // `Simd.from_array(arr)` / `simd.add(other)` etc.). Also
        // records `wide` transitively (the pure-Rust portable SIMD
        // wrapper crate that `buff_simd::Simd` wraps — `wide::f32x4`).
        // Mirrors the T9 Image / T45 buff-geo / T43 buff-scrape pattern.
        // Pure-Rust, CPU-only per Metis G7 lock (NO GPU dispatch — GPU
        // SIMD is WGSL's job via `buff-lang-codegen-wgsl`); NO nightly
        // `std::simd`, NO runtime `is_x86_feature_detected!` detection
        // per T54 spec ("Must NOT" clause).
        if program_uses_namespace(decls, "Simd") {
            self.extern_crates.insert("buff-simd".to_string());
            self.extern_crates.insert("wide".to_string());
        }
        // T59: register `buff-actors` when the program references any
        // of the actor namespaces (`ActorSystem.*` / `ActorRef.*` /
        // `Supervisor.*` / `ChildSpec.*` / `RestartStrategy.*`).
        // Also records `crossbeam-channel` transitively (the
        // per-actor mailbox primitive). The MVP uses `std::thread`
        // (NOT `tokio`) for deterministic `JoinHandle::join` on
        // graceful shutdown; a future v1.18+ async variant would
        // also record `tokio`. Mirrors the T54 Simd walker pattern.
        if program_uses_namespace(decls, "ActorSystem")
            || program_uses_namespace(decls, "ActorRef")
            || program_uses_namespace(decls, "Supervisor")
            || program_uses_namespace(decls, "ChildSpec")
            || program_uses_namespace(decls, "RestartStrategy")
        {
            self.extern_crates.insert("buff-actors".to_string());
            self.extern_crates.insert("crossbeam-channel".to_string());
        }
        // T50: register `buff-xml` when the program references either
        // of the two prelude xml namespaces (`Xml.*` /
        // `XmlElement.*`). Also records `quick-xml` transitively (the
        // pure-Rust streaming XML parser wrapped by
        // `buff_xml::XmlDocument::from_str`). The walker checks both
        // namespaces because the user typically composes Xml +
        // XmlElement together (Xml.from_str returns XmlDocument,
        // whose .root() / .find() return XmlElement); recording once
        // for either is sufficient (idempotent BTreeSet insert).
        // Mirrors the T9 Image / T43 buff-scrape / T45 buff-geo
        // pattern. Pure-Rust, CPU-only.
        if program_uses_namespace(decls, "Xml") || program_uses_namespace(decls, "XmlElement") {
            self.extern_crates.insert("buff-xml".to_string());
            self.extern_crates.insert("quick-xml".to_string());
        }
        // T47: register `buff-chat` when the program references any of
        // the three prelude chat namespaces (`Bot.*` /
        // `ChatMessage.*` / `Platform.*`). Also records `serenity`
        // (Discord Gateway + HTTP API — pure-Rust via rustls_backend,
        // NOT native_tls_backend) + `teloxide` (Telegram Bot API —
        // pure-Rust via rustls, NOT native_tls) + `async-trait`
        // (serenity's EventHandler trait bridge) + `tokio` (the
        // multi-threaded runtime `Bot::start` builds internally)
        // transitively. The walker checks all three namespaces because
        // the user typically composes Bot + ChatMessage + Platform
        // together (Bot.new takes Platform; ChatMessage values arise
        // only inside handler closures); recording once for any of
        // them is sufficient (idempotent BTreeSet insert). Mirrors the
        // T9 Image / T43 buff-scrape / T45 buff-geo / T50 buff-xml
        // pattern. Pure-Rust, CPU-only; both serenity + teloxide use
        // rustls + ring (NO native-tls, NO cc-rs — matches the
        // "no C library, no Docker" hard rule from T126/T127).
        if program_uses_namespace(decls, "Bot")
            || program_uses_namespace(decls, "ChatMessage")
            || program_uses_namespace(decls, "Platform")
        {
            self.extern_crates.insert("buff-chat".to_string());
            self.extern_crates.insert("serenity".to_string());
            self.extern_crates.insert("teloxide".to_string());
            self.extern_crates.insert("async-trait".to_string());
            self.extern_crates.insert("tokio".to_string());
        }
        // T48: register `buff-web3` when the program references any of
        // the five prelude web3 namespaces (`Provider.*` / `Wallet.*` /
        // `ConnectedWallet.*` / `Contract.*` / `ContractMethod.*`).
        // Also records `ethers` (the upstream Ethereum RPC + signer
        // crate, with the `rustls` feature — NOT native-tls per
        // AGENTS.md hard rule), `tokio` (the multi-threaded runtime
        // shared via `buff_web3`'s OnceLock), `reqwest` (transitive
        // via ethers' Http provider — rustls-tls), `serde_json`
        // (transitive via ethers' ABI parser), and `hex` (transitive
        // via Wallet.sign_message hex-encoding + ContractMethod tx-
        // hash formatting). The walker checks all five namespaces
        // because the user always composes Provider + Wallet +
        // Contract together (a ContractMethod value arises only via
        // `contract.method(name)` which requires a Contract, which
        // requires either a Provider or ConnectedWallet); recording
        // once for any of them is sufficient (idempotent BTreeSet
        // insert). Mirrors the T9 Image / T18 Database / T34 buff-auth
        // / T42 buff-email / T47 buff-chat pattern. Pure-Rust, CPU-
        // only (network I/O never runs on the GPU path).
        if program_uses_namespace(decls, "Provider")
            || program_uses_namespace(decls, "Wallet")
            || program_uses_namespace(decls, "ConnectedWallet")
            || program_uses_namespace(decls, "Contract")
            || program_uses_namespace(decls, "ContractMethod")
        {
            self.extern_crates.insert("buff-web3".to_string());
            self.extern_crates.insert("ethers".to_string());
            self.extern_crates.insert("tokio".to_string());
            self.extern_crates.insert("reqwest".to_string());
            self.extern_crates.insert("serde_json".to_string());
            self.extern_crates.insert("hex".to_string());
        }
        // T49: register `buff-crypto-extras` when the program references
        // any of the five prelude crypto-extras namespaces (`AES.*` /
        // `RSA.*` / `ECDH.*` / `Argon2.*` / `RsaKeypair.*`). Also
        // records the upstream RustCrypto crates the wrapper consumes:
        // `aes-gcm` (AES-256-GCM AEAD), `rsa` (PKCS#1 v1.5 SHA-256
        // signatures), `p256` + `p384` (NIST ECDH key agreement),
        // `argon2` (raw Argon2id KDF — shared with T34 buff-auth's
        // PHC-string Password hashing), `sha2` (pulled transitively by
        // rsa + p256 + argon2; recorded explicitly for clarity),
        // `rand` (CSPRNG for nonce/key/salt generation), `signature`
        // (Verifier + RandomizedSigner traits used by the RSA path —
        // the rsa crate re-exports them but we record signature
        // explicitly for the extern_crates contract), and `hex` (for
        // hex-encoded test vectors + diagnostics). The walker checks
        // all five namespaces because the user typically composes
        // AES + RSA + ECDH + Argon2 + RsaKeypair together (a
        // RsaKeypair value arises only via `RSA.generate_keypair`);
        // recording once for any of them is sufficient (idempotent
        // BTreeSet insert). Mirrors the T9 Image / T43 buff-scrape /
        // T45 buff-geo / T50 buff-xml / T47 buff-chat / T48 buff-web3
        // pattern. Pure-Rust, CPU-only (NO ring, NO native-tls, NO
        // cc-rs — matches the AGENTS.md "no C library" hard rule).
        if program_uses_namespace(decls, "AES")
            || program_uses_namespace(decls, "RSA")
            || program_uses_namespace(decls, "ECDH")
            || program_uses_namespace(decls, "Argon2")
            || program_uses_namespace(decls, "RsaKeypair")
        {
            self.extern_crates.insert("buff-crypto-extras".to_string());
            self.extern_crates.insert("aes-gcm".to_string());
            self.extern_crates.insert("rsa".to_string());
            self.extern_crates.insert("p256".to_string());
            self.extern_crates.insert("p384".to_string());
            self.extern_crates.insert("argon2".to_string());
            self.extern_crates.insert("sha2".to_string());
            self.extern_crates.insert("rand".to_string());
            self.extern_crates.insert("signature".to_string());
            self.extern_crates.insert("hex".to_string());
        }
    }
}

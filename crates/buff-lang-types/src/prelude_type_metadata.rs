//! T105b: PreludeType metadata impl + lookup helpers.
//!
//! MECHANICAL EXTRACTION from prelude_types.rs (T105b God Class split).
//! No logic changes — moved verbatim. The `impl PreludeType` block + the
//! `is_prelude_type` / `prelude_type_lookup` free functions live here;
//! the `pub enum PreludeType` definition stays in prelude_types.rs.

use crate::prelude_types::PreludeType;
use crate::ty::Type;

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
        // T23: Json - JSON serialization namespace wrapping serde_json.
        // Mirrors Yaml / Toml exactly (parse + stringify).
        PreludeType::Yaml,
        PreludeType::Json,
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
        // T37: Faker — runtime-value type with rich instance methods.
        // Mirrors Image / Regex. Codegen lowering lives in the
        // buff-fake crate (`buff_fake::Faker::*`); the codegen arm +
        // PreludeAssocFn/InstanceFn entries are added in this same
        // commit (T37 owns the full Faker surface).
        PreludeType::Faker,
        // T31: Cache — runtime-value type wrapping `buff_cache::Cache`
        // (moka sync backend). Mirrors Image / DataFrame: codegen
        // lowering lives in the buff-cache crate
        // (`buff_cache::Cache::*`); the codegen arm +
        // PreludeAssocFn/InstanceFn entries are added in the same
        // commit (T31 owns the full Cache surface). Distributed
        // Redis backend deferred to v1.18+ per the T31 spec.
        PreludeType::Cache,
        // T44: I18n — internationalization runtime-value type wrapping
        // `buff_i18n::I18n` (Mozilla `fluent-bundle` + `unic-langid`).
        // Mirrors Image / Cache / Faker: codegen lowering lives in the
        // buff-i18n crate; the codegen arm + PreludeAssocFn/InstanceFn
        // entries for the MVP ctor + instance-method surface (New /
        // WithFallback / AddResource / Load / Translate) are added in
        // the same commit (T44 owns the full MVP surface). The other
        // 7 instance methods (SetFallback / AvailableLocales /
        // CurrentLocale / FallbackLocale / TranslateWithArgs /
        // HasMessage / Warnings) are available in the Rust crate but
        // codegen-wiring deferred to a follow-up to keep the shared-
        // zone footprint minimal. EXPERIMENTAL badge per T44 spec.
        // Pure-Rust only — NO machine translation, NO RTL helpers.
        PreludeType::I18n,
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
        // T10 (v1.13 wave 2): AudioBuffer - the runtime-value audio
        // type wrapping the in-tree `buff-audio` crate
        // (`buff_audio::AudioBuffer`) backed by `hound` (WAV) +
        // `symphonia` (MP3/FLAC/Vorbis). Constructed via `AudioBuffer.
        // from_path(p)` (decode) or `AudioBuffer.from_samples(s, sr,
        // ch)` (programmatic). Nine instance methods: `samples` /
        // `sample_rate` / `channels` / `duration_secs` / `save` /
        // `amplify` / `normalize` / `mix` / `slice`. Codegen lowering
        // lives in the buff-audio crate (`buff_audio::AudioBuffer::*`).
        // EXPERIMENTAL badge per T10 spec. CPU-only per Metis G7.
        // Real-time playback deferred to v1.18+; synthesis deferred
        // to buff-dsp T11.
        PreludeType::Audio,
        // T17 (v1.15 frameworks wave 3): Web - HTTP server runtime-value
        // type wrapping the in-tree `buff-web` crate (`buff_web::Web`)
        // backed by `axum` 0.8 + `tokio` + `serde_json`. Constructed via
        // `Web.new()` (empty) or `Web.bind(addr)` (empty + bind addr);
        // 8 instance methods (get / post / put / delete / patch /
        // middleware / listen / run). EXPERIMENTAL badge per T17 spec.
        // The assoc fns return `Type::Unknown` (forward-declaration -
        // the coordinated `Type::Web` variant in `ty.rs` is a follow-up
        // sibling task outside the T17 shared zone, mirroring the T8
        // Tensor / T11 Signal / T12-Tensor precedent). Codegen lowering
        // for the 2 assoc fns is shipped in this T17 commit (dispatch
        // on PreludeType, NOT Type, so no Type::Web variant needed).
        PreludeType::Web,
        // T18: Database — runtime-value type returned by
        // `Database.connect(url)`. Forward-declared as `Type::Unknown`
        // (mirrors the T17 Web precedent); the codegen lowering for
        // `Database.connect` is shipped in this same T18 commit, but
        // instance-method dispatch (`pool.query` / `pool.execute` /
        // `pool.begin` / `tx.commit` / `tx.rollback`) is deferred to
        // a sibling task that adds the coordinated `Type::Pool`
        // variant in `ty.rs` (outside the T18 shared zone per the
        // MUST NOT in the task brief). EXPERIMENTAL badge per T18
        // spec. Records `buff-db` + `sqlx` + `tokio` in codegen
        // `extern_crates` when a Buff program uses `Database.*`.
        PreludeType::Database,
        // T33: HttpClient — idiomatic HTTP client wrapping reqwest.
        // Runtime-value type (mirrors Regex / Image / World). The
        // assoc fn `HttpClient.new()` returns the runtime value;
        // instance methods `client.get(url)` / `client.post(url)` /
        // `client.put(url)` / `client.delete(url)` return opaque
        // RequestBuilder values (typed `Type::Unknown` for MVP).
        // Records `buff-http-client` + `reqwest` in codegen
        // `extern_crates` when a Buff program uses `HttpClient.*`.
        PreludeType::HttpClient,
        // T30: Config — layered configuration namespace (viper-equivalent).
        // Wraps the `buff-config` crate (`buff_config::Config`). Namespace-
        // only module (mirror Log / Toml / Math / Random). The assoc fns
        // `Config.new()` / `Config.set_default(key, val)` / `Config.load_file(p)`
        // / `Config.load_env(prefix)` / `Config.load_args(args)` / `Config.get(key)`
        // / `Config.get_int(key)` / `Config.get_float(key)` / `Config.get_bool(key)`
        // / `Config.watch(path, callback)` are all dispatched on the
        // PreludeType::Config namespace. `buff_type()` returns Type::Void
        // (Config is namespace-only — no runtime value). Records `buff-config`
        // + `figment` + `notify` in codegen `extern_crates` when a Buff
        // program uses `Config.*` (mirrors the chrono / regex / tracing
        // codegen-only linking boundary). Pure-Rust, no native deps.
        PreludeType::Config,
        // T21: Observe — observability namespace (OpenTelemetry-equivalent).
        // Namespace-only module (mirror Log / Toml / Math / Random).
        // Records `buff-observe` in codegen `extern_crates` when a Buff
        // program uses `Observe.*`.
        PreludeType::Observe,
        // T29: Validator — declarative schema validator (pydantic-
        // equivalent). Runtime-value type (mirrors Regex / Image /
        // HttpClient). The assoc fn `Validator.new()` returns an empty
        // Validator; the instance methods `validator.with_email(field)`
        // / `.with_url(field)` / `.with_length(field, min, max)` /
        // `.with_range(field, min, max)` / `.with_regex(field, pattern)`
        // are builder methods that consume self and return a new
        // Validator (Buff "no visible references" stance); the action
        // methods `validator.validate(map) -> Result<Void, String>` and
        // `validator.to_json_schema() -> String` run validation and
        // serialize JSON Schema respectively. Records `buff-validate`
        // + `validator` + `serde_json` in codegen `extern_crates` when
        // a Buff program uses `Validator.*`. Pure-Rust, no native deps.
        PreludeType::Validator,
        // T39: Archive — namespace-only module (mirror Log / Toml /
        // Math / Config / Observe) wrapping the in-tree pure-Rust
        // `buff-archive` crate. Two assoc fns: `Archive.compress_dir`
        // / `Archive.extract`. Records `buff-archive` + `zip` + `tar`
        // + `flate2` + `ruzstd` in codegen `extern_crates` when a
        // Buff program uses `Archive.*`. Pure-Rust, no native deps
        // (NOT the canonical `zstd` crate — see the variant rustdoc
        // above + root Cargo.toml workspace rationale).
        PreludeType::Archive,
        // T46: Text — namespace-only module (mirror Archive / Log /
        // Toml / Math / Config / Observe) wrapping the in-tree pure-
        // Rust `buff-nlp` crate. Four assoc fns: `Text.detect_
        // language` / `Text.stem` / `Text.tokenize` / `Text.
        // sentences`. Records `buff-nlp` + `whatlang` + `rust-
        // stemmers` + `unicode-segmentation` in codegen `extern_
        // crates` when a Buff program uses `Text.*`. Pure-Rust, no
        // native deps (whatlang + rust-stemmers + unicode-segmentation
        // — all pure-Rust, NO C bindings, NO cc-rs).
        PreludeType::Text,
        // T42: Email — runtime-value type with rich instance methods.
        // Mirrors Image / Faker / HttpClient / Validator. Codegen
        // lowering lives in the buff-email crate
        // (`buff_email::Email::*`); the codegen arm +
        // PreludeAssocFn/InstanceFn entries are added in this same
        // commit (T42 owns the full Email surface).
        PreludeType::Email,
        // T42: SmtpClient — runtime-value type wrapping
        // `buff_email::SmtpClient` (lettre SmtpTransport). Mirrors
        // Email: codegen lowering in the buff-email crate, codegen
        // arm + PreludeAssocFn/InstanceFn entries in this same
        // commit.
        PreludeType::SmtpClient,
        // T43: Document / Element / Crawler — three runtime-value
        // types wrapping the in-tree pure-Rust `buff-scrape` crate
        // (`buff_scrape::{Document, Element, Crawler}`) backed by
        // `scraper` (HTML parse + CSS selectors) + `reqwest`
        // (rustls-tls HTTP). All three are runtime-value-with-rich-
        // instance-methods types (after Regex / URL / Path / Process
        // / Image / Cache / HttpClient). Codegen lowering lives in
        // the buff-scrape crate; the codegen arm +
        // PreludeAssocFn/InstanceFn entries are added in this same
        // commit (T43 owns the full scrape surface). Pure-Rust,
        // CPU-only; NO JS rendering (T43 spec); NO distributed
        // crawling (T43 spec).
        PreludeType::Document,
        PreludeType::Element,
        PreludeType::Crawler,
        // T50: Xml — runtime-value type wrapping `buff_xml::XmlDocument`.
        // Constructed via `Xml.from_str(xml)`; carries the instance methods
        // `.root()`, `.find(xpath)`, `.to_string()`. Mirrors Image / Faker
        // as a runtime-value-with-instance-methods type. Pure-Rust, CPU-only.
        PreludeType::Xml,
        // T50: XmlElement — runtime-value type wrapping
        // `buff_xml::XmlElement`. Constructed via
        // `XmlElement.new(name, text, attrs)`; carries the instance
        // methods `.name()`, `.attr(name)`, `.text()`, `.children()`.
        // Pure-Rust, CPU-only.
        PreludeType::XmlElement,
        // T51: MsgPack — MessagePack binary format namespace.
        // Namespace-only (like Log / Toml / Base64 / Hex / Yaml /
        // Csv). Provides `MsgPack.serialize(value) -> Bytes` and
        // `MsgPack.deserialize(bytes) -> Value`. Codegen lowering
        // lives in the buff-msgpack crate (`buff_msgpack::serialize`
        // / `buff_msgpack::deserialize`). Pure-Rust, no native deps.
        PreludeType::MsgPack,
        // T45: Point / LineString / Polygon — three runtime-value geo
        // types wrapping the in-tree pure-Rust `buff-geo` crate
        // (`buff_geo::{Point, LineString, Polygon}`) backed by `geo` +
        // `geo-types`. All three are runtime-value-with-rich-instance-
        // methods types (after Regex / URL / Path / Process / Image /
        // Cache / HttpClient). Codegen lowering lives in the buff-geo
        // crate; the codegen arm + PreludeAssocFn/InstanceFn entries
        // are added in this same commit (T45 owns the full geo surface).
        // Pure-Rust, CPU-only per Metis G7 lock; NO GPU dispatch.
        PreludeType::Point,
        PreludeType::LineString,
        PreludeType::Polygon,
        // T46: Language / StemAlgorithm — two new prelude types for the
        // buff-nlp surface. Language is a runtime-value (mirrors Point —
        // constructed via Text.detect_language, carries code/name
        // instance methods). StemAlgorithm is an opaque enum passed
        // only as an arg to Text.stem (mirrors no prior type exactly).
        // Codegen lowering lives in the buff-nlp crate; the codegen
        // arms + PreludeInstanceFn entries are added in this same
        // commit (T46 owns the full nlp prelude+codegen surface).
        // Pure-Rust, CPU-only.
        PreludeType::Language,
        PreludeType::StemAlgorithm,
        // T52: Protobuf / Message — two new prelude types for the
        // buff-protobuf surface. Protobuf is namespace-only (mirrors
        // MsgPack — Protobuf.serialize / .deserialize / .roundtrip
        // are the only callable fns; the namespace itself has no value).
        // Message is a runtime-value (mirrors Image / Xml — Message.new
        // / Message.from_bytes / Message.decode are the ctors; carries
        // byte_size / type_url / payload / encode instance methods).
        // Codegen lowering lives in the buff-protobuf crate; the codegen
        // arms + PreludeInstanceFn entries are added in this same
        // commit (T52 owns the full protobuf prelude+codegen surface).
        // Pure-Rust, CPU-only; gRPC streaming + prost-build deferred.
        // T52: Protobuf / Message — protobuf runtime-value + namespace
        // types. `Protobuf` is namespace-only (mirrors MsgPack); `Message`
        // is a runtime value (mirrors Image / Xml / Point). Records
        // `buff-protobuf` + `prost` + `prost-types` + `serde_json` in
        // extern_crates via the `program_uses_namespace("Protobuf")` /
        // ("Message") walkers.
        PreludeType::Protobuf,
        PreludeType::Message,
        // T47: Bot / ChatMessage / Platform — buff-chat runtime values
        // (Discord + Telegram via serenity + teloxide). `Bot` /
        // `ChatMessage` are runtime values (mirrors Image / Point);
        // `Platform` is an enum-like runtime value (mirrors
        // StemAlgorithm). Records `buff-chat` + `serenity` + `teloxide`
        // + `async-trait` + `tokio` in extern_crates via the
        // `program_uses_namespace("Bot")` / ("ChatMessage") /
        // ("Platform") walkers.
        PreludeType::Bot,
        PreludeType::ChatMessage,
        PreludeType::Platform,
        // T48: Provider / Wallet / ConnectedWallet / Contract /
        // ContractMethod — buff-web3 runtime values (Ethereum RPC +
        // smart contract bindings via ethers-rs). All five are runtime
        // values (mirrors Image / Point / Bot — none are namespace-only
        // like Log / Toml / MsgPack). Records `buff-web3` + `ethers`
        // + `tokio` + `reqwest` + `serde_json` + `hex` in extern_crates
        // via the `program_uses_namespace("Provider")` / ("Wallet") /
        // ("ConnectedWallet") / ("Contract") / ("ContractMethod")
        // walkers (the walker fires on any of the five names because
        // the user always composes Provider + Wallet + Contract
        // together; recording once for any of them is sufficient —
        // idempotent BTreeSet insert).
        PreludeType::Provider,
        PreludeType::Wallet,
        PreludeType::ConnectedWallet,
        PreludeType::Contract,
        PreludeType::ContractMethod,
        // T49: buff-crypto-extras prelude types — 4 namespaces
        // (AES / RSA / ECDH / Argon2) + 1 instance type
        // (RsaKeypair, constructed via RSA.generate_keypair). All
        // registered via `program_uses_namespace("AES")` /
        // ("RSA") / ("ECDH") / ("Argon2") / ("RsaKeypair")
        // walkers.
        PreludeType::AES,
        PreludeType::RSA,
        PreludeType::ECDH,
        PreludeType::Argon2,
        PreludeType::RsaKeypair,
        // T54: Simd — 4-lane f32 SIMD register runtime-value type
        // wrapping the in-tree pure-Rust `buff-simd` crate
        // (`buff_simd::Simd`) backed by `wide` (f32x4). Registered via
        // `program_uses_namespace("Simd")` walker. Pure-Rust, CPU-only
        // per Metis G7 lock (NO GPU dispatch); NO nightly std::simd,
        // NO runtime detection per T54 spec.
        PreludeType::Simd,
        // T59: buff-actors prelude types — 5 namespaces / runtime
        // values (ActorSystem + ActorRef + Supervisor + ChildSpec +
        // RestartStrategy). ActorSystem / ActorRef / Supervisor /
        // ChildSpec ARE runtime values; RestartStrategy is namespace-
        // only. All registered via `program_uses_namespace
        // ("ActorSystem" | "Supervisor" | ...)` walker.
        PreludeType::ActorSystem,
        PreludeType::ActorRef,
        PreludeType::Supervisor,
        PreludeType::ChildSpec,
        PreludeType::RestartStrategy,
        // T24: File — namespace-only file I/O module wrapping
        // std::fs. NO extern crate needed (std-only, mirroring
        // Math / Strings / Args / Env).
        PreludeType::File,
        // T25: Http — namespace-only HTTP client module wrapping
        // reqwest (blocking, rustls-tls). Records `reqwest` in
        // extern_crates when a Buff program uses `Http.*`.
        PreludeType::Http,
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
            // `rand::rng().random_range(...)` etc. for the four
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
            // T23: the Json prelude type name. Mirrors the Rust crate
            // name (`serde_json`) so the codegen can splice
            // `serde_json::from_str` / `serde_json::to_string` paths
            // without rewriting. PascalCase mirrors Yaml / Toml / Csv.
            PreludeType::Json => "Json",
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
            // T37: Faker - canonical name matching the user-facing
            // `Faker.new()` / `Faker.with_locale(...)` / `faker.name()`
            // surface. The underlying Rust type is `buff_fake::Faker`.
            PreludeType::Faker => "Faker",
            // T31: Cache - canonical name matching the user-facing
            // `Cache.new(...)` / `cache.get(...)` / `cache.set(...)`
            // surface. The underlying Rust type is `buff_cache::Cache`.
            PreludeType::Cache => "Cache",
            // T44: I18n - canonical name matching the user-facing
            // `I18n.new(...)` / `i18n.translate(...)` surface. The
            // underlying Rust type is `buff_i18n::I18n`.
            PreludeType::I18n => "I18n",
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
            // T26: Audit / Signature — canonical PascalCase names
            // matching the user-facing `Audit.scan(...)` /
            // `Signature.sign(...)` surface.
            PreludeType::Audit => "Audit",
            PreludeType::Signature => "Signature",
            // T20: ReactiveSignal / ReactiveComputed / ReactiveEffect —
            // canonical PascalCase names matching the user-facing
            // `ReactiveSignal.new(...)` / `ReactiveComputed.new(...)`
            // / `ReactiveEffect.new(...)` surface. The codegen
            // splices `buff_reactive::Signal::new` /
            // `buff_reactive::Computed::new` / `buff_reactive::Effect::new`
            // paths directly. The `Reactive` prefix avoids clashing
            // with the existing T11 DSP `Signal` namespace.
            PreludeType::ReactiveSignal => "ReactiveSignal",
            PreludeType::ReactiveComputed => "ReactiveComputed",
            PreludeType::ReactiveEffect => "ReactiveEffect",
            // T17: Web - canonical name matching the user-facing
            // `Web.new()` / `Web.bind(addr)` / `web.get(...)` surface.
            // The underlying Rust type is `buff_web::Web` (the wrapper
            // around `axum::Router` + `tokio::runtime` + serde_json).
            PreludeType::Web => "Web",
            // T18: Database - canonical PascalCase name matching the
            // user-facing `Database.connect(url)` surface. The codegen
            // splices `buff_db::Pool::connect(url)` directly.
            PreludeType::Database => "Database",
            // T33: HttpClient — canonical PascalCase name matching the
            // user-facing `HttpClient.new()` / `client.get(url)` surface.
            // The codegen splices `buff_http_client::HttpClient::new()`
            // directly.
            PreludeType::HttpClient => "HttpClient",
            // T29: Validator — canonical PascalCase name matching the
            // user-facing `Validator.new()` / `validator.with_email(field)`
            // / `validator.validate(map)` surface. The codegen splices
            // `buff_validate::Validator::new()` directly.
            PreludeType::Validator => "Validator",
            // T42: Email — canonical PascalCase name matching the
            // user-facing `Email.new(from, to, subject)` /
            // `email.body(text)` / `email.html(template, ctx)` /
            // `email.attach(path)` surface. The codegen splices
            // `buff_email::Email::new(...)` directly.
            PreludeType::Email => "Email",
            // T42: SmtpClient — canonical PascalCase name matching the
            // user-facing `SmtpClient.new(host, port, user, pass)` /
            // `client.send(email)` surface. The codegen splices
            // `buff_email::SmtpClient::new(...)` directly.
            PreludeType::SmtpClient => "SmtpClient",
            // T43: Document / Element / Crawler — canonical PascalCase
            // names matching the user-facing `Document.from_html(html)`
            // / `doc.select(css)` / `el.text()` / `Crawler.new(seed)`
            // / `crawler.fetch(url)` / `crawler.crawl(max_pages)`
            // surfaces. The codegen splices
            // `buff_scrape::{Document, Element, Crawler}::*` directly.
            PreludeType::Document => "Document",
            PreludeType::Element => "Element",
            PreludeType::Crawler => "Crawler",
            // T30: Config — canonical PascalCase name matching the
            // user-facing `Config.new()` / `cfg.set_default(key, val)` /
            // `cfg.load_file(path)` / `cfg.load_env(prefix)` /
            // `cfg.load_args(args)` / `cfg.get(key)` / `cfg.get_int(key)` /
            // `cfg.get_float(key)` / `cfg.get_bool(key)` /
            // `cfg.watch(path, callback)` surface. The underlying Rust
            // type is `buff_config::Config`. Namespace-only (no runtime
            // value — mirrors Log / Toml / Math / Random).
            PreludeType::Config => "Config",
            // T21: Observe — canonical PascalCase name matching the
            // user-facing `Observe.span(name)` / `Observe.counter(name)`
            // surface. Namespace-only (no runtime value).
            PreludeType::Observe => "Observe",
            // Forward-declared by parallel sibling tasks (T27 Fuzz,
            // T34 Jwt, etc.) — name matches the Debug variant name.
            PreludeType::Fuzz => "Fuzz",
            PreludeType::Strategy => "Strategy",
            PreludeType::Jwt => "Jwt",
            PreludeType::OAuth2Client => "OAuth2Client",
            PreludeType::Password => "Password",
            PreludeType::Rbac => "Rbac",
            // T39: Archive — canonical PascalCase name matching the
            // user-facing `Archive.compress_dir(...)` /
            // `Archive.extract(...)` surface. The underlying Rust
            // namespace is `buff_archive::Archive` (a unit struct
            // namespace marker — never instantiated). Namespace-only
            // module (mirrors Log / Toml / Math / Config / Observe).
            PreludeType::Archive => "Archive",
            // T46: Text — canonical PascalCase name matching the
            // user-facing `Text.detect_language(...)` / `Text.stem(...)`
            // / `Text.tokenize(...)` / `Text.sentences(...)` surface.
            // The underlying Rust namespace is `buff_nlp::Text` (a unit
            // struct namespace marker — never instantiated).
            // Namespace-only module (mirrors Archive / Log / Toml /
            // Math / Config / Observe).
            PreludeType::Text => "Text",
            // T50: Xml — canonical PascalCase name matching the user-facing
            // `Xml.from_str(xml)` / `doc.root()` / `doc.find(xpath)` surface.
            // The underlying Rust type is `buff_xml::XmlDocument`.
            PreludeType::Xml => "Xml",
            // T50: XmlElement — canonical PascalCase name matching the
            // user-facing `XmlElement.new(name, text, attrs)` /
            // `el.name()` / `el.attr(name)` / `el.text()` /
            // `el.children()` surface. The underlying Rust type is
            // `buff_xml::XmlElement`.
            PreludeType::XmlElement => "XmlElement",
            // T51: MsgPack — MessagePack binary format namespace.
            // Namespace-only (no runtime value — mirrors Log / Toml /
            // Base64 / Hex / Yaml / Csv). The underlying Rust crate
            // is `buff_msgpack`.
            PreludeType::MsgPack => "MsgPack",
            // T45: Point / LineString / Polygon — canonical PascalCase
            // names matching the user-facing `Point.new(x, y)` /
            // `LineString.from_coords(...)` / `Polygon.new(ring)`
            // surface. The underlying Rust types are
            // `buff_geo::{Point, LineString, Polygon}`.
            PreludeType::Point => "Point",
            PreludeType::LineString => "LineString",
            PreludeType::Polygon => "Polygon",
            // T54: Simd — canonical PascalCase name matching the
            // user-facing `Simd.splat(x)` / `Simd.from_array(arr)`
            // surface. The underlying Rust type is `buff_simd::Simd`.
            PreludeType::Simd => "Simd",
            // T59: actor types — canonical PascalCase names matching
            // the user-facing `ActorSystem.new()` / `system.spawn(...)`
            // / `Supervisor.new(...)` / `ChildSpec.new(...)` /
            // `RestartStrategy.permanent` surface. Underlying Rust
            // crate is `buff-actors`.
            PreludeType::ActorSystem => "ActorSystem",
            PreludeType::ActorRef => "ActorRef",
            PreludeType::Supervisor => "Supervisor",
            PreludeType::ChildSpec => "ChildSpec",
            PreludeType::RestartStrategy => "RestartStrategy",
            // T46: Language / StemAlgorithm — canonical PascalCase names
            // matching the user-facing `lang.code()` / `lang.name()`
            // (Language) and `Text.stem(word, algorithm: .english)`
            // (StemAlgorithm — written as `.english` enum-variant literal
            // at the call site, but the type name itself surfaces only
            // in diagnostics). The underlying Rust types are
            // `buff_nlp::{Language, StemAlgorithm}`.
            PreludeType::Language => "Language",
            PreludeType::StemAlgorithm => "StemAlgorithm",
            // T52: Protobuf / Message — canonical PascalCase names
            // matching the user-facing `Protobuf.serialize(value)` /
            // `Protobuf.deserialize(bytes)` / `Message.new(value)` /
            // `msg.byte_size()` surface. The underlying Rust crate is
            // `buff_protobuf` (`buff_protobuf::serialize` /
            // `buff_protobuf::Message`).
            PreludeType::Protobuf => "Protobuf",
            PreludeType::Message => "Message",
            // T47: Bot / ChatMessage / Platform — canonical PascalCase
            // names matching the user-facing `Bot.new(platform, token)`
            // / `bot.command(name, handler)` / `ChatMessage.new(...)`
            // / `msg.text()` / `Platform.Discord` /
            // `platform.is_discord()` surface. The underlying Rust crate
            // is `buff_chat` (`buff_chat::Bot` / `buff_chat::Message` /
            // `buff_chat::Platform`). Note `ChatMessage` (not
            // `Message`) — T52 owns the shorter `Message` name
            // (protobuf).
            PreludeType::Bot => "Bot",
            PreludeType::ChatMessage => "ChatMessage",
            PreludeType::Platform => "Platform",
            // T48: buff-web3 types — canonical PascalCase names
            // matching the user-facing `Provider.new(url)` /
            // `Wallet.from_private_key(key)` / `wallet.connect(p)` /
            // `Contract.new(addr, abi, wallet)` / `contract.method(name)`
            // / `m.arg(name, value)` / `m.call()` / `m.send()` surface.
            // The underlying Rust crate is `buff_web3`
            // (`buff_web3::{Provider, Wallet, ConnectedWallet, Contract,
            // ContractMethod}`).
            PreludeType::Provider => "Provider",
            PreludeType::Wallet => "Wallet",
            PreludeType::ConnectedWallet => "ConnectedWallet",
            PreludeType::Contract => "Contract",
            PreludeType::ContractMethod => "ContractMethod",
            // T49: buff-crypto-extras prelude types — names mirror the
            // Buff surface so source-level identifiers map 1:1.
            PreludeType::AES => "AES",
            PreludeType::RSA => "RSA",
            PreludeType::ECDH => "ECDH",
            PreludeType::Argon2 => "Argon2",
            PreludeType::RsaKeypair => "RsaKeypair",
            // T24: File — namespace-only file I/O module.
            PreludeType::File => "File",
            // T25: Http — namespace-only HTTP client module.
            PreludeType::Http => "Http",
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
            // T23: namespace-only - Json has no value representation.
            // Mirrors Yaml / Toml exactly: the namespace itself is
            // never a value, only its associated functions
            // (`Json.parse` / `Json.stringify`) are callable.
            PreludeType::Json => Type::Void,
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
            // T37: Faker IS a runtime value (NOT namespace-only).
            // Returns the opaque [`Type::Faker`] variant; the codegen
            // layer maps it to `buff_fake::Faker`.
            PreludeType::Faker => Type::Faker,
            // T31: Cache IS a runtime value (NOT namespace-only).
            // Returns the opaque [`Type::Cache`] variant; the codegen
            // layer maps it to `buff_cache::Cache`.
            PreludeType::Cache => Type::Cache,
            // T44: I18n IS a runtime value (NOT namespace-only).
            // Returns the opaque [`Type::I18n`] variant; the codegen
            // layer maps it to `buff_i18n::I18n`.
            PreludeType::I18n => Type::I18n,
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
            // T26: Audit / Signature are namespace-only modules
            // (mirror Log / Toml / Hash / HMAC / OS). The namespace
            // itself has no value representation; only its associated
            // functions are callable.
            PreludeType::Audit => Type::Void,
            PreludeType::Signature => Type::Void,
            // T20: ReactiveSignal / ReactiveComputed / ReactiveEffect
            // are namespace-only modules. Their assoc fn returns are
            // typed at the call site (Type::Unknown forward-declaration
            // in assoc_fn_return_type). Mirrors Channel / Audit /
            // Signature exactly.
            PreludeType::ReactiveSignal => Type::Void,
            PreludeType::ReactiveComputed => Type::Void,
            PreludeType::ReactiveEffect => Type::Void,
            // T17: Web IS a runtime value (NOT namespace-only) but
            // for MVP returns Type::Unknown as a forward-declaration
            // contract. The coordinated `Type::Web` variant in `ty.rs`
            // is a follow-up sibling task OUTSIDE the T17 shared zone
            // (mirrors the T8 Tensor / T11 Signal / T12-Tensor
            // precedent). The codegen layer splices `buff_web::Web::*`
            // paths directly in the assoc-fn lowering; the instance-
            // method lowering (web.get / web.listen / ...) is also a
            // follow-up (requires Type::Web for receiver-type dispatch
            // in `instance_fn_lookup`).
            PreludeType::Web => Type::Unknown,
            // T18: Database IS a runtime value (NOT namespace-only).
            // Returns [`Type::Unknown`] (forward-declaration, mirrors
            // the T17 Web precedent); the codegen layer splices the
            // real `buff_db::Pool::*` paths. A coordinated
            // [`Type::Pool`] variant in `ty.rs` is a follow-up
            // sibling task OUTSIDE the T18 shared zone per the MUST
            // NOT in the task brief. `Database.connect(url)` returns
            // the runtime Pool value (also typed as `Type::Unknown`
            // in `assoc_fn_return_type` — the codegen emits
            // `buff_db::Pool::connect(&url).await?`).
            PreludeType::Database => Type::Unknown,
            // T33: HttpClient IS a runtime value (NOT namespace-only).
            // Returns the opaque [`Type::HttpClient`] variant; the
            // codegen layer maps it to `buff_http_client::HttpClient`.
            PreludeType::HttpClient => Type::HttpClient,
            // T29: Validator IS a runtime value (NOT namespace-only).
            // Returns the opaque [`Type::Validator`] variant; the
            // codegen layer maps it to `buff_validate::Validator`.
            PreludeType::Validator => Type::Validator,
            // T42: Email IS a runtime value (NOT namespace-only).
            // Returns the opaque [`Type::Email`] variant; the codegen
            // layer maps it to `buff_email::Email`.
            PreludeType::Email => Type::Email,
            // T42: SmtpClient IS a runtime value (NOT namespace-only).
            // Returns the opaque [`Type::SmtpClient`] variant; the
            // codegen layer maps it to `buff_email::SmtpClient`.
            PreludeType::SmtpClient => Type::SmtpClient,
            // T43: Document / Element / Crawler ARE runtime values
            // (NOT namespace-only). Returns the opaque
            // [`Type::Document`] / [`Type::Element`] /
            // [`Type::Crawler`] variants; the codegen layer maps them
            // to `buff_scrape::{Document, Element, Crawler}`.
            PreludeType::Document => Type::Document,
            PreludeType::Element => Type::Element,
            PreludeType::Crawler => Type::Crawler,
            // T30: Config is a namespace-only module (mirror Log / Toml /
            // Math / Random). The namespace itself has no value
            // representation; only its associated functions are callable.
            // `buff_type()` returns Type::Void (Config is NOT a runtime
            // value — it's a namespace for layered config operations).
            PreludeType::Config => Type::Void,
            // T21: Observe is a namespace-only module (mirror Log / Toml /
            // Math / Random). The namespace itself has no value
            // representation; only its associated functions are callable.
            // `buff_type()` returns Type::Void (Observe is NOT a runtime
            // value — it's a namespace for observability operations).
            PreludeType::Observe => Type::Void,
            // T27: Fuzz + Strategy are namespace-only modules (mirror
            // Log / Toml / Hash / HMAC / OS / Audit / Signature). The
            // assoc fns return opaque `buff_fuzz::*` values whose Type
            // variants are forward-declared as Unknown (mirrors the
            // Channel / Tensor / Signal-DSP precedent).
            PreludeType::Fuzz => Type::Unknown,
            PreludeType::Strategy => Type::Unknown,
            // T34: JWT + Password are namespace-only modules (mirror
            // Audit / Signature / Hash / HMAC). The assoc fns return
            // String / Map / Bool — NOT an opaque value type — so the
            // namespace itself returns Type::Void (NO forward-declaration
            // gap; the lowering is direct).
            PreludeType::Jwt => Type::Void,
            PreludeType::Password => Type::Void,
            // T34: OAuth2Client + Rbac ARE runtime values (NOT
            // namespace-only). Returns [`Type::Unknown`] for MVP — the
            // coordinated Type::OAuth2Client / Type::Rbac variants in
            // `ty.rs` are follow-up sibling tasks OUTSIDE the T34 shared
            // zone (mirrors the T17 Web / T18 Database forward-declaration
            // precedent). The codegen layer splices the real
            // `buff_auth::OAuth2Client::*` / `buff_auth::Rbac::*` paths.
            PreludeType::OAuth2Client => Type::Unknown,
            PreludeType::Rbac => Type::Unknown,
            // T39: Archive is a namespace-only module (mirror Log /
            // Toml / Math / Config / Observe). The namespace itself
            // has no value representation; only its associated
            // functions (`Archive.compress_dir` / `Archive.extract`)
            // are callable. Both return Void (the side-effecting
            // archive operations write to disk; the Buff user reads
            // the result via the filesystem). Mirrors the Log / Toml /
            // Config / Observe / Hash / HMAC / OS pattern exactly.
            PreludeType::Archive => Type::Void,
            // T46: Text is a namespace-only module (mirror MsgPack /
            // Archive / Log / Toml). The namespace itself has no value
            // representation; only its associated functions
            // (`Text.detect_language` / `Text.stem` / `Text.tokenize` /
            // `Text.sentences`) are callable. detect_language returns
            // Option<Language>, stem returns String (with `?`
            // propagating NlpError), tokenize / sentences return
            // Vector<String>. The arm returns `Type::Text` (not Void)
            // for match-exhaustiveness — mirrors the T51 MsgPack
            // pattern (`MsgPack => Type::MsgPack`).
            PreludeType::Text => Type::Text,
            // T46: Language IS a runtime value (NOT namespace-only).
            // Returns the opaque [`Type::Language`] variant; the codegen
            // layer maps it to `buff_nlp::Language`. Mirrors Point /
            // Image / Xml.
            PreludeType::Language => Type::Language,
            // T46: StemAlgorithm IS a runtime value (an opaque enum
            // passed only as an arg). Returns the opaque
            // [`Type::StemAlgorithm`] variant; the codegen layer maps
            // it to `buff_nlp::StemAlgorithm`.
            PreludeType::StemAlgorithm => Type::StemAlgorithm,
            // T52: Protobuf is a namespace-only module (mirror MsgPack
            // / Log / Toml / Base64 / Hex / Yaml / Csv). The namespace
            // itself has no value representation; only its associated
            // functions (`Protobuf.serialize` / `Protobuf.deserialize`
            // / `Protobuf.roundtrip`) are callable. Returns
            // [`Type::Protobuf`] for match-exhaustiveness (mirrors
            // `MsgPack => Type::MsgPack`); the codegen arm rarely fires.
            PreludeType::Protobuf => Type::Protobuf,
            // T52: Message IS a runtime value (NOT namespace-only).
            // Returns the opaque [`Type::Message`] variant; the codegen
            // layer maps it to `buff_protobuf::Message`.
            PreludeType::Message => Type::Message,
            // T51: MsgPack is a namespace-only module (mirror Log /
            // Toml / Base64 / Hex / Yaml / Csv). The namespace itself
            // has no value representation; only its associated
            // functions (`MsgPack.serialize` / `MsgPack.deserialize`)
            // are callable. Both return `Vector<Byte>` (Bytes) and
            // `Value` respectively.
            PreludeType::MsgPack => Type::MsgPack,
            // T50: Xml IS a runtime value (NOT namespace-only).
            // Returns the opaque [`Type::Xml`] variant; the codegen
            // layer maps it to `buff_xml::XmlDocument`.
            PreludeType::Xml => Type::Xml,
            // T50: XmlElement IS a runtime value (NOT namespace-only).
            // Returns the opaque [`Type::XmlElement`] variant; the
            // codegen layer maps it to `buff_xml::XmlElement`.
            PreludeType::XmlElement => Type::XmlElement,
            // T45: Point / LineString / Polygon ARE runtime values
            // (NOT namespace-only). Returns the matching opaque Type
            // variant; the codegen layer maps them to
            // `buff_geo::{Point, LineString, Polygon}`.
            PreludeType::Point => Type::Point,
            PreludeType::LineString => Type::LineString,
            PreludeType::Polygon => Type::Polygon,
            // T54: Simd IS a runtime value (NOT namespace-only).
            // Returns the opaque [`Type::Simd`] variant; the codegen
            // layer maps it to `buff_simd::Simd`.
            PreludeType::Simd => Type::Simd,
            // T59: actor types — ActorSystem / ActorRef / Supervisor /
            // ChildSpec / RestartStrategy. Returns the matching opaque
            // Type variant; the codegen layer maps them to
            // `buff_actors::*` / `buff_actors::supervisor::*`.
            PreludeType::ActorSystem => Type::ActorSystem,
            PreludeType::ActorRef => Type::ActorRef,
            PreludeType::Supervisor => Type::Supervisor,
            PreludeType::ChildSpec => Type::ChildSpec,
            PreludeType::RestartStrategy => Type::RestartStrategy,
            // T47: Bot / ChatMessage / Platform ARE runtime values
            // (NOT namespace-only). Returns the matching opaque Type
            // variant; the codegen layer maps them to
            // `buff_chat::{Bot, Message (as ChatMessage), Platform}`.
            PreludeType::Bot => Type::Bot,
            PreludeType::ChatMessage => Type::ChatMessage,
            PreludeType::Platform => Type::Platform,
            // T48: Provider / Wallet / ConnectedWallet / Contract /
            // ContractMethod ARE runtime values (NOT namespace-only).
            // Returns the matching opaque Type variant; the codegen
            // layer maps them to `buff_web3::{Provider, Wallet,
            // ConnectedWallet, Contract, ContractMethod}`.
            PreludeType::Provider => Type::Provider,
            PreludeType::Wallet => Type::Wallet,
            PreludeType::ConnectedWallet => Type::ConnectedWallet,
            PreludeType::Contract => Type::Contract,
            PreludeType::ContractMethod => Type::ContractMethod,
            // T49: AES / RSA / ECDH / Argon2 are namespace-only modules
            // (mirror MsgPack / Log / Toml). The namespace itself has no
            // value representation; only its associated functions are
            // callable. Returns the matching opaque Type variant for
            // match-exhaustiveness (mirrors `MsgPack => Type::MsgPack`);
            // the codegen arm rarely fires in practice.
            PreludeType::AES => Type::AES,
            PreludeType::RSA => Type::RSA,
            PreludeType::ECDH => Type::ECDH,
            PreludeType::Argon2 => Type::Argon2,
            // T49: RsaKeypair IS a runtime value (NOT namespace-only).
            // Returns the opaque [`Type::RsaKeypair`] variant; the codegen
            // layer maps it to `buff_crypto_extras::RsaKeypair`. Mirrors
            // Image / Point / Bot / Wallet as runtime-value-with-instance-
            // methods types.
            PreludeType::RsaKeypair => Type::RsaKeypair,
            // T24: File — namespace-only file I/O module (mirrors
            // Log / Toml / Math). Returns Void — the namespace itself
            // is never a value, only its associated functions are callable.
            PreludeType::File => Type::Void,
            // T25: Http — namespace-only HTTP client module (mirrors
            // File / Log / Toml / Math). Returns Void — the namespace
            // itself is never a value, only its associated functions
            // are callable.
            PreludeType::Http => Type::Void,
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
                | PreludeType::Json
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
                | PreludeType::Audit
                | PreludeType::Signature
                | PreludeType::ReactiveSignal
                | PreludeType::ReactiveComputed
                | PreludeType::ReactiveEffect
                | PreludeType::Config
                | PreludeType::Observe
                | PreludeType::Jwt
                | PreludeType::Password
                | PreludeType::Archive
                | PreludeType::Text
                | PreludeType::MsgPack
                | PreludeType::Protobuf
                | PreludeType::AES
                | PreludeType::RSA
                | PreludeType::ECDH
                | PreludeType::Argon2
                | PreludeType::Simd
                | PreludeType::RestartStrategy
                | PreludeType::File
                | PreludeType::Http
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

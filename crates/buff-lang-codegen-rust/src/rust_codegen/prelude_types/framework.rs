//! ITER-42 - framework-crate prelude-type associated-function lowering:
//! the DataFrame .. Decimal arms of the `(PreludeType, PreludeAssocFn)`
//! match (pure move out of the parent, `prelude_types.rs`).
//!
//! Verbatim move of the trailing ~90 match arms plus the original
//! unreachable-combination fallback. The parent match reaches this
//! method only through its trailing wildcard arm, so the combined arm
//! order (and therefore behavior) is identical to the pre-split single
//! match: no parent arm overlaps any pattern here. The
//! one_arg/no_args/two_args/n_args arity closures are DEFINED in the
//! parent (they capture the original call ptype/pmethod/args for the
//! arity-mismatch error text) and travel here as ONE 4-tuple of
//! `impl Fn` parameters (clippy counts `self`, so four separate
//! closure params would trip too_many_arguments); the tuple is
//! destructured into the original names before the match, so every
//! arm body is byte-identical. The grandchild
//! inherits the parent glob-imported names via `use super::*`
//! (chained through prelude_types) and may call the parent private
//! methods (descendant privacy).

use super::*;

impl RustCodegen {
    /// Lower the framework-crate prelude-type associated-function calls
    /// (`DataFrame.from_csv`, `Image.from_path`, ..., `Decimal.from_float`):
    /// the second half of the lowering table documented on
    /// [`Self::lower_prelude_type_assoc_fn`]. Reached only via the
    /// parent's trailing wildcard delegation; the arity closures come
    /// from the parent, so arity-mismatch errors carry the same text as
    /// pre-split.
    pub(super) fn lower_prelude_type_assoc_fn_framework(
        &mut self,
        ptype: buff_lang_types::PreludeType,
        pmethod: buff_lang_types::PreludeAssocFn,
        args: &[Expr],
        arity: (
            impl Fn(&mut Self) -> Result<SynExpr, CodegenError>,
            impl Fn(&mut Self) -> Result<(), CodegenError>,
            impl Fn(&mut Self) -> Result<(SynExpr, SynExpr), CodegenError>,
            impl Fn(&mut Self, usize) -> Result<Vec<SynExpr>, CodegenError>,
        ),
    ) -> Result<SynExpr, CodegenError> {
        use buff_lang_types::{PreludeAssocFn as A, PreludeType as T};
        let (one_arg, no_args, two_args, n_args) = arity;
        match (ptype, pmethod) {
            // T7: DataFrame.from_csv(path) -> DataFrame. Wraps
            // `buff_dataframe::DataFrame::from_csv(path)
            // .unwrap_or_default()` (panic-free on file-not-found /
            // parse failure — DataFrame impls Default as the empty
            // frame, matching Buff's "no panicking generated code"
            // rule). Records `buff-dataframe` in extern_crates via
            // the narrow `program_uses_namespace("DataFrame")` walker.
            (T::DataFrame, A::FromCsv) => {
                let arg = one_arg(self)?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    buff_dataframe::DataFrame::from_csv(#arg).unwrap_or_default()
                };
                syn::parse2(tokens).map_err(|e| {
                    self.unsupported(&format!("DataFrame.from_csv codegen parse: {e}"))
                })
            }
            // T7: DataFrame.from_json(path) -> DataFrame. Same shape
            // as FromCsv — panic-free via `unwrap_or_default()`.
            // Reads JSON-lines (one JSON object per line).
            (T::DataFrame, A::FromJson) => {
                let arg = one_arg(self)?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    buff_dataframe::DataFrame::from_json(#arg).unwrap_or_default()
                };
                syn::parse2(tokens).map_err(|e| {
                    self.unsupported(&format!("DataFrame.from_json codegen parse: {e}"))
                })
            }
            // T9: Image.from_path(path) -> Image. Wraps
            // `buff_image::Image::from_path(arg).unwrap_or_default()`
            // (panic-free on file-not-found / decode failure / codec
            // panic — Image impls Default as a 1x1 transparent pixel,
            // matching Buff's "no panicking generated code" rule).
            // Records `buff-image` + `image` in extern_crates via the
            // `program_uses_namespace("Image")` walker.
            (T::Image, A::FromPath) => {
                let arg = one_arg(self)?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    buff_image::Image::from_path(#arg).unwrap_or_default()
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Image.from_path codegen parse: {e}")))
            }
            // T9: Image.from_bytes(bytes) -> Image. Same shape as
            // FromPath — panic-free via `unwrap_or_default()`. The
            // arg is a `Vector<Byte>` on the Buff surface (Vec<u8>
            // after codegen lowering); the codegen passes it by ref
            // to `buff_image::Image::from_bytes(&bytes)`.
            (T::Image, A::FromBytes) => {
                let arg = one_arg(self)?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    buff_image::Image::from_bytes(&#arg).unwrap_or_default()
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Image.from_bytes codegen parse: {e}")))
            }
            // T37: Faker.new() -> Faker. Wraps
            // `buff_fake::Faker::new()` (default locale en-US, random
            // seed). Infallible — no unwrap_or_default needed. Records
            // `buff-fake` + `fake` in extern_crates via the
            // `program_uses_namespace("Faker")` walker.
            (T::Faker, A::New) => {
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    buff_fake::Faker::new()
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Faker.new codegen parse: {e}")))
            }
            // T37: Faker.with_locale(locale) -> Faker. One arg (String
            // locale, either "en-US" or "pt-BR"). Wraps
            // `buff_fake::Faker::with_locale(locale)`.
            (T::Faker, A::WithLocale) => {
                let arg = one_arg(self)?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    buff_fake::Faker::with_locale(match #arg.as_str() {
                        "pt-BR" => buff_fake::FakerLocale::PtBr,
                        _ => buff_fake::FakerLocale::EnUs,
                    })
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Faker.with_locale codegen parse: {e}")))
            }
            // T37: Faker.with_seed(locale, seed) -> Faker. Two args
            // (String locale, Int seed). Wraps
            // `buff_fake::Faker::with_seed(locale, seed)`.
            (T::Faker, A::WithSeed) => {
                if args.len() != 2 {
                    return Err(self.unsupported(&format!(
                        "Faker.with_seed expects exactly 2 args (locale, seed), got {}",
                        args.len()
                    )));
                }
                let locale = self.lower_expr(&args[0])?;
                let seed = self.lower_expr(&args[1])?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    buff_fake::Faker::with_seed(match #locale.as_str() {
                        "pt-BR" => buff_fake::FakerLocale::PtBr,
                        _ => buff_fake::FakerLocale::EnUs,
                    }, #seed as u64)
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Faker.with_seed codegen parse: {e}")))
            }
            // T10: AudioBuffer.from_path(path) -> AudioBuffer. Wraps
            // `buff_audio::AudioBuffer::from_path(arg)
            // .unwrap_or_default()` (panic-free on file-not-found /
            // decode failure / codec panic — AudioBuffer impls Default
            // as an empty 44100Hz mono buffer, matching Buff's "no
            // panicking generated code" rule). Records `buff-audio` +
            // `hound` + `symphonia` in extern_crates via the
            // `program_uses_namespace("AudioBuffer")` walker.
            (T::Audio, A::FromPath) => {
                let arg = one_arg(self)?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    buff_audio::AudioBuffer::from_path(#arg).unwrap_or_default()
                };
                syn::parse2(tokens).map_err(|e| {
                    self.unsupported(&format!("AudioBuffer.from_path codegen parse: {e}"))
                })
            }
            // T10: AudioBuffer.from_samples(samples, sample_rate,
            // channels) -> AudioBuffer. Three args (Vec<f32>, u32,
            // u16). The Buff surface passes (Vector<Float>, Int,
            // Int); codegen casts Int -> u32 / u16 at the call site
            // (mirrors the i64 -> usize cast in Channel.new).
            // Panic-free via `unwrap_or_default()` (AudioBuffer
            // impls Default — invalid params collapse to empty).
            (T::Audio, A::FromSamples) => {
                if args.len() != 3 {
                    return Err(self.unsupported(&format!(
                        "from_samples() expects exactly 3 args (samples, sample_rate, channels), got {}",
                        args.len()
                    )));
                }
                let samples = self.lower_expr(&args[0])?;
                let sample_rate = self.lower_expr(&args[1])?;
                let channels = self.lower_expr(&args[2])?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    buff_audio::AudioBuffer::from_samples(#samples, #sample_rate as u32, #channels as u16)
                        .unwrap_or_default()
                };
                syn::parse2(tokens).map_err(|e| {
                    self.unsupported(&format!("AudioBuffer.from_samples codegen parse: {e}"))
                })
            }
            // T26: Audit.scan(path) -> Vector<String>. One arg (String
            // / Path). Wraps `buff_audit::scan(&arg)
            // .unwrap_or_default()` (panic-free on io / advisory-DB
            // failure - empty Vec, matching Buff's "no panicking
            // generated code" rule). Records `buff-audit` +
            // `ed25519-dalek` + `sha2` + `hex` + `rand` in
            // extern_crates via the narrow
            // `program_uses_namespace("Audit")` walker.
            (T::Audit, A::Scan) => {
                let arg = one_arg(self)?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    buff_audit::scan(#arg).unwrap_or_default()
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Audit.scan codegen parse: {e}")))
            }
            // T26: Audit.list() -> Vector<String>. Zero args. Wraps
            // `buff_audit::known_advisories()` (infallible - returns
            // the static `advisory_db::ALL` ID list). Records the
            // same extern_crates set as Audit.scan. Reuses the
            // existing T124g `List` variant (shared between Args.list
            // / Env.list / Audit.list - same shared-variant pattern
            // as Parse / Get / Encode).
            (T::Audit, A::List) => {
                no_args(self)?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    buff_audit::known_advisories()
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Audit.list codegen parse: {e}")))
            }
            // T26: Signature.sign(data, secret_hex) -> String. Two
            // args (Vector<Byte>, String). Wraps `buff_audit::sign
            // (&data, &secret_hex).unwrap_or_default()` (panic-free
            // on bad-key / bad-hex - empty String). The `&#data` is
            // `&Vec<u8>` which Rust auto-derefs to `&[u8]` at the
            // call site. Records the same extern_crates set as
            // Audit.* via the `program_uses_namespace("Signature")`
            // walker.
            (T::Signature, A::Sign) => {
                let mut lowered = n_args(self, 2)?;
                let data = lowered.remove(0);
                let secret_hex = lowered.remove(0);
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    buff_audit::sign(&#data, &#secret_hex).unwrap_or_default()
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Signature.sign codegen parse: {e}")))
            }
            // T26: Signature.verify(data, sig_hex, public_hex) ->
            // Bool. Three args. Wraps `buff_audit::verify(...).unwrap_
            // or(false)` (the unwrap_or(false) is the contract: bad
            // signature, bad key, OR bad hex all collapse to false -
            // NEVER panics, NEVER errors. The T26 task spec mandates
            // the bool return so a future `buff add --no-verify`
            // bypass layers cleanly).
            (T::Signature, A::Verify) => {
                let mut lowered = n_args(self, 3)?;
                let data = lowered.remove(0);
                let sig_hex = lowered.remove(0);
                let public_hex = lowered.remove(0);
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    buff_audit::verify(&#data, &#sig_hex, &#public_hex).unwrap_or(false)
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Signature.verify codegen parse: {e}")))
            }
            // T26: Signature.keypair() -> (String, String). Zero
            // args. Wraps `buff_audit::keypair()
            // .unwrap_or_default()` (the unwrap_or_default collapses
            // a Panic error to two empty Strings - NEVER panics).
            // Used by `buff publish --sign` to mint a fresh signing
            // identity per package release.
            (T::Signature, A::Keypair) => {
                no_args(self)?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    buff_audit::keypair().unwrap_or_default()
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Signature.keypair codegen parse: {e}")))
            }
            // T27: Fuzz.run(strategy, iterations, closure) -> Void.
            // Three args (Strategy, Int, closure). Wraps
            // `buff_fuzz::run(&strategy, iterations as u32, |n: i64| closure_body(n))`.
            // The lowered call returns FuzzSummary; the codegen
            // discards it via `let _ = buff_fuzz::run(...)` so the
            // Buff surface stays Void-only. Records `buff-fuzz` +
            // `proptest` in extern_crates via the narrow
            // `program_uses_namespace("Fuzz")` walker.
            //
            // The `Run` variant is Fuzz-only - no other prelude type
            // exposes a method named `run` today.
            (T::Fuzz, A::Run) => {
                if args.len() != 3 {
                    return Err(self.unsupported(&format!(
                        "Fuzz.run() expects exactly 3 args (strategy, iterations, closure), got {}",
                        args.len()
                    )));
                }
                let strategy = self.lower_expr(&args[0])?;
                let iterations = self.lower_expr(&args[1])?;
                let closure = self.lower_expr(&args[2])?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    let _ = buff_fuzz::run(&#strategy, #iterations as u32, #closure);
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Fuzz.run codegen parse: {e}")))
            }
            // T27: Strategy.int(min, max) -> Strategy. Two args (Int, Int).
            // Wraps `buff_fuzz::Strategy::int(min, max)`. The `Int`
            // variant is shared with `Random.int(min, max)` (T124f),
            // dispatched on the (Strategy, Int) pair.
            (T::Strategy, A::Int) => {
                if args.len() != 2 {
                    return Err(self.unsupported(&format!(
                        "Strategy.int() expects exactly 2 args (min, max), got {}",
                        args.len()
                    )));
                }
                let min = self.lower_expr(&args[0])?;
                let max = self.lower_expr(&args[1])?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    buff_fuzz::Strategy::int(#min, #max)
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Strategy.int codegen parse: {e}")))
            }
            // T27: Strategy.float(min, max) -> Strategy. Two args (Float, Float).
            // Wraps `buff_fuzz::Strategy::float(min, max)`. The `Float`
            // variant is shared with `Random.float()` (T124f), dispatched
            // on the (Strategy, Float) pair.
            (T::Strategy, A::Float) => {
                if args.len() != 2 {
                    return Err(self.unsupported(&format!(
                        "Strategy.float() expects exactly 2 args (min, max), got {}",
                        args.len()
                    )));
                }
                let min = self.lower_expr(&args[0])?;
                let max = self.lower_expr(&args[1])?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    buff_fuzz::Strategy::float(#min as f64, #max as f64)
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Strategy.float codegen parse: {e}")))
            }
            // T27: Strategy.bool() -> Strategy. Zero args. Wraps
            // `buff_fuzz::Strategy::bool()`.
            (T::Strategy, A::Bool) => {
                no_args(self)?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    buff_fuzz::Strategy::bool()
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Strategy.bool codegen parse: {e}")))
            }
            // T27: Strategy.string(max_len) -> Strategy. One arg (Int).
            // Wraps `buff_fuzz::Strategy::string(max_len)`.
            (T::Strategy, A::String) => {
                let arg = one_arg(self)?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    buff_fuzz::Strategy::string(#arg as usize)
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Strategy.string codegen parse: {e}")))
            }
            // T27: Strategy.bytes(max_len) -> Strategy. One arg (Int).
            // Wraps `buff_fuzz::Strategy::bytes(max_len)`.
            (T::Strategy, A::Bytes) => {
                let arg = one_arg(self)?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    buff_fuzz::Strategy::bytes(#arg as usize)
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Strategy.bytes codegen parse: {e}")))
            }
            // T20: ReactiveSignal.new(value) -> Signal<T>. One arg (T).
            // Wraps `buff_reactive::Signal::new(value)` (infallible).
            (T::ReactiveSignal, A::New) => {
                let arg = one_arg(self)?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    buff_reactive::Signal::new(#arg)
                };
                syn::parse2(tokens).map_err(|e| {
                    self.unsupported(&format!("ReactiveSignal.new codegen parse: {e}"))
                })
            }
            // T20: ReactiveComputed.new(fn) -> Computed<T>. One arg
            // (closure `Fn() -> T`). Wraps `buff_reactive::Computed::new(fn)`.
            (T::ReactiveComputed, A::New) => {
                let arg = one_arg(self)?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    buff_reactive::Computed::new(#arg)
                };
                syn::parse2(tokens).map_err(|e| {
                    self.unsupported(&format!("ReactiveComputed.new codegen parse: {e}"))
                })
            }
            // T20: ReactiveEffect.new(fn) -> Effect. One arg (closure
            // `Fn() -> Void`). Wraps `buff_reactive::Effect::new(fn)`.
            (T::ReactiveEffect, A::New) => {
                let arg = one_arg(self)?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    buff_reactive::Effect::new(#arg)
                };
                syn::parse2(tokens).map_err(|e| {
                    self.unsupported(&format!("ReactiveEffect.new codegen parse: {e}"))
                })
            }
            // T18: Database.connect(url) -> Pool (forward-declared as
            // `Type::Unknown` in prelude_types.rs; the buff-db crate's
            // `Pool` type IS the runtime value). Wraps
            // `buff_db::Pool::connect(&url).await?` (the `?` propagates
            // `DbError` per Buff's R3 error-mapping contract — the
            // Buff user's surrounding fn must return
            // `Result<T, buff_db::DbError>` so the `?` splices
            // cleanly; the Buff `?` operator is the standard error-
            // propagation idiom, mirroring `regex::Regex::new(p)?`
            // from T124d and `buff_image::Image::from_path(p)?` from
            // T9). The `.await` is auto-inserted by the T31 async-
            // propagation pass when the surrounding fn is async
            // (Buff has no `await` keyword). Records `buff-db` +
            // `sqlx` + `tokio` in extern_crates via the narrow
            // `program_uses_namespace("Database")` walker. Same
            // shared `Connect` variant as `TCP.connect(host, port)`
            // / `WebSocket.connect(url)`; dispatched on the
            // (Database, Connect) pair (mirrors `Parse` shared
            // between DateTime / Date / Toml / URL / UUID).
            //
            // One arg (String url — e.g. `"sqlite::memory:"` or
            // `"postgres://user:pass@host/db"`). The codegen does NOT
            // cast the arg — it passes the owned `String` directly to
            // `Pool::connect(url: &str)` (Rust's deref coercion lifts
            // `String` to `&str` automatically). The returned `Pool`
            // value is the receiver for the deferred `.query()` /
            // `.execute()` / `.begin()` instance methods (a sibling
            // task adds `Type::Pool` + instance-method lowering arms).
            (T::Database, A::Connect) => {
                let url = one_arg(self)?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    buff_db::Pool::connect(&#url).await?
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Database.connect codegen parse: {e}")))
            }
            // T17: Web.new() -> Web. Zero args. Wraps
            // `buff_web::Web::new()` (infallible - returns an empty
            // Web with no routes / no middleware / no bind addr).
            // Records `buff-web` + `axum` + `tokio` + `serde_json` in
            // extern_crates via the `program_uses_namespace("Web")`
            // walker. Dispatch on (PreludeType::Web, New) - mirrors
            // the (Channel, New) precedent (Channel.new also returns
            // a runtime value via a zero-arg ctor).
            (T::Web, A::New) => {
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    buff_web::Web::new()
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Web.new codegen parse: {e}")))
            }
            // T17: Web.bind(addr) -> Web. One arg (String). Wraps
            // `buff_web::Web::bind(addr)` (infallible - returns an
            // empty Web with the bind addr preset; the user adds
            // routes via web.get / web.post / ... and starts serving
            // via web.run()).
            //
            // Same shared `Bind` variant as `UDP.bind(host, port)`
            // (T124m) - dispatched on (Web, Bind) pair (mirrors
            // `Parse` shared between DateTime / Date / Toml / URL /
            // UUID, `Connect` shared between TCP / WebSocket).
            (T::Web, A::Bind) => {
                let arg = one_arg(self)?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    buff_web::Web::bind(#arg)
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Web.bind codegen parse: {e}")))
            }
            // T33: HttpClient.new() -> HttpClient. Zero args. Wraps
            // `buff_http_client::HttpClient::new()` (infallible -
            // returns a new client with default settings). Records
            // `buff-http-client` + `reqwest` in extern_crates via the
            // `program_uses_namespace("HttpClient")` walker. Dispatch
            // on (PreludeType::HttpClient, New) - mirrors the (Web,
            // New) / (Channel, New) precedent.
            (T::HttpClient, A::New) => {
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    buff_http_client::HttpClient::new()
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("HttpClient.new codegen parse: {e}")))
            }
            // T29: Validator.new() -> Validator. Zero args. Wraps
            // `buff_validate::Validator::new()` (infallible - returns
            // an empty rule set). Records `buff-validate` +
            // `validator` + `serde_json` + `regex` in extern_crates
            // via the `program_uses_namespace("Validator")` walker.
            // Dispatch on (PreludeType::Validator, New) - mirrors the
            // (HttpClient, New) / (Channel, New) precedent.
            (T::Validator, A::New) => {
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    buff_validate::Validator::new()
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Validator.new codegen parse: {e}")))
            }
            // T42: Email.new(from, to, subject) -> Email. Three args
            // (String from, String to, String subject). Wraps
            // `buff_email::Email::new(&from, &to, &subject)?` (the
            // `?` propagates EmailError::InvalidAddress per Buff's
            // R3 error-mapping contract). Records `buff-email` +
            // `lettre` + `handlebars` in extern_crates via the
            // `program_uses_namespace("Email")` walker. Dispatch on
            // (PreludeType::Email, New) - mirrors the (Validator,
            // New) / (HttpClient, New) / (Cache, New) precedent.
            (T::Email, A::New) => {
                if args.len() != 3 {
                    return Err(self.unsupported(&format!(
                        "Email.new() expects exactly 3 args (from, to, subject), got {}",
                        args.len()
                    )));
                }
                let from = self.lower_expr(&args[0])?;
                let to = self.lower_expr(&args[1])?;
                let subject = self.lower_expr(&args[2])?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    buff_email::Email::new(&#from, &#to, &#subject)?
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Email.new codegen parse: {e}")))
            }
            // T42: SmtpClient.new(host, port, username, password) ->
            // SmtpClient. Four args (String host, Int port, String
            // username, String password). Wraps
            // `buff_email::SmtpClient::new(&host, port as u16, &user,
            // &pass)?` (the `?` propagates EmailError::InvalidRelay).
            // Records `buff-email` + `lettre` in extern_crates
            // (shared walker with Email). Dispatch on
            // (PreludeType::SmtpClient, New).
            (T::SmtpClient, A::New) => {
                if args.len() != 4 {
                    return Err(self.unsupported(&format!(
                        "SmtpClient.new() expects exactly 4 args (host, port, username, password), got {}",
                        args.len()
                    )));
                }
                let host = self.lower_expr(&args[0])?;
                let port = self.lower_expr(&args[1])?;
                let username = self.lower_expr(&args[2])?;
                let password = self.lower_expr(&args[3])?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    buff_email::SmtpClient::new(&#host, #port as u16, &#username, &#password)?
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("SmtpClient.new codegen parse: {e}")))
            }
            // T31: Cache.new(max_capacity) -> Cache. One arg (Int).
            // Wraps `buff_cache::Cache::new(max_capacity as u64)
            // .unwrap_or_default()` (panic-free on zero-capacity —
            // Cache impls Default as a 1024-capacity empty cache,
            // matching Buff's "no panicking generated code" rule).
            // Records `buff-cache` + `moka` in extern_crates via the
            // `program_uses_namespace("Cache")` walker.
            (T::Cache, A::New) => {
                let arg = one_arg(self)?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    buff_cache::Cache::new(#arg as u64).unwrap_or_default()
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Cache.new codegen parse: {e}")))
            }
            // T44: I18n.new(locale) -> I18n. One arg (String). Wraps
            // `buff_i18n::I18n::new(&locale).unwrap_or_default()`
            // (panic-free on invalid locale — I18n impls Default as
            // an empty English catalog, matching Buff's "no
            // panicking generated code" rule + the Image / Cache /
            // Document precedent). Records `buff-i18n` +
            // `fluent-bundle` + `unic-langid` in extern_crates via
            // the `program_uses_namespace("I18n")` walker.
            (T::I18n, A::New) => {
                let arg = one_arg(self)?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    buff_i18n::I18n::new(&#arg).unwrap_or_default()
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("I18n.new codegen parse: {e}")))
            }
            // T44: I18n.with_fallback(locale, fallback) -> I18n. Two
            // args (String locale, String fallback). Wraps
            // `buff_i18n::I18n::with_fallback(&locale, &fallback)
            // .unwrap_or_default()` (panic-free).
            (T::I18n, A::WithFallback) => {
                if args.len() != 2 {
                    return Err(self.unsupported(&format!(
                        "with_fallback() expects exactly 2 args (locale, fallback), got {}",
                        args.len()
                    )));
                }
                let locale = self.lower_expr(&args[0])?;
                let fallback = self.lower_expr(&args[1])?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    buff_i18n::I18n::with_fallback(&#locale, &#fallback).unwrap_or_default()
                };
                syn::parse2(tokens).map_err(|e| {
                    self.unsupported(&format!("I18n.with_fallback codegen parse: {e}"))
                })
            }
            // T43: Document.from_html(html) -> Document. One arg
            // (String). Wraps `buff_scrape::Document::from_html(&html)
            // .unwrap_or_default()` (panic-free on empty input —
            // Document impls Default as `<html></html>`, matching
            // Buff's "no panicking generated code" rule; mirrors the
            // Image.from_path `unwrap_or_default()` precedent). Records
            // `buff-scrape` + `scraper` in extern_crates via the
            // `program_uses_namespace("Document")` walker.
            (T::Document, A::FromHtml) => {
                let arg = one_arg(self)?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    buff_scrape::Document::from_html(&#arg).unwrap_or_default()
                };
                syn::parse2(tokens).map_err(|e| {
                    self.unsupported(&format!("Document.from_html codegen parse: {e}"))
                })
            }
            // T43: Crawler.new(seed_url) -> Crawler. One arg (String).
            // Wraps `buff_scrape::Crawler::new(&seed)
            // .unwrap_or_default()` (panic-free on empty seed —
            // Crawler impls Default as an about:blank-seeded client).
            // Records `buff-scrape` + `reqwest` in extern_crates via
            // the `program_uses_namespace("Crawler")` walker.
            (T::Crawler, A::New) => {
                let arg = one_arg(self)?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    buff_scrape::Crawler::new(&#arg).unwrap_or_default()
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Crawler.new codegen parse: {e}")))
            }
            // T30: Config module — namespace-only (no runtime value).
            // `Config.new()` creates a `buff_config::Config` and stores
            // it in a thread-local static so subsequent method calls
            // (`cfg.set_default`, `cfg.load_file`, etc.) operate on the
            // same instance. The codegen emits a lazy-static pattern
            // (one Config per thread — no Mutex contention for the
            // common single-threaded case). Mirrors the Log / Toml /
            // Math namespace-only pattern.
            (T::Config, A::New) => {
                no_args(self)?;
                let tokens: proc_macro2::TokenStream = quote::quote! {{
                    static CONFIG: std::sync::LazyLock<buff_config::Config> =
                        std::sync::LazyLock::new(buff_config::Config::new);
                    &*CONFIG
                }};
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Config.new codegen parse: {e}")))
            }
            // `Config.set_default(key, val)` -> Void. Two args.
            (T::Config, A::SetDefault) => {
                if args.len() != 2 {
                    return Err(self.unsupported(&format!(
                        "set_default() expects exactly 2 args (key, value), got {}",
                        args.len()
                    )));
                }
                let key = self.lower_expr(&args[0])?;
                let val = self.lower_expr(&args[1])?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    CONFIG.set_default(#key, #val)
                };
                syn::parse2(tokens).map_err(|e| {
                    self.unsupported(&format!("Config.set_default codegen parse: {e}"))
                })
            }
            // `Config.load_file(path)` -> Void. One arg.
            (T::Config, A::LoadFile) => {
                let arg = one_arg(self)?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    CONFIG.load_file(#arg).unwrap_or_default()
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Config.load_file codegen parse: {e}")))
            }
            // `Config.load_env(prefix)` -> Void. One arg.
            (T::Config, A::LoadEnv) => {
                let arg = one_arg(self)?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    CONFIG.load_env(#arg)
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Config.load_env codegen parse: {e}")))
            }
            // `Config.load_args(args)` -> Void. One arg (Vector<String>).
            (T::Config, A::LoadArgs) => {
                let arg = one_arg(self)?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    CONFIG.load_args(&#arg)
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Config.load_args codegen parse: {e}")))
            }
            // `Config.get(key)` -> Option<String>. One arg. Reuses the
            // shared `Get` variant (also used by Args.get / Env.get).
            (T::Config, A::Get) => {
                let arg = one_arg(self)?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    CONFIG.get(#arg)
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Config.get codegen parse: {e}")))
            }
            // `Config.get_int(key)` -> Option<Int>. One arg.
            (T::Config, A::GetInt) => {
                let arg = one_arg(self)?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    CONFIG.get_int(#arg)
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Config.get_int codegen parse: {e}")))
            }
            // `Config.get_float(key)` -> Option<Float>. One arg.
            (T::Config, A::GetFloat) => {
                let arg = one_arg(self)?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    CONFIG.get_float(#arg)
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Config.get_float codegen parse: {e}")))
            }
            // `Config.get_bool(key)` -> Option<Bool>. One arg.
            (T::Config, A::GetBool) => {
                let arg = one_arg(self)?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    CONFIG.get_bool(#arg)
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Config.get_bool codegen parse: {e}")))
            }
            // `Config.watch(path, callback)` -> Void. Two args.
            (T::Config, A::Watch) => {
                if args.len() != 2 {
                    return Err(self.unsupported(&format!(
                        "watch() expects exactly 2 args (path, callback), got {}",
                        args.len()
                    )));
                }
                let path = self.lower_expr(&args[0])?;
                let cb = self.lower_expr(&args[1])?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    CONFIG.watch(#path, #cb).unwrap_or_default()
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Config.watch codegen parse: {e}")))
            }
            // T34: buff-auth assoc fns. The 5 (type, method) pairs
            // below cover the MVP surface: JWT.encode / JWT.decode /
            // Password.hash / Password.verify / OAuth2Client.new /
            // Rbac.new. The instance-method forms
            // (client.authorization_url / client.exchange_code /
            // policy.add / policy.enforce) are deferred to the sibling
            // task that adds Type::OAuth2Client / Type::Rbac — mirrors
            // the T17 Web (web.get / web.listen) + T18 Database
            // (pool.query / pool.execute) forward-declaration
            // precedent. Records `buff-auth` + `jsonwebtoken` +
            // `argon2` + `oauth2` + `reqwest` in extern_crates via the
            // shared `program_uses_namespace("JWT"|"OAuth2Client"|
            // "Password"|"Rbac")` walker.
            //
            // JWT.encode(claims, secret) -> String. Two args
            // (Map<String, Unknown>, String). Wraps
            // `buff_auth::jwt_encode(&claims_obj, &secret)
            // .unwrap_or_default()` (panic-free — empty String on
            // encode failure, NEVER panics). The codegen serialises
            // the Buff Map<String, Unknown> arg to a serde_json Value
            // and extracts the inner Map<String, Value> via
            // `.as_object().cloned()` (the Buff Map<String, Unknown>
            // lowers to a `std::collections::HashMap<String, ?>`
            // whose serde_json serialisation round-trips through
            // Map<String, Value>).
            (T::Jwt, A::Encode) => {
                if args.len() != 2 {
                    return Err(self.unsupported(&format!(
                        "JWT.encode() expects exactly 2 args (claims, secret), got {}",
                        args.len()
                    )));
                }
                let claims = self.lower_expr(&args[0])?;
                let secret = self.lower_expr(&args[1])?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    buff_auth::jwt_encode(
                        &serde_json::to_value(&#claims)
                            .ok()
                            .and_then(|v| v.as_object().cloned())
                            .unwrap_or_default(),
                        &#secret,
                    ).unwrap_or_default()
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("JWT.encode codegen parse: {e}")))
            }
            // JWT.decode(token, secret) -> Map<String, Unknown>. Two
            // args (String, String). Wraps
            // `buff_auth::jwt_decode(&token, &secret).unwrap_or_default()`
            // (panic-free — invalid signature / malformed token /
            // expired all collapse to an empty Map, NEVER panics).
            (T::Jwt, A::Decode) => {
                let (token, secret) = two_args(self)?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    buff_auth::jwt_decode(#token, #secret)
                        .unwrap_or_default()
                        .into_iter().collect()
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("JWT.decode codegen parse: {e}")))
            }
            // Password.hash(plain) -> String. One arg (String). Wraps
            // `buff_auth::password_hash(plain).unwrap_or_default()`
            // (panic-free — empty String on hash failure, NEVER
            // panics).
            (T::Password, A::PasswordHash) => {
                let arg = one_arg(self)?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    buff_auth::password_hash(#arg).unwrap_or_default()
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Password.hash codegen parse: {e}")))
            }
            // Password.verify(plain, phc_hash) -> Bool. Two args
            // (String, String). Wraps
            // `buff_auth::password_verify(plain, hash).unwrap_or(false)`
            // (panic-free — false on mismatch or hash-format failure,
            // NEVER panics). Mirrors the T26 Signature.verify lowering
            // stance: verification failure is Ok(false), NOT an error.
            (T::Password, A::PasswordVerify) => {
                let (plain, hash) = two_args(self)?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    buff_auth::password_verify(#plain, #hash).unwrap_or(false)
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Password.verify codegen parse: {e}")))
            }
            // T39: Archive.compress_dir(input_dir, output_path) -> Void.
            // Two args. Wraps `buff_archive::Archive::compress_dir(
            // input_dir, output_path, buff_archive::Format::from_path(
            // std::path::Path::new(&output_path)).unwrap_or(
            // buff_archive::Format::Zip))?` (the `?` propagates
            // `ArchiveError` per Buff's R3 error-mapping contract; the
            // format is auto-detected from the output_path extension —
            // `.zip` → Zip, `.tar.gz` → Gz, `.tar.zst` → Zstd, etc.,
            // matching the cross-language convention of `tar -czf
            // x.tar.gz src/`). Records `buff-archive` + `zip` + `tar`
            // + `flate2` + `ruzstd` in extern_crates via the
            // `program_uses_namespace("Archive")` walker.
            (T::Archive, A::CompressDir) => {
                let (input_dir, output_path) = two_args(self)?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    buff_archive::Archive::compress_dir(
                        #input_dir,
                        #output_path,
                        buff_archive::Format::from_path(std::path::Path::new(&#output_path))
                            .unwrap_or(buff_archive::Format::Zip),
                    )?
                };
                syn::parse2(tokens).map_err(|e| {
                    self.unsupported(&format!("Archive.compress_dir codegen parse: {e}"))
                })
            }
            // T39: Archive.extract(archive_path, output_dir) -> Void.
            // Two args. Wraps `buff_archive::Archive::extract(
            // archive_path, output_dir)?` (the format is auto-detected
            // from the file's extension inside the wrapper). Records
            // `buff-archive` + `zip` + `tar` + `flate2` + `ruzstd` in
            // extern_crates via the `program_uses_namespace("Archive")`
            // walker.
            (T::Archive, A::Extract) => {
                let (archive_path, output_dir) = two_args(self)?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    buff_archive::Archive::extract(#archive_path, #output_dir)?
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Archive.extract codegen parse: {e}")))
            }
            // T51: MsgPack.serialize(value) -> Vector<Byte>. Wraps
            // `buff_msgpack::serialize(&value).unwrap_or_default()`
            // (empty Vec on serialize failure — NEVER panics, matching
            // Buff's "no panicking generated code" rule). Records
            // `buff-msgpack` + `rmp-serde` + `serde_json` in
            // extern_crates via the
            // `program_uses_namespace("MsgPack")` walker.
            (T::MsgPack, A::Serialize) => {
                let arg = one_arg(self)?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    buff_msgpack::serialize(&#arg).unwrap_or_default()
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("MsgPack.serialize codegen parse: {e}")))
            }
            // T51: MsgPack.deserialize(bytes) -> Value. Wraps
            // `buff_msgpack::deserialize(&bytes).unwrap_or_default()`
            // (returns `serde_json::Value::Null` on deserialize failure
            // — `Value` impls `Default`; NEVER panics). The arg is a
            // `Vector<Byte>` on the Buff surface (Vec<u8> after
            // codegen lowering); the codegen passes it by ref.
            (T::MsgPack, A::Deserialize) => {
                let arg = one_arg(self)?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    buff_msgpack::deserialize(&#arg).unwrap_or_default()
                };
                syn::parse2(tokens).map_err(|e| {
                    self.unsupported(&format!("MsgPack.deserialize codegen parse: {e}"))
                })
            }
            // T51: MsgPack.roundtrip(value) -> Option<Value>. Wraps
            // `buff_msgpack::roundtrip(&value)` directly — the
            // runtime fn already returns `Option<serde_json::Value>`
            // (None on either step failing). NEVER panics.
            (T::MsgPack, A::Roundtrip) => {
                let arg = one_arg(self)?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    buff_msgpack::roundtrip(&#arg)
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("MsgPack.roundtrip codegen parse: {e}")))
            }
            // T52: Protobuf.serialize(value) -> Vector<Byte>. Wraps
            // `buff_protobuf::serialize(&value).unwrap_or_default()`
            // (empty Vec on serialize failure — NEVER panics, matching
            // Buff's "no panicking generated code" rule). Records
            // `buff-protobuf` + `prost` + `prost-types` + `serde_json`
            // in extern_crates via the
            // `program_uses_namespace("Protobuf")` /
            // `program_uses_namespace("Message")` walker. Mirrors T51
            // MsgPack.serialize 1:1.
            (T::Protobuf, A::Serialize) => {
                let arg = one_arg(self)?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    buff_protobuf::serialize(&#arg).unwrap_or_default()
                };
                syn::parse2(tokens).map_err(|e| {
                    self.unsupported(&format!("Protobuf.serialize codegen parse: {e}"))
                })
            }
            // T52: Protobuf.deserialize(bytes) -> Value. Wraps
            // `buff_protobuf::deserialize(&bytes).unwrap_or_default()`
            // (returns `serde_json::Value::Null` on deserialize failure
            // — `Value` impls `Default`; NEVER panics). The arg is a
            // `Vector<Byte>` on the Buff surface (Vec<u8> after
            // codegen lowering); the codegen passes it by ref. Mirrors
            // T51 MsgPack.deserialize 1:1.
            (T::Protobuf, A::Deserialize) => {
                let arg = one_arg(self)?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    buff_protobuf::deserialize(&#arg).unwrap_or_default()
                };
                syn::parse2(tokens).map_err(|e| {
                    self.unsupported(&format!("Protobuf.deserialize codegen parse: {e}"))
                })
            }
            // T52: Protobuf.roundtrip(value) -> Option<Value>. Wraps
            // `buff_protobuf::roundtrip(&value)` directly — the
            // runtime fn already returns `Option<serde_json::Value>`
            // (None on either step failing). NEVER panics. Mirrors T51
            // MsgPack.roundtrip 1:1.
            (T::Protobuf, A::Roundtrip) => {
                let arg = one_arg(self)?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    buff_protobuf::roundtrip(&#arg)
                };
                syn::parse2(tokens).map_err(|e| {
                    self.unsupported(&format!("Protobuf.roundtrip codegen parse: {e}"))
                })
            }
            // T52: Message.new(value) -> Message. One arg (the value
            // to encode). Wraps `buff_protobuf::Message::new(&value)
            // .unwrap_or_default()` (panic-free on encode failure —
            // Message impls Default as an empty-payload message).
            // `New` is shared with Channel.new / Faker.new /
            // Crawler.new / Point.new / XmlElement.new — dispatched
            // on the (Message, New) pair. Records `buff-protobuf` +
            // `prost` + `prost-types` + `serde_json` in extern_crates
            // via the `program_uses_namespace("Message")` walker.
            (T::Message, A::New) => {
                let arg = one_arg(self)?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    buff_protobuf::Message::new(&#arg).unwrap_or_default()
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Message.new codegen parse: {e}")))
            }
            // T52: Message.from_bytes(bytes) -> Message. One arg
            // (Vector<Byte>). Wraps
            // `buff_protobuf::Message::from_bytes(bytes)
            // .unwrap_or_default()` (panic-free on decode failure /
            // empty buffer — Message impls Default). `FromBytes` is
            // shared with Image.from_bytes — dispatched on the
            // (Message, FromBytes) pair.
            (T::Message, A::FromBytes) => {
                let arg = one_arg(self)?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    buff_protobuf::Message::from_bytes(#arg).unwrap_or_default()
                };
                syn::parse2(tokens).map_err(|e| {
                    self.unsupported(&format!("Message.from_bytes codegen parse: {e}"))
                })
            }
            // T52: Message.decode(bytes) -> Message. Class-method
            // alias for Message.from_bytes — same lowering shape but
            // takes a `&[u8]` (Bytes ref) instead of `Vec<u8>` (owned
            // bytes). The codegen splices the arg directly (Buff's
            // `Vector<Byte>` lowers to `Vec<u8>` which deref-coerces
            // to `&[u8]` for the underlying `Message::decode(&[u8])`
            // Rust signature). `Decode` is shared with Base64.decode /
            // Hex.decode / URLEncode.decode — dispatched on the
            // (Message, Decode) pair.
            (T::Message, A::Decode) => {
                let arg = one_arg(self)?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    buff_protobuf::Message::decode(&#arg).unwrap_or_default()
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Message.decode codegen parse: {e}")))
            }
            // T47: Bot.new(platform, token) -> Bot. Two args (Platform,
            // String). Wraps `buff_chat::Bot::new(platform, token)
            // .unwrap_or_default()` (panic-free on construction
            // failure — Bot impls Default as an empty Discord bot,
            // added in the T47 MVP commit). `New` is shared with
            // Channel.new / Faker.new / Point.new / XmlElement.new /
            // Message.new (T52) — dispatched on the (Bot, New) pair.
            // Records `buff-chat` + `serenity` + `teloxide` +
            // `async-trait` + `tokio` in extern_crates via the
            // `program_uses_namespace("Bot")` walker.
            (T::Bot, A::New) => {
                if args.len() != 2 {
                    return Err(self.unsupported(&format!(
                        "Bot.new expects exactly 2 args (platform, token), got {}",
                        args.len()
                    )));
                }
                let platform = self.lower_expr(&args[0])?;
                let token = self.lower_expr(&args[1])?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    buff_chat::Bot::new(#platform, (#token).to_string()).unwrap_or_default()
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Bot.new codegen parse: {e}")))
            }
            // T47: ChatMessage.new(text, channel, author, platform,
            // is_dm) -> ChatMessage. Five args (String, String,
            // String, Platform, Bool). Wraps
            // `buff_chat::Message::new(text, channel, author,
            // platform, is_dm)` directly (infallible — the underlying
            // Message::new ctor has no failure mode). The token-
            // coercion `(#x).to_string()` on each String arg handles
            // Buff's `&str`-from-literal lowering (the codegen inserts
            // the coercion so the user can pass either String or &str
            // — the wrapper ctor takes owned `String` per FFI guide
            // R5). Records `buff-chat` + `serenity` + `teloxide` +
            // `async-trait` + `tokio` in extern_crates via the
            // `program_uses_namespace("ChatMessage")` walker.
            (T::ChatMessage, A::New) => {
                if args.len() != 5 {
                    return Err(self.unsupported(&format!(
                        "ChatMessage.new expects exactly 5 args (text, channel, author, platform, is_dm), got {}",
                        args.len()
                    )));
                }
                let text = self.lower_expr(&args[0])?;
                let channel = self.lower_expr(&args[1])?;
                let author = self.lower_expr(&args[2])?;
                let platform = self.lower_expr(&args[3])?;
                let is_dm = self.lower_expr(&args[4])?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    buff_chat::Message::new(
                        (#text).to_string(),
                        (#channel).to_string(),
                        (#author).to_string(),
                        #platform,
                        #is_dm,
                    )
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("ChatMessage.new codegen parse: {e}")))
            }
            // T50: Xml.from_str(xml) -> XmlDocument. One arg (String).
            // Wraps `buff_xml::XmlDocument::from_str(&xml)
            // .unwrap_or_default()` (panic-free on empty/parse failure —
            // XmlDocument impls Default as a root-only document). Records
            // `buff-xml` + `quick-xml` in extern_crates via the
            // `program_uses_namespace("Xml")` walker.
            (T::Xml, A::FromStr) => {
                let arg = one_arg(self)?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    buff_xml::XmlDocument::from_str(&#arg).unwrap_or_default()
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Xml.from_str codegen parse: {e}")))
            }
            // T50: XmlElement.new(name, text, attrs) -> XmlElement.
            // Three args (String, String, Map<String,String>). Wraps
            // `buff_xml::XmlElement::new(&name, &text, attrs_vec)`
            // where `attrs_vec` is built by `.into_iter().map(|(k, v)|
            // (k.to_string(), v.to_string())).collect::<Vec<(String,
            // String)>>()` — the conversion accepts any IntoIterator
            // yielding string-like tuples (Buff Map literal codegens
            // to `HashMap<&str, &str>`; user-passed `HashMap<String,
            // String>` / `Vec<(String, String)>` also work). Infallible
            // (the wrapper ctor never fails — no `?` / unwrap_or_default
            // needed). Records `buff-xml` + `quick-xml` in extern_crates
            // via the `program_uses_namespace("XmlElement")` walker.
            (T::XmlElement, A::New) => {
                if args.len() != 3 {
                    return Err(self.unsupported(&format!(
                        "XmlElement.new expects exactly 3 args (name, text, attrs), got {}",
                        args.len()
                    )));
                }
                let name = self.lower_expr(&args[0])?;
                let text = self.lower_expr(&args[1])?;
                let attrs = self.lower_expr(&args[2])?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    buff_xml::XmlElement::new(
                        &(#name).to_string(),
                        &(#text).to_string(),
                        (#attrs)
                            .into_iter()
                            .map(|(k, v)| (
                                std::string::ToString::to_string(&k),
                                std::string::ToString::to_string(&v)
                            ))
                            .collect::<Vec<(String, String)>>(),
                    )
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("XmlElement.new codegen parse: {e}")))
            }
            // T45: Point.new(x, y) -> Point. Two args (Float, Float).
            // Wraps `buff_geo::Point::new(x, y)` (infallible — the
            // underlying geo_types::Point::new never fails). Records
            // `buff-geo` + `geo` + `geo-types` in extern_crates via the
            // `program_uses_namespace("Point")` walker.
            (T::Point, A::New) => {
                if args.len() != 2 {
                    return Err(self.unsupported(&format!(
                        "Point.new expects exactly 2 args (x, y), got {}",
                        args.len()
                    )));
                }
                let x = self.lower_expr(&args[0])?;
                let y = self.lower_expr(&args[1])?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    buff_geo::Point::new(#x, #y)
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Point.new codegen parse: {e}")))
            }
            // T45: LineString.new(points) -> LineString. One arg
            // (Vector<Point>). Wraps
            // `buff_geo::LineString::new(points).unwrap_or_default()`
            // (panic-free on empty input — LineString impls Default).
            (T::LineString, A::New) => {
                let arg = one_arg(self)?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    buff_geo::LineString::new(#arg).unwrap_or_default()
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("LineString.new codegen parse: {e}")))
            }
            // T45: LineString.from_coords(flat) -> LineString. One arg
            // (Vector<Float>). Wraps
            // `buff_geo::LineString::from_coords(coords).unwrap_or_default()`
            // (panic-free on empty / odd-length input).
            (T::LineString, A::FromCoords) => {
                let arg = one_arg(self)?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    buff_geo::LineString::from_coords(#arg).unwrap_or_default()
                };
                syn::parse2(tokens).map_err(|e| {
                    self.unsupported(&format!("LineString.from_coords codegen parse: {e}"))
                })
            }
            // T45: Polygon.new(ring) -> Polygon. One arg (Vector<Point>).
            // Wraps `buff_geo::Polygon::new(ring).unwrap_or_default()`
            // (panic-free on degenerate input — Polygon impls Default).
            (T::Polygon, A::New) => {
                let arg = one_arg(self)?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    buff_geo::Polygon::new(#arg).unwrap_or_default()
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Polygon.new codegen parse: {e}")))
            }
            // T45: Polygon.from_coords(flat) -> Polygon. One arg
            // (Vector<Float>). Wraps
            // `buff_geo::Polygon::from_coords(coords).unwrap_or_default()`
            // (panic-free on empty / odd-length / degenerate input).
            (T::Polygon, A::FromCoords) => {
                let arg = one_arg(self)?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    buff_geo::Polygon::from_coords(#arg).unwrap_or_default()
                };
                syn::parse2(tokens).map_err(|e| {
                    self.unsupported(&format!("Polygon.from_coords codegen parse: {e}"))
                })
            }
            // T54: buff-simd constructors. Each lowers to the matching
            // `buff_simd::Simd::*` associated function. `Simd.splat(x)`
            // and `Simd.from_array(arr)` are infallible (wrap the
            // underlying `wide::f32x4` ctors directly). `Simd.from_slice
            // (slice)` is fallible in Rust (returns
            // `Result<_, SimdError>`) but surfaces as infallible on the
            // Buff side via `.unwrap_or_default()` (Simd impls Default
            // as `splat(0.0)`). Records `buff-simd` + `wide` in
            // extern_crates via the `program_uses_namespace("Simd")`
            // walker.
            //
            // `Simd.splat(x)` -> Simd. One arg (Float). Wraps
            // `buff_simd::Simd::splat(x)`.
            (T::Simd, A::Splat) => {
                if args.len() != 1 {
                    return Err(self.unsupported(&format!(
                        "Simd.splat expects exactly 1 arg (x: Float), got {}",
                        args.len()
                    )));
                }
                let x = self.lower_expr(&args[0])?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    buff_simd::Simd::splat(#x as f32)
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Simd.splat codegen parse: {e}")))
            }
            // `Simd.from_slice(slice)` -> Simd. One arg (Vector<Float>).
            // Wraps
            // `buff_simd::Simd::from_slice(&slice).unwrap_or_default()`
            // (panic-free on too-short / non-finite input).
            (T::Simd, A::FromSlice) => {
                let arg = one_arg(self)?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    buff_simd::Simd::from_slice(&#arg).unwrap_or_default()
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Simd.from_slice codegen parse: {e}")))
            }
            // `Simd.from_array(arr)` -> Simd. One arg (Vector<Float>).
            // Wraps `buff_simd::Simd::from_array(...)` via the slice
            // path (the wrapper accepts a slice; codegen passes the
            // array as a slice reference). Infallible via
            // `.unwrap_or_default()`.
            (T::Simd, A::FromArray) => {
                let arg = one_arg(self)?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    buff_simd::Simd::from_slice(&#arg).unwrap_or_default()
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Simd.from_array codegen parse: {e}")))
            }
            // T46: Text.detect_language(text) -> Option<Language>. One
            // arg (String). Wraps `buff_nlp::Text::detect_language(&text)`
            // directly — the wrapper already returns Option<Language>
            // (None on empty input / detection failure). NEVER panics
            // (the wrapper uses catch_unwind per FFI guide R6). Records
            // `buff-nlp` + `whatlang` + `rust-stemmers` +
            // `unicode-segmentation` in extern_crates via the
            // `program_uses_namespace("Text")` walker.
            (T::Text, A::DetectLanguage) => {
                let arg = one_arg(self)?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    buff_nlp::Text::detect_language(&#arg)
                };
                syn::parse2(tokens).map_err(|e| {
                    self.unsupported(&format!("Text.detect_language codegen parse: {e}"))
                })
            }
            // T46: Text.stem(word, algorithm) -> String. Two args
            // (String word, String algorithm — lowercase Snowball name
            // like "english" / "portuguese"). Wraps
            // `buff_nlp::Text::stem(&word,
            // buff_nlp::StemAlgorithm::from_code(&algorithm)
            // .unwrap_or(buff_nlp::StemAlgorithm::English))?` — the
            // `?` propagates `NlpError` per Buff's R3 error-mapping
            // contract; unknown algorithm names fall back to English
            // (defensive, never silently corrupts). The wrapper uses
            // catch_unwind per FFI guide R6.
            (T::Text, A::Stem) => {
                if args.len() != 2 {
                    return Err(self.unsupported(&format!(
                        "Text.stem expects exactly 2 args (word, algorithm), got {}",
                        args.len()
                    )));
                }
                let word = self.lower_expr(&args[0])?;
                let algorithm = self.lower_expr(&args[1])?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    buff_nlp::Text::stem(
                        &#word,
                        buff_nlp::StemAlgorithm::from_code(&#algorithm)
                            .unwrap_or(buff_nlp::StemAlgorithm::English),
                    )?
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Text.stem codegen parse: {e}")))
            }
            // T46: Text.tokenize(text) -> Vector<String>. One arg
            // (String). Wraps `buff_nlp::Text::tokenize(&text)` —
            // pure iterator over UAX #29 word boundaries (no panic
            // vectors; catch_unwind omitted per the lib.rs note).
            (T::Text, A::Tokenize) => {
                let arg = one_arg(self)?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    buff_nlp::Text::tokenize(&#arg)
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Text.tokenize codegen parse: {e}")))
            }
            // T46: Text.sentences(text) -> Vector<String>. One arg
            // (String). Wraps `buff_nlp::Text::sentences(&text)` —
            // pure iterator over UAX #29 sentence boundaries.
            (T::Text, A::Sentences) => {
                let arg = one_arg(self)?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    buff_nlp::Text::sentences(&#arg)
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Text.sentences codegen parse: {e}")))
            }
            // T48: Provider.new(rpc_url) -> Provider. One arg (String).
            // Wraps `buff_web3::Provider::new(&url).unwrap_or_default()`
            // (panic-free — Provider impls Default as a localhost-pointed
            // no-op provider; the codegen-lowered `.unwrap_or_default()`
            // collapses `Web3Error::InvalidUrl` / `Web3Error::Panic` to
            // the default Provider per Buff's "no panicking generated
            // code" rule). Records `buff-web3` + `ethers` + `tokio` +
            // `reqwest` + `serde_json` + `hex` in extern_crates via the
            // `program_uses_namespace("Provider")` walker.
            (T::Provider, A::New) => {
                let arg = one_arg(self)?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    buff_web3::Provider::new(&#arg).unwrap_or_default()
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Provider.new codegen parse: {e}")))
            }
            // T48: Wallet.from_private_key(key) -> Wallet. One arg
            // (String — accepts `0x`-prefixed or bare 64-char hex).
            // Wraps `buff_web3::Wallet::from_private_key(&key)
            // .unwrap_or_default()` (panic-free — Wallet impls Default
            // as a "burner" wallet derived from a fixed test key,
            // NEVER use on mainnet; the codegen-lowered
            // `.unwrap_or_default()` collapses
            // `Web3Error::InvalidPrivateKey` / `Web3Error::Panic` to
            // the default Wallet). Records `buff-web3` + `ethers` +
            // `tokio` + `reqwest` + `serde_json` + `hex` in
            // extern_crates via the `program_uses_namespace("Wallet")`
            // walker.
            (T::Wallet, A::FromPrivateKey) => {
                let arg = one_arg(self)?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    buff_web3::Wallet::from_private_key(&#arg).unwrap_or_default()
                };
                syn::parse2(tokens).map_err(|e| {
                    self.unsupported(&format!("Wallet.from_private_key codegen parse: {e}"))
                })
            }
            // T48: Contract.new(address, abi_json, client) -> Contract.
            // Three args (String address, String abi JSON,
            // Provider|ConnectedWallet client). Wraps
            // `buff_web3::Contract::new(&addr, &abi, #client)
            // .unwrap_or_default()` (panic-free — Contract impls
            // Default as a zero-address + empty-ABI + read-only
            // contract; the codegen-lowered `.unwrap_or_default()`
            // collapses `Web3Error::InvalidAddress` /
            // `Web3Error::InvalidAbi` / `Web3Error::Panic` to the
            // default Contract). The `client` arg is spliced directly
            // — the buff_web3 `IntoClient` trait accepts both Provider
            // (read-only) and ConnectedWallet (signing). Records
            // `buff-web3` + `ethers` + `tokio` + `reqwest` +
            // `serde_json` + `hex` in extern_crates via the
            // `program_uses_namespace("Contract")` walker.
            (T::Contract, A::New) => {
                if args.len() != 3 {
                    return Err(self.unsupported(&format!(
                        "Contract.new expects exactly 3 args (address, abi_json, client), got {}",
                        args.len()
                    )));
                }
                let address = self.lower_expr(&args[0])?;
                let abi = self.lower_expr(&args[1])?;
                let client = self.lower_expr(&args[2])?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    buff_web3::Contract::new(&#address, &#abi, #client).unwrap_or_default()
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Contract.new codegen parse: {e}")))
            }
            // T49: AES.generate_key() -> Vector<Byte>. Zero args.
            // Wraps `buff_crypto_extras::aes_gcm_api::generate_key()`
            // (infallible — returns Vec<u8> directly via
            // `Aes256Gcm::generate_key(&mut OsRng)`; OsRng::fill_bytes
            // is infallible on all platforms Buff supports). Records
            // `buff-crypto-extras` + 8 RustCrypto crates in
            // extern_crates via the
            // `program_uses_namespace("AES")` walker.
            (T::AES, A::GenerateKey) => {
                no_args(self)?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    buff_crypto_extras::aes_gcm_api::generate_key()
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("AES.generate_key codegen parse: {e}")))
            }
            // T49: AES.generate_nonce() -> Vector<Byte>. Zero args.
            // Wraps `buff_crypto_extras::aes_gcm_api::generate_nonce()`
            // (infallible — returns the 12-byte GCM nonce via
            // `Aes256Gcm::generate_nonce(&mut OsRng)`).
            (T::AES, A::GenerateNonce) => {
                no_args(self)?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    buff_crypto_extras::aes_gcm_api::generate_nonce()
                };
                syn::parse2(tokens).map_err(|e| {
                    self.unsupported(&format!("AES.generate_nonce codegen parse: {e}"))
                })
            }
            // T49: AES.encrypt(key, nonce, plaintext) -> Vector<Byte>.
            // Three args. Wraps `buff_crypto_extras::aes_gcm_api::
            // encrypt(&key, &nonce, &plaintext).unwrap_or_default()`
            // (empty Vec on any failure — wrong key/nonce length,
            // AES engine error, panic — NEVER panics, matching
            // Buff's "no panicking generated code" rule). The args
            // are spliced by reference so the underlying `&[u8]`
            // bounds are satisfied for both Vec<u8> and slices.
            (T::AES, A::Encrypt) => {
                let mut lowered = n_args(self, 3)?;
                let key = lowered.remove(0);
                let nonce = lowered.remove(0);
                let plaintext = lowered.remove(0);
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    buff_crypto_extras::aes_gcm_api::encrypt(#key.as_slice(), #nonce.as_slice(), #plaintext.as_slice()).unwrap_or_default()
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("AES.encrypt codegen parse: {e}")))
            }
            // T49: AES.decrypt(key, nonce, ciphertext) -> Vector<Byte>.
            // Three args. Wraps `buff_crypto_extras::aes_gcm_api::
            // decrypt(&key, &nonce, &ciphertext).unwrap_or_default()`
            // (empty Vec on auth-tag mismatch / wrong key / wrong
            // nonce length / panic — NEVER panics). Same shape as
            // AES.encrypt.
            (T::AES, A::Decrypt) => {
                let mut lowered = n_args(self, 3)?;
                let key = lowered.remove(0);
                let nonce = lowered.remove(0);
                let ciphertext = lowered.remove(0);
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    buff_crypto_extras::aes_gcm_api::decrypt(#key.as_slice(), #nonce.as_slice(), #ciphertext.as_slice()).unwrap_or_default()
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("AES.decrypt codegen parse: {e}")))
            }
            // T49: RSA.generate_keypair(bits) -> RsaKeypair. One arg
            // (Int). Wraps `buff_crypto_extras::rsa_api::generate_keypair
            // (bits as usize).unwrap_or_default()` (panic-free — the
            // wrapper crate's RsaKeypair impls Default as the
            // empty-PEM-string fallback; the codegen-lowered
            // `.unwrap_or_default()` collapses CryptoError::
            // InvalidLength / Panic to the default RsaKeypair per
            // Buff's "no panicking generated code" rule). The `as
            // usize` lifts Buff's Int<64> to the usize Rust expects.
            // Computationally expensive (~100ms for 2048-bit, ~1s
            // for 4096-bit).
            (T::RSA, A::GenerateKeypair) => {
                let arg = one_arg(self)?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    buff_crypto_extras::rsa_api::generate_keypair(#arg as usize).unwrap_or_default()
                };
                syn::parse2(tokens).map_err(|e| {
                    self.unsupported(&format!("RSA.generate_keypair codegen parse: {e}"))
                })
            }
            // T49: RSA.sign(private_pem, data) -> Vector<Byte>. Two
            // args (String, Vector<Byte>). Wraps
            // `buff_crypto_extras::rsa_api::sign(&private_pem,
            // data.as_slice()).unwrap_or_default()` (empty Vec on
            // malformed PEM / sign engine failure / panic — NEVER
            // panics; the empty-Vec fallback is the correct
            // user-facing behavior since RSA.verify will return
            // false for any non-matching signature, including an
            // empty one).
            (T::RSA, A::Sign) => {
                let (private_pem, data) = two_args(self)?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    buff_crypto_extras::rsa_api::sign(&#private_pem, #data.as_slice()).unwrap_or_default()
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("RSA.sign codegen parse: {e}")))
            }
            // T49: RSA.verify(public_pem, data, signature) -> Bool.
            // Three args. Wraps `buff_crypto_extras::rsa_api::verify(
            // &public_pem, data.as_slice(), signature.as_slice())`
            // (the wrapper already returns `bool` — false on any
            // failure: signature mismatch, malformed PEM, invalid
            // signature bytes, or panic — mirrors T26 Signature.
            // verify + T34 Password.verify stance so a future
            // verify_allow policy can layer cleanly). NO
            // `.unwrap_or_default()` needed (the wrapper collapses
            // all failures to `false` itself).
            (T::RSA, A::Verify) => {
                let mut lowered = n_args(self, 3)?;
                let public_pem = lowered.remove(0);
                let data = lowered.remove(0);
                let signature = lowered.remove(0);
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    buff_crypto_extras::rsa_api::verify(&#public_pem, #data.as_slice(), #signature.as_slice())
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("RSA.verify codegen parse: {e}")))
            }
            // T49: ECDH.generate_private() -> Vector<Byte>. Zero
            // args. Wraps `buff_crypto_extras::ecdh_api::
            // p256_generate_private()` (infallible — returns the
            // 32-byte P-256 scalar via `P256Secret::random(&mut
            // OsRng)`).
            (T::ECDH, A::GeneratePrivate) => {
                no_args(self)?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    buff_crypto_extras::ecdh_api::p256_generate_private()
                };
                syn::parse2(tokens).map_err(|e| {
                    self.unsupported(&format!("ECDH.generate_private codegen parse: {e}"))
                })
            }
            // T49: ECDH.public_from_private(private) -> Vector<Byte>.
            // One arg (Vector<Byte>). Wraps
            // `buff_crypto_extras::ecdh_api::p256_public_from_private
            // (private.as_slice()).unwrap_or_default()` (empty Vec
            // on wrong length / invalid scalar / panic — NEVER
            // panics).
            (T::ECDH, A::PublicFromPrivate) => {
                let arg = one_arg(self)?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    buff_crypto_extras::ecdh_api::p256_public_from_private(#arg.as_slice()).unwrap_or_default()
                };
                syn::parse2(tokens).map_err(|e| {
                    self.unsupported(&format!("ECDH.public_from_private codegen parse: {e}"))
                })
            }
            // T49: ECDH.derive_shared(private, public) ->
            // Vector<Byte>. Two args. Wraps
            // `buff_crypto_extras::ecdh_api::p256_derive_shared(
            // private.as_slice(), public.as_slice())
            // .unwrap_or_default()` (empty Vec on wrong length /
            // invalid point / cofactor edge case / panic — NEVER
            // panics).
            (T::ECDH, A::DeriveShared) => {
                let (private, public) = two_args(self)?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    buff_crypto_extras::ecdh_api::p256_derive_shared(#private.as_slice(), #public.as_slice()).unwrap_or_default()
                };
                syn::parse2(tokens).map_err(|e| {
                    self.unsupported(&format!("ECDH.derive_shared codegen parse: {e}"))
                })
            }
            // T49: Argon2.generate_salt() -> Vector<Byte>. Zero
            // args. Wraps `buff_crypto_extras::argon2_api::
            // generate_salt()` (infallible — fills a 16-byte Vec
            // via `rand::rng().fill_bytes`).
            (T::Argon2, A::GenerateSalt) => {
                no_args(self)?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    buff_crypto_extras::argon2_api::generate_salt()
                };
                syn::parse2(tokens).map_err(|e| {
                    self.unsupported(&format!("Argon2.generate_salt codegen parse: {e}"))
                })
            }
            // T49: Argon2.derive_key(password, salt) -> Vector<Byte>.
            // Two args (String, Vector<Byte>). Wraps
            // `buff_crypto_extras::argon2_api::derive_key(&password,
            // salt.as_slice()).unwrap_or_default()` (empty Vec on
            // wrong salt length / Argon2 engine failure / panic —
            // NEVER panics).
            (T::Argon2, A::DeriveKey) => {
                let (password, salt) = two_args(self)?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    buff_crypto_extras::argon2_api::derive_key(&#password, #salt.as_slice()).unwrap_or_default()
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Argon2.derive_key codegen parse: {e}")))
            }
            // T89: Decimal constructors.
            // `Decimal.new(str)` -> Decimal. One arg (String). Wraps
            // `rust_decimal::Decimal::from_str(&s).unwrap_or_default()`
            // (panic-free — invalid input collapses to Decimal::ZERO).
            (T::Decimal, A::New) => {
                let s = one_arg(self)?;
                let s_ref = coerce_str_arg_to_ref(s, &args[0]);
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    rust_decimal::Decimal::from_str(#s_ref).unwrap_or_default()
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Decimal.new codegen parse: {e}")))
            }
            // `Decimal.from_float(f)` -> Decimal. One arg (Float). Wraps
            // `rust_decimal::Decimal::from_f64(f).unwrap_or_default()`
            // (panic-free — NaN/Inf collapse to Decimal::ZERO).
            (T::Decimal, A::FromFloat) => {
                let f = one_arg(self)?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    rust_decimal::Decimal::from_f64(#f).unwrap_or_default()
                };
                syn::parse2(tokens).map_err(|e| {
                    self.unsupported(&format!("Decimal.from_float codegen parse: {e}"))
                })
            }
            // Every other combination was already rejected by
            // `assoc_fn_lookup` in the caller; this arm is unreachable but
            // required for exhaustiveness.
            _ => Err(self.unsupported(&format!(
                "prelude type+method combination {:?}.{:?}",
                ptype, pmethod
            ))),
        }
    }
}

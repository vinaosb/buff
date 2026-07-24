//! T105a - type mapping: ast_typeref_to_syn + buff_type_to_syn (mechanically extracted from rust_codegen.rs).
//!
//! Verbatim move of `impl RustCodegen` methods into this child module so the
//! parent file shrinks. Methods are pub(super); the parent declares only
//! `mod <name>;` (inherent methods resolve by type, no `use` needed). Child
//! inherits parent imports via use super::* and may call the parent private
//! methods (descendant privacy) and the extracted helper modules.

use super::*;

impl RustCodegen {

    /// Convert a Buff [`TypeRef`] to a Rust [`syn::Type`].
    ///
    /// Returns an error for unsupported forms (function types); these will
    /// land in T12/T13.
    pub(super) fn ast_typeref_to_syn(&mut self, ty: &TypeRef) -> Result<SynType, CodegenError> {
        match ty {
            TypeRef::Named { name, .. } => {
                // T124b: `DateTime` is the only prelude type that takes a
                // generic argument (`<chrono::Utc>`). Handle it before the
                // primitive-name table (which returns the bare path string
                // and would drop the generic).
                if name.name == "DateTime" {
                    return Ok(make_generic_path_type(
                        "chrono::DateTime",
                        vec![rust_path_type("chrono::Utc")],
                    ));
                }
                // T124d: `Regex` source-level annotation lowers to the
                // fully-qualified `regex::Regex` path. No generic arg
                // (unlike DateTime). Handled before the primitive-name
                // table so the table stays the bare primitive-name mapping
                // (Int/Bool/...) without leaking the regex path.
                if name.name == "Regex" {
                    return Ok(rust_path_type("regex::Regex"));
                }
                // T84: `Range<T>` source-level annotation lowers to the
                // fully-qualified `std::ops::Range<T>` path (the lazy
                // iterator Rust std type). The element type arg flows
                // through the generic lowering below via the
                // `TypeRef::Generic` arm (a `Range<Int>` annotation
                // parses as `TypeRef::Generic { base: "Range", args:
                // [Int] }`, so this Named-path fast-path only fires for
                // the bare `Range` form without args — rare, but
                // handled for completeness). The Generic arm emits
                // `std::ops::Range<i64>` directly via the helper below.
                if name.name == "Range" {
                    return Ok(rust_path_type("std::ops::Range"));
                }
                // T32: the Buff→Rust primitive-name mapping is now a
                // single named, configurable table at
                // [`buff_primitive_to_rust_name`] (covers all 9 primitive
                // names: Int, Byte, Bits, Float, Double, Bool, String,
                // Char, Decimal). Unknown names pass through unchanged so
                // user-defined types (struct/enum names) keep their spelling.
                let rust_name = buff_primitive_to_rust_name(&name.name);
                Ok(rust_path_type(rust_name))
            }
            TypeRef::Option(inner, _) => {
                let inner_ty = self.ast_typeref_to_syn(inner)?;
                Ok(make_generic_path_type("Option", vec![inner_ty]))
            }
            TypeRef::Generic { base, args, .. } => {
                // Lower the base type to a path string (we only support Named base for now).
                let base_name = match base.as_ref() {
                    TypeRef::Named { name, .. } => name.name.clone(),
                    _ => return Err(self.unsupported("generic with non-named base type")),
                };
                let lowered_args: Result<Vec<SynType>, CodegenError> =
                    args.iter().map(|a| self.ast_typeref_to_syn(a)).collect();
                let lowered_args = lowered_args?;
                // T84: `Range<T>` source annotation lowers to the
                // fully-qualified `std::ops::Range<T>` path (the lazy
                // iterator Rust std type — NOT a `Range` user struct).
                // Without this rewrite, the generic lowering below
                // would emit `Range<i64>` (a user-type-shaped path),
                // which Rust cannot resolve. Other base names pass
                // through unchanged so user-defined generics keep
                // their spelling (Pair<Int, String> etc.).
                let path = if base_name == "Range" {
                    "std::ops::Range"
                } else {
                    &base_name
                };
                Ok(make_generic_path_type(path, lowered_args))
            }
            TypeRef::Function { .. } => Err(self.unsupported("function-type codegen (T12/T13)")),
            // T76: union types `A | B | C`. Compute canonical name
            // (join member display names with "Or"), collect into
            // `collected_unions`, and return the wrapper enum name as a
            // SynType::Path.
            TypeRef::Union(members, _) => {
                // Compute canonical union name: "String | Int" → "StringOrInt".
                let union_name: String = members
                    .iter()
                    .map(|m| m.to_string())
                    .collect::<Vec<_>>()
                    .join("Or");
                // Collect for dedup emission (emit once per unique union).
                self.collected_unions
                    .entry(union_name.clone())
                    .or_insert_with(|| members.clone());
                Ok(rust_path_type(&union_name))
            }
            // T103: tuple types `(T, U, ...)`. Lower each member to a syn::Type
            // and build a Rust tuple type via `quote!` + `parse2`. The 2+-
            // element rule lives at parse time, so this always carries 2+
            // members (Rust tuples need 2+ fields to be a "real" tuple; a
            // single-field `(T,)` is the trailing-comma idiom, which Buff
            // does not produce at the TYPE layer).
            TypeRef::Tuple(members, _) => {
                let lowered: Vec<SynType> = members
                    .iter()
                    .map(|m| self.ast_typeref_to_syn(m))
                    .collect::<Result<Vec<_>, _>>()?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    ( #( #lowered ),* )
                };
                syn::parse2::<SynType>(tokens)
                    .map_err(|e| self.unsupported(&format!("tuple type codegen parse: {e}")))
            }
        }
    }

    /// Map a resolved Buff [`Type`] (post-inference) to a Rust [`syn::Type`].
    ///
    /// Returns `None` for [`Type::Unknown`] and [`Type::Void`] — callers
    /// (notably `let` lowering) treat `None` as "no annotation emitted".
    /// [`Type::Decimal`] maps to `rust_decimal::Decimal` (the crate is a
    /// dependency of `buff-lang-codegen-rust` so generated crates must depend
    /// on it as well — the runtime/driver is responsible for that).
    pub(super) fn buff_type_to_syn(&self, ty: &Type) -> Option<SynType> {
        // Handle generic types (Vector, Matrix, Option) first.
        match ty {
            Type::Vector(elem) => {
                let inner = self.buff_type_to_syn(elem)?;
                return Some(make_generic_path_type("Vec", vec![inner]));
            }
            Type::Matrix(elem) => {
                // T24: Matrix<T> maps to the builtin `Matrix<T>` struct that
                // this codegen emits on-demand. The inner element type uses
                // the standard mapping; an Unknown element falls back to
                // i64 (Buff's default Int) so the annotation still compiles.
                let inner = self
                    .buff_type_to_syn(elem)
                    .unwrap_or_else(|| rust_path_type("i64"));
                return Some(make_generic_path_type("Matrix", vec![inner]));
            }
            Type::Option(inner) => {
                let inner = self.buff_type_to_syn(inner)?;
                return Some(make_generic_path_type("Option", vec![inner]));
            }
            // T25: Map<K, V> → Rust `std::collections::HashMap<K, V>`. We use
            // the fully-qualified path so generated programs do NOT need a
            // `use std::collections::HashMap;` import (the literal codegen
            // also uses the fully-qualified path, keeping import-management
            // out of the picture for v0.5).
            Type::Map(key, value) => {
                let k = self
                    .buff_type_to_syn(key)
                    .unwrap_or_else(|| rust_path_type("i64"));
                let v = self
                    .buff_type_to_syn(value)
                    .unwrap_or_else(|| rust_path_type("i64"));
                return Some(make_qualified_generic_path_type(
                    "std::collections::HashMap",
                    vec![k, v],
                ));
            }
            // T30: Result<T, E> → Rust `Result<T, E>` (the std Result is in
            // scope by default, so no fully-qualified path needed — mirroring
            // Option<T>'s 1:1 mapping from T28). Both inners must resolve to
            // a concrete Rust type; an Unknown inner (e.g. `Ok(42)` infers
            // `Result<Int<64>, Unknown>`) makes the whole annotation
            // indeterminate, so we return None and let Rust infer from
            // context (function return type, etc.).
            Type::Result(ok, err) => {
                let ok_ty = self.buff_type_to_syn(ok)?;
                let err_ty = self.buff_type_to_syn(err)?;
                return Some(make_generic_path_type("Result", vec![ok_ty, err_ty]));
            }
            // T76: union types — resolved `Type::Union` is only reached via
            // `typeref_to_type` for source unions; there's no inference
            // path that produces Union directly. Return None so Rust
            // inference handles the annotation (the wrapper enum type is
            // determined in `ast_typeref_to_syn` which collects unions from
            // TypeRef::Union).
            Type::Union(_) => return None,
            // T103: tuple types `(T, U, ...)`. Lower each member to a syn::Type
            // and build a Rust tuple type via `quote!` + `parse2`. Any
            // unresolvable member (Unknown/Void) makes the whole annotation
            // indeterminate — return None so Rust infers the tuple type from
            // context (function return type, etc.).
            Type::Tuple(members) => {
                let lowered: Vec<SynType> = members
                    .iter()
                    .map(|m| self.buff_type_to_syn(m))
                    .collect::<Option<Vec<_>>>()?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    ( #( #lowered ),* )
                };
                match syn::parse2::<SynType>(tokens) {
                    Ok(ty) => return Some(ty),
                    Err(_) => return None,
                }
            }
            // T37: user-defined generic type application. Emit the user
            // type's name verbatim with a turbofish when args are present
            // (`Pair<i64, String>`), or the bare name when args are empty
            // (`Point`). Unknown args fall back to i64 (Buff's default Int)
            // so the annotation still compiles — mirrors the Matrix<T>
            // precedent. This produces byte-identical Rust to the
            // `ast_typeref_to_syn` path that lowers the same shape directly
            // from a `TypeRef`, so a `Type::User` flowing through inference
            // and a `TypeRef::Generic` flowing through annotation lowering
            // emit the same source.
            Type::User { name, args } => {
                if args.is_empty() {
                    return Some(rust_path_type(name));
                }
                let lowered_args: Vec<SynType> = args
                    .iter()
                    .map(|a| self.buff_type_to_syn(a).unwrap_or_else(|| rust_path_type("i64")))
                    .collect();
                return Some(make_generic_path_type(name, lowered_args));
            }
            // T68: trait object `Box<dyn Trait>`. Lower the inner trait
            // type to a syn::Type (conventionally a bare path like
            // `Drawable`), then wrap in `Box<dyn ...>` via `quote!` +
            // `parse2` (the same shape the Tuple arm uses). An unresolvable
            // inner (e.g. Unknown trait) makes the whole annotation
            // indeterminate — return None so Rust infers from context.
            // Buff's hide-the-borrow-checker philosophy: the user writes
            // only the trait name; codegen always emits the single owned
            // `Box<dyn Trait>` form (never `&dyn`, never visible
            // lifetimes).
            Type::DynamicDispatch(trait_ty) => {
                let inner = self.buff_type_to_syn(trait_ty)?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    Box<dyn #inner>
                };
                match syn::parse2::<SynType>(tokens) {
                    Ok(ty) => return Some(ty),
                    Err(_) => return None,
                }
            }
            _ => {}
        }
        let rust_name: &str = match ty {
            Type::Int {
                width: IntWidth::W8,
            } => "i8",
            Type::Int {
                width: IntWidth::W16,
            } => "i16",
            Type::Int {
                width: IntWidth::W32,
            } => "i32",
            Type::Int {
                width: IntWidth::W64,
            } => "i64",
            Type::Int {
                width: IntWidth::W128,
            } => "i128",
            Type::Bits {
                width: IntWidth::W8,
            } => "u8",
            Type::Bits {
                width: IntWidth::W16,
            } => "u16",
            Type::Bits {
                width: IntWidth::W32,
            } => "u32",
            Type::Bits {
                width: IntWidth::W64,
            } => "u64",
            Type::Bits {
                width: IntWidth::W128,
            } => "u128",
            // f16 is unstable in std; we map to f32 as a safe approximation.
            Type::Float {
                width: FloatWidth::W16,
            } => "f32",
            Type::Float {
                width: FloatWidth::W32,
            } => "f32",
            Type::Float {
                width: FloatWidth::W64,
            } => "f64",
            Type::Double => "f64",
            Type::Bool => "bool",
            Type::String => "String",
            // T21: Char → Rust's `char` (a 4-byte Unicode scalar value).
            Type::Char => "char",
            Type::Decimal => "rust_decimal::Decimal",
            // T124b: prelude datetime family. The plain Rust path mapping
            // is reused for everything except DateTime (which needs the
            // generic `<chrono::Utc>` argument — handled by the early
            // return just below).
            Type::Date => "chrono::NaiveDate",
            Type::Time => "chrono::NaiveTime",
            Type::Duration => "chrono::TimeDelta",
            Type::Instant => "std::time::Instant",
            // T124d: prelude Regex type. Plain `regex::Regex` path — no
            // generic argument needed (unlike DateTime). Generated code
            // uses the fully-qualified path so no `use` import is emitted.
            Type::Regex => "regex::Regex",
            // T124h: prelude URL type. Plain `url::Url` path - no
            // generic argument needed. Generated code uses the fully-
            // qualified path so no `use` import is emitted. Note the
            // case mapping: Buff surface is `URL` (all-caps per the
            // DateTime / Regex convention); the underlying Rust type is
            // `url::Url` (capital U, lowercase rl - the canonical Rust
            // spelling).
            Type::Url => "url::Url",
            // T124j: prelude Path type. Plain `std::path::PathBuf`
            // path - no generic argument needed. Generated code uses
            // the fully-qualified std path so no `use` import is
            // emitted AND no extern crate is recorded (std-only,
            // mirrors the Math/Strings/Args/Env stance from T124f/
            // T124g). Note: the underlying Rust type is `PathBuf`
            // (the owned, mutable path type) - Buff surfaces owned
            // values; `&Path` is hidden from users. Buff surface is
            // `Path` (capitalised per the DateTime / Regex / URL
            // convention); the case mapping happens here.
            Type::Path => "std::path::PathBuf",
            // T124l: prelude Process type. Plain
            // `Option<std::process::Child>` path - the Option
            // wrapper lets `Process.spawn` be panic-free (a spawn
            // failure collapses to `None`; `.wait()` / `.id()`
            // chain `.map(...).unwrap_or_default()` through the
            // Option). Generated code uses the fully-qualified
            // std path so no `use` import is emitted AND no extern
            // crate is recorded (std-only - mirrors the Path /
            // Dir.list / Tempfile.dir stance from T124j). The
            // generic argument over `Child` is constructed via
            // `make_generic_path_type` so the emitted Rust type is
            // `Option<std::process::Child>` (Buff surfaces the
            // Option wrapper to the user - they observe spawn
            // failure as a `Process` value whose `.wait()` / `.id()`
            // return `0`; a future task may surface spawn failure
            // via a `Result<Process, Error>` if a use case emerges).
            Type::Process => {
                return Some(make_generic_path_type(
                    "Option",
                    vec![rust_path_type("std::process::Child")],
                ));
            }
            // T124m: prelude TCP-Connection type. Plain
            // `Option<tokio::net::TcpStream>` path - the Option
            // wrapper lets `TCP.connect` be panic-free (a
            // connect failure collapses to `None`; `.send()` /
            // `.recv()` / `.close()` then operate on the
            // Option via `if let Some(mut s) = ...`). Generated
            // code uses the fully-qualified tokio path so no
            // top-level `use` import is emitted - but the
            // recorded `tokio` in extern_crates signals to the
            // pipeline / build-driver that the generated Cargo
            // project must declare `tokio` in `[dependencies]`
            // (idempotent with the existing tokio walker from
            // T124g).
            Type::Connection => {
                return Some(make_generic_path_type(
                    "Option",
                    vec![rust_path_type("tokio::net::TcpStream")],
                ));
            }
            // T124m: prelude UDP-Socket type. Plain
            // `Option<tokio::net::UdpSocket>` path - same
            // Option-wrapper stance as Type::Connection / Type::
            // Process (panic-free bind via `.ok()` collapse).
            Type::Socket => {
                return Some(make_generic_path_type(
                    "Option",
                    vec![rust_path_type("tokio::net::UdpSocket")],
                ));
            }
            // T124m: prelude WebSocket-WsConnection type. Plain
            // `Option<tokio_tungstenite::WebSocketStream<
            // tokio_tungstenite::MaybeTlsStream<tokio::net::
            // TcpStream>>>` path - the nested generic carries
            // the MaybeTlsStream wrapper (so `wss://` TLS
            // endpoints work) over the TcpStream transport. The
            // Option wrapper keeps connect panic-free via `.ok()
            // .map(...)`. The `tokio-tungstenite` + `futures-
            // util` crates are recorded in extern_crates (via
            // the narrow `program_uses_tokio_tungstenite`
            // walker). Built via `make_qualified_generic_path_type`
            // (NOT `make_generic_path_type`) because the path
            // segments include `::` - the simpler helper panics
            // on `::`-bearing names since `Ident::new` rejects
            // them.
            Type::WsConnection => {
                let inner_ty = make_qualified_generic_path_type(
                    "tokio_tungstenite::WebSocketStream",
                    vec![make_qualified_generic_path_type(
                        "tokio_tungstenite::MaybeTlsStream",
                        vec![rust_path_type("tokio::net::TcpStream")],
                    )],
                );
                return Some(make_generic_path_type("Option", vec![inner_ty]));
            }
            Type::Unknown | Type::Void => return None,
            // T124b: DateTime is the only prelude type that needs a generic
            // argument. Return early with the proper generic-argument form
            // so `let dt: DateTime = ...` emits
            // `let dt: chrono::DateTime<chrono::Utc> = ...`.
            Type::DateTime => {
                return Some(make_generic_path_type(
                    "chrono::DateTime",
                    vec![rust_path_type("chrono::Utc")],
                ));
            }
            // Vector, Matrix, Map, Option, and Result are handled by the
            // early-return match above; this arm is unreachable but required
            // for exhaustiveness.
            Type::Vector(_)
            | Type::Matrix(_)
            | Type::Option(_)
            | Type::Map(_, _)
            | Type::Result(_, _)
            | Type::Union(_)
            | Type::Tuple(_)
            // T37: User is handled by the early-return match above
            // (turbofish-or-bare-path emission); unreachable here but
            // required for exhaustiveness.
            | Type::User { .. }
            // T84: Range<T> is a lazy iterator produced by the `..` /
            // `..=` operator. The codegen lowers the AST directly via
            // `lower_range` (which emits `start..end` / `start..=end`),
            // so a `Type::Range` flowing through `buff_type_to_syn` is
            // only consulted when the user writes an explicit
            // annotation like `let r: Range<Int> = 0..10`. We return
            // None so Rust infers the type from the initializer — this
            // avoids the Range-vs-RangeInclusive mismatch (Buff
            // surfaces a single `Range<T>` abstraction; Rust has two
            // distinct types). Mirrors Unknown / Void / Sender /
            // Receiver: let Rust infer from context.
            | Type::Range(_) => return None,
            // T71: lazy iterator `Iterator<T>`. Maps to Rust's iterator
            // adapters (`.iter().map().filter()...`). The codegen lowers
            // the AST directly via `lower_method_call` (which emits
            // `recv.iter().map(f).filter(p)...`), so a `Type::Iterator`
            // flowing through `buff_type_to_syn` is only consulted when
            // the user writes an explicit annotation like
            // `let it: Iterator<Int> = vec.lazy()`. We return None so
            // Rust infers the type from the initializer — this avoids
            // the concrete-iterator-type mismatch (Buff surfaces a
            // single `Iterator<T>` abstraction; Rust has many distinct
            // adapter types). Mirrors Range / Unknown / Void: let Rust
            // infer from context.
            | Type::Iterator(_) => return None,
            // T68: trait object `Box<dyn Trait>` is handled by the
            // early-return match above (lowers to `Box<dyn Trait>` via
            // `quote!`); unreachable here but required for exhaustiveness.
            | Type::DynamicDispatch(_) => return None,
            // T2: channel sender / receiver. Opaque runtime-value types
            // mapped to `buff_lang_runtime::Sender<T>` /
            // `buff_lang_runtime::Receiver<T>`. The element type T is
            // implicit (Type-level we don't carry it); codegen emits
            // an opaque path WITHOUT a turbofish so Rust's type
            // inference derives T from subsequent `sender.send(value)`
            // / `receiver.recv()` usage. If a user annotates a let
            // binding with an explicit Sender/Receiver type, codegen
            // returns None and lets Rust infer the type from the
            // initializer (mirroring Unknown / Void behavior).
            Type::Sender | Type::Receiver => return None,
            // T9: image. Opaque runtime-value type mapped to
            // `buff_image::Image`. No generic parameter, no turbofish
            // needed. If a user annotates a let binding with an
            // explicit Image type, codegen emits the concrete path;
            // otherwise Rust infers the type from the initializer
            // (mirroring Regex / Path / Process behavior).
            Type::Image => "buff_image::Image",
            // T37: prelude Faker type. Plain `buff_fake::Faker` path
            // — no generic argument needed. Generated code uses the
            // fully-qualified path so no `use` import is emitted.
            // Mirrors the T9 Image precedent.
            Type::Faker => "buff_fake::Faker",
            // T31: cache. Opaque runtime-value type mapped to
            // `buff_cache::Cache`. No generic parameter, no turbofish
            // needed. Mirrors the T9 Image precedent: if a user
            // annotates a let binding with an explicit Cache type,
            // codegen emits the concrete path; otherwise Rust infers
            // the type from the initializer (Cache.new).
            Type::Cache => "buff_cache::Cache",
            // T44: I18n runtime-value type maps to `buff_i18n::I18n`
            // at codegen time. Mirrors the Cache/Image precedent: no
            // generic parameter, no turbofish. If a user annotates a
            // let binding with an explicit I18n type, codegen emits
            // the concrete path; otherwise Rust infers the type from
            // the initializer (I18n.new).
            Type::I18n => "buff_i18n::I18n",
            // T10: audio. Opaque runtime-value type mapped to
            // `buff_audio::AudioBuffer`. No generic parameter, no
            // turbofish needed. Mirrors the T9 Image precedent: if a
            // user annotates a let binding with an explicit
            // AudioBuffer type, codegen emits the concrete path;
            // otherwise Rust infers the type from the initializer
            // (AudioBuffer.from_path / AudioBuffer.from_samples).
            Type::Audio => "buff_audio::AudioBuffer",
            // T7: columnar DataFrame. Opaque runtime-value type mapped
            // to `buff_dataframe::DataFrame`. No generic parameter, no
            // turbofish needed. Mirrors the T9 Image / T10 Audio
            // precedent: if a user annotates a let binding with an
            // explicit DataFrame type, codegen emits the concrete path;
            // otherwise Rust infers the type from the initializer
            // (DataFrame.from_csv / DataFrame.from_json). The
            // `buff-dataframe` crate is recorded in `extern_crates`
            // when a Buff program uses `DataFrame.*` (via the narrow
            // `program_uses_namespace("DataFrame")` walker).
            Type::DataFrame => "buff_dataframe::DataFrame",
            // T33: HTTP client. Opaque runtime-value type mapped to
            // `buff_http_client::HttpClient`. No generic parameter, no
            // turbofish needed. Mirrors the T9 Image / T7 DataFrame
            // precedent: if a user annotates a let binding with an
            // explicit HttpClient type, codegen emits the concrete
            // path; otherwise Rust infers the type from the initializer
            // (HttpClient.new()). The `buff-http-client` crate is
            // recorded in `extern_crates` when a Buff program uses
            // `HttpClient.*` (via the narrow
            // `program_uses_namespace("HttpClient")` walker).
            Type::HttpClient => "buff_http_client::HttpClient",
            // T29: validator. Opaque runtime-value type mapped to
            // `buff_validate::Validator`. No generic parameter, no
            // turbofish needed. Mirrors the T9 Image / T33 HttpClient
            // precedent: if a user annotates a let binding with an
            // explicit Validator type, codegen emits the concrete
            // path; otherwise Rust infers the type from the initializer
            // (Validator.new()). The `buff-validate` + `validator` +
            // `serde_json` + `regex` crates are recorded in
            // `extern_crates` when a Buff program uses `Validator.*`
            // (via the narrow `program_uses_namespace("Validator")`
            // walker).
            Type::Validator => "buff_validate::Validator",
            // T42: email. Opaque runtime-value type mapped to
            // `buff_email::Email`. No generic parameter, no turbofish
            // needed. Mirrors the T9 Image / T33 HttpClient / T29
            // Validator precedent: if a user annotates a let binding
            // with an explicit Email type, codegen emits the concrete
            // path; otherwise Rust infers the type from the
            // initializer (Email.new). The `buff-email` + `lettre`
            // + `handlebars` crates are recorded in `extern_crates`
            // when a Buff program uses `Email.*` (via the narrow
            // `program_uses_namespace("Email")` walker).
            Type::Email => "buff_email::Email",
            // T42: SMTP client. Opaque runtime-value type mapped to
            // `buff_email::SmtpClient`. No generic parameter, no
            // turbofish needed. Mirrors the Email precedent: if a
            // user annotates a let binding with an explicit
            // SmtpClient type, codegen emits the concrete path;
            // otherwise Rust infers the type from the initializer
            // (SmtpClient.new). The `buff-email` + `lettre` crates
            // are recorded in `extern_crates` (shared walker with
            // `Email.*`).
            Type::SmtpClient => "buff_email::SmtpClient",
            // T43: HTML Document / Element / Crawler. Opaque runtime-
            // value types mapped to `buff_scrape::{Document, Element,
            // Crawler}`. No generic parameter, no turbofish needed.
            // Mirrors the T9 Image / T31 Cache / T42 Email precedent:
            // if a user annotates a let binding with an explicit
            // Document / Element / Crawler type, codegen emits the
            // concrete path; otherwise Rust infers the type from the
            // initializer (Document.from_html / Element returned from
            // select / Crawler.new). The `buff-scrape` + `scraper`
            // + `reqwest` crates are recorded in `extern_crates` when
            // a Buff program uses any of these (via the shared
            // `program_uses_namespace("Document" / "Element" /
            // "Crawler")` walker).
            Type::Document => "buff_scrape::Document",
            Type::Element => "buff_scrape::Element",
            Type::Crawler => "buff_scrape::Crawler",
            // T12: ECS World + Entity. Opaque runtime-value types
            // mapped to `buff_ecs::World` / `buff_ecs::Entity`. Added
            // by T31 (this commit) because T12 added the variants in
            // ty.rs but missed the codegen arm — codegen cannot
            // compile otherwise. No generic parameter, no turbofish
            // needed (mirrors Image / DataFrame / HttpClient).
            Type::World => "buff_ecs::World",
            Type::Entity => "buff_ecs::Entity",
            // T19: Template. Opaque runtime-value type mapped to
            // `buff_template::Template`. Added by T31 (this commit)
            // because T19 added the codegen method arm (M::Render)
            // without the matching `Type::Template` variant in ty.rs
            // or this `buff_type_to_syn` arm — both required to keep
            // codegen compiling.
            Type::Template => "buff_template::Template",
            // T50: Xml. Opaque runtime-value type mapped to
            // `buff_xml::XmlDocument`. No generic parameter, no
            // turbofish needed. Mirrors the T9 Image / T31 Cache
            // precedent: if a user annotates a let binding with an
            // explicit Xml type, codegen emits the concrete path;
            // otherwise Rust infers the type from the initializer
            // (Xml.from_str). The `buff-xml` + `quick-xml` crates
            // are recorded in `extern_crates` when a Buff program
            // uses `Xml.*` (via the narrow
            // `program_uses_namespace("Xml")` walker).
            Type::Xml => "buff_xml::XmlDocument",
            // T50: XmlElement. Opaque runtime-value type mapped to
            // `buff_xml::XmlElement`. No generic parameter, no
            // turbofish needed. Mirrors the Type::Xml arm above
            // (T50 Xml / XmlDocument precedent).
            Type::XmlElement => "buff_xml::XmlElement",
            // T51: MsgPack namespace. Opaque type marker — MsgPack is
            // namespace-only (`MsgPack.serialize(...)` /
            // `MsgPack.deserialize(...)` are never instantiated as
            // values), so this arm rarely fires in practice. Required
            // for match exhaustiveness (the rust_name match has no `_`
            // wildcard). Mirrors Type::Regex arm shape above.
            Type::MsgPack => "buff_msgpack::MsgPack",
            // T45: prelude geo types. Opaque runtime-value types
            // mapped to `buff_geo::{Point, LineString, Polygon}`. No
            // generic parameters, no turbofish needed. Mirrors the T9
            // Image / T50 Xml precedent.
            Type::Point => "buff_geo::Point",
            Type::LineString => "buff_geo::LineString",
            Type::Polygon => "buff_geo::Polygon",
            // T54: prelude SIMD type. Opaque runtime-value type mapped
            // to `buff_simd::Simd` (a 4-lane f32x4 register wrapping
            // `wide::f32x4`). No generic parameters, no turbofish
            // needed. Mirrors the T9 Image / T45 Point precedent.
            Type::Simd => "buff_simd::Simd",
            // T59: prelude actor types. Opaque runtime-value types
            // mapped to `buff_actors::{ActorSystem, ActorRef,
            // Supervisor}` + `buff_actors::supervisor::{ChildSpec,
            // RestartStrategy}`. No generic parameters.
            Type::ActorSystem => "buff_actors::ActorSystem",
            Type::ActorRef => "buff_actors::ActorRef",
            Type::Supervisor => "buff_actors::Supervisor",
            Type::ChildSpec => "buff_actors::supervisor::ChildSpec",
            Type::RestartStrategy => "buff_actors::supervisor::RestartStrategy",
            // T46: prelude NLP types. `Text` is namespace-only (mirrors
            // MsgPack — the arm rarely fires in practice but is required
            // for match exhaustiveness). `Language` is a runtime value
            // (mirrors Point). `StemAlgorithm` is an opaque enum passed
            // only as an arg to Text.stem. All three map to
            // `buff_nlp::*` paths.
            Type::Text => "buff_nlp::Text",
            Type::Language => "buff_nlp::Language",
            Type::StemAlgorithm => "buff_nlp::StemAlgorithm",
            // T52: prelude Protobuf namespace + Message instance type.
            // `Protobuf` is namespace-only (mirrors MsgPack — the arm
            // rarely fires in practice but is required for match
            // exhaustiveness). `Message` is a runtime value (mirrors
            // Image / Xml). Both map to `buff_protobuf::*` paths.
            Type::Protobuf => "buff_protobuf::Protobuf",
            Type::Message => "buff_protobuf::Message",
            // T47: prelude chat types. `Bot` / `ChatMessage` /
            // `Platform` are all runtime values (mirrors Image /
            // Point). Map to `buff_chat::*` paths. Note: `ChatMessage`
            // (Buff surface) maps to `buff_chat::Message` (Rust surface)
            // — the renaming avoids colliding with T52's
            // `buff_protobuf::Message` (the shorter `Message` Buff name
            // is owned by T52).
            Type::Bot => "buff_chat::Bot",
            Type::ChatMessage => "buff_chat::Message",
            Type::Platform => "buff_chat::Platform",
            // T48: prelude web3 types. `Provider` / `Wallet` /
            // `ConnectedWallet` / `Contract` / `ContractMethod` are
            // all runtime values (mirrors Image / Point / Bot — none
            // are namespace-only). Map to `buff_web3::*` paths 1:1
            // (no renaming needed — Buff surface names match the Rust
            // struct names in `buff_web3::`).
            Type::Provider => "buff_web3::Provider",
            Type::Wallet => "buff_web3::Wallet",
            Type::ConnectedWallet => "buff_web3::ConnectedWallet",
            Type::Contract => "buff_web3::Contract",
            Type::ContractMethod => "buff_web3::ContractMethod",
            // T49: prelude crypto-extras types. AES / RSA / ECDH /
            // Argon2 are namespace-only (mirrors MsgPack — the arms
            // rarely fire in practice but are required for match
            // exhaustiveness). RsaKeypair is a runtime value (mirrors
            // Image / Point / Wallet). All five map to
            // `buff_crypto_extras::*` paths.
            Type::AES => "buff_crypto_extras::AES",
            Type::RSA => "buff_crypto_extras::RSA",
            Type::ECDH => "buff_crypto_extras::ECDH",
            Type::Argon2 => "buff_crypto_extras::Argon2",
            Type::RsaKeypair => "buff_crypto_extras::RsaKeypair",
            // T8/T11/T17/T18/T27/T34: framework runtime-value types
            // whose PreludeType registrations previously forward-declared
            // as Type::Unknown/Void. Each maps to the matching
            // `buff_*::TypeName` path 1:1 (no generic parameter, no
            // turbofish needed). Mirrors the T9 Image / T7 DataFrame /
            // T33 HttpClient precedent: if a user annotates a let
            // binding with an explicit type, codegen emits the concrete
            // path; otherwise Rust infers the type from the
            // initializer.
            Type::Tensor => "buff_tensor::Tensor",
            Type::Signal => "buff_dsp::Signal",
            Type::Spectrum => "buff_dsp::Spectrum",
            Type::Web => "buff_web::Web",
            Type::Pool => "buff_db::Pool",
            Type::Strategy => "buff_fuzz::Strategy",
            Type::OAuth2Client => "buff_auth::OAuth2Client",
            Type::Rbac => "buff_auth::Rbac",
        };
        Some(rust_path_type(rust_name))
    }

}

#[cfg(test)]
mod tests {
    //! T68 inline unit tests for the `Box<dyn Trait>` trait-object lowering.
    //! Exercises `buff_type_to_syn` directly (the method is `pub(super)`,
    //! reachable from this child module) so the lowering is verified without
    //! needing a full `generate_rust` round-trip or parser support for the
    //! `Box<dyn ...>` source form.

    use super::*;
    use buff_lang_types::Type;
    use quote::ToTokens;

    #[test]
    fn dynamic_dispatch_lowers_to_box_dyn_trait() {
        let cg = RustCodegen::new();
        let trait_obj = Type::dynamic_dispatch(Type::user("Drawable", Vec::new()));
        let syn_ty = cg
            .buff_type_to_syn(&trait_obj)
            .expect("Box<dyn Drawable> must lower to a syn::Type");
        // Render via ToTokens so we can assert on the token fragments
        // (spacing is unspecified; assert on the meaningful tokens).
        let rendered = syn_ty.to_token_stream().to_string();
        assert!(
            rendered.contains("Box") && rendered.contains("dyn") && rendered.contains("Drawable"),
            "expected `Box<dyn Drawable>`-shaped lowering, got: {rendered}"
        );
        assert!(
            !rendered.contains("&dyn"),
            "must emit owned Box<dyn>, never a reference: {rendered}"
        );
    }

    #[test]
    fn dynamic_dispatch_inner_unknown_returns_none() {
        // When the inner trait type is Unknown (indeterminate), the whole
        // annotation is indeterminate — return None so Rust infers from
        // context (mirrors Option/Result/Tuple Unknown handling).
        let cg = RustCodegen::new();
        let trait_obj = Type::dynamic_dispatch(Type::Unknown);
        assert!(
            cg.buff_type_to_syn(&trait_obj).is_none(),
            "Unknown inner trait must yield None"
        );
    }
}

#[cfg(test)]
mod t68_display_tests {
    use buff_lang_types::Type;

    #[test]
    fn dynamic_dispatch_display_renders_box_dyn() {
        let trait_obj = Type::dynamic_dispatch(Type::user("Drawable", Vec::new()));
        assert_eq!(trait_obj.to_string(), "Box<dyn Drawable>");
    }

    #[test]
    fn dynamic_dispatch_is_not_numeric_nor_gpu_eligible() {
        let trait_obj = Type::dynamic_dispatch(Type::user("Drawable", Vec::new()));
        assert!(!trait_obj.is_numeric());
        assert!(!trait_obj.is_float_like());
        assert!(!trait_obj.is_integer_like());
        assert!(!trait_obj.is_gpu_eligible());
    }
}


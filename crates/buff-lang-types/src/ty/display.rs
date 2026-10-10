//! Display formatting for [`Type`] - ITER-55 pure move from ty.rs.
//! Keeps ty.rs under the ~2.5k-line ceiling; the impl is unchanged.

use super::Type;
use std::fmt;

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
            // value â€” like Log / Toml / Base64 / Hex / Yaml / Csv).
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
            // T54: prelude SIMD type. Opaque runtime-value type mapped
            // to `buff_simd::Simd` (a 4-lane f32x4 register). Display
            // mirrors the Buff surface name.
            Type::Simd => f.write_str("Simd"),
            // T59: prelude actor types. Opaque runtime-value types
            // mapped to `buff_actors::{ActorSystem, ActorRef,
            // Supervisor}` + `buff_actors::supervisor::{ChildSpec,
            // RestartStrategy}`.
            Type::ActorSystem => f.write_str("ActorSystem"),
            Type::ActorRef => f.write_str("ActorRef"),
            Type::Supervisor => f.write_str("Supervisor"),
            Type::ChildSpec => f.write_str("ChildSpec"),
            Type::RestartStrategy => f.write_str("RestartStrategy"),
            // T8/T11/T17/T18/T27/T34: framework runtime-value types
            // whose canonical Rust representations live in the
            // matching `buff-*` framework crates. Display mirrors the
            // Buff surface name so diagnostics read naturally.
            Type::Tensor => f.write_str("Tensor"),
            Type::Signal => f.write_str("Signal"),
            Type::Spectrum => f.write_str("Spectrum"),
            Type::Window => f.write_str("Window"),
            Type::Web => f.write_str("Web"),
            Type::Pool => f.write_str("Pool"),
            Type::Strategy => f.write_str("Strategy"),
            Type::OAuth2Client => f.write_str("OAuth2Client"),
            Type::Rbac => f.write_str("Rbac"),
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
            // T47: prelude chat types. `Bot` / `ChatMessage` /
            // `Platform` are all runtime values (mirrors Point /
            // Language). Display mirrors the Buff surface name in all
            // three cases. Note `ChatMessage` (not `Message`) â€” T52
            // owns the shorter `Message` name (protobuf).
            Type::Bot => f.write_str("Bot"),
            Type::ChatMessage => f.write_str("ChatMessage"),
            Type::Platform => f.write_str("Platform"),
            // T48: prelude web3 types. All five are runtime values
            // (mirrors Provider / Wallet / Contract surfaces from
            // ethers-rs / web3.py / ethers.js). Display mirrors the
            // Buff surface name in all five cases.
            Type::Provider => f.write_str("Provider"),
            Type::Wallet => f.write_str("Wallet"),
            Type::ConnectedWallet => f.write_str("ConnectedWallet"),
            Type::Contract => f.write_str("Contract"),
            Type::ContractMethod => f.write_str("ContractMethod"),
            // T49: prelude crypto-extras types. AES / RSA / ECDH /
            // Argon2 are namespace-only (mirrors MsgPack); RsaKeypair
            // is a runtime value (mirrors Image / Point). Display
            // mirrors the Buff surface name in all five cases.
            Type::AES => f.write_str("AES"),
            Type::RSA => f.write_str("RSA"),
            Type::ECDH => f.write_str("ECDH"),
            Type::Argon2 => f.write_str("Argon2"),
            Type::RsaKeypair => f.write_str("RsaKeypair"),
            // T37: user-defined generic type application. Renders the
            // user type's name with comma-separated resolved args in
            // angle brackets (matching the source form), or the bare
            // name when there are no args.
            Type::User { name, args } => {
                if args.is_empty() {
                    f.write_str(name)
                } else {
                    write!(f, "{name}<")?;
                    for (i, a) in args.iter().enumerate() {
                        if i > 0 {
                            f.write_str(", ")?;
                        }
                        write!(f, "{a}")?;
                    }
                    f.write_str(">")
                }
            }
            // T84: lazy integer range `Range<T>`. Renders the Buff
            // surface form `Range<elem>` so diagnostics read naturally
            // (mirrors Vector<T> / Matrix<T>). The element is the
            // inferred bound type (`Range<Int<64>>` for `0..10`).
            Type::Range(elem) => write!(f, "Range<{elem}>"),
            // T71: lazy iterator `Iterator<T>`. Renders the Buff
            // surface form `Iterator<elem>` so diagnostics read
            // naturally (mirrors Vector<T> / Range<T>).
            Type::Iterator(elem) => write!(f, "Iterator<{elem}>"),
            // T68: trait object `Box<dyn Trait>`. Renders the Rust surface
            // form verbatim so diagnostics read naturally and the codegen
            // output matches 1:1. The inner type is the trait (conventionally
            // a `Type::User { name: "Drawable", .. }`).
            Type::DynamicDispatch(trait_ty) => write!(f, "Box<dyn {trait_ty}>"),
        }
    }
}

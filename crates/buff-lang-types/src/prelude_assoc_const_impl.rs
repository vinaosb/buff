//! T105b: PreludeAssocConst enum + impl + lookup helpers.
//!
//! MECHANICAL EXTRACTION from prelude_types.rs (T105b God Class split).
//! No logic changes — moved verbatim.

use crate::prelude_types::PreludeType;
use crate::prelude_types::prelude_type_lookup;
use crate::ty::Type;

// ---------------------------------------------------------------------------
// Associated constants: `Type.CONST` (T124f)
// ---------------------------------------------------------------------------

/// A recognised **associated constant** on a prelude type - the
/// `Type.CONST` access shape. The receiver is the type name itself (a
/// bare `Expr::Ident`), and the constant is accessed as a zero-arg
/// "method call" `Type.NAME` (Buff's parser produces `MethodCall` with
/// `args == []` for both `obj.field` and `obj.field()`; the codegen
/// consults this registry to decide whether `Math.PI` is a prelude
/// constant access vs. a field access on a user struct named `Math`).
///
/// # Why a separate registry
///
/// The associated-FUNCTION registry ([`PreludeAssocFn`]) dispatches
/// CALLS (`Type.method(args)`); associated CONSTANTS are accessed
/// WITHOUT parens (`Math.PI`). The codegen consults this registry in
/// the `lower_method_call` zero-arg arm BEFORE the T26 field-access
/// heuristic so a prelude constant access is rewritten to the Rust
/// path (`std::f64::consts::PI`) rather than the literal Rust field
/// access `Math.PI` (which would not compile).
///
/// # Naming convention
///
/// Variants are named after the constant's surface identifier (the
/// user-facing name). Dispatch on `(PreludeType, PreludeAssocConst)`
/// pairs is exhaustive in [`assoc_const_return_type`].
///
/// # T124f scope
///
/// Currently only the Math namespace has associated constants (`PI`,
/// `E`). Future prelude modules with constants (e.g. a future `Physics`
/// module exposing `Physics.G` for the gravitational constant) extend
/// this enum + the lookup/return-type matches.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PreludeAssocConst {
    /// `Math.PI` (pi) - the ratio of a circle's circumference to its
    /// diameter. Returns Float (lowers to `std::f64::consts::PI`).
    Pi,
    /// `Math.E` (Euler's number) - the base of the natural logarithm.
    /// Returns Float (lowers to `std::f64::consts::E`).
    E,
    /// T47: `Platform.Discord` — the Discord variant of the chat
    /// `Platform` enum. Returns [`Type::Platform`] (lowers to
    /// `buff_chat::Platform::Discord`). Used as the `platform:` arg to
    /// `Bot.new(...)` and surfaced by `msg.platform()`. Mirrors the
    /// T124f Math assoc-const shape (zero-arg `Type.NAME` access); the
    /// variant name `Discord` is PascalCase (NOT UPPERCASE like PI/E —
    /// Rust enum-variant convention) and matches the source surface
    /// `Platform.Discord` 1:1 so the codegen can splice the canonical
    /// Rust path without rewriting.
    Discord,
    /// T47: `Platform.Telegram` — the Telegram variant of the chat
    /// `Platform` enum. Returns [`Type::Platform`] (lowers to
    /// `buff_chat::Platform::Telegram`). Mirrors [`Self::Discord`].
    Telegram,
}

impl PreludeAssocConst {
    /// All recognised associated-constant names.
    pub const ALL: &'static [PreludeAssocConst] = &[
        PreludeAssocConst::Pi,
        PreludeAssocConst::E,
        // T47: Platform enum variants — accessed as `Platform.Discord` /
        // `Platform.Telegram` (zero-arg). Dispatched on the
        // (Platform, Discord) / (Platform, Telegram) pairs in
        // `assoc_const_return_type`.
        PreludeAssocConst::Discord,
        PreludeAssocConst::Telegram,
    ];

    /// The source name of this associated constant (the identifier the user
    /// writes after the dot). Note constant names are UPPERCASE per the
    /// Rust / Buff convention for consts.
    pub const fn name(self) -> &'static str {
        match self {
            // `PI` / `E` match Rust's `std::f64::consts::PI` / `E`
            // exactly so the codegen can splice `std::f64::consts::PI`
            // without rewriting.
            PreludeAssocConst::Pi => "PI",
            PreludeAssocConst::E => "E",
            // T47: PascalCase Rust enum-variant naming for the two
            // `Platform` variants — matches `buff_chat::Platform::Discord`
            // / `buff_chat::Platform::Telegram` 1:1 so the codegen can
            // splice the canonical Rust path without rewriting.
            PreludeAssocConst::Discord => "Discord",
            PreludeAssocConst::Telegram => "Telegram",
        }
    }
}

/// Look up a prelude associated constant by the (type, name) pair.
///
/// Returns `None` when the combination is not a recognised prelude
/// constant access (e.g. `Math.TAU` is invalid - TAU is not in the
/// T124f surface; `DateTime.PI` is invalid - PI belongs to Math).
/// This is the function the type inferencer + codegen consult to
/// decide whether a `Type.NAME` AST node (zero-arg method call) is a
/// prelude constant access.
pub fn assoc_const_lookup(type_name: &str, name: &str) -> Option<(PreludeType, PreludeAssocConst)> {
    let t = prelude_type_lookup(type_name)?;
    let c = PreludeAssocConst::ALL
        .iter()
        .copied()
        .find(|c| c.name() == name)?;
    // Validate the (type, const) pair is a recognised combination.
    assoc_const_return_type(t, c).map(|_| (t, c))
}

/// Infer the resolved Buff [`Type`] of a prelude associated constant.
///
/// Returns `None` for invalid `(type, const)` combinations. Currently
/// every associated constant is `Float` (Math.PI / Math.E both lower
/// to `std::f64::consts::PI` / `E` which are `f64`). Future
/// associated constants of other types (e.g. `Int`) extend this match.
pub fn assoc_const_return_type(type_: PreludeType, const_: PreludeAssocConst) -> Option<Type> {
    match (type_, const_) {
        // Math.PI / Math.E -> Float (f64).
        (PreludeType::Math, PreludeAssocConst::Pi) => Some(Type::float_default()),
        (PreludeType::Math, PreludeAssocConst::E) => Some(Type::float_default()),
        // T47: Platform.Discord / Platform.Telegram -> Platform. Lowers
        // to `buff_chat::Platform::Discord` / `::Telegram` at codegen
        // time. Both variants carry the same Buff type (`Platform`);
        // the codegen dispatches on the (Platform, Discord) /
        // (Platform, Telegram) pair to splice the matching Rust path.
        (PreludeType::Platform, PreludeAssocConst::Discord) => Some(Type::platform()),
        (PreludeType::Platform, PreludeAssocConst::Telegram) => Some(Type::platform()),
        // Every other (type, const) pair is invalid.
        _ => None,
    }
}


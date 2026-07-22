//! Parser edition selection (T57).
//!
//! An [`Edition`] selects which language extensions are accepted by the
//! parser. The default ([`Edition::Standard`]) is the historical Buff
//! language as documented through v1.18 — it has 100% backwards
//! compatibility. [`Edition::Scientific`] opts into the Julia-inspired
//! mathematical syntax extensions (T57): implicit multiplication (`2x`),
//! Unicode operators (`∑ ∏ √ ∈ ∉ ⊂ ≈ ≤ ≥ ≠ →`), matrix literals
//! (`[1 2; 3 4]`), and the adjoint/transpose postfix (`A'`).
//!
//! Editions are selected in `buff.toml` via the `edition` key:
//!
//! ```toml
//! edition = "scientific"
//! ```
//!
//! The CLI / pipeline reads the field and threads it into
//! [`parse_with_edition`](crate::parse_with_edition). The parser itself
//! never reads `buff.toml` — it receives the edition as a parameter, so the
//! edition decision stays in the build pipeline (single source of truth).
//!
//! # Hard contract: ASCII alternatives
//!
//! Every scientific-edition extension has an ASCII alternative. Buff never
//! forces users to type Unicode. The `∑` operator is exactly equivalent to
//! `sum(...)`, `√` to `sqrt(...)`, etc. This means a program written for
//! the scientific edition can ALWAYS be re-spelled in the standard edition
//! (and vice versa, modulo cosmetic readability).

use buff_lang_lexer::TokenKind;

/// Which language edition the parser should accept.
///
/// Default: [`Edition::Standard`] (backwards-compatible with all prior
/// Buff releases).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Edition {
    /// The default Buff language (v0.1 → v1.18 surface).
    ///
    /// All historical programs parse identically under this edition. Unicode
    /// mathematical operators (`∑ ∏ √ ∈ ∉ ⊂ ≈`) and the adjoint postfix (`'`
    /// after an expression) are REJECTED as parse errors with a helpful
    /// "set `edition = \"scientific\"` in buff.toml" note. The four Unicode
    /// comparison aliases (`≤ ≥ ≠ →`) ARE lexed into their existing ASCII
    /// token kinds regardless of edition, so they continue to work — they
    /// are pure spelling variants, not new syntax.
    #[default]
    Standard,
    /// The opt-in mathematical edition (T57, v1.19).
    ///
    /// Accepts every Standard-edition program PLUS the Julia-inspired
    /// extensions: implicit multiplication (`2x`, `2(x+y)`, `3sin(x)`),
    /// Unicode operators, matrix literals (`[1 2; 3 4]`), and the adjoint
    /// postfix (`A'` → `A.transpose()`).
    Scientific,
}

impl Edition {
    /// Returns `true` when this is [`Edition::Scientific`].
    pub fn is_scientific(self) -> bool {
        matches!(self, Self::Scientific)
    }

    /// Returns `Ok(())` if `kind` is acceptable under this edition, else a
    /// human-readable error message naming the missing edition opt-in.
    ///
    /// Used by the parser to gate scientific-edition-only tokens. The four
    /// Unicode comparison aliases (`≤ ≥ ≠ →`) are NOT gated here because
    /// they lex directly into the existing ASCII token kinds (`LtEq`,
    /// `GtEq`, `NotEq`, `Arrow`) — the parser never sees them as distinct
    /// tokens.
    pub fn require_for(self, kind: &TokenKind) -> Result<(), &'static str> {
        let needs_scientific = matches!(
            kind,
            TokenKind::Sum
                | TokenKind::Product
                | TokenKind::Sqrt
                | TokenKind::InUni
                | TokenKind::NotInUni
                | TokenKind::SubsetUni
                | TokenKind::ApproxUni
                | TokenKind::Adjoint
        );
        if needs_scientific && !self.is_scientific() {
            return Err(
                "this syntax requires `edition = \"scientific\"` in buff.toml \
                 (T57 mathematical syntax edition)",
            );
        }
        Ok(())
    }
}

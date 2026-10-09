//! ITER-40 / ITER-43 - T124 stdlib prelude lowering, INSTANCE-method +
//! associated-CONSTANT seam (mechanically extracted from rust_codegen.rs,
//! then split in half along the PreludeInstanceFn match family order).
//!
//! Layout since ITER-43:
//! - THIS file: the two pub(super) entry points (their only call sites
//!   are in the method_call_lowering sibling, after the registry
//!   lookups) + the `mod` declarations for the arm-table children.
//!   lower_prelude_type_assoc_const (T124f const table) stays whole here;
//!   lower_prelude_type_instance_fn lowers `recv` ONCE (lower_expr
//!   allocates temps) and delegates the ~170-arm match.
//! - prelude_lowering/instance_fns.rs: first half of the arms (DateTime
//!   .. ChatMessage/Platform), ordered wildcard-delegation into the extra
//!   sibling.
//! - prelude_lowering/instance_fns_extra.rs: second half of the arms
//!   (Faker .. RsaKeypair) + the `one_arg` arity closure (every call site
//!   lives in that half) + the T31 gap-fill wildcard fallback.
//!
//! The delegation chain preserves the ORIGINAL arm order: every guard
//! above the seam is a pure `matches!(recv_ty, ..)` test, so "no arm
//! matched" is side-effect-free and the extra sibling simply continues
//! the same match. The prelude_types sibling (ITER-31) keeps the
//! associated-FUNCTION table (lower_prelude_type_assoc_fn); the
//! prelude_fns sibling (ITER-34) keeps the free-fn dispatch
//! (lower_prelude_call). rust_codegen.rs still declares only
//! `mod prelude_lowering;` (inherent methods resolve by type, no `use`
//! needed). Children inherit parent imports via use super::* and may
//! call the parent private methods (descendant privacy).

use super::*;

mod instance_fns;
mod instance_fns_extra;

impl RustCodegen {
    /// T124f: lower a prelude-type associated-CONSTANT access
    /// (`Type.NAME`) to the corresponding Rust path.
    ///
    /// Dispatched from [`Self::lower_method_call`] when the receiver is
    /// a bare Ident naming a prelude type with a registered associated
    /// constant (currently only `Math.PI` / `Math.E`). The lowering is
    /// a fully-qualified Rust path so generated code requires no `use`
    /// import (mirrors the chrono / regex / toml fully-qualified-path
    /// pattern).
    ///
    /// # Lowering table
    ///
    /// | Buff source  | Generated Rust              |
    /// |--------------|------------------------------|
    /// | `Math.PI`    | `std::f64::consts::PI`      |
    /// | `Math.E`     | `std::f64::consts::E`       |
    ///
    /// Both lower to `f64` consts from Rust's `std::f64::consts` module
    /// - NO extern crate needed (Math uses only Rust `std`).
    ///
    /// Built via `rust_call_expr`'s path machinery... wait, that
    /// produces a `path(args)` CALL. We need a bare PATH (no parens).
    /// The simplest path is `syn::parse_str` -> `syn::ExprPath` (any
    /// `::`-separated path string parses cleanly as a path expression).
    /// That's already the pattern used in [`lower_graphemes_call`] for
    /// the `unicode_segmentation::UnicodeSegmentation::graphemes` path
    /// fragment. We reuse it here for consistency.
    pub(super) fn lower_prelude_type_assoc_const(
        &mut self,
        ptype: buff_lang_types::PreludeType,
        pconst: buff_lang_types::PreludeAssocConst,
    ) -> Result<SynExpr, CodegenError> {
        use buff_lang_types::{PreludeAssocConst as C, PreludeType as T};
        let path: &str = match (ptype, pconst) {
            // `Math.PI` / `Math.E` -> `std::f64::consts::PI` / `E`.
            // Both Rust consts are `f64`; the codegen-lowered path is
            // fully-qualified so the generated crate needs no `use
            // std::f64::consts;` import.
            (T::Math, C::Pi) => "std::f64::consts::PI",
            (T::Math, C::E) => "std::f64::consts::E",
            // T47: `Platform.Discord` / `Platform.Telegram` ->
            // `buff_chat::Platform::Discord` / `::Telegram`. Both
            // variants are `Copy` enum units; the codegen-lowered path
            // is fully-qualified so the generated crate needs no `use
            // buff_chat::Platform;` import. Mirrors the Math const
            // lowering shape (zero-arg `Type.NAME` access).
            (T::Platform, C::Discord) => "buff_chat::Platform::Discord",
            (T::Platform, C::Telegram) => "buff_chat::Platform::Telegram",
            // Every other combination was already rejected by
            // `assoc_const_lookup` in the caller; this arm is
            // unreachable but required for exhaustiveness.
            _ => {
                return Err(self.unsupported(&format!(
                    "prelude type+const combination {:?}.{:?}",
                    ptype, pconst
                )));
            }
        };
        syn::parse_str::<SynExpr>(path)
            .map_err(|e| self.unsupported(&format!("Math const codegen parse ({path}): {e}")))
    }

    /// T124b: lower a prelude-type instance-method call (`recv.method(args)`)
    /// to the corresponding chrono / std::time Rust idiom.
    ///
    /// Dispatched from [`Self::lower_method_call`] when the receiver's
    /// inferred type is one of the prelude datetime family AND the method
    /// name is a recognised instance method on that type.
    ///
    /// # Lowering table
    ///
    /// | Buff source           | Generated Rust                                  |
    /// |-----------------------|-------------------------------------------------|
    /// | `dt.format("%Y-%m-%d")` | `dt.format("%Y-%m-%d").to_string()`           |
    /// | `dt.year()`           | `dt.year()`  (i32 → promoted to i64 by annotation) |
    /// | `dt.month()`          | `dt.month()`                                    |
    /// | `dt.day()`            | `dt.day()`                                      |
    /// | `dt.hour()`           | `dt.hour()`                                     |
    /// | `dt.minute()`         | `dt.minute()`                                   |
    /// | `dt.second()`         | `dt.second()`                                   |
    /// | `dt.timestamp()`      | `dt.timestamp()`                                |
    ///
    /// `format` returns `chrono::DelayedFormat<...>`, which doesn't impl
    /// `Into<String>` directly — we chain `.to_string()` so the result is
    /// a real Rust `String` (Display-able via `.to_string()`). The other
    /// accessors return `i32` / `i64` which Rust coerces implicitly when
    /// the surrounding context expects `i64`.
    pub(super) fn lower_prelude_type_instance_fn(
        &mut self,
        recv_ty: &Type,
        pmethod: buff_lang_types::PreludeInstanceFn,
        receiver: &Expr,
        args: &[Expr],
    ) -> Result<SynExpr, CodegenError> {
        // ITER-43: the ~170-arm match moved verbatim into the
        // instance_fns / instance_fns_extra child modules (ordered
        // wildcard-delegation chain). `recv` is lowered ONCE here and
        // threaded down by value - lower_expr allocates temps, so
        // per-half lowering would change generated temp names.
        let recv = self.lower_expr(receiver)?;
        self.lower_prelude_type_instance_fn_arms(recv_ty, pmethod, recv, args)
    }
}

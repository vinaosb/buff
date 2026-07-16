//! Move analysis for Rust codegen (T33a + T33).
//!
//! Buff uses **move-by-default** semantics: every binding is MOVED into its
//! consumer (Rust move semantics). When a variable is used AFTER being
//! moved, codegen automatically inserts `.clone()`.
//!
//! ## Algorithm
//!
//! 1. [`MoveAnalyzer::preanalyze_func`] runs the pure
//!    [`buff_lang_types::analyze_ownership`] pass over the function body
//!    to derive [`OwnershipFacts`] — which bindings are `Copy`, which are
//!    Arc-shared across `spawn`, and which Arc-shared bindings are
//!    subsequently mutated (CoW sites).
//! 2. During lowering, [`MoveAnalyzer::needs_clone`] is called for every
//!    `Expr::Ident`. It returns `true` if the variable is non-Copy AND has
//!    already been moved once before in the current function. The first
//!    use is the move itself (no clone); the second+ use gets `.clone()`.
//! 3. Codegen consults [`MoveAnalyzer::is_arc_var`] /
//!    [`MoveAnalyzer::is_arc_mut_var`] when lowering `let` bindings
//!    (emit `Arc::new(...)`), spawn-body ident uses (emit
//!    `Arc::clone(&x)`), and assignment targets (emit
//!    `Arc::make_mut(&mut x)`).
//!
//! ## What moved in T33 (vs T33a)
//!
//! - **Char is now Copy** — the v0.1 classifier knew Int/Float/Double/
//!   Bool/Byte/Bits but missed Char (added to the language in T21).
//!   All Copy decisions now live in [`OwnershipFacts`] for reuse by
//!   downstream tooling (LSP, refactors).
//! - **Arc-across-spawn** — the analysis flags non-Copy bindings
//!   captured by `spawn <task>`; codegen wraps them in `Arc::new(...)`
//!   and inserts `Arc::clone(&x)` inside the spawn body.
//! - **CoW mutation** — Arc-shared bindings that are subsequently
//!   mutated get `Arc::make_mut(&mut x)` at the assignment site.
//! - **REFACTOR** — the (formerly inline) Copy classification moved
//!   into [`buff_lang_types::ownership`] (a pure, deterministic pass
//!   reusable beyond codegen). The stateful move counter stays here
//!   because it is inherently tied to lowering order.
//!
//! ## Remaining limitations
//!
//! - **Reassignment**: `s = "new"` does NOT reset the use counter. The
//!   second use after reassignment may spuriously insert `.clone()`.
//!   Documented by the `#[ignore]` test `test_reassignment_resets_counter_limitation`;
//!   acknowledged as a v0.5 limitation, deferred to v1.0.
//! - **Cross-scope tracking**: variables in nested scopes are treated
//!   uniformly. May over-insert clones in rare cases.
//! - **Shadowing**: a rebinding that changes Copy-ness is not tracked.
//!   The first classification "sticks" for the lifetime of the name.

use buff_lang_ast::FuncDecl;
use buff_lang_types::OwnershipFacts;

/// Tracks which variables are "moved" (consumed) at each point in the code.
///
/// When a variable is used after being moved, codegen inserts `.clone()`.
/// Owned by [`crate::RustCodegen`](crate::RustCodegen) and reset between
/// functions.
///
/// # State
///
/// - `used`: per-name use counter, incremented on every `needs_clone` call.
///   First use of a non-Copy var = move (no clone); second+ use = clone.
///   Kept as a [`std::collections::BTreeMap`] so snapshot output is
///   deterministic (the T29 flaky-test lesson — never rely on HashMap
///   iteration order for codegen output).
/// - `facts`: the per-function [`OwnershipFacts`] produced by
///   [`buff_lang_types::analyze_ownership`]. Carries the Copy / Arc /
///   CoW classifications that drive codegen decisions outside the
///   `needs_clone` counter.
pub struct MoveAnalyzer {
    used: std::collections::BTreeMap<String, u32>,
    facts: OwnershipFacts,
}

impl MoveAnalyzer {
    /// Create a fresh analyzer with empty state.
    pub fn new() -> Self {
        Self {
            used: std::collections::BTreeMap::new(),
            facts: OwnershipFacts::default(),
        }
    }

    /// Analyze a function body, pre-computing ownership facts (Copy /
    /// Arc-shared / CoW-mutated bindings).
    ///
    /// Call this BEFORE lowering statements (typically at the top of
    /// [`crate::RustCodegen::lower_func`]). The resulting facts are
    /// consulted via [`Self::needs_clone`], [`Self::is_arc_var`], and
    /// [`Self::is_arc_mut_var`] during lowering.
    pub fn preanalyze_func(&mut self, func: &FuncDecl) {
        // T33: delegate the (pure, deterministic) classification to
        // buff_lang_types::analyze_ownership. It returns a fresh
        // OwnershipFacts each call; assigning here replaces any prior
        // state (so a stale set from the previous function can't leak in).
        self.facts = buff_lang_types::analyze_ownership(func);
    }

    /// Borrow the ownership facts (read-only) for codegen decisions.
    ///
    /// Exposed so the codegen pass can query [`OwnershipFacts::is_copy`]
    /// / `is_arc` / `is_arc_mut` directly when lowering `let` bindings,
    /// spawn bodies, and assignment targets.
    pub fn facts(&self) -> &OwnershipFacts {
        &self.facts
    }

    /// Is `name` a Copy binding (Int/Float/Double/Bool/Byte/Bits/Char)?
    ///
    /// Copy bindings never get `.clone()` (Rust copies them). Exposed as
    /// a convenience wrapper around [`OwnershipFacts::is_copy`].
    pub fn is_copy_var(&self, name: &str) -> bool {
        self.facts.is_copy(name)
    }

    /// Is `name` an Arc-shared binding (captured across a `spawn`)?
    ///
    /// Codegen wraps Arc-shared bindings' definitions in `Arc::new(...)`
    /// and inserts `Arc::clone(&x)` at spawn-body use sites.
    pub fn is_arc_var(&self, name: &str) -> bool {
        self.facts.is_arc(name)
    }

    /// Is `name` Arc-shared AND subsequently mutated (needs `Arc::make_mut`)?
    ///
    /// Codegen emits `Arc::make_mut(&mut x)` at the assignment site,
    /// giving copy-on-write semantics.
    pub fn is_arc_mut_var(&self, name: &str) -> bool {
        self.facts.is_arc_mut(name)
    }

    /// Check if a variable usage at this point should emit `.clone()`.
    ///
    /// Returns `true` if `.clone()` should be inserted. Specifically:
    ///
    /// - Copy types → always `false` (they are copied, not moved).
    /// - Non-Copy types: first use → `false` (the move); second+ use →
    ///   `true` (insert `.clone()` so the move is valid).
    ///
    /// **Side effect**: increments the use counter for `var_name` unless
    /// the variable is known Copy.
    ///
    /// **Arc-shared bindings**: still go through this counter. Codegen
    /// additionally suppresses `.clone()` for arc-vars when emitting an
    /// `Arc::clone(&x)` instead inside spawn bodies (handled at the
    /// codegen call site, not here).
    pub fn needs_clone(&mut self, var_name: &str) -> bool {
        if self.facts.is_copy(var_name) {
            return false;
        }
        let count = self.used.entry(var_name.to_string()).or_insert(0);
        *count += 1;
        *count > 1
    }

    /// Reset all state for a new function.
    ///
    /// Clears both the use counters and the ownership facts.
    pub fn reset(&mut self) {
        self.used.clear();
        self.facts = OwnershipFacts::default();
    }
}

impl Default for MoveAnalyzer {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use buff_lang_ast::common::{Block, Ident, Param};
    use buff_lang_ast::{Expr, Literal, Stmt, TypeRef};
    use buff_lang_error::Span;

    fn span() -> Span {
        Span::dummy()
    }

    fn ident_expr(s: &str) -> Expr {
        Expr::Ident(Ident::new(s, span()), span())
    }

    fn int_expr(n: i64) -> Expr {
        Expr::Literal(Literal::Int(n), span())
    }

    fn char_expr(c: char) -> Expr {
        Expr::Literal(Literal::Char(c), span())
    }

    fn string_expr(s: &str) -> Expr {
        Expr::Literal(Literal::String(s.to_string()), span())
    }

    fn let_stmt(name: &str, value: Expr) -> Stmt {
        Stmt::LetDecl {
            name: Ident::new(name, span()),
            value,
            mutable: false,
            ty: None,
            span: span(),
        }
    }

    fn func_with_stmts(stmts: Vec<Stmt>) -> FuncDecl {
        FuncDecl {
            name: Ident::new("f", span()),
            params: Vec::new(),
            return_type: None,
            body: Block {
                stmts,
                span: span(),
            },
            is_async: false,
            is_unsafe: false,
            is_extern: false,
            attributes: Vec::new(),
            span: span(),
        }
    }

    fn named_type(s: &str) -> TypeRef {
        TypeRef::Named {
            name: Ident::new(s, span()),
            span: span(),
        }
    }

    #[test]
    fn copy_int_var_never_needs_clone() {
        let f = func_with_stmts(vec![
            let_stmt("x", int_expr(42)),
            let_stmt("y", ident_expr("x")),
            let_stmt("z", ident_expr("x")),
        ]);
        let mut a = MoveAnalyzer::new();
        a.preanalyze_func(&f);
        // x is Copy — never needs clone, no matter how many uses.
        assert!(!a.needs_clone("x"));
        assert!(!a.needs_clone("x"));
        assert!(!a.needs_clone("x"));
    }

    #[test]
    fn copy_char_var_never_needs_clone() {
        // T33: Char is now Copy (it was added to the language in T21 but
        // the v0.1 MoveAnalyzer missed it).
        let f = func_with_stmts(vec![
            let_stmt("c", char_expr('A')),
            let_stmt("c2", ident_expr("c")),
            let_stmt("c3", ident_expr("c")),
        ]);
        let mut a = MoveAnalyzer::new();
        a.preanalyze_func(&f);
        assert!(!a.needs_clone("c"));
        assert!(!a.needs_clone("c"));
        assert!(a.is_copy_var("c"));
    }

    #[test]
    fn non_copy_string_first_use_no_clone() {
        let f = func_with_stmts(vec![let_stmt("s", string_expr("hi"))]);
        let mut a = MoveAnalyzer::new();
        a.preanalyze_func(&f);
        // First use of s — no clone.
        assert!(!a.needs_clone("s"));
    }

    #[test]
    fn non_copy_string_second_use_needs_clone() {
        let f = func_with_stmts(vec![let_stmt("s", string_expr("hi"))]);
        let mut a = MoveAnalyzer::new();
        a.preanalyze_func(&f);
        assert!(!a.needs_clone("s")); // first use = move, no clone
        assert!(a.needs_clone("s")); // second use = clone
        assert!(a.needs_clone("s")); // third use = clone
    }

    #[test]
    fn reset_clears_state() {
        let mut a = MoveAnalyzer::new();
        a.needs_clone("foo"); // count: 1
        a.needs_clone("foo"); // count: 2
        a.reset();
        // After reset, first use is no-clone again.
        assert!(!a.needs_clone("foo"));
    }

    #[test]
    fn copy_propagates_through_let_chain() {
        // let x = 42; let y = x; — both should be Copy (x via literal,
        // y via x which is already known Copy).
        let f = func_with_stmts(vec![
            let_stmt("x", int_expr(42)),
            let_stmt("y", ident_expr("x")),
        ]);
        let mut a = MoveAnalyzer::new();
        a.preanalyze_func(&f);
        assert!(!a.needs_clone("x"));
        assert!(!a.needs_clone("y"));
    }

    #[test]
    fn param_with_int_type_is_copy() {
        let mut f = func_with_stmts(vec![]);
        f.params.push(Param {
            name: Ident::new("n", span()),
            ty: named_type("Int"),
            span: span(),
        });
        let mut a = MoveAnalyzer::new();
        a.preanalyze_func(&f);
        // n is an Int param — Copy.
        assert!(!a.needs_clone("n"));
        assert!(!a.needs_clone("n"));
    }

    #[test]
    fn param_with_char_type_is_copy() {
        // T33: Char param is Copy.
        let mut f = func_with_stmts(vec![]);
        f.params.push(Param {
            name: Ident::new("c", span()),
            ty: named_type("Char"),
            span: span(),
        });
        let mut a = MoveAnalyzer::new();
        a.preanalyze_func(&f);
        assert!(!a.needs_clone("c"));
        assert!(a.is_copy_var("c"));
    }

    #[test]
    fn param_with_string_type_is_not_copy() {
        let mut f = func_with_stmts(vec![]);
        f.params.push(Param {
            name: Ident::new("s", span()),
            ty: named_type("String"),
            span: span(),
        });
        let mut a = MoveAnalyzer::new();
        a.preanalyze_func(&f);
        assert!(!a.needs_clone("s")); // first use
        assert!(a.needs_clone("s")); // second use
    }

    #[test]
    fn unbound_var_is_treated_as_non_copy() {
        // A variable that was never declared still goes through
        // needs_clone. It should be treated as non-Copy (the safe default).
        let mut a = MoveAnalyzer::new();
        assert!(!a.needs_clone("unknown")); // first use, no clone
        assert!(a.needs_clone("unknown")); // second use, clone
    }

    #[test]
    fn double_and_bool_literals_are_copy() {
        let f = func_with_stmts(vec![
            let_stmt("d", Expr::Literal(Literal::Double(2.5), span())),
            let_stmt("b", Expr::Literal(Literal::Bool(true), span())),
        ]);
        let mut a = MoveAnalyzer::new();
        a.preanalyze_func(&f);
        assert!(!a.needs_clone("d"));
        assert!(!a.needs_clone("b"));
    }
}

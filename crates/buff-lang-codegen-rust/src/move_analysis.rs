//! Move analysis for Rust codegen.
//!
//! Buff uses **move-by-default** semantics: every binding is MOVED into its
//! consumer (Rust move semantics). When a variable is used AFTER being
//! moved, codegen automatically inserts `.clone()`.
//!
//! For Copy types (`Int`, `Float`, `Double`, `Bool`, `Byte`, `Bits`) no
//! clone is ever needed — Rust copies them. For non-Copy types (`String`,
//! future `Vec`/`Struct`) the first use is a move and any subsequent use
//! gets `.clone()` inserted at the move site.
//!
//! ## Algorithm
//!
//! 1. [`MoveAnalyzer::preanalyze_func`] walks a function body ONCE to
//!    classify which bindings are Copy. A binding is Copy if:
//!    - its declared type is a primitive numeric/bool type, OR
//!    - its initializer is a primitive literal, OR
//!    - its initializer is another known-Copy variable.
//! 2. During lowering, [`MoveAnalyzer::needs_clone`] is called for every
//!    `Expr::Ident`. It returns `true` if the variable is non-Copy AND has
//!    already been moved once before in the current function.
//!
//! ## Limitations (v0.1)
//!
//! - **Shadowing**: a rebinding that changes Copy-ness is not tracked.
//!   The first classification "sticks" for the lifetime of the name.
//! - **Reassignment**: `s = "new"` does NOT reset the use counter. The
//!   second use after reassignment may spuriously insert `.clone()`.
//!   Acknowledged as a v0.1 limitation; T33b (v1.0) will address it.
//! - **Cross-scope tracking**: variables in nested scopes are treated
//!   uniformly. May over-insert clones in rare cases.

use std::collections::{HashMap, HashSet};

use buff_lang_ast::{Expr, FuncDecl, Literal, Stmt, TypeRef};

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
/// - `copy_vars`: names known to be `Copy` (Int/Float/Double/Bool/Byte/Bits).
///   `needs_clone` always returns `false` for these.
pub struct MoveAnalyzer {
    used: HashMap<String, u32>,
    copy_vars: HashSet<String>,
}

impl MoveAnalyzer {
    /// Create a fresh analyzer with empty state.
    pub fn new() -> Self {
        Self {
            used: HashMap::new(),
            copy_vars: HashSet::new(),
        }
    }

    /// Analyze a function body, pre-computing which variables are Copy.
    ///
    /// Call this BEFORE lowering statements (typically at the top of
    /// [`crate::RustCodegen::lower_func`]). Idempotent within a function —
    /// safe to call more than once, but the second call is a no-op for
    /// names already classified.
    pub fn preanalyze_func(&mut self, func: &FuncDecl) {
        // Classify parameters based on their declared TypeRef.
        for p in &func.params {
            if self.is_copy_typeref(&p.ty) {
                self.copy_vars.insert(p.name.name.clone());
            }
        }
        // Classify let-bound variables based on their initializer expr.
        for stmt in &func.body.stmts {
            self.classify_stmt(stmt);
        }
    }

    fn classify_stmt(&mut self, stmt: &Stmt) {
        if let Stmt::LetDecl { name, value, .. } = stmt {
            if self.is_copy_expr(value) {
                self.copy_vars.insert(name.name.clone());
            }
        }
    }

    fn is_copy_typeref(&self, ty: &TypeRef) -> bool {
        match ty {
            TypeRef::Named { name, .. } => matches!(
                name.name.as_str(),
                "Int" | "Float" | "Double" | "Bool" | "Byte" | "Bits"
            ),
            _ => false,
        }
    }

    fn is_copy_expr(&self, expr: &Expr) -> bool {
        match expr {
            Expr::Literal(lit, _) => matches!(
                lit,
                Literal::Int(_)
                    | Literal::Float(_)
                    | Literal::Double(_)
                    | Literal::Bool(_)
                    | Literal::Byte(_)
            ),
            Expr::Ident(name, _) => self.copy_vars.contains(&name.name),
            _ => false,
        }
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
    pub fn needs_clone(&mut self, var_name: &str) -> bool {
        if self.copy_vars.contains(var_name) {
            return false;
        }
        let count = self.used.entry(var_name.to_string()).or_insert(0);
        *count += 1;
        *count > 1
    }

    /// Reset all state for a new function.
    ///
    /// Clears both the use counters and the Copy-var classification.
    pub fn reset(&mut self) {
        self.used.clear();
        self.copy_vars.clear();
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

//! Ownership analysis for Buff programs (T33).
//!
//! Buff uses **move-by-default** semantics (Rust move semantics) but hides
//! the borrow-checker from the user. The codegen pass consults the facts
//! produced here to decide where to insert:
//!
//! - `.clone()` — when a non-Copy binding is used after being moved.
//! - `Arc::new(...)` — at the definition of a non-Copy binding captured
//!   across a `spawn` boundary (so the spawned task can grab a cheap
//!   `Arc::clone(&x)` rather than moving or deep-cloning the data).
//! - `Arc::clone(&x)` — at the use site of an Arc-wrapped binding INSIDE
//!   a spawn body.
//! - `Arc::make_mut(&mut x)` — at the mutation site of Arc-shared data
//!   (copy-on-write: clones the inner value if the refcount > 1).
//!
//! ## Scope (v0.5)
//!
//! The analysis is performed **per function**. It is a pure,
//! deterministic (BTreeSet-based — see the T29 flaky-test lesson)
//! function of the AST: same AST → same [`OwnershipFacts`] every time.
//!
//! ## Algorithm
//!
//! 1. **Copy classification** — walk the function body in source order,
//!    marking each `let`-bound name (and each parameter) as Copy iff:
//!    - its declared [`TypeRef`] is one of the Copy primitives
//!      (`Int`/`Float`/`Double`/`Bool`/`Byte`/`Bits`/`Char`), OR
//!    - its initializer is a literal of a Copy kind, OR
//!    - its initializer is another known-Copy variable.
//!
//!    Order matters: `let y = x` propagates `x`'s Copy-ness forward.
//! 2. **Arc-shared detection** — for each `Expr::Spawn` anywhere in the
//!    function body, collect the free-variable [`Ident`]s read inside the
//!    spawned task. A variable that is (a) a local/param, AND (b) non-Copy,
//!    AND (c) referenced inside a spawn body, is added to `arc_vars`.
//! 3. **CoW mutation detection** — for each `Stmt::Assignment` whose
//!    target is a bare `Expr::Ident` naming an Arc-shared variable, add
//!    the name to `arc_mut_vars`. Codegen will then emit
//!    `Arc::make_mut(&mut x)` at the LHS.
//!
//! ## Limitations (v0.5)
//!
//! - **Cross-scope tracking**: nested scopes (if/for/match arms) are
//!   flattened — a binding defined inside an `if` branch is treated as
//!   visible function-wide. This may over-Arc-wrap in rare cases; safe
//!   (Arc is sound) but slightly less precise.
//! - **Reassignment**: the move counter inside [`MoveAnalyzer`] does NOT
//!   reset on reassignment. Documented v0.1 limitation; the existing
//!   `#[ignore]` test `test_reassignment_resets_counter_limitation`
//!   still documents this. T33 does not change that behaviour.
//! - **Multiple spawns of the same var**: handled correctly — Arc-wrap
//!   happens once at the definition; each spawn captures its own
//!   `Arc::clone`.
//!
//! [`MoveAnalyzer`]: buff_lang_codegen_rust::MoveAnalyzer

use std::collections::BTreeSet;

use buff_lang_ast::{Block, Expr, FuncDecl, Literal, Stmt, TypeRef};

/// Ownership facts derived for a single function.
///
/// Produced by [`analyze_func`]. All three sets carry binding NAMES
/// (strings) so the codegen pass can do cheap `O(log n)` membership
/// queries during lowering.
///
/// # Determinism
///
/// All fields are [`BTreeSet`]s — iteration order is sorted, so two
/// runs of [`analyze_func`] on the same AST produce byte-identical
/// [`OwnershipFacts`]. The codegen pass relies on this for
/// snapshot-stable output (the T29 flaky-test lesson).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct OwnershipFacts {
    /// Names of bindings whose type is `Copy` (Int/Float/Double/Bool/Byte/
    /// Bits/Char — Rust copies them, so `.clone()` is never inserted).
    ///
    /// Populated from parameter [`TypeRef`]s and `let`-initializer
    /// classification (literal kind or known-Copy var chain).
    pub copy_vars: BTreeSet<String>,
    /// Names of non-Copy bindings captured across a `spawn` boundary.
    ///
    /// Codegen wraps their definition in `Arc::new(...)` so the spawned
    /// closure can grab `Arc::clone(&x)` (a refcount bump) rather than
    /// moving (which would break later uses) or deep-cloning the data.
    pub arc_vars: BTreeSet<String>,
    /// Subset of [`arc_vars`](Self::arc_vars) that are subsequently
    /// mutated via an assignment whose target is the bare ident.
    ///
    /// Codegen emits `Arc::make_mut(&mut x)` at the assignment site,
    /// giving copy-on-write semantics: the inner value is cloned only
    /// when the refcount is > 1 (i.e. when the spawn is actually
    /// observing the same Arc).
    pub arc_mut_vars: BTreeSet<String>,
}

impl OwnershipFacts {
    /// Convenience predicate: is `name` a Copy binding?
    pub fn is_copy(&self, name: &str) -> bool {
        self.copy_vars.contains(name)
    }

    /// Convenience predicate: is `name` Arc-shared across a spawn?
    pub fn is_arc(&self, name: &str) -> bool {
        self.arc_vars.contains(name)
    }

    /// Convenience predicate: is `name` Arc-shared AND mutated (needs
    /// `Arc::make_mut`)?
    pub fn is_arc_mut(&self, name: &str) -> bool {
        self.arc_mut_vars.contains(name)
    }
}

/// Analyze a function body and return its [`OwnershipFacts`].
///
/// Pure and deterministic — same AST → same facts, every time. The
/// codegen pass calls this once per function (at the top of
/// `lower_func`) and consults the resulting [`OwnershipFacts`]
/// throughout lowering.
///
/// # Walk order
///
/// Copy classification walks `let` statements in source order so an
/// ident initializer (`let y = x`) can see the Copy-ness of an
/// already-classified prior binding (`let x = 42`). The arc/mut passes
/// are order-independent (set unions).
pub fn analyze_func(func: &FuncDecl) -> OwnershipFacts {
    let mut facts = OwnershipFacts::default();
    // Locals = parameters + let-bound names. Used to filter spawn-captured
    // free vars so we don't try to Arc-wrap globals/prelude-fn names that
    // happen to appear inside a spawn body.
    let mut locals: BTreeSet<String> = BTreeSet::new();

    // 1. Copy classification (params first, then lets in source order).
    for p in &func.params {
        if is_copy_typeref(&p.ty) {
            facts.copy_vars.insert(p.name.name.clone());
        }
        locals.insert(p.name.name.clone());
    }
    classify_stmts(&func.body.stmts, &mut facts.copy_vars, &mut locals);

    // 2. Arc-shared detection: scan for `spawn <task>` and collect the
    //    non-Copy local idents referenced inside the task body.
    let mut spawn_free_vars: BTreeSet<String> = BTreeSet::new();
    collect_spawn_free_vars_in_stmts(&func.body.stmts, &mut spawn_free_vars);
    for v in spawn_free_vars {
        // Only locals (not globals/prelude) and only non-Copy get Arc-wrapped.
        if locals.contains(&v) && !facts.copy_vars.contains(&v) {
            facts.arc_vars.insert(v);
        }
    }

    // 3. CoW mutation: any assignment whose target is a bare ident naming
    //    an arc_var is a CoW mutation site.
    let mut assigned: BTreeSet<String> = BTreeSet::new();
    collect_assignment_targets_in_stmts(&func.body.stmts, &mut assigned);
    for v in assigned {
        if facts.arc_vars.contains(&v) {
            facts.arc_mut_vars.insert(v);
        }
    }

    facts
}

// ---------------------------------------------------------------------------
// Closure capture analysis (T34) — SHARED with T33's spawn free-var walker
// ---------------------------------------------------------------------------

/// Compute the set of variable NAMES **captured** by a closure (T34).
///
/// A captured variable is a free variable read inside the closure `body`
/// that is neither:
/// - a **closure parameter** (`params`), nor
/// - a **let-binding local to the closure body** (including nested-closure
///   params and `for` loop variables).
///
/// These are the variables the closure "closes over" from its enclosing
/// scope. Rust handles the actual capture (by reference or by move)
/// automatically when it sees `|params| body`; Buff's job is only to
/// **identify** them so the codegen pass can avoid inserting spurious
/// `.clone()` calls for captured variables inside the closure body
/// (which would compile but be wasteful and semantically wrong for
/// multi-use captures).
///
/// # Shared with T33
///
/// This reuses the SAME free-variable walker (`collect_free_vars_in_expr` /
/// `collect_free_vars_in_block`) that T33's spawn-capture detection uses.
/// Both T33 and T34 need "which variables does this sub-expression read
/// from its enclosing scope?"; T33 intersects with function-level `locals`
/// and filters Copy vars to find Arc-wrap candidates, while T34 subtracts
/// closure-local bindings to find captures. The walker itself is shared;
/// only the post-processing differs.
///
/// # Determinism
///
/// Returns a [`BTreeSet`] — sorted, so same closure → same capture set
/// every time (the T29 flaky-test lesson). The codegen pass relies on
/// this for deterministic snapshot output.
///
/// # Example
///
/// ```text
/// closure_captures(params=[x], body={ x + f })  =>  { "f" }
/// closure_captures(params=[x,y], body={ x + y }) =>  { }  (no captures)
/// closure_captures(params=[x], body={ let y = 1; x + y + g }) => { "g" }
/// ```
pub fn closure_captures(params: &[buff_lang_ast::common::Param], body: &Block) -> BTreeSet<String> {
    // 1. Collect every ident NAME read inside the body (free-variable walk).
    //    This is the same walker T33 uses for spawn-capture detection.
    let mut used: BTreeSet<String> = BTreeSet::new();
    collect_free_vars_in_block(body, &mut used);
    // 2. Collect names BOUND inside the closure: params + local lets +
    //    for-loop vars + nested-closure params. These are NOT captures.
    let mut bound: BTreeSet<String> = BTreeSet::new();
    for p in params {
        bound.insert(p.name.name.clone());
    }
    collect_bound_names_in_block(body, &mut bound);
    // 3. Captures = used minus bound (deterministic via BTreeSet).
    used.into_iter().filter(|n| !bound.contains(n)).collect()
}

/// Walk a block's statements collecting names bound by `let` / `for` /
/// nested-closure params (T34). These are local to the closure body and
/// therefore NOT captures.
fn collect_bound_names_in_block(block: &Block, out: &mut BTreeSet<String>) {
    for stmt in &block.stmts {
        collect_bound_names_in_stmt(stmt, out);
    }
}

fn collect_bound_names_in_stmt(stmt: &Stmt, out: &mut BTreeSet<String>) {
    match stmt {
        Stmt::LetDecl { name, value, .. } => {
            out.insert(name.name.clone());
            collect_bound_names_in_expr(value, out);
        }
        // T71: every destructured binding is a new local name.
        Stmt::LetPattern { pattern, value, .. } => {
            for b in pattern.bindings() {
                out.insert(b.name);
            }
            collect_bound_names_in_expr(value, out);
        }
        Stmt::Assignment { value, .. } => collect_bound_names_in_expr(value, out),
        Stmt::ExprStmt(e, _) => collect_bound_names_in_expr(e, out),
        Stmt::Return(Some(e), _) => collect_bound_names_in_expr(e, out),
        Stmt::Return(None, _) | Stmt::Break(_) | Stmt::Continue(_) => {}
        Stmt::ForIn {
            var, iter, body, ..
        } => {
            out.insert(var.name.clone());
            collect_bound_names_in_expr(iter, out);
            collect_bound_names_in_block(body, out);
        }
        Stmt::ForWhile { cond, body, .. } => {
            collect_bound_names_in_expr(cond, out);
            collect_bound_names_in_block(body, out);
        }
        // T72: every pattern binding is a new local name (loop-scoped).
        Stmt::ForLet {
            pattern,
            value,
            body,
            ..
        } => {
            for b in pattern.bindings() {
                out.insert(b.name);
            }
            collect_bound_names_in_expr(value, out);
            collect_bound_names_in_block(body, out);
        }
        // T73: `guard <conds> else { block }` — `let` conditions introduce
        // bindings in the ENCLOSING scope (let-else semantics); walk every
        // condition + the else-block.
        Stmt::Guard {
            conditions,
            else_block,
            ..
        } => {
            for c in conditions {
                if let buff_lang_ast::GuardCondition::Let { pattern, value, .. } = c {
                    for b in pattern.bindings() {
                        out.insert(b.name);
                    }
                    collect_bound_names_in_expr(value, out);
                } else if let buff_lang_ast::GuardCondition::Bool(e) = c {
                    collect_bound_names_in_expr(e, out);
                }
            }
            collect_bound_names_in_block(else_block, out);
        }
    }
}

/// Recursively scan `expr` for names bound by nested `let`s / closure
/// params / `for` loops (so they are excluded from the capture set of
/// the ENCLOSING closure).
fn collect_bound_names_in_expr(expr: &Expr, out: &mut BTreeSet<String>) {
    match expr {
        // A nested closure binds its own params (and its body's lets).
        Expr::Lambda { params, body, .. } => {
            for p in params {
                out.insert(p.name.name.clone());
            }
            collect_bound_names_in_block(body, out);
        }
        Expr::Literal(_, _) | Expr::Ident(_, _) => {}
        Expr::BinaryOp { lhs, rhs, .. } => {
            collect_bound_names_in_expr(lhs, out);
            collect_bound_names_in_expr(rhs, out);
        }
        Expr::UnaryOp { operand, .. } => collect_bound_names_in_expr(operand, out),
        Expr::FuncCall { callee, args, .. } => {
            collect_bound_names_in_expr(callee, out);
            for a in args {
                collect_bound_names_in_expr(a, out);
            }
        }
        Expr::MethodCall { receiver, args, .. } => {
            collect_bound_names_in_expr(receiver, out);
            for a in args {
                collect_bound_names_in_expr(a, out);
            }
        }
        Expr::IfExpr {
            cond,
            then_block,
            else_block,
            ..
        } => {
            collect_bound_names_in_expr(cond, out);
            collect_bound_names_in_block(then_block, out);
            if let Some(eb) = else_block {
                collect_bound_names_in_block(eb, out);
            }
        }
        Expr::StructInit { fields, .. } => {
            for (_, v) in fields {
                collect_bound_names_in_expr(v, out);
            }
        }
        Expr::MatchExpr {
            scrutinee, arms, ..
        } => {
            collect_bound_names_in_expr(scrutinee, out);
            for arm in arms {
                collect_bound_names_in_block(&arm.body, out);
            }
        }
        Expr::SuspendExpr { inner, .. } => collect_bound_names_in_expr(inner, out),
        Expr::ArrayLit { elements, .. } => {
            for e in elements {
                collect_bound_names_in_expr(e, out);
            }
        }
        Expr::Index { base, indices, .. } => {
            collect_bound_names_in_expr(base, out);
            for i in indices {
                collect_bound_names_in_expr(i, out);
            }
        }
        Expr::StringInterp { parts, .. } => {
            for part in parts {
                if let buff_lang_ast::InterpPart::Expr(e) = part {
                    collect_bound_names_in_expr(e, out);
                }
            }
        }
        Expr::MapLit { entries, .. } => {
            for (k, v) in entries {
                collect_bound_names_in_expr(k, out);
                collect_bound_names_in_expr(v, out);
            }
        }
        Expr::Try { expr, .. } => collect_bound_names_in_expr(expr, out),
        Expr::Spawn { task, .. } => collect_bound_names_in_expr(task, out),
        Expr::Range { start, end, .. } => {
            collect_bound_names_in_expr(start, out);
            collect_bound_names_in_expr(end, out);
        }
        // T72: `if let PAT = EXPR { then } else { else }` — the pattern's
        // bindings are new local names (then-scoped); recurse into value
        // and both blocks for further nested bindings.
        Expr::IfLet {
            pattern,
            value,
            then_block,
            else_block,
            ..
        } => {
            for b in pattern.bindings() {
                out.insert(b.name);
            }
            collect_bound_names_in_expr(value, out);
            collect_bound_names_in_block(then_block, out);
            if let Some(eb) = else_block {
                collect_bound_names_in_block(eb, out);
            }
        }
        // T103: `(e1, e2, ...)` — recurse into each element for nested
        // bindings (tuple literals carry no bindings of their own).
        Expr::TupleLit(members, _) => {
            for m in members {
                collect_bound_names_in_expr(m, out);
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Copy classification
// ---------------------------------------------------------------------------

/// Walk statements in source order, classifying each `let`-binding as Copy
/// or non-Copy. Mutates `copy_vars` (adding Copy names) and `locals`
/// (adding every let-bound name).
fn classify_stmts(stmts: &[Stmt], copy_vars: &mut BTreeSet<String>, locals: &mut BTreeSet<String>) {
    for stmt in stmts {
        classify_stmt(stmt, copy_vars, locals);
    }
}

fn classify_stmt(stmt: &Stmt, copy_vars: &mut BTreeSet<String>, locals: &mut BTreeSet<String>) {
    match stmt {
        Stmt::LetDecl {
            name, value, ty, ..
        } => {
            // An explicit TypeRef annotation wins; otherwise classify by
            // initializer expression.
            let is_copy = ty
                .as_ref()
                .map(is_copy_typeref)
                .unwrap_or_else(|| is_copy_expr(value, copy_vars));
            if is_copy {
                copy_vars.insert(name.name.clone());
            }
            locals.insert(name.name.clone());
        }
        // T71: a destructuring let introduces each bound name as a local.
        // Per-field Copy classification is deferred (v0.5 keeps whole-tuple
        // ownership coarse); the names are still recorded as locals.
        Stmt::LetPattern { pattern, .. } => {
            for b in pattern.bindings() {
                locals.insert(b.name);
            }
        }
        // Recurse into nested blocks so lets inside if/for/match branches
        // are classified too. (Cross-scope flattening — see LIMITATIONS.)
        Stmt::ForIn { body, .. } | Stmt::ForWhile { body, .. } => {
            classify_stmts(&body.stmts, copy_vars, locals);
        }
        // T72: `for let PAT = EXPR { body }` — record pattern bindings as
        // locals and recurse into the body (mirroring ForIn/ForWhile).
        Stmt::ForLet { pattern, body, .. } => {
            for b in pattern.bindings() {
                locals.insert(b.name);
            }
            classify_stmts(&body.stmts, copy_vars, locals);
        }
        // T73: `guard <conds> else { block }` — record `let`-condition
        // bindings as locals (per-field Copy deferred — v0.5 whole-stmt
        // coarse ownership); recurse into the else-block.
        Stmt::Guard {
            conditions,
            else_block,
            ..
        } => {
            for c in conditions {
                if let buff_lang_ast::GuardCondition::Let { pattern, .. } = c {
                    for b in pattern.bindings() {
                        locals.insert(b.name);
                    }
                }
            }
            classify_stmts(&else_block.stmts, copy_vars, locals);
        }
        Stmt::Assignment { .. }
        | Stmt::ExprStmt(_, _)
        | Stmt::Return(_, _)
        | Stmt::Break(_)
        | Stmt::Continue(_) => {}
    }
}

/// Is this [`TypeRef`] a Copy primitive?
fn is_copy_typeref(ty: &TypeRef) -> bool {
    match ty {
        TypeRef::Named { name, .. } => is_copy_primitive_name(&name.name),
        _ => false,
    }
}

/// Is this Buff type name one of the Copy primitives?
///
/// The seven Copy names: `Int`, `Float`, `Double`, `Bool`, `Byte`,
/// `Bits`, `Char` (Char added in T21). All other named types (e.g.
/// `String`) and all compound types (`Vector<T>`, `Map<K,V>`, struct
/// names, ...) are non-Copy.
fn is_copy_primitive_name(name: &str) -> bool {
    matches!(
        name,
        "Int" | "Float" | "Double" | "Bool" | "Byte" | "Bits" | "Char"
    )
}

/// Is this initializer expression a Copy value?
///
/// A literal of a Copy kind is Copy. A bare ident referencing a known
/// Copy variable propagates its Copy-ness forward. Anything else
/// (ArrayLit, MapLit, StructInit, function calls, ...) is non-Copy.
fn is_copy_expr(expr: &Expr, copy_vars: &BTreeSet<String>) -> bool {
    match expr {
        Expr::Literal(lit, _) => matches!(
            lit,
            Literal::Int(_)
                | Literal::Float(_)
                | Literal::Double(_)
                | Literal::Bool(_)
                | Literal::Byte(_)
                | Literal::Char(_)
        ),
        Expr::Ident(name, _) => copy_vars.contains(&name.name),
        _ => false,
    }
}

// ---------------------------------------------------------------------------
// Spawn-captured free-variable collection
// ---------------------------------------------------------------------------

/// Walk statements looking for `spawn <task>` expressions; collect the
/// free-variable ident NAMES read inside each spawned task body.
///
/// Free-variable = any `Expr::Ident` that names data (function-call
/// callees that are bare idents do NOT count — they name the function,
/// not data). See [`collect_free_vars_in_expr`].
fn collect_spawn_free_vars_in_stmts(stmts: &[Stmt], out: &mut BTreeSet<String>) {
    for stmt in stmts {
        collect_spawn_free_vars_in_stmt(stmt, out);
    }
}

fn collect_spawn_free_vars_in_stmt(stmt: &Stmt, out: &mut BTreeSet<String>) {
    match stmt {
        Stmt::LetDecl { value, .. } => collect_spawn_free_vars_in_expr(value, out),
        // T71: the pattern itself binds (no outer free vars); only the RHS
        // value can read outer names.
        Stmt::LetPattern { value, .. } => collect_spawn_free_vars_in_expr(value, out),
        Stmt::Assignment { target, value, .. } => {
            collect_spawn_free_vars_in_expr(target, out);
            collect_spawn_free_vars_in_expr(value, out);
        }
        Stmt::ExprStmt(e, _) => collect_spawn_free_vars_in_expr(e, out),
        Stmt::Return(Some(e), _) => collect_spawn_free_vars_in_expr(e, out),
        Stmt::Return(None, _) | Stmt::Break(_) | Stmt::Continue(_) => {}
        Stmt::ForIn { iter, body, .. } => {
            collect_spawn_free_vars_in_expr(iter, out);
            collect_spawn_free_vars_in_stmts(&body.stmts, out);
        }
        Stmt::ForWhile { cond, body, .. } => {
            collect_spawn_free_vars_in_expr(cond, out);
            collect_spawn_free_vars_in_stmts(&body.stmts, out);
        }
        // T72: `for let PAT = EXPR { body }` — the value may read outer
        // names; the pattern binds (loop-local).
        Stmt::ForLet { value, body, .. } => {
            collect_spawn_free_vars_in_expr(value, out);
            collect_spawn_free_vars_in_stmts(&body.stmts, out);
        }
        // T73: `guard <conds> else { block }` — conditions may read outer
        // names; else-block recurses.
        Stmt::Guard {
            conditions,
            else_block,
            ..
        } => {
            for c in conditions {
                let e = match c {
                    buff_lang_ast::GuardCondition::Let { value, .. } => value,
                    buff_lang_ast::GuardCondition::Bool(e) => e,
                };
                collect_spawn_free_vars_in_expr(e, out);
            }
            collect_spawn_free_vars_in_stmts(&else_block.stmts, out);
        }
    }
}

/// Recursively scan `expr` for `Expr::Spawn` nodes; for each one, collect
/// the free-variable ident NAMES read inside the spawned task body.
fn collect_spawn_free_vars_in_expr(expr: &Expr, out: &mut BTreeSet<String>) {
    match expr {
        // The case we care about: a spawn body. Collect its free vars.
        // (We do NOT recurse from here back into the outer walker — the
        // task body is a self-contained expression and free vars inside
        // it are what we want to record.)
        Expr::Spawn { task, .. } => {
            collect_free_vars_in_expr(task, out);
        }
        // Recursive cases: keep hunting for nested spawns.
        Expr::Literal(_, _) | Expr::Ident(_, _) => {}
        Expr::BinaryOp { lhs, rhs, .. } => {
            collect_spawn_free_vars_in_expr(lhs, out);
            collect_spawn_free_vars_in_expr(rhs, out);
        }
        Expr::UnaryOp { operand, .. } => collect_spawn_free_vars_in_expr(operand, out),
        Expr::FuncCall { callee, args, .. } => {
            collect_spawn_free_vars_in_expr(callee, out);
            for a in args {
                collect_spawn_free_vars_in_expr(a, out);
            }
        }
        Expr::MethodCall { receiver, args, .. } => {
            collect_spawn_free_vars_in_expr(receiver, out);
            for a in args {
                collect_spawn_free_vars_in_expr(a, out);
            }
        }
        Expr::IfExpr {
            cond,
            then_block,
            else_block,
            ..
        } => {
            collect_spawn_free_vars_in_expr(cond, out);
            collect_spawn_free_vars_in_block(then_block, out);
            if let Some(eb) = else_block {
                collect_spawn_free_vars_in_block(eb, out);
            }
        }
        Expr::Lambda { body, .. } => collect_spawn_free_vars_in_block(body, out),
        Expr::StructInit { fields, .. } => {
            for (_, v) in fields {
                collect_spawn_free_vars_in_expr(v, out);
            }
        }
        Expr::MatchExpr {
            scrutinee, arms, ..
        } => {
            collect_spawn_free_vars_in_expr(scrutinee, out);
            for arm in arms {
                collect_spawn_free_vars_in_block(&arm.body, out);
            }
        }
        Expr::SuspendExpr { inner, .. } => collect_spawn_free_vars_in_expr(inner, out),
        Expr::Range { start, end, .. } => {
            collect_spawn_free_vars_in_expr(start, out);
            collect_spawn_free_vars_in_expr(end, out);
        }
        Expr::ArrayLit { elements, .. } => {
            for e in elements {
                collect_spawn_free_vars_in_expr(e, out);
            }
        }
        Expr::Index { base, indices, .. } => {
            collect_spawn_free_vars_in_expr(base, out);
            for i in indices {
                collect_spawn_free_vars_in_expr(i, out);
            }
        }
        Expr::StringInterp { parts, .. } => {
            for part in parts {
                if let buff_lang_ast::InterpPart::Expr(e) = part {
                    collect_spawn_free_vars_in_expr(e, out);
                }
            }
        }
        Expr::MapLit { entries, .. } => {
            for (k, v) in entries {
                collect_spawn_free_vars_in_expr(k, out);
                collect_spawn_free_vars_in_expr(v, out);
            }
        }
        Expr::Try { expr, .. } => collect_spawn_free_vars_in_expr(expr, out),
        // T72: `if let PAT = EXPR { then } else { else }` — recurse into
        // the value and both blocks (the pattern's bindings are then-scoped).
        Expr::IfLet {
            value,
            then_block,
            else_block,
            ..
        } => {
            collect_spawn_free_vars_in_expr(value, out);
            collect_spawn_free_vars_in_block(then_block, out);
            if let Some(eb) = else_block {
                collect_spawn_free_vars_in_block(eb, out);
            }
        }
        // T103: `(e1, e2, ...)` — recurse into each element.
        Expr::TupleLit(members, _) => {
            for m in members {
                collect_spawn_free_vars_in_expr(m, out);
            }
        }
    }
}

/// Collect free-variable ident NAMES read inside a spawn's task body.
///
/// A simple-Ident function-call callee does NOT count (it names the
/// function, not data). Method-call receivers DO count (they carry
/// state). Recursive into nested blocks so a spawn body like
/// `spawn { if c { use(x) } }` is handled.
fn collect_free_vars_in_expr(expr: &Expr, out: &mut BTreeSet<String>) {
    match expr {
        Expr::Literal(_, _) => {}
        Expr::Ident(id, _) => {
            out.insert(id.name.clone());
        }
        Expr::BinaryOp { lhs, rhs, .. } => {
            collect_free_vars_in_expr(lhs, out);
            collect_free_vars_in_expr(rhs, out);
        }
        Expr::UnaryOp { operand, .. } => collect_free_vars_in_expr(operand, out),
        Expr::FuncCall { callee, args, .. } => {
            // Simple-Ident callee = function name, not data.
            if !matches!(callee.as_ref(), Expr::Ident(_, _)) {
                collect_free_vars_in_expr(callee, out);
            }
            for a in args {
                collect_free_vars_in_expr(a, out);
            }
        }
        Expr::MethodCall { receiver, args, .. } => {
            collect_free_vars_in_expr(receiver, out);
            for a in args {
                collect_free_vars_in_expr(a, out);
            }
        }
        Expr::IfExpr {
            cond,
            then_block,
            else_block,
            ..
        } => {
            collect_free_vars_in_expr(cond, out);
            collect_free_vars_in_block(then_block, out);
            if let Some(eb) = else_block {
                collect_free_vars_in_block(eb, out);
            }
        }
        Expr::Lambda { body, .. } => collect_free_vars_in_block(body, out),
        Expr::StructInit { fields, .. } => {
            for (_, v) in fields {
                collect_free_vars_in_expr(v, out);
            }
        }
        Expr::MatchExpr {
            scrutinee, arms, ..
        } => {
            collect_free_vars_in_expr(scrutinee, out);
            for arm in arms {
                collect_free_vars_in_block(&arm.body, out);
            }
        }
        Expr::SuspendExpr { inner, .. } => collect_free_vars_in_expr(inner, out),
        Expr::ArrayLit { elements, .. } => {
            for e in elements {
                collect_free_vars_in_expr(e, out);
            }
        }
        Expr::Index { base, indices, .. } => {
            collect_free_vars_in_expr(base, out);
            for i in indices {
                collect_free_vars_in_expr(i, out);
            }
        }
        Expr::StringInterp { parts, .. } => {
            for part in parts {
                if let buff_lang_ast::InterpPart::Expr(e) = part {
                    collect_free_vars_in_expr(e, out);
                }
            }
        }
        Expr::MapLit { entries, .. } => {
            for (k, v) in entries {
                collect_free_vars_in_expr(k, out);
                collect_free_vars_in_expr(v, out);
            }
        }
        Expr::Try { expr, .. } => collect_free_vars_in_expr(expr, out),
        // A nested spawn inside a spawn — collect its task body's free
        // vars too (conservative; the outer Arc-wrap will handle them).
        Expr::Spawn { task, .. } => collect_free_vars_in_expr(task, out),
        Expr::Range { start, end, .. } => {
            collect_free_vars_in_expr(start, out);
            collect_free_vars_in_expr(end, out);
        }
        // T72: `if let PAT = EXPR { then } else { else }` — the value reads
        // outer names; the pattern's bindings are then-scoped locals.
        Expr::IfLet {
            value,
            then_block,
            else_block,
            ..
        } => {
            collect_free_vars_in_expr(value, out);
            collect_free_vars_in_block(then_block, out);
            if let Some(eb) = else_block {
                collect_free_vars_in_block(eb, out);
            }
        }
        // T103: `(e1, e2, ...)` — recurse into each element.
        Expr::TupleLit(members, _) => {
            for m in members {
                collect_free_vars_in_expr(m, out);
            }
        }
    }
}

fn collect_spawn_free_vars_in_block(block: &Block, out: &mut BTreeSet<String>) {
    collect_spawn_free_vars_in_stmts(&block.stmts, out);
}

fn collect_free_vars_in_block(block: &Block, out: &mut BTreeSet<String>) {
    for stmt in &block.stmts {
        // Inside a spawn body, lets introduce new bindings (shadowing);
        // we still scan their initializers for outer free vars but the
        // bound name itself is local to the spawn. For v0.5 simplicity
        // we treat every ident read inside the spawn body as a free var
        // candidate — the caller (analyze_func) intersects with `locals`
        // so spawn-local lets that aren't in the outer `locals` set are
        // naturally filtered out.
        match stmt {
            Stmt::LetDecl { value, .. } => collect_free_vars_in_expr(value, out),
            // T71: destructured bindings are local; only the value reads.
            Stmt::LetPattern { value, .. } => collect_free_vars_in_expr(value, out),
            Stmt::Assignment { target, value, .. } => {
                collect_free_vars_in_expr(target, out);
                collect_free_vars_in_expr(value, out);
            }
            Stmt::ExprStmt(e, _) => collect_free_vars_in_expr(e, out),
            Stmt::Return(Some(e), _) => collect_free_vars_in_expr(e, out),
            Stmt::Return(None, _) | Stmt::Break(_) | Stmt::Continue(_) => {}
            Stmt::ForIn { iter, body, .. } => {
                collect_free_vars_in_expr(iter, out);
                collect_free_vars_in_block(body, out);
            }
            Stmt::ForWhile { cond, body, .. } => {
                collect_free_vars_in_expr(cond, out);
                collect_free_vars_in_block(body, out);
            }
            // T72: `for let PAT = EXPR { body }` — value reads outer names;
            // pattern bindings are loop-local.
            Stmt::ForLet { value, body, .. } => {
                collect_free_vars_in_expr(value, out);
                collect_free_vars_in_block(body, out);
            }
            // T73: `guard <conds> else { block }` — conditions read outer
            // names; else-block recurses.
            Stmt::Guard {
                conditions,
                else_block,
                ..
            } => {
                for c in conditions {
                    let e = match c {
                        buff_lang_ast::GuardCondition::Let { value, .. } => value,
                        buff_lang_ast::GuardCondition::Bool(e) => e,
                    };
                    collect_free_vars_in_expr(e, out);
                }
                collect_free_vars_in_block(else_block, out);
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Assignment-target collection (for CoW detection)
// ---------------------------------------------------------------------------

/// Walk all statements collecting the NAMES of bare-ident assignment
/// targets. Used to detect CoW mutation sites on Arc-shared data.
fn collect_assignment_targets_in_stmts(stmts: &[Stmt], out: &mut BTreeSet<String>) {
    for stmt in stmts {
        collect_assignment_targets_in_stmt(stmt, out);
    }
}

fn collect_assignment_targets_in_stmt(stmt: &Stmt, out: &mut BTreeSet<String>) {
    match stmt {
        Stmt::Assignment { target, value, .. } => {
            if let Expr::Ident(id, _) = target {
                out.insert(id.name.clone());
            }
            // The RHS of an assignment may itself contain nested
            // assignments (rare but possible inside a lambda); recurse.
            collect_assignment_targets_in_expr(value, out);
        }
        Stmt::LetDecl { value, .. } => {
            collect_assignment_targets_in_expr(value, out);
        }
        // T71: a destructuring let introduces no assignment targets.
        Stmt::LetPattern { value, .. } => {
            collect_assignment_targets_in_expr(value, out);
        }
        Stmt::ExprStmt(e, _) => collect_assignment_targets_in_expr(e, out),
        Stmt::Return(Some(e), _) => collect_assignment_targets_in_expr(e, out),
        Stmt::Return(None, _) | Stmt::Break(_) | Stmt::Continue(_) => {}
        Stmt::ForIn { iter, body, .. } => {
            collect_assignment_targets_in_expr(iter, out);
            collect_assignment_targets_in_stmts(&body.stmts, out);
        }
        Stmt::ForWhile { cond, body, .. } => {
            collect_assignment_targets_in_expr(cond, out);
            collect_assignment_targets_in_stmts(&body.stmts, out);
        }
        // T72: `for let PAT = EXPR { body }` — value + body may contain
        // nested assignment targets.
        Stmt::ForLet { value, body, .. } => {
            collect_assignment_targets_in_expr(value, out);
            collect_assignment_targets_in_stmts(&body.stmts, out);
        }
        // T73: `guard <conds> else { block }` — conditions + else-block may
        // contain nested assignment targets.
        Stmt::Guard {
            conditions,
            else_block,
            ..
        } => {
            for c in conditions {
                let e = match c {
                    buff_lang_ast::GuardCondition::Let { value, .. } => value,
                    buff_lang_ast::GuardCondition::Bool(e) => e,
                };
                collect_assignment_targets_in_expr(e, out);
            }
            collect_assignment_targets_in_stmts(&else_block.stmts, out);
        }
    }
}

fn collect_assignment_targets_in_expr(expr: &Expr, out: &mut BTreeSet<String>) {
    match expr {
        Expr::Literal(_, _) | Expr::Ident(_, _) => {}
        Expr::BinaryOp { lhs, rhs, .. } => {
            collect_assignment_targets_in_expr(lhs, out);
            collect_assignment_targets_in_expr(rhs, out);
        }
        Expr::UnaryOp { operand, .. } => collect_assignment_targets_in_expr(operand, out),
        Expr::FuncCall { callee, args, .. } => {
            collect_assignment_targets_in_expr(callee, out);
            for a in args {
                collect_assignment_targets_in_expr(a, out);
            }
        }
        Expr::MethodCall { receiver, args, .. } => {
            collect_assignment_targets_in_expr(receiver, out);
            for a in args {
                collect_assignment_targets_in_expr(a, out);
            }
        }
        Expr::IfExpr {
            cond,
            then_block,
            else_block,
            ..
        } => {
            collect_assignment_targets_in_expr(cond, out);
            collect_assignment_targets_in_block(then_block, out);
            if let Some(eb) = else_block {
                collect_assignment_targets_in_block(eb, out);
            }
        }
        Expr::Lambda { body, .. } => collect_assignment_targets_in_block(body, out),
        Expr::StructInit { fields, .. } => {
            for (_, v) in fields {
                collect_assignment_targets_in_expr(v, out);
            }
        }
        Expr::MatchExpr {
            scrutinee, arms, ..
        } => {
            collect_assignment_targets_in_expr(scrutinee, out);
            for arm in arms {
                collect_assignment_targets_in_block(&arm.body, out);
            }
        }
        Expr::SuspendExpr { inner, .. } => collect_assignment_targets_in_expr(inner, out),
        Expr::ArrayLit { elements, .. } => {
            for e in elements {
                collect_assignment_targets_in_expr(e, out);
            }
        }
        Expr::Index { base, indices, .. } => {
            collect_assignment_targets_in_expr(base, out);
            for i in indices {
                collect_assignment_targets_in_expr(i, out);
            }
        }
        Expr::StringInterp { parts, .. } => {
            for part in parts {
                if let buff_lang_ast::InterpPart::Expr(e) = part {
                    collect_assignment_targets_in_expr(e, out);
                }
            }
        }
        Expr::MapLit { entries, .. } => {
            for (k, v) in entries {
                collect_assignment_targets_in_expr(k, out);
                collect_assignment_targets_in_expr(v, out);
            }
        }
        Expr::Try { expr, .. } => collect_assignment_targets_in_expr(expr, out),
        Expr::Spawn { task, .. } => collect_assignment_targets_in_expr(task, out),
        Expr::Range { start, end, .. } => {
            collect_assignment_targets_in_expr(start, out);
            collect_assignment_targets_in_expr(end, out);
        }
        // T72: `if let PAT = EXPR { then } else { else }` — recurse into
        // the value and both blocks for nested assignment targets.
        Expr::IfLet {
            value,
            then_block,
            else_block,
            ..
        } => {
            collect_assignment_targets_in_expr(value, out);
            collect_assignment_targets_in_block(then_block, out);
            if let Some(eb) = else_block {
                collect_assignment_targets_in_block(eb, out);
            }
        }
        // T103: `(e1, e2, ...)` — recurse into each element.
        Expr::TupleLit(members, _) => {
            for m in members {
                collect_assignment_targets_in_expr(m, out);
            }
        }
    }
}

fn collect_assignment_targets_in_block(block: &Block, out: &mut BTreeSet<String>) {
    collect_assignment_targets_in_stmts(&block.stmts, out);
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

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

    fn char_expr(c: char) -> Expr {
        Expr::Literal(Literal::Char(c), span())
    }

    fn string_expr(s: &str) -> Expr {
        Expr::Literal(Literal::String(s.to_string()), span())
    }

    fn array_expr(els: Vec<Expr>) -> Expr {
        Expr::ArrayLit {
            elements: els,
            span: span(),
        }
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

    fn let_stmt_mut(name: &str, value: Expr) -> Stmt {
        Stmt::LetDecl {
            name: Ident::new(name, span()),
            value,
            mutable: true,
            ty: None,
            span: span(),
        }
    }

    fn assign_stmt(name: &str, value: Expr) -> Stmt {
        Stmt::Assignment {
            target: ident_expr(name),
            op: buff_lang_ast::op::BinaryOp::Assign,
            value,
            span: span(),
        }
    }

    fn call_expr(name: &str, args: Vec<Expr>) -> Expr {
        Expr::FuncCall {
            callee: Box::new(ident_expr(name)),
            args,
            span: span(),
        }
    }

    fn spawn_expr(task: Expr) -> Expr {
        Expr::Spawn {
            task: Box::new(task),
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

    fn func_with_param(name: &str, ty: &str, stmts: Vec<Stmt>) -> FuncDecl {
        FuncDecl {
            name: Ident::new("f", span()),
            params: vec![Param {
                name: Ident::new(name, span()),
                ty: named_type(ty),
                span: span(),
            }],
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

    #[test]
    fn int_let_is_copy() {
        let f = func_with_stmts(vec![let_stmt("x", int_expr(42))]);
        let facts = analyze_func(&f);
        assert!(facts.is_copy("x"));
        assert!(!facts.is_arc("x"));
        assert!(!facts.is_arc_mut("x"));
    }

    #[test]
    fn char_let_is_copy() {
        // T21: Char is Copy. T33 ensures the ownership analysis knows it.
        let f = func_with_stmts(vec![let_stmt("c", char_expr('A'))]);
        let facts = analyze_func(&f);
        assert!(facts.is_copy("c"));
    }

    #[test]
    fn string_let_is_not_copy() {
        let f = func_with_stmts(vec![let_stmt("s", string_expr("hi"))]);
        let facts = analyze_func(&f);
        assert!(!facts.is_copy("s"));
    }

    #[test]
    fn array_let_is_not_copy() {
        let f = func_with_stmts(vec![let_stmt(
            "v",
            array_expr(vec![int_expr(1), int_expr(2)]),
        )]);
        let facts = analyze_func(&f);
        assert!(!facts.is_copy("v"));
    }

    #[test]
    fn copy_propagates_through_let_chain() {
        // let x = 42; let y = x; — both Copy.
        let f = func_with_stmts(vec![
            let_stmt("x", int_expr(42)),
            let_stmt("y", ident_expr("x")),
        ]);
        let facts = analyze_func(&f);
        assert!(facts.is_copy("x"));
        assert!(facts.is_copy("y"));
    }

    #[test]
    fn param_with_int_type_is_copy() {
        let f = func_with_param("n", "Int", vec![]);
        let facts = analyze_func(&f);
        assert!(facts.is_copy("n"));
    }

    #[test]
    fn param_with_char_type_is_copy() {
        let f = func_with_param("c", "Char", vec![]);
        let facts = analyze_func(&f);
        assert!(facts.is_copy("c"));
    }

    #[test]
    fn spawn_with_string_capture_arc_var() {
        // let s = "hi"; spawn consume(s)
        let f = func_with_stmts(vec![
            let_stmt("s", string_expr("hi")),
            Stmt::ExprStmt(
                spawn_expr(call_expr("consume", vec![ident_expr("s")])),
                span(),
            ),
        ]);
        let facts = analyze_func(&f);
        assert!(facts.is_arc("s"), "s should be Arc-wrapped: {facts:?}");
        assert!(!facts.is_copy("s"));
    }

    #[test]
    fn spawn_with_int_does_not_arc_wrap() {
        // let n = 42; spawn consume(n)  — Int is Copy, no Arc needed.
        let f = func_with_stmts(vec![
            let_stmt("n", int_expr(42)),
            Stmt::ExprStmt(
                spawn_expr(call_expr("consume", vec![ident_expr("n")])),
                span(),
            ),
        ]);
        let facts = analyze_func(&f);
        assert!(
            !facts.is_arc("n"),
            "Int should not be Arc-wrapped: {facts:?}"
        );
        assert!(facts.is_copy("n"));
    }

    #[test]
    fn no_spawn_no_arc_vars() {
        let f = func_with_stmts(vec![
            let_stmt("s", string_expr("hi")),
            Stmt::ExprStmt(call_expr("consume", vec![ident_expr("s")]), span()),
        ]);
        let facts = analyze_func(&f);
        assert!(
            facts.arc_vars.is_empty(),
            "no spawn -> no arc vars: {facts:?}"
        );
    }

    #[test]
    fn arc_var_mutated_becomes_arc_mut() {
        // let mut v = [...]; spawn consume(v); v = [...]
        let f = func_with_stmts(vec![
            let_stmt_mut("v", array_expr(vec![int_expr(1), int_expr(2)])),
            Stmt::ExprStmt(
                spawn_expr(call_expr("consume", vec![ident_expr("v")])),
                span(),
            ),
            assign_stmt("v", array_expr(vec![int_expr(3), int_expr(4)])),
        ]);
        let facts = analyze_func(&f);
        assert!(facts.is_arc("v"), "v should be Arc-wrapped: {facts:?}");
        assert!(facts.is_arc_mut("v"), "v should be Arc-mut: {facts:?}");
    }

    #[test]
    fn arc_var_not_mutated_is_not_arc_mut() {
        // let s = "hi"; spawn consume(s)  — no subsequent assignment
        let f = func_with_stmts(vec![
            let_stmt("s", string_expr("hi")),
            Stmt::ExprStmt(
                spawn_expr(call_expr("consume", vec![ident_expr("s")])),
                span(),
            ),
        ]);
        let facts = analyze_func(&f);
        assert!(facts.is_arc("s"));
        assert!(!facts.is_arc_mut("s"), "s is not mutated: {facts:?}");
    }

    #[test]
    fn deterministic_facts_two_runs_match() {
        // The T29 flaky-test lesson — same AST must produce same facts.
        let mk = || {
            func_with_stmts(vec![
                let_stmt("s", string_expr("hi")),
                let_stmt("n", int_expr(42)),
                Stmt::ExprStmt(
                    spawn_expr(call_expr("consume", vec![ident_expr("s"), ident_expr("n")])),
                    span(),
                ),
                assign_stmt("s", string_expr("bye")),
            ])
        };
        let a = analyze_func(&mk());
        let b = analyze_func(&mk());
        assert_eq!(a, b);
    }

    // -----------------------------------------------------------------
    // T34: closure capture analysis tests
    // -----------------------------------------------------------------

    fn binary_op(op: buff_lang_ast::op::BinaryOp, lhs: Expr, rhs: Expr) -> Expr {
        Expr::BinaryOp {
            op,
            lhs: Box::new(lhs),
            rhs: Box::new(rhs),
            span: span(),
        }
    }

    fn param(name: &str) -> buff_lang_ast::common::Param {
        buff_lang_ast::common::Param {
            name: Ident::new(name, span()),
            ty: named_type("_"),
            span: span(),
        }
    }

    fn block(stmts: Vec<Stmt>) -> Block {
        Block {
            stmts,
            span: span(),
        }
    }

    fn expr_block(e: Expr) -> Block {
        block(vec![Stmt::ExprStmt(e, span())])
    }

    #[test]
    fn closure_no_captures_single_param() {
        // { x => x * 2 } — body only uses param x; no captures.
        let body = expr_block(binary_op(
            buff_lang_ast::op::BinaryOp::Mul,
            ident_expr("x"),
            int_expr(2),
        ));
        let caps = closure_captures(&[param("x")], &body);
        assert!(caps.is_empty(), "expected no captures, got {caps:?}");
    }

    #[test]
    fn closure_no_captures_two_params() {
        // { x, y => x + y } — body only uses params; no captures.
        let body = expr_block(binary_op(
            buff_lang_ast::op::BinaryOp::Add,
            ident_expr("x"),
            ident_expr("y"),
        ));
        let caps = closure_captures(&[param("x"), param("y")], &body);
        assert!(caps.is_empty(), "expected no captures, got {caps:?}");
    }

    #[test]
    fn closure_captures_external_var() {
        // { x => x + f } — captures f.
        let body = expr_block(binary_op(
            buff_lang_ast::op::BinaryOp::Add,
            ident_expr("x"),
            ident_expr("f"),
        ));
        let caps = closure_captures(&[param("x")], &body);
        assert_eq!(caps.len(), 1);
        assert!(caps.contains("f"), "expected f captured, got {caps:?}");
    }

    #[test]
    fn closure_captures_multiple_external_vars() {
        // { x => x + f + g } — captures f and g.
        let body = expr_block(binary_op(
            buff_lang_ast::op::BinaryOp::Add,
            binary_op(
                buff_lang_ast::op::BinaryOp::Add,
                ident_expr("x"),
                ident_expr("f"),
            ),
            ident_expr("g"),
        ));
        let caps = closure_captures(&[param("x")], &body);
        assert_eq!(caps.len(), 2);
        assert!(caps.contains("f") && caps.contains("g"), "got {caps:?}");
    }

    #[test]
    fn closure_local_let_is_not_capture() {
        // { x => let y = 1; x + y + f } — y is local; only f is captured.
        let body = block(vec![
            let_stmt("y", int_expr(1)),
            Stmt::ExprStmt(
                binary_op(
                    buff_lang_ast::op::BinaryOp::Add,
                    binary_op(
                        buff_lang_ast::op::BinaryOp::Add,
                        ident_expr("x"),
                        ident_expr("y"),
                    ),
                    ident_expr("f"),
                ),
                span(),
            ),
        ]);
        let caps = closure_captures(&[param("x")], &body);
        assert_eq!(caps.len(), 1);
        assert!(caps.contains("f"), "expected only f captured, got {caps:?}");
        assert!(!caps.contains("y"), "y is local, not a capture");
    }

    #[test]
    fn closure_does_not_capture_function_names() {
        // { x => print(x) } — print is a function name, not a captured variable.
        let body = expr_block(call_expr("print", vec![ident_expr("x")]));
        let caps = closure_captures(&[param("x")], &body);
        assert!(
            caps.is_empty(),
            "print should NOT be a capture, got {caps:?}"
        );
    }

    #[test]
    fn closure_nested_inner_param_not_capture_of_outer() {
        // { x => { y => x + y } } — outer captures nothing (x is param,
        // y is the inner closure's param).
        let inner = Expr::Lambda {
            params: vec![param("y")],
            body: expr_block(binary_op(
                buff_lang_ast::op::BinaryOp::Add,
                ident_expr("x"),
                ident_expr("y"),
            )),
            return_type: None,
            span: span(),
        };
        let outer_body = expr_block(inner);
        let caps = closure_captures(&[param("x")], &outer_body);
        assert!(caps.is_empty(), "expected no captures, got {caps:?}");
    }

    #[test]
    fn closure_captures_are_deterministic() {
        // Same closure → same capture set every time (BTreeSet).
        let mk = || {
            let body = expr_block(binary_op(
                buff_lang_ast::op::BinaryOp::Add,
                binary_op(
                    buff_lang_ast::op::BinaryOp::Add,
                    ident_expr("x"),
                    ident_expr("g"),
                ),
                ident_expr("f"),
            ));
            closure_captures(&[param("x")], &body)
        };
        assert_eq!(mk(), mk());
    }
}

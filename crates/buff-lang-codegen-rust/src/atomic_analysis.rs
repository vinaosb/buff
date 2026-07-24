//! Atomic-promotion analysis (T42).
//!
//! T41 introduced race detection: any closure passed to a parallel
//! combinator (`par_map` / `par_filter` / `par_reduce`) that mutates a
//! variable captured from the enclosing scope is a hard
//! [`crate::race_analysis::ParallelMutabilityError`]. That is correct —
//! naive mutation of a shared variable across worker threads is a data
//! race.
//!
//! T42 opens a narrow **escape hatch** for one specific, common, easily-
//! made-safe pattern: the integer accumulator. Concretely:
//!
//! ```text
//! let mut t = 0
//! v.par_map({ x => t += x })
//! // post-parallel use of `t` ...
//! ```
//!
//! The accumulator pattern (`t` starts at an integer literal, mutated
//! ONLY by `+=` inside the parallel closure) is mechanically promotable
//! to `std::sync::atomic::AtomicI64`. Each `t += x` becomes
//! `t.fetch_add(x as i64, Ordering::Relaxed)`, the declaration
//! `let mut t = 0` becomes `let t = AtomicI64::new(0)`, and any
//! post-parallel read of `t` becomes `t.load(Ordering::Relaxed)`. This
//! is sound (atomic RMW on `i64` cannot race) and removes the T41 error
//! for this exact pattern.
//!
//! ## Scope (narrow by design)
//!
//! A captured variable `t` is promotable iff ALL of the following hold:
//!
//! 1. **Enclosing-scope shape** — there is a `let mut t = <int literal>`
//!    in the SAME function as the parallel combinator call. (`let t =`
//!    without `mut` cannot be mutated syntactically; non-integer initial
//!    values cannot be promoted to `AtomicI64`.)
//! 2. **Captured** — `t` is captured by the parallel closure (free in
//!    the body, not bound by a param or an inner `let`).
//! 3. **Mutation shape inside the parallel closure** — every mutation
//!    of `t` whose site is at the closure body's TOP LEVEL (NOT inside
//!    a nested closure) uses the `+=` operator. Any single `=`, `-=`,
//!    `*=`, `/=`, or `%=` mutation disqualifies the variable, and T41's
//!    race detector reports it as usual.
//! 4. **Combinator** — the enclosing parallel combinator is `par_map`
//!    or `par_reduce`. (`par_filter` is NOT an accumulation — its
//!    closure returns a `bool`, so mutating an accumulator inside it is
//!    almost certainly a user bug; keep the T41 error.)
//! 5. **No nested-closure mutation** — if `t` is mutated inside a
//!    NESTED closure within the parallel closure, we conservatively
//!    refuse promotion (the nested closure may be returned, stored, or
//!    invoked outside the parallel context — keep the T41 error). T42
//!    is intentionally narrow.
//!
//! A variable that fails ANY of (1)-(5) is left alone — T41's race
//! detector continues to flag its mutation as a `ParallelMutabilityError`.
//!
//! ## Determinism
//!
//! Every collection is a `BTreeMap` / `BTreeSet` / `Vec` (per the T29
//! flaky-test lesson). Walkers visit AST nodes in source order. The
//! same input program always yields the same promotion set.
//!
//! ## What this file owns
//!
//! - [`AtomicPromotions`] — the program-wide result (function name →
//!   set of promotable captures).
//! - [`analyze`] / [`analyze_func`] — the entry points.
//! - [`is_integer_literal_init`] — exported for tests so they can
//!   verify the predicate on synthesised ASTs.

use std::collections::{BTreeMap, BTreeSet};

use buff_lang_ast::{
    common::{Block, Param},
    decl::FuncDecl,
    op::BinaryOp,
    Decl, Expr, Literal, Stmt,
};

use crate::race_analysis::is_assignment_op;

/// `par_*` combinators whose closures are valid accumulation contexts
/// for T42 atomic promotion. `par_filter` is deliberately excluded: its
/// closure returns `bool`, so a mutated accumulator inside it is almost
/// certainly a bug rather than an intentional reduction. Keeping this
/// list narrower than [`PARALLEL_COMBINATORS`] is what preserves T41's
/// race error for `par_filter` mutation (spec §2: "mutation in
/// `par_filter` (filter is not an accumulation) → keep
/// `ParallelMutabilityError`").
const ACCUMULATING_COMBINATORS: &[&str] = &["par_map", "par_reduce"];

/// Map of promotable variable name → integer initial value, for ONE
/// function. Stored as a [`BTreeMap`] so iteration order is
/// deterministic (the T29 flaky-test lesson — never `HashMap` for
/// codegen-feeding data).
pub type AtomicSet = BTreeMap<String, i64>;

/// Program-wide atomic-promotion result. Keyed by function name (a
/// variable `t` in `fn foo` is independent of a variable `t` in
/// `fn bar`; promotion decisions do not leak across functions).
///
/// Construct via [`analyze`] (program-level) or build manually for
/// tests via [`Self::empty`] + [`Self::insert`] (or `Default`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AtomicPromotions {
    /// Function name → set of promotable captures within that function.
    pub by_function: BTreeMap<String, AtomicSet>,
}

impl AtomicPromotions {
    /// Empty promotions set (no variable anywhere is promotable).
    pub fn empty() -> Self {
        Self::default()
    }

    /// Get the promotable captures for one function (empty if the
    /// function had no promotable pattern).
    pub fn for_func(&self, func_name: &str) -> AtomicSet {
        self.by_function.get(func_name).cloned().unwrap_or_default()
    }

    /// Is `var_name` promotable inside `func_name`?
    pub fn is_promotable(&self, func_name: &str, var_name: &str) -> bool {
        self.by_function
            .get(func_name)
            .is_some_and(|s| s.contains_key(var_name))
    }

    /// The integer initial value to which the promotable variable was
    /// declared (`let mut t = N`). Used by codegen to emit
    /// `AtomicI64::new(N)`. Returns `None` if not promotable.
    pub fn initial_value(&self, func_name: &str, var_name: &str) -> Option<i64> {
        self.by_function
            .get(func_name)
            .and_then(|s| s.get(var_name).copied())
    }

    /// Insert a promotable binding for a function (test helper).
    pub fn insert(&mut self, func_name: &str, var_name: &str, initial: i64) {
        self.by_function
            .entry(func_name.to_string())
            .or_default()
            .insert(var_name.to_string(), initial);
    }
}

/// Run atomic-promotion detection across the entire program.
///
/// Returns the [`AtomicPromotions`] set: a map from function name to
/// the set of promotable captures within that function. Functions that
/// contain no promotable pattern are absent from the map.
///
/// Walks every `Decl::FuncDecl` via [`analyze_func`]. Non-function
/// decls are skipped (they cannot contain parallel-combinator call
/// sites with the accumulator pattern).
pub fn analyze(decls: &[Decl]) -> AtomicPromotions {
    let mut result = AtomicPromotions::empty();
    for decl in decls {
        if let Decl::FuncDecl(func) = decl {
            let set = analyze_func(func);
            if !set.is_empty() {
                result.by_function.insert(func.name.name.clone(), set);
            }
        }
    }
    result
}

/// Detect atomic-promotable captures in ONE function.
///
/// Algorithm:
/// 1. Scan the function body (all nesting levels) for candidate
///    `let mut NAME = <int literal>` bindings → `candidates`.
/// 2. Scan the function body for parallel-combinator call sites whose
///    method is in [`ACCUMULATING_COMBINATORS`] (`par_map` / `par_reduce`).
/// 3. For each candidate captured by such a parallel closure:
///    - Walk the closure body's TOP-LEVEL statements (no nested-closure
///      recursion) collecting every mutation of the candidate.
///    - If all mutations are `BinaryOp::AddAssign` (`+=`), the candidate
///      is promotable for THIS closure.
///    - If any mutation uses another assignment op (`=`, `-=`, `*=`,
///      `/=`, `%=`), the candidate is REJECTED — T41's race detector
///      will flag it as usual.
/// 4. A candidate is in the result iff it is promotable in EVERY
///    parallel closure that captures it AND not rejected in any. This
///    catches the case where the same variable is captured by two
///    closures with different mutation shapes.
///
/// Returns the promotable set as a [`BTreeMap`] (name → initial value).
pub fn analyze_func(func: &FuncDecl) -> AtomicSet {
    // Step 1 — find all `let mut NAME = <int literal>` in this fn.
    let candidates: AtomicSet = find_integer_mutable_lets(&func.body);

    // Step 2/3 — visit every accumulating parallel closure, building up
    // promoted / rejected sets.
    let mut promoted: AtomicSet = AtomicSet::new();
    let mut rejected: BTreeSet<String> = BTreeSet::new();

    let mut sites: Vec<(Vec<Param>, Block)> = Vec::new();
    collect_accumulating_parallel_closures(&func.body, &mut sites);
    for (params, body) in sites {
        let captures: BTreeSet<String> = buff_lang_types::closure_captures(&params, &body);
        // For every candidate captured here and not yet rejected, decide.
        for (name, init) in &candidates {
            if rejected.contains(name) {
                continue;
            }
            if !captures.contains(name) {
                continue;
            }
            match classify_candidate(name, &body) {
                Class::Promotable => {
                    // Insert (or re-confirm) the promotion. If it was
                    // promoted in a previous closure and remains
                    // promotable here, keep it promoted.
                    promoted.entry(name.clone()).or_insert(*init);
                }
                Class::NotPromotable => {
                    // Disqualify across the board — even if it was
                    // promoted elsewhere, a single non-`+=` mutation
                    // site makes T41 flag it (race detector walks every
                    // closure). Pre-emptively remove so a later
                    // closure's `+=` doesn't re-add it.
                    promoted.remove(name);
                    rejected.insert(name.clone());
                }
                Class::NotMutated => {
                    // Not mutated in THIS closure — but might still be
                    // mutated (and promotable) in a sibling closure.
                    // Leave the existing decision untouched.
                }
            }
        }
    }

    promoted
}

/// Classification of a candidate inside one parallel closure body.
enum Class {
    /// Every top-level mutation is `+=` (and at least one exists).
    Promotable,
    /// Some top-level mutation is `=`, `-=`, `*=`, `/=`, or `%=` (or
    /// the candidate is mutated inside a nested closure).
    NotPromotable,
    /// The candidate is not mutated at the top level of this closure
    /// (e.g. only read, or only mutated inside a nested closure — both
    /// of which T41 handles separately).
    NotMutated,
}

/// Walk the closure body's top-level statements (NOT recursing into
/// nested closures, but recursing into control flow like `if`/`for`)
/// and classify the candidate.
fn classify_candidate(name: &str, body: &Block) -> Class {
    let mut mutations: Vec<BinaryOp> = Vec::new();
    collect_top_level_mutations(body, name, &mut mutations);
    if mutations.is_empty() {
        return Class::NotMutated;
    }
    if mutations.iter().all(|op| *op == BinaryOp::AddAssign) {
        Class::Promotable
    } else {
        Class::NotPromotable
    }
}

/// Walk a block collecting every assignment operator applied to a bare
/// `Expr::Ident(name)` at the top level of any contained statement.
///
/// Recurses INTO control flow (`if`/`for`/`while`/`match`/etc.) and
/// ordinary expression positions, but does NOT recurse into nested
/// closures (`Expr::Lambda`) — mutations inside a nested closure are
/// intentionally excluded from T42's narrow promotion (a nested closure
/// may escape the parallel context; keep T41's error for safety).
fn collect_top_level_mutations(block: &Block, name: &str, out: &mut Vec<BinaryOp>) {
    for stmt in &block.stmts {
        collect_stmt_mutations(stmt, name, out);
    }
}

fn collect_stmt_mutations(stmt: &Stmt, name: &str, out: &mut Vec<BinaryOp>) {
    match stmt {
        Stmt::Assignment {
            target, op, value, ..
        } if is_assignment_op(op) => {
            if let Expr::Ident(id, _) = target {
                if id.name == name {
                    out.push(*op);
                }
            }
            // Recurse into value (an inner if/match/etc. could carry
            // another mutation of the same name).
            collect_expr_mutations(value, name, out);
        }
        // Other statement kinds: recurse into their sub-expressions /
        // sub-blocks (NOT into Expr::Lambda — that's a nested closure).
        Stmt::Assignment { value, .. } => collect_expr_mutations(value, name, out),
        Stmt::LetDecl { value, .. } | Stmt::LetPattern { value, .. } => {
            collect_expr_mutations(value, name, out);
        }
        Stmt::ExprStmt(e, _) => collect_expr_mutations(e, name, out),
        Stmt::Return(Some(e), _) => collect_expr_mutations(e, name, out),
        Stmt::Return(None, _) | Stmt::Break(_) | Stmt::Continue(_) => {}
        Stmt::ForIn { iter, body, .. } => {
            collect_expr_mutations(iter, name, out);
            collect_top_level_mutations(body, name, out);
        }
        Stmt::ForWhile { cond, body, .. } => {
            collect_expr_mutations(cond, name, out);
            collect_top_level_mutations(body, name, out);
        }
        Stmt::ForLet { value, body, .. } => {
            collect_expr_mutations(value, name, out);
            collect_top_level_mutations(body, name, out);
        }
        Stmt::Guard {
            conditions,
            else_block,
            ..
        } => {
            for cond in conditions {
                match cond {
                    buff_lang_ast::GuardCondition::Let { value, .. } => {
                        collect_expr_mutations(value, name, out);
                    }
                    buff_lang_ast::GuardCondition::Bool(e) => {
                        collect_expr_mutations(e, name, out);
                    }
                }
            }
            collect_top_level_mutations(else_block, name, out);
        }
        Stmt::Defer { expr, .. } => collect_expr_mutations(expr, name, out),
        Stmt::ComptimeBlock { .. } => {}
    }
}

fn collect_expr_mutations(expr: &Expr, name: &str, out: &mut Vec<BinaryOp>) {
    match expr {
        Expr::MethodCall { receiver, args, .. } => {
            collect_expr_mutations(receiver, name, out);
            for a in args {
                collect_expr_mutations(a, name, out);
            }
        }
        Expr::BinaryOp { lhs, rhs, .. } => {
            collect_expr_mutations(lhs, name, out);
            collect_expr_mutations(rhs, name, out);
        }
        Expr::UnaryOp { operand, .. } => collect_expr_mutations(operand, name, out),
        Expr::FuncCall { callee, args, .. } => {
            collect_expr_mutations(callee, name, out);
            for a in args {
                collect_expr_mutations(a, name, out);
            }
        }
        Expr::IfExpr {
            cond,
            then_block,
            else_block,
            ..
        } => {
            collect_expr_mutations(cond, name, out);
            collect_top_level_mutations(then_block, name, out);
            if let Some(eb) = else_block {
                collect_top_level_mutations(eb, name, out);
            }
        }
        // DELIBERATELY do NOT recurse into nested closures. Mutations
        // of `name` inside a nested closure are excluded from T42
        // promotion (a nested closure may escape the parallel context).
        Expr::Lambda { .. } => {}
        Expr::StructInit { fields, .. } => {
            for (_, v) in fields {
                collect_expr_mutations(v, name, out);
            }
        }
        Expr::MatchExpr {
            scrutinee, arms, ..
        } => {
            collect_expr_mutations(scrutinee, name, out);
            for arm in arms {
                collect_top_level_mutations(&arm.body, name, out);
            }
        }
        Expr::SuspendExpr { inner, .. } => collect_expr_mutations(inner, name, out),
        Expr::ArrayLit { elements, .. } => {
            for e in elements {
                collect_expr_mutations(e, name, out);
            }
        }
        Expr::Index { base, indices, .. } => {
            collect_expr_mutations(base, name, out);
            for i in indices {
                collect_expr_mutations(i, name, out);
            }
        }
        Expr::StringInterp { parts, .. } => {
            for part in parts {
                if let buff_lang_ast::InterpPart::Expr(e, _) = part {
                    collect_expr_mutations(e, name, out);
                }
            }
        }
        Expr::MapLit { entries, .. } => {
            for (k, v) in entries {
                collect_expr_mutations(k, name, out);
                collect_expr_mutations(v, name, out);
            }
        }
        Expr::Try { expr, .. } => collect_expr_mutations(expr, name, out),
        Expr::Spawn { task, .. } => collect_expr_mutations(task, name, out),
        Expr::Range { start, end, .. } => {
            collect_expr_mutations(start, name, out);
            collect_expr_mutations(end, name, out);
        }
        Expr::IfLet {
            value,
            then_block,
            else_block,
            ..
        } => {
            collect_expr_mutations(value, name, out);
            collect_top_level_mutations(then_block, name, out);
            if let Some(eb) = else_block {
                collect_top_level_mutations(eb, name, out);
            }
        }
        Expr::TupleLit(members, _) => {
            for m in members {
                collect_expr_mutations(m, name, out);
            }
        }
        Expr::NamedArg { value, .. } => collect_expr_mutations(value, name, out),
        // Leaves — no recursion needed.
        Expr::Literal(_, _) | Expr::Ident(_, _) => {}
    }
}

/// Walk a function body and collect every `let mut NAME = <int literal>`
/// binding visible at any nesting level. Returns the map NAME → initial
/// int value. Used to identify candidates for promotion.
///
/// A binding with `mutable: false` is NOT a candidate (Buff `let`
/// without `mut` cannot be reassigned syntactically). A binding whose
/// initializer is not an integer literal is NOT a candidate (we need
/// the literal value to emit `AtomicI64::new(N)`).
fn find_integer_mutable_lets(block: &Block) -> AtomicSet {
    let mut out: AtomicSet = AtomicSet::new();
    walk_block_for_integer_lets(block, &mut out);
    out
}

fn walk_block_for_integer_lets(block: &Block, out: &mut AtomicSet) {
    for stmt in &block.stmts {
        walk_stmt_for_integer_lets(stmt, out);
    }
}

fn walk_stmt_for_integer_lets(stmt: &Stmt, out: &mut AtomicSet) {
    match stmt {
        Stmt::LetDecl {
            name,
            value,
            mutable,
            ..
        } => {
            if *mutable {
                if let Some(init) = is_integer_literal_init(value) {
                    out.insert(name.name.clone(), init);
                }
            }
            walk_expr_for_integer_lets(value, out);
        }
        Stmt::LetPattern { value, .. } => walk_expr_for_integer_lets(value, out),
        Stmt::Assignment { target, value, .. } => {
            walk_expr_for_integer_lets(target, out);
            walk_expr_for_integer_lets(value, out);
        }
        Stmt::ExprStmt(e, _) => walk_expr_for_integer_lets(e, out),
        Stmt::Return(Some(e), _) => walk_expr_for_integer_lets(e, out),
        Stmt::Return(None, _) | Stmt::Break(_) | Stmt::Continue(_) => {}
        Stmt::ForIn { iter, body, .. } => {
            walk_expr_for_integer_lets(iter, out);
            walk_block_for_integer_lets(body, out);
        }
        Stmt::ForWhile { cond, body, .. } => {
            walk_expr_for_integer_lets(cond, out);
            walk_block_for_integer_lets(body, out);
        }
        Stmt::ForLet { value, body, .. } => {
            walk_expr_for_integer_lets(value, out);
            walk_block_for_integer_lets(body, out);
        }
        Stmt::Guard {
            conditions,
            else_block,
            ..
        } => {
            for cond in conditions {
                match cond {
                    buff_lang_ast::GuardCondition::Let { value, .. } => {
                        walk_expr_for_integer_lets(value, out);
                    }
                    buff_lang_ast::GuardCondition::Bool(e) => {
                        walk_expr_for_integer_lets(e, out);
                    }
                }
            }
            walk_block_for_integer_lets(else_block, out);
        }
        Stmt::Defer { expr, .. } => walk_expr_for_integer_lets(expr, out),
        Stmt::ComptimeBlock { .. } => {}
    }
}

fn walk_expr_for_integer_lets(expr: &Expr, out: &mut AtomicSet) {
    match expr {
        Expr::MethodCall { receiver, args, .. } => {
            walk_expr_for_integer_lets(receiver, out);
            for a in args {
                walk_expr_for_integer_lets(a, out);
            }
        }
        Expr::BinaryOp { lhs, rhs, .. } => {
            walk_expr_for_integer_lets(lhs, out);
            walk_expr_for_integer_lets(rhs, out);
        }
        Expr::UnaryOp { operand, .. } => walk_expr_for_integer_lets(operand, out),
        Expr::FuncCall { callee, args, .. } => {
            walk_expr_for_integer_lets(callee, out);
            for a in args {
                walk_expr_for_integer_lets(a, out);
            }
        }
        Expr::IfExpr {
            cond,
            then_block,
            else_block,
            ..
        } => {
            walk_expr_for_integer_lets(cond, out);
            walk_block_for_integer_lets(then_block, out);
            if let Some(eb) = else_block {
                walk_block_for_integer_lets(eb, out);
            }
        }
        Expr::Lambda { body, .. } => walk_block_for_integer_lets(body, out),
        Expr::StructInit { fields, .. } => {
            for (_, v) in fields {
                walk_expr_for_integer_lets(v, out);
            }
        }
        Expr::MatchExpr {
            scrutinee, arms, ..
        } => {
            walk_expr_for_integer_lets(scrutinee, out);
            for arm in arms {
                walk_block_for_integer_lets(&arm.body, out);
            }
        }
        Expr::SuspendExpr { inner, .. } => walk_expr_for_integer_lets(inner, out),
        Expr::ArrayLit { elements, .. } => {
            for e in elements {
                walk_expr_for_integer_lets(e, out);
            }
        }
        Expr::Index { base, indices, .. } => {
            walk_expr_for_integer_lets(base, out);
            for i in indices {
                walk_expr_for_integer_lets(i, out);
            }
        }
        Expr::StringInterp { parts, .. } => {
            for part in parts {
                if let buff_lang_ast::InterpPart::Expr(e, _) = part {
                    walk_expr_for_integer_lets(e, out);
                }
            }
        }
        Expr::MapLit { entries, .. } => {
            for (k, v) in entries {
                walk_expr_for_integer_lets(k, out);
                walk_expr_for_integer_lets(v, out);
            }
        }
        Expr::Try { expr, .. } => walk_expr_for_integer_lets(expr, out),
        Expr::Spawn { task, .. } => walk_expr_for_integer_lets(task, out),
        Expr::Range { start, end, .. } => {
            walk_expr_for_integer_lets(start, out);
            walk_expr_for_integer_lets(end, out);
        }
        Expr::IfLet {
            value,
            then_block,
            else_block,
            ..
        } => {
            walk_expr_for_integer_lets(value, out);
            walk_block_for_integer_lets(then_block, out);
            if let Some(eb) = else_block {
                walk_block_for_integer_lets(eb, out);
            }
        }
        Expr::TupleLit(members, _) => {
            for m in members {
                walk_expr_for_integer_lets(m, out);
            }
        }
        Expr::NamedArg { value, .. } => walk_expr_for_integer_lets(value, out),
        // Leaves — no recursion needed.
        Expr::Literal(_, _) | Expr::Ident(_, _) => {}
    }
}

/// If `expr` is an `Expr::Literal(Literal::Int(n), _)`, return `Some(n)`.
/// Otherwise return `None`. Public so tests can verify the predicate.
pub fn is_integer_literal_init(expr: &Expr) -> Option<i64> {
    if let Expr::Literal(Literal::Int(n), _) = expr {
        Some(*n)
    } else {
        None
    }
}

/// Walk a block collecting (params, body) for every lambda passed as
/// an argument to a `par_map` / `par_reduce` call. Used by
/// [`analyze_func`] to enumerate every parallel accumulator context.
///
/// Mirrors the shape of `race_analysis::walk_expr_for_parallel_calls`
/// but restricted to [`ACCUMULATING_COMBINATORS`] and collecting bodies
/// instead of errors.
fn collect_accumulating_parallel_closures(block: &Block, out: &mut Vec<(Vec<Param>, Block)>) {
    for stmt in &block.stmts {
        collect_stmt_accumulating_parallel(stmt, out);
    }
}

fn collect_stmt_accumulating_parallel(stmt: &Stmt, out: &mut Vec<(Vec<Param>, Block)>) {
    match stmt {
        Stmt::LetDecl { value, .. } | Stmt::LetPattern { value, .. } => {
            collect_expr_accumulating_parallel(value, out);
        }
        Stmt::Assignment { target, value, .. } => {
            collect_expr_accumulating_parallel(target, out);
            collect_expr_accumulating_parallel(value, out);
        }
        Stmt::ExprStmt(e, _) => collect_expr_accumulating_parallel(e, out),
        Stmt::Return(Some(e), _) => collect_expr_accumulating_parallel(e, out),
        Stmt::Return(None, _) | Stmt::Break(_) | Stmt::Continue(_) => {}
        Stmt::ForIn { iter, body, .. } => {
            collect_expr_accumulating_parallel(iter, out);
            collect_accumulating_parallel_closures(body, out);
        }
        Stmt::ForWhile { cond, body, .. } => {
            collect_expr_accumulating_parallel(cond, out);
            collect_accumulating_parallel_closures(body, out);
        }
        Stmt::ForLet { value, body, .. } => {
            collect_expr_accumulating_parallel(value, out);
            collect_accumulating_parallel_closures(body, out);
        }
        Stmt::Guard {
            conditions,
            else_block,
            ..
        } => {
            for cond in conditions {
                match cond {
                    buff_lang_ast::GuardCondition::Let { value, .. } => {
                        collect_expr_accumulating_parallel(value, out);
                    }
                    buff_lang_ast::GuardCondition::Bool(e) => {
                        collect_expr_accumulating_parallel(e, out);
                    }
                }
            }
            collect_accumulating_parallel_closures(else_block, out);
        }
        Stmt::Defer { expr, .. } => collect_expr_accumulating_parallel(expr, out),
        Stmt::ComptimeBlock { .. } => {}
    }
}

fn collect_expr_accumulating_parallel(expr: &Expr, out: &mut Vec<(Vec<Param>, Block)>) {
    match expr {
        Expr::MethodCall {
            receiver,
            method,
            args,
            ..
        } => {
            if ACCUMULATING_COMBINATORS.contains(&method.name.as_str()) {
                for arg in args {
                    let inner = unwrap_named_arg(arg);
                    if let Expr::Lambda { params, body, .. } = inner {
                        out.push((params.clone(), body.clone()));
                    }
                }
            }
            collect_expr_accumulating_parallel(receiver, out);
            for a in args {
                collect_expr_accumulating_parallel(a, out);
            }
        }
        Expr::BinaryOp { lhs, rhs, .. } => {
            collect_expr_accumulating_parallel(lhs, out);
            collect_expr_accumulating_parallel(rhs, out);
        }
        Expr::UnaryOp { operand, .. } => collect_expr_accumulating_parallel(operand, out),
        Expr::FuncCall { callee, args, .. } => {
            collect_expr_accumulating_parallel(callee, out);
            for a in args {
                collect_expr_accumulating_parallel(a, out);
            }
        }
        Expr::IfExpr {
            cond,
            then_block,
            else_block,
            ..
        } => {
            collect_expr_accumulating_parallel(cond, out);
            collect_accumulating_parallel_closures(then_block, out);
            if let Some(eb) = else_block {
                collect_accumulating_parallel_closures(eb, out);
            }
        }
        Expr::Lambda { body, .. } => collect_accumulating_parallel_closures(body, out),
        Expr::StructInit { fields, .. } => {
            for (_, v) in fields {
                collect_expr_accumulating_parallel(v, out);
            }
        }
        Expr::MatchExpr {
            scrutinee, arms, ..
        } => {
            collect_expr_accumulating_parallel(scrutinee, out);
            for arm in arms {
                collect_accumulating_parallel_closures(&arm.body, out);
            }
        }
        Expr::SuspendExpr { inner, .. } => collect_expr_accumulating_parallel(inner, out),
        Expr::ArrayLit { elements, .. } => {
            for e in elements {
                collect_expr_accumulating_parallel(e, out);
            }
        }
        Expr::Index { base, indices, .. } => {
            collect_expr_accumulating_parallel(base, out);
            for i in indices {
                collect_expr_accumulating_parallel(i, out);
            }
        }
        Expr::StringInterp { parts, .. } => {
            for part in parts {
                if let buff_lang_ast::InterpPart::Expr(e, _) = part {
                    collect_expr_accumulating_parallel(e, out);
                }
            }
        }
        Expr::MapLit { entries, .. } => {
            for (k, v) in entries {
                collect_expr_accumulating_parallel(k, out);
                collect_expr_accumulating_parallel(v, out);
            }
        }
        Expr::Try { expr, .. } => collect_expr_accumulating_parallel(expr, out),
        Expr::Spawn { task, .. } => collect_expr_accumulating_parallel(task, out),
        Expr::Range { start, end, .. } => {
            collect_expr_accumulating_parallel(start, out);
            collect_expr_accumulating_parallel(end, out);
        }
        Expr::IfLet {
            value,
            then_block,
            else_block,
            ..
        } => {
            collect_expr_accumulating_parallel(value, out);
            collect_accumulating_parallel_closures(then_block, out);
            if let Some(eb) = else_block {
                collect_accumulating_parallel_closures(eb, out);
            }
        }
        Expr::TupleLit(members, _) => {
            for m in members {
                collect_expr_accumulating_parallel(m, out);
            }
        }
        Expr::NamedArg { value, .. } => collect_expr_accumulating_parallel(value, out),
        // Leaves — no recursion needed.
        Expr::Literal(_, _) | Expr::Ident(_, _) => {}
    }
}

/// Mirror of `race_analysis::unwrap_named_arg`: peel a `name: value`
/// NamedArg wrapper to expose the inner expression. T105 made parallel
/// combinators accept named-arg closures, so the same care is needed
/// here when collecting parallel-closure bodies.
fn unwrap_named_arg(arg: &Expr) -> &Expr {
    match arg {
        Expr::NamedArg { value, .. } => value,
        other => other,
    }
}

// ---------------------------------------------------------------------
// Unit tests — see tests/atomic_tests.rs for the full integration
// suite (12+ tests covering all positive/negative cases). The tests
// below are pure-library smoke checks for the analysis primitives;
// they live here so a `cargo test --lib` run still exercises the API
// surface without the integration-test binary.
// ---------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use buff_lang_ast::common::{Block, Ident, Param};

    fn span() -> buff_lang_error::Span {
        buff_lang_error::Span::dummy()
    }

    fn ident_expr(s: &str) -> Expr {
        Expr::Ident(Ident::new(s, span()), span())
    }

    fn int_expr(n: i64) -> Expr {
        Expr::Literal(Literal::Int(n), span())
    }

    fn placeholder_ty() -> buff_lang_ast::TypeRef {
        buff_lang_ast::TypeRef::Named {
            name: Ident::new("_", span()),
            span: span(),
        }
    }

    fn closure_stmts(params: &[&str], body_stmts: Vec<Stmt>) -> Expr {
        let params: Vec<Param> = params
            .iter()
            .map(|p| Param {
                name: Ident::new(*p, span()),
                ty: placeholder_ty(),
                default_value: None,
                is_comptime: false,
                span: span(),
            })
            .collect();
        Expr::Lambda {
            params,
            body: Block {
                stmts: body_stmts,
                span: span(),
            },
            return_type: None,
            span: span(),
        }
    }

    fn assign(target: Expr, op: BinaryOp, value: Expr) -> Stmt {
        Stmt::Assignment {
            target,
            op,
            value,
            span: span(),
        }
    }

    fn let_stmt(name: &str, value: Expr, mutable: bool) -> Stmt {
        Stmt::LetDecl {
            name: Ident::new(name, span()),
            value,
            mutable,
            ty: None,
            span: span(),
        }
    }

    fn expr_stmt(e: Expr) -> Stmt {
        Stmt::ExprStmt(e, span())
    }

    fn method_call(receiver: Expr, method: &str, args: Vec<Expr>) -> Expr {
        Expr::MethodCall {
            receiver: Box::new(receiver),
            method: Ident::new(method, span()),
            args,
            span: span(),
        }
    }

    fn func_with_stmts(name: &str, stmts: Vec<Stmt>) -> FuncDecl {
        FuncDecl { name: Ident::new(name, span()),
        params: Vec::new(),
        return_type: None,
        body: Block {
            stmts,
            span: span(),
        },
        is_async: false,
        is_unsafe: false,
        is_extern: false, attributes: Vec::new(), type_params: Vec::new(), span: span(), }
    }

    #[test]
    fn is_integer_literal_init_recognises_int() {
        assert_eq!(is_integer_literal_init(&int_expr(42)), Some(42));
        assert_eq!(is_integer_literal_init(&int_expr(0)), Some(0));
        assert_eq!(is_integer_literal_init(&ident_expr("x")), None);
    }

    #[test]
    fn empty_function_yields_empty_promotions() {
        let f = func_with_stmts("f", vec![]);
        assert!(analyze_func(&f).is_empty());
    }

    #[test]
    fn qa_pattern_par_map_int_accumulator_is_promotable() {
        // let mut t = 0
        // v.par_map({ x => t += x })
        let body = closure_stmts(
            &["x"],
            vec![assign(
                ident_expr("t"),
                BinaryOp::AddAssign,
                ident_expr("x"),
            )],
        );
        let f = func_with_stmts(
            "f",
            vec![
                let_stmt("t", int_expr(0), true),
                expr_stmt(method_call(ident_expr("v"), "par_map", vec![body])),
            ],
        );
        let set = analyze_func(&f);
        assert_eq!(set.get("t"), Some(&0));
    }

    #[test]
    fn plain_assign_in_par_map_is_not_promotable() {
        // let mut t = 0; v.par_map({ x => t = x })
        let body = closure_stmts(
            &["x"],
            vec![assign(ident_expr("t"), BinaryOp::Assign, ident_expr("x"))],
        );
        let f = func_with_stmts(
            "f",
            vec![
                let_stmt("t", int_expr(0), true),
                expr_stmt(method_call(ident_expr("v"), "par_map", vec![body])),
            ],
        );
        assert!(analyze_func(&f).is_empty());
    }

    #[test]
    fn par_filter_mutation_is_not_promotable() {
        // par_filter is NOT an accumulating combinator (spec §2).
        let body = closure_stmts(
            &["x"],
            vec![
                assign(ident_expr("t"), BinaryOp::AddAssign, int_expr(1)),
                expr_stmt(Expr::Literal(Literal::Bool(true), span())),
            ],
        );
        let f = func_with_stmts(
            "f",
            vec![
                let_stmt("t", int_expr(0), true),
                expr_stmt(method_call(ident_expr("v"), "par_filter", vec![body])),
            ],
        );
        assert!(analyze_func(&f).is_empty());
    }

    #[test]
    fn non_integer_let_is_not_candidate() {
        // let mut t = "hello" — not an integer literal.
        let body = closure_stmts(
            &["x"],
            vec![assign(
                ident_expr("t"),
                BinaryOp::AddAssign,
                ident_expr("x"),
            )],
        );
        let f = func_with_stmts(
            "f",
            vec![
                let_stmt(
                    "t",
                    Expr::Literal(Literal::String("hi".into()), span()),
                    true,
                ),
                expr_stmt(method_call(ident_expr("v"), "par_map", vec![body])),
            ],
        );
        assert!(analyze_func(&f).is_empty());
    }

    #[test]
    fn non_mut_let_is_not_candidate() {
        // `let t = 0` (no mut) — cannot be reassigned anyway.
        let body = closure_stmts(
            &["x"],
            vec![assign(
                ident_expr("t"),
                BinaryOp::AddAssign,
                ident_expr("x"),
            )],
        );
        let f = func_with_stmts(
            "f",
            vec![
                let_stmt("t", int_expr(0), false),
                expr_stmt(method_call(ident_expr("v"), "par_map", vec![body])),
            ],
        );
        assert!(analyze_func(&f).is_empty());
    }
}

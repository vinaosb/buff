//! Race detection for parallel closures (T41).
//!
//! Buff v1.0 introduces parallel combinators (`par_map`, `par_filter`,
//! `par_reduce`) whose closures execute concurrently across multiple
//! threads (backed by rayon inside [`buff_lang_runtime::CpuDispatcher`]).
//! A closure that **mutates** a variable captured from the enclosing
//! scope would race with itself across the worker threads — undefined
//! behaviour at runtime. T41 is the **static detection** pass that
//! rejects such programs at compile time.
//!
//! ## Scope (T41 detect-only)
//!
//! This pass **detects and rejects** mutable capture in parallel
//! closures. It does NOT transform racy code into atomics — that is
//! T42's job (`AtomicI64` auto-insertion for `par_reduce` accumulators).
//! For T41, every mutable capture in a parallel closure is a hard
//! [`ParallelMutabilityError`].
//!
//! ## Rule
//!
//! For every closure passed as an argument to a parallel combinator
//! (`par_map` / `par_filter` / `par_reduce`), the pass walks the
//! closure body (recursing into nested closures) looking for any
//! [`Stmt::Assignment`] whose target is a bare [`Expr::Ident`] naming
//! a variable captured from the enclosing scope. If found, the pass
//! returns an error.
//!
//! What counts as a CAPTURE (race-prone) vs a LOCAL (always-fine):
//! - **Closure parameters** (`x` in `{ x => ... }`) — local, mutating
//!   is fine.
//! - **`let` bindings INSIDE the closure body** — local, mutating is
//!   fine.
//! - **Variables read or written inside the closure that are NOT bound
//!   by a param or an inner `let`** — captured from the enclosing
//!   scope. Mutating these in a parallel closure is a race.
//!
//! Immutable reads of captured variables are always fine.
//!
//! ## Detection algorithm
//!
//! 1. Walk the entire program looking for [`Expr::MethodCall`] whose
//!    `method.name` is in [`PARALLEL_COMBINATORS`].
//! 2. For each such call, for each [`Expr::Lambda`] argument, compute
//!    the captured-variable set via
//!    [`buff_lang_types::closure_captures`] (a deterministic
//!    `BTreeSet<String>` — never `HashMap`, per the T29 flaky-test
//!    lesson).
//! 3. Walk the lambda body (recursing into nested closures, control
//!    flow, and expressions) collecting every [`Stmt::Assignment`]
//!    whose `op` is an assignment operator ([`is_assignment_op`]) and
//!    whose target is a bare `Ident` whose name is in the captures
//!    set.
//! 4. If any such mutation is found, return the first as a
//!    [`ParallelMutabilityError`].
//!
//! ## What's NOT parallel
//!
//! Plain `.map` / `.filter` / `.reduce` (without the `par_` prefix)
//! are **sequential** in Buff v1.0 codegen — they lower to
//! `recv.into_iter().map(...).collect::<Vec<_>>()` (single-threaded).
//! Mutating captures there is fine (no race); Rust's borrow checker
//! would also permit it. The race detector ONLY flags calls to the
//! `par_*` combinators.
//!
//! ## Determinism
//!
//! Every collection used here is a `BTreeSet` / `BTreeMap` /
//! `Vec` — iteration order is independent of hash seed. The walker
//! visits AST nodes in source order and returns the FIRST mutation
//! site encountered, so the same input program always yields the
//! same error (or the same `Ok(())`).
//!
//! ## What this file owns
//!
//! - [`ParallelMutabilityError`] — the error type.
//! - [`analyze`] — the top-level entry: walks a slice of [`Decl`]
//!   and returns the first detected race, if any.
//! - [`PARALLEL_COMBINATORS`] / [`is_assignment_op`] — public
//!   constants/helpers tests can reference.

use std::collections::BTreeSet;

use buff_lang_ast::{common::Block, decl::FuncDecl, Decl, Expr, GuardCondition, Stmt};
use buff_lang_error::{CodegenError, Diagnostic, Span};

/// Method names that dispatch their closure argument(s) across worker
/// threads in the buff-lang-runtime. Today this is the `par_*` family
/// added by T39 on [`buff_lang_runtime::CpuDispatcher`].
///
/// **NOT in this set**: the plain `.map` / `.filter` / `.reduce`
/// combinators — those lower to sequential `into_iter().map(...)` and
/// are NOT races. Adding a name here makes the race detector flag
/// mutable capture in its closure argument(s).
pub const PARALLEL_COMBINATORS: &[&str] = &["par_map", "par_filter", "par_reduce"];

/// A data-race-via-mutable-capture error detected by [`analyze`].
///
/// Carries the offending variable name and the span of the assignment
/// site (so a rendered diagnostic can point at the exact line). The
/// span is the [`Stmt::Assignment::span`], NOT the closure span or
/// the parallel-combinator call span — pointing at the mutation site
/// is the most actionable for the user.
///
/// Tests assert on this type via [`Self::variable`] and via the
/// `From<ParallelMutabilityError> for CodegenError` impl, which
/// stamps a stable, parseable message prefix the test suite matches
/// against.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParallelMutabilityError {
    /// The captured variable name being mutated.
    pub variable: String,
    /// The span of the offending assignment statement.
    pub span: Span,
}

impl ParallelMutabilityError {
    /// Construct a new error pointing at `span` (the mutation site).
    pub fn new(variable: String, span: Span) -> Self {
        Self { variable, span }
    }

    /// The captured variable name being mutated. Tests query this to
    /// assert WHICH variable triggered the race.
    pub fn variable(&self) -> &str {
        &self.variable
    }

    /// The span of the offending assignment statement. Tests query
    /// this when they need to assert the diagnostic points at the
    /// right location.
    pub fn span(&self) -> Span {
        self.span
    }

    /// Build the user-facing [`Diagnostic`] for this error. The
    /// message format is the SINGLE source of truth — tests match
    /// against the substrings `"ParallelMutability"` and the quoted
    /// variable name, so changing this format requires updating the
    /// tests in lockstep.
    ///
    /// Format: `ParallelMutability: cannot mutate captured variable
    /// `<name>` inside a parallel closure (data race)`.
    ///
    /// We use a stable, prefix-tagged message (NOT a `Display` impl
    /// on [`ParallelMutabilityError`]) so a downstream test harness
    /// that only has the [`CodegenError`] in hand can still detect
    /// the variant via substring match — `CodegenError` is a struct
    /// wrapping a single [`Diagnostic`], not an enum, so there's no
    /// natural `kind()` discriminant without changing the upstream
    /// crate.
    pub fn into_diagnostic(self) -> Diagnostic {
        Diagnostic::error(
            format!(
                "ParallelMutability: cannot mutate captured variable `{}` inside a parallel closure (data race)",
                self.variable
            ),
            self.span,
        )
        .with_note(
            "parallel closures (par_map/par_filter/par_reduce) run on \
             multiple worker threads; mutating a captured variable races \
             across workers".to_string(),
        )
        .with_note(
            "hint: make the capture immutable, or use a local accumulator \
             (a future task will auto-insert atomics for some patterns)"
                .to_string(),
        )
    }
}

impl From<ParallelMutabilityError> for CodegenError {
    fn from(err: ParallelMutabilityError) -> Self {
        CodegenError::new(err.into_diagnostic())
    }
}

impl std::fmt::Display for ParallelMutabilityError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "ParallelMutability: cannot mutate captured variable `{}` inside a parallel closure",
            self.variable
        )
    }
}

/// `true` iff `op` performs a write to its LHS target.
///
/// Covers both the plain assignment (`=`) and the five compound
/// assignments (`+=`, `-=`, `*=`, `/=`, `%=`). Bitwise compound
/// assignments (`<<=`, `>>=`, `&=`, `|=`, `^=`) are NOT in Buff's
/// `BinaryOp` (see `crates/buff-lang-ast/src/op.rs`), so they're not
/// listed here.
pub fn is_assignment_op(op: &buff_lang_ast::op::BinaryOp) -> bool {
    use buff_lang_ast::op::BinaryOp;
    matches!(
        op,
        BinaryOp::Assign
            | BinaryOp::AddAssign
            | BinaryOp::SubAssign
            | BinaryOp::MulAssign
            | BinaryOp::DivAssign
            | BinaryOp::ModAssign
    )
}

/// Run race detection over the entire program. Returns `Ok(())` if
/// no parallel closure mutates a captured variable, or `Err(first
/// detected)` otherwise.
///
/// Walks EVERY function body in `decls`, recursing into every
/// [`Expr::MethodCall`] looking for parallel combinators. The walk
/// visits source order and returns the first detected mutation
/// (deterministic — same input always yields the same error site).
///
/// This is invoked from [`crate::rust_codegen::RustCodegen::generate`]
/// BEFORE per-function lowering starts, so any race is reported
/// before codegen emits a single line of Rust. Returning `Err` here
/// propagates up through `generate_rust` as a [`CodegenError`]
/// (via the `From` impl).
pub fn analyze(decls: &[Decl]) -> Result<(), ParallelMutabilityError> {
    for decl in decls {
        // T41: race analysis only concerns itself with function bodies.
        // Other top-level decls (struct/enum/trait/extend/import/etc.)
        // don't contain executable closure-receiving call sites.
        if let Decl::FuncDecl(func) = decl {
            analyze_func(func)?;
        }
    }
    Ok(())
}

/// Run race detection on a single function body.
///
/// Walks every statement / expression looking for parallel-combinator
/// call sites. Re-exported (via the file-level visibility) so tests
/// can drive it directly on a synthesized [`FuncDecl`] without
/// wrapping in a `Vec<Decl>`.
pub fn analyze_func(func: &FuncDecl) -> Result<(), ParallelMutabilityError> {
    let mut errors: Vec<ParallelMutabilityError> = Vec::new();
    walk_block_for_parallel_calls(&func.body, &mut errors);
    // Determinism: errors are collected in source order; pick the
    // first. (There can be more than one — we report the earliest
    // mutation site the walker reached.)
    if let Some(first) = errors.into_iter().next() {
        return Err(first);
    }
    Ok(())
}

// ---------------------------------------------------------------------
// Walker — find parallel-combinator call sites
// ---------------------------------------------------------------------

fn walk_block_for_parallel_calls(block: &Block, errors: &mut Vec<ParallelMutabilityError>) {
    for stmt in &block.stmts {
        walk_stmt_for_parallel_calls(stmt, errors);
    }
}

#[allow(clippy::too_many_lines)]
fn walk_stmt_for_parallel_calls(stmt: &Stmt, errors: &mut Vec<ParallelMutabilityError>) {
    match stmt {
        Stmt::LetDecl { value, .. } => walk_expr_for_parallel_calls(value, errors),
        Stmt::LetPattern { value, .. } => walk_expr_for_parallel_calls(value, errors),
        Stmt::Assignment { target, value, .. } => {
            walk_expr_for_parallel_calls(target, errors);
            walk_expr_for_parallel_calls(value, errors);
        }
        Stmt::ExprStmt(e, _) => walk_expr_for_parallel_calls(e, errors),
        Stmt::Return(Some(e), _) => walk_expr_for_parallel_calls(e, errors),
        Stmt::Return(None, _) | Stmt::Break(_) | Stmt::Continue(_) => {}
        Stmt::ForIn { iter, body, .. } => {
            walk_expr_for_parallel_calls(iter, errors);
            walk_block_for_parallel_calls(body, errors);
        }
        Stmt::ForWhile { cond, body, .. } => {
            walk_expr_for_parallel_calls(cond, errors);
            walk_block_for_parallel_calls(body, errors);
        }
        Stmt::ForLet { value, body, .. } => {
            walk_expr_for_parallel_calls(value, errors);
            walk_block_for_parallel_calls(body, errors);
        }
        Stmt::Guard {
            conditions,
            else_block,
            ..
        } => {
            for cond in conditions {
                match cond {
                    GuardCondition::Let { value, .. } => {
                        walk_expr_for_parallel_calls(value, errors);
                    }
                    GuardCondition::Bool(e) => {
                        walk_expr_for_parallel_calls(e, errors);
                    }
                }
            }
            walk_block_for_parallel_calls(else_block, errors);
        }
        Stmt::Defer { expr, .. } => walk_expr_for_parallel_calls(expr, errors),
    }
}

fn walk_expr_for_parallel_calls(expr: &Expr, errors: &mut Vec<ParallelMutabilityError>) {
    match expr {
        Expr::MethodCall {
            receiver,
            method,
            args,
            ..
        } => {
            // If this is a parallel combinator, scan each lambda arg
            // for captured-variable mutation.
            if PARALLEL_COMBINATORS.contains(&method.name.as_str()) {
                for arg in args {
                    if let Expr::Lambda { params, body, .. } = unwrap_named_arg(arg) {
                        check_parallel_lambda(params, body, errors);
                    }
                }
            }
            // Recurse regardless — nested method calls may contain
            // further parallel combinators (e.g.
            // `u.filter(...).par_map({...})`).
            walk_expr_for_parallel_calls(receiver, errors);
            for arg in args {
                walk_expr_for_parallel_calls(arg, errors);
            }
        }
        // Recurse into the sub-expressions of every other variant so
        // we catch parallel calls nested inside binary ops, function
        // call args, struct init values, array literals, etc.
        Expr::BinaryOp { lhs, rhs, .. } => {
            walk_expr_for_parallel_calls(lhs, errors);
            walk_expr_for_parallel_calls(rhs, errors);
        }
        Expr::UnaryOp { operand, .. } => walk_expr_for_parallel_calls(operand, errors),
        Expr::FuncCall { callee, args, .. } => {
            walk_expr_for_parallel_calls(callee, errors);
            for a in args {
                walk_expr_for_parallel_calls(a, errors);
            }
        }
        Expr::IfExpr {
            cond,
            then_block,
            else_block,
            ..
        } => {
            walk_expr_for_parallel_calls(cond, errors);
            walk_block_for_parallel_calls(then_block, errors);
            if let Some(eb) = else_block {
                walk_block_for_parallel_calls(eb, errors);
            }
        }
        // Lambda: recurse into its body. The body may itself contain
        // a parallel call (e.g. a closure that internally does
        // `par_map`). The captures of THIS lambda don't matter —
        // they only matter when THIS lambda is itself the arg to a
        // parallel combinator (handled above).
        Expr::Lambda { body, .. } => walk_block_for_parallel_calls(body, errors),
        Expr::StructInit { fields, .. } => {
            for (_, v) in fields {
                walk_expr_for_parallel_calls(v, errors);
            }
        }
        Expr::MatchExpr {
            scrutinee, arms, ..
        } => {
            walk_expr_for_parallel_calls(scrutinee, errors);
            for arm in arms {
                walk_block_for_parallel_calls(&arm.body, errors);
            }
        }
        Expr::SuspendExpr { inner, .. } => walk_expr_for_parallel_calls(inner, errors),
        Expr::ArrayLit { elements, .. } => {
            for e in elements {
                walk_expr_for_parallel_calls(e, errors);
            }
        }
        Expr::Index { base, indices, .. } => {
            walk_expr_for_parallel_calls(base, errors);
            for i in indices {
                walk_expr_for_parallel_calls(i, errors);
            }
        }
        Expr::StringInterp { parts, .. } => {
            for part in parts {
                if let buff_lang_ast::InterpPart::Expr(e) = part {
                    walk_expr_for_parallel_calls(e, errors);
                }
            }
        }
        Expr::MapLit { entries, .. } => {
            for (k, v) in entries {
                walk_expr_for_parallel_calls(k, errors);
                walk_expr_for_parallel_calls(v, errors);
            }
        }
        Expr::Try { expr, .. } => walk_expr_for_parallel_calls(expr, errors),
        Expr::Spawn { task, .. } => walk_expr_for_parallel_calls(task, errors),
        Expr::Range { start, end, .. } => {
            walk_expr_for_parallel_calls(start, errors);
            walk_expr_for_parallel_calls(end, errors);
        }
        Expr::IfLet {
            value,
            then_block,
            else_block,
            ..
        } => {
            walk_expr_for_parallel_calls(value, errors);
            walk_block_for_parallel_calls(then_block, errors);
            if let Some(eb) = else_block {
                walk_block_for_parallel_calls(eb, errors);
            }
        }
        Expr::TupleLit(members, _) => {
            for m in members {
                walk_expr_for_parallel_calls(m, errors);
            }
        }
        Expr::NamedArg { value, .. } => walk_expr_for_parallel_calls(value, errors),
        // Leaves — no recursion needed.
        Expr::Literal(_, _) | Expr::Ident(_, _) => {}
    }
}

/// T105: a named arg `name: value` may wrap a closure. The race
/// detector needs to see THROUGH the NamedArg wrapper to find the
/// underlying Lambda. Returns the inner expression (the Lambda) if
/// present; otherwise the arg itself (which the caller treats as
/// "not a lambda").
fn unwrap_named_arg(arg: &Expr) -> &Expr {
    match arg {
        Expr::NamedArg { value, .. } => value,
        other => other,
    }
}

/// Check one parallel-combinator lambda argument for mutable-capture
/// races. Appends any detected mutations to `errors` (in source order).
fn check_parallel_lambda(
    params: &[buff_lang_ast::common::Param],
    body: &Block,
    errors: &mut Vec<ParallelMutabilityError>,
) {
    // Reuse the deterministic capture analysis from buff_lang_types.
    // This returns the set of names the lambda reads or writes that
    // are NOT bound by params or by lets inside the body — i.e. the
    // names captured from the enclosing scope.
    let captures: BTreeSet<String> = buff_lang_types::closure_captures(params, body);
    if captures.is_empty() {
        // No captures → no possible race. Skip the body walk entirely
        // (cheap fast path that also keeps error messages focused).
        return;
    }
    walk_body_for_captured_mutations(body, &captures, errors);
}

// ---------------------------------------------------------------------
// Walker — find mutations of captured variables inside a parallel
// closure body
// ---------------------------------------------------------------------

fn walk_body_for_captured_mutations(
    block: &Block,
    captures: &BTreeSet<String>,
    errors: &mut Vec<ParallelMutabilityError>,
) {
    for stmt in &block.stmts {
        walk_stmt_for_captured_mutations(stmt, captures, errors);
    }
}

fn walk_stmt_for_captured_mutations(
    stmt: &Stmt,
    captures: &BTreeSet<String>,
    errors: &mut Vec<ParallelMutabilityError>,
) {
    match stmt {
        Stmt::Assignment {
            target,
            op,
            value,
            span,
        } => {
            // Mutation of a captured variable: target is a bare
            // Ident naming a captured var. The op must be a write
            // (covers `=` and all compound assignments).
            if is_assignment_op(op) {
                if let Expr::Ident(name, _) = target {
                    if captures.contains(&name.name) {
                        errors.push(ParallelMutabilityError::new(name.name.clone(), *span));
                    }
                }
            }
            // Recurse in case the target or value contains a nested
            // closure with its own mutations of these captures.
            walk_expr_for_captured_mutations(target, captures, errors);
            walk_expr_for_captured_mutations(value, captures, errors);
        }
        Stmt::LetDecl { value, .. } => {
            walk_expr_for_captured_mutations(value, captures, errors);
        }
        Stmt::LetPattern { value, .. } => {
            walk_expr_for_captured_mutations(value, captures, errors);
        }
        Stmt::ExprStmt(e, _) => {
            walk_expr_for_captured_mutations(e, captures, errors);
        }
        Stmt::Return(Some(e), _) => {
            walk_expr_for_captured_mutations(e, captures, errors);
        }
        Stmt::Return(None, _) | Stmt::Break(_) | Stmt::Continue(_) => {}
        Stmt::ForIn { iter, body, .. } => {
            walk_expr_for_captured_mutations(iter, captures, errors);
            walk_body_for_captured_mutations(body, captures, errors);
        }
        Stmt::ForWhile { cond, body, .. } => {
            walk_expr_for_captured_mutations(cond, captures, errors);
            walk_body_for_captured_mutations(body, captures, errors);
        }
        Stmt::ForLet { value, body, .. } => {
            walk_expr_for_captured_mutations(value, captures, errors);
            walk_body_for_captured_mutations(body, captures, errors);
        }
        Stmt::Guard {
            conditions,
            else_block,
            ..
        } => {
            for cond in conditions {
                match cond {
                    GuardCondition::Let { value, .. } => {
                        walk_expr_for_captured_mutations(value, captures, errors);
                    }
                    GuardCondition::Bool(e) => {
                        walk_expr_for_captured_mutations(e, captures, errors);
                    }
                }
            }
            walk_body_for_captured_mutations(else_block, captures, errors);
        }
        Stmt::Defer { expr, .. } => {
            walk_expr_for_captured_mutations(expr, captures, errors);
        }
    }
}

fn walk_expr_for_captured_mutations(
    expr: &Expr,
    captures: &BTreeSet<String>,
    errors: &mut Vec<ParallelMutabilityError>,
) {
    // (see `walk_expr_for_parallel_calls` for the per-variant
    // recursion rationale — this walker mirrors its shape but
    // searches for ASSIGNMENTS rather than method calls.)
    match expr {
        Expr::MethodCall { receiver, args, .. } => {
            walk_expr_for_captured_mutations(receiver, captures, errors);
            for a in args {
                walk_expr_for_captured_mutations(a, captures, errors);
            }
        }
        Expr::BinaryOp { lhs, rhs, .. } => {
            walk_expr_for_captured_mutations(lhs, captures, errors);
            walk_expr_for_captured_mutations(rhs, captures, errors);
        }
        Expr::UnaryOp { operand, .. } => {
            walk_expr_for_captured_mutations(operand, captures, errors);
        }
        Expr::FuncCall { callee, args, .. } => {
            walk_expr_for_captured_mutations(callee, captures, errors);
            for a in args {
                walk_expr_for_captured_mutations(a, captures, errors);
            }
        }
        Expr::IfExpr {
            cond,
            then_block,
            else_block,
            ..
        } => {
            walk_expr_for_captured_mutations(cond, captures, errors);
            walk_body_for_captured_mutations(then_block, captures, errors);
            if let Some(eb) = else_block {
                walk_body_for_captured_mutations(eb, captures, errors);
            }
        }
        // Lambda: recurse into its body. A nested closure that
        // mutates the OUTER parallel closure's captured variables is
        // still a race (the nested closure executes inside the
        // parallel context). We DON'T re-compute captures for the
        // nested closure — we keep the OUTER closure's captures set,
        // which already excludes the nested closure's params/lets
        // (so mutating a nested-closure-local is correctly NOT
        // flagged).
        Expr::Lambda { body, .. } => {
            walk_body_for_captured_mutations(body, captures, errors);
        }
        Expr::StructInit { fields, .. } => {
            for (_, v) in fields {
                walk_expr_for_captured_mutations(v, captures, errors);
            }
        }
        Expr::MatchExpr {
            scrutinee, arms, ..
        } => {
            walk_expr_for_captured_mutations(scrutinee, captures, errors);
            for arm in arms {
                walk_body_for_captured_mutations(&arm.body, captures, errors);
            }
        }
        Expr::SuspendExpr { inner, .. } => {
            walk_expr_for_captured_mutations(inner, captures, errors);
        }
        Expr::ArrayLit { elements, .. } => {
            for e in elements {
                walk_expr_for_captured_mutations(e, captures, errors);
            }
        }
        Expr::Index { base, indices, .. } => {
            walk_expr_for_captured_mutations(base, captures, errors);
            for i in indices {
                walk_expr_for_captured_mutations(i, captures, errors);
            }
        }
        Expr::StringInterp { parts, .. } => {
            for part in parts {
                if let buff_lang_ast::InterpPart::Expr(e) = part {
                    walk_expr_for_captured_mutations(e, captures, errors);
                }
            }
        }
        Expr::MapLit { entries, .. } => {
            for (k, v) in entries {
                walk_expr_for_captured_mutations(k, captures, errors);
                walk_expr_for_captured_mutations(v, captures, errors);
            }
        }
        Expr::Try { expr, .. } => {
            walk_expr_for_captured_mutations(expr, captures, errors);
        }
        Expr::Spawn { task, .. } => {
            walk_expr_for_captured_mutations(task, captures, errors);
        }
        Expr::Range { start, end, .. } => {
            walk_expr_for_captured_mutations(start, captures, errors);
            walk_expr_for_captured_mutations(end, captures, errors);
        }
        Expr::IfLet {
            value,
            then_block,
            else_block,
            ..
        } => {
            walk_expr_for_captured_mutations(value, captures, errors);
            walk_body_for_captured_mutations(then_block, captures, errors);
            if let Some(eb) = else_block {
                walk_body_for_captured_mutations(eb, captures, errors);
            }
        }
        Expr::TupleLit(members, _) => {
            for m in members {
                walk_expr_for_captured_mutations(m, captures, errors);
            }
        }
        Expr::NamedArg { value, .. } => {
            walk_expr_for_captured_mutations(value, captures, errors);
        }
        // Leaves — no recursion needed.
        Expr::Literal(_, _) | Expr::Ident(_, _) => {}
    }
}

// ---------------------------------------------------------------------
// Unit tests — see tests/race_detection_tests.rs for the full
// integration test suite (10+ tests covering all positive/negative
// cases). The tests below are pure-library smoke checks for the
// helper functions and error type; they live here so a `cargo test
// --lib` run still exercises the API surface without the integration
// test binary.
// ---------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parallel_mutability_error_carries_variable_and_span() {
        let err = ParallelMutabilityError::new("total".to_string(), Span::dummy());
        assert_eq!(err.variable(), "total");
        assert_eq!(err.span(), Span::dummy());
    }

    #[test]
    fn parallel_mutability_error_into_diagnostic_has_stable_prefix() {
        let err = ParallelMutabilityError::new("total".to_string(), Span::dummy());
        let diag = err.into_diagnostic();
        assert!(diag.message.contains("ParallelMutability"));
        assert!(diag.message.contains("total"));
        assert_eq!(diag.severity, buff_lang_error::Severity::Error);
    }

    #[test]
    fn parallel_mutability_error_converts_into_codegen_error() {
        let err = ParallelMutabilityError::new("x".to_string(), Span::dummy());
        let cg: CodegenError = err.into();
        assert!(cg.diagnostic.message.contains("ParallelMutability"));
        assert!(cg.diagnostic.message.contains('x'));
    }

    #[test]
    fn is_assignment_op_classifies_correctly() {
        use buff_lang_ast::op::BinaryOp;
        assert!(is_assignment_op(&BinaryOp::Assign));
        assert!(is_assignment_op(&BinaryOp::AddAssign));
        assert!(is_assignment_op(&BinaryOp::SubAssign));
        assert!(is_assignment_op(&BinaryOp::MulAssign));
        assert!(is_assignment_op(&BinaryOp::DivAssign));
        assert!(is_assignment_op(&BinaryOp::ModAssign));
        // Non-assignment ops are NOT mutations.
        assert!(!is_assignment_op(&BinaryOp::Add));
        assert!(!is_assignment_op(&BinaryOp::Eq));
        assert!(!is_assignment_op(&BinaryOp::And));
    }

    #[test]
    fn parallel_combinators_set_is_pinned() {
        // Pin the set so adding/removing a name here is a deliberate
        // decision visible in any code review.
        assert_eq!(
            PARALLEL_COMBINATORS,
            &["par_map", "par_filter", "par_reduce"]
        );
        // Sequential combinators are deliberately NOT in the set.
        // (Adding `.map` here would flag all closure-capture mutation
        // in single-threaded map — too noisy for v1.0.)
        for seq in &["map", "filter", "reduce"] {
            assert!(!PARALLEL_COMBINATORS.contains(seq));
        }
    }
}

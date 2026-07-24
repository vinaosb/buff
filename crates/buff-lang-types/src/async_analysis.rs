//! Async call-graph propagation (T31).
//!
//! Buff's headline async feature is that **there is no `await` keyword**: a
//! function becomes `async` either because it is declared `async func ...` OR
//! because it transitively calls an `async` function. This module is the
//! analysis that performs that propagation.
//!
//! # Algorithm (fixpoint)
//!
//! 1. **Build the call graph.** For every [`Decl::FuncDecl`] in the program
//!    we collect the set of bare-ident callee names appearing inside its
//!    body (any [`Expr::FuncCall`] whose callee is an [`Expr::Ident`]). The
//!    result is a [`CallGraph`] mapping `caller_name -> { callee_name, ... }`.
//!    Recursion (self-edges), cycles, and diamonds are all handled naturally
//!    by the next step.
//!
//! 2. **Seed the async set.** Every function whose [`FuncDecl::is_async`] is
//!    `true` enters the initial [`AsyncSet`].
//!
//! 3. **Run the fixpoint.** Repeat until no change: for every function in the
//!    graph, if any of its direct callees is in the async set, the function
//!    itself joins the async set. The loop terminates because the set only
//!    grows and the universe (function-name set) is finite.
//!
//! 4. **Return the final async set** ([`AsyncSet`]).
//!
//! # Determinism
//!
//! All data structures used here are [`BTreeMap`]/[`BTreeSet`]-based (or
//! sorted `Vec`s). Iteration order is therefore fully deterministic — the
//! same AST always produces byte-identical output. This is the lesson T29
//! taught us (HashMap iteration order made module-graph topo-sort flaky);
//! we apply it pre-emptively here.
//!
//! The fixpoint terminates after at most `N` iterations where `N` is the
//! number of functions: each iteration either grows the async set by at
//! least one element or terminates the loop. Worst case is `O(N²)`
//! edge-relaxations which is negligible for any realistic program.
//!
//! # Cycle / recursion handling
//!
//! Self-recursion (`f` calls `f`) and mutual recursion (`f` calls `g`
//! calls `f`) are handled implicitly: when we relax an edge we look at
//! the **current** async set, so if either `f` or `g` becomes async
//! (because some other callee is async) the partner joins on the next
//! iteration. A cycle of purely-sync functions stays sync — exactly the
//! desired behaviour.
//!
//! # What this module does NOT do
//!
//! - It does NOT insert `.await` suspension points — that's the codegen
//!   pass's job (see `buff_lang_codegen_rust`). This module only answers
//!   "is function X async?".
//! - It does NOT emit `#[tokio::main]` — also codegen's job.
//! - It does NOT lower `spawn` / `block` / `Task<T>.result()` — those are
//!   AST/codegen concerns. The analysis treats them uniformly: if a
//!   function calls any async function (directly or via these idioms),
//!   it becomes async.
//!
//! # Example
//!
//! ```text
//! async func http_get(url)        -> declared async (seed)
//! func fetch()       { http_get(u) }  -> calls async -> async (1-hop)
//! func pipeline()    { fetch() }      -> calls async -> async (transitive)
//! func add(a, b)     { a + b }        -> no async calls -> stays sync
//! ```
//!
//! The analysis returns `{http_get, fetch, pipeline}` and leaves `add`
//! out — exactly what codegen needs to decide which Rust fns get `async`.

use std::collections::{BTreeMap, BTreeSet};

use buff_lang_ast::{Block, Decl, Expr, Stmt};

/// A call graph: maps a function name to the **set** of bare-ident callee
/// names that appear in its body.
///
/// Built by [`build_call_graph`]. The keys are caller names; the values
/// are callee-name sets. Built deterministically (BTreeMap/BTreeSet) so
/// iteration order is stable across runs.
///
/// Self-edges (a function calling itself recursively) are recorded; the
/// fixpoint handles them naturally. Calls to names that are NOT defined
/// functions (prelude fns, free variables, builtin Matrix::new, etc.)
/// are also recorded but they don't matter for propagation — only calls
/// to KNOWN async fns trigger propagation, and prelude fns aren't async.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct CallGraph {
    /// `caller -> { callees }`. Sorted by caller name; callees sorted too.
    pub edges: BTreeMap<String, BTreeSet<String>>,
}

impl CallGraph {
    /// Construct an empty graph.
    pub fn new() -> Self {
        Self::default()
    }

    /// Number of caller nodes in the graph.
    pub fn len(&self) -> usize {
        self.edges.len()
    }

    /// `true` if no callers have been recorded.
    pub fn is_empty(&self) -> bool {
        self.edges.is_empty()
    }

    /// Returns the set of direct callees of `caller`, or an empty set if
    /// the caller has no body / isn't in the graph.
    pub fn callees_of(&self, caller: &str) -> &BTreeSet<String> {
        self.edges
            .get(caller)
            .unwrap_or_else(|| Self::empty_callees())
    }

    /// Returns an empty shared reference for use in [`Self::callees_of`].
    fn empty_callees() -> &'static BTreeSet<String> {
        // Use a leaked-once static for the empty case so we can hand out
        // a `&BTreeSet` without allocating. Built once per process.
        use std::sync::OnceLock;
        static EMPTY: OnceLock<BTreeSet<String>> = OnceLock::new();
        EMPTY.get_or_init(BTreeSet::new)
    }

    /// Iterate `(caller, callees)` pairs in deterministic (sorted) order.
    pub fn iter_sorted(&self) -> impl Iterator<Item = (&String, &BTreeSet<String>)> {
        self.edges.iter()
    }
}

/// The set of function names that are async after fixpoint propagation.
///
/// Always sorted (BTreeSet), so `to_sorted_vec()` / iteration is
/// deterministic across runs.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct AsyncSet {
    /// Function names that are async (declared + propagated).
    pub names: BTreeSet<String>,
}

impl AsyncSet {
    /// Construct an empty set.
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns `true` if `name` is async (declared or propagated).
    pub fn contains(&self, name: &str) -> bool {
        self.names.contains(name)
    }

    /// Returns the async names in sorted order.
    pub fn to_sorted_vec(&self) -> Vec<&String> {
        self.names.iter().collect()
    }

    /// Number of async functions.
    pub fn len(&self) -> usize {
        self.names.len()
    }

    /// `true` if no functions are async.
    pub fn is_empty(&self) -> bool {
        self.names.is_empty()
    }
}

/// Build a [`CallGraph`] from a list of declarations.
///
/// Only [`Decl::FuncDecl`]s contribute nodes (other top-level decls are
/// type metadata / imports / exports of inner decls). The inner body is
/// walked recursively via [`collect_func_calls`]; every `Expr::FuncCall`
/// whose callee is a bare [`Expr::Ident`] records the callee name.
///
/// Method calls (`recv.method(...)`) are NOT recorded as call-graph edges
/// here because their target fn is dynamic (depends on the receiver's
/// type) — only free-function calls propagate async-ness for v0.5.
///
/// The result is deterministic: the underlying [`BTreeMap`]/[`BTreeSet`]
/// produce sorted iteration order so callers can rely on byte-identical
/// output for the same AST.
pub fn build_call_graph(decls: &[Decl]) -> CallGraph {
    let mut graph = CallGraph::new();
    for decl in decls {
        // Unwrap export-wrapped decls so their inner fn body still
        // contributes to the graph (T29 export wraps a FuncDecl in a
        // Decl::ExportDecl).
        let inner = match decl {
            Decl::FuncDecl(f) => Some(f),
            Decl::ExportDecl(e) => match e.inner.as_ref() {
                Decl::FuncDecl(f) => Some(f),
                _ => None,
            },
            _ => None,
        };
        let Some(f) = inner else {
            continue;
        };
        let mut callees: BTreeSet<String> = BTreeSet::new();
        collect_func_calls_in_block(&f.body, &mut callees);
        graph.edges.insert(f.name.name.clone(), callees);
    }
    graph
}

/// Recursively collect bare-ident function-call callee names from a block.
fn collect_func_calls_in_block(block: &Block, out: &mut BTreeSet<String>) {
    for stmt in &block.stmts {
        collect_func_calls_in_stmt(stmt, out);
    }
}

/// Walk a statement, recursing into any sub-expressions and nested blocks.
fn collect_func_calls_in_stmt(stmt: &Stmt, out: &mut BTreeSet<String>) {
    match stmt {
        Stmt::LetDecl { value, .. } => collect_func_calls(value, out),
        // T71: only the RHS value can contain callee references.
        Stmt::LetPattern { value, .. } => collect_func_calls(value, out),
        Stmt::ExprStmt(e, _) => collect_func_calls(e, out),
        Stmt::Return(opt_e, _) => {
            if let Some(e) = opt_e {
                collect_func_calls(e, out);
            }
        }
        Stmt::Assignment { target, value, .. } => {
            collect_func_calls(target, out);
            collect_func_calls(value, out);
        }
        Stmt::Break(_) | Stmt::Continue(_) => {}
        Stmt::ForIn { iter, body, .. } => {
            collect_func_calls(iter, out);
            collect_func_calls_in_block(body, out);
        }
        Stmt::ForWhile { cond, body, .. } => {
            collect_func_calls(cond, out);
            collect_func_calls_in_block(body, out);
        }
        // T72: `for let PAT = EXPR { body }` — the value may contain calls;
        // the pattern's bindings don't.
        Stmt::ForLet { value, body, .. } => {
            collect_func_calls(value, out);
            collect_func_calls_in_block(body, out);
        }
        // T73: `guard <conds> else { block }` — each condition's value/expr
        // may contain calls; the else-block recurses.
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
                collect_func_calls(e, out);
            }
            collect_func_calls_in_block(else_block, out);
        }
        // T100: `defer EXPR` — the deferred expression may contain calls.
        Stmt::Defer { expr, .. } => collect_func_calls(expr, out),
        // T53: comptime block — recurse into body for call collection.
        Stmt::ComptimeBlock { body, .. } => collect_func_calls_in_block(body, out),
    }
}

/// Recursively walk an expression, recording every `Expr::FuncCall` whose
/// callee is a bare `Expr::Ident`. The callee name is inserted into `out`.
///
/// We recurse into:
/// - All sub-expressions of any compound expression.
/// - All nested blocks (if/lambda/match arms).
///
/// We do NOT record:
/// - `Expr::MethodCall` callees (dynamic dispatch, type-dependent).
/// - `Expr::FuncCall` callees that aren't bare idents (`(fptr)(x)`).
/// - Prelude fn names — they get recorded but the fixpoint only relaxes
///   edges to KNOWN async functions, so a prelude name that isn't a real
///   declared fn has no effect.
fn collect_func_calls(expr: &Expr, out: &mut BTreeSet<String>) {
    match expr {
        Expr::FuncCall { callee, args, .. } => {
            if let Expr::Ident(name, _) = callee.as_ref() {
                out.insert(name.name.clone());
            } else {
                // Complex callee — recurse into it.
                collect_func_calls(callee, out);
            }
            for a in args {
                collect_func_calls(a, out);
            }
        }
        Expr::MethodCall { receiver, args, .. } => {
            // Method calls don't contribute async edges (dynamic dispatch).
            collect_func_calls(receiver, out);
            for a in args {
                collect_func_calls(a, out);
            }
        }
        Expr::BinaryOp { lhs, rhs, .. } => {
            collect_func_calls(lhs, out);
            collect_func_calls(rhs, out);
        }
        Expr::UnaryOp { operand, .. } => collect_func_calls(operand, out),
        Expr::IfExpr {
            cond,
            then_block,
            else_block,
            ..
        } => {
            collect_func_calls(cond, out);
            collect_func_calls_in_block(then_block, out);
            if let Some(eb) = else_block {
                collect_func_calls_in_block(eb, out);
            }
        }
        Expr::Lambda { body, .. } => collect_func_calls_in_block(body, out),
        Expr::StructInit { fields, .. } => {
            for (_, v) in fields {
                collect_func_calls(v, out);
            }
        }
        Expr::MatchExpr {
            scrutinee, arms, ..
        } => {
            collect_func_calls(scrutinee, out);
            for arm in arms {
                collect_func_calls_in_block(&arm.body, out);
            }
        }
        Expr::ArrayLit { elements, .. } => {
            for e in elements {
                collect_func_calls(e, out);
            }
        }
        Expr::Index { base, indices, .. } => {
            collect_func_calls(base, out);
            for i in indices {
                collect_func_calls(i, out);
            }
        }
        Expr::MapLit { entries, .. } => {
            for (k, v) in entries {
                collect_func_calls(k, out);
                collect_func_calls(v, out);
            }
        }
        Expr::StringInterp { parts, .. } => {
            for p in parts {
                if let buff_lang_ast::InterpPart::Expr(e) = p {
                    collect_func_calls(e, out);
                }
            }
        }
        Expr::SuspendExpr { inner, .. } => collect_func_calls(inner, out),
        Expr::Try { expr, .. } => collect_func_calls(expr, out),
        // T31: `spawn expr` — does NOT contribute to the call graph. A
        // spawn is a task-launch (the spawner kicks off the work on a
        // separate executor), not a function call from the spawner's
        // frame. Per the spec's example `spawn task() → tokio::spawn(async
        // move { task() })`, the spawner stays sync — only the spawned
        // task runs the async body. (The body itself is in an async-block
        // context, but that's a codegen concern, not a propagation one.)
        // We deliberately do NOT call `collect_func_calls(task, out)` here.
        Expr::Spawn { task: _, .. } => {}
        // T68: `start..end` — recurse into both bounds for function calls.
        Expr::Range { start, end, .. } => {
            collect_func_calls(start, out);
            collect_func_calls(end, out);
        }
        // T72: `if let PAT = EXPR { then } else { else }` — recurse into the
        // value and both blocks (the pattern's bindings don't contain calls).
        Expr::IfLet {
            value,
            then_block,
            else_block,
            ..
        } => {
            collect_func_calls(value, out);
            collect_func_calls_in_block(then_block, out);
            if let Some(eb) = else_block {
                collect_func_calls_in_block(eb, out);
            }
        }
        // T103: a tuple literal `(e1, e2, ...)` — recurse into each element.
        Expr::TupleLit(members, _) => {
            for m in members {
                collect_func_calls(m, out);
            }
        }
        // T105: a named arg `name: value` — recurse into the value (the
        // name is metadata, not a call).
        Expr::NamedArg { value, .. } => collect_func_calls(value, out),
        Expr::Literal(_, _) | Expr::Ident(_, _) => {}
    }
}

/// Run the fixpoint propagation: any function that (transitively) calls an
/// async function becomes async. Returns the final [`AsyncSet`].
///
/// The algorithm:
///
/// 1. Seed the async set with declared-async functions (`f.is_async == true`).
/// 2. Repeat until no change: for every caller, if any of its direct
///    callees is in the async set, add the caller to the async set.
/// 3. Return the (now-stable) async set.
///
/// Determinism: every iteration scans callers in sorted (BTreeMap) order
/// and checks callees in sorted (BTreeSet) order, so the result is
/// byte-identical across runs.
///
/// Termination: the set only grows; the universe is finite (one name per
/// function). After at most N iterations the set stabilises.
pub fn propagate_async(graph: &CallGraph, decls: &[Decl]) -> AsyncSet {
    let mut async_set: BTreeSet<String> = BTreeSet::new();

    // 1. Seed with declared-async functions.
    for decl in decls {
        let f = match decl {
            Decl::FuncDecl(f) => f,
            Decl::ExportDecl(e) => match e.inner.as_ref() {
                Decl::FuncDecl(f) => f,
                _ => continue,
            },
            _ => continue,
        };
        if f.is_async {
            async_set.insert(f.name.name.clone());
        }
    }

    // 2. Fixpoint: relax edges until the set stops growing.
    //
    // We scan in sorted (BTreeMap) order so the result is deterministic.
    // The fixpoint terminates because each iteration either grows the
    // set by at least one element or stops.
    loop {
        let mut grew = false;
        for (caller, callees) in graph.edges.iter() {
            // Skip callers already in the async set (no work to do).
            if async_set.contains(caller) {
                continue;
            }
            // If any callee is async, this caller becomes async.
            let calls_async = callees.iter().any(|c| async_set.contains(c));
            if calls_async {
                async_set.insert(caller.clone());
                grew = true;
            }
        }
        if !grew {
            break;
        }
    }

    AsyncSet { names: async_set }
}

/// Convenience: build the call graph AND run propagation in one call.
///
/// Equivalent to:
///
/// ```ignore
/// let graph = build_call_graph(decls);
/// propagate_async(&graph, decls)
/// ```
///
/// Useful when callers don't need the intermediate [`CallGraph`].
pub fn analyze_async(decls: &[Decl]) -> AsyncSet {
    let graph = build_call_graph(decls);
    propagate_async(&graph, decls)
}

/// Returns `true` if the function `name` is async after propagation.
///
/// Convenience wrapper around [`analyze_async`] for callers that only
/// need a single boolean answer. If you need to query multiple functions,
/// prefer calling [`analyze_async`] once and using [`AsyncSet::contains`].
pub fn is_async_after_propagation(decls: &[Decl], name: &str) -> bool {
    analyze_async(decls).contains(name)
}

// ---------------------------------------------------------------------------
// Unit tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use buff_lang_ast::common::{Block, Ident};
    use buff_lang_ast::decl::FuncDecl;
    use buff_lang_error::Span;

    fn dummy() -> Span {
        Span::dummy()
    }

    fn ident_expr(name: &str) -> Expr {
        Expr::Ident(Ident::new(name, dummy()), dummy())
    }

    fn int_expr(n: i64) -> Expr {
        Expr::Literal(buff_lang_ast::Literal::Int(n), dummy())
    }

    fn call_expr(name: &str, args: Vec<Expr>) -> Expr {
        Expr::FuncCall {
            callee: Box::new(ident_expr(name)),
            args,
            span: dummy(),
        }
    }

    /// Build a FuncDecl with the given body and async flag.
    fn func(name: &str, is_async: bool, body_stmts: Vec<Stmt>) -> FuncDecl {
        FuncDecl { name: Ident::new(name, dummy()),
        params: Vec::new(),
        return_type: None,
        body: Block {
            stmts: body_stmts,
            span: dummy(),
        },
        is_async,
        is_unsafe: false,
        is_extern: false, attributes: Vec::new(), type_params: Vec::new(), span: dummy(), }
    }

    fn ret_call(name: &str) -> Stmt {
        Stmt::Return(Some(call_expr(name, Vec::new())), dummy())
    }

    fn ret_int(n: i64) -> Stmt {
        Stmt::Return(Some(int_expr(n)), dummy())
    }

    fn decls(fs: Vec<FuncDecl>) -> Vec<Decl> {
        fs.into_iter().map(Decl::FuncDecl).collect()
    }

    #[test]
    fn empty_program_yields_empty_graph_and_async_set() {
        let graph = build_call_graph(&[]);
        assert!(graph.is_empty());
        let asyncs = propagate_async(&graph, &[]);
        assert!(asyncs.is_empty());
    }

    #[test]
    fn direct_async_mark_seeds_the_set() {
        // async func http_get(url) { ... }   -> in async set
        let f = func("http_get", true, vec![ret_int(0)]);
        let d = decls(vec![f]);
        let asyncs = analyze_async(&d);
        assert!(asyncs.contains("http_get"));
        assert_eq!(asyncs.len(), 1);
    }

    #[test]
    fn sync_fn_with_no_async_calls_stays_sync() {
        // func add(a, b) { a + b }   -> stays sync
        let f = func("add", false, vec![ret_int(0)]);
        let d = decls(vec![f]);
        let asyncs = analyze_async(&d);
        assert!(!asyncs.contains("add"));
        assert!(asyncs.is_empty());
    }

    #[test]
    fn one_hop_propagation() {
        // async func http_get(url)
        // func fetch() { http_get(u) }   -> becomes async
        let io = func("http_get", true, vec![ret_int(0)]);
        let fetch = func("fetch", false, vec![ret_call("http_get")]);
        let d = decls(vec![io, fetch]);
        let asyncs = analyze_async(&d);
        assert!(asyncs.contains("http_get"));
        assert!(asyncs.contains("fetch"));
        assert_eq!(asyncs.len(), 2);
    }

    #[test]
    fn transitive_multi_hop_propagation() {
        // async func http_get(url)
        // func fetch()    { http_get() }
        // func pipeline() { fetch() }
        // func main()     { pipeline() }
        let io = func("http_get", true, vec![ret_int(0)]);
        let fetch = func("fetch", false, vec![ret_call("http_get")]);
        let pipeline = func("pipeline", false, vec![ret_call("fetch")]);
        let main = func("main", false, vec![ret_call("pipeline")]);
        let d = decls(vec![io, fetch, pipeline, main]);
        let asyncs = analyze_async(&d);
        // All four should be async.
        for name in &["http_get", "fetch", "pipeline", "main"] {
            assert!(
                asyncs.contains(name),
                "expected {name} to be async, got: {:?}",
                asyncs.to_sorted_vec()
            );
        }
        assert_eq!(asyncs.len(), 4);
    }

    #[test]
    fn diamond_call_graph_propagates_to_all_paths() {
        // async func io()
        // func left()  { io() }
        // func right() { io() }
        // func top()   { left(); right() }
        let io = func("io", true, vec![ret_int(0)]);
        let left = func("left", false, vec![ret_call("io")]);
        let right = func("right", false, vec![ret_call("io")]);
        let top = func("top", false, vec![ret_call("left"), ret_call("right")]);
        let d = decls(vec![io, left, right, top]);
        let asyncs = analyze_async(&d);
        for name in &["io", "left", "right", "top"] {
            assert!(asyncs.contains(name), "diamond: {name} should be async");
        }
    }

    #[test]
    fn recursion_does_not_force_async_unless_callee_is_async() {
        // func loop(n) { loop(n - 1) }  -- pure recursion, no async -> stays sync
        let rec = func("loop", false, vec![ret_call("loop")]);
        let d = decls(vec![rec]);
        let asyncs = analyze_async(&d);
        assert!(!asyncs.contains("loop"));
    }

    #[test]
    fn mutual_recursion_propagates_when_one_is_async() {
        // async func a() { b() }
        // func b()       { a() }
        // Both become async: a is declared async; b calls a -> async.
        let a = func("a", true, vec![ret_call("b")]);
        let b = func("b", false, vec![ret_call("a")]);
        let d = decls(vec![a, b]);
        let asyncs = analyze_async(&d);
        assert!(asyncs.contains("a"));
        assert!(asyncs.contains("b"));
    }

    #[test]
    fn mutual_recursion_stays_sync_if_neither_is_async() {
        // func a() { b() }
        // func b() { a() }
        // Neither declared async -> both stay sync.
        let a = func("a", false, vec![ret_call("b")]);
        let b = func("b", false, vec![ret_call("a")]);
        let d = decls(vec![a, b]);
        let asyncs = analyze_async(&d);
        assert!(!asyncs.contains("a"));
        assert!(!asyncs.contains("b"));
    }

    #[test]
    fn calls_to_undefined_names_are_ignored() {
        // func foo() { undefined_thing() }
        // undefined_thing isn't a declared fn -> no propagation; foo stays sync.
        let foo = func("foo", false, vec![ret_call("undefined_thing")]);
        let d = decls(vec![foo]);
        let asyncs = analyze_async(&d);
        assert!(!asyncs.contains("foo"));
    }

    #[test]
    fn call_graph_captures_callees_in_let_bindings() {
        // func f() { let x = g(); return x; }
        let body = vec![
            Stmt::LetDecl {
                name: Ident::new("x", dummy()),
                value: call_expr("g", Vec::new()),
                mutable: false,
                ty: None,
                span: dummy(),
            },
            Stmt::Return(Some(ident_expr("x")), dummy()),
        ];
        let f = func("f", false, body);
        let d = decls(vec![f]);
        let graph = build_call_graph(&d);
        assert_eq!(
            graph.callees_of("f"),
            // g is in callees but f itself is NOT (we don't record self-calls
            // when none exist).
            &{
                let mut s = BTreeSet::new();
                s.insert("g".to_string());
                s
            }
        );
    }

    #[test]
    fn method_calls_are_not_recorded_as_call_graph_edges() {
        // func f() { return recv.method(); }
        // Method-call target is dynamic -> not recorded.
        let method = Expr::MethodCall {
            receiver: Box::new(ident_expr("recv")),
            method: Ident::new("method", dummy()),
            args: Vec::new(),
            span: dummy(),
        };
        let f = func("f", false, vec![Stmt::Return(Some(method), dummy())]);
        let d = decls(vec![f]);
        let graph = build_call_graph(&d);
        // Method-call name "method" must NOT be in callees.
        assert!(
            !graph.callees_of("f").contains("method"),
            "method calls must not be call-graph edges"
        );
    }

    #[test]
    fn nested_calls_in_expressions_are_collected() {
        // func f() { return g() + h(); }
        let body = vec![Stmt::Return(
            Some(Expr::BinaryOp {
                op: buff_lang_ast::op::BinaryOp::Add,
                lhs: Box::new(call_expr("g", Vec::new())),
                rhs: Box::new(call_expr("h", Vec::new())),
                span: dummy(),
            }),
            dummy(),
        )];
        let f = func("f", false, body);
        let d = decls(vec![f]);
        let graph = build_call_graph(&d);
        let callees = graph.callees_of("f");
        assert!(callees.contains("g"));
        assert!(callees.contains("h"));
        assert_eq!(callees.len(), 2);
    }

    #[test]
    fn deterministic_order_for_same_input() {
        // The async set output is sorted; running the analysis twice on the
        // same input yields the same Vec.
        let io = func("io", true, vec![ret_int(0)]);
        let fetch = func("fetch", false, vec![ret_call("io")]);
        let d = decls(vec![io, fetch]);
        let set1 = analyze_async(&d);
        let set2 = analyze_async(&d);
        let a1: Vec<String> = set1.to_sorted_vec().iter().map(|s| s.to_string()).collect();
        let a2: Vec<String> = set2.to_sorted_vec().iter().map(|s| s.to_string()).collect();
        assert_eq!(a1, a2);
        // The order is sorted alphabetically.
        assert_eq!(
            a1.iter().map(|s| s.as_str()).collect::<Vec<_>>(),
            vec!["fetch", "io"]
        );
    }

    #[test]
    fn export_wrapped_funcs_contribute_to_graph_and_async_set() {
        // export async func io() { ... }
        // export func fetch()    { io() }
        let io = Decl::ExportDecl(buff_lang_ast::decl::ExportDecl {
            inner: Box::new(Decl::FuncDecl(func("io", true, vec![ret_int(0)]))),
            span: dummy(),
        });
        let fetch = Decl::ExportDecl(buff_lang_ast::decl::ExportDecl {
            inner: Box::new(Decl::FuncDecl(func("fetch", false, vec![ret_call("io")]))),
            span: dummy(),
        });
        let d = vec![io, fetch];
        let asyncs = analyze_async(&d);
        assert!(asyncs.contains("io"));
        assert!(asyncs.contains("fetch"));
    }

    #[test]
    fn is_async_after_propagation_helper() {
        let io = func("io", true, vec![ret_int(0)]);
        let fetch = func("fetch", false, vec![ret_call("io")]);
        let sync = func("sync", false, vec![ret_int(0)]);
        let d = decls(vec![io, fetch, sync]);
        assert!(is_async_after_propagation(&d, "io"));
        assert!(is_async_after_propagation(&d, "fetch"));
        assert!(!is_async_after_propagation(&d, "sync"));
        assert!(!is_async_after_propagation(&d, "nonexistent"));
    }

    #[test]
    fn three_branch_diamond_two_async_one_sync() {
        // async func io()
        // async func net()
        // func sync_only()    { return 1; }
        // func combined()     { io(); net(); sync_only(); }
        // func just_sync()    { sync_only(); }
        // Expected: io, net, combined are async; sync_only, just_sync are not.
        let io = func("io", true, vec![ret_int(0)]);
        let net = func("net", true, vec![ret_int(0)]);
        let sync_only = func("sync_only", false, vec![ret_int(1)]);
        let combined = func(
            "combined",
            false,
            vec![ret_call("io"), ret_call("net"), ret_call("sync_only")],
        );
        let just_sync = func("just_sync", false, vec![ret_call("sync_only")]);
        let d = decls(vec![io, net, sync_only, combined, just_sync]);
        let asyncs = analyze_async(&d);
        assert!(asyncs.contains("io"));
        assert!(asyncs.contains("net"));
        assert!(asyncs.contains("combined"));
        assert!(!asyncs.contains("sync_only"));
        assert!(!asyncs.contains("just_sync"));
    }

    #[test]
    fn cycle_in_call_graph_handled() {
        // Mutual recursion of 3 nodes, one of them declared async:
        // async func a() { c() }
        // func b()       { a() }
        // func c()       { b() }
        // All three should end up async.
        let a = func("a", true, vec![ret_call("c")]);
        let b = func("b", false, vec![ret_call("a")]);
        let c = func("c", false, vec![ret_call("b")]);
        let d = decls(vec![a, b, c]);
        let asyncs = analyze_async(&d);
        assert!(asyncs.contains("a"));
        assert!(asyncs.contains("b"));
        assert!(asyncs.contains("c"));
    }

    #[test]
    fn for_loop_body_calls_are_collected() {
        // func f() { for x in items { g(x) } }
        let body = vec![Stmt::ForIn {
            var: Ident::new("x", dummy()),
            iter: ident_expr("items"),
            body: Block {
                stmts: vec![Stmt::ExprStmt(
                    call_expr("g", vec![ident_expr("x")]),
                    dummy(),
                )],
                span: dummy(),
            },
            span: dummy(),
        }];
        let f = func("f", false, body);
        let d = decls(vec![f]);
        let graph = build_call_graph(&d);
        assert!(graph.callees_of("f").contains("g"));
    }

    #[test]
    fn if_expr_branches_are_collected() {
        // func f() { return if c { g() } else { h() }; }
        let if_e = Expr::IfExpr {
            cond: Box::new(ident_expr("c")),
            then_block: Block {
                stmts: vec![Stmt::Return(Some(call_expr("g", Vec::new())), dummy())],
                span: dummy(),
            },
            else_block: Some(Block {
                stmts: vec![Stmt::Return(Some(call_expr("h", Vec::new())), dummy())],
                span: dummy(),
            }),
            span: dummy(),
        };
        let f = func("f", false, vec![Stmt::ExprStmt(if_e, dummy())]);
        let d = decls(vec![f]);
        let graph = build_call_graph(&d);
        let callees = graph.callees_of("f");
        assert!(callees.contains("g"));
        assert!(callees.contains("h"));
    }

    #[test]
    fn match_arms_are_collected() {
        // func f() {
        //     return x match {
        //         Some(v) => g(v),
        //         None => h(),
        //     };
        // }
        let match_e = Expr::MatchExpr {
            scrutinee: Box::new(ident_expr("x")),
            arms: vec![
                buff_lang_ast::MatchArm {
                    pattern: buff_lang_ast::Pattern::Variant {
                        enum_name: Ident::new("Option", dummy()),
                        variant: Ident::new("Some", dummy()),
                        subpatterns: vec![buff_lang_ast::Pattern::Ident(
                            Ident::new("v", dummy()),
                            dummy(),
                        )],
                        span: dummy(),
                    },
                    body: Block {
                        stmts: vec![Stmt::Return(
                            Some(call_expr("g", vec![ident_expr("v")])),
                            dummy(),
                        )],
                        span: dummy(),
                    },
                    span: dummy(),
                },
                buff_lang_ast::MatchArm {
                    pattern: buff_lang_ast::Pattern::Ident(Ident::new("None", dummy()), dummy()),
                    body: Block {
                        stmts: vec![Stmt::Return(Some(call_expr("h", Vec::new())), dummy())],
                        span: dummy(),
                    },
                    span: dummy(),
                },
            ],
            span: dummy(),
        };
        let f = func("f", false, vec![Stmt::ExprStmt(match_e, dummy())]);
        let d = decls(vec![f]);
        let graph = build_call_graph(&d);
        let callees = graph.callees_of("f");
        assert!(callees.contains("g"));
        assert!(callees.contains("h"));
    }
}

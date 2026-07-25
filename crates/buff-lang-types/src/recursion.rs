//! Recursion detection (T48).
//!
//! WGSL — the GPU shader language Buff transpiles to — has **no recursion**.
//! Any Buff function that participates in a call-graph cycle (direct self-
//! recursion `f → f`, or mutual recursion `a → b → a`, or a longer cycle
//! `a → b → c → a`) is therefore fundamentally **not GPU-dispatchable**: it
//! must run on the CPU path (Rayon) even if the user hinted `@prefer(gpu)`.
//!
//! This module computes the set of functions that participate in any
//! call-graph cycle and exposes them as [`RecursionFacts::cpu_only`]. It is
//! the recursive-function counterpart of [`crate::ty::Type::must_run_on_cpu`]
//! (which classifies TYPES like `Decimal` as CPU-only because WGSL has no
//! 128-bit fixed-point). Both feed T40's dispatch-decision algorithm.
//!
//! # Algorithm
//!
//! 1. **Build the call graph** by reusing [`async_analysis::build_call_graph`]
//!    (T31). That function already walks every compound expression inside
//!    every function body and records bare-ident callee names. The result is
//!    a deterministic `BTreeMap<String, BTreeSet<String>>` mapping
//!    `caller_name → { callee_name, ... }`. Edges to names that aren't
//!    declared functions (prelude fns like `print`, free variables, etc.) are
//!    included in the graph but cannot close a cycle (they have no node).
//!
//! 2. **DFS cycle detection** via [`detect_cycles`]. We perform a deterministic
//!    DFS keeping an explicit `on_stack` set (the current path from the DFS
//!    root). When we encounter a callee that is already on the stack, every
//!    node from that callee's position upward to the current node inclusive
//!    is on a cycle. All such nodes are inserted into the `cpu_only`
//!    [`BTreeSet`]. Nodes are visited in sorted (BTreeMap) order, callees are
//!    iterated in sorted (BTreeSet) order — so the result is byte-identical
//!    across runs (the T29 flaky-test lesson).
//!
//! 3. **`@prefer(gpu)` validation** via [`check_prefer_gpu_on_recursive`].
//!    After computing `cpu_only`, we walk the decls once more for any
//!    function that BOTH is in `cpu_only` AND carries an `@prefer(gpu)`
//!    attribute. Such a combination is a hard ERROR (not a warning): the
//!    user explicitly asked for GPU dispatch on something that cannot run on
//!    a GPU. We return the lexicographically-first offender so the error is
//!    deterministic across runs.
//!
//! # What this module does NOT do
//!
//! - It does NOT mark transitive callers-of-recursive-functions as
//!   `cpu_only`. The classification is **on-cycle only**: a function `g`
//!   that calls a recursive `fib` is itself NOT `cpu_only` (it could still
//!   be GPU-dispatched if it happens to not recurse itself — though in
//!   practice it'd be reclassified once T49's full codegen decision runs).
//!   The spec defines "recursive = on a cycle" explicitly; transitive
//!   marking is left to T49's hint-driven codegen, which can layer a
//!   conservative "calls cpu_only" rule if needed.
//! - It does NOT decide dispatch — it only PUBLISHES facts. The actual
//!   "should this run on GPU?" decision lives in T40's `decide()` and will
//!   be refined by T49's hint-driven codegen.
//! - It does NOT handle trait-default-method recursion, `extend` blocks, or
//!   `extern` declarations. The call graph (T31) intentionally includes only
//!   top-level `func` declarations (and `export func` unwrappings); method
//!   calls are dynamic-dispatch and don't contribute edges.
//!
//! # Example
//!
//! ```text
//! func fib(n) { if n < 2 { 1 } else { fib(n-1) + fib(n-2) } }
//! func double(x) { x * 2 }
//! ```
//!
//! The call graph is `{ fib → {fib}, double → {} }`. The DFS finds a self-
//! loop on `fib`, so `cpu_only = {fib}`. `double` is not on any cycle.
//! `facts.is_cpu_only("fib") == true`; `facts.is_cpu_only("double") == false`.
//!
//! If `fib` had been declared `@prefer(gpu) func fib(n) { ... }`, the
//! analysis would return `Err(TypeError)` with the message
//! `"cannot @prefer(gpu) on recursive function `fib`: recursion is not
//! GPU-dispatchable"`.
//!
//! # `@prefer(gpu)` representation (CRITICAL for T49)
//!
//! The Buff AST models `@prefer(gpu)` as
//! [`Attribute { name: Ident("prefer"), args: vec!["gpu".to_string()] }`]
//! (see `crates/buff-lang-ast/src/decl.rs::Attribute` — T35 introduced this
//! shape precisely to carry the `@prefer(gpu)` form without a future AST
//! migration). The parser (`crates/buff-lang-parser/src/stmt.rs` line ~2484)
//! already accepts `@name(arg, arg, ...)` and stores the args as raw
//! strings. Detection of `@prefer(gpu)` is therefore a one-liner: scan
//! `func.attributes` for any entry where `name == "prefer"` and
//! `args == ["gpu"]` (see [`has_prefer_gpu_attr`]). T49's full hint-driven
//! codegen will extend this to `@prefer(cpu)` and possibly other targets
//! using the same `Attribute.args` shape.

use std::collections::{BTreeMap, BTreeSet};

use buff_lang_ast::{Decl, FuncDecl};
use buff_lang_error::{Diagnostic, ErrorCode, Span, TypeError};

use crate::async_analysis::{build_call_graph as build_async_call_graph, CallGraph};

/// A deterministic map from caller name → set of callee names directly
/// invoked from the caller's body.
///
/// Built by [`build_call_graph`]. Equivalent to the `edges` field of
/// [`CallGraph`] — exposed as a plain `BTreeMap` for callers that want
/// to inspect edges without depending on the [`CallGraph`] wrapper.
pub type CallEdges = BTreeMap<String, BTreeSet<String>>;

/// The result of recursion analysis: the set of functions that participate
/// in any call-graph cycle and are therefore restricted to CPU dispatch.
///
/// Built by [`analyze_recursion`]. Always deterministic (BTreeSet): the
/// same input program always yields byte-identical facts.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RecursionFacts {
    /// Names of functions on any call-graph cycle (self-recursion or mutual
    /// recursion). Sorted (BTreeSet) for deterministic iteration.
    pub cpu_only: BTreeSet<String>,
}

impl RecursionFacts {
    /// Construct an empty facts set (no recursion detected).
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns `true` if `name` is part of a call-graph cycle and therefore
    /// restricted to CPU-only dispatch (cannot be GPU-dispatched).
    pub fn is_cpu_only(&self, name: &str) -> bool {
        self.cpu_only.contains(name)
    }

    /// Number of cpu_only (recursive) functions.
    pub fn len(&self) -> usize {
        self.cpu_only.len()
    }

    /// `true` when no function in the program is recursive.
    pub fn is_empty(&self) -> bool {
        self.cpu_only.is_empty()
    }

    /// Sorted snapshot of the cpu_only set. Useful for assertions + debug
    /// rendering.
    pub fn to_sorted_vec(&self) -> Vec<&String> {
        self.cpu_only.iter().collect()
    }
}

/// Build a deterministic caller → callees map from a list of declarations.
///
/// This is a thin adapter over [`async_analysis::build_call_graph`] (T31),
/// which already implements the exact walking algorithm we need — recursing
/// into every compound expression that can contain an `Expr::FuncCall` site
/// (if/match/lambda/for/binop/string-interp/struct-init/...). We expose the
/// underlying `BTreeMap<String, BTreeSet<String>>` here for callers that
/// want to inspect edges directly without the [`CallGraph`] wrapper.
///
/// Edges to undeclared callees (prelude fns like `print`, free variables,
/// method calls) are recorded by the walker but do NOT have graph nodes —
/// they cannot close a cycle in [`detect_cycles`] because we only follow
/// edges to keys that exist in the map.
pub fn build_call_graph(decls: &[Decl]) -> CallEdges {
    build_async_call_graph(decls).edges
}

/// Detect every function that participates in ANY cycle (self-loop OR
/// mutual recursion) via deterministic DFS over the call graph.
///
/// # Algorithm
///
/// For each node (in sorted BTreeMap order) we launch a DFS if it hasn't
/// been visited yet. The DFS maintains an explicit `on_stack` set (the
/// path from the current DFS root to the active node). When we encounter a
/// callee already on the stack, every node from that callee's position in
/// the stack upward to (and including) the current node is on a cycle —
/// all such nodes join the `cpu_only` result.
///
/// # Determinism
///
/// - Nodes are visited in [`BTreeMap`] sorted-key order.
/// - Callees of each node are iterated in [`BTreeSet`] sorted order.
/// - The result [`BTreeSet`] iterates sorted.
///
/// Same call-graph input → byte-identical cycle set, always. This is the
/// T29 flaky-test lesson (HashMap iteration order is forbidden here).
///
/// # Edges to undeclared callees
///
/// The walker in [`crate::async_analysis`] records edges to every callee
/// name appearing in a body, including prelude fns and free variables. We
/// skip those during DFS: an edge is only followed when the callee has a
/// node in the graph (i.e. is itself a declared function).
///
/// # Termination
///
/// The DFS terminates because every node is visited at most once (the
/// `visited` set guards re-entry at the top-level loop, and the
/// `on_stack` check prevents infinite recursion along back-edges). Worst
/// case is `O(V + E)` — one DFS step per edge, one outer-loop iteration
/// per node.
pub fn detect_cycles(graph: &CallGraph) -> BTreeSet<String> {
    let mut on_cycle: BTreeSet<String> = BTreeSet::new();
    let mut visited: BTreeSet<String> = BTreeSet::new();
    // For each unvisited node (sorted by name), launch a DFS.
    for root in graph.edges.keys() {
        if visited.contains(root) {
            continue;
        }
        let mut stack: Vec<String> = Vec::new();
        let mut on_stack: BTreeSet<String> = BTreeSet::new();
        dfs_collect(
            root,
            graph,
            &mut visited,
            &mut on_stack,
            &mut stack,
            &mut on_cycle,
        );
    }
    on_cycle
}

/// Recursive DFS worker.
///
/// Visits children in sorted ([`BTreeSet`]) order so the traversal is
/// deterministic. When a back-edge is found (child already on the current
/// stack), every node from the child's prior position upward is marked
/// `on_cycle`.
///
/// Recursion depth is bounded by the call-graph depth — at most the number
/// of declared functions in the program. Realistic Buff programs have
/// hundreds at most, well within Rust's default 8 MB stack. Pathological
/// inputs (millions of nested decls) could overflow; that's an acceptable
/// limit (the parser would have OOM'd long before reaching the types
/// crate).
fn dfs_collect(
    node: &str,
    graph: &CallGraph,
    visited: &mut BTreeSet<String>,
    on_stack: &mut BTreeSet<String>,
    stack: &mut Vec<String>,
    on_cycle: &mut BTreeSet<String>,
) {
    // Mark visited + push onto the current DFS stack.
    visited.insert(node.to_string());
    stack.push(node.to_string());
    on_stack.insert(node.to_string());

    // Visit each callee in sorted order. Self-edges (node → node) close a
    // cycle trivially: `node` is on_stack, so the back-edge marks it.
    if let Some(callees) = graph.edges.get(node) {
        for child in callees {
            // Skip edges to names that aren't graph nodes (prelude fns,
            // free variables, etc.) — they can't close a cycle.
            if !graph.edges.contains_key(child) {
                continue;
            }
            if on_stack.contains(child) {
                // Back-edge → cycle. Mark every node from `child` upward
                // in the stack (inclusive) as on-cycle.
                let mut in_cycle = false;
                for n in stack.iter() {
                    if n.as_str() == child.as_str() {
                        in_cycle = true;
                    }
                    if in_cycle {
                        on_cycle.insert(n.clone());
                    }
                }
            } else if !visited.contains(child) {
                dfs_collect(child, graph, visited, on_stack, stack, on_cycle);
            }
        }
    }

    // Pop from the current DFS stack.
    stack.pop();
    on_stack.remove(node);
}

/// Walk `decls` for `@prefer(gpu)` attributes on functions that are part of
/// any call-graph cycle. Returns `Err(TypeError)` naming the
/// lexicographically-first offender; `Ok(())` if no conflict.
///
/// The error message is deterministic (offenders are collected into a
/// [`BTreeSet`] and the smallest name is reported first), so the same input
/// always yields the same diagnostic.
fn check_prefer_gpu_on_recursive(
    decls: &[Decl],
    cpu_only: &BTreeSet<String>,
) -> Result<(), TypeError> {
    // Collect all offenders in a BTreeSet so the reported name is the
    // lexicographically-smallest (deterministic across runs).
    let mut offenders: BTreeSet<String> = BTreeSet::new();
    for decl in decls {
        let f: Option<&FuncDecl> = match decl {
            Decl::FuncDecl(f) => Some(f),
            Decl::ExportDecl(e) => match e.inner.as_ref() {
                Decl::FuncDecl(f) => Some(f),
                _ => None,
            },
            _ => None,
        };
        let Some(f) = f else {
            continue;
        };
        if !cpu_only.contains(&f.name.name) {
            continue;
        }
        if has_prefer_gpu_attr(f) {
            offenders.insert(f.name.name.clone());
        }
    }
    if let Some(name) = offenders.iter().next() {
        return Err(TypeError::new(
            Diagnostic::error(
                format!(
                    "cannot @prefer(gpu) on recursive function `{name}`: \
                     recursion is not GPU-dispatchable"
                ),
                Span::dummy(),
            )
            .with_code(ErrorCode::PreferGpuOnRecursiveFunction),
        ));
    }
    Ok(())
}

/// Returns `true` iff `f` carries an attribute matching `@prefer(gpu)`.
///
/// # `@prefer(gpu)` representation (T49 — needed by hint-driven codegen)
///
/// `@prefer(gpu)` parses (since T35) to
/// `Attribute { name: Ident("prefer"), args: vec!["gpu".to_string()] }`.
/// The single argument is matched **case-sensitively** against the literal
/// `"gpu"`. Future `@prefer(cpu)` / `@prefer(adaptive)` will follow the
/// same shape (`name == "prefer"`, `args.len() == 1`, `args[0] == "<target>"`)
/// and can be detected by extending this helper or adding a sibling.
///
/// Multiple attributes are independent — a function carrying both
/// `@prefer(gpu)` and `@inline` still matches here (we scan for ANY
/// attribute satisfying the `prefer(gpu)` predicate).
pub fn has_prefer_gpu_attr(f: &FuncDecl) -> bool {
    f.attributes
        .iter()
        .any(|a| a.name.name == "prefer" && a.args.len() == 1 && a.args[0] == "gpu")
}

/// Analyze a program's declarations for recursion, returning the set of
/// functions that must run on CPU only because they participate in a
/// call-graph cycle (direct self-recursion or mutual recursion).
///
/// # Error case
///
/// If any function carrying `@prefer(gpu)` is part of a cycle, returns
/// `Err(TypeError)` naming the lexicographically-first offender with a
/// message like:
///
/// ```text
/// cannot @prefer(gpu) on recursive function `fib`: recursion is not GPU-dispatchable
/// ```
///
/// This is a hard error (not a warning) because the user's explicit hint
/// is fundamentally incompatible with the function's shape — silent
/// fallback to CPU would hide a real bug.
///
/// # Determinism
///
/// All intermediate structures ([`BTreeMap`]/[`BTreeSet`]) iterate in
/// sorted order; the same input program always yields byte-identical
/// facts (or byte-identical error message).
///
/// # Example
///
/// ```text
/// func fib(n) { fib(n-1) + fib(n-2) }
/// func main() { print(fib(10)) }
/// ```
///
/// Returns `Ok(RecursionFacts { cpu_only: {"fib"} })`. `main` is NOT
/// `cpu_only` — it isn't itself on a cycle (it merely CALLS a recursive
/// fn, which doesn't disqualify it from GPU dispatch per the spec).
pub fn analyze_recursion(decls: &[Decl]) -> Result<RecursionFacts, TypeError> {
    let graph = build_async_call_graph(decls);
    let cpu_only = detect_cycles(&graph);
    check_prefer_gpu_on_recursive(decls, &cpu_only)?;
    Ok(RecursionFacts { cpu_only })
}

// ---------------------------------------------------------------------------
// Unit tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use buff_lang_ast::common::{Block, Ident};
    use buff_lang_ast::decl::{Attribute, FuncDecl};
    use buff_lang_ast::{Expr, Stmt};
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

    fn prefer_gpu_attr() -> Attribute {
        Attribute {
            name: Ident::new("prefer", dummy()),
            args: vec!["gpu".to_string()],
            named_args: std::collections::BTreeMap::new(),
            span: dummy(),
        }
    }

    fn prefer_cpu_attr() -> Attribute {
        Attribute {
            name: Ident::new("prefer", dummy()),
            args: vec!["cpu".to_string()],
            named_args: std::collections::BTreeMap::new(),
            span: dummy(),
        }
    }

    fn func(name: &str, body_stmts: Vec<Stmt>) -> FuncDecl {
        FuncDecl {
            name: Ident::new(name, dummy()),
            params: Vec::new(),
            return_type: None,
            body: Block {
                stmts: body_stmts,
                span: dummy(),
            },
            is_async: false,
            is_unsafe: false,
            is_extern: false,
            attributes: Vec::new(),
            type_params: Vec::new(),
            span: dummy(),
        }
    }

    fn func_with_attrs(name: &str, attrs: Vec<Attribute>, body_stmts: Vec<Stmt>) -> FuncDecl {
        let mut f = func(name, body_stmts);
        f.attributes = attrs;
        f
    }

    fn ret_call(name: &str) -> Stmt {
        Stmt::Return(Some(call_expr(name, Vec::new())), dummy())
    }

    fn ret_int(n: i64) -> Stmt {
        Stmt::Return(Some(int_expr(n)), dummy())
    }

    fn ret_binop_call(left: &str, right: &str) -> Stmt {
        Stmt::Return(
            Some(Expr::BinaryOp {
                op: buff_lang_ast::op::BinaryOp::Add,
                lhs: Box::new(call_expr(left, Vec::new())),
                rhs: Box::new(call_expr(right, Vec::new())),
                span: dummy(),
            }),
            dummy(),
        )
    }

    fn decls(fs: Vec<FuncDecl>) -> Vec<Decl> {
        fs.into_iter().map(Decl::FuncDecl).collect()
    }

    // -- build_call_graph ---------------------------------------------------

    #[test]
    fn build_call_graph_returns_sorted_deterministic_edges() {
        // func a() { b(); c() }
        // func b() { c() }
        // func c() { return 0 }
        let a = func("a", vec![ret_call("b"), ret_call("c")]);
        let b = func("b", vec![ret_call("c")]);
        let c = func("c", vec![ret_int(0)]);
        let d = decls(vec![a, b, c]);
        let edges = build_call_graph(&d);
        let keys: Vec<&String> = edges.keys().collect();
        assert_eq!(keys, vec!["a", "b", "c"]);
        assert_eq!(edges.get("a").map(|s| s.len()), Some(2));
        assert!(edges["a"].contains("b"));
        assert!(edges["a"].contains("c"));
    }

    #[test]
    fn build_call_graph_ignores_calls_to_undefined_when_following() {
        // func foo() { undefined_thing() }
        // The graph records the edge, but `undefined_thing` has no node,
        // so detect_cycles skips it.
        let foo = func("foo", vec![ret_call("undefined_thing")]);
        let d = decls(vec![foo]);
        let edges = build_call_graph(&d);
        assert!(edges["foo"].contains("undefined_thing"));
        // detect_cycles should not classify anything.
        let facts = analyze_recursion(&d).expect("no recursion");
        assert!(facts.is_empty());
    }

    // -- detect_cycles ------------------------------------------------------

    #[test]
    fn detect_cycles_no_cycles_returns_empty() {
        // func a() { b() }
        // func b() { c() }
        // func c() { return 0 }
        let a = func("a", vec![ret_call("b")]);
        let b = func("b", vec![ret_call("c")]);
        let c = func("c", vec![ret_int(0)]);
        let d = decls(vec![a, b, c]);
        let graph = build_async_call_graph(&d);
        let cycles = detect_cycles(&graph);
        assert!(cycles.is_empty());
    }

    #[test]
    fn detect_cycles_self_loop_marked() {
        // func loop_(n) { loop_(n - 1) }
        let l = func("loop_", vec![ret_call("loop_")]);
        let d = decls(vec![l]);
        let graph = build_async_call_graph(&d);
        let cycles = detect_cycles(&graph);
        assert!(cycles.contains("loop_"));
        assert_eq!(cycles.len(), 1);
    }

    #[test]
    fn detect_cycles_mutual_two_marked() {
        // func a() { b() }
        // func b() { a() }
        let a = func("a", vec![ret_call("b")]);
        let b = func("b", vec![ret_call("a")]);
        let d = decls(vec![a, b]);
        let graph = build_async_call_graph(&d);
        let cycles = detect_cycles(&graph);
        assert!(cycles.contains("a"));
        assert!(cycles.contains("b"));
    }

    #[test]
    fn detect_cycles_three_node_chain_cycle_marked() {
        // a→b→c→a
        let a = func("a", vec![ret_call("b")]);
        let b = func("b", vec![ret_call("c")]);
        let c = func("c", vec![ret_call("a")]);
        let d = decls(vec![a, b, c]);
        let graph = build_async_call_graph(&d);
        let cycles = detect_cycles(&graph);
        for n in &["a", "b", "c"] {
            assert!(cycles.contains(*n), "expected {n} to be on cycle");
        }
    }

    #[test]
    fn detect_cycles_caller_of_cycle_not_marked() {
        // entry → fib → fib (self-loop)
        // entry is NOT on cycle.
        let entry = func("entry", vec![ret_call("fib")]);
        let fib = func("fib", vec![ret_call("fib")]);
        let d = decls(vec![entry, fib]);
        let graph = build_async_call_graph(&d);
        let cycles = detect_cycles(&graph);
        assert!(cycles.contains("fib"));
        assert!(!cycles.contains("entry"), "entry is not on cycle");
    }

    // -- analyze_recursion (high-level) -------------------------------------

    #[test]
    fn analyze_recursion_empty_program_returns_empty_facts() {
        let facts = analyze_recursion(&[]).expect("empty program ok");
        assert!(facts.is_empty());
        assert_eq!(facts.len(), 0);
        assert!(!facts.is_cpu_only("anything"));
    }

    #[test]
    fn analyze_recursion_fib_self_recursion_marked_cpu_only() {
        // func fib(n) { fib(n-1) + fib(n-2) }
        let fib_body = vec![ret_binop_call("fib", "fib")];
        let fib = func("fib", fib_body);
        let d = decls(vec![fib]);
        let facts = analyze_recursion(&d).expect("no prefer-gpu conflict");
        assert!(facts.is_cpu_only("fib"));
        assert_eq!(facts.to_sorted_vec().len(), 1);
    }

    #[test]
    fn analyze_recursion_non_recursive_func_not_cpu_only() {
        // func double(x) { return x * 2 }
        let body = vec![Stmt::Return(
            Some(Expr::BinaryOp {
                op: buff_lang_ast::op::BinaryOp::Mul,
                lhs: Box::new(ident_expr("x")),
                rhs: Box::new(int_expr(2)),
                span: dummy(),
            }),
            dummy(),
        )];
        let double = func("double", body);
        let d = decls(vec![double]);
        let facts = analyze_recursion(&d).expect("ok");
        assert!(!facts.is_cpu_only("double"));
        assert!(facts.is_empty());
    }

    #[test]
    fn analyze_recursion_mutual_recursion_both_marked() {
        // a ↔ b
        let a = func("a", vec![ret_call("b")]);
        let b = func("b", vec![ret_call("a")]);
        let d = decls(vec![a, b]);
        let facts = analyze_recursion(&d).expect("ok");
        assert!(facts.is_cpu_only("a"));
        assert!(facts.is_cpu_only("b"));
        assert_eq!(facts.len(), 2);
    }

    #[test]
    fn analyze_recursion_three_cycle_all_marked() {
        let a = func("a", vec![ret_call("b")]);
        let b = func("b", vec![ret_call("c")]);
        let c = func("c", vec![ret_call("a")]);
        let d = decls(vec![a, b, c]);
        let facts = analyze_recursion(&d).expect("ok");
        for n in &["a", "b", "c"] {
            assert!(facts.is_cpu_only(n), "{n} should be cpu_only");
        }
    }

    #[test]
    fn analyze_recursion_deep_chain_no_cycle_none_marked() {
        // a → b → c → d → (terminal)
        let a = func("a", vec![ret_call("b")]);
        let b = func("b", vec![ret_call("c")]);
        let c = func("c", vec![ret_call("d")]);
        let d = func("d", vec![ret_int(0)]);
        let dd = decls(vec![a, b, c, d]);
        let facts = analyze_recursion(&dd).expect("ok");
        for n in &["a", "b", "c", "d"] {
            assert!(!facts.is_cpu_only(n), "{n} should NOT be cpu_only");
        }
        assert!(facts.is_empty());
    }

    #[test]
    fn analyze_recursion_caller_of_recursive_fn_not_marked() {
        // entry → fib → fib
        // entry is NOT on cycle; only `fib` is.
        let entry = func("entry", vec![ret_call("fib")]);
        let fib = func("fib", vec![ret_call("fib")]);
        let d = decls(vec![entry, fib]);
        let facts = analyze_recursion(&d).expect("ok");
        assert!(facts.is_cpu_only("fib"));
        assert!(!facts.is_cpu_only("entry"), "entry is not on cycle");
    }

    #[test]
    fn analyze_recursion_disconnected_components_only_one_marked() {
        // a → a (self-loop)
        // b → c → d (chain, no cycle)
        // e (isolated, no calls)
        let a = func("a", vec![ret_call("a")]);
        let b = func("b", vec![ret_call("c")]);
        let c = func("c", vec![ret_call("d")]);
        let d = func("d", vec![ret_int(0)]);
        let e = func("e", vec![ret_int(0)]);
        let dd = decls(vec![a, b, c, d, e]);
        let facts = analyze_recursion(&dd).expect("ok");
        assert!(facts.is_cpu_only("a"));
        assert!(!facts.is_cpu_only("b"));
        assert!(!facts.is_cpu_only("c"));
        assert!(!facts.is_cpu_only("d"));
        assert!(!facts.is_cpu_only("e"));
        assert_eq!(facts.len(), 1);
    }

    // -- @prefer(gpu) conflict ---------------------------------------------

    #[test]
    fn analyze_recursion_prefer_gpu_on_recursive_returns_err() {
        // @prefer(gpu) func fib(n) { fib(n-1) }
        let fib = func_with_attrs("fib", vec![prefer_gpu_attr()], vec![ret_call("fib")]);
        let d = decls(vec![fib]);
        let err = analyze_recursion(&d).expect_err("recursive + @prefer(gpu) must error");
        let msg = err.diagnostic.message.clone();
        assert!(
            msg.contains("fib") && msg.contains("@prefer(gpu)") && msg.contains("recursive"),
            "expected message naming fib and @prefer(gpu) on recursive, got: {msg}"
        );
    }

    #[test]
    fn analyze_recursion_prefer_gpu_on_non_recursive_returns_ok() {
        // @prefer(gpu) func double(x) { x * 2 }
        let body = vec![Stmt::Return(
            Some(Expr::BinaryOp {
                op: buff_lang_ast::op::BinaryOp::Mul,
                lhs: Box::new(ident_expr("x")),
                rhs: Box::new(int_expr(2)),
                span: dummy(),
            }),
            dummy(),
        )];
        let double = func_with_attrs("double", vec![prefer_gpu_attr()], body);
        let d = decls(vec![double]);
        let facts = analyze_recursion(&d).expect("non-recursive + @prefer(gpu) is fine");
        assert!(!facts.is_cpu_only("double"));
    }

    #[test]
    fn analyze_recursion_prefer_cpu_on_recursive_returns_ok() {
        // @prefer(cpu) is NOT @prefer(gpu) — recursion is fine (cpu_only still
        // recorded, but no error).
        let fib = func_with_attrs("fib", vec![prefer_cpu_attr()], vec![ret_call("fib")]);
        let d = decls(vec![fib]);
        let facts = analyze_recursion(&d).expect("prefer(cpu) does not conflict");
        assert!(facts.is_cpu_only("fib"));
    }

    #[test]
    fn analyze_recursion_prefer_gpu_offender_is_lexicographically_first() {
        // Two recursive fns with @prefer(gpu): `aaa` and `zzz`. The error
        // must deterministically name `aaa` (smallest name).
        let aaa = func_with_attrs("aaa", vec![prefer_gpu_attr()], vec![ret_call("aaa")]);
        let zzz = func_with_attrs("zzz", vec![prefer_gpu_attr()], vec![ret_call("zzz")]);
        let d = decls(vec![zzz, aaa]); // input order shouldn't matter
        let err = analyze_recursion(&d).expect_err("both offend");
        assert!(
            err.diagnostic.message.contains("`aaa`"),
            "expected offender `aaa`, got: {}",
            err.diagnostic.message
        );
        assert!(!err.diagnostic.message.contains("`zzz`"));
    }

    #[test]
    fn has_prefer_gpu_attr_detects_exact_match() {
        let mut f = func("f", vec![ret_int(0)]);
        assert!(!has_prefer_gpu_attr(&f));
        f.attributes = vec![prefer_gpu_attr()];
        assert!(has_prefer_gpu_attr(&f));
        // Wrong arg (cpu instead of gpu) must NOT match.
        f.attributes = vec![prefer_cpu_attr()];
        assert!(!has_prefer_gpu_attr(&f));
    }

    #[test]
    fn has_prefer_gpu_attr_ignores_multi_arg_prefer() {
        // @prefer(gpu, force) — multi-arg form. We require exactly args=["gpu"].
        let mut f = func("f", vec![ret_int(0)]);
        f.attributes = vec![Attribute {
            name: Ident::new("prefer", dummy()),
            args: vec!["gpu".to_string(), "force".to_string()],
            named_args: std::collections::BTreeMap::new(),
            span: dummy(),
        }];
        assert!(!has_prefer_gpu_attr(&f));
    }

    // -- determinism --------------------------------------------------------

    #[test]
    fn analyze_recursion_deterministic_for_same_input() {
        // Mixed: a → a (self-loop), b → c → b (mutual), d (terminal).
        let mk = || {
            decls(vec![
                func("a", vec![ret_call("a")]),
                func("b", vec![ret_call("c")]),
                func("c", vec![ret_call("b")]),
                func("d", vec![ret_int(0)]),
            ])
        };
        let f1 = analyze_recursion(&mk()).expect("ok");
        let f2 = analyze_recursion(&mk()).expect("ok");
        let v1: Vec<String> = f1.to_sorted_vec().iter().map(|s| s.to_string()).collect();
        let v2: Vec<String> = f2.to_sorted_vec().iter().map(|s| s.to_string()).collect();
        assert_eq!(v1, v2);
        // Spot-check the contents.
        assert!(f1.is_cpu_only("a"));
        assert!(f1.is_cpu_only("b"));
        assert!(f1.is_cpu_only("c"));
        assert!(!f1.is_cpu_only("d"));
    }

    #[test]
    fn analyze_recursion_export_wrapped_recursive_marked() {
        // export func fib(n) { fib(n-1) }
        let inner = Decl::FuncDecl(func("fib", vec![ret_call("fib")]));
        let d = vec![Decl::ExportDecl(buff_lang_ast::decl::ExportDecl {
            inner: Box::new(inner),
            span: dummy(),
        })];
        let facts = analyze_recursion(&d).expect("ok");
        assert!(facts.is_cpu_only("fib"));
    }

    #[test]
    fn analyze_recursion_recursion_facts_default_empty() {
        let f = RecursionFacts::new();
        assert!(f.is_empty());
        assert_eq!(f.len(), 0);
        assert!(!f.is_cpu_only("anything"));
        assert!(f.to_sorted_vec().is_empty());
    }
}

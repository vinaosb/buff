//! Buff Intermediate Representation (IR) — a dataflow graph.
//!
//! The IR separates **what to compute** from **how to schedule it** (the
//! Halide algorithm/schedule split). Each node records its data dependencies;
//! the scheduler (v1.0 T40) later decides CPU vs GPU, serial vs parallel.
//!
//! # Module layout
//!
//! - [`NodeId`]: opaque identifier for a graph node.
//! - [`IrNode`] / [`ComputeNode`] / [`IoNode`] / [`TransferNode`] / [`ScheduleNode`]:
//!   the node types.
//! - [`IrGraph`]: the dependency DAG with forward/reverse edges and topo sort.
//! - [`AstLowerer`]: converts typed AST ([`Decl`]/[`Block`]) into an [`IrGraph`].
//!
//! # Design notes
//!
//! - One IR node per AST statement (statement-level granularity, not full SSA).
//! - Dependency edges: if node N *uses* a variable that node M *defines*, then
//!   `N depends on M` (edge `M -> N` in the forward direction).
//! - I/O boundary detection is driven by the `async_functions` set on the
//!   lowerer; a call to a registered async function becomes an [`IrNode::IONode`]
//!   (a suspension point).

use std::collections::{BTreeSet, HashMap, HashSet};
use std::fmt;

use crate::{Block, Decl, Expr, Ident, Span, Stmt};

// ---------------------------------------------------------------------------
// NodeId
// ---------------------------------------------------------------------------

/// Unique identifier for an IR node. Assigned monotonically by [`IrGraph`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct NodeId(pub u32);

impl fmt::Display for NodeId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "%{}", self.0)
    }
}

// ---------------------------------------------------------------------------
// Memory spaces & dispatch decisions (v1.0 placeholders)
// ---------------------------------------------------------------------------

/// Memory space for data transfers (populated by the v1.0 scheduler).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MemorySpace {
    /// Host RAM.
    Cpu,
    /// Per-thread GPU local memory.
    GpuLocal,
    /// GPU shared/block memory.
    GpuShared,
}

impl fmt::Display for MemorySpace {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            MemorySpace::Cpu => "Cpu",
            MemorySpace::GpuLocal => "GpuLocal",
            MemorySpace::GpuShared => "GpuShared",
        })
    }
}

/// Dispatch decision filled in by the scheduler (v1.0 T40).
/// In v0.1 every node is effectively [`DispatchDecision::SequentialCpu`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DispatchDecision {
    /// Sequential CPU execution (v0.1 default).
    SequentialCpu,
    /// Parallel CPU via Rayon (v1.0).
    ParallelCpu,
    /// GPU compute dispatch (v1.0).
    GpuCompute,
    /// Auto-decide based on heuristics (v1.0).
    Auto,
}

impl fmt::Display for DispatchDecision {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            DispatchDecision::SequentialCpu => "SequentialCpu",
            DispatchDecision::ParallelCpu => "ParallelCpu",
            DispatchDecision::GpuCompute => "GpuCompute",
            DispatchDecision::Auto => "Auto",
        })
    }
}

// ---------------------------------------------------------------------------
// Node payload structs
// ---------------------------------------------------------------------------

/// Pure computation — no side effects, deterministic.
#[derive(Debug, Clone, PartialEq)]
pub struct ComputeNode {
    pub id: NodeId,
    /// The AST expression this node computes (kept for traceability).
    pub source_expr: Option<Expr>,
    /// The AST statement (for let bindings, assignments, control flow).
    pub source_stmt: Option<Stmt>,
    /// Variables this node produces (defined bindings).
    pub defs: Vec<Ident>,
    /// Variables this node reads (used bindings).
    pub uses: Vec<Ident>,
    pub span: Span,
    /// Human-readable description for debugging/snapshots.
    pub description: String,
}

/// I/O boundary — async function call, network, file system.
/// These are suspension points in the async runtime.
#[derive(Debug, Clone, PartialEq)]
pub struct IoNode {
    pub id: NodeId,
    /// The async function being called.
    pub callee: Ident,
    /// Argument expressions.
    pub args: Vec<Expr>,
    /// Result variable(s) defined by this I/O operation.
    pub defs: Vec<Ident>,
    /// Variables read as arguments.
    pub uses: Vec<Ident>,
    pub span: Span,
    /// This node is a suspension point in the async runtime (always `true`
    /// for [`IrNode::IONode`]).
    pub is_suspension_point: bool,
}

/// Data movement between CPU and GPU memory (v1.0 GPU dispatch planning).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TransferNode {
    pub id: NodeId,
    /// Variable being transferred.
    pub var: Ident,
    /// Source memory space.
    pub from: MemorySpace,
    /// Destination memory space.
    pub to: MemorySpace,
    pub span: Span,
}

/// Dispatch decision placeholder governing a group of nodes.
/// The scheduler (v1.0 T40) fills in the real decision.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScheduleNode {
    pub id: NodeId,
    /// The nodes this schedule governs.
    pub governed: Vec<NodeId>,
    /// Current dispatch decision (placeholder in v0.1).
    pub decision: DispatchDecision,
    pub span: Span,
}

/// A node in the IR dataflow graph.
///
/// The `Compute` variant is boxed because it dominates the enum size
/// (it carries the source `Expr`/`Stmt`). Use [`IrNode::compute`] to
/// construct one without spelling out `Box::new`.
#[derive(Debug, Clone, PartialEq)]
pub enum IrNode {
    /// Pure computation — no side effects, deterministic.
    /// Examples: literals, arithmetic, pure function calls.
    Compute(Box<ComputeNode>),

    /// I/O boundary — async function call, network, file system.
    /// These are suspension points in the async runtime.
    IONode(IoNode),

    /// Data movement between CPU and GPU memory.
    /// Used in v1.0 GPU dispatch planning.
    Transfer(TransferNode),

    /// Dispatch decision placeholder.
    /// The scheduler (v1.0 T40) fills in CPU/GPU/parallel decision.
    /// In v0.1 IR, all ScheduleNodes are placeholders marked "Sequential CPU".
    Schedule(ScheduleNode),
}

impl IrNode {
    /// Convenience constructor that boxes a [`ComputeNode`] into an
    /// [`IrNode::Compute`] (keeps the enum compact — see
    /// `clippy::large_enum_variant`).
    pub fn compute(node: ComputeNode) -> Self {
        IrNode::Compute(Box::new(node))
    }

    /// Returns the [`NodeId`] stored on this node.
    pub fn id(&self) -> NodeId {
        match self {
            IrNode::Compute(n) => n.id,
            IrNode::IONode(n) => n.id,
            IrNode::Transfer(n) => n.id,
            IrNode::Schedule(n) => n.id,
        }
    }

    /// Overwrite the stored [`NodeId`] (used by [`IrGraph::add_node`] when it
    /// assigns the canonical id).
    pub fn set_id(&mut self, id: NodeId) {
        match self {
            IrNode::Compute(n) => n.id = id,
            IrNode::IONode(n) => n.id = id,
            IrNode::Transfer(n) => n.id = id,
            IrNode::Schedule(n) => n.id = id,
        }
    }

    /// Returns `true` if this node is a suspension point (always an [`IoNode`]).
    pub fn is_suspension_point(&self) -> bool {
        matches!(self, IrNode::IONode(io) if io.is_suspension_point)
    }
}

// ---------------------------------------------------------------------------
// Cycle error
// ---------------------------------------------------------------------------

/// Returned by [`IrGraph::topological_order`] when the graph contains a cycle.
///
/// A valid AST lowering never produces a cycle; this error indicates either a
/// manually-constructed test graph or a lowering bug.
#[derive(Debug, thiserror::Error)]
#[error("IR graph contains a cycle (should not happen after valid AST lowering)")]
pub struct IrCycleError;

// ---------------------------------------------------------------------------
// IrGraph
// ---------------------------------------------------------------------------

/// A dataflow graph of IR nodes with dependency edges.
///
/// # Edge semantics
///
/// - `edges[B]` = the set of nodes that **depend on B** (B's *dependents*).
/// - `reverse_edges[A]` = the set of nodes that **A depends on** (A's
///   *dependencies*).
///
/// Use [`IrGraph::dependencies`] / [`IrGraph::dependents`] for ergonomic access.
#[derive(Debug, Clone, PartialEq)]
pub struct IrGraph {
    /// All nodes indexed by [`NodeId`].
    pub nodes: HashMap<NodeId, IrNode>,
    /// Forward edges: `edges[B]` contains `A` when `A` depends on `B`.
    pub edges: HashMap<NodeId, HashSet<NodeId>>,
    /// Reverse edges: `reverse_edges[A]` contains `B` when `A` depends on `B`.
    pub reverse_edges: HashMap<NodeId, HashSet<NodeId>>,
    /// Entry nodes (no dependencies). Populated by [`IrGraph::finalize`].
    pub entry_nodes: Vec<NodeId>,
    /// Exit nodes (no dependents). Populated by [`IrGraph::finalize`].
    pub exit_nodes: Vec<NodeId>,
    /// Next available [`NodeId`].
    next_id: u32,
}

impl Default for IrGraph {
    fn default() -> Self {
        Self::new()
    }
}

impl IrGraph {
    /// Create an empty graph.
    pub fn new() -> Self {
        Self {
            nodes: HashMap::new(),
            edges: HashMap::new(),
            reverse_edges: HashMap::new(),
            entry_nodes: Vec::new(),
            exit_nodes: Vec::new(),
            next_id: 0,
        }
    }

    /// Returns the number of nodes in the graph.
    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    /// Returns `true` if the graph has no nodes.
    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }

    /// Insert a node, assigning it the next canonical [`NodeId`].
    /// Returns the assigned id. The node's stored `id` field is overwritten.
    pub fn add_node(&mut self, mut node: IrNode) -> NodeId {
        let id = NodeId(self.next_id);
        self.next_id += 1;
        node.set_id(id);
        self.nodes.insert(id, node);
        self.edges.entry(id).or_default();
        self.reverse_edges.entry(id).or_default();
        id
    }

    /// Add a dependency edge: `dependent` uses an output of `dependency`.
    ///
    /// After this call, `dependent` is in `edges[dependency]` and `dependency`
    /// is in `reverse_edges[dependent]`. Duplicate insertions are idempotent.
    pub fn add_dependency(&mut self, dependent: NodeId, dependency: NodeId) {
        self.edges.entry(dependency).or_default().insert(dependent);
        self.reverse_edges
            .entry(dependent)
            .or_default()
            .insert(dependency);
    }

    /// All nodes that this node **directly depends on** (its inputs), sorted
    /// by [`NodeId`] for deterministic output.
    pub fn dependencies(&self, node: NodeId) -> Vec<NodeId> {
        let mut v: Vec<NodeId> = self
            .reverse_edges
            .get(&node)
            .map_or(Vec::new(), |s| s.iter().copied().collect());
        v.sort();
        v
    }

    /// All nodes that **directly depend on** this node (its consumers), sorted
    /// by [`NodeId`] for deterministic output.
    pub fn dependents(&self, node: NodeId) -> Vec<NodeId> {
        let mut v: Vec<NodeId> = self
            .edges
            .get(&node)
            .map_or(Vec::new(), |s| s.iter().copied().collect());
        v.sort();
        v
    }

    /// Compute [`entry_nodes`] and [`exit_nodes`] from the current edge sets.
    ///
    /// Call this once after all nodes and edges have been added.
    ///
    /// [`entry_nodes`]: IrGraph::entry_nodes
    /// [`exit_nodes`]: IrGraph::exit_nodes
    pub fn finalize(&mut self) {
        self.entry_nodes.clear();
        self.exit_nodes.clear();
        for &id in self.nodes.keys() {
            let has_deps = self.reverse_edges.get(&id).is_some_and(|s| !s.is_empty());
            let has_dependents = self.edges.get(&id).is_some_and(|s| !s.is_empty());
            if !has_deps {
                self.entry_nodes.push(id);
            }
            if !has_dependents {
                self.exit_nodes.push(id);
            }
        }
        self.entry_nodes.sort();
        self.exit_nodes.sort();
    }

    /// Topological sort via Kahn's algorithm. Returns nodes in an order where
    /// every dependency appears before its dependent. Ties are broken by
    /// ascending [`NodeId`] for deterministic output.
    ///
    /// Returns [`Err(IrCycleError)`](IrCycleError) if the graph contains a
    /// cycle.
    pub fn topological_order(&self) -> Result<Vec<NodeId>, IrCycleError> {
        // in_degree[N] = number of dependencies of N (size of reverse_edges[N]).
        let mut in_degree: HashMap<NodeId, usize> = HashMap::with_capacity(self.nodes.len());
        for &id in self.nodes.keys() {
            let deg = self.reverse_edges.get(&id).map_or(0, |s| s.len());
            in_degree.insert(id, deg);
        }

        // BTreeSet gives us deterministic ascending-NodeId ordering.
        let mut ready: BTreeSet<NodeId> = in_degree
            .iter()
            .filter(|(_, &d)| d == 0)
            .map(|(&id, _)| id)
            .collect();

        let mut order = Vec::with_capacity(self.nodes.len());
        while let Some(&id) = ready.iter().next() {
            ready.remove(&id);
            order.push(id);
            if let Some(dependents) = self.edges.get(&id) {
                for &dep in dependents {
                    if let Some(d) = in_degree.get_mut(&dep) {
                        *d -= 1;
                        if *d == 0 {
                            ready.insert(dep);
                        }
                    }
                }
            }
        }

        if order.len() != self.nodes.len() {
            return Err(IrCycleError);
        }
        Ok(order)
    }
}

// ---------------------------------------------------------------------------
// AstLowerer
// ---------------------------------------------------------------------------

/// Lowers typed AST (decls/statements) into an [`IrGraph`] dataflow graph.
///
/// The lowerer maintains a `bindings` map (variable name -> defining
/// [`NodeId`]) in single-static-assignment style: a later `let` with the same
/// name shadows the earlier definition for downstream uses.
pub struct AstLowerer {
    graph: IrGraph,
    /// Current variable -> defining NodeId mapping.
    bindings: HashMap<String, NodeId>,
    /// Known async function names (I/O boundary markers).
    async_functions: HashSet<String>,
}

impl Default for AstLowerer {
    fn default() -> Self {
        Self::new()
    }
}

impl AstLowerer {
    /// Create a new lowerer with an empty graph and no registered async fns.
    pub fn new() -> Self {
        Self {
            graph: IrGraph::new(),
            bindings: HashMap::new(),
            async_functions: HashSet::new(),
        }
    }

    /// Register a function name as async (I/O boundary). Calls to it will be
    /// lowered to [`IrNode::IONode`] suspension points.
    pub fn mark_async(&mut self, name: &str) {
        self.async_functions.insert(name.to_string());
    }

    /// Lower a list of declarations (typically a module/function body).
    ///
    /// Resets the lowerer's graph and bindings but preserves the registered
    /// async-function set. The returned graph has been [`finalize`](IrGraph::finalize)d.
    pub fn lower(&mut self, decls: &[Decl]) -> IrGraph {
        self.graph = IrGraph::new();
        self.bindings.clear();
        for decl in decls {
            self.lower_decl(decl);
        }
        self.graph.finalize();
        std::mem::take(&mut self.graph)
    }

    /// Lower a block of statements into `self.graph` (does not finalize).
    /// Public so callers can lower a standalone [`Block`] without wrapping it
    /// in a [`Decl`].
    pub fn lower_block(&mut self, block: &Block) {
        for stmt in &block.stmts {
            self.lower_stmt(stmt);
        }
    }

    /// Returns `true` if `name` is registered as an async function.
    fn is_async_call(&self, name: &str) -> bool {
        self.async_functions.contains(name)
    }

    /// If `expr` is a direct call to a registered async function, return the
    /// callee [`Ident`] and cloned args.
    fn detect_async_call(&self, expr: &Expr) -> Option<(Ident, Vec<Expr>)> {
        if let Expr::FuncCall { callee, args, .. } = expr {
            if let Expr::Ident(name, _) = callee.as_ref() {
                if self.is_async_call(&name.name) {
                    return Some(((*name).clone(), args.clone()));
                }
            }
        }
        None
    }

    /// Lower a single declaration. Currently only [`Decl::FuncDecl`] produces
    /// IR nodes; other declarations (struct/enum/import/module/trait) are type
    /// metadata and are skipped in v0.1.
    fn lower_decl(&mut self, decl: &Decl) {
        if let Decl::FuncDecl(f) = decl {
            // Auto-register async-declared functions as I/O boundaries.
            if f.is_async {
                self.mark_async(&f.name.name);
            }
            self.lower_block(&f.body);
        }
    }

    /// Lower a single statement, producing exactly one IR node and returning
    /// its [`NodeId`].
    fn lower_stmt(&mut self, stmt: &Stmt) -> NodeId {
        match stmt {
            Stmt::LetDecl {
                name, value, span, ..
            } => self.create_expr_node(value, Some(name), Some(stmt), *span),

            // T71: destructuring let — the value is the data-flow source; the
            // pattern's bindings are all defined by this one node (we register
            // each in the `bindings` map so later reads wire back here). The
            // node's own `defs` list stays empty (the IR `defs` field is a
            // single-name nicety; the `bindings` map is the source of truth
            // for dependency wiring — see `wire_dependencies`).
            Stmt::LetPattern {
                pattern,
                value,
                span,
                ..
            } => {
                let node = self.create_expr_node(value, None, Some(stmt), *span);
                for b in pattern.bindings() {
                    self.bindings.insert(b.name, node);
                }
                node
            }

            Stmt::ExprStmt(expr, span) => self.create_expr_node(expr, None, Some(stmt), *span),

            Stmt::Assignment {
                target,
                value,
                span,
                ..
            } => {
                let mut uses = Vec::new();
                collect_uses(target, &mut uses);
                collect_uses(value, &mut uses);
                let defs: Vec<Ident> = match target {
                    Expr::Ident(id, _) => vec![id.clone()],
                    _ => Vec::new(),
                };
                let id = self.graph.add_node(IrNode::compute(ComputeNode {
                    id: NodeId(0),
                    source_expr: Some(value.clone()),
                    source_stmt: Some(stmt.clone()),
                    defs: defs.clone(),
                    uses: uses.clone(),
                    span: *span,
                    description: stmt.to_string(),
                }));
                self.wire_dependencies(id, &defs, &uses);
                if let Expr::Ident(ident, _) = target {
                    self.bindings.insert(ident.name.clone(), id);
                }
                id
            }

            Stmt::Return(opt_expr, span) => {
                let mut uses = Vec::new();
                if let Some(e) = opt_expr {
                    collect_uses(e, &mut uses);
                }
                let id = self.graph.add_node(IrNode::compute(ComputeNode {
                    id: NodeId(0),
                    source_expr: opt_expr.clone(),
                    source_stmt: Some(stmt.clone()),
                    defs: Vec::new(),
                    uses: uses.clone(),
                    span: *span,
                    description: stmt.to_string(),
                }));
                self.wire_dependencies(id, &[], &uses);
                id
            }

            Stmt::Break(span) => self.add_pure_control_node(stmt, *span, "Break"),
            Stmt::Continue(span) => self.add_pure_control_node(stmt, *span, "Continue"),

            Stmt::ForIn {
                var,
                iter,
                body,
                span,
            } => {
                // Represent the loop header as a Compute node that consumes
                // the iterator expression; the loop variable is bound inside
                // the loop body.
                let mut uses = Vec::new();
                collect_uses(iter, &mut uses);
                uses.retain(|i| i.name != var.name);
                let header_id = self.graph.add_node(IrNode::compute(ComputeNode {
                    id: NodeId(0),
                    source_expr: Some(iter.clone()),
                    source_stmt: Some(stmt.clone()),
                    defs: Vec::new(),
                    uses: uses.clone(),
                    span: *span,
                    description: format!("ForIn({var} in {{...}})"),
                }));
                self.wire_dependencies(header_id, &[], &uses);
                // Lower the loop body in this context. The resulting node IDs
                // depend on the iterator header via the `bindings` map only if
                // they read the loop variable; we model the loop var binding
                // as a synthetic binding pointing at the header so reads
                // inside the body wire back to it.
                self.bindings.insert(var.name.clone(), header_id);
                self.lower_block(body);
                header_id
            }

            Stmt::ForWhile { cond, body, span } => {
                let mut uses = Vec::new();
                collect_uses(cond, &mut uses);
                let header_id = self.graph.add_node(IrNode::compute(ComputeNode {
                    id: NodeId(0),
                    source_expr: Some(cond.clone()),
                    source_stmt: Some(stmt.clone()),
                    defs: Vec::new(),
                    uses: uses.clone(),
                    span: *span,
                    description: "ForWhile({...})".to_string(),
                }));
                self.wire_dependencies(header_id, &[], &uses);
                self.lower_block(body);
                header_id
            }

            // T72: `for let PAT = EXPR { body }` — a looping binding. The
            // value expression is the data-flow source (consumed each
            // iteration); the pattern's bindings are introduced inside the
            // loop body only. We model the header as a Compute node that
            // consumes the value's uses, then register each pattern binding
            // pointing at the header so reads inside the body wire back to
            // it (mirroring the ForIn treatment of the loop variable).
            Stmt::ForLet {
                pattern,
                value,
                body,
                span,
            } => {
                let mut uses = Vec::new();
                collect_uses(value, &mut uses);
                // The pattern's bindings don't read outer names (they bind),
                // but a binding NAME that also appears in `value` would be a
                // shadow — drop it from the header's uses for the same reason
                // ForIn drops the loop variable.
                let binding_names: std::collections::HashSet<String> =
                    pattern.bindings().into_iter().map(|i| i.name).collect();
                uses.retain(|i| !binding_names.contains(&i.name));
                let header_id = self.graph.add_node(IrNode::compute(ComputeNode {
                    id: NodeId(0),
                    source_expr: Some(value.clone()),
                    source_stmt: Some(stmt.clone()),
                    defs: Vec::new(),
                    uses: uses.clone(),
                    span: *span,
                    description: format!("ForLet({pattern} = {{...}})"),
                }));
                self.wire_dependencies(header_id, &[], &uses);
                for b in pattern.bindings() {
                    self.bindings.insert(b.name, header_id);
                }
                self.lower_block(body);
                header_id
            }

            // T73: `guard <conds> else { block }` — model each condition as
            // a Compute node that consumes the condition's uses; the
            // else-block is walked as a continuation. For `let` conditions,
            // the pattern's bindings are introduced IN THE ENCLOSING SCOPE
            // (mirroring Rust let-else semantics) — register each binding
            // pointing at the condition's node so subsequent reads wire
            // back to it. The whole guard collapses to its LAST condition's
            // node ID (or the else-block's last node if conditions is empty,
            // which is a parse-time error anyway).
            Stmt::Guard {
                conditions,
                else_block,
                span,
            } => {
                let mut last_id = self.add_pure_control_node(stmt, *span, "Guard");
                for c in conditions {
                    let (expr_opt, pat_bindings): (Option<Expr>, Vec<Ident>) = match c {
                        crate::stmt::GuardCondition::Let { pattern, value, .. } => {
                            (Some(value.clone()), pattern.bindings())
                        }
                        crate::stmt::GuardCondition::Bool(e) => (Some(e.clone()), Vec::new()),
                    };
                    let mut uses = Vec::new();
                    if let Some(e) = &expr_opt {
                        collect_uses(e, &mut uses);
                    }
                    let binding_names: std::collections::HashSet<String> =
                        pat_bindings.iter().map(|i| i.name.clone()).collect();
                    uses.retain(|i| !binding_names.contains(&i.name));
                    let node_id = self.graph.add_node(IrNode::compute(ComputeNode {
                        id: NodeId(0),
                        source_expr: expr_opt,
                        source_stmt: Some(stmt.clone()),
                        defs: pat_bindings.clone(),
                        uses: uses.clone(),
                        span: *span,
                        description: format!("GuardCond({c})"),
                    }));
                    self.wire_dependencies(node_id, &[], &uses);
                    // Register the let-pattern's bindings in the ENCLOSING
                    // scope (let-else semantics: they survive the guard).
                    for b in pat_bindings {
                        self.bindings.insert(b.name, node_id);
                    }
                    last_id = node_id;
                }
                // Walk the else-block as a continuation (its stmts are
                // reachable when any condition fails).
                self.lower_block(else_block);
                last_id
            }
            // T100: `defer EXPR` — model the deferred expression as a
            // Compute node consuming its uses (the expression reads outer
            // names at the defer site). The actual function-exit re-emission
            // happens in codegen, not here; the IR just records the
            // data-flow fact that the expression's uses are live at this
            // point. No bindings are introduced.
            Stmt::Defer { expr, span } => {
                let mut uses = Vec::new();
                collect_uses(expr, &mut uses);
                let node_id = self.graph.add_node(IrNode::compute(ComputeNode {
                    id: NodeId(0),
                    source_expr: Some(expr.clone()),
                    source_stmt: Some(stmt.clone()),
                    defs: Vec::new(),
                    uses: uses.clone(),
                    span: *span,
                    description: format!("Defer({expr})"),
                }));
                self.wire_dependencies(node_id, &[], &uses);
                node_id
            }
        }
    }

    /// Helper for Break/Continue: a [`ComputeNode`] with no data flow.
    fn add_pure_control_node(&mut self, stmt: &Stmt, span: Span, label: &str) -> NodeId {
        self.graph.add_node(IrNode::compute(ComputeNode {
            id: NodeId(0),
            source_expr: None,
            source_stmt: Some(stmt.clone()),
            defs: Vec::new(),
            uses: Vec::new(),
            span,
            description: label.to_string(),
        }))
    }

    /// Core node-creation routine: inspects `expr` to pick [`IrNode::IONode`]
    /// vs [`IrNode::Compute`], wires dependencies, and updates bindings.
    ///
    /// - `binding`: if `Some`, the node defines that variable (let binding).
    /// - `source_stmt`: the originating statement, kept for traceability.
    fn create_expr_node(
        &mut self,
        expr: &Expr,
        binding: Option<&Ident>,
        source_stmt: Option<&Stmt>,
        span: Span,
    ) -> NodeId {
        let defs: Vec<Ident> = binding.map_or(Vec::new(), |n| vec![n.clone()]);
        let mut uses = Vec::new();
        collect_uses(expr, &mut uses);

        let id = if let Some((callee, args)) = self.detect_async_call(expr) {
            // I/O suspension point.
            self.graph.add_node(IrNode::IONode(IoNode {
                id: NodeId(0),
                callee,
                args,
                defs: defs.clone(),
                uses: uses.clone(),
                span,
                is_suspension_point: true,
            }))
        } else {
            self.graph.add_node(IrNode::compute(ComputeNode {
                id: NodeId(0),
                source_expr: Some(expr.clone()),
                source_stmt: source_stmt.cloned(),
                defs: defs.clone(),
                uses: uses.clone(),
                span,
                description: expr.to_string(),
            }))
        };

        self.wire_dependencies(id, &defs, &uses);
        if let Some(name) = binding {
            self.bindings.insert(name.name.clone(), id);
        }
        id
    }

    /// Build dependency edges for a freshly added node.
    ///
    /// For each used variable, look up its defining node in `bindings` and
    /// add a `new_node -> def_node` dependency. For each defined variable,
    /// update `bindings` (shadowing any prior definition).
    fn wire_dependencies(&mut self, new_node: NodeId, defs: &[Ident], uses: &[Ident]) {
        for u in uses {
            if let Some(&def_node) = self.bindings.get(&u.name) {
                self.graph.add_dependency(new_node, def_node);
            }
            // Variables not in `bindings` are free (parameters/globals) — no edge.
        }
        for d in defs {
            self.bindings.insert(d.name.clone(), new_node);
        }
    }
}

// ---------------------------------------------------------------------------
// Free-variable extraction
// ---------------------------------------------------------------------------

/// Collect all free-variable [`Ident`]s read by `expr` into `out`.
///
/// Function-call *callee* identifiers that are simple names are NOT counted as
/// data uses (they name the function, not a variable). Method-call *receivers*
/// ARE counted (they carry state).
fn collect_uses(expr: &Expr, out: &mut Vec<Ident>) {
    match expr {
        Expr::Literal(_, _) => {}
        Expr::Ident(id, _) => out.push(id.clone()),

        Expr::BinaryOp { lhs, rhs, .. } => {
            collect_uses(lhs, out);
            collect_uses(rhs, out);
        }
        Expr::UnaryOp { operand, .. } => collect_uses(operand, out),

        Expr::FuncCall { callee, args, .. } => {
            // A simple-Ident callee names the function, not a data variable.
            // A complex callee (e.g. `(fptr)(x)`) is treated as a use.
            if !matches!(callee.as_ref(), Expr::Ident(_, _)) {
                collect_uses(callee, out);
            }
            for a in args {
                collect_uses(a, out);
            }
        }
        Expr::MethodCall { receiver, args, .. } => {
            collect_uses(receiver, out);
            for a in args {
                collect_uses(a, out);
            }
        }
        Expr::StructInit { fields, .. } => {
            for (_, v) in fields {
                collect_uses(v, out);
            }
        }
        Expr::SuspendExpr { inner, .. } => collect_uses(inner, out),

        // T23/T24: a collection literal uses every element expression; an
        // index expression uses its base plus every index in the (possibly
        // multi-dimensional) index list.
        Expr::ArrayLit { elements, .. } => {
            for e in elements {
                collect_uses(e, out);
            }
        }
        Expr::Index { base, indices, .. } => {
            collect_uses(base, out);
            for idx in indices {
                collect_uses(idx, out);
            }
        }

        // Compound expressions: conservatively recurse into nested blocks.
        Expr::IfExpr {
            cond,
            then_block,
            else_block,
            ..
        } => {
            collect_uses(cond, out);
            for s in &then_block.stmts {
                collect_stmt_uses(s, out);
            }
            if let Some(eb) = else_block {
                for s in &eb.stmts {
                    collect_stmt_uses(s, out);
                }
            }
        }
        Expr::Lambda { body, .. } => {
            // Conservative: collect all body uses without subtracting params.
            for s in &body.stmts {
                collect_stmt_uses(s, out);
            }
        }
        Expr::MatchExpr {
            scrutinee, arms, ..
        } => {
            collect_uses(scrutinee, out);
            for arm in arms {
                for s in &arm.body.stmts {
                    collect_stmt_uses(s, out);
                }
            }
        }
        // T21: a string interpolation uses every embedded expression. Literal
        // text runs do not contribute any uses.
        Expr::StringInterp { parts, .. } => {
            for part in parts {
                if let crate::InterpPart::Expr(e) = part {
                    collect_uses(e, out);
                }
            }
        }
        // T25: a map literal uses every key and value expression.
        Expr::MapLit { entries, .. } => {
            for (k, v) in entries {
                collect_uses(k, out);
                collect_uses(v, out);
            }
        }
        // T30: `expr?` uses its operand expression.
        Expr::Try { expr, .. } => collect_uses(expr, out),
        // T31: `spawn expr` uses its operand expression.
        Expr::Spawn { task, .. } => collect_uses(task, out),
        // T68: `start..end` uses both bounds.
        Expr::Range { start, end, .. } => {
            collect_uses(start, out);
            collect_uses(end, out);
        }
        // T72: `if let PAT = EXPR { then } else { else }` uses the value
        // expression and conservatively recurses into both blocks. The
        // pattern's bindings are NOT uses (they bind names in the then-block).
        Expr::IfLet {
            value,
            then_block,
            else_block,
            ..
        } => {
            collect_uses(value, out);
            for s in &then_block.stmts {
                collect_stmt_uses(s, out);
            }
            if let Some(eb) = else_block {
                for s in &eb.stmts {
                    collect_stmt_uses(s, out);
                }
            }
        }
        // T103: a tuple literal `(e1, e2, ...)` uses every element expression.
        Expr::TupleLit(members, _) => {
            for e in members {
                collect_uses(e, out);
            }
        }
        // T105: a named arg `name: value` uses the value expression. The
        // name is not a use (it binds to a param name, not a variable).
        Expr::NamedArg { value, .. } => collect_uses(value, out),
    }
}

/// Collect free-variable uses inside a statement (used when recursing into
/// nested blocks for if/lambda/match expressions).
fn collect_stmt_uses(stmt: &Stmt, out: &mut Vec<Ident>) {
    match stmt {
        Stmt::LetDecl { value, .. } => collect_uses(value, out),
        // T71: the destructured bindings don't read outer names (they bind);
        // only the RHS value contributes uses.
        Stmt::LetPattern { value, .. } => collect_uses(value, out),
        Stmt::Assignment { target, value, .. } => {
            collect_uses(target, out);
            collect_uses(value, out);
        }
        Stmt::ExprStmt(e, _) => collect_uses(e, out),
        Stmt::Return(Some(e), _) => collect_uses(e, out),
        Stmt::Return(None, _) | Stmt::Break(_) | Stmt::Continue(_) => {}
        Stmt::ForIn {
            var, iter, body, ..
        } => {
            // The iterator expression's uses count; the loop variable is
            // bound by the statement (introduces a new name, doesn't read
            // an outer one).
            collect_uses(iter, out);
            for s in &body.stmts {
                collect_stmt_uses(s, out);
            }
            // Remove the loop variable if it was added by the iter expr.
            out.retain(|i| i.name != var.name);
        }
        Stmt::ForWhile { cond, body, .. } => {
            collect_uses(cond, out);
            for s in &body.stmts {
                collect_stmt_uses(s, out);
            }
        }
        // T72: `for let PAT = EXPR { body }` — the value's uses count; the
        // pattern's bindings are loop-local (drop them from uses if the
        // value happened to mention the same name). Mirrors ForIn's handling
        // of the loop variable.
        Stmt::ForLet {
            pattern,
            value,
            body,
            ..
        } => {
            collect_uses(value, out);
            for s in &body.stmts {
                collect_stmt_uses(s, out);
            }
            for b in pattern.bindings() {
                out.retain(|i| i.name != b.name);
            }
        }
        // T73: `guard <conds> else { block }` — each condition's value/expr
        // reads outer names; let-pattern bindings are introduced in the
        // ENCLOSING scope (so they don't count as uses after the binding
        // site, but we still scan them out defensively). The else-block's
        // stmts are recursed.
        Stmt::Guard {
            conditions,
            else_block,
            ..
        } => {
            for c in conditions {
                match c {
                    crate::stmt::GuardCondition::Let { pattern, value, .. } => {
                        collect_uses(value, out);
                        // The pattern's bindings are introduced by this
                        // guard; remove any same-named uses that the value
                        // may have added (mirroring ForLet's treatment).
                        for b in pattern.bindings() {
                            out.retain(|i| i.name != b.name);
                        }
                    }
                    crate::stmt::GuardCondition::Bool(e) => collect_uses(e, out),
                }
            }
            for s in &else_block.stmts {
                collect_stmt_uses(s, out);
            }
        }
        // T100: `defer EXPR` — the deferred expression reads outer names.
        Stmt::Defer { expr, .. } => collect_uses(expr, out),
    }
}

// ---------------------------------------------------------------------------
// Display
// ---------------------------------------------------------------------------

/// Format a slice of [`Ident`]s as `a, b, c`.
fn fmt_idents(idents: &[Ident]) -> String {
    idents
        .iter()
        .map(|i| i.name.as_str())
        .collect::<Vec<_>>()
        .join(", ")
}

impl fmt::Display for ComputeNode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "Compute({}) defs=[{}] uses=[{}]",
            self.description,
            fmt_idents(&self.defs),
            fmt_idents(&self.uses),
        )
    }
}

impl fmt::Display for IoNode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "IO({}) defs=[{}] uses=[{}] suspend={}",
            self.callee,
            fmt_idents(&self.defs),
            fmt_idents(&self.uses),
            self.is_suspension_point,
        )
    }
}

impl fmt::Display for TransferNode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Transfer({}, {} -> {})", self.var, self.from, self.to)
    }
}

impl fmt::Display for ScheduleNode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let governed: Vec<String> = self.governed.iter().map(|n| n.to_string()).collect();
        write!(
            f,
            "Schedule(decision={}, governed=[{}])",
            self.decision,
            governed.join(", ")
        )
    }
}

impl fmt::Display for IrNode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            IrNode::Compute(n) => write!(f, "{n}"),
            IrNode::IONode(n) => write!(f, "{n}"),
            IrNode::Transfer(n) => write!(f, "{n}"),
            IrNode::Schedule(n) => write!(f, "{n}"),
        }
    }
}

impl fmt::Display for IrGraph {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let edge_count: usize = self.edges.values().map(|s| s.len()).sum();
        writeln!(
            f,
            "IR Graph ({} nodes, {} edges):",
            self.nodes.len(),
            edge_count
        )?;

        // Nodes in ascending NodeId order.
        let mut ids: Vec<NodeId> = self.nodes.keys().copied().collect();
        ids.sort();
        for id in &ids {
            if let Some(node) = self.nodes.get(id) {
                writeln!(f, "  {id} {node}")?;
            }
        }

        writeln!(f, "Edges:")?;
        for id in &ids {
            if let Some(dependents) = self.edges.get(id) {
                let mut dep_vec: Vec<NodeId> = dependents.iter().copied().collect();
                dep_vec.sort();
                for dep in dep_vec {
                    writeln!(f, "  {id} -> {dep}")?;
                }
            }
        }

        write!(f, "Entry: [")?;
        for (i, id) in self.entry_nodes.iter().enumerate() {
            if i > 0 {
                f.write_str(", ")?;
            }
            write!(f, "{id}")?;
        }
        writeln!(f, "]")?;

        write!(f, "Exit: [")?;
        for (i, id) in self.exit_nodes.iter().enumerate() {
            if i > 0 {
                f.write_str(", ")?;
            }
            write!(f, "{id}")?;
        }
        writeln!(f, "]")?;

        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Unit tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{BinaryOp, Literal};

    fn dummy() -> Span {
        Span::dummy()
    }

    fn int_lit(n: i64) -> Expr {
        Expr::Literal(Literal::Int(n), dummy())
    }

    fn ident_expr(name: &str) -> Expr {
        Expr::Ident(Ident::new(name, dummy()), dummy())
    }

    #[test]
    fn graph_new_is_empty() {
        let g = IrGraph::new();
        assert!(g.is_empty());
        assert_eq!(g.len(), 0);
        assert!(g.edges.is_empty());
        assert!(g.reverse_edges.is_empty());
    }

    #[test]
    fn node_id_display_uses_percent_prefix() {
        assert_eq!(NodeId(7).to_string(), "%7");
        assert_eq!(NodeId(0).to_string(), "%0");
    }

    #[test]
    fn add_node_assigns_sequential_ids() {
        let mut g = IrGraph::new();
        let a = g.add_node(IrNode::compute(ComputeNode {
            id: NodeId(0),
            source_expr: Some(int_lit(1)),
            source_stmt: None,
            defs: Vec::new(),
            uses: Vec::new(),
            span: dummy(),
            description: "one".to_string(),
        }));
        let b = g.add_node(IrNode::compute(ComputeNode {
            id: NodeId(0),
            source_expr: Some(int_lit(2)),
            source_stmt: None,
            defs: Vec::new(),
            uses: Vec::new(),
            span: dummy(),
            description: "two".to_string(),
        }));
        assert_eq!(a, NodeId(0));
        assert_eq!(b, NodeId(1));
        assert_eq!(g.nodes[&a].id(), a);
        assert_eq!(g.nodes[&b].id(), b);
    }

    #[test]
    fn add_dependency_populates_both_edge_maps() {
        let mut g = IrGraph::new();
        let a = g.add_node(IrNode::compute(ComputeNode {
            id: NodeId(0),
            source_expr: None,
            source_stmt: None,
            defs: Vec::new(),
            uses: Vec::new(),
            span: dummy(),
            description: "a".to_string(),
        }));
        let b = g.add_node(IrNode::compute(ComputeNode {
            id: NodeId(0),
            source_expr: None,
            source_stmt: None,
            defs: Vec::new(),
            uses: Vec::new(),
            span: dummy(),
            description: "b".to_string(),
        }));
        // b depends on a
        g.add_dependency(b, a);
        // edges[a] should contain b (a's dependent)
        assert!(g.edges[&a].contains(&b));
        // reverse_edges[b] should contain a (b's dependency)
        assert!(g.reverse_edges[&b].contains(&a));
        // And the reverse should NOT hold
        assert!(!g.edges[&b].contains(&a));
        assert!(!g.reverse_edges[&a].contains(&b));
    }

    #[test]
    fn finalize_classifies_entry_and_exit() {
        let mut g = IrGraph::new();
        let a = g.add_node(IrNode::compute(ComputeNode {
            id: NodeId(0),
            source_expr: None,
            source_stmt: None,
            defs: Vec::new(),
            uses: Vec::new(),
            span: dummy(),
            description: "a".to_string(),
        }));
        let b = g.add_node(IrNode::compute(ComputeNode {
            id: NodeId(0),
            source_expr: None,
            source_stmt: None,
            defs: Vec::new(),
            uses: Vec::new(),
            span: dummy(),
            description: "b".to_string(),
        }));
        let c = g.add_node(IrNode::compute(ComputeNode {
            id: NodeId(0),
            source_expr: None,
            source_stmt: None,
            defs: Vec::new(),
            uses: Vec::new(),
            span: dummy(),
            description: "c".to_string(),
        }));
        // Chain: a -> b -> c
        g.add_dependency(b, a);
        g.add_dependency(c, b);
        g.finalize();
        assert_eq!(g.entry_nodes, vec![a]);
        assert_eq!(g.exit_nodes, vec![c]);
    }

    #[test]
    fn lowerer_let_binding_produces_one_compute_node() {
        let s = Stmt::LetDecl {
            name: Ident::new("x", dummy()),
            value: int_lit(42),
            mutable: false,
            ty: None,
            span: dummy(),
        };
        let block = Block {
            stmts: vec![s],
            span: dummy(),
        };
        let func = Decl::FuncDecl(crate::FuncDecl {
            name: Ident::new("f", dummy()),
            params: Vec::new(),
            return_type: None,
            body: block,
            is_async: false,
            is_unsafe: false,
            is_extern: false,
            attributes: Vec::new(),
            span: dummy(),
        });
        let mut lowerer = AstLowerer::new();
        let g = lowerer.lower(&[func]);
        assert_eq!(g.len(), 1);
        match g.nodes.get(&NodeId(0)) {
            Some(IrNode::Compute(c)) => {
                assert_eq!(c.defs.len(), 1);
                assert_eq!(c.defs[0].name, "x");
                assert!(c.uses.is_empty());
            }
            other => panic!("expected ComputeNode, got {other:?}"),
        }
        assert!(g.entry_nodes.contains(&NodeId(0)));
        assert!(g.exit_nodes.contains(&NodeId(0)));
    }

    #[test]
    fn lowerer_chain_of_lets_no_transitive_edges() {
        // let a = 1; let b = a + 2; let c = b * 3;
        let a = Stmt::LetDecl {
            name: Ident::new("a", dummy()),
            value: int_lit(1),
            mutable: false,
            ty: None,
            span: dummy(),
        };
        let b = Stmt::LetDecl {
            name: Ident::new("b", dummy()),
            value: Expr::BinaryOp {
                op: BinaryOp::Add,
                lhs: Box::new(ident_expr("a")),
                rhs: Box::new(int_lit(2)),
                span: dummy(),
            },
            mutable: false,
            ty: None,
            span: dummy(),
        };
        let c = Stmt::LetDecl {
            name: Ident::new("c", dummy()),
            value: Expr::BinaryOp {
                op: BinaryOp::Mul,
                lhs: Box::new(ident_expr("b")),
                rhs: Box::new(int_lit(3)),
                span: dummy(),
            },
            mutable: false,
            ty: None,
            span: dummy(),
        };
        let block = Block {
            stmts: vec![a, b, c],
            span: dummy(),
        };
        let func = Decl::FuncDecl(crate::FuncDecl {
            name: Ident::new("chain", dummy()),
            params: Vec::new(),
            return_type: None,
            body: block,
            is_async: false,
            is_unsafe: false,
            is_extern: false,
            attributes: Vec::new(),
            span: dummy(),
        });
        let mut lowerer = AstLowerer::new();
        let g = lowerer.lower(&[func]);

        // Exactly 3 nodes (statement-level granularity).
        assert_eq!(g.len(), 3);

        // b (%1) depends on a (%0).
        assert!(g.reverse_edges[&NodeId(1)].contains(&NodeId(0)));
        // c (%2) depends on b (%1).
        assert!(g.reverse_edges[&NodeId(2)].contains(&NodeId(1)));
        // No transitive edge c -> a.
        assert!(!g.reverse_edges[&NodeId(2)].contains(&NodeId(0)));
    }

    #[test]
    fn lowerer_unrelated_lets_have_no_edge() {
        // let a = 1; let b = 2;  — no dependency between them.
        let a = Stmt::LetDecl {
            name: Ident::new("a", dummy()),
            value: int_lit(1),
            mutable: false,
            ty: None,
            span: dummy(),
        };
        let b = Stmt::LetDecl {
            name: Ident::new("b", dummy()),
            value: int_lit(2),
            mutable: false,
            ty: None,
            span: dummy(),
        };
        let block = Block {
            stmts: vec![a, b],
            span: dummy(),
        };
        let func = Decl::FuncDecl(crate::FuncDecl {
            name: Ident::new("pair", dummy()),
            params: Vec::new(),
            return_type: None,
            body: block,
            is_async: false,
            is_unsafe: false,
            is_extern: false,
            attributes: Vec::new(),
            span: dummy(),
        });
        let mut lowerer = AstLowerer::new();
        let g = lowerer.lower(&[func]);
        assert_eq!(g.len(), 2);
        // No edges at all.
        assert!(g.edges[&NodeId(0)].is_empty());
        assert!(g.edges[&NodeId(1)].is_empty());
        assert!(g.reverse_edges[&NodeId(0)].is_empty());
        assert!(g.reverse_edges[&NodeId(1)].is_empty());
        // Both are entry AND exit nodes.
        g.entry_nodes.iter().for_each(|_| ());
        assert_eq!(g.entry_nodes.len(), 2);
        assert_eq!(g.exit_nodes.len(), 2);
    }

    #[test]
    fn lowerer_async_call_becomes_io_node() {
        // let data = http_get(url);   with http_get marked async.
        let call = Expr::FuncCall {
            callee: Box::new(ident_expr("http_get")),
            args: vec![ident_expr("url")],
            span: dummy(),
        };
        let s = Stmt::LetDecl {
            name: Ident::new("data", dummy()),
            value: call,
            mutable: false,
            ty: None,
            span: dummy(),
        };
        let block = Block {
            stmts: vec![s],
            span: dummy(),
        };
        let func = Decl::FuncDecl(crate::FuncDecl {
            name: Ident::new("fetch", dummy()),
            params: Vec::new(),
            return_type: None,
            body: block,
            is_async: false,
            is_unsafe: false,
            is_extern: false,
            attributes: Vec::new(),
            span: dummy(),
        });
        let mut lowerer = AstLowerer::new();
        lowerer.mark_async("http_get");
        let g = lowerer.lower(&[func]);

        assert_eq!(g.len(), 1);
        match g.nodes.get(&NodeId(0)) {
            Some(IrNode::IONode(io)) => {
                assert_eq!(io.callee.name, "http_get");
                assert!(io.is_suspension_point);
                assert_eq!(io.defs.len(), 1);
                assert_eq!(io.defs[0].name, "data");
                // `url` is a free variable (no defining node) -> use recorded but no edge.
                assert_eq!(io.uses.len(), 1);
                assert_eq!(io.uses[0].name, "url");
            }
            other => panic!("expected IoNode, got {other:?}"),
        }
        assert!(g.nodes[&NodeId(0)].is_suspension_point());
    }

    #[test]
    fn lowerer_io_node_feeds_dependent_compute() {
        // let data = http_get(url); let len = data.length();
        let fetch = Stmt::LetDecl {
            name: Ident::new("data", dummy()),
            value: Expr::FuncCall {
                callee: Box::new(ident_expr("http_get")),
                args: vec![ident_expr("url")],
                span: dummy(),
            },
            mutable: false,
            ty: None,
            span: dummy(),
        };
        let length = Stmt::LetDecl {
            name: Ident::new("len", dummy()),
            value: Expr::MethodCall {
                receiver: Box::new(ident_expr("data")),
                method: Ident::new("length", dummy()),
                args: Vec::new(),
                span: dummy(),
            },
            mutable: false,
            ty: None,
            span: dummy(),
        };
        let block = Block {
            stmts: vec![fetch, length],
            span: dummy(),
        };
        let func = Decl::FuncDecl(crate::FuncDecl {
            name: Ident::new("fetch_len", dummy()),
            params: Vec::new(),
            return_type: None,
            body: block,
            is_async: false,
            is_unsafe: false,
            is_extern: false,
            attributes: Vec::new(),
            span: dummy(),
        });
        let mut lowerer = AstLowerer::new();
        lowerer.mark_async("http_get");
        let g = lowerer.lower(&[func]);

        assert_eq!(g.len(), 2);
        // %0 is the IoNode, %1 is the ComputeNode.
        assert!(matches!(g.nodes[&NodeId(0)], IrNode::IONode(_)));
        assert!(matches!(g.nodes[&NodeId(1)], IrNode::Compute(_)));
        // %1 depends on %0 (data defined by http_get).
        assert!(g.reverse_edges[&NodeId(1)].contains(&NodeId(0)));
        // Entry = %0, exit = %1.
        assert_eq!(g.entry_nodes, vec![NodeId(0)]);
        assert_eq!(g.exit_nodes, vec![NodeId(1)]);
    }

    #[test]
    fn topological_order_respects_dependencies() {
        // Chain a -> b -> c (b depends on a, c depends on b).
        let a = Stmt::LetDecl {
            name: Ident::new("a", dummy()),
            value: int_lit(1),
            mutable: false,
            ty: None,
            span: dummy(),
        };
        let b = Stmt::LetDecl {
            name: Ident::new("b", dummy()),
            value: ident_expr("a"),
            mutable: false,
            ty: None,
            span: dummy(),
        };
        let c = Stmt::LetDecl {
            name: Ident::new("c", dummy()),
            value: ident_expr("b"),
            mutable: false,
            ty: None,
            span: dummy(),
        };
        let block = Block {
            stmts: vec![a, b, c],
            span: dummy(),
        };
        let func = Decl::FuncDecl(crate::FuncDecl {
            name: Ident::new("topo", dummy()),
            params: Vec::new(),
            return_type: None,
            body: block,
            is_async: false,
            is_unsafe: false,
            is_extern: false,
            attributes: Vec::new(),
            span: dummy(),
        });
        let mut lowerer = AstLowerer::new();
        let g = lowerer.lower(&[func]);
        let order = g.topological_order().expect("DAG should topo-sort");
        // a (%0) before b (%1) before c (%2).
        let pos = |id: NodeId| order.iter().position(|&x| x == id);
        assert!(pos(NodeId(0)) < pos(NodeId(1)));
        assert!(pos(NodeId(1)) < pos(NodeId(2)));
    }

    #[test]
    fn cycle_detection_returns_error() {
        let mut g = IrGraph::new();
        let a = g.add_node(IrNode::compute(ComputeNode {
            id: NodeId(0),
            source_expr: None,
            source_stmt: None,
            defs: Vec::new(),
            uses: Vec::new(),
            span: dummy(),
            description: "a".to_string(),
        }));
        let b = g.add_node(IrNode::compute(ComputeNode {
            id: NodeId(0),
            source_expr: None,
            source_stmt: None,
            defs: Vec::new(),
            uses: Vec::new(),
            span: dummy(),
            description: "b".to_string(),
        }));
        // Manual cycle: a -> b -> a.
        g.add_dependency(a, b);
        g.add_dependency(b, a);
        assert!(g.topological_order().is_err());
    }

    #[test]
    fn graph_display_is_human_readable() {
        let a = Stmt::LetDecl {
            name: Ident::new("a", dummy()),
            value: int_lit(1),
            mutable: false,
            ty: None,
            span: dummy(),
        };
        let b = Stmt::LetDecl {
            name: Ident::new("b", dummy()),
            value: ident_expr("a"),
            mutable: false,
            ty: None,
            span: dummy(),
        };
        let block = Block {
            stmts: vec![a, b],
            span: dummy(),
        };
        let func = Decl::FuncDecl(crate::FuncDecl {
            name: Ident::new("disp", dummy()),
            params: Vec::new(),
            return_type: None,
            body: block,
            is_async: false,
            is_unsafe: false,
            is_extern: false,
            attributes: Vec::new(),
            span: dummy(),
        });
        let mut lowerer = AstLowerer::new();
        let g = lowerer.lower(&[func]);
        let s = g.to_string();
        assert!(s.contains("IR Graph (2 nodes, 1 edges):"));
        assert!(s.contains("%0"));
        assert!(s.contains("%1"));
        assert!(s.contains("defs=[a]"));
        assert!(s.contains("uses=[a]"));
        assert!(s.contains("%0 -> %1"));
        assert!(s.contains("Entry: [%0]"));
        assert!(s.contains("Exit: [%1]"));
    }

    #[test]
    fn async_func_decl_auto_registered() {
        // async fn io_op() { ... } called from another fn should produce IoNode.
        let io_body = Block {
            stmts: vec![Stmt::Return(Some(int_lit(0)), dummy())],
            span: dummy(),
        };
        let io_fn = Decl::FuncDecl(crate::FuncDecl {
            name: Ident::new("io_op", dummy()),
            params: Vec::new(),
            return_type: None,
            body: io_body,
            is_async: true,
            is_unsafe: false,
            is_extern: false,
            attributes: Vec::new(),
            span: dummy(),
        });
        let caller_body = Block {
            stmts: vec![Stmt::LetDecl {
                name: Ident::new("r", dummy()),
                value: Expr::FuncCall {
                    callee: Box::new(ident_expr("io_op")),
                    args: Vec::new(),
                    span: dummy(),
                },
                mutable: false,
                ty: None,
                span: dummy(),
            }],
            span: dummy(),
        };
        let caller = Decl::FuncDecl(crate::FuncDecl {
            name: Ident::new("caller", dummy()),
            params: Vec::new(),
            return_type: None,
            body: caller_body,
            is_async: false,
            is_unsafe: false,
            is_extern: false,
            attributes: Vec::new(),
            span: dummy(),
        });
        let mut lowerer = AstLowerer::new();
        let g = lowerer.lower(&[io_fn, caller]);
        // The caller's `let r = io_op()` should be an IoNode (auto-registered).
        let io_nodes: Vec<&IrNode> = g
            .nodes
            .values()
            .filter(|n| matches!(n, IrNode::IONode(_)))
            .collect();
        assert!(
            io_nodes
                .iter()
                .any(|n| matches!(n, IrNode::IONode(io) if io.callee.name == "io_op")),
            "expected an IoNode for io_op, got: {}",
            g
        );
    }
}

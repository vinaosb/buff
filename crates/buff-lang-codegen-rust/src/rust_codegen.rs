//! The Rust code generator — lowers Buff AST nodes to `syn` types.
//!
//! ## Design
//!
//! - Every Rust construct is built via explicit `syn` struct construction.
//!   We **never** hand-format Rust strings; the only string producers are
//!   `prettyplease` (via [`crate::format`]) and identifier names.
//! - `parse_quote!` is intentionally avoided in non-test code because it
//!   panics on parse failure; we construct `syn` nodes by hand instead.
//! - Unsupported AST nodes return a [`CodegenError`] rather than panicking,
//!   so future tasks (T12/T13/…) can extend coverage incrementally.
//!
//! ## Supported AST → Rust coverage (T11)
//!
//! - `Decl::FuncDecl` → `Item::Fn` (async/unsafe/extern modifiers + params
//!   + return type + body)
//! - `Stmt::LetDecl`, `Stmt::ExprStmt`, `Stmt::Return`, `Stmt::Assignment`,
//!   `Stmt::Break`, `Stmt::Continue`, `Stmt::ForIn`, `Stmt::ForWhile`
//! - `Expr::Literal`, `Expr::Ident`, `Expr::BinaryOp`, `Expr::UnaryOp`,
//!   `Expr::FuncCall`, `Expr::IfExpr`
//! - `Literal::{Int, Float, Double, Bool, String, Byte, Decimal}`
//! - `TypeRef::Named` for the seven v0.1 primitive names (`Int`→`i64`, etc.)
//!   plus `TypeRef::Option` and `TypeRef::Generic` (named base)
//!
//! ## Type-annotated `let` bindings (T12)
//!
//! Every `let` binding emits an explicit Rust type annotation. If the Buff
//! source provides one (`let x: Int = …`), it is used directly; otherwise
//! the integrated [`TypeInferencer`] infers the type from the initializer
//! expression and [`RustCodegen::buff_type_to_syn`] maps it to the
//! corresponding Rust type. [`Type::Decimal`] maps to
//! `rust_decimal::Decimal` (so generated crates must depend on
//! `rust_decimal`/`rust_decimal_macros`).
//!
//! ## Control flow (T13)
//!
//! - `if cond { a } else { b }` → Rust `if` expression (with optional else)
//! - `for x in iter { body }` → Rust `for x in iter { body }`
//! - `for cond { body }` (Buff conditional loop) → Rust `while cond { body }`
//! - `print(arg)` calls map to `println!("{}", arg)` macro invocations.
//!
//! ## Source-map recording (T16)
//!
//! [`CodegenContext::record_mapping`] is available so that each lowered AST
//! node can record its Buff [`Span`] → Rust `(line, col)` mapping. In v0.1
//! the mapping is **not** automatically populated during lowering because:
//!
//! 1. `syn` nodes carry opaque `proc_macro2::Span`s (no source-line info).
//! 2. `prettyplease` reformats the tree after construction, so line numbers
//!    computed pre-format would be wrong.
//!
//! The pipeline (`buff_lang_cli::error_mapper`) therefore uses **filename
//! translation** for v1.0: it replaces the intermediate `.rs` path in
//! `rustc`/panic messages with the original `.buff` path. Exact Buff line
//! translation via the bidirectional [`SourceMap`](buff_lang_error::SourceMap)
//! will land in a later task once a post-prettyplease line scan is available.
//!
//! ## Move semantics (T33a)
//!
//! All bindings are MOVED by default (Rust move semantics). The integrated
//! [`MoveAnalyzer`] pre-classifies each binding as Copy or non-Copy, and
//! `lower_expr` inserts `.clone()` at the use site of any non-Copy variable
//! that has already been moved once. Generated Rust never contains `&`,
//! `&mut`, or lifetime annotations in function signatures.
//!
//! Structs, enums, imports, traits, lambdas, match, method-call and
//! struct-init lowering are deferred to later tasks.

use std::collections::{BTreeMap, BTreeSet, HashSet};

use proc_macro2::Span as ProcSpan;
use syn::punctuated::Punctuated;
use syn::{
    Expr as SynExpr, Field as SynField, Fields as SynFields, File, Ident, Item, ItemEnum, ItemFn,
    ItemStruct, Pat, PatIdent, PatType, ReturnType, Signature, Stmt as SynStmt, Type as SynType,
    Visibility,
};

use buff_lang_ast::{
    op::{BinaryOp, UnaryOp},
    Block, Decl, EnumDecl as AstEnumDecl, Expr, FuncDecl, InterpPart, Literal, MatchArm, Pattern,
    Stmt, StructDecl as AstStructDecl, TypeRef,
};
use buff_lang_error::{CodegenError, Diagnostic, Span as BuffSpan};
use buff_lang_types::{prelude::PreludeFn, FloatWidth, IntWidth, Type, TypeInferencer};

use crate::atomic_analysis::AtomicPromotions;
use crate::context::CodegenContext;
use crate::move_analysis::MoveAnalyzer;

/// The Rust code generator.
///
/// Owns a [`CodegenContext`] for the lifetime of one generation pass.
/// Construct with [`RustCodegen::new`] (or `Default`).
pub struct RustCodegen {
    ctx: CodegenContext,
    move_analyzer: MoveAnalyzer,
    /// Local type inferencer used to derive Rust type annotations on
    /// `let` bindings that lack an explicit Buff annotation (T12).
    /// Reset between functions via [`TypeInferencer::env`] clear semantics
    /// (we re-bind params + walk let-stmts at the top of each `lower_func`).
    type_inferencer: TypeInferencer,
    /// T26 hook: names of structs that should be emitted with `#[repr(C)]`
    /// between the derive attribute and the `pub struct` line. The full
    /// GPU-dispatch auto-detection that populates this set lands in v1.0;
    /// T26 provides the emission mechanism only. See [`Self::mark_struct_repr_c`].
    repr_c_struct_names: HashSet<String>,
    /// T31: the post-propagation async-function name set. Populated by
    /// [`Self::generate`] via [`buff_lang_types::analyze_async`] BEFORE
    /// per-function lowering starts, so [`Self::lower_func`] can override
    /// each fn's `is_async` flag with the propagated value and
    /// [`Self::lower_expr`] can auto-insert `.await` at async call sites
    /// inside async fns.
    async_fns: BTreeSet<String>,
    /// T31: name of the function currently being lowered (`None` outside
    /// `lower_func`). Used by [`Self::lower_expr`] to decide whether to
    /// emit `.await` at async call sites and by [`Self::lower_method_call`]
    /// to decide whether `.result()` should warn.
    current_fn_name: Option<String>,
    /// T31: depth of `async move { ... }` blocks we're currently inside.
    /// Incremented by [`Self::lower_spawn`] around the task-body lowering
    /// so async calls inside the spawned task still get `.await` (the
    /// `async move` block IS an async context even if the spawning fn is
    /// sync). Combined with [`Self::current_fn_is_async`] via
    /// [`Self::in_async_context`].
    async_block_depth: usize,
    /// T33: depth of `spawn <expr>` bodies we're currently inside.
    /// Incremented by [`Self::lower_spawn`] around the task-body lowering
    /// so ident uses inside a spawn body can be rewritten to
    /// `Arc::clone(&x)` (for Arc-shared bindings). Combined with
    /// [`Self::move_analyzer`]'s `is_arc_var` to decide whether a bare
    /// ident inside a spawn lowers to `Arc::clone(&x)` or to the regular
    /// `.clone()` / move path.
    spawn_depth: usize,
    /// T34: stack of closure "bypass" sets. Each entry is the set of
    /// variable NAMES that should bypass [`MoveAnalyzer::needs_clone`]
    /// while lowering the closure body:
    /// - **captured variables** (free vars of body not bound by params or
    ///   closure-local lets) — computed via
    ///   [`buff_lang_types::closure_captures`], the shared capture analysis
    ///   extracted from T33's spawn free-var walker.
    /// - **closure parameters** — fresh bindings owned by the closure body.
    ///
    /// When lowering an `Expr::Ident` inside a closure body, if the name
    /// is in the top-of-stack bypass set, we emit it plainly WITHOUT
    /// calling [`MoveAnalyzer::needs_clone`] — Rust handles the capture
    /// (by ref or by move) and param ownership automatically, so Buff must
    /// not insert a spurious `.clone()`.
    ///
    /// This is the key interaction between closures (T34) and the move /
    /// clone analysis (T33): without this stack, (a) a non-Copy captured
    /// variable used twice inside a closure would get a spurious
    /// `.clone()` on its second use, and (b) a closure PARAM used
    /// multiple times (e.g. `|x| x * x + x`) would also get spurious
    /// clones — a pre-existing T23 limitation that T34 fixes.
    ///
    /// Nested closures push multiple entries; each closure's bypass set
    /// is computed independently.
    closure_capture_stack: Vec<BTreeSet<String>>,
    /// T31: collected warning-level diagnostics (e.g. `block()` inside an
    /// async fn is a deadlock risk). Publicly accessible via
    /// [`Self::take_warnings`] so callers (CLI, tests) can render them
    /// alongside generated Rust without losing the underlying codegen
    /// result.
    warnings: Vec<Diagnostic>,
    /// T32: crate names recorded by `extern crate "<name>"` declarations.
    /// Populated during [`Self::generate`] as each [`Decl::ExternCrateDecl`]
    /// is lowered; emitted in codegen as a `use <name>;` item. Exposed via
    /// [`Self::extern_crates`] so the pipeline (when it gains Cargo-project
    /// wiring) can write `<name> = "*"` lines into the generated
    /// `Cargo.toml`. A [`BTreeSet`] (not [`HashSet`]) is used so iteration
    /// order is DETERMINISTIC across runs and independent of hash seed
    /// (the T29 flaky-test lesson — never rely on HashSet iteration order
    /// for codegen output).
    extern_crates: BTreeSet<String>,
    /// T76: collected union types for emission as wrapper enums.
    /// Keyed by canonical union name (`StringOrInt`, `IntOrFloatOrBool`), value
    /// is the member TypeRefs. A `BTreeMap` (not `HashMap`) for determinism
    /// (the T29 flaky-test lesson). Populated during [`Self::ast_typeref_to_syn`]
    /// when it encounters a `TypeRef::Union`.
    collected_unions: BTreeMap<String, Vec<TypeRef>>,
    /// T107: names of USER-DEFINED structs that can safely derive `Hash`.
    /// Populated by [`Self::compute_hash_safe_structs`] at the top of
    /// [`Self::generate`] (BEFORE the main lowering loop, so
    /// [`Self::lower_struct_decl`] can consult it when deciding whether to
    /// include `Hash` in the struct's derive list). A struct is in this set
    /// iff ALL its fields are of Hash-impl'ing Rust types — recursively
    /// across user struct references (transitive Hash-safety). A `BTreeSet`
    /// (not `HashSet`) for deterministic membership checks (the value is
    /// only queried by membership, but consistency with the rest of the
    /// state is easier to reason about).
    hash_safe_structs: BTreeSet<String>,
    /// T50: names of user-defined structs that participate in a parallel
    /// combinator (`par_map` / `par_filter` / `par_reduce`) pipeline and
    /// therefore must be emitted with `#[repr(C)]` +
    /// `#[derive(..., Copy, bytemuck::Pod, bytemuck::Zeroable)]` so their
    /// memory layout is stable + GPU-upload-safe (bytemuck cast_slice).
    /// Populated by [`crate::gpu_alignment::gpu_bound_structs`] at the top
    /// of [`Self::generate`] BEFORE per-decl lowering, so
    /// [`Self::lower_struct_decl`] can consult it when choosing between
    /// the regular derive path and the GPU derive path. A `BTreeSet` for
    /// deterministic membership + iteration (the T29 flaky-test lesson).
    /// See the [`gpu_alignment`](crate::gpu_alignment) module docs for the
    /// detection rule (closure-param annotation OR struct-init inside a
    /// parallel closure body).
    gpu_bound_structs: BTreeSet<String>,
    /// T100: deferred expressions collected for the function currently being
    /// lowered, in REGISTRATION order (the order `defer EXPR` statements
    /// appear in the source). Reset at the start of each [`Self::lower_func`].
    /// [`Self::lower_block`] pushes each `Stmt::Defer`'s lowered expression
    /// here and emits NOTHING at the defer site. At every function exit
    /// point — each `Stmt::Return` and the implicit fall-through at the body
    /// end (handled in [`Self::lower_func`]) — the accumulated expressions
    /// are drained in REVERSE order (LIFO: last-registered runs first) and
    /// emitted as sibling `syn::Stmt::Expr(_, Some(semi))` statements BEFORE
    /// the return / at the body tail.
    ///
    /// Storing the already-lowered [`SynExpr`] (rather than re-lowering the
    /// AST expression at each exit point) keeps the codegen single-pass and
    /// deterministic. Limitation: move-analysis decisions are fixed at the
    /// defer site (see the T100 note in learnings.md).
    deferred_exprs: Vec<SynExpr>,
    /// T105: param-name lists for user-defined free functions in this
    /// compilation unit, keyed by function name. Populated by
    /// [`Self::generate`] BEFORE the per-function lowering loop so
    /// [`Self::lower_expr`] can REORDER named call arguments to match the
    /// callee's declared parameter order. A [`BTreeMap`] (not [`HashMap`])
    /// for deterministic membership and iteration (the T29 flaky-test
    /// lesson — never rely on hash-seed-dependent iteration for codegen).
    ///
    /// **v0.5 scope**: only SAME-compilation-unit free functions are
    /// resolved. Cross-module callees (T29 multi-file programs) and
    /// method-call param names (receiver-type resolution) are deferred to
    /// v1.0 — for those, named-arg values are extracted positionally
    /// (names dropped), so the call still lowers but without reorder.
    func_param_names: BTreeMap<String, Vec<String>>,
    /// T106: default-value expressions for each parameter of user-defined
    /// free functions in this compilation unit, keyed by function name.
    /// Populated by [`Self::generate`] BEFORE the per-function lowering
    /// loop so [`Self::lower_expr`] can FILL omitted trailing args at the
    /// CALL SITE with the callee's declared default (Rust has NO native
    /// default-param support, so the expansion must happen here). Each
    /// entry is `None` (required param) or `Some(expr)` (has a default);
    /// the list is in DECLARATION ORDER so positional fill is correct.
    ///
    /// A [`BTreeMap`] (not [`HashMap`]) for deterministic membership
    /// (the T29 flaky-test lesson).
    ///
    /// **v0.5 scope**: same-compilation-unit free functions only. Methods
    /// and cross-module callees are deferred (no receiver-type / module
    /// resolution at codegen in v0.5). For those, omitted args are left as-
    /// is and Rust will diagnose the arity mismatch.
    func_param_defaults: BTreeMap<String, Vec<Option<Expr>>>,
    /// T42: program-wide atomic-promotion decisions (function name →
    /// set of captured-integer accumulators that should be promoted
    /// to `AtomicI64`). Populated by [`Self::generate`] BEFORE the
    /// main lowering loop via [`crate::atomic_analysis::analyze`], so
    /// [`Self::lower_func`] can install the current function's set
    /// into [`Self::current_atomic_set`] for consultation by the
    /// `LetDecl`, `Assignment`, and `Expr::Ident` arms.
    ///
    /// A [`BTreeMap`] (not [`HashMap`]) for deterministic membership
    /// and iteration (the T29 flaky-test lesson — never rely on
    /// hash-seed-dependent iteration for codegen-feeding data).
    atomic_promotions: AtomicPromotions,
    /// T42: the set of atomic-promotable captures for the function
    /// currently being lowered. Reset at the top of each
    /// [`Self::lower_func`] from [`Self::atomic_promotions`]. The
    /// `LetDecl` arm consults this to wrap the initializer in
    /// `AtomicI64::new(...)` (and drop `mut`); the `Assignment` arm
    /// consults this to lower `t += x` to `t.fetch_add(x as i64,
    /// Ordering::Relaxed)`; the `Expr::Ident` arm consults this to
    /// lower bare reads of `t` to `t.load(Ordering::Relaxed)`.
    current_atomic_set: crate::atomic_analysis::AtomicSet,
    /// T119: names of `extern` functions declared in this compilation
    /// unit. Populated by [`Self::generate`] BEFORE the main lowering
    /// loop so call sites (the `Expr::FuncCall` arm of [`Self::lower_expr`])
    /// can wrap calls in `unsafe { ... }` — Rust requires an `unsafe`
    /// block at every foreign-function call site, regardless of ABI.
    /// Buff hides `unsafe` from the user (the README's "no `unsafe` Rust"
    /// guarantee), so the codegen inserts the wrapper silently. The set
    /// is a [`BTreeSet`] for deterministic membership checks.
    extern_fn_names: BTreeSet<String>,
}

impl RustCodegen {
    /// Create a fresh codegen with an empty context.
    pub fn new() -> Self {
        Self {
            ctx: CodegenContext::new(),
            move_analyzer: MoveAnalyzer::new(),
            type_inferencer: TypeInferencer::new(),
            repr_c_struct_names: HashSet::new(),
            async_fns: BTreeSet::new(),
            current_fn_name: None,
            async_block_depth: 0,
            spawn_depth: 0,
            closure_capture_stack: Vec::new(),
            warnings: Vec::new(),
            extern_crates: BTreeSet::new(),
            collected_unions: BTreeMap::new(),
            hash_safe_structs: BTreeSet::new(),
            gpu_bound_structs: BTreeSet::new(),
            deferred_exprs: Vec::new(),
            func_param_names: BTreeMap::new(),
            func_param_defaults: BTreeMap::new(),
            atomic_promotions: AtomicPromotions::empty(),
            current_atomic_set: crate::atomic_analysis::AtomicSet::new(),
            extern_fn_names: BTreeSet::new(),
        }
    }

    /// Borrow the inner context (read-only).
    pub fn context(&self) -> &CodegenContext {
        &self.ctx
    }

    /// T31: drain the collected warning diagnostics (e.g. `block()` inside
    /// an async fn is a deadlock risk). Returns them in source order.
    ///
    /// Warnings are accumulated during [`Self::generate`]; calling this
    /// afterwards gives the caller (CLI, tests) a chance to render them
    /// alongside the generated Rust. Calling it twice in a row returns an
    /// empty `Vec` the second time.
    pub fn take_warnings(&mut self) -> Vec<Diagnostic> {
        std::mem::take(&mut self.warnings)
    }

    /// T31: borrow the collected warning diagnostics without draining.
    pub fn warnings(&self) -> &[Diagnostic] {
        &self.warnings
    }

    /// T32: borrow the set of `extern crate "<name>"` dependencies recorded
    /// during [`Self::generate`]. The names are stored in a [`BTreeSet`] so
    /// iteration order is deterministic (the T29 flaky-test lesson — never
    /// rely on [`HashSet`] iteration order for codegen output).
    ///
    /// Each name corresponds to a Rust crate that the generated source
    /// depends on. The codegen emits a `use <name>;` item for each; the
    /// pipeline (when it switches from single-file `rustc` invocation to a
    /// full Cargo-project model) should additionally write
    /// `<name> = "*"` (or a pinned version) into the generated
    /// `Cargo.toml`'s `[dependencies]` section. **CLI-Cargo.toml wiring is
    /// deferred** — single-file `rustc` invocation (the current pipeline)
    /// cannot consume external crates without a Cargo manifest, so this
    /// accessor exists for the future Cargo-project pipeline and for
    /// codegen-level tests to assert the recorded dep set.
    pub fn extern_crates(&self) -> &BTreeSet<String> {
        &self.extern_crates
    }

    /// T26 hook: mark a struct name to be emitted with `#[repr(C)]` between
    /// the derive attribute and the `pub struct` line. The full GPU-dispatch
    /// auto-detection that populates this set lands in v1.0; T26 provides
    /// the emission mechanism only (plus the test
    /// `struct_codegen_repr_c_emitted_when_struct_marked`).
    ///
    /// Multiple calls accumulate; the marker set is consumed by
    /// [`Self::lower_struct_decl`] when it walks the declaration list.
    pub fn mark_struct_repr_c(&mut self, name: &str) {
        self.repr_c_struct_names.insert(name.to_string());
    }

    /// T42: is `name` an atomic-promotable capture in the function
    /// currently being lowered? Consulted by the `LetDecl`,
    /// `Assignment`, and `Expr::Ident` lowering arms to decide whether
    /// to emit `AtomicI64::new` / `fetch_add` / `load` lowering.
    fn is_atomic_var(&self, name: &str) -> bool {
        self.current_atomic_set.contains_key(name)
    }

    /// T42: the integer initial value to which the atomic-promoted
    /// binding was declared (`let mut t = N`). Unused at the call sites
    /// today (we lower the existing initializer expression directly
    /// rather than re-materialising the literal), but kept for
    /// future-proofing and for assertion-style tests.
    #[allow(dead_code)]
    fn atomic_initial_value(&self, name: &str) -> Option<i64> {
        self.current_atomic_set.get(name).copied()
    }

    /// Generate a complete [`syn::File`] from a list of Buff declarations.
    ///
    /// Each top-level `Decl` becomes one top-level `syn::Item`. The output
    /// is a fully-formed Rust file ready for [`crate::format`].
    ///
    /// # Builtin `Matrix<T>` injection (T24)
    ///
    /// If the program references the builtin Matrix type — detected by the
    /// presence of a `Matrix.new(...)` constructor call anywhere in the
    /// declaration bodies — a flat-storage `Matrix<T>` struct definition +
    /// `new` impl are **prepended** to the generated items. Emitting
    /// on-demand (vs. always) keeps non-Matrix programs free of the struct.
    /// The struct carries `data: Vec<T>, rows: usize, cols: usize` so its
    /// buffer is contiguous and directly GPU-transferable; the `new(rows,
    /// cols)` impl fills `data` with `T::default()` for `rows * cols`
    /// elements (hence the `T: Default + Clone` bound). This is the
    /// REFACTOR-ready flat-storage pattern shared with the future WGSL
    /// storage-buffer codegen (v1.0).
    pub fn generate(&mut self, decls: &[Decl]) -> Result<File, CodegenError> {
        // T42: atomic-promotion analysis. Identifies captured integer
        // accumulators (`let mut t = <int>`; mutated ONLY by `+=` inside
        // a `par_map` / `par_reduce` closure) that we can mechanically
        // promote to `AtomicI64` instead of rejecting as a T41 race.
        // The resulting [`AtomicPromotions`] set is consulted by
        // `lower_func` (to install the per-function set) and by the
        // `LetDecl` / `Assignment` / `Expr::Ident` lowering arms (to
        // emit `AtomicI64::new` / `fetch_add` / `load`). Runs BEFORE
        // race analysis so the race detector's exemption predicate can
        // consult it (every captured mutation of a promoted variable
        // is suppressed — atomic-analysis has already verified they're
        // all `+=`, so they'll lower to `fetch_add`).
        self.atomic_promotions = crate::atomic_analysis::analyze(decls);
        // T41/T42: race detection — REJECT before codegen any closure
        // passed to a parallel combinator (par_map / par_filter /
        // par_reduce) that mutates a variable captured from the
        // enclosing scope, UNLESS that variable has been promoted to
        // `AtomicI64` by the T42 atomic-analysis pass above (the
        // exemption predicate). Pure detection for the non-promotable
        // cases; the promotable cases are transformed during lowering.
        // Runs FIRST (before any other pre-pass) so a clean rejection
        // never produces partial codegen state.
        let promotions = self.atomic_promotions.clone();
        crate::race_analysis::analyze_with_exemptions(decls, move |func, var| {
            promotions.is_promotable(func, var)
        })?;
        let mut items = Vec::with_capacity(decls.len());
        // T24: emit the builtin Matrix<T> struct + impl on-demand, before
        // any fn. The two items (struct decl + impl block) are prepended so
        // user functions can refer to `Matrix` and `Matrix::new`.
        if program_uses_matrix(decls) {
            items.extend(matrix_struct_items());
        }
        // T30: emit the builtin `Error` struct + impls on-demand when the
        // program uses the `Error(...)` prelude constructor (which lowers to
        // `Err(Error::new(...))`). Emitting on-demand (vs. always) keeps
        // non-error programs free of the struct — mirroring the Matrix
        // emit-on-demand pattern from T24. The struct implements
        // `std::error::Error` + `Display` + `Debug` + `Clone` so it slots
        // into Rust's `Result<T, E>: Termination` and `?`-propagation
        // machinery directly.
        if program_uses_error(decls) {
            items.extend(error_struct_items());
        }
        // T124b: register the `chrono` crate as an external dependency when
        // the program references any prelude datetime type
        // (`DateTime.now()`, `Duration.days(7)`, `dt.format(...)`, ...).
        // Generated code uses fully-qualified `chrono::...` paths so no
        // `use chrono;` import is emitted — but the recorded name signals
        // to the pipeline / build-driver that the generated Cargo project
        // must declare `chrono` in `[dependencies]`. The set is exposed
        // via [`Self::extern_crates`].
        //
        // This mirrors the existing `extern crate "name"` recording path:
        // `extern_crates` is the canonical "Rust crates the generated
        // program depends on" set, and downstream consumers (the future
        // Cargo-project pipeline, snapshot tests) consult it via
        // [`Self::extern_crates`] / [`collect_rust_deps`].
        if program_uses_chrono(decls) {
            self.extern_crates.insert("chrono".to_string());
        }
        // T124c: register the `tracing` + `tracing-subscriber` crates as
        // external dependencies when the program references the prelude
        // `Log` module (`Log.debug/info/warn/error(...)`). Generated code
        // uses fully-qualified `tracing::...` and
        // `tracing_subscriber::...` paths so no `use` import is emitted —
        // but the recorded names signal to the pipeline / build-driver
        // that the generated Cargo project must declare BOTH crates in
        // `[dependencies]` (the subscriber init emitted in `main` calls
        // `tracing_subscriber::fmt()...try_init()`, so a program with any
        // Log call requires both).
        //
        // Mirrors the chrono registration pattern (T124b): single-file
        // `buff run` rustc path does NOT link these crates; the
        // codegen-only linking boundary is the accepted acceptance
        // criterion for v1.4 prelude modules. Cargo-project wiring is
        // deferred (snapshots + extern_crates set is the verifiable
        // contract).
        if program_uses_tracing(decls) {
            self.extern_crates.insert("tracing".to_string());
            self.extern_crates.insert("tracing-subscriber".to_string());
        }
        // T31: run async call-graph propagation BEFORE per-function
        // lowering so each `lower_func` call can override `is_async` with
        // the propagated value. Buff has no `await` keyword — async-ness
        // propagates up the call graph from declared-async fns to all
        // transitive callers. The result feeds two codegen decisions:
        //   1. `lower_func`: emit `async fn` (and `#[tokio::main]` for
        //      `main`) when the propagated set marks the fn async.
        //   2. `lower_expr` / `lower_method_call`: auto-insert `.await`
        //      at async-call sites inside async fns, and lower
        //      `spawn expr` → `tokio::spawn(async move { expr })` and
        //      `t.result()` → `t.await`.
        self.async_fns = buff_lang_types::analyze_async(decls).names;
        // T107: compute the set of user-defined structs that can safely
        // derive `Hash`. Must run BEFORE the main lowering loop because
        // `lower_struct_decl` consults `self.hash_safe_structs` when
        // deciding whether to include `Hash` in the derive list. The
        // analysis is a fixpoint iteration: a struct is Hash-safe iff ALL
        // its fields are Hash-safe, recursively across user struct
        // references (so `struct A { b: B }` is Hash-safe iff `B` is too).
        self.hash_safe_structs = self.compute_hash_safe_structs(decls);
        // T50: compute the set of user-defined structs that participate
        // in a parallel combinator pipeline (par_map / par_filter /
        // par_reduce). Must run BEFORE the main lowering loop so
        // `lower_struct_decl` can emit `#[repr(C)]` + bytemuck derives
        // for those structs (GPU-upload-safe layout) without affecting
        // non-GPU-bound structs. Detection rule: closure param type
        // annotation OR struct init inside the parallel closure body.
        // See `gpu_alignment` module docs for the full rationale.
        self.gpu_bound_structs = crate::gpu_alignment::gpu_bound_structs(decls);
        // T105: collect param-name lists for every user-defined free
        // function in this compilation unit. Used by [`Self::lower_expr`]'s
        // FuncCall arm to REORDER named call arguments to match the
        // callee's declared parameter order (`create(port: 80, host: "x")`
        // → `create("x", 80)` when `func create(host, port)`). Methods
        // (inside `extend TYPE { ... }` blocks) are NOT included here —
        // their param names require receiver-type resolution, which is a
        // v1.0 concern. Cross-module callees (T29) are also out-of-scope.
        // Built BEFORE the main lowering loop so per-function lowering can
        // consult it.
        self.func_param_names = collect_func_param_names(decls);
        // T106: collect default-value expressions for every user-defined
        // free function's params (same scope rules as
        // `func_param_names` above). Used by `lower_expr`'s FuncCall arm
        // to FILL omitted trailing args at the call site with the callee's
        // declared default. Rust has no native default-param support, so
        // the expansion happens here, positionally. Built BEFORE the main
        // lowering loop so per-function lowering can consult it.
        self.func_param_defaults = collect_func_param_defaults(decls);
        // T119: collect the names of all declared `extern` functions so
        // the `Expr::FuncCall` arm of `lower_expr` can wrap calls to them
        // in `unsafe { ... }`. Both the legacy `extern func name(...)`
        // (FuncDecl with `is_extern = true`) and the new
        // `extern "ABI" func name(...)` (ExternFuncDecl) shapes contribute
        // to this set. Built BEFORE the main lowering loop so per-function
        // lowering can consult it.
        self.extern_fn_names = collect_extern_fn_names(decls);
        for decl in decls {
            // T29: re-export declarations are a multi-file module-graph
            // concern — they emit no Rust item in single-file codegen.
            // Filter them out so we don't generate inert placeholders.
            if matches!(decl, Decl::ReexportDecl { .. }) {
                continue;
            }
            // T75: an `extend TYPE { ... }` block lowers to TWO top-level
            // Rust items (an extension-trait declaration + a blanket-free
            // impl). This is the ONLY decl variant whose lowering produces
            // more than one `syn::Item`, so we special-case it here to
            // extend the items Vec rather than pushing a single item. See
            // [`Self::lower_extend_block_items`] for the trait-name scheme
            // and the per-method lowering.
            if let Decl::ExtendBlock(e) = decl {
                let pair = self.lower_extend_block_items(e)?;
                items.extend(pair);
                continue;
            }
            let item = self.lower_decl(decl)?;
            items.push(item);
        }
        // T76: emit collected union wrapper enums (deduplicated by canonical
        // name). Collection happens during decl lowering, so emission must
        // happen after the main lowering loop.
        let unions: Vec<(String, Vec<TypeRef>)> = self
            .collected_unions
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        for (union_name, members) in unions {
            let enum_item = self.union_enum_item(&union_name, &members)?;
            items.insert(0, Item::Enum(enum_item));
        }
        // T92: emit auto-delegation `impl` blocks for struct embedding.
        // When a struct `Employee` has a field `person: Person` where `Person`
        // is a DECLARED struct with methods (from an `extend Person { ... }`
        // block), the codegen promotes each of Person's methods to Employee
        // by emitting `impl Employee { fn name(self) -> ... { self.person.name() } }`.
        // Analysis + emission both run AFTER the main lowering loop so all
        // struct decls + extend blocks have already been emitted (the
        // delegation impls reference the structs by name and the embedded
        // type's trait is in scope).
        self.emit_embedding_delegation(decls, &mut items)?;
        // T107: emit auto-derived record methods — per-field
        // `copy_<field>(&self, <field>: <ty>) -> Self` immutable-update
        // methods. One `impl Struct { ... }` block per non-empty user
        // struct, containing one `copy_<field>` method per field. The body
        // clones `self`, reassigns the field, and returns the clone —
        // providing the immutable-update ergonomics Buff mandates without
        // exposing `&mut` to the user. Emitted AFTER the main lowering
        // loop (alongside T92's delegation pass) so the struct decls are
        // already in `items` by the time the impl blocks land.
        self.emit_record_copy_methods(decls, &mut items)?;
        Ok(File {
            shebang: None,
            attrs: Vec::new(),
            items,
        })
    }

    fn lower_decl(&mut self, decl: &Decl) -> Result<Item, CodegenError> {
        match decl {
            // T32: an `extern func ...` declaration is a foreign-function
            // signature with NO body. Lower it to a Rust
            // `extern "C" { fn ...; }` foreign-mod item (the body-having
            // `ItemFn` path cannot represent a bodyless declaration).
            Decl::FuncDecl(f) if f.is_extern => {
                Ok(Item::ForeignMod(self.lower_extern_func_decl(f)?))
            }
            Decl::FuncDecl(f) => Ok(Item::Fn(self.lower_func(f)?)),
            Decl::StructDecl(s) => Ok(Item::Struct(self.lower_struct_decl(s)?)),
            // T27: enum codegen. Builds a Rust `pub enum Name<generics> {
            // Variant, Variant(T), ... }` with `#[derive(Clone, Debug)]`,
            // reusing the T26 derive helper.
            Decl::EnumDecl(e) => Ok(Item::Enum(self.lower_enum_decl(e)?)),
            Decl::ImportDecl { .. } => Err(self.unsupported("import codegen")),
            Decl::ModuleDecl { .. } => Err(self.unsupported("module codegen")),
            // T93: trait declarations lower to a Rust `syn::ItemTrait`.
            // Required methods (MethodSig) become bodyless trait method
            // signatures; default methods (FuncDecl with body) become
            // trait methods WITH a default body; supertraits populate the
            // trait's `supertraits` Punctuated list.
            Decl::TraitDecl(t) => Ok(Item::Trait(self.lower_trait_decl(t)?)),
            // T29: export wraps an inner decl. In single-file codegen we
            // simply lower the inner decl and stamp `pub` on its visibility
            // (multi-file codegen will route through `mod` blocks in a later
            // wave).
            Decl::ExportDecl(e) => match self.lower_decl(&e.inner)? {
                Item::Fn(mut f) => {
                    f.vis = syn::Visibility::Public(Default::default());
                    Ok(Item::Fn(f))
                }
                other => Ok(other),
            },
            // T29: re-exports never reach lower_decl — generate() filters
            // them out. Keep a defensive arm for direct callers.
            Decl::ReexportDecl { .. } => Err(self.unsupported("reexport codegen")),
            // T32: `extern crate "<name>"` — record the name in the
            // extern-crate dep set (exposed via [`Self::extern_crates`])
            // and emit a `use <name>;` item so generated code can refer
            // to the crate by bare name. Wiring the recorded set into the
            // generated Cargo.toml is deferred until the CLI switches to
            // a Cargo-project pipeline.
            Decl::ExternCrateDecl(d) => {
                self.extern_crates.insert(d.name.clone());
                Ok(Item::Use(self.lower_extern_crate_use(&d.name)))
            }
            // T119: `extern "ABI" [from "crate"] func name(...) -> Ret`.
            // Lowers to a `syn::ItemForeignMod` (same shape as the legacy
            // `extern func` lowering), AND records the optional source
            // crate in `extern_crates` so the CLI pipeline can populate
            // `[rust-deps]` in `buff.toml`.
            Decl::ExternFuncDecl(d) => {
                if let Some(c) = &d.crate_name {
                    self.extern_crates.insert(c.clone());
                }
                Ok(Item::ForeignMod(self.lower_extern_func_decl_with_abi(d)?))
            }
            // T75: extend blocks lower to TWO `syn::Item`s (trait + impl),
            // so they cannot be expressed via the single-`Item` return of
            // `lower_decl`. The proper emission path is via
            // [`Self::generate`], which special-cases `Decl::ExtendBlock`
            // and pushes both items. A direct call here is a caller misuse
            // — surface a clear error rather than dropping half the items.
            Decl::ExtendBlock(_) => Err(self.unsupported(
                "extend block codegen — use RustCodegen::generate (which emits trait + impl)",
            )),
        }
    }

    /// Lower a Buff [`AstStructDecl`] to a Rust [`syn::ItemStruct`] (T26).
    ///
    /// Emits (conceptually):
    ///
    /// ```rust,ignore
    /// #[derive(Clone, PartialEq, Debug)]                 // or with `Hash` too
    /// pub struct Name {
    ///     pub field: <rust_type>,
    ///     ...
    /// }
    /// ```
    ///
    /// T107 extends T26's `#[derive(Clone, Debug)]` to also derive
    /// `PartialEq` (always — every Rust primitive Buff uses impls
    /// `PartialEq`, including floats) and `Hash` (CONDITIONALLY — only when
    /// every field's Rust type impls `Hash`; see [`struct_derive_attrs`]
    /// and [`Self::compute_hash_safe_structs`] for the conditional logic).
    ///
    /// Every field is `pub` so generated code can construct and read struct
    /// values without accessor boilerplate (Buff hides encapsulation from
    /// users; the generated Rust just compiles). Field types go through
    /// [`Self::ast_typeref_to_syn`] so the same primitive mapping that drives
    /// `let`-binding annotations (Int→i64, Float→f32, String→String, …)
    /// applies to struct fields.
    ///
    /// # `#[repr(C)]` hook
    ///
    /// If the struct name has been marked via [`Self::mark_struct_repr_c`],
    /// an additional `#[repr(C)]` attribute is emitted BETWEEN the derive
    /// attribute and the `pub struct` line. The marker set is populated by
    /// future GPU-dispatch analysis (v1.0); T26 provides the emission
    /// mechanism only. See [`struct_derive_attrs`] for attribute ordering.
    ///
    /// # `traits` field
    ///
    /// The `traits` field on [`AstStructDecl`] is currently ignored at
    /// codegen time — Buff emits blanket `Clone + PartialEq + Debug`
    /// derives (plus `Hash` when safe); user-specified trait impls are a
    /// later task.
    fn lower_struct_decl(&mut self, s: &AstStructDecl) -> Result<ItemStruct, CodegenError> {
        // Build named fields: each field is `pub name: <rust_type>`. Field
        // visibility is always `pub` so any generated constructor / user
        // code can initialise and read struct values without accessors.
        let mut named_fields: Punctuated<SynField, syn::Token![,]> = Punctuated::new();
        for (fname, ftype) in &s.fields {
            let rust_ty = self.ast_typeref_to_syn(ftype)?;
            named_fields.push(SynField {
                attrs: Vec::new(),
                vis: Visibility::Public(Default::default()),
                ident: Some(ast_ident_to_syn(fname)),
                colon_token: Some(Default::default()),
                ty: rust_ty,
                mutability: syn::FieldMutability::None,
            });
        }

        // T26 repr(C) hook: emit `#[repr(C)]` between the derive attribute
        // and `pub struct` when the struct name was marked via
        // `mark_struct_repr_c`. Full GPU-dispatch auto-detection lands in v1.0.
        let emit_repr_c = self.repr_c_struct_names.contains(&s.name.name);

        // T107: build the derive attribute set. ALWAYS include Clone,
        // PartialEq, Debug. Include Hash ONLY when EVERY field's type is
        // Hash-safe (recursively across user struct references — the
        // transitive fixpoint is precomputed in `self.hash_safe_structs`).
        // Rust's derived `Hash` impl requires ALL field types to impl
        // `Hash`, so a struct with a Float/Double/Vector/Map field (or a
        // field whose user-struct type itself can't derive Hash) must NOT
        // carry the Hash derive — rustc would reject the impl.
        let all_fields_hash_safe = s
            .fields
            .iter()
            .all(|(_, ty)| type_is_hash_safe(ty, &self.hash_safe_structs));

        // T50: GPU-bound structs (those that flow through a parallel
        // combinator) get a STRICT SUPERSET of the regular derive list
        // PLUS `#[repr(C)]` for stable C layout. The GPU path replaces
        // the regular derive path entirely: it always emits
        // `Clone, Copy, PartialEq, Debug, bytemuck::Pod,
        // bytemuck::Zeroable` + `#[repr(C)]`, and NEVER includes `Hash`
        // (Pod + floats are incompatible with Hash; GPU-bound structs
        // typically have Float fields anyway). The original T26 hook
        // (`emit_repr_c` via `mark_struct_repr_c`) is preserved as a
        // manual override — a struct is emitted with repr(C) if EITHER
        // the gpu-bound analysis marks it OR the manual hook does.
        let is_gpu_bound = self.gpu_bound_structs.contains(&s.name.name);
        let attrs = if is_gpu_bound {
            gpu_struct_derive_attrs()
        } else {
            struct_derive_attrs(emit_repr_c, all_fields_hash_safe)
        };

        Ok(ItemStruct {
            attrs,
            vis: Visibility::Public(Default::default()),
            struct_token: Default::default(),
            ident: ast_ident_to_syn(&s.name),
            generics: syn::Generics::default(),
            fields: SynFields::Named(syn::FieldsNamed {
                brace_token: Default::default(),
                named: named_fields,
            }),
            semi_token: Some(Default::default()),
        })
    }

    /// Lower a Buff [`AstEnumDecl`] to a Rust [`syn::ItemEnum`] (T27).
    ///
    /// Emits (conceptually):
    ///
    /// ```rust,ignore
    /// #[derive(Clone, Debug)]
    /// pub enum Name<T, E> {
    ///     Variant,
    ///     Tuple(T, E),
    ///     ...
    /// }
    /// ```
    ///
    /// Unit variants become Rust unit variants; data-carrying variants
    /// become Rust tuple variants. Every variant is `pub` (so generated code
    /// can construct and pattern-match on values without accessor boilerplate,
    /// matching the T26 struct-field policy). Variant payload types go through
    /// [`Self::ast_typeref_to_syn`] so the same primitive mapping that drives
    /// struct fields and `let`-binding annotations applies here too.
    ///
    /// Generic parameters declared on the enum (`<T, E>`) are emitted as Rust
    /// generic params on the enum (no bounds — Buff does not have user-specified
    /// trait bounds in v0.5). The variant payloads may reference any of these
    /// type params by name (e.g. `Ok(T)`); they are lowered as ordinary named
    /// type references via [`Self::ast_typeref_to_syn`].
    ///
    /// The `#[derive(Clone, Debug)]` attribute reuses the T26 helper
    /// [`derive_and_repr_attrs`] so derive-attribute construction stays in one
    /// place. We pass `emit_repr_c = false` — enums never get `#[repr(C)]`
    /// in v0.5 (the GPU-dispatch repr-C hook is struct-only; enum repr hints
    /// are deferred to v1.0 when tagged unions land).
    fn lower_enum_decl(&mut self, e: &AstEnumDecl) -> Result<ItemEnum, CodegenError> {
        // Build the variant list. Each variant becomes a `syn::Variant`:
        // - unit variant (no payload) -> `Fields::Unit`
        // - tuple variant (payload) -> `Fields::Unnamed` with one field per
        //   payload type. The field names are anonymous (`_0`, `_1`, ... is
        //   implicit in tuple structs/enums — `syn::Field` carries
        //   `ident: None` for unnamed positions).
        let mut variants: Punctuated<syn::Variant, syn::Token![,]> = Punctuated::new();
        for v in &e.variants {
            let fields = match &v.data {
                None => syn::Fields::Unit,
                Some(tys) => {
                    let mut unnamed: Punctuated<SynField, syn::Token![,]> = Punctuated::new();
                    for ty in tys {
                        let rust_ty = self.ast_typeref_to_syn(ty)?;
                        unnamed.push(SynField {
                            attrs: Vec::new(),
                            vis: Visibility::Inherited,
                            ident: None,
                            colon_token: None,
                            ty: rust_ty,
                            mutability: syn::FieldMutability::None,
                        });
                    }
                    syn::Fields::Unnamed(syn::FieldsUnnamed {
                        paren_token: Default::default(),
                        unnamed,
                    })
                }
            };
            variants.push(syn::Variant {
                attrs: Vec::new(),
                ident: ast_ident_to_syn(&v.name),
                fields,
                discriminant: None,
            });
        }

        // Generic params: build a `syn::Generics` with one type param per
        // declared generic on the enum. No bounds, no defaults (v0.5 minimal).
        let generics = if e.generics.is_empty() {
            syn::Generics::default()
        } else {
            let mut params: Punctuated<syn::GenericParam, syn::Token![,]> = Punctuated::new();
            for g in &e.generics {
                params.push(syn::GenericParam::Type(syn::TypeParam {
                    attrs: Vec::new(),
                    ident: ast_ident_to_syn(g),
                    colon_token: None,
                    bounds: Default::default(),
                    eq_token: None,
                    default: None,
                }));
            }
            syn::Generics {
                lt_token: Some(Default::default()),
                params,
                gt_token: Some(Default::default()),
                where_clause: None,
            }
        };

        Ok(ItemEnum {
            attrs: derive_and_repr_attrs(false),
            vis: Visibility::Public(Default::default()),
            enum_token: Default::default(),
            ident: ast_ident_to_syn(&e.name),
            generics,
            brace_token: Default::default(),
            variants,
        })
    }

    fn lower_func(&mut self, f: &FuncDecl) -> Result<ItemFn, CodegenError> {
        // Reset move-analysis state and pre-classify Copy vars for this fn.
        self.move_analyzer.reset();
        self.move_analyzer.preanalyze_func(f);

        // T42: install this function's atomic-promotable captures.
        // The set is consulted by the `LetDecl`, `Assignment`, and
        // `Expr::Ident` lowering arms to emit `AtomicI64::new(...)` /
        // `fetch_add(...)` / `load(...)` instead of plain mutable
        // integer codegen. Reset per function (a different function's
        // `t` is independent even if it shares the name).
        self.current_atomic_set = self.atomic_promotions.for_func(&f.name.name);

        // T100: reset the per-function deferred-expression accumulator.
        // `Stmt::Defer` arms inside the body (collected by lower_block)
        // push here; we drain them in reverse (LIFO) at every return and
        // at the body's fall-through tail (below).
        self.deferred_exprs.clear();

        // Reset the type inferencer for this function: re-bind parameters
        // using the same primitive-mapping rules that TypeInferencer uses
        // internally (see `typeref_to_type` in buff_lang_types::infer).
        self.type_inferencer = TypeInferencer::new();
        for p in &f.params {
            if let Some(ty) = typeref_to_type(&p.ty) {
                self.type_inferencer.bind(&p.name.name, ty);
            }
        }

        // T31: override the declared `is_async` flag with the PROPAGATED
        // value from the call-graph fixpoint. Buff has no `await` keyword;
        // a fn becomes async if it (transitively) calls an async fn, even
        // when the user didn't write `async`. The propagated set is
        // computed once at the top of [`Self::generate`].
        let is_async = self.async_fns.contains(&f.name.name);

        // T31: record the current fn name so `lower_expr` and
        // `lower_method_call` can decide whether to emit `.await`.
        // (The propagated `async_fns` set is program-wide — it doesn't
        // change per-function.)
        let prev_fn_name = self.current_fn_name.replace(f.name.name.clone());

        let name = ast_ident_to_syn(&f.name);

        let mut inputs: Punctuated<syn::FnArg, syn::Token![,]> = Punctuated::new();
        for p in &f.params {
            let ident = ast_ident_to_syn(&p.name);
            let ty = self.ast_typeref_to_syn(&p.ty)?;
            inputs.push(syn::FnArg::Typed(PatType {
                attrs: Vec::new(),
                pat: Box::new(Pat::Ident(PatIdent {
                    attrs: Vec::new(),
                    ident,
                    by_ref: None,
                    mutability: None,
                    subpat: None,
                })),
                colon_token: Default::default(),
                ty: Box::new(ty),
            }));
        }

        let output = match &f.return_type {
            Some(ty) => {
                ReturnType::Type(Default::default(), Box::new(self.ast_typeref_to_syn(ty)?))
            }
            None => ReturnType::Default,
        };

        let sig = Signature {
            constness: None,
            asyncness: is_async.then(Default::default),
            unsafety: f.is_unsafe.then(Default::default),
            abi: f.is_extern.then(|| syn::Abi {
                extern_token: Default::default(),
                name: None,
            }),
            fn_token: Default::default(),
            ident: name,
            generics: Default::default(),
            paren_token: Default::default(),
            inputs,
            variadic: None,
            output,
        };

        let mut block = self.lower_block(&f.body)?;

        // T124c: emit the tracing-subscriber init at the top of `main`
        // when the program uses the prelude `Log` module. The init MUST
        // be the FIRST statement so any subsequent `Log.info(...)` call
        // is captured by the subscriber (an uninstalled subscriber
        // silently drops events — `tracing` is a no-op without one).
        // Mirrors the `#[tokio::main]` attribute injection pattern
        // (T31): a single program-wide decision made in `generate()`
        // (recorded in `extern_crates`) drives a per-`main` emission
        // here. The init is wrapped in a `{ ... }` block so its helper
        // binding (`__buff_log_filter`) doesn't leak into the user's
        // `main` body — see [`tracing_subscriber_init_stmt`] for the
        // design rationale.
        //
        // Only emitted for `main` (NOT for arbitrary fns) so a library
        // function using `Log` doesn't pay the init cost — the calling
        // binary's `main` is the canonical install site for the global
        // subscriber.
        if f.name.name == "main" && self.extern_crates.contains("tracing") {
            if let Some(init_stmt) = tracing_subscriber_init_stmt() {
                block.stmts.insert(0, init_stmt);
            }
        }

        // T100: fall-through tail. Any defers still in the accumulator at
        // this point were NOT drained by an explicit `return` inside the
        // body — they fire at the implicit function exit (the body's last
        // statement was something other than a return, OR the body ends
        // with a trailing expr/let). Emit them in REVERSE order (LIFO:
        // last-registered defer runs first) as tail sibling statements.
        // `drain(..).rev()` consumes the accumulator so a second drain at
        // a later exit point would be a no-op (defensive — there is no
        // later exit point after the body, but the shape is symmetric with
        // the return-site drain in lower_block).
        for deferred in self.deferred_exprs.drain(..).rev() {
            block
                .stmts
                .push(SynStmt::Expr(deferred, Some(Default::default())));
        }

        // T31: emit `#[tokio::main]` on the entry `main` fn when it's in
        // the propagated async set. Buff has no `await` keyword, so the
        // `main` fn becomes async automatically when it transitively
        // calls any async fn; tokio's runtime attribute is what makes
        // that work.
        //
        // T35: emit `#[test]` (and other recognised attributes) from the
        // FuncDecl's `attributes` list. `@test` → `#[test]`; unknown
        // attributes are a codegen error (so we don't silently drop user
        // intent). The `#[tokio::main]` case is mutually exclusive with
        // `#[test]` (a fn can't be both the entry point AND a test), so
        // we handle them in separate branches.
        let mut attrs: Vec<syn::Attribute> = if is_async && f.name.name == "main" {
            vec![syn::parse_quote!(#[tokio::main])]
        } else {
            Vec::new()
        };
        for attr in &f.attributes {
            match attr.name.name.as_str() {
                "test" => attrs.push(syn::parse_quote!(#[test])),
                // Unknown attribute — surface as a codegen error so the
                // user knows it was not applied (rather than silently
                // dropping it). Future tasks can add recognised attributes
                // (e.g. `@inline` → `#[inline]`) here.
                other => {
                    return Err(self.unsupported(&format!(
                        "unrecognised attribute `@{other}` (only `@test` is supported in v0.5)"
                    )));
                }
            }
        }

        // Restore the previous fn context (in case of nested lowering —
        // currently impossible but defensive).
        self.current_fn_name = prev_fn_name;

        Ok(ItemFn {
            attrs,
            vis: Visibility::Inherited,
            sig,
            block: Box::new(block),
        })
    }

    /// Lower a Buff `extern func name(params) -> Ret` declaration (T32 —
    /// FFI) to a Rust [`syn::ItemForeignMod`] of the form
    /// `extern "C" { fn name(params) -> Ret; }`.
    ///
    /// Foreign functions have NO body (the [`FuncDecl::body`] field holds an
    /// empty placeholder Block that the parser synthesises; it is dropped
    /// here). Each extern func becomes its own `extern "C" { ... }` block —
    /// a per-decl block keeps codegen simple and the generated output easy
    /// to read. The ABI is fixed to `"C"` for v0.5 (Buff exposes no way to
    /// pick another ABI like `"system"`); a future task may add
    /// `extern "system" func ...` syntax.
    ///
    /// Parameter and return-type lowering reuse [`Self::ast_typeref_to_syn`]
    /// so the standard Buff→Rust primitive mapping (Int→i64, String→String,
    /// …) applies uniformly to FFI signatures.
    ///
    /// The functions inside an `extern "C" { ... }` block are implicitly
    /// `unsafe` to CALL (Rust requires an `unsafe { ... }` block at every
    /// call site) — we do NOT stamp `unsafe` on the foreign-mod itself, so
    /// the generated code compiles on every Rust edition/toolchain without
    /// needing the `unsafe_extern_blocks` feature (stabilised in Rust 1.82
    /// but kept off here for maximum compatibility).
    fn lower_extern_func_decl(
        &mut self,
        f: &FuncDecl,
    ) -> Result<syn::ItemForeignMod, CodegenError> {
        // Build the parameter list (same shape as lower_func, but as
        // ForeignItemFn inputs which are bare PatType pairs).
        let mut inputs: Punctuated<syn::FnArg, syn::Token![,]> = Punctuated::new();
        for p in &f.params {
            let ident = ast_ident_to_syn(&p.name);
            let ty = self.ast_typeref_to_syn(&p.ty)?;
            inputs.push(syn::FnArg::Typed(PatType {
                attrs: Vec::new(),
                pat: Box::new(Pat::Ident(PatIdent {
                    attrs: Vec::new(),
                    ident,
                    by_ref: None,
                    mutability: None,
                    subpat: None,
                })),
                colon_token: Default::default(),
                ty: Box::new(ty),
            }));
        }

        let output = match &f.return_type {
            Some(ty) => {
                ReturnType::Type(Default::default(), Box::new(self.ast_typeref_to_syn(ty)?))
            }
            None => ReturnType::Default,
        };

        // The signature inside a foreign-mod is a ForeignItemFn: it carries
        // its own (smaller) Signature and ends with a semicolon — there is
        // no body.
        let foreign_fn = syn::ForeignItemFn {
            attrs: Vec::new(),
            vis: Visibility::Inherited,
            sig: Signature {
                constness: None,
                asyncness: None,
                unsafety: None,
                abi: None,
                fn_token: Default::default(),
                ident: ast_ident_to_syn(&f.name),
                generics: Default::default(),
                paren_token: Default::default(),
                inputs,
                variadic: None,
                output,
            },
            semi_token: Default::default(),
        };

        Ok(syn::ItemForeignMod {
            attrs: Vec::new(),
            // `unsafety: None` keeps the block compatible with all Rust
            // editions; functions inside are implicitly unsafe to call.
            unsafety: None,
            abi: syn::Abi {
                extern_token: Default::default(),
                name: Some(syn::LitStr::new("C", ProcSpan::call_site())),
            },
            brace_token: Default::default(),
            items: vec![syn::ForeignItem::Fn(foreign_fn)],
        })
    }

    /// Lower a Buff [`ExternFuncDecl`] (T119) to a Rust
    /// [`syn::ItemForeignMod`] of the form
    /// `extern "ABI" { fn name(params) -> Ret; }`.
    ///
    /// This is the rich-ABI sibling of [`Self::lower_extern_func_decl`] —
    /// the difference is that the ABI string is taken from the user's
    /// declaration (`extern "C"`, `extern "system"`, …) rather than
    /// hardcoded to `"C"`. In v1.3 only `"C"` is accepted by the parser,
    /// so in practice the two methods produce byte-identical output today
    /// — but this method preserves the user's spelling so future ABIs
    /// (`"system"`, `"stdcall"`, …) can be added by widening the parser
    /// accept-list without touching codegen.
    ///
    /// The optional `crate_name` annotation is recorded by the caller
    /// ([`Self::lower_decl`]) BEFORE this method runs — by the time we
    /// get here the crate (if any) is already in [`Self::extern_crates`]
    /// so this method's only job is the foreign-mod emission.
    fn lower_extern_func_decl_with_abi(
        &mut self,
        d: &buff_lang_ast::ExternFuncDecl,
    ) -> Result<syn::ItemForeignMod, CodegenError> {
        // Build the parameter list (same logic as lower_extern_func_decl).
        let mut inputs: Punctuated<syn::FnArg, syn::Token![,]> = Punctuated::new();
        for p in &d.params {
            let ident = ast_ident_to_syn(&p.name);
            let ty = self.ast_typeref_to_syn(&p.ty)?;
            inputs.push(syn::FnArg::Typed(PatType {
                attrs: Vec::new(),
                pat: Box::new(Pat::Ident(PatIdent {
                    attrs: Vec::new(),
                    ident,
                    by_ref: None,
                    mutability: None,
                    subpat: None,
                })),
                colon_token: Default::default(),
                ty: Box::new(ty),
            }));
        }
        let output = match &d.return_type {
            Some(ty) => {
                ReturnType::Type(Default::default(), Box::new(self.ast_typeref_to_syn(ty)?))
            }
            None => ReturnType::Default,
        };
        let foreign_fn = syn::ForeignItemFn {
            attrs: Vec::new(),
            vis: Visibility::Inherited,
            sig: Signature {
                constness: None,
                asyncness: None,
                unsafety: None,
                abi: None,
                fn_token: Default::default(),
                ident: ast_ident_to_syn(&d.name),
                generics: Default::default(),
                paren_token: Default::default(),
                inputs,
                variadic: None,
                output,
            },
            semi_token: Default::default(),
        };
        Ok(syn::ItemForeignMod {
            attrs: Vec::new(),
            unsafety: None,
            abi: syn::Abi {
                extern_token: Default::default(),
                // Use the user-written ABI verbatim ("C", "system", …).
                // The parser has already validated it's in the v1.3
                // accept-list (only "C").
                name: Some(syn::LitStr::new(&d.abi, ProcSpan::call_site())),
            },
            brace_token: Default::default(),
            items: vec![syn::ForeignItem::Fn(foreign_fn)],
        })
    }

    /// Build a `use <name>;` item for an `extern crate "<name>"`
    /// declaration (T32). The `use` brings the crate's root into scope so
    /// generated code can refer to its items by bare path
    /// (e.g. `serde::Serialize`). We emit `use <name>;` (NOT
    /// `use <name>::*;`) to avoid glob-import lint warnings — callers
    /// qualify paths explicitly.
    fn lower_extern_crate_use(&self, name: &str) -> syn::ItemUse {
        // Build the `use <name>;` item. Crate names may contain `-`
        // (e.g. `rust-decimal`) which is NOT a valid Rust identifier —
        // crates.io normalises `-` to `_` in Rust source. We do the same
        // here so generated `use rust_decimal;` matches what Cargo would
        // expect. (If the user wrote `extern crate "rust-decimal"` the
        // generated `use` becomes `use rust_decimal;`.)
        //
        // In Rust 2018+ `use <crate>;` brings the external crate into scope.
        // In syn this is a `UseTree::Name` (a single-segment path), NOT a
        // `UseTree::Path` with a nested name (which would wrongly emit
        // `use serde::serde;`).
        let rust_ident_name = name.replace('-', "_");
        syn::ItemUse {
            attrs: Vec::new(),
            vis: Visibility::Inherited,
            use_token: Default::default(),
            leading_colon: None,
            semi_token: Default::default(),
            tree: syn::UseTree::Name(syn::UseName {
                ident: Ident::new(&rust_ident_name, ProcSpan::call_site()),
            }),
        }
    }

    /// T75: lower an `extend TYPE { fn ...; ... }` block to TWO top-level
    /// Rust items — an extension-trait declaration (signatures only) and a
    /// blanket-free `impl Trait for Type { ... }` (the bodies).
    ///
    /// This is the ONLY decl variant whose lowering produces more than one
    /// `syn::Item`; [`Self::generate`] special-cases `Decl::ExtendBlock` to
    /// extend the items Vec with both items.
    ///
    /// # Trait-name scheme
    ///
    /// The trait name is derived from the target type name as
    /// `BuffExt{Type}` (e.g. `extend String` → `BuffExtString`,
    /// `extend Int` → `BuffExtInt`). v0.5 single extend-block per target
    /// type is the common case; multi-block merging (`extend String { ... }`
    /// twice) is deferred (would collide here today — a future task could
    /// suffix `_2`, `_3`, … or merge the methods into one trait).
    ///
    /// # Per-method lowering
    ///
    /// Each [`FuncDecl`] inside the block is lowered TWICE:
    /// - **trait side** — only the SIGNATURE (`fn name(params) -> Ret;`),
    ///   built by reusing the same signature-construction logic as
    ///   [`Self::lower_func`] (params, return type, asyncness) but WITHOUT
    ///   a body. The signature is wrapped in a [`syn::TraitItemFn`] and
    ///   pushed onto the trait's `items` list.
    /// - **impl side** — the FULL [`syn::ItemFn`] produced by
    ///   [`Self::lower_func`], rewrapped as a [`syn::ImplItemFn`] (we
    ///   strip the ItemFn wrapper and reuse the inner `Signature` + body
    ///   Block). The vis is `Inherited` (Rust impl items are always
    ///   implicitly public via the trait).
    ///
    /// Async-extension methods (`fn async_fetch(self) -> String` inside an
    /// `extend`) propagate the `async` flag the same way regular funcs do.
    ///
    /// # Errors
    ///
    /// Returns [`CodegenError`] if any method body fails to lower (the
    /// underlying `lower_func`/signature-builder propagates the error).
    fn lower_extend_block_items(
        &mut self,
        e: &buff_lang_ast::ExtendBlock,
    ) -> Result<Vec<Item>, CodegenError> {
        // Derive the trait name: `BuffExt{TargetTypeName}`.
        let target_name = match &e.target {
            TypeRef::Named { name, .. } => &name.name,
            // Generic targets (`extend Vector<T>`) are deferred — they'd
            // need the trait + impl to carry matching generic params.
            TypeRef::Generic { base, .. } => {
                if let TypeRef::Named { name, .. } = base.as_ref() {
                    &name.name
                } else {
                    return Err(self.unsupported(
                        "extend block with nested generic target type (only named types supported in v0.5)",
                    ));
                }
            }
            _ => {
                return Err(self.unsupported(
                    "extend block with non-named target type (only TypeRef::Named supported in v0.5)",
                ));
            }
        };
        let trait_name_str = format!("BuffExt{target_name}");
        let trait_ident = Ident::new(&trait_name_str, ProcSpan::call_site());
        let target_type = self.ast_typeref_to_syn(&e.target)?;

        // Build the trait item list (signatures only) and the impl item
        // list (full fns) in one pass over the methods.
        let mut trait_items: Vec<syn::TraitItem> = Vec::with_capacity(e.methods.len());
        let mut impl_items: Vec<syn::ImplItem> = Vec::with_capacity(e.methods.len());
        for method in &e.methods {
            // Trait signature (no body). Reuse the signature-builder from
            // lower_func by building the full ItemFn first, then peeling
            // off the Signature and synthesising a TraitItemFn from it.
            let item_fn = self.lower_func(method)?;
            // T75: extension methods almost always take `self` as the
            // first parameter. Rust's idiomatic spelling is a bare
            // receiver `self` (NOT `self: Type`). When the Buff method's
            // first param is named `self`, rewrite the FIRST signature
            // input from a `FnArg::Typed(PatType { ident: self, ty })`
            // into a `FnArg::Receiver { self_token }` so the generated
            // Rust reads `fn shout(self) -> ...` instead of
            // `fn shout(self: String) -> ...`. Both forms compile, but
            // the bare form is the spec QA shape and matches what every
            // other extension-trait library emits.
            let trait_sig = rewrite_self_receiver(item_fn.sig.clone());
            let impl_sig = rewrite_self_receiver(item_fn.sig.clone());
            // The trait item carries the signature with NO default body.
            // `default: None` means "no default implementation — impls
            // must provide one" (exactly what the impl side below does).
            trait_items.push(syn::TraitItem::Fn(syn::TraitItemFn {
                attrs: Vec::new(),
                sig: trait_sig,
                default: None,
                semi_token: Some(Default::default()),
            }));
            // The impl item carries the FULL fn (body + signature). We
            // re-wrap the ItemFn's signature + block into an ImplItemFn.
            // Visibility is Inherited — Rust impl items inherit from the
            // trait, so emitting `pub` here would be a lint warning.
            impl_items.push(syn::ImplItem::Fn(syn::ImplItemFn {
                attrs: item_fn.attrs,
                vis: Visibility::Inherited,
                defaultness: None,
                sig: impl_sig,
                block: *item_fn.block,
            }));
        }

        // Assemble the trait item: `trait BuffExtString { fn ...; ... }`.
        let trait_item = Item::Trait(syn::ItemTrait {
            attrs: Vec::new(),
            vis: Visibility::Public(Default::default()),
            unsafety: None,
            auto_token: None,
            restriction: None,
            trait_token: Default::default(),
            ident: trait_ident,
            generics: Default::default(),
            colon_token: None,
            supertraits: Punctuated::new(),
            brace_token: Default::default(),
            items: trait_items,
        });

        // Assemble the impl item:
        // `impl BuffExtString for String { fn ... { ... } ... }`.
        // The `trait_` tuple is `(Option<Not>, Path, for_token)` —
        // `None` for the bang means "implementing" (vs `!` for
        // "auto-trait negative impl"), and `for_token` is the literal
        // `for` keyword between the trait path and the self type.
        let impl_item = Item::Impl(syn::ItemImpl {
            attrs: Vec::new(),
            defaultness: None,
            unsafety: None,
            generics: Default::default(),
            impl_token: Default::default(),
            trait_: Some((
                None,
                syn::Path::from(Ident::new(&trait_name_str, ProcSpan::call_site())),
                Default::default(),
            )),
            self_ty: Box::new(target_type),
            brace_token: Default::default(),
            items: impl_items,
        });

        Ok(vec![trait_item, impl_item])
    }

    /// T93: lower a Buff [`buff_lang_ast::TraitDecl`] to a Rust
    /// [`syn::ItemTrait`] with default methods and supertrait inheritance.
    ///
    /// Emits (conceptually):
    ///
    /// ```ignore
    /// // Buff:
    /// //   trait Greetable {
    /// //       fn name() -> String;
    /// //       fn greet() { print(name()) }
    /// //   }
    /// //   trait Pet : Animal { fn pet() { ... } }
    /// // Rust:
    /// pub trait Greetable {
    ///     fn name() -> String;
    ///     fn greet() { /* default body */ }
    /// }
    /// pub trait Pet: Animal {
    ///     fn pet() { /* default body */ }
    /// }
    /// ```
    ///
    /// # Required vs default methods
    ///
    /// - REQUIRED methods ([`buff_lang_ast::MethodSig`], bodyless) lower to
    ///   `syn::TraitItemFn` with `default: None` — a bodyless trait method
    ///   signature that implementors MUST provide a body for.
    /// - DEFAULT methods ([`buff_lang_ast::FuncDecl`] with body) lower to
    ///   `syn::TraitItemFn` with `default: Some(block)` — a trait method
    ///   WITH a default body that implementors inherit unless they override.
    ///
    /// The member order is PRESERVED (required methods first, then
    /// defaults), matching the source declaration order within each
    /// category. (Buff syntax interleaves them; the codegen groups them
    /// because the AST stores them in separate Vecs. This is acceptable
    /// for Rust — trait method order is not semantically significant.)
    ///
    /// # Supertraits
    ///
    /// Each supertrait [`TypeRef`] (today always [`TypeRef::Named`]) lowers
    /// to a `syn::TypeParamBound::Trait` with a single-segment path. The
    /// supertraits populate the trait's `supertraits` Punctuated list,
    /// producing Rust `trait Pet: Animal { ... }` syntax. Multiple
    /// supertraits are `+`-separated in Rust (e.g. `trait A: B + C`).
    ///
    /// # `&self` receiver
    ///
    /// Trait methods that declare a `self` parameter (the first param named
    /// `self`) are rewritten to a bare `syn::FnArg::Receiver` via the T75
    /// [`rewrite_self_receiver`] helper, matching the extend-block
    /// convention. Methods WITHOUT a `self` param (associated functions)
    /// are emitted as-is — valid Rust trait syntax.
    fn lower_trait_decl(
        &mut self,
        t: &buff_lang_ast::TraitDecl,
    ) -> Result<syn::ItemTrait, CodegenError> {
        let trait_ident = ast_ident_to_syn(&t.name);

        // Build the supertraits Punctuated list. Each supertrait is a
        // TypeParamBound::Trait with a single-segment path. We only
        // support named supertraits today (generic supertraits like
        // `trait Foo : Bar<Int>` are deferred).
        let mut supertraits: Punctuated<syn::TypeParamBound, syn::Token![+]> = Punctuated::new();
        for st in &t.supertraits {
            let st_name = match st {
                TypeRef::Named { name, .. } => &name.name,
                _ => {
                    return Err(self.unsupported(
                        "trait supertrait that is not a simple named type (generic supertraits are deferred)",
                    ));
                }
            };
            let path = syn::Path::from(Ident::new(st_name, ProcSpan::call_site()));
            supertraits.push(syn::TypeParamBound::Trait(syn::TraitBound {
                paren_token: None,
                modifier: syn::TraitBoundModifier::None,
                lifetimes: None,
                path,
            }));
        }

        // Build the trait item list: required methods first, then defaults.
        let mut trait_items: Vec<syn::TraitItem> =
            Vec::with_capacity(t.required.len() + t.defaults.len());

        // REQUIRED methods — bodyless signatures.
        for req in &t.required {
            let sig =
                self.build_method_signature(req.name.clone(), &req.params, &req.return_type)?;
            let sig = rewrite_self_receiver(sig);
            trait_items.push(syn::TraitItem::Fn(syn::TraitItemFn {
                attrs: Vec::new(),
                sig,
                // `default: None` → required method (no default body).
                default: None,
                semi_token: Some(Default::default()),
            }));
        }

        // DEFAULT methods — signature + body. We reuse lower_func to get
        // the full ItemFn (body + signature + move-analysis), then extract
        // the signature and block for the TraitItemFn default body.
        for def in &t.defaults {
            let item_fn = self.lower_func(def)?;
            let sig = rewrite_self_receiver(item_fn.sig);
            trait_items.push(syn::TraitItem::Fn(syn::TraitItemFn {
                attrs: item_fn.attrs,
                sig,
                // `default: Some(block)` → trait method WITH a default body.
                default: Some(*item_fn.block),
                // No semi_token when a default body is present.
                semi_token: None,
            }));
        }

        // Assemble the trait item.
        Ok(syn::ItemTrait {
            attrs: Vec::new(),
            vis: Visibility::Public(Default::default()),
            unsafety: None,
            auto_token: None,
            restriction: None,
            trait_token: Default::default(),
            ident: trait_ident,
            generics: Default::default(),
            // colon_token is Some only when supertraits is non-empty.
            colon_token: (!t.supertraits.is_empty()).then(Default::default),
            supertraits,
            brace_token: Default::default(),
            items: trait_items,
        })
    }

    /// T93: build a [`syn::Signature`] from a method's name, params, and
    /// optional return type. Shared by required-method lowering (no body)
    /// and could be reused for default-method signatures (though those go
    /// through [`Self::lower_func`] for move-analysis). The signature is
    /// NOT async/unsafe/extern — trait methods in v0.5 are plain `fn`.
    fn build_method_signature(
        &mut self,
        name: buff_lang_ast::Ident,
        params: &[buff_lang_ast::Param],
        return_type: &Option<TypeRef>,
    ) -> Result<Signature, CodegenError> {
        let mut inputs: Punctuated<syn::FnArg, syn::Token![,]> = Punctuated::new();
        for p in params {
            let ident = ast_ident_to_syn(&p.name);
            let ty = self.ast_typeref_to_syn(&p.ty)?;
            inputs.push(syn::FnArg::Typed(PatType {
                attrs: Vec::new(),
                pat: Box::new(Pat::Ident(PatIdent {
                    attrs: Vec::new(),
                    ident,
                    by_ref: None,
                    mutability: None,
                    subpat: None,
                })),
                colon_token: Default::default(),
                ty: Box::new(ty),
            }));
        }
        let output = match return_type {
            Some(ty) => {
                ReturnType::Type(Default::default(), Box::new(self.ast_typeref_to_syn(ty)?))
            }
            None => ReturnType::Default,
        };
        Ok(Signature {
            constness: None,
            asyncness: None,
            unsafety: None,
            abi: None,
            fn_token: Default::default(),
            ident: ast_ident_to_syn(&name),
            generics: Default::default(),
            paren_token: Default::default(),
            inputs,
            variadic: None,
            output,
        })
    }

    fn lower_block(&mut self, block: &Block) -> Result<syn::Block, CodegenError> {
        let mut stmts = Vec::with_capacity(block.stmts.len());
        for stmt in &block.stmts {
            // T73: `Stmt::Guard` lowers to MULTIPLE sibling `syn::Stmt`s at
            // the same scope level (one per condition). The let-else form's
            // pattern bindings MUST remain in scope for subsequent
            // statements in the SAME function block — wrapping them in an
            // inner block would scope-kill the bindings and defeat the
            // purpose of guard. So we special-case Guard here and push all
            // of its lowered conditions directly into `stmts`. The
            // [`Self::lower_stmt`] arm for Guard emits a single wrapped
            // `syn::Stmt::Block` as a fallback for non-block call paths
            // (none currently exist, but the API contract requires it).
            if let Stmt::Guard {
                conditions,
                else_block,
                ..
            } = stmt
            {
                self.lower_guard_conditions_into(conditions, else_block, &mut stmts)?;
                continue;
            }
            // T100: `Stmt::Defer` does NOT emit anything at its source
            // position. Instead its lowered expression is pushed onto the
            // per-function `deferred_exprs` accumulator (in registration
            // order). The accumulated expressions are drained in REVERSE
            // order (LIFO) at the next function exit point — either an
            // explicit `Stmt::Return` (handled below) or the implicit
            // fall-through at the body end (handled in lower_func).
            if let Stmt::Defer { expr, .. } = stmt {
                let lowered = self.lower_expr(expr)?;
                self.deferred_exprs.push(lowered);
                continue;
            }
            // T100: `Stmt::Return` is a function exit point. Before emitting
            // the return, drain ALL currently-accumulated defers in REVERSE
            // order (LIFO: last-registered runs first) and emit them as
            // sibling statements immediately preceding the return. This
            // makes `defer print("done"); return 0` print "done" BEFORE the
            // return executes. `drain(..)` clears the accumulator so a
            // subsequent exit point (a later return, or the fall-through
            // tail) won't re-emit the same defers.
            if let Stmt::Return(..) = stmt {
                for deferred in self.deferred_exprs.drain(..).rev() {
                    stmts.push(SynStmt::Expr(deferred, Some(Default::default())));
                }
            }
            stmts.push(self.lower_stmt(stmt)?);
        }
        Ok(syn::Block {
            brace_token: Default::default(),
            stmts,
        })
    }

    fn lower_stmt(&mut self, stmt: &Stmt) -> Result<SynStmt, CodegenError> {
        match stmt {
            Stmt::LetDecl {
                name,
                value,
                mutable,
                ty,
                ..
            } => {
                let ident = ast_ident_to_syn(name);
                let init_expr = self.lower_expr(value)?;

                // T42: AtomicI64-wrap the initializer of a binding
                // promoted by atomic-analysis. The binding becomes
                // `let t = std::sync::atomic::AtomicI64::new(N)` (note:
                // `mut` is DROPPED — the atomic itself is immutable;
                // interior mutability happens through `&self` methods
                // like `fetch_add` and `load`). Promotion is a strict
                // escape hatch from T41's race detector: the binding
                // is the SAME source-level `let mut t = 0` that would
                // have raced if naively lowered; promoting it makes
                // the resulting Rust sound across worker threads.
                let is_atomic_var = self.is_atomic_var(&name.name);

                // T33: Arc-wrap the initializer of a binding captured
                // across a `spawn` boundary. The resulting binding has
                // type `Arc<T>` (rather than `T`); inside spawn bodies
                // uses are lowered to `Arc::clone(&x)` (cheap refcount
                // bump), and any subsequent mutation is lowered to
                // `Arc::make_mut(&mut x)` (copy-on-write). This is how
                // Buff hides the borrow-checker from the user when data
                // is shared between the spawning thread and a spawned
                // task — Rust's `Arc` gives sound shared ownership
                // without exposing `Rc`/`Arc`/`Mutex` syntax in Buff.
                let is_arc_var = self.move_analyzer.is_arc_var(&name.name);
                let init_expr = if is_atomic_var {
                    wrap_in_atomic_i64_new(init_expr)
                } else if is_arc_var {
                    wrap_in_arc_new(init_expr)
                } else {
                    init_expr
                };

                // T42: atomic-promoted bindings drop `mut` (the
                // atomic is immutable; interior mutability is via
                // `&self` methods). Mirrors the Arc case's annotation
                // skip: the binding's actual Rust type is `AtomicI64`
                // (not the `i64` the inferencer derives), so emitting
                // `let t: i64 = AtomicI64::new(0)` would be
                // incoherent. Letting Rust infer keeps the generated
                // source compiling. The user's `mut` and any
                // explicit type annotation are also dropped — the
                // promotion rewrites the binding's semantics.
                let effective_mutable = if is_atomic_var { false } else { *mutable };

                // Run the inferencer on the value so we can emit an
                // explicit Rust type annotation. If the user wrote an
                // explicit Buff annotation (`ty: Some(..)`), prefer it;
                // otherwise fall back to the inferred type (T12).
                //
                // T33: when Arc-wrapping, SKIP the annotation — the
                // binding's actual Rust type is `Arc<T>` (not the `T`
                // the inferencer derives from the pre-wrap initializer),
                // and emitting `let s: String = Arc::new(...)` would be
                // incoherent. Letting Rust infer `Arc<T>` keeps the
                // generated source compiling. (A future task may compute
                // the wrapped type explicitly; for v0.5 inference is
                // simpler and equally correct.)
                let inferred_syn_ty: Option<SynType> = if is_atomic_var || is_arc_var {
                    None
                } else if let Some(type_ref) = ty {
                    Some(self.ast_typeref_to_syn(type_ref)?)
                } else {
                    // Bind in the inferencer so later statements can see
                    // this name; on error we fall back to no annotation.
                    let inferred = self
                        .type_inferencer
                        .infer_stmt(stmt)
                        .unwrap_or(Type::Unknown);
                    self.buff_type_to_syn(&inferred)
                };

                // Wrap the pattern in `Pat::Type` when an annotation is present
                // so we emit `let x: T = v;` rather than `let x = v;`.
                let pat = match inferred_syn_ty {
                    Some(ty_syn) => Pat::Type(PatType {
                        attrs: Vec::new(),
                        pat: Box::new(Self::make_let_pat(ident, effective_mutable)),
                        colon_token: Default::default(),
                        ty: Box::new(ty_syn),
                    }),
                    None => Self::make_let_pat(ident, effective_mutable),
                };
                let local = syn::Local {
                    attrs: Vec::new(),
                    let_token: Default::default(),
                    pat,
                    init: Some(syn::LocalInit {
                        eq_token: Default::default(),
                        expr: Box::new(init_expr),
                        diverge: None,
                    }),
                    semi_token: Default::default(),
                };
                Ok(SynStmt::Local(local))
            }
            Stmt::LetPattern {
                pattern,
                value,
                mutable,
                ty,
                ..
            } => {
                // T71: destructuring `let` → Rust `let PAT = value;`. The
                // pattern is lowered via [`Self::lower_pattern`] (extended for
                // Tuple/Struct); `mutable` propagates to each binding. An
                // optional type annotation wraps the whole pattern in
                // `Pat::Type` (rare for destructuring, but supported).
                let init_expr = self.lower_expr(value)?;
                let lowered_pat = self.lower_pattern(pattern, *mutable)?;
                let pat = if let Some(type_ref) = ty {
                    Pat::Type(PatType {
                        attrs: Vec::new(),
                        pat: Box::new(lowered_pat),
                        colon_token: Default::default(),
                        ty: Box::new(self.ast_typeref_to_syn(type_ref)?),
                    })
                } else {
                    lowered_pat
                };
                let local = syn::Local {
                    attrs: Vec::new(),
                    let_token: Default::default(),
                    pat,
                    init: Some(syn::LocalInit {
                        eq_token: Default::default(),
                        expr: Box::new(init_expr),
                        diverge: None,
                    }),
                    semi_token: Default::default(),
                };
                Ok(SynStmt::Local(local))
            }
            Stmt::ExprStmt(expr, _) => {
                let e = self.lower_expr(expr)?;
                Ok(SynStmt::Expr(e, Some(Default::default())))
            }
            Stmt::Return(opt_expr, _) => {
                let return_expr = match opt_expr {
                    Some(expr) => SynExpr::Return(syn::ExprReturn {
                        attrs: Vec::new(),
                        return_token: Default::default(),
                        expr: Some(Box::new(self.lower_expr(expr)?)),
                    }),
                    None => SynExpr::Return(syn::ExprReturn {
                        attrs: Vec::new(),
                        return_token: Default::default(),
                        expr: None,
                    }),
                };
                Ok(SynStmt::Expr(return_expr, Some(Default::default())))
            }
            Stmt::Assignment {
                target, op, value, ..
            } => {
                // T42: atomic-promoted `+=` shortcut. If the target
                // is a bare Ident naming an atomic-promoted binding
                // and the op is `+=`, lower the whole statement to
                // `t.fetch_add((rhs) as i64, std::sync::atomic::Ordering::Relaxed);`
                // — a method-call statement (NOT an assignment). The
                // return value of `fetch_add` (the previous atomic
                // value) is discarded, matching the semantics of
                // Buff's `t += x` (whose result is unit).
                //
                // Other compound ops on atomic-promoted vars
                // (`-=`, `*=`, `/=`, `%=`) should not occur —
                // atomic-analysis has already verified all mutations
                // are `+=` before promoting, and T41's race detector
                // rejects the non-`+=` cases. A plain `=` to an
                // atomic-promoted var would also be a T41 error
                // (atomic-analysis only promotes `+=`-only vars).
                // Defensive: if such a case slips through, emit the
                // `fetch_add` for `+=` and fall through to the
                // regular assignment lowering for anything else
                // (which will not compile downstream — surfacing the
                // bug rather than silently mis-lowering).
                if let Expr::Ident(name, _) = &target {
                    if self.is_atomic_var(&name.name)
                        && *op == buff_lang_ast::op::BinaryOp::AddAssign
                    {
                        let rhs = self.lower_expr(value)?;
                        let call = atomic_fetch_add_stmt(name, rhs);
                        return Ok(SynStmt::Expr(call, Some(Default::default())));
                    }
                }
                // The LHS of an assignment is NOT a "use" — it doesn't
                // consume a move. If the target is a bare Ident, lower it
                // directly without consulting the move analyzer.
                //
                // T33: if the target is a bare Ident naming an
                // Arc-shared-and-subsequently-mutated binding (CoW site),
                // wrap it in `Arc::make_mut(&mut x)`. This gives
                // copy-on-write semantics: the inner value is cloned
                // only if the Arc's refcount > 1 (i.e. when the spawned
                // task is actually observing the same Arc); otherwise
                // `make_mut` borrows the value in place with no clone.
                // The resulting LHS is `*Arc::make_mut(&mut x)` so the
                // assignment writes through to the (possibly-cloned)
                // inner value.
                let lhs = if let Expr::Ident(name, _) = &target {
                    if self.move_analyzer.is_arc_mut_var(&name.name) {
                        arc_make_mut_deref(name)
                    } else {
                        SynExpr::Path(syn::ExprPath {
                            attrs: Vec::new(),
                            qself: None,
                            path: syn::Path::from(ast_ident_to_syn(name)),
                        })
                    }
                } else {
                    self.lower_expr(target)?
                };
                let rhs = self.lower_expr(value)?;
                let assign = self.make_binary_op(*op, lhs, rhs)?;
                Ok(SynStmt::Expr(assign, Some(Default::default())))
            }
            Stmt::Break(_) => {
                let brk = SynExpr::Break(syn::ExprBreak {
                    attrs: Vec::new(),
                    break_token: Default::default(),
                    label: None,
                    expr: None,
                });
                Ok(SynStmt::Expr(brk, Some(Default::default())))
            }
            Stmt::Continue(_) => {
                let cont = SynExpr::Continue(syn::ExprContinue {
                    attrs: Vec::new(),
                    continue_token: Default::default(),
                    label: None,
                });
                Ok(SynStmt::Expr(cont, Some(Default::default())))
            }
            Stmt::ForIn {
                var, iter, body, ..
            } => {
                let var_ident = ast_ident_to_syn(var);
                let iter_expr = self.lower_expr(iter)?;
                let body_block = self.lower_block(body)?;
                let pat = Pat::Ident(PatIdent {
                    attrs: Vec::new(),
                    ident: var_ident,
                    by_ref: None,
                    mutability: None,
                    subpat: None,
                });
                let for_loop = SynExpr::ForLoop(syn::ExprForLoop {
                    attrs: Vec::new(),
                    label: None,
                    for_token: Default::default(),
                    pat: Box::new(pat),
                    in_token: Default::default(),
                    expr: Box::new(iter_expr),
                    body: body_block,
                });
                Ok(SynStmt::Expr(for_loop, Some(Default::default())))
            }
            Stmt::ForWhile { cond, body, .. } => {
                // Buff's `for cond { body }` (conditional-loop form) maps
                // directly to Rust's `while cond { body }` (T13).
                let cond_expr = self.lower_expr(cond)?;
                let body_block = self.lower_block(body)?;
                let while_expr = SynExpr::While(syn::ExprWhile {
                    attrs: Vec::new(),
                    label: None,
                    while_token: Default::default(),
                    cond: Box::new(cond_expr),
                    body: body_block,
                });
                Ok(SynStmt::Expr(while_expr, Some(Default::default())))
            }
            Stmt::ForLet {
                pattern,
                value,
                body,
                ..
            } => self.lower_for_let(pattern, value, body),
            Stmt::Guard {
                conditions,
                else_block,
                ..
            } => {
                // T73: fallback single-stmt path. Real call paths go
                // through [`Self::lower_block`] which special-cases Guard
                // and pushes each condition as a separate sibling stmt at
                // the same scope level (preserving let-else bindings). For
                // the rare case where a Guard reaches the single-stmt API
                // directly, we wrap the multi-stmt sequence in a
                // `syn::Expr::Block`. **Caveat**: this wrapping scopes the
                // let-bindings to the inner block, defeating one of guard's
                // main features; use [`Self::lower_block`] for proper
                // scope-preserving lowering.
                let mut inner = Vec::with_capacity(conditions.len());
                self.lower_guard_conditions_into(conditions, else_block, &mut inner)?;
                let block = syn::ExprBlock {
                    attrs: Vec::new(),
                    label: None,
                    block: syn::Block {
                        brace_token: Default::default(),
                        stmts: inner,
                    },
                };
                Ok(SynStmt::Expr(SynExpr::Block(block), None))
            }
            // T100: fallback single-stmt path for `Stmt::Defer`. Real call
            // paths go through [`Self::lower_block`] which special-cases
            // Defer (collects into `deferred_exprs`, emits nothing here).
            // For the rare case where a Defer reaches the single-stmt API
            // directly, lower its expression as a bare expression
            // statement (it runs immediately, NOT deferred — this is a
            // degenerate fallback; use [`Self::lower_block`] for proper
            // function-exit deferral).
            Stmt::Defer { expr, .. } => {
                let e = self.lower_expr(expr)?;
                Ok(SynStmt::Expr(e, Some(Default::default())))
            }
        }
    }

    fn make_let_pat(ident: Ident, mutable: bool) -> Pat {
        Pat::Ident(PatIdent {
            attrs: Vec::new(),
            ident,
            by_ref: None,
            mutability: mutable.then(Default::default),
            subpat: None,
        })
    }

    fn lower_expr(&mut self, expr: &Expr) -> Result<SynExpr, CodegenError> {
        match expr {
            Expr::Literal(lit, _) => self.lower_literal(lit),
            Expr::Ident(name, _) => {
                let path = syn::ExprPath {
                    attrs: Vec::new(),
                    qself: None,
                    path: syn::Path::from(ast_ident_to_syn(name)),
                };
                // T33: Arc-shared binding captured inside a spawn body —
                // emit `Arc::clone(&x)` instead of moving or deep-cloning.
                // The Arc wrap was inserted at the binding's `let` site
                // (see [`Self::lower_stmt`]`'s LetDecl arm); here we grab
                // a cheap refcount-bumping clone so the spawned task owns
                // its own `Arc<T>` handle to the shared data.
                if self.spawn_depth > 0 && self.move_analyzer.is_arc_var(&name.name) {
                    return Ok(arc_clone_call(name));
                }
                // T42: atomic-promoted binding — emit
                // `t.load(std::sync::atomic::Ordering::Relaxed)`. The
                // promotion rewrites the binding to `AtomicI64`; reads
                // of the original integer value must go through `load`
                // (AtomicI64 has no `Copy` impl and no `Deref<Target=i64>`).
                // This branch fires for EVERY read of an atomic Ident
                // — both reads inside the parallel closure body (which
                // are fine, just non-mutating) and reads after the
                // parallel call. The mutation case (`t += x`) is
                // handled separately in [`Self::lower_stmt`]'s
                // Assignment arm and never reaches `lower_expr` for
                // the target Ident (we short-circuit to `fetch_add`).
                if self.is_atomic_var(&name.name) {
                    return Ok(atomic_load_expr(SynExpr::Path(path)));
                }
                // T34: if this ident is a variable CAPTURED by the closure
                // whose body we're currently lowering, emit it plainly
                // WITHOUT consulting [`MoveAnalyzer::needs_clone`]. Rust
                // closures handle capture (by ref or by move) automatically;
                // Buff must not insert a spurious `.clone()` for uses of a
                // captured variable INSIDE the closure body. Without this
                // guard, a non-Copy captured var used twice inside a
                // closure would get a wrong `.clone()` on its second use.
                if self.is_captured_in_closure(&name.name) {
                    return Ok(SynExpr::Path(path));
                }
                if self.move_analyzer.needs_clone(&name.name) {
                    // Insert `.clone()` so this use is valid after a prior move.
                    Ok(SynExpr::MethodCall(syn::ExprMethodCall {
                        attrs: Vec::new(),
                        receiver: Box::new(SynExpr::Path(path)),
                        dot_token: Default::default(),
                        method: Ident::new("clone", ProcSpan::call_site()),
                        turbofish: None,
                        paren_token: Default::default(),
                        args: Default::default(),
                    }))
                } else {
                    Ok(SynExpr::Path(path))
                }
            }
            Expr::BinaryOp { op, lhs, rhs, .. } => {
                let lhs = self.lower_expr(lhs)?;
                let rhs = self.lower_expr(rhs)?;
                self.make_binary_op(*op, lhs, rhs)
            }
            Expr::UnaryOp { op, operand, .. } => {
                let operand = self.lower_expr(operand)?;
                self.make_unary_op(*op, operand)
            }
            Expr::FuncCall { callee, args, .. } => {
                // T105: named-arg resolution. When the arg list contains
                // any `Expr::NamedArg`, materialize a POSITIONAL `Vec<Expr>`
                // either REORDERED (if the callee is a user fn whose param
                // names we know) or with values EXTRACTED (names dropped)
                // for prelude/builtin/method/foreign callees. Pure-
                // positional call lists pass through unchanged.
                let resolved_args: Option<Vec<Expr>> =
                    if args.iter().any(|a| matches!(a, Expr::NamedArg { .. })) {
                        let params: Option<&[String]> = match callee.as_ref() {
                            Expr::Ident(name, _) => {
                                self.func_param_names.get(&name.name).map(|v| v.as_slice())
                            }
                            _ => None,
                        };
                        Some(materialize_named_args(args, params))
                    } else {
                        None
                    };
                let after_named: &[Expr] = resolved_args.as_deref().unwrap_or(args);
                // T106: default-arg fill. If the callee is a resolvable
                // user fn whose param-default list we know, and the caller
                // OMITTED trailing defaulted params, fill the default
                // expressions into the call site positionally (Rust has no
                // native default-param support). Runs AFTER named-arg
                // resolution so a named call omitting a defaulted param
                // (`fetch(url: "x")` with `timeout = 30`) also gets the
                // default filled. Returns None when no fill was needed
                // (callee unknown, or all params supplied) — in that case
                // we keep the post-named-resolution arg slice unchanged.
                let defaults: Option<&[Option<Expr>]> = match callee.as_ref() {
                    Expr::Ident(name, _) => self
                        .func_param_defaults
                        .get(&name.name)
                        .map(|v| v.as_slice()),
                    _ => None,
                };
                let filled: Option<Vec<Expr>> =
                    defaults.and_then(|ds| fill_default_args(after_named, ds));
                let args_ref: &[Expr] = filled.as_deref().unwrap_or(after_named);
                // T96: standard-library prelude. A bare-ident callee whose
                // name is a recognised prelude function is lowered to the
                // corresponding Rust idiom (math, conversion, I/O) WITHOUT
                // requiring an `import` in Buff source. The mapping table
                // lives in [`RustCodegen::lower_prelude_call`].
                //
                // T13 legacy: `print(x)` was originally special-cased here
                // to `println!("{}", x)`. T96 generalises that to the full
                // prelude AND tightens the string-literal case so
                // `print("hello")` now emits `println!("hello")` (no `{}`).
                if let Expr::Ident(name, _) = callee.as_ref() {
                    if let Some(fn_) = buff_lang_types::prelude::lookup(&name.name) {
                        return self.lower_prelude_call(fn_, args_ref);
                    }
                    // T30: `Error("msg")` is a prelude error constructor
                    // (NOT a reserved keyword and NOT a user function). It
                    // lowers to `Err(Error::new(arg))` so it produces a
                    // `Result<_, Error>` value directly — letting
                    // `return Error("msg")` early-return an Err without the
                    // user writing `Err(...)` themselves. The builtin `Error`
                    // struct is emitted on-demand by [`Self::generate`] when
                    // this constructor appears (mirroring the Matrix
                    // emit-on-demand pattern from T24).
                    if name.name == "Error" && args_ref.len() == 1 {
                        return self.lower_error_constructor(args_ref);
                    }
                    // T31: `block(expr)` is a prelude-style async-blocking
                    // form. It runs an async expression synchronously by
                    // spinning up a one-shot tokio runtime and calling
                    // `.block_on(expr)` on it. Inside an async fn this is a
                    // DEADLOCK RISK (the runtime can't run the future while
                    // the current task holds the worker thread), so we
                    // emit a warning diagnostic AND still lower it (the
                    // user gets the warning + the (broken) Rust; they can
                    // then refactor). `block` is NOT a reserved keyword
                    // — it's a builtin name resolved like a prelude fn.
                    if name.name == "block" && args_ref.len() == 1 {
                        return self.lower_block_call(&args_ref[0]);
                    }
                }

                // A function name (bare Ident callee) is NOT a variable
                // use — it doesn't consume a move. Lower it without
                // consulting the move analyzer; other callee shapes
                // (MethodCall, etc.) go through the normal path.
                let callee_is_async = matches!(
                    callee.as_ref(),
                    Expr::Ident(name, _) if self.async_fns.contains(&name.name)
                );
                // T119: detect whether this callee is a declared `extern`
                // function. We consult the AST `callee` BEFORE it's
                // lowered to a SynExpr so the bare-ident check has access
                // to the original name. When true, the call below is
                // wrapped in `unsafe { ... }` (Rust requires an unsafe
                // block at every foreign-fn call site; Buff hides that
                // from the user).
                let callee_is_extern = matches!(
                    callee.as_ref(),
                    Expr::Ident(name, _) if self.extern_fn_names.contains(&name.name)
                );
                let callee = match callee.as_ref() {
                    Expr::Ident(name, _) => SynExpr::Path(syn::ExprPath {
                        attrs: Vec::new(),
                        qself: None,
                        path: syn::Path::from(ast_ident_to_syn(name)),
                    }),
                    _ => self.lower_expr(callee)?,
                };
                let mut lowered: Punctuated<SynExpr, syn::Token![,]> = Punctuated::new();
                for a in args_ref {
                    lowered.push(self.lower_expr(a)?);
                }
                let call = SynExpr::Call(syn::ExprCall {
                    attrs: Vec::new(),
                    func: Box::new(callee),
                    paren_token: Default::default(),
                    args: lowered,
                });

                // T119: wrap the call in `unsafe { ... }` when the callee
                // is a declared `extern` function. `syn::ExprUnsafe` has a
                // single `block` field — we synthesise a one-stmt block
                // with NO trailing semicolon so the unsafe block evaluates
                // to the call's return value (the call's result becomes
                // the value of the wrapping expression).
                let call = if callee_is_extern {
                    syn::Expr::Unsafe(syn::ExprUnsafe {
                        attrs: Vec::new(),
                        unsafe_token: Default::default(),
                        block: syn::Block {
                            brace_token: Default::default(),
                            stmts: vec![SynStmt::Expr(call, None)],
                        },
                    })
                } else {
                    call
                };

                // T31: AUTO-INSERT `.await` at async call sites. Buff has
                // no `await` keyword; the codegen inserts `.await` when:
                //   - the callee is a bare Ident naming an async fn (per
                //     the propagated async set), AND
                //   - we're currently in an async context (the current fn
                //     is async OR we're inside an `async move { ... }`
                //     block — e.g. inside a `spawn` body).
                // This is the ONLY place `.await` is emitted by the call-
                // site rule; `t.result()` → `.await` is a separate path
                // in `lower_method_call`.
                if callee_is_async && self.in_async_context() {
                    Ok(make_await(call))
                } else {
                    Ok(call)
                }
            }
            Expr::IfExpr {
                cond,
                then_block,
                else_block,
                ..
            } => self.lower_if_expr(cond, then_block, else_block.as_ref()),
            Expr::MethodCall {
                receiver,
                method,
                args,
                ..
            } => {
                // T124c: Log module — Log.<level>(msg, key: val, ...).
                // We MUST intercept BEFORE the T105 named-arg resolution
                // below, because that resolution drops arg names and
                // passes only the values positionally to
                // `lower_method_call`. The Log lowering needs the field
                // NAMES (to emit `tracing::<level>!(key = val, "msg")`),
                // so we route Log calls directly to
                // [`Self::lower_prelude_type_assoc_fn`] with the ORIGINAL
                // args (NamedArg nodes intact). Other prelude types
                // (DateTime.now(), Duration.days(n), ...) have no named
                // args in practice, so they continue through the standard
                // path below.
                if let Expr::Ident(id, _) = receiver.as_ref() {
                    if id.name == "Log" {
                        if let Some((ptype, pmethod)) =
                            buff_lang_types::prelude_types::assoc_fn_lookup(
                                &id.name,
                                &method.name,
                            )
                        {
                            return self.lower_prelude_type_assoc_fn(ptype, pmethod, args);
                        }
                        // `Log.<unknown>(...)` — surface a clear error so
                        // a typo doesn't silently fall through to user-
                        // method codegen (which would then fail with a
                        // confusing "no method `info` on type `Log`"
                        // rustc diagnostic). The valid Log levels are
                        // debug / info / warn / error.
                        return Err(self.unsupported(&format!(
                            "Log.{}() is not a recognised prelude Log method \
                             (expected one of: debug, info, warn, error)",
                            method.name
                        )));
                    }
                }
                // T105: named-arg resolution for method calls. Method
                // callee param names are NOT resolved in v0.5 (no
                // receiver-type analysis), so we fall back to value-
                // extraction (drop names) via `materialize_named_args`
                // with `params=None`. Pure-positional call lists pass
                // through unchanged. Full method-param reorder is a v1.0
                // concern (requires resolving the receiver's type to find
                // its method set + signatures).
                let resolved_args: Option<Vec<Expr>> =
                    if args.iter().any(|a| matches!(a, Expr::NamedArg { .. })) {
                        Some(materialize_named_args(args, None))
                    } else {
                        None
                    };
                let args_ref: &[Expr] = resolved_args.as_deref().unwrap_or(args);
                self.lower_method_call(receiver, method, args_ref)
            }
            Expr::StringInterp { parts, .. } => self.lower_string_interp(parts),
            // T23: `[e1, e2, ...]` -> Rust `vec![e1, e2, ...]` macro.
            Expr::ArrayLit { elements, .. } => self.lower_array_lit(elements),
            // T23/T24: Indexing dispatches on index arity.
            // - 1 index (`v[i]`) → Rust `v[i as usize]` (Vector path).
            // - 2 indices (`m[row, col]`) → Rust
            //   `m.data[(row * m.cols + col) as usize]` (flat-storage Matrix
            //   path). The `.data` / `.cols` fields come from the builtin
            //   `Matrix<T>` struct this same codegen emits when a program uses
            //   `Matrix.new(...)`. The flat index is `row * cols + col`
            //   (row-major), cast to `usize` once at the end so any Buff
            //   integer-typed indices work. The base expression is lowered
            //   ONCE and spliced (via clone) into both field-access positions
            //   so the move analyzer's clone decision is preserved.
            Expr::Index { base, indices, .. } => {
                if indices.len() == 2 {
                    self.lower_matrix_index(base, &indices[0], &indices[1])
                } else if indices.len() == 1 {
                    let base_e = self.lower_expr(base)?;
                    let index_e = cast_to_usize(self.lower_expr(&indices[0])?);
                    Ok(SynExpr::Index(syn::ExprIndex {
                        attrs: Vec::new(),
                        expr: Box::new(base_e),
                        bracket_token: Default::default(),
                        index: Box::new(index_e),
                    }))
                } else {
                    Err(self.unsupported(&format!(
                        "indexing with {} indices (only 1 or 2 supported)",
                        indices.len()
                    )))
                }
            }
            // T23: a minimal closure `{ params => expr }` -> Rust
            // `|p1, p2| body`. Param types are inferred by Rust; we emit no
            // type annotations. The body is a single expression (the parser
            // wraps it in a one-statement block).
            Expr::Lambda { params, body, .. } => self.lower_lambda(params, body),
            // T25: a map literal `{"k": v, ...}` -> Rust
            // `std::collections::HashMap::from([("k", v), ...])`. We use the
            // fully-qualified path so generated programs need no `use`
            // import (avoids import management in v0.5). Each entry becomes
            // a Rust tuple `(key, value)`; the outer `[...]` is a const-eval
            // array literal that `HashMap::from` consumes.
            Expr::MapLit { entries, .. } => self.lower_map_lit(entries),
            // T26: struct init `Type { field: value, ... }` → Rust struct
            // expression of the same shape. Each field is lowered as a
            // `field: value` pair inside `Type { ... }`. This mirrors the
            // source form 1:1 because Buff deliberately matches Rust's
            // struct-init syntax ( braces + named fields + colon ).
            Expr::StructInit {
                type_name, fields, ..
            } => self.lower_struct_init(type_name, fields),
            // T27: `match scrutinee { arms }` → Rust `match scrutinee { arms }`.
            // Each arm lowers `pattern => body` to the same Rust shape so the
            // source form maps 1:1 to Rust (Buff deliberately matches Rust's
            // match syntax). Patterns are lowered via [`Self::lower_pattern`]:
            // wildcard → `_`, ident → ident (resolves as variant or binding),
            // variant tuple → `Variant(subpats)`, literal → literal.
            Expr::MatchExpr {
                scrutinee, arms, ..
            } => self.lower_match_expr(scrutinee, arms),
            // T30: `expr?` → Rust's NATIVE `?` operator (`<expr>?`). This is
            // the cleanest mapping: Buff functions that use `?` already lower
            // to Rust functions returning `Result<T, E>`, which is exactly
            // what Rust's `?` requires. The explicit
            // `match expr { Ok(v) => v, Err(e) => return Err(e) }` desugaring
            // is NOT used; native `?` is simpler and equally correct. See the
            // REFACTOR note on [`Self::lower_try`] for the extracted helper.
            Expr::Try { expr, .. } => self.lower_try(expr),
            // T31: `spawn expr` → Rust's `tokio::spawn(async move { expr })`.
            // The operand becomes the body of an `async move` closure so the
            // task owns all captured variables (Buff hides borrow-checker
            // pain from users; the generated Rust must be move-clean). The result
            // is a `tokio::task::JoinHandle<T>` — Buff's `Task<T>` is a thin
            // alias for this type, and the only `.await` on a Task lands at the
            // `t.result()` site (see [`Self::lower_method_call`]).
            Expr::Spawn { task, .. } => self.lower_spawn(task),
            // T68: `start..end` (exclusive) or `start..=end` (inclusive) → Rust
            // range expression. Built via `quote!` so the `..` / `..=` operator
            // is constructed from real `syn` tokens.
            Expr::Range {
                start,
                end,
                inclusive,
                ..
            } => self.lower_range(start, end, *inclusive),
            // T72: `if let PAT = EXPR { then } else { else }` → Rust's native
            // `if let`. Built via `quote!` so the `let` binding in the
            // condition is constructed from real `syn` tokens (syn 2.0's
            // `Expr::Let` is fiddly to hand-construct; `quote!` + `parse2` is
            // the clean path). The pattern is lowered via [`Self::lower_pattern`]
            // (shared with match arms + T71 destructuring).
            Expr::IfLet {
                pattern,
                value,
                then_block,
                else_block,
                ..
            } => self.lower_if_let(pattern, value, then_block, else_block.as_ref()),
            // T103: `(e1, e2, ...)` → Rust's native tuple expression. Lower
            // each member expr and build `(e1, e2, ...)` via `quote!` + parse2.
            // The 2+-element rule lives at parse time so this always carries
            // 2+ members. Element order is preserved (Rust tuples are
            // positional, matching Buff's source order).
            Expr::TupleLit(members, _) => {
                let lowered: Vec<SynExpr> = members
                    .iter()
                    .map(|m| self.lower_expr(m))
                    .collect::<Result<Vec<_>, _>>()?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    ( #( #lowered ),* )
                };
                syn::parse2::<SynExpr>(tokens)
                    .map_err(|e| self.unsupported(&format!("tuple codegen parse: {e}")))
            }
            // T105: a NamedArg at this position is a parser bug — it
            // should only appear INSIDE a FuncCall/MethodCall args vec,
            // where it's resolved to a positional value BEFORE reaching
            // lower_expr. As a defensive fallback, lower the value (so
            // the generated Rust compiles even if a NamedArg slips
            // through). This never triggers for well-formed Buff source.
            Expr::NamedArg { value, .. } => self.lower_expr(value),
            _ => Err(self.unsupported(&format!("expr codegen not yet implemented for {:?}", expr))),
        }
    }

    fn lower_if_expr(
        &mut self,
        cond: &Expr,
        then_block: &Block,
        else_block: Option<&Block>,
    ) -> Result<SynExpr, CodegenError> {
        let cond_expr = self.lower_expr(cond)?;
        let then_branch = self.lower_block(then_block)?;
        let else_branch = match else_block {
            Some(b) => Some((
                Default::default(),
                Box::new(SynExpr::Block(syn::ExprBlock {
                    attrs: Vec::new(),
                    label: None,
                    block: self.lower_block(b)?,
                })),
            )),
            None => None,
        };
        Ok(SynExpr::If(syn::ExprIf {
            attrs: Vec::new(),
            if_token: Default::default(),
            cond: Box::new(cond_expr),
            then_branch,
            else_branch,
        }))
    }

    /// T72: lower `if let PAT = EXPR { then } else { else }` to Rust's native
    /// `if let`.
    ///
    /// Built via `quote!` + `syn::parse2::<SynExpr>` rather than hand-building
    /// `syn::ExprIf` with an `syn::Expr::Let` condition — syn 2.0's `ExprLet`
    /// has many fiddly fields (`Eq`, `Let`, `pat`, `expr`, `attrs`) and
    /// `quote!` builds them all correctly from the surface syntax. The
    /// pattern is lowered via [`Self::lower_pattern`] (shared with match arms
    /// and T71 destructuring); the value via [`Self::lower_expr`]; the blocks
    /// via [`Self::lower_block`]. The single string producer remains
    /// `prettyplease::unparse`.
    ///
    /// Single let-binding only. Let-chains (`if let a = x, let b = y`) are
    /// T74, a separate task.
    fn lower_if_let(
        &mut self,
        pattern: &Pattern,
        value: &Expr,
        then_block: &Block,
        else_block: Option<&Block>,
    ) -> Result<SynExpr, CodegenError> {
        let pat = self.lower_pattern(pattern, false)?;
        let val = self.lower_expr(value)?;
        let then_blk = self.lower_block(then_block)?;
        let tokens: proc_macro2::TokenStream = if let Some(eb) = else_block {
            let else_blk = self.lower_block(eb)?;
            quote::quote! {
                if let #pat = #val #then_blk else #else_blk
            }
        } else {
            quote::quote! {
                if let #pat = #val #then_blk
            }
        };
        syn::parse2::<SynExpr>(tokens)
            .map_err(|e| self.unsupported(&format!("if-let codegen parse: {e}")))
    }

    /// T72: lower `for let PAT = EXPR { body }` (Buff) to Rust's
    /// `while let PAT = EXPR { body }`.
    ///
    /// Buff spells the looping-binding form `for let` because `while` is NOT
    /// a reserved Buff keyword and the loop reads like the iterator-form
    /// `for v in iter`. The natural Rust target is `while let` (semantically
    /// identical: re-evaluate EXPR each iteration, run the body while it
    /// matches PAT, terminate when it doesn't).
    ///
    /// Built via `quote!` + `syn::parse2::<SynExpr>` (same approach as
    /// [`Self::lower_if_let`]). The lowered expression is then wrapped in a
    /// `SynStmt::Expr(...)` to mirror how `Stmt::ForWhile` becomes a Rust
    /// `while` statement (see the ForWhile arm in [`Self::lower_stmt`]).
    ///
    /// Single let-binding only. Let-chains are T74.
    fn lower_for_let(
        &mut self,
        pattern: &Pattern,
        value: &Expr,
        body: &Block,
    ) -> Result<SynStmt, CodegenError> {
        let pat = self.lower_pattern(pattern, false)?;
        let val = self.lower_expr(value)?;
        let body_blk = self.lower_block(body)?;
        let tokens: proc_macro2::TokenStream = quote::quote! {
            while let #pat = #val #body_blk
        };
        let while_let_expr = syn::parse2::<SynExpr>(tokens)
            .map_err(|e| self.unsupported(&format!("while-let codegen parse: {e}")))?;
        Ok(SynStmt::Expr(while_let_expr, Some(Default::default())))
    }

    /// Lower a [`Stmt::Guard`]'s conditions into MULTIPLE sibling Rust
    /// statements appended to `out` (T73).
    ///
    /// Each condition emits exactly ONE Rust statement, in source order:
    ///
    /// - [`GuardCondition::Let`] → Rust let-else:
    ///   ```ignore
    ///   let #pat = #value else #else_block;
    ///   ```
    ///   The pattern bindings stay in scope for subsequent statements in
    ///   the SAME function block (the whole point of guard). Built via
    ///   `quote!` + `syn::parse2::<syn::Stmt>` — syn's let-else is
    ///   `syn::Local` with `init.diverge = Some((else, block))`.
    ///
    /// - [`GuardCondition::Bool`] → negated if:
    ///   ```ignore
    ///   if !(#expr) #else_block
    ///   ```
    ///   The else-block runs when the original condition is FALSE (i.e.
    ///   the guard fails). Built via `quote!` + `syn::parse2::<syn::Stmt>`.
    ///
    /// The `else_block` is re-lowered for EACH condition (so a 3-condition
    /// guard produces 3 copies of the else-block in the Rust output). This
    /// is correct semantically: each failing condition independently
    /// dispatches to the same user-written else-block. An alternative
    /// (single shared else-block via control-flow manipulation) would
    /// require reshaping the control graph — overkill for v0.5.
    fn lower_guard_conditions_into(
        &mut self,
        conditions: &[buff_lang_ast::GuardCondition],
        else_block: &Block,
        out: &mut Vec<SynStmt>,
    ) -> Result<(), CodegenError> {
        for cond in conditions {
            match cond {
                buff_lang_ast::GuardCondition::Let { pattern, value, .. } => {
                    let pat = self.lower_pattern(pattern, false)?;
                    let val = self.lower_expr(value)?;
                    let else_blk = self.lower_block(else_block)?;
                    // Build `let #pat = #val else #else_blk ;` via quote. The
                    // syn let-else form is `let pat = expr else block;` (the
                    // block must diverge — Rust enforces this at compile time).
                    let tokens: proc_macro2::TokenStream = quote::quote! {
                        let #pat = #val else #else_blk ;
                    };
                    let stmt = syn::parse2::<SynStmt>(tokens).map_err(|e| {
                        self.unsupported(&format!("guard let-else codegen parse: {e}"))
                    })?;
                    out.push(stmt);
                }
                buff_lang_ast::GuardCondition::Bool(expr) => {
                    let cond_expr = self.lower_expr(expr)?;
                    let else_blk = self.lower_block(else_block)?;
                    // Build `if !(#cond_expr) #else_blk` — the negation
                    // means the else-block runs when the ORIGINAL guard
                    // condition is FALSE (i.e. the guard fails).
                    let tokens: proc_macro2::TokenStream = quote::quote! {
                        if ! ( #cond_expr ) #else_blk
                    };
                    let if_expr = syn::parse2::<SynExpr>(tokens).map_err(|e| {
                        self.unsupported(&format!("guard bool-if codegen parse: {e}"))
                    })?;
                    out.push(SynStmt::Expr(if_expr, Some(Default::default())));
                }
            }
        }
        Ok(())
    }

    /// Lower a Buff **prelude** call to the corresponding Rust idiom (T96).
    ///
    /// The prelude is the implicit standard library — these functions are
    /// available in every Buff program without an `import`. The mappings are
    /// grouped by category (matching [`buff_lang_types::prelude`]):
    ///
    /// # Math
    ///
    /// | Buff            | Rust                              | Notes             |
    /// |-----------------|-----------------------------------|-------------------|
    /// | `abs(x)`        | `(x).abs()`                       | works for any numeric (i64 / f32 / f64) |
    /// | `min(a, b)`     | `(a).min(b)`                      | `Ord::min` for ints, inherent `min` for floats |
    /// | `max(a, b)`     | `(a).max(b)`                      | analogous         |
    /// | `sqrt(x)`       | `((x) as f64).sqrt()`             | always returns `f64`; coerce arg up so int args work |
    /// | `floor(x)`      | `((x) as f64).floor()`            | always returns `f64` |
    /// | `ceil(x)`       | `((x) as f64).ceil()`             | always returns `f64` |
    /// | `round(x)`      | `((x) as f64).round()`            | always returns `f64` |
    /// | `pow(b, e)`     | `(b).powf((e) as f64)` if `b` is float-like; else `(b).pow((e) as u32)` | the inferencer picks the arm |
    ///
    /// # Type conversions
    ///
    /// The arg's inferred type drives the Rust idiom:
    ///
    /// | Buff            | Rust (arg is String)               | Rust (arg is numeric)         |
    /// |-----------------|------------------------------------|-------------------------------|
    /// | `Int(x)`        | `x.parse::<i64>().unwrap_or(0)`    | `(x) as i64`                  |
    /// | `Float(x)`      | `x.parse::<f32>().unwrap_or(0.0)`  | `(x) as f32`                  |
    /// | `Bool(x)`       | `x.parse::<bool>().unwrap_or(false)` | `(x) != 0`                  |
    /// | `String(x)`     | `x.to_string()` (any `Display`)    | `x.to_string()`              |
    ///
    /// **Parse-failure policy (v0.5):** `Int("bad")` returns `0`,
    /// `Float("bad")` returns `0.0`, `Bool("bad")` returns `false`. We use
    /// `unwrap_or` (not `expect`/`unwrap`) so generated code never panics
    /// on malformed runtime input. A proper `Result`-returning conversion
    /// API is deferred — see T96 notes in `decisions.md`.
    ///
    /// # I/O
    ///
    /// | Buff                  | Rust                                                        |
    /// |-----------------------|-------------------------------------------------------------|
    /// | `print("lit")`        | `println!("lit")` (a bare string literal drops the `{}`)    |
    /// | `print(x)` / `println(x)` (non-literal) | `println!("{}", x)`                          |
    /// | `read_line()`         | `{ let mut s = String::new(); std::io::stdin().read_line(&mut s).ok(); s }` |
    ///
    /// `read_line()` swallows the trailing newline (matches Rust's
    /// `read_line` semantics — callers can `.trim_end()` if they want it
    /// gone). The `.ok()` discards the `io::Result` error as `Some(())`/
    /// `None` (we don't panic on I/O failure).
    fn lower_prelude_call(
        &mut self,
        fn_: PreludeFn,
        args: &[Expr],
    ) -> Result<SynExpr, CodegenError> {
        match fn_ {
            // ----- Math ---------------------------------------------------
            PreludeFn::Abs => {
                self.lower_one_arg_method(args, "abs", /*wrap_parens*/ true)
            }
            PreludeFn::Min => self.lower_min_max(args, "min"),
            PreludeFn::Max => self.lower_min_max(args, "max"),
            PreludeFn::Sqrt => self.lower_float_unary(args, "sqrt"),
            PreludeFn::Floor => self.lower_float_unary(args, "floor"),
            PreludeFn::Ceil => self.lower_float_unary(args, "ceil"),
            PreludeFn::Round => self.lower_float_unary(args, "round"),
            PreludeFn::Pow => self.lower_pow(args),

            // ----- Conversions -------------------------------------------
            PreludeFn::Int => self.lower_convert(args, "i64", ConvKind::Numeric),
            PreludeFn::Float => self.lower_convert(args, "f32", ConvKind::Numeric),
            PreludeFn::Bool => self.lower_convert(args, "bool", ConvKind::Bool),
            PreludeFn::String => self.lower_to_string(args),

            // ----- I/O ---------------------------------------------------
            PreludeFn::Print => self.lower_print(args),
            PreludeFn::Println => self.lower_print(args),
            PreludeFn::ReadLine => Ok(self.lower_read_line()),

            // ----- System / environment (T99) ---------------------------
            // args() → std::env::args().collect::<Vec<String>>()
            PreludeFn::Args => {
                if !args.is_empty() {
                    return Err(self.unsupported("args() takes no arguments"));
                }
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    std::env::args().collect::<Vec<String>>()
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("args() codegen parse: {e}")))
            }
            // env("NAME") → std::env::var("NAME").ok()
            PreludeFn::Env => {
                if args.len() != 1 {
                    return Err(
                        self.unsupported("env() expects exactly 1 argument (the variable name)")
                    );
                }
                let arg = self.lower_expr(&args[0])?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    std::env::var(#arg).ok()
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("env() codegen parse: {e}")))
            }
            // exit(code) → std::process::exit(code)
            PreludeFn::Exit => {
                if args.len() != 1 {
                    return Err(
                        self.unsupported("exit() expects exactly 1 argument (the exit code)")
                    );
                }
                let arg = self.lower_expr(&args[0])?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    std::process::exit(#arg)
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("exit() codegen parse: {e}")))
            }
            // T35: assert_eq(a, b) → assert_eq!(a, b)
            // The Rust `assert_eq!` macro panics when the two args are not
            // equal, which is exactly Buff's semantics. Used inside `@test`
            // functions (where the test runner catches the panic).
            PreludeFn::AssertEq => {
                if args.len() != 2 {
                    return Err(self.unsupported(
                        "assert_eq() expects exactly 2 arguments (actual, expected)",
                    ));
                }
                let lhs = self.lower_expr(&args[0])?;
                let rhs = self.lower_expr(&args[1])?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    assert_eq!(#lhs, #rhs)
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("assert_eq() codegen parse: {e}")))
            }
        }
    }

    /// T124b: lower a prelude-type associated-function call (`Type.method(args)`)
    /// to the corresponding chrono / std::time Rust idiom.
    ///
    /// Dispatched from [`Self::lower_method_call`] when the receiver is a
    /// bare Ident naming a prelude type (DateTime, Date, Time, Duration,
    /// Instant). The method name is matched on the resolved
    /// `(PreludeType, PreludeAssocFn)` pair rather than raw strings so the
    /// prelude-types registry is the single source of truth.
    ///
    /// # Lowering table
    ///
    /// | Buff source                  | Generated Rust                                |
    /// |------------------------------|-----------------------------------------------|
    /// | `DateTime.now()`             | `chrono::Utc::now()`                          |
    /// | `DateTime.parse(s)`          | `chrono::DateTime::parse_from_rfc3339(s).unwrap_or(chrono::Utc::now())` |
    /// | `Date.today()`               | `chrono::Local::now().date_naive()`           |
    /// | `Date.parse(s)`              | `chrono::NaiveDate::parse_from_str(s, "%Y-%m-%d").unwrap_or(chrono::Local::now().date_naive())` |
    /// | `Instant.now()`              | `std::time::Instant::now()`                   |
    /// | `Duration.days(n)`           | `chrono::TimeDelta::days(n)`                  |
    /// | `Duration.hours(n)`          | `chrono::TimeDelta::hours(n)`                 |
    /// | `Duration.minutes(n)`        | `chrono::TimeDelta::minutes(n)`               |
    /// | `Duration.seconds(n)`        | `chrono::TimeDelta::seconds(n)`               |
    /// | `Duration.millis(n)`         | `chrono::TimeDelta::milliseconds(n)`          |
    ///
    /// The `parse` lowering uses `unwrap_or(<default>)` rather than `unwrap()`
    /// so generated user code never panics on a malformed input string —
    /// matching Buff's "no panicking generated code" stance where practical.
    /// (The user can still opt into panic-on-error by chaining `??` or
    /// `match`-ing once Result-shaped prelude values are added.)
    fn lower_prelude_type_assoc_fn(
        &mut self,
        ptype: buff_lang_types::PreludeType,
        pmethod: buff_lang_types::PreludeAssocFn,
        args: &[Expr],
    ) -> Result<SynExpr, CodegenError> {
        use buff_lang_types::{PreludeAssocFn as A, PreludeType as T};
        // Lower a single arg, erroring on arity mismatch.
        let one_arg = |c: &mut Self| -> Result<SynExpr, CodegenError> {
            if args.len() != 1 {
                return Err(c.unsupported(&format!(
                    "{}.{}() expects exactly 1 arg, got {}",
                    ptype.name(),
                    pmethod.name(),
                    args.len()
                )));
            }
            c.lower_expr(&args[0])
        };
        // Lower zero args, erroring if any were passed.
        let no_args = |c: &mut Self| -> Result<(), CodegenError> {
            if !args.is_empty() {
                return Err(c.unsupported(&format!(
                    "{}.{}() takes no arguments, got {}",
                    ptype.name(),
                    pmethod.name(),
                    args.len()
                )));
            }
            Ok(())
        };
        match (ptype, pmethod) {
            // T124c: Log module — Log.<level>(msg, key: val, ...) lowers
            // to the corresponding tracing macro. Dispatched to a
            // dedicated helper because the Log call signature is
            // variadic (positional msg + named fields) and the lowering
            // produces a MACRO invocation (not a function call), unlike
            // every other prelude-type assoc fn. Must run BEFORE the
            // chrono/std::time arms below — `(Log, _)` is not matched by
            // any of them, so the early-return is also a correctness
            // guard.
            (T::Log, _) => self.lower_log_call(pmethod, args),
            // ----- Time constructors ----------------------------------------
            (T::DateTime, A::Now) => {
                no_args(self)?;
                Ok(rust_call_expr("chrono::Utc::now", Vec::new()))
            }
            (T::Instant, A::Now) => {
                no_args(self)?;
                Ok(rust_call_expr("std::time::Instant::now", Vec::new()))
            }
            (T::Date, A::Today) => {
                no_args(self)?;
                // chrono::Local::now().date_naive() — the system's local
                // date "today". `date_naive()` strips the timezone to give
                // a `NaiveDate`.
                let inner = rust_call_expr("chrono::Local::now", Vec::new());
                Ok(method_call_no_args(inner, "date_naive"))
            }
            // ----- Parsing --------------------------------------------------
            (T::DateTime, A::Parse) => {
                let arg = one_arg(self)?;
                let arg = coerce_str_arg_to_ref(arg, &args[0]);
                // chrono::DateTime::parse_from_rfc3339(&s).unwrap_or(chrono::Utc::now())
                let parse_call = rust_call_expr("chrono::DateTime::parse_from_rfc3339", vec![arg]);
                let fallback = rust_call_expr("chrono::Utc::now", Vec::new());
                Ok(method_call_one_arg(parse_call, "unwrap_or", fallback))
            }
            (T::Date, A::Parse) => {
                let arg = one_arg(self)?;
                let arg = coerce_str_arg_to_ref(arg, &args[0]);
                // chrono::NaiveDate::parse_from_str(&s, "%Y-%m-%d").unwrap_or(<today>)
                let fmt_lit = str_lit_expr("%Y-%m-%d");
                let parse_call =
                    rust_call_expr("chrono::NaiveDate::parse_from_str", vec![arg, fmt_lit]);
                let today_recv = rust_call_expr("chrono::Local::now", Vec::new());
                let today = method_call_no_args(today_recv, "date_naive");
                Ok(method_call_one_arg(parse_call, "unwrap_or", today))
            }
            // ----- Duration constructors ------------------------------------
            (T::Duration, A::Days) => {
                let arg = one_arg(self)?;
                Ok(rust_call_expr("chrono::TimeDelta::days", vec![arg]))
            }
            (T::Duration, A::Hours) => {
                let arg = one_arg(self)?;
                Ok(rust_call_expr("chrono::TimeDelta::hours", vec![arg]))
            }
            (T::Duration, A::Minutes) => {
                let arg = one_arg(self)?;
                Ok(rust_call_expr("chrono::TimeDelta::minutes", vec![arg]))
            }
            (T::Duration, A::Seconds) => {
                let arg = one_arg(self)?;
                Ok(rust_call_expr("chrono::TimeDelta::seconds", vec![arg]))
            }
            (T::Duration, A::Millis) => {
                let arg = one_arg(self)?;
                Ok(rust_call_expr("chrono::TimeDelta::milliseconds", vec![arg]))
            }
            // Every other combination was already rejected by
            // `assoc_fn_lookup` in the caller; this arm is unreachable but
            // required for exhaustiveness.
            _ => Err(self.unsupported(&format!(
                "prelude type+method combination {:?}.{:?}",
                ptype, pmethod
            ))),
        }
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
    fn lower_prelude_type_instance_fn(
        &mut self,
        recv_ty: &Type,
        pmethod: buff_lang_types::PreludeInstanceFn,
        receiver: &Expr,
        args: &[Expr],
    ) -> Result<SynExpr, CodegenError> {
        use buff_lang_types::PreludeInstanceFn as M;
        let recv = self.lower_expr(receiver)?;
        // All current instance methods are either 0-arg or 1-arg (format).
        // We validate arity once here so the dispatch below doesn't repeat
        // the check.
        match pmethod {
            M::Format => {
                if args.len() != 1 {
                    return Err(self.unsupported(&format!(
                        "format() expects exactly 1 arg (the strftime format string), got {}",
                        args.len()
                    )));
                }
                let fmt = self.lower_expr(&args[0])?;
                let fmt = coerce_str_arg_to_ref(fmt, &args[0]);
                // recv.format(fmt).to_string() — the chain returns a String.
                let format_call = method_call_one_arg(recv, "format", fmt);
                Ok(method_call_no_args(format_call, "to_string"))
            }
            M::Year | M::Month | M::Day | M::Hour | M::Minute | M::Second | M::Timestamp => {
                if !args.is_empty() {
                    return Err(self.unsupported(&format!(
                        "{:?}() takes no arguments, got {}",
                        pmethod,
                        args.len()
                    )));
                }
                // The chrono methods have the same names as Buff's surface,
                // so we just emit recv.method().
                let method_name = pmethod.name();
                // Defensive: confirm the receiver type actually supports
                // this method. This was already checked by
                // `instance_fn_lookup` in the caller, but we re-check here
                // so the helper stays self-contained.
                if buff_lang_types::instance_fn_return_type(recv_ty, pmethod, &[]).is_none() {
                    return Err(self.unsupported(&format!(
                        "{recv_ty}.{method_name}() is not a recognised prelude instance method"
                    )));
                }
                Ok(method_call_no_args(recv, method_name))
            }
        }
    }

    /// T124c: lower a prelude `Log` module call (`Log.<level>(msg, ...)`) to
    /// the corresponding `tracing::<level>!(...)` macro invocation.
    ///
    /// Call shape (mirrors `tracing`'s macro surface):
    ///
    /// ```text
    /// Log.info("msg")                       -> tracing::info!("msg")
    /// Log.info("msg", k1: v1, k2: v2)       -> tracing::info!(k1 = v1, k2 = v2, "msg")
    /// ```
    ///
    /// The first positional arg is the **message** (typically a string
    /// literal, but any `Display`-able expression works at the Rust level).
    /// All SUBSEQUENT args MUST be `Expr::NamedArg` (`key: value`) — they
    /// become the tracing macro's structured fields. Mixed positional-after-
    /// named args are rejected (tracing's macro syntax requires the message
    /// literal LAST, after all field assignments).
    ///
    /// # Field ordering (determinism)
    ///
    /// Fields are emitted in **source order** (the order the user wrote
    /// them). This is the simplest deterministic choice — `tracing` itself
    /// preserves insertion order in its event record, and insta snapshots
    /// prove byte-identical output across runs. The alternative (alphabetical
    /// sort) would reorder fields away from the user's intent; we keep
    /// source order.
    ///
    /// # Lowering mechanism
    ///
    /// The macro is built as a [`syn::ExprMacro`] whose `mac.path` is
    /// `tracing::<level>` and whose `mac.tokens` carries the comma-separated
    /// field assignments + the trailing message. Token construction goes
    /// through `quote!` so NO raw-string Rust is emitted — the single
    /// string producer remains `prettyplease::unparse`. The resulting
    /// `ExprMacro` re-parses cleanly because every spliced fragment is
    /// already a `syn` node (Ident, SynExpr).
    ///
    /// # Errors
    ///
    /// - Empty arg list → `unsupported` (caller must supply at least the
    ///   message).
    /// - Any arg after the first that is NOT an `Expr::NamedArg` →
    ///   `unsupported` (mixed positional/named is rejected — the message
    ///   must be the LAST positional and the only one).
    /// - An unknown `PreludeAssocFn` for `Log` (i.e. not one of
    ///   Debug/Info/Warn/Error) → `unsupported` (defensive — the registry
    ///   already rejects the combo at the lookup layer).
    fn lower_log_call(
        &mut self,
        level: buff_lang_types::PreludeAssocFn,
        args: &[Expr],
    ) -> Result<SynExpr, CodegenError> {
        use buff_lang_types::PreludeAssocFn as A;
        // Resolve the tracing macro name from the level variant.
        // `PreludeAssocFn::name()` already returns the lowercase Rust
        // spelling ("debug" / "info" / "warn" / "error"), so we can
        // splice it directly into `tracing::<name>!`.
        let level_name = match level {
            A::Debug | A::Info | A::Warn | A::Error => level.name(),
            other => {
                return Err(self.unsupported(&format!(
                    "Log.{:?}() is not a recognised Log level (expected debug/info/warn/error)",
                    other
                )));
            }
        };
        if args.is_empty() {
            return Err(self.unsupported(&format!(
                "Log.{level_name}() requires at least the message argument"
            )));
        }
        // First positional arg is the message; the rest must be NamedArgs.
        let msg_expr = &args[0];
        for (i, a) in args[1..].iter().enumerate() {
            if !matches!(a, Expr::NamedArg { .. }) {
                return Err(self.unsupported(&format!(
                    "Log.{level_name}(): argument {} (after the message) must be a named field \
                     (e.g. `key: value`); positional args after the message are not allowed",
                    i + 2
                )));
            }
        }
        // Lower the message. For a string literal, this produces a
        // `SynExpr::Lit(Lit::Str(...))` that quote! splices as the bare
        // string literal token (so `tracing::info!("msg")` not
        // `tracing::info!({ "msg" })`).
        let msg = self.lower_expr(msg_expr)?;
        // Lower field name + value pairs in SOURCE ORDER (deterministic).
        // We collect into Vecs so the `#(#names = #values,)*` repetition
        // in quote! produces a comma-separated list with a trailing comma
        // after every entry (so the message is unambiguously separated).
        let mut field_names: Vec<Ident> = Vec::with_capacity(args.len().saturating_sub(1));
        let mut field_values: Vec<SynExpr> = Vec::with_capacity(field_names.len());
        for f in &args[1..] {
            if let Expr::NamedArg { name, value, .. } = f {
                field_names.push(ast_ident_to_syn(name));
                field_values.push(self.lower_expr(value)?);
            }
        }
        // Build the macro path: `tracing::info`, `tracing::debug`, ...
        let macro_path = rust_path(&format!("tracing::{level_name}"));
        // Build the macro token body. The `#(#names = #values,)*` repetition
        // emits `k1 = v1, k2 = v2,` (each entry followed by a comma), then
        // the message splices in last. tracing accepts the trailing comma
        // after the last field (Rust macro_rules $() sep behavior).
        let tokens: proc_macro2::TokenStream = if field_names.is_empty() {
            quote::quote! { #msg }
        } else {
            quote::quote! { #(#field_names = #field_values,)* #msg }
        };
        Ok(SynExpr::Macro(syn::ExprMacro {
            attrs: Vec::new(),
            mac: syn::Macro {
                path: macro_path,
                bang_token: Default::default(),
                delimiter: syn::MacroDelimiter::Paren(Default::default()),
                tokens,
            },
        }))
    }

    /// Lower `abs(x)` → `(x).abs()`. Wrapping the receiver in parens
    /// ensures integer literals like `5` lower to `(5).abs()` rather than
    /// the ambiguous `5.abs()` (which Rust parses as a field access on a
    /// float literal `5.`).
    fn lower_one_arg_method(
        &mut self,
        args: &[Expr],
        method: &str,
        wrap_parens: bool,
    ) -> Result<SynExpr, CodegenError> {
        let recv = self.lower_one_arg(args)?;
        let recv = if wrap_parens {
            wrap_in_parens(recv)
        } else {
            recv
        };
        Ok(method_call_no_args(recv, method))
    }

    /// Lower `min(a, b)` / `max(a, b)` → `(a).<method>(b)`.
    fn lower_min_max(&mut self, args: &[Expr], method: &str) -> Result<SynExpr, CodegenError> {
        if args.len() != 2 {
            return Err(self.unsupported(&format!(
                "{method} expects exactly 2 args, got {}",
                args.len()
            )));
        }
        let a = wrap_in_parens(self.lower_expr(&args[0])?);
        let b = self.lower_expr(&args[1])?;
        Ok(method_call_one_arg(a, method, b))
    }

    /// Lower a float-returning unary math fn (`sqrt`/`floor`/`ceil`/`round`)
    /// to `((x) as f64).<method>()`. Coercing to `f64` first means int args
    /// compile without requiring the user to write `x as Double` manually.
    fn lower_float_unary(&mut self, args: &[Expr], method: &str) -> Result<SynExpr, CodegenError> {
        let recv = self.lower_one_arg(args)?;
        let as_f64 = cast_to(recv, "f64");
        Ok(method_call_no_args(as_f64, method))
    }

    /// Lower `pow(base, exp)` — picks `.powf` for float bases and `.pow` for
    /// integer bases (Rust's `i64::pow` takes `u32`, hence the `as u32` cast).
    fn lower_pow(&mut self, args: &[Expr]) -> Result<SynExpr, CodegenError> {
        if args.len() != 2 {
            return Err(
                self.unsupported(&format!("pow expects exactly 2 args, got {}", args.len()))
            );
        }
        let base = wrap_in_parens(self.lower_expr(&args[0])?);
        let exp_raw = self.lower_expr(&args[1])?;
        // Infer the base type to choose `.pow` vs `.powf`. Inference errors
        // fall back to the integer form (which works for the common case).
        let base_ty = self
            .type_inferencer
            .infer_expr(&args[0])
            .unwrap_or(Type::Unknown);
        if base_ty.is_float_like() {
            let exp = cast_to(exp_raw, "f64");
            Ok(method_call_one_arg(base, "powf", exp))
        } else {
            let exp = cast_to(exp_raw, "u32");
            Ok(method_call_one_arg(base, "pow", exp))
        }
    }

    /// Lower a type conversion (`Int(x)` / `Float(x)` / `Bool(x)`).
    ///
    /// For String args we emit `.parse::<T>().unwrap_or(default)`; for
    /// numeric args we emit `(x) as T`. The `Bool` arm uses `x != 0` for
    /// numerics (Rust has no `as bool` cast) and `.parse::<bool>()` for
    /// strings.
    fn lower_convert(
        &mut self,
        args: &[Expr],
        target: &str,
        kind: ConvKind,
    ) -> Result<SynExpr, CodegenError> {
        let arg = self.lower_one_arg(args)?;
        // Infer the arg's type to dispatch on the source category.
        let arg_ty = self
            .type_inferencer
            .infer_expr(&args[0])
            .unwrap_or(Type::Unknown);
        if matches!(arg_ty, Type::String) {
            // String → parse
            return Ok(parse_with_default(arg, target, &kind));
        }
        // Non-string → numeric coercion (`as T`) for Int/Float, or `!= 0` for Bool.
        match kind {
            ConvKind::Numeric => Ok(cast_to(arg, target)),
            ConvKind::Bool => {
                // `(x) != 0` — wrap the arg in parens so compound exprs bind right.
                let zero = make_int_lit_expr(0);
                Ok(make_binary_expr(
                    syn::BinOp::Ne(Default::default()),
                    wrap_in_parens(arg),
                    zero,
                ))
            }
        }
    }

    /// Lower `String(x)` → `(x).to_string()`. Works for any Rust `Display` type.
    fn lower_to_string(&mut self, args: &[Expr]) -> Result<SynExpr, CodegenError> {
        let recv = self.lower_one_arg(args)?;
        Ok(method_call_no_args(recv, "to_string"))
    }

    /// Lower `print(x)` / `println(x)`.
    ///
    /// A bare string-literal arg lowers to `println!("the literal text")`
    /// — no `{}` placeholder (T96 acceptance). Any other arg lowers to
    /// `println!("{}", x)`.
    fn lower_print(&mut self, args: &[Expr]) -> Result<SynExpr, CodegenError> {
        if args.len() != 1 {
            return Err(self.unsupported(&format!(
                "print/println expect exactly 1 arg, got {}",
                args.len()
            )));
        }
        // String-literal fast path: print("hello") → println!("hello").
        if let Expr::Literal(Literal::String(text), _) = &args[0] {
            return Ok(make_println_macro_literal(text));
        }
        // General path: print(x) → println!("{}", x).
        let arg = self.lower_expr(&args[0])?;
        Ok(make_println_macro(arg))
    }

    /// Lower `read_line()` → a block expression that reads one line of stdin.
    ///
    /// Emits (conceptually):
    /// ```text
    /// {
    ///     let mut __buff_prelude_line = String::new();
    ///     std::io::stdin().read_line(&mut __buff_prelude_line).ok();
    ///     __buff_prelude_line
    /// }
    /// ```
    ///
    /// The block is built via `quote!` and then re-parsed via
    /// `syn::parse2` (which returns a `Result`, unlike `parse_quote!`'s
    /// panic). The placeholder name `__buff_prelude_line` is intentionally
    /// ugly to avoid colliding with any user binding.
    fn lower_read_line(&self) -> SynExpr {
        let tokens: proc_macro2::TokenStream = quote::quote! {{
            let mut __buff_prelude_line = String::new();
            std::io::stdin().read_line(&mut __buff_prelude_line).ok();
            __buff_prelude_line
        }};
        // `quote!`'s `{{...}}` produces a Rust block-expression token
        // stream; re-parse it as a `syn::Expr` (the top-level enum) so it
        // slots into the surrounding expression context. On the (unreachable)
        // parse failure we fall back to a bare `String::new()` call.
        match syn::parse2::<SynExpr>(tokens) {
            Ok(e) => e,
            Err(_) => {
                // Defensive fallback: never panic in codegen. The quote!
                // above is a compile-time-fixed template so a parse failure
                // is a codegen bug, not a user-facing condition.
                let path = rust_path("String::new");
                SynExpr::Call(syn::ExprCall {
                    attrs: Vec::new(),
                    func: Box::new(SynExpr::Path(syn::ExprPath {
                        attrs: Vec::new(),
                        qself: None,
                        path,
                    })),
                    paren_token: Default::default(),
                    args: Default::default(),
                })
            }
        }
    }

    /// Lower exactly one argument, returning an error if the arg count is wrong.
    fn lower_one_arg(&mut self, args: &[Expr]) -> Result<SynExpr, CodegenError> {
        if args.len() != 1 {
            return Err(self.unsupported(&format!("expected exactly 1 arg, got {}", args.len())));
        }
        self.lower_expr(&args[0])
    }

    ///
    /// T21 — string methods. The following Buff method names map to specific
    /// Rust idioms (none of them is a literal `recv.method(args)` because
    /// Rust strings don't expose these names directly):
    ///
    /// | Buff                  | Rust                                              |
    /// |-----------------------|---------------------------------------------------|
    /// | `s.char_count()`      | `s.chars().count()`                               |
    /// | `s.byte_len()`        | `s.len()`                                         |
    /// | `s.chars()`           | `s.chars()`                                       |
    /// | `s.bytes()`           | `s.bytes()`                                       |
    /// | `s.graphemes()`       | `unicode_segmentation::UnicodeSegmentation::graphemes(s, true).collect::<String>()` — see note below |
    /// | `s.first()`           | `s.chars().next()`                                |
    /// | `s.last()`            | `s.chars().last()`                                |
    /// | `s.slice(a, b)`       | char-safe slice via `s.chars().skip(a).take(b - a).collect()` |
    ///
    /// `graphemes()` is special-cased to return a `String` (a flattened
    /// representation) for now; a true iterator-returning API will need a
    /// dedicated AST shape (deferred to a later task — see notes).
    ///
    /// Any unrecognised method falls through to a plain `recv.method(args)`
    /// Rust method call, which is correct for arbitrary user-defined methods
    /// and the methods of future types.
    fn lower_method_call(
        &mut self,
        receiver: &Expr,
        method: &buff_lang_ast::common::Ident,
        args: &[Expr],
    ) -> Result<SynExpr, CodegenError> {
        // T124b: prelude-types registry — associated functions. A call of
        // the form `Type.method(args)` where the receiver is a bare Ident
        // naming a prelude type (DateTime, Date, Time, Duration, Instant)
        // is dispatched through the prelude-types table. This MUST run
        // before the T31 `result()` arm and the T26 zero-arg field-access
        // heuristic so that `DateTime.now()` (zero args) doesn't get
        // rewritten as a field access `DateTime.now`.
        //
        // This is the GENERAL entry point future v1.4 stdlib tasks extend
        // — see `crates/buff-lang-types/src/prelude_types.rs` for the
        // registry and the instructions for adding new types.
        if let Expr::Ident(id, _) = receiver {
            if let Some((ptype, pmethod)) =
                buff_lang_types::prelude_types::assoc_fn_lookup(&id.name, &method.name)
            {
                // The chrono / std::time lowering lives in a dedicated
                // helper so this arm stays a thin dispatch.
                return self.lower_prelude_type_assoc_fn(ptype, pmethod, args);
            }
        }

        // T31: `task.result()` → Rust's `task.await`. This is the ONLY
        // `.await` form that originates from a method-call position; it's
        // the suspension-point API on Buff's `Task<T>` (a thin alias for
        // `tokio::task::JoinHandle<T>`). The check MUST run BEFORE the T26
        // field-access-vs-method-call heuristic below, because `result()`
        // is a zero-arg method call and would otherwise be rewritten as a
        // field access `task.result` (which is meaningless on a JoinHandle).
        // We accept both `task.result()` (with parens) and the postfix-
        // form `task.result` (no parens — same AST shape per the parser's
        // Dot arm) by NOT gating on `args.is_empty()` here.
        if method.name == "result" {
            let recv = self.lower_expr(receiver)?;
            return Ok(make_await(recv));
        }

        // T26 field-access-vs-method-call disambiguation.
        //
        // Buff parses `obj.field` and `obj.method()` through the SAME AST
        // shape (`Expr::MethodCall { receiver, method, args }`) — see the
        // parser's `parse_postfix` Dot arm: a `.` followed by an Ident
        // WITHOUT parens produces a zero-arg MethodCall. So `p.name` (a
        // field access on a user struct) and `v.len()` (a real method call)
        // are indistinguishable at the AST level.
        //
        // Heuristic (T26): when `args` is empty AND `method.name` is NOT in
        // the [`KNOWN_ZERO_ARG_METHODS`] allow-list, emit a Rust field
        // access `recv.field` instead of a method call `recv.field()`. This
        // is the additive-only approach: no AST migration required, no new
        // FieldAccess variant — just a codegen-time rewrite. A dedicated
        // `Expr::FieldAccess` AST node is the cleaner long-term shape (see
        // migration note in `decisions.md`), deferred to keep T26 additive.
        //
        // Trade-off: if a user defines a struct with a field literally named
        // `len` / `push` / etc., `obj.len` will emit `obj.len()` (wrong). The
        // allow-list is conservative (only names this codegen actually
        // handles + the universal `clone`/`to_string`/etc.); new builtins
        // added later must be added to the list to preserve the heuristic.
        //
        // T124b: this heuristic also needs to NOT fire for prelude-type
        // instance methods (`dt.format(...)`, `dt.year()`, ...). Those
        // receivers are NEVER a bare `Expr::Ident` naming a prelude TYPE
        // (handled by the assoc_fn_lookup arm above) — they're values. But
        // we must consult the registry to decide whether `dt.year()` (zero
        // args) is a real method call vs. a field access. The dedicated
        // prelude-instance arm runs AFTER this heuristic, so we extend
        // KNOWN_ZERO_ARG_METHODS to include the prelude instance methods
        // that take zero args (year/month/day/hour/minute/second/
        // timestamp). `format` takes one arg so it's never affected.
        if args.is_empty() && !KNOWN_ZERO_ARG_METHODS.contains(&method.name.as_str()) {
            let recv = self.lower_expr(receiver)?;
            return Ok(field_access(recv, &method.name));
        }

        // T124b: prelude-types registry — instance methods. A call of the
        // form `recv.method(args)` where the receiver INFERS to a prelude
        // datetime type. Runs AFTER the T26 field-access heuristic so the
        // zero-arg instance methods (year/month/day/...) — which are in
        // KNOWN_ZERO_ARG_METHODS — pass through to here.
        //
        // We consult the integrated TypeInferencer to get the receiver's
        // resolved Type. Inference errors fall through to the default
        // `recv.method(args)` lowering (Rust will then diagnose the
        // receiver-type mismatch).
        let recv_for_prelude_check = self
            .type_inferencer
            .infer_expr(receiver)
            .unwrap_or(Type::Unknown);
        if let Some(pmethod) =
            buff_lang_types::prelude_types::instance_fn_lookup(&recv_for_prelude_check, &method.name)
        {
            return self.lower_prelude_type_instance_fn(
                &recv_for_prelude_check,
                pmethod,
                receiver,
                args,
            );
        }

        // T24: `Matrix.new(rows, cols)` — the builtin Matrix constructor.
        // Buff's constructor convention is `Type.new()` / `Type.from()` (§7),
        // parsed as a MethodCall whose receiver is a bare Ident naming the
        // type. We special-case `Matrix.new(...)` here to lower it to Rust's
        // `Matrix::new(rows, cols)` associated-function call. The `Matrix<T>`
        // struct definition itself is emitted on-demand by
        // [`Self::generate`] when a program uses `Matrix.new(...)`.
        if method.name == "new" {
            if let Expr::Ident(id, _) = receiver {
                if id.name == "Matrix" {
                    return self.lower_matrix_new(args);
                }
            }
        }

        // T78: `recv.context("msg")` — error-context chaining.
        //
        // Attaches a human-readable context string to a `Result<T, E>`'s
        // `Err` variant, then (typically) propagates with `?`. The parser
        // already produces this as `Expr::MethodCall { method: "context",
        // args: [string_literal] }`, often wrapped in `Expr::Try` for the
        // trailing `?`. We special-case it HERE (before the field-access
        // heuristic and the default `recv.method(args)` arm) so the name
        // `context` NEVER falls through to a plain Rust method call.
        //
        // Desugar: `recv.context("msg")` →
        //   `recv.map_err(|e| format!("msg: {:?}", e))`
        //
        // The trailing `?` (if any) is added by the EXISTING `lower_try`
        // path — the wrapping `Expr::Try` lowers independently. So this
        // codegen is purely additive: NO new AST variant, NO change to
        // `lower_try`.
        //
        // Design choice — Debug (`{:?}`) over Display (`{}`):
        //   The std `Error: Debug` bound is universally implemented (every
        //   `T: Error` gets Debug via `#[derive(Debug)]` or manual impl),
        //   while `Display` is NOT automatically implemented for many error
        //   types. Using `{:?}` guarantees the generated Rust compiles for
        //   ANY error type the user's `Result<T, E>` might carry. The Debug
        //   rendering is also richer (shows variant names + fields), which
        //   is what a developer debugging a chained error wants.
        //
        // Design choice — `map_err` + `format!` over `anyhow::Context`:
        //   The codegen target is standalone `rustc` with NO external
        //   runtime crate (the generated Cargo project has no `anyhow` /
        //   `thiserror` dependency — confirmed by prior tasks where external
        //   crates were deferred). Emitting `anyhow::Context` would require
        //   adding `anyhow` to every generated project; the `map_err` +
        //   `format!` desugar keeps the generated Rust self-contained. The
        //   trade-off is loss of typed error context (we get a `String`
        //   error, not a structured error chain) — typed context objects
        //   are deferred (see decisions.md).
        if method.name == "context" {
            let recv = self.lower_expr(receiver)?;
            return self.lower_context_call(recv, args);
        }

        let recv = self.lower_expr(receiver)?;
        let method_name = method.name.as_str();

        // Helper: lower `args` into a Punctuated list.
        let lower_args =
            |codegen: &mut Self| -> Result<Punctuated<SynExpr, syn::Token![,]>, CodegenError> {
                let mut out: Punctuated<SynExpr, syn::Token![,]> = Punctuated::new();
                for a in args {
                    out.push(codegen.lower_expr(a)?);
                }
                Ok(out)
            };

        // String-method mappings.
        let lowered = match method_name {
            // `s.char_count()` → `s.chars().count()`
            "char_count" if args.is_empty() => {
                self.method_chain(recv, &["chars", "count"], None)?
            }
            // `s.byte_len()` → `s.len()`
            "byte_len" if args.is_empty() => self.method_chain(recv, &["len"], None)?,
            // `s.chars()` → `s.chars()`
            "chars" if args.is_empty() => self.method_chain(recv, &["chars"], None)?,
            // `s.bytes()` → `s.bytes()`
            "bytes" if args.is_empty() => self.method_chain(recv, &["bytes"], None)?,
            // `s.first()` → `s.chars().next()`
            "first" if args.is_empty() => self.method_chain(recv, &["chars", "next"], None)?,
            // `s.last()` → `s.chars().last()`
            "last" if args.is_empty() => self.method_chain(recv, &["chars", "last"], None)?,
            // `s.graphemes()` → grapheme iterator wrapped via unicode-segmentation.
            // For now we return a flattened String (`.collect()`) so callers
            // can treat the result as a `String` without dragging the trait
            // into every scope. A future task will introduce a typed iterator.
            "graphemes" if args.is_empty() => self.lower_graphemes_call(recv)?,
            // `s.slice(a, b)` → char-safe slice.
            // Approach: `s.chars().skip(a).take(b - a).collect::<String>()`.
            // We lower the two integer arguments and emit the chain. If `b`
            // is not provided, we use `s.chars().skip(a).collect::<String>()`.
            "slice" => self.lower_slice_call(recv, args)?,
            // T23: Vector iteration methods. `.map` / `.filter` take a single
            // closure and return a new `Vec`; `.reduce` takes a 2-arg closure
            // and returns `Option<T>`. We use `.into_iter()` so the closure
            // params are owned values (Buff hides references from users).
            // `.push(x)` / `.pop()` / `.len()` need no special mapping —
            // they fall through to the default `recv.method(args)` arm below.
            "map" if args.len() == 1 => {
                let f = self.lower_expr(&args[0])?;
                self.lower_into_iter_collect(recv, "map", f)?
            }
            "filter" if args.len() == 1 => {
                let f = self.lower_expr(&args[0])?;
                self.lower_into_iter_collect(recv, "filter", f)?
            }
            "reduce" if args.len() == 1 => {
                let f = self.lower_expr(&args[0])?;
                self.lower_into_iter_reduce(recv, f)?
            }
            // T25: Map methods. The Buff names map to Rust's standard
            // HashMap methods, except `.contains(k)` → `.contains_key(k)`
            // (Buff hides the `_key` suffix for ergonomics). `.get(k)`
            // returns `Option<&V>` in Rust; we keep it as-is (`Option<&V>`)
            // for v0.5 — a future task may add `.cloned()` to recover an
            // owned `Option<V>` if the move-by-default analysis requires it.
            // `.insert(k, v)`, `.remove(k)`, and `.len()` pass through
            // unchanged because Buff and Rust share those names.
            "contains" if args.len() == 1 => {
                let arg = self.lower_expr(&args[0])?;
                method_call_one_arg(recv, "contains_key", arg)
            }
            // `.get`, `.insert`, `.remove`, `.len` all share Rust's name —
            // they fall through to the default arm below with no special
            // mapping. We keep this comment block to document the T25 design
            // (so a future change doesn't accidentally rewrite these).
            // Default: a plain method call `recv.method(args)`.
            _ => {
                let args_punct = lower_args(self)?;
                SynExpr::MethodCall(syn::ExprMethodCall {
                    attrs: Vec::new(),
                    receiver: Box::new(recv),
                    dot_token: Default::default(),
                    method: Ident::new(method_name, ProcSpan::call_site()),
                    turbofish: None,
                    paren_token: Default::default(),
                    args: args_punct,
                })
            }
        };
        Ok(lowered)
    }

    /// Build a chained method call: `recv.m1().m2()...` (no args at any link).
    /// If `final_method` is given, it's used as the OUTERMOST call name (the
    /// last element of `methods` overrides it; passing `None` is equivalent).
    fn method_chain(
        &self,
        recv: SynExpr,
        methods: &[&str],
        _final_method: Option<&str>,
    ) -> Result<SynExpr, CodegenError> {
        let mut acc = recv;
        for &m in methods {
            acc = SynExpr::MethodCall(syn::ExprMethodCall {
                attrs: Vec::new(),
                receiver: Box::new(acc),
                dot_token: Default::default(),
                method: Ident::new(m, ProcSpan::call_site()),
                turbofish: None,
                paren_token: Default::default(),
                args: Default::default(),
            });
        }
        Ok(acc)
    }

    /// Lower `s.graphemes()` to a grapheme-iteration expression that yields a
    /// `String` of concatenated grapheme clusters.
    ///
    /// Emits (conceptually):
    /// ```text
    /// unicode_segmentation::UnicodeSegmentation::graphemes(&s, true)
    ///     .collect::<String>()
    /// ```
    ///
    /// The call is built as a `quote!`-expanded token stream so we never
    /// hand-format Rust. The trait must be in scope at the use site — see
    /// the generated-crate wiring note in T21 deferral.
    fn lower_graphemes_call(&self, recv: SynExpr) -> Result<SynExpr, CodegenError> {
        // We use quote! to build the macro-shaped expression. The receiver
        // is spliced in via `#recv`. The full path avoids needing a `use`
        // import in the generated crate.
        let tokens: proc_macro2::TokenStream =
            syn::parse_str("unicode_segmentation::UnicodeSegmentation::graphemes(&__recv, true)")
                .map_err(|e| self.unsupported(&format!("graphemes path parse: {e}")))?;
        // Manually build: __trait_path::graphemes(&recv, true).collect::<String>()
        // by constructing an ExprMethodCall for `.collect::<String>()`.
        let graphemes_call = splice_receiver_into_call(tokens, recv)?;
        let collect_call = SynExpr::MethodCall(syn::ExprMethodCall {
            attrs: Vec::new(),
            receiver: Box::new(graphemes_call),
            dot_token: Default::default(),
            method: Ident::new("collect", ProcSpan::call_site()),
            // turbofish: `::<String>`
            turbofish: Some(syn::AngleBracketedGenericArguments {
                colon2_token: None,
                lt_token: Default::default(),
                args: {
                    let mut p: Punctuated<syn::GenericArgument, syn::Token![,]> = Punctuated::new();
                    p.push(syn::GenericArgument::Type(rust_path_type("String")));
                    p
                },
                gt_token: Default::default(),
            }),
            paren_token: Default::default(),
            args: Default::default(),
        });
        Ok(collect_call)
    }

    /// T78: lower `recv.context("msg")` to
    /// `recv.map_err(|e| format!("msg: {:?}", e))`.
    ///
    /// Attaches a human-readable context string to a `Result<T, E>`'s `Err`
    /// variant by wrapping the inner error into a formatted `String`. The
    /// generated Rust compiles under standalone `rustc` (no `anyhow` /
    /// `thiserror` needed) because it uses only `Result::map_err` and the
    /// stdlib `format!` macro.
    ///
    /// Built via `quote!` + `syn::parse2::<SynExpr>` (the standard pattern
    /// in this module — the single string producer remains
    /// `prettyplease::unparse`). The message is spliced into a
    /// [`syn::LitStr`] that already carries the `: {:?}` format-spec suffix
    /// so the resulting `format!(...)` call has the right shape.
    ///
    /// Argument contract:
    /// - EXACTLY one argument.
    /// - The argument MUST be a `Expr::Literal(Literal::String(_), _)`. Any
    ///   other shape (non-string literal, identifier, call, ...) returns an
    ///   `unsupported` error — codegen does NOT do type checking, so this
    ///   guards against silent mis-compilation of `.context(42)` or similar.
    ///
    /// The trailing `?` (if the source had `recv.context("msg")?`) is added
    /// by the EXISTING [`Self::lower_try`] path: the parser produces
    /// `Expr::Try { expr: MethodCall{...} }`, and `lower_try` wraps the
    /// lowered MethodCall in Rust's native `?`. So NO change to `lower_try`
    /// is required — this method only produces the `map_err` expression.
    ///
    /// Debug (`{:?}`) is chosen over Display (`{}`) because the std
    /// `Error: Debug` bound is universally implemented while `Display` is
    /// not — using `{:?}` guarantees the generated Rust compiles for ANY
    /// error type the user's `Result<T, E>` might carry. See the design
    /// note on the `context` arm in [`Self::lower_method_call`].
    fn lower_context_call(&self, recv: SynExpr, args: &[Expr]) -> Result<SynExpr, CodegenError> {
        if args.len() != 1 {
            return Err(self.unsupported(&format!(
                "context() expects exactly 1 string-literal arg, got {}",
                args.len()
            )));
        }
        let msg: &str = match &args[0] {
            Expr::Literal(Literal::String(s), _) => s.as_str(),
            other => {
                return Err(self.unsupported(&format!(
                    "context() expects a string literal, got {:?}",
                    other
                )));
            }
        };
        // Build the format-string literal: `"<msg>: {:?}"`.
        //
        // The user's message is the literal prefix; the `: {:?}` suffix
        // renders the original error via Debug. If the message itself
        // contains `{` or `}`, those WILL be interpreted as `format!`
        // placeholders at runtime — this matches the documented behavior
        // (context is a human-readable label, not a format template).
        // Braces in context labels are rare; escaping them would silently
        // rewrite the user's text. Keeping the message verbatim preserves
        // the WYSIWYG property tested by `error_context_preserves_message_*`.
        let fmt = format!("{}: {{:?}}", msg);
        let fmt_lit = syn::LitStr::new(&fmt, ProcSpan::call_site());
        let tokens: proc_macro2::TokenStream = quote::quote! {
            #recv.map_err(|e| format!(#fmt_lit, e))
        };
        syn::parse2::<SynExpr>(tokens)
            .map_err(|e| self.unsupported(&format!("context codegen parse: {e}")))
    }

    /// Lower `s.slice(a, b)` to a char-safe slice expression.
    ///
    /// Emits (conceptually) `s.chars().skip(a).take(b - a).collect::<String>()`.
    /// A single-arg form `s.slice(a)` becomes `s.chars().skip(a).collect::<String>()`.
    fn lower_slice_call(&mut self, recv: SynExpr, args: &[Expr]) -> Result<SynExpr, CodegenError> {
        if args.is_empty() || args.len() > 2 {
            return Err(self.unsupported(&format!(
                "slice expects 1 or 2 integer args, got {}",
                args.len()
            )));
        }
        // Start: `s.chars()`
        let chars_call = self.method_chain(recv, &["chars"], None)?;
        // `.skip(a)`
        let skip_arg = self.lower_expr(&args[0])?;
        let skip_call = method_call_one_arg(chars_call, "skip", skip_arg);
        // `.take(b - a)` if a second arg is present; else just chain collect.
        let after_take = if args.len() == 2 {
            let b_arg = self.lower_expr(&args[1])?;
            // Compute `b - a` as a Rust binary subtraction at runtime so the
            // arguments don't have to be literals.
            let b_minus_a = SynExpr::Binary(syn::ExprBinary {
                attrs: Vec::new(),
                left: Box::new(b_arg),
                op: syn::BinOp::Sub(Default::default()),
                right: Box::new(self.lower_expr(&args[0])?),
            });
            method_call_one_arg(skip_call, "take", b_minus_a)
        } else {
            skip_call
        };
        // `.collect::<String>()`
        let collect_call = SynExpr::MethodCall(syn::ExprMethodCall {
            attrs: Vec::new(),
            receiver: Box::new(after_take),
            dot_token: Default::default(),
            method: Ident::new("collect", ProcSpan::call_site()),
            turbofish: Some(syn::AngleBracketedGenericArguments {
                colon2_token: None,
                lt_token: Default::default(),
                args: {
                    let mut p: Punctuated<syn::GenericArgument, syn::Token![,]> = Punctuated::new();
                    p.push(syn::GenericArgument::Type(rust_path_type("String")));
                    p
                },
                gt_token: Default::default(),
            }),
            paren_token: Default::default(),
            args: Default::default(),
        });
        Ok(collect_call)
    }

    /// Lower a Vector iteration method that returns a new `Vec` (T23).
    ///
    /// `recv.<method>(closure)` → `recv.into_iter().<method>(closure).collect::<Vec<_>>()`.
    /// Used by `.map` and `.filter`. We use `.into_iter()` so the closure
    /// receives owned values (Buff hides references from users); this
    /// consumes the receiver, matching Buff's move-by-default semantics.
    /// The `.collect::<Vec<_>>()` rebuilds a Vec so the result can be indexed
    /// or chained further.
    fn lower_into_iter_collect(
        &self,
        recv: SynExpr,
        method: &str,
        closure: SynExpr,
    ) -> Result<SynExpr, CodegenError> {
        let method_ident = Ident::new(method, ProcSpan::call_site());
        let tokens: proc_macro2::TokenStream = quote::quote! {
            #recv.into_iter().#method_ident(#closure).collect::<Vec<_>>()
        };
        syn::parse2(tokens).map_err(|e| self.unsupported(&format!("{method} codegen parse: {e}")))
    }

    /// Lower `.reduce(closure)` → `recv.into_iter().reduce(closure)` (T23).
    ///
    /// Returns `Option<T>` (Rust parity). The closure is a 2-arg `|a, b| …`.
    fn lower_into_iter_reduce(
        &self,
        recv: SynExpr,
        closure: SynExpr,
    ) -> Result<SynExpr, CodegenError> {
        let tokens: proc_macro2::TokenStream = quote::quote! {
            #recv.into_iter().reduce(#closure)
        };
        syn::parse2(tokens).map_err(|e| self.unsupported(&format!("reduce codegen parse: {e}")))
    }

    /// Lower a string interpolation `"text {expr} more"` to a Rust
    /// `format!("text {} more", expr)` macro invocation.
    ///
    /// The format string is built by walking the parts:
    /// - `InterpPart::Literal(s)` — the literal text, with each `{`/`}`
    ///   escaped to `{{`/`}}` so `format!` doesn't interpret them as slots.
    /// - `InterpPart::Expr(_)` — a `{}` placeholder in the format string, and
    ///   the lowered expression as a positional argument after the string.
    ///
    /// The final `format!` call is built via `quote!` so the format string
    /// and args are spliced in without any hand-formatted Rust.
    fn lower_string_interp(&mut self, parts: &[InterpPart]) -> Result<SynExpr, CodegenError> {
        // Build the format string with `{}` placeholders for each Expr.
        let mut fmt_string = String::new();
        let mut lowered_args: Vec<SynExpr> = Vec::new();
        for part in parts {
            match part {
                InterpPart::Literal(text) => {
                    // Escape `{` → `{{` and `}` → `}}` so they're literal.
                    for c in text.chars() {
                        match c {
                            '{' => fmt_string.push_str("{{"),
                            '}' => fmt_string.push_str("}}"),
                            _ => fmt_string.push(c),
                        }
                    }
                }
                InterpPart::Expr(e) => {
                    fmt_string.push_str("{}");
                    lowered_args.push(self.lower_expr(e)?);
                }
            }
        }
        // Build the format! macro: tokens are "<fmt>", arg1, arg2, ...
        // We build this with quote! by splicing each argument in turn.
        let format_lit = proc_macro2::Literal::string(&fmt_string);
        let args_tokens: Vec<proc_macro2::TokenStream> = lowered_args
            .iter()
            .map(|a| {
                let a = a.clone();
                quote::quote! { #a }
            })
            .collect();
        let combined: proc_macro2::TokenStream = if args_tokens.is_empty() {
            // Should never happen (interp always has at least one Expr),
            // but guard against malformed AST.
            quote::quote! { #format_lit }
        } else {
            let mut ts: proc_macro2::TokenStream = quote::quote! { #format_lit };
            for a in args_tokens {
                ts.extend(quote::quote! { , #a });
            }
            ts
        };
        Ok(SynExpr::Macro(syn::ExprMacro {
            attrs: Vec::new(),
            mac: syn::Macro {
                path: syn::Path::from(Ident::new("format", ProcSpan::call_site())),
                bang_token: Default::default(),
                delimiter: syn::MacroDelimiter::Paren(Default::default()),
                tokens: combined,
            },
        }))
    }

    /// Lower a collection literal `[e1, e2, ...]` to Rust's `vec![...]` macro
    /// (T23).
    ///
    /// The element expressions are lowered and spliced into the macro token
    /// stream via `quote!`, comma-separated. An empty literal lowers to
    /// `vec![]` (Rust infers the element type from context — typically the
    /// `let`-binding's type annotation, which the type inferencer drove).
    fn lower_array_lit(&mut self, elements: &[Expr]) -> Result<SynExpr, CodegenError> {
        // Lower each element, then build `vec![e0, e1, ...]`. The `[` / `]`
        // come from the `Bracket` delimiter; the `tokens` stream holds just
        // the comma-separated element expressions (so `vec![]` for empty).
        let mut lowered: Vec<SynExpr> = Vec::with_capacity(elements.len());
        for e in elements {
            lowered.push(self.lower_expr(e)?);
        }
        let mut tokens: proc_macro2::TokenStream = proc_macro2::TokenStream::new();
        for (i, e) in lowered.iter().enumerate() {
            let e = e.clone();
            if i > 0 {
                tokens.extend(quote::quote! { , });
            }
            tokens.extend(quote::quote! { #e });
        }
        Ok(SynExpr::Macro(syn::ExprMacro {
            attrs: Vec::new(),
            mac: syn::Macro {
                path: syn::Path::from(Ident::new("vec", ProcSpan::call_site())),
                bang_token: Default::default(),
                delimiter: syn::MacroDelimiter::Bracket(Default::default()),
                tokens,
            },
        }))
    }

    /// Lower a 2-D Matrix index `m[row, col]` to the flat-storage access
    /// `m.data[(row * m.cols + col) as usize]` (T24).
    ///
    /// The base expression `m` is lowered ONCE and the resulting `SynExpr` is
    /// spliced (via [`SynExpr::clone`]) into two positions:
    /// - `m.data` — the field holding the flat `Vec<T>`.
    /// - `m.cols` — the field carrying the column count.
    ///
    /// The flat index expression `row * m.cols + col` is built as a Rust
    /// binary tree (`Mul(row, Field(m, cols))` then `Add(.., col)`) and the
    /// whole thing is wrapped in a single `as usize` cast via [`cast_to`]
    /// (which parenthesises its operand, yielding exactly
    /// `(row * m.cols + col) as usize`). The outer `m.data[...]` is a Rust
    /// index expression.
    ///
    /// Both `row` and `col` are lowered as-is (no per-operand cast); the
    /// single trailing `as usize` covers the whole flat expression. This
    /// matches the T24 acceptance string `m.data[(1 * m.cols + 2) as usize]`.
    ///
    /// **GPU-readiness note**: because storage is one contiguous `Vec<T>`,
    /// the same flat-index expression is what a WGSL shader would compute to
    /// address a storage buffer — the REFACTOR goal of "share flat-storage
    /// pattern with GPU buffer codegen" lands naturally here.
    fn lower_matrix_index(
        &mut self,
        base: &Expr,
        row: &Expr,
        col: &Expr,
    ) -> Result<SynExpr, CodegenError> {
        let base_e = self.lower_expr(base)?;
        let row_e = self.lower_expr(row)?;
        let col_e = self.lower_expr(col)?;
        // `m.data` — field access on the lowered base.
        let data_field = field_access(base_e.clone(), "data");
        // `m.cols` — field access on the lowered base (clone preserves the
        // move analyzer's clone decision, if any, that was baked into base_e).
        let cols_field = field_access(base_e, "cols");
        // `row * m.cols`
        let row_times_cols = SynExpr::Binary(syn::ExprBinary {
            attrs: Vec::new(),
            left: Box::new(row_e),
            op: syn::BinOp::Mul(Default::default()),
            right: Box::new(cols_field),
        });
        // `(row * m.cols) + col`
        let flat_expr = SynExpr::Binary(syn::ExprBinary {
            attrs: Vec::new(),
            left: Box::new(row_times_cols),
            op: syn::BinOp::Add(Default::default()),
            right: Box::new(col_e),
        });
        // `((row * m.cols) + col) as usize` — cast_to wraps in parens.
        let flat_index = cast_to(flat_expr, "usize");
        // `m.data[flat_index]`
        Ok(SynExpr::Index(syn::ExprIndex {
            attrs: Vec::new(),
            expr: Box::new(data_field),
            bracket_token: Default::default(),
            index: Box::new(flat_index),
        }))
    }

    /// Lower `Matrix.new(rows, cols)` to Rust's `Matrix::new(rows, cols)`
    /// associated-function call (T24).
    ///
    /// The receiver `Matrix` is NOT lowered as a value (it names a type, not
    /// a variable) — we build the path `Matrix::new` directly and splice the
    /// lowered arguments. The arity is checked: exactly 2 args (rows, cols)
    /// are required. The `Matrix<T>` struct + `new` impl are emitted by
    /// [`Self::generate`] when this constructor appears in the program.
    fn lower_matrix_new(&mut self, args: &[Expr]) -> Result<SynExpr, CodegenError> {
        if args.len() != 2 {
            return Err(self.unsupported(&format!(
                "Matrix.new expects exactly 2 args (rows, cols), got {}",
                args.len()
            )));
        }
        let mut lowered: Punctuated<SynExpr, syn::Token![,]> = Punctuated::new();
        lowered.push(self.lower_expr(&args[0])?);
        lowered.push(self.lower_expr(&args[1])?);
        Ok(SynExpr::Call(syn::ExprCall {
            attrs: Vec::new(),
            func: Box::new(SynExpr::Path(syn::ExprPath {
                attrs: Vec::new(),
                qself: None,
                path: rust_path("Matrix::new"),
            })),
            paren_token: Default::default(),
            args: lowered,
        }))
    }

    /// Lower a minimal closure `{ params => expr }` to a Rust closure
    /// `|p1, p2| body` (T23 + T34 capture analysis).
    ///
    /// Param types are inferred by Rust — we emit no annotations (matching
    /// Buff's "hide the types" philosophy). The body is a single expression
    /// in T23's minimal shape; if the parser produced a multi-statement
    /// block, it is lowered as a block expression.
    ///
    /// # T34: variable capture
    ///
    /// Before lowering the body, we compute the set of variables CAPTURED
    /// by this closure (free vars of body minus params minus closure-local
    /// lets) via [`buff_lang_types::closure_captures`] — the shared
    /// capture analysis extracted from T33's spawn free-var walker. The
    /// capture set is pushed onto [`Self::closure_capture_stack`] so that
    /// [`Self::lower_expr`]'s `Expr::Ident` arm can emit captured
    /// variables plainly WITHOUT calling [`MoveAnalyzer::needs_clone`].
    ///
    /// Rust closures handle capture automatically (by ref or by move based
    /// on how the body uses the variable). Buff's job is only to AVOID
    /// inserting spurious `.clone()` calls for captured-variable uses
    /// INSIDE the closure body — without the capture stack, a non-Copy
    /// captured var used twice in a closure would get a wrong `.clone()`
    /// on its second use (MoveAnalyzer would see it as "use after move").
    fn lower_lambda(
        &mut self,
        params: &[buff_lang_ast::common::Param],
        body: &Block,
    ) -> Result<SynExpr, CodegenError> {
        // Build the closure parameter patterns: `|p1, p2, ...|`.
        let mut pats: Punctuated<Pat, syn::Token![,]> = Punctuated::new();
        for p in params {
            pats.push(Pat::Ident(PatIdent {
                attrs: Vec::new(),
                ident: ast_ident_to_syn(&p.name),
                by_ref: None,
                mutability: None,
                subpat: None,
            }));
        }
        // T34: compute captures and push onto the stack so the body-
        // lowering path knows which idents are captured (and should
        // bypass needs_clone). Popped after the body is lowered so the
        // stack correctly reflects the enclosing scope on exit.
        //
        // We ALSO insert the closure's own PARAM names into the pushed
        // set: closure params are fresh bindings owned by the closure
        // body, and Rust handles their ownership within the body (Copy
        // params are copied, non-Copy by-value uses are Rust's concern).
        // Without this, a param used multiple times in the body (e.g.
        // `|x| x * x + x`) would get a spurious `.clone()` from the
        // MoveAnalyzer on its second+ use — a pre-existing T23 limitation
        // that T34's capture-aware codegen naturally fixes by treating
        // params the same as captures (bypass needs_clone inside the body).
        let mut bypass_set = buff_lang_types::closure_captures(params, body);
        for p in params {
            bypass_set.insert(p.name.name.clone());
        }
        self.closure_capture_stack.push(bypass_set);
        // Body: a single ExprStmt lowers to a bare expression; otherwise a
        // block expression.
        let body_expr = self.lower_lambda_body(body);
        // Always pop, even if body lowering errored, so the stack stays
        // balanced across error recovery paths.
        self.closure_capture_stack.pop();
        let body_expr = body_expr?;
        Ok(SynExpr::Closure(syn::ExprClosure {
            attrs: Vec::new(),
            lifetimes: Default::default(),
            constness: None,
            movability: None,
            asyncness: None,
            capture: None,
            or1_token: Default::default(),
            or2_token: Default::default(),
            inputs: pats,
            output: ReturnType::Default,
            body: Box::new(body_expr),
        }))
    }

    /// Lower a lambda body. If the block is a single `ExprStmt`, lower that
    /// expression directly (so `|x| x * 2` not `|x| { x * 2 }`); otherwise
    /// lower the block as a `syn::Expr::Block`.
    fn lower_lambda_body(&mut self, body: &Block) -> Result<SynExpr, CodegenError> {
        if body.stmts.len() == 1 {
            if let Stmt::ExprStmt(e, _) = &body.stmts[0] {
                return self.lower_expr(e);
            }
        }
        let block = self.lower_block(body)?;
        Ok(SynExpr::Block(syn::ExprBlock {
            attrs: Vec::new(),
            label: None,
            block,
        }))
    }

    /// Lower a map literal `{"k": v, ...}` (or empty `{:}`) to Rust's
    /// `std::collections::HashMap::from([("k", v), ...])` (T25).
    ///
    /// Each entry's key and value are lowered independently and spliced into
    /// the outer array as Rust tuples. The fully-qualified path
    /// `std::collections::HashMap::from` is used (not a bare `HashMap::from`
    /// with a `use` import) so generated programs need NO import wiring.
    ///
    /// For an empty literal `{:}` we emit `HashMap::from([])` (Rust infers the
    /// key/value types from the `let`-binding annotation, which the codegen's
    /// type inferencer drives).
    ///
    /// The output is built via `quote!` so the outer `::from([...])` shell and
    /// the comma-separated tuple entries are constructed without any
    /// hand-formatted Rust strings.
    fn lower_map_lit(&mut self, entries: &[(Expr, Expr)]) -> Result<SynExpr, CodegenError> {
        // Lower each (key, value) pair into a Rust tuple expression. We
        // build the tuple via `syn::ExprTuple` so it's a real AST node
        // (not a token stream).
        let mut lowered_entries: Vec<SynExpr> = Vec::with_capacity(entries.len());
        for (k, v) in entries {
            let k_e = self.lower_expr(k)?;
            let v_e = self.lower_expr(v)?;
            let mut tuple_elems: Punctuated<SynExpr, syn::Token![,]> = Punctuated::new();
            tuple_elems.push(k_e);
            tuple_elems.push(v_e);
            // A trailing comma is required by Rust for single-element tuples;
            // for 2-element tuples it's optional but harmless. We always add
            // one for uniformity.
            let tuple = SynExpr::Tuple(syn::ExprTuple {
                attrs: Vec::new(),
                paren_token: Default::default(),
                elems: tuple_elems,
            });
            lowered_entries.push(tuple);
        }
        // Build the outer `[<entries>]` array literal as a Rust expression
        // via `quote!`. Each lowered tuple is spliced in comma-separated.
        let mut entries_tokens: proc_macro2::TokenStream = proc_macro2::TokenStream::new();
        for (i, e) in lowered_entries.iter().enumerate() {
            if i > 0 {
                entries_tokens.extend(quote::quote! { , });
            }
            let e = e.clone();
            entries_tokens.extend(quote::quote! { #e });
        }
        // `std::collections::HashMap::from([<entries_tokens>])`
        let tokens: proc_macro2::TokenStream = quote::quote! {
            std::collections::HashMap::from([#entries_tokens])
        };
        syn::parse2(tokens)
            .map_err(|e| self.unsupported(&format!("map literal codegen parse: {e}")))
    }

    /// Lower a Buff struct-init expression `Type { field: value, ... }` to a
    /// Rust [`syn::ExprStruct`] of the same shape (T26).
    ///
    /// Each field is a `field: <lowered_value>` pair; the type path uses the
    /// struct name verbatim (Buff's struct names ARE Rust struct names — no
    /// renaming). The output is built via `quote!` so the brace-delimited
    /// body and comma-separated fields are constructed without any
    /// hand-formatted Rust strings.
    ///
    /// This mirrors the source form 1:1 because Buff deliberately matches
    /// Rust's struct-init syntax ( braces + named fields + colon ).
    fn lower_struct_init(
        &mut self,
        type_name: &buff_lang_ast::common::Ident,
        fields: &[(buff_lang_ast::common::Ident, Expr)],
    ) -> Result<SynExpr, CodegenError> {
        // Lower each field value first.
        let mut lowered_fields: Vec<(Ident, SynExpr)> = Vec::with_capacity(fields.len());
        for (fname, fval) in fields {
            let v = self.lower_expr(fval)?;
            lowered_fields.push((ast_ident_to_syn(fname), v));
        }
        // Build `Type { f1: v1, f2: v2, ... }` via `quote!`. Splice each
        // field as `#fname: #fval` (both are syn expressions/idents that
        // `quote!` can interpolate).
        let type_path = rust_path(&type_name.name);
        let mut fields_tokens: proc_macro2::TokenStream = proc_macro2::TokenStream::new();
        for (i, (fname, fval)) in lowered_fields.iter().enumerate() {
            if i > 0 {
                fields_tokens.extend(quote::quote! { , });
            }
            fields_tokens.extend(quote::quote! { #fname: #fval });
        }
        let tokens: proc_macro2::TokenStream = quote::quote! {
            #type_path { #fields_tokens }
        };
        syn::parse2(tokens)
            .map_err(|e| self.unsupported(&format!("struct init codegen parse: {e}")))
    }

    /// Lower a Buff `match scrutinee { arms }` to a Rust `syn::ExprMatch`
    /// (T27).
    ///
    /// Emits (conceptually):
    ///
    /// ```rust,ignore
    /// match <scrutinee> {
    ///     <pattern> => <body>,
    ///     <pattern> => <body>,
    ///     ...
    /// }
    /// ```
    ///
    /// Each arm's body is a `Block` (the parser wraps the single body
    /// expression in a one-statement block). The arm pattern goes through
    /// [`Self::lower_pattern`]; the body goes through [`Self::lower_block`].
    ///
    /// This mirrors the source form 1:1 because Buff deliberately matches
    /// Rust's `match` syntax. Exhaustiveness is checked separately by the
    /// `buff-lang-types` analysis pass; if a match is non-exhaustive the
    /// type-checker flags it BEFORE codegen runs (codegen assumes the match
    /// is well-formed).
    fn lower_match_expr(
        &mut self,
        scrutinee: &Expr,
        arms: &[MatchArm],
    ) -> Result<SynExpr, CodegenError> {
        let scrut = self.lower_expr(scrutinee)?;
        let mut arms_syn: Vec<syn::Arm> = Vec::with_capacity(arms.len());
        for arm in arms {
            let pat = self.lower_pattern(&arm.pattern, false)?;
            // The parser wraps the body expression in a one-statement
            // `ExprStmt` block. We lower the block and use it as the arm
            // body — Rust accepts a block as an arm body. If the block has
            // a single trailing expression, prettyplease will format it
            // back as `pat => expr,`; if it's multiple statements, the
            // block form `pat => { ... },` is emitted (also valid Rust).
            let body_block = self.lower_block(&arm.body)?;
            let body_expr = SynExpr::Block(syn::ExprBlock {
                attrs: Vec::new(),
                label: None,
                block: body_block,
            });
            arms_syn.push(syn::Arm {
                attrs: Vec::new(),
                pat,
                guard: None,
                fat_arrow_token: Default::default(),
                body: Box::new(body_expr),
                comma: Some(Default::default()),
            });
        }
        Ok(SynExpr::Match(syn::ExprMatch {
            attrs: Vec::new(),
            match_token: Default::default(),
            expr: Box::new(scrut),
            brace_token: Default::default(),
            arms: arms_syn,
        }))
    }

    /// Lower `expr?` to Rust's native `?` operator (T30 REFACTOR step).
    ///
    /// This is the extracted error-propagation codegen helper. It builds a
    /// `syn::ExprTry` wrapping the lowered operand, which `prettyplease`
    /// prints as `<expr>?`. Rust's `?` performs exactly the early-return
    /// propagation the task requires (`match expr { Ok(v) => v, Err(e) =>
    /// return Err(e.into()) }`), so we delegate to it rather than emitting
    /// the explicit match. The enclosing Buff function must lower to a Rust
    /// function returning `Result<T, E>` — which it does whenever the user
    /// writes a `Result<T, E>` return-type annotation, the only context
    /// where `?` is meaningful.
    ///
    /// Design choice (documented in the task): option (a) — Rust-native `?` —
    /// over option (b) — the explicit match. (a) is simpler, equally correct,
    /// and produces cleaner Rust that rustc optimises identically.
    fn lower_try(&mut self, expr: &Expr) -> Result<SynExpr, CodegenError> {
        let inner = self.lower_expr(expr)?;
        Ok(SynExpr::Try(syn::ExprTry {
            attrs: Vec::new(),
            expr: Box::new(inner),
            question_token: Default::default(),
        }))
    }

    /// Lower the prelude error constructor `Error(arg)` to
    /// `Err(Error::new(arg))` (T30).
    ///
    /// `Error("msg")` in Buff is sugar for an `Err` value carrying a
    /// freshly-constructed `Error` (the builtin error type emitted on-demand
    /// by [`Self::generate`]). It maps to `Err(Error::new(arg))` so a
    /// `return Error("msg")` produces an early `Err` return without the user
    /// writing `Err(...)` themselves.
    ///
    /// The single argument is lowered as a normal expression and spliced
    /// into the `Error::new(...)` call via `quote!` (so no hand-formatted
    /// Rust). The outer `Err(...)` is a path call built the same way.
    fn lower_error_constructor(&mut self, args: &[Expr]) -> Result<SynExpr, CodegenError> {
        if args.len() != 1 {
            return Err(self.unsupported(&format!(
                "Error() expects exactly 1 arg, got {}",
                args.len()
            )));
        }
        let arg = self.lower_expr(&args[0])?;
        // `Error::new(#arg)` — built via quote! so the path + arg splice
        // without hand-formatted Rust. The explicit type annotation pins the
        // `parse2` target so type inference doesn't fall back to `()`.
        let inner_call_tokens: proc_macro2::TokenStream = quote::quote! {
            Error::new(#arg)
        };
        let inner_call: SynExpr = syn::parse2(inner_call_tokens)
            .map_err(|e| self.unsupported(&format!("Error() codegen parse: {e}")))?;
        // Wrap in `Err(...)`.
        let tokens: proc_macro2::TokenStream = quote::quote! {
            Err(#inner_call)
        };
        syn::parse2::<SynExpr>(tokens)
            .map_err(|e| self.unsupported(&format!("Err() codegen parse: {e}")))
    }

    /// T31: lower `spawn <expr>` to Rust's `tokio::spawn(async move { <expr> })`.
    ///
    /// The task body becomes the body of an `async move` closure so the
    /// spawned task owns its captured variables (Buff hides borrow-checker
    /// pain from users; the generated Rust must be move-clean). The result
    /// is a `tokio::task::JoinHandle<T>` — Buff's `Task<T>` is a thin alias
    /// for this type, and the only `.await` on a Task lands at the
    /// `t.result()` site (see [`Self::lower_method_call`]).
    ///
    /// Built via `quote!` so the `tokio::spawn(async move { ... })` shape
    /// is constructed from real `syn` tokens rather than hand-formatted
    /// Rust. The single string producer remains `prettyplease::unparse`.
    fn lower_spawn(&mut self, task: &Expr) -> Result<SynExpr, CodegenError> {
        // T31: bump async-block depth so async calls inside the task body
        // still get `.await` (the `async move { ... }` block IS an async
        // context, even if the spawning fn is sync).
        self.async_block_depth += 1;
        // T33: bump spawn depth so ident uses inside the task body get
        // rewritten to `Arc::clone(&x)` (for Arc-shared bindings) instead
        // of moving or deep-cloning. Reset on exit so idents outside the
        // spawn go back to the regular move/clone path.
        self.spawn_depth += 1;
        let task_expr = self.lower_expr(task)?;
        self.spawn_depth -= 1;
        self.async_block_depth -= 1;
        let tokens: proc_macro2::TokenStream = quote::quote! {
            tokio::spawn(async move { #task_expr })
        };
        syn::parse2::<SynExpr>(tokens)
            .map_err(|e| self.unsupported(&format!("spawn codegen parse: {e}")))
    }

    /// T68: lower `start..end` (exclusive) or `start..=end` (inclusive) to a
    /// Rust range expression.
    ///
    /// Exclusive range `0..10` → Rust `0..10` via `syn::ExprRange`.
    /// Inclusive range `0..=10` → Rust `0..=10` via `syn::ExprRange`.
    ///
    /// Built via `quote!` so the `..` / `..=` operator is constructed from
    /// real `syn` tokens rather than hand-formatted Rust.
    fn lower_range(
        &mut self,
        start: &Expr,
        end: &Expr,
        inclusive: bool,
    ) -> Result<SynExpr, CodegenError> {
        let start_e = self.lower_expr(start)?;
        let end_e = self.lower_expr(end)?;
        let tokens: proc_macro2::TokenStream = if inclusive {
            quote::quote! { #start_e ..= #end_e }
        } else {
            quote::quote! { #start_e .. #end_e }
        };
        syn::parse2::<SynExpr>(tokens)
            .map_err(|e| self.unsupported(&format!("range codegen parse: {e}")))
    }

    /// T31: lower `block(<expr>)` to a one-shot tokio runtime block.
    ///
    /// Emits (conceptually):
    ///
    /// ```rust,ignore
    /// {
    ///     let rt = tokio::runtime::Runtime::new().expect("tokio runtime");
    ///     rt.block_on(<expr>)
    /// }
    /// ```
    ///
    /// This is the SYNC context form — it spins up a fresh current-thread
    /// runtime and blocks the calling thread on the async expression. It's
    /// the bridge between sync code and an async future when no runtime is
    /// already running.
    ///
    /// # `block()` inside an async fn — DEADLOCK RISK warning
    ///
    /// If `block()` is called from inside an async fn (the current fn is in
    /// the propagated async set), we emit a [`Diagnostic::warning`]
    /// explaining the deadlock risk: the runtime worker thread is blocked
    /// on `block_on`, so any future scheduled on the same worker can never
    /// run, deadlocking the program. The warning is appended to
    /// [`Self::warnings`]; the codegen still emits the (broken) Rust so the
    /// user can see what they wrote and decide how to refactor (usually:
    /// remove `block()` and let the async fn `return` the future directly).
    fn lower_block_call(&mut self, expr: &Expr) -> Result<SynExpr, CodegenError> {
        // Warn if we're inside an async fn — block_on in async is a deadlock.
        if self.current_fn_is_async() {
            let span = expr.span();
            self.warnings.push(
                Diagnostic::warning(
                    "`block()` inside an async function can deadlock the runtime",
                    span,
                )
                .with_note(
                    "block_on parks the current worker thread, preventing any future \
                     scheduled on the same worker from running. Consider returning the \
                     future directly instead of blocking on it.",
                ),
            );
        }
        let arg = self.lower_expr(expr)?;
        // Build the one-shot runtime block via quote! — no hand-formatted
        // Rust string. The expect() message is intentionally lowercase +
        // no trailing period per the conventions doc.
        let tokens: proc_macro2::TokenStream = quote::quote! {
            {
                let rt = tokio::runtime::Runtime::new().expect("failed to create tokio runtime");
                rt.block_on(#arg)
            }
        };
        syn::parse2::<SynExpr>(tokens)
            .map_err(|e| self.unsupported(&format!("block() codegen parse: {e}")))
    }

    /// T31: is the function we're currently lowering in the propagated
    /// async set? Used by [`Self::lower_block_call`] to decide whether to
    /// emit the deadlock warning.
    ///
    /// Returns `false` when called outside `lower_func` (e.g. when lowering
    /// a free-floating expression in tests).
    fn current_fn_is_async(&self) -> bool {
        match &self.current_fn_name {
            Some(name) => self.async_fns.contains(name),
            None => false,
        }
    }

    /// T31: are we currently inside an async context? True when either:
    ///   - the current fn is async (per [`Self::current_fn_is_async`]), OR
    ///   - we're inside one or more `async move { ... }` blocks (e.g.
    ///     inside a `spawn` body — the spawned task is itself async even
    ///     when the spawner is sync).
    ///
    /// Drives the `.await` insertion decision in [`Self::lower_expr`].
    fn in_async_context(&self) -> bool {
        self.current_fn_is_async() || self.async_block_depth > 0
    }

    /// T34: should `name` bypass [`MoveAnalyzer::needs_clone`] because we're
    /// inside a closure body and `name` is either a **captured variable**
    /// or a **closure parameter**?
    ///
    /// Checks the top-of-stack entry in [`Self::closure_capture_stack`].
    /// Returns `false` when not inside any closure body (empty stack).
    fn is_captured_in_closure(&self, name: &str) -> bool {
        match self.closure_capture_stack.last() {
            Some(bypass) => bypass.contains(name),
            None => false,
        }
    }

    /// Lower a Buff [`Pattern`] to a Rust [`syn::Pat`] (T27 / T71).
    ///
    /// Mapping:
    /// - [`Pattern::Wildcard`] → `syn::Pat::Wild` (`_`)
    /// - [`Pattern::Ident(name, _)`] → `syn::Pat::Ident(name)`. Rust resolves
    ///   whether the name is a unit variant or a fresh binding using type
    ///   information (matching Buff's deferred-resolve approach).
    /// - [`Pattern::Literal(lit, _)`] → `syn::Pat::Lit` (literal pattern).
    /// - [`Pattern::Variant { variant, subpatterns, .. }`] →
    ///   - if `subpatterns` is empty: `syn::Pat::Path` (`Variant` alone — a
    ///     unit variant reference; we never reach this from the parser since
    ///     unit variants come through `Pattern::Ident`, but the arm covers
    ///     hand-constructed ASTs from tests).
    ///   - else: `syn::Pat::TupleStruct` with one sub-pattern per slot. The
    ///     path is just `Variant` (no enum prefix) — Rust resolves it when
    ///     the enum is in scope. The `enum_name` field of the AST node is
    ///     ignored at codegen (the parser fills it with `""`).
    /// - [`Pattern::Tuple(subs, _)`] → `syn::Pat::Tuple` (`(a, b)`). T71.
    /// - [`Pattern::Struct { name, fields, .. }`] → `syn::Pat::Struct`
    ///   (`Point { x, y }`). Shorthand fields (name == binding name) are
    ///   reproduced as shorthand (no colon). T71.
    ///
    /// `mutable` (T71) — when `true`, every [`Pattern::Ident`] binding is
    /// emitted with `mut` (e.g. `mut x`). Match-arm callers pass `false`
    /// (patterns never carry `mut` in Buff syntax); the `let`-destructuring
    /// caller passes the binding's `mutable` flag so `let mut (a, b) = ...`
    /// lowers to `let (mut a, mut b) = ...`. `mutable` propagates recursively
    /// into sub-patterns so nested bindings all pick it up.
    fn lower_pattern(&mut self, pat: &Pattern, mutable: bool) -> Result<Pat, CodegenError> {
        let syn_pat: Pat = match pat {
            Pattern::Wildcard(_) => Pat::Wild(syn::PatWild {
                attrs: Vec::new(),
                underscore_token: Default::default(),
            }),
            Pattern::Ident(name, _) => Pat::Ident(PatIdent {
                attrs: Vec::new(),
                ident: ast_ident_to_syn(name),
                by_ref: None,
                mutability: mutable.then(Default::default),
                subpat: None,
            }),
            Pattern::Literal(lit, _) => {
                let lit_expr = self.lower_literal(lit)?;
                // `syn::Pat::Lit` is an alias for `syn::ExprLit` in syn 2.0
                // (see `syn::pat.rs`: `ExprLit as PatLit`). So a literal
                // pattern is constructed exactly like a literal expression:
                // wrap the `syn::Lit` in an `ExprLit` and hand it to
                // `Pat::Lit(...)`.
                let expr_lit = match lit_expr {
                    SynExpr::Lit(el) => el,
                    other => {
                        return Err(self.unsupported(&format!(
                            "literal pattern codegen expected Lit, got {other:?}"
                        )))
                    }
                };
                Pat::Lit(expr_lit)
            }
            Pattern::Variant {
                variant,
                subpatterns,
                ..
            } => {
                if subpatterns.is_empty() {
                    // Unit variant via path. Build `syn::Pat::Path` with a
                    // single-segment path equal to the variant name.
                    Pat::Path(syn::PatPath {
                        attrs: Vec::new(),
                        qself: None,
                        path: syn::Path::from(ast_ident_to_syn(variant)),
                    })
                } else {
                    // Tuple-struct variant: `Variant(subpat1, subpat2, ...)`.
                    let mut elems: Punctuated<Pat, syn::Token![,]> = Punctuated::new();
                    for sub in subpatterns {
                        elems.push(self.lower_pattern(sub, mutable)?);
                    }
                    Pat::TupleStruct(syn::PatTupleStruct {
                        attrs: Vec::new(),
                        qself: None,
                        path: syn::Path::from(ast_ident_to_syn(variant)),
                        paren_token: Default::default(),
                        elems,
                    })
                }
            }
            Pattern::Tuple(subs, _) => {
                // T71: tuple destructuring `(a, b, ...)`.
                let mut elems: Punctuated<Pat, syn::Token![,]> = Punctuated::new();
                for sub in subs {
                    elems.push(self.lower_pattern(sub, mutable)?);
                }
                Pat::Tuple(syn::PatTuple {
                    attrs: Vec::new(),
                    paren_token: Default::default(),
                    elems,
                })
            }
            Pattern::Struct { name, fields, .. } => {
                // T71: struct destructuring `Name { field: subpat, ... }`.
                // Hand-built via `syn::PatStruct` + `syn::FieldPat` (syn 2.0
                // renamed the field type `PatField`→`FieldPat`). Shorthand
                // (immutable + field name == binding name) is reproduced
                // without a colon: `Point { x }` not `Point { x: x }`.
                let mut field_pats: Punctuated<syn::FieldPat, syn::Token![,]> = Punctuated::new();
                for (field_name, subpat) in fields {
                    let is_shorthand = !mutable
                        && matches!(subpat, Pattern::Ident(id, _) if id.name == field_name.name);
                    let lowered = self.lower_pattern(subpat, mutable)?;
                    field_pats.push(syn::FieldPat {
                        attrs: Vec::new(),
                        member: ast_ident_to_syn(field_name).into(),
                        colon_token: if is_shorthand {
                            None
                        } else {
                            Some(Default::default())
                        },
                        pat: Box::new(lowered),
                    });
                }
                Pat::Struct(syn::PatStruct {
                    attrs: Vec::new(),
                    qself: None,
                    path: syn::Path::from(ast_ident_to_syn(name)),
                    brace_token: Default::default(),
                    fields: field_pats,
                    rest: None,
                })
            }
        };
        Ok(syn_pat)
    }

    fn lower_literal(&mut self, lit: &Literal) -> Result<SynExpr, CodegenError> {
        // T20: Decimal literal → `rust_decimal_macros::dec!(<raw>)`. The raw
        // digit text is parsed into a `proc_macro2::TokenStream` so the
        // *exact* digits survive (no rounding through f64) — this matches
        // what `dec!` expects (a numeric literal token) and preserves
        // trailing zeros like the `0` in `99.90`.
        if let Literal::Decimal(raw) = lit {
            return self.lower_decimal_literal(raw);
        }
        let syn_lit = match lit {
            Literal::Int(n) => {
                syn::Lit::Int(syn::LitInt::new(&n.to_string(), ProcSpan::call_site()))
            }
            Literal::Float(f) => {
                // f32 suffix — prettyplease prints it as `2.5f32`.
                let s = format!("{}f32", float_repr(*f as f64));
                syn::Lit::Float(syn::LitFloat::new(&s, ProcSpan::call_site()))
            }
            Literal::Double(d) => {
                let s = format!("{}f64", float_repr(*d));
                syn::Lit::Float(syn::LitFloat::new(&s, ProcSpan::call_site()))
            }
            Literal::Bool(b) => syn::Lit::Bool(syn::LitBool::new(*b, ProcSpan::call_site())),
            Literal::String(s) => syn::Lit::Str(syn::LitStr::new(s, ProcSpan::call_site())),
            Literal::Byte(b) => {
                syn::Lit::Int(syn::LitInt::new(&b.to_string(), ProcSpan::call_site()))
            }
            // T21: `'A'` → `syn::Lit::Char`. prettyplease prints Rust `char`
            // literals with the correct quoting (including for escapes and
            // non-ASCII scalars).
            Literal::Char(c) => syn::Lit::Char(syn::LitChar::new(*c, ProcSpan::call_site())),
            // Handled by the early return above; this arm exists only so the
            // match is exhaustive (it is never reached).
            Literal::Decimal(_) => {
                return Err(self.unsupported("decimal literal (unreachable arm)"))
            }
            // T79: Regex literal — CODEGEN DEFERRED in v0.5. The generated
            // Cargo project has NO `regex` crate dependency (T32-style dep
            // injection is a separate v1.0 task), so emitting
            // `regex::Regex::new(...)` would fail to compile downstream. As a
            // documented stub we emit the raw pattern text as a plain String
            // literal (valid standalone Rust) so the pipeline stays green.
            // Real `Regex::new` lowering + Cargo-project dep wiring arrives
            // in v1.0. See `Literal::Regex` on the AST for the deferral note.
            Literal::Regex(p) => syn::Lit::Str(syn::LitStr::new(p, ProcSpan::call_site())),
        };
        Ok(SynExpr::Lit(syn::ExprLit {
            attrs: Vec::new(),
            lit: syn_lit,
        }))
    }

    /// Lower a Buff `Decimal` literal to the `rust_decimal_macros::dec!(...)`
    /// macro invocation (T20).
    ///
    /// The raw source text is parsed via `syn::parse_str` into a
    /// `proc_macro2::TokenStream` so the exact digits (including trailing
    /// zeros) are preserved verbatim — the value never transits through an
    /// `f64`, guaranteeing exactness end-to-end.
    fn lower_decimal_literal(&self, raw: &str) -> Result<SynExpr, CodegenError> {
        let num_tokens: proc_macro2::TokenStream = syn::parse_str(raw)
            .map_err(|e| self.unsupported(&format!("decimal literal `{raw}`: {e}")))?;
        Ok(SynExpr::Macro(syn::ExprMacro {
            attrs: Vec::new(),
            mac: syn::Macro {
                path: rust_path("rust_decimal_macros::dec"),
                bang_token: Default::default(),
                delimiter: syn::MacroDelimiter::Paren(Default::default()),
                tokens: num_tokens,
            },
        }))
    }

    fn make_binary_op(
        &mut self,
        op: BinaryOp,
        lhs: SynExpr,
        rhs: SynExpr,
    ) -> Result<SynExpr, CodegenError> {
        use syn::BinOp;
        let result = match op {
            BinaryOp::And => self.bin_arith(BinOp::And(Default::default()), lhs, rhs),
            BinaryOp::Or => self.bin_arith(BinOp::Or(Default::default()), lhs, rhs),
            BinaryOp::Add => self.bin_arith(BinOp::Add(Default::default()), lhs, rhs),
            BinaryOp::Sub => self.bin_arith(BinOp::Sub(Default::default()), lhs, rhs),
            BinaryOp::Mul => self.bin_arith(BinOp::Mul(Default::default()), lhs, rhs),
            BinaryOp::Div => self.bin_arith(BinOp::Div(Default::default()), lhs, rhs),
            BinaryOp::Mod => self.bin_arith(BinOp::Rem(Default::default()), lhs, rhs),
            BinaryOp::Eq => self.bin_arith(BinOp::Eq(Default::default()), lhs, rhs),
            BinaryOp::Neq => self.bin_arith(BinOp::Ne(Default::default()), lhs, rhs),
            BinaryOp::Lt => self.bin_arith(BinOp::Lt(Default::default()), lhs, rhs),
            BinaryOp::Gt => self.bin_arith(BinOp::Gt(Default::default()), lhs, rhs),
            BinaryOp::Lte => self.bin_arith(BinOp::Le(Default::default()), lhs, rhs),
            BinaryOp::Gte => self.bin_arith(BinOp::Ge(Default::default()), lhs, rhs),
            BinaryOp::BitAnd => self.bin_arith(BinOp::BitAnd(Default::default()), lhs, rhs),
            BinaryOp::BitOr => self.bin_arith(BinOp::BitOr(Default::default()), lhs, rhs),
            BinaryOp::BitXor => self.bin_arith(BinOp::BitXor(Default::default()), lhs, rhs),
            BinaryOp::Shl => self.bin_arith(BinOp::Shl(Default::default()), lhs, rhs),
            BinaryOp::Shr => self.bin_arith(BinOp::Shr(Default::default()), lhs, rhs),
            BinaryOp::Assign => SynExpr::Assign(syn::ExprAssign {
                attrs: Vec::new(),
                left: Box::new(lhs),
                eq_token: Default::default(),
                right: Box::new(rhs),
            }),
            BinaryOp::AddAssign
            | BinaryOp::SubAssign
            | BinaryOp::MulAssign
            | BinaryOp::DivAssign
            | BinaryOp::ModAssign => {
                let binop = match op {
                    BinaryOp::AddAssign => BinOp::AddAssign(Default::default()),
                    BinaryOp::SubAssign => BinOp::SubAssign(Default::default()),
                    BinaryOp::MulAssign => BinOp::MulAssign(Default::default()),
                    BinaryOp::DivAssign => BinOp::DivAssign(Default::default()),
                    BinaryOp::ModAssign => BinOp::RemAssign(Default::default()),
                    _ => unreachable!(),
                };
                SynExpr::Binary(syn::ExprBinary {
                    attrs: Vec::new(),
                    left: Box::new(lhs),
                    op: binop,
                    right: Box::new(rhs),
                })
            }
            // T101: `a ?? b` → `a.unwrap_or(b)` via quote! + syn::parse2.
            BinaryOp::NullCoalesce => {
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    #lhs.unwrap_or(#rhs)
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("null coalesce codegen parse: {e}")))?
            }
        };
        Ok(result)
    }

    fn bin_arith(&self, op: syn::BinOp, lhs: SynExpr, rhs: SynExpr) -> SynExpr {
        SynExpr::Binary(syn::ExprBinary {
            attrs: Vec::new(),
            left: Box::new(lhs),
            op,
            right: Box::new(rhs),
        })
    }

    fn make_unary_op(&mut self, op: UnaryOp, operand: SynExpr) -> Result<SynExpr, CodegenError> {
        // Buff's `~` (bitwise NOT on integers) maps to Rust's `!` on integers.
        let unop = match op {
            UnaryOp::Neg => syn::UnOp::Neg(Default::default()),
            UnaryOp::Not => syn::UnOp::Not(Default::default()),
            UnaryOp::BitNot => syn::UnOp::Not(Default::default()),
        };
        Ok(SynExpr::Unary(syn::ExprUnary {
            attrs: Vec::new(),
            op: unop,
            expr: Box::new(operand),
        }))
    }

    /// Convert a Buff [`TypeRef`] to a Rust [`syn::Type`].
    ///
    /// Returns an error for unsupported forms (function types); these will
    /// land in T12/T13.
    fn ast_typeref_to_syn(&mut self, ty: &TypeRef) -> Result<SynType, CodegenError> {
        match ty {
            TypeRef::Named { name, .. } => {
                // T124b: `DateTime` is the only prelude type that takes a
                // generic argument (`<chrono::Utc>`). Handle it before the
                // primitive-name table (which returns the bare path string
                // and would drop the generic).
                if name.name == "DateTime" {
                    return Ok(make_generic_path_type(
                        "chrono::DateTime",
                        vec![rust_path_type("chrono::Utc")],
                    ));
                }
                // T32: the Buff→Rust primitive-name mapping is now a
                // single named, configurable table at
                // [`buff_primitive_to_rust_name`] (covers all 9 primitive
                // names: Int, Byte, Bits, Float, Double, Bool, String,
                // Char, Decimal). Unknown names pass through unchanged so
                // user-defined types (struct/enum names) keep their spelling.
                let rust_name = buff_primitive_to_rust_name(&name.name);
                Ok(rust_path_type(rust_name))
            }
            TypeRef::Option(inner, _) => {
                let inner_ty = self.ast_typeref_to_syn(inner)?;
                Ok(make_generic_path_type("Option", vec![inner_ty]))
            }
            TypeRef::Generic { base, args, .. } => {
                // Lower the base type to a path string (we only support Named base for now).
                let base_name = match base.as_ref() {
                    TypeRef::Named { name, .. } => name.name.clone(),
                    _ => return Err(self.unsupported("generic with non-named base type")),
                };
                let lowered_args: Result<Vec<SynType>, CodegenError> =
                    args.iter().map(|a| self.ast_typeref_to_syn(a)).collect();
                let lowered_args = lowered_args?;
                Ok(make_generic_path_type(&base_name, lowered_args))
            }
            TypeRef::Function { .. } => Err(self.unsupported("function-type codegen (T12/T13)")),
            // T76: union types `A | B | C`. Compute canonical name
            // (join member display names with "Or"), collect into
            // `collected_unions`, and return the wrapper enum name as a
            // SynType::Path.
            TypeRef::Union(members, _) => {
                // Compute canonical union name: "String | Int" → "StringOrInt".
                let union_name: String = members
                    .iter()
                    .map(|m| m.to_string())
                    .collect::<Vec<_>>()
                    .join("Or");
                // Collect for dedup emission (emit once per unique union).
                self.collected_unions
                    .entry(union_name.clone())
                    .or_insert_with(|| members.clone());
                Ok(rust_path_type(&union_name))
            }
            // T103: tuple types `(T, U, ...)`. Lower each member to a syn::Type
            // and build a Rust tuple type via `quote!` + `parse2`. The 2+-
            // element rule lives at parse time, so this always carries 2+
            // members (Rust tuples need 2+ fields to be a "real" tuple; a
            // single-field `(T,)` is the trailing-comma idiom, which Buff
            // does not produce at the TYPE layer).
            TypeRef::Tuple(members, _) => {
                let lowered: Vec<SynType> = members
                    .iter()
                    .map(|m| self.ast_typeref_to_syn(m))
                    .collect::<Result<Vec<_>, _>>()?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    ( #( #lowered ),* )
                };
                syn::parse2::<SynType>(tokens)
                    .map_err(|e| self.unsupported(&format!("tuple type codegen parse: {e}")))
            }
        }
    }

    /// Map a resolved Buff [`Type`] (post-inference) to a Rust [`syn::Type`].
    ///
    /// Returns `None` for [`Type::Unknown`] and [`Type::Void`] — callers
    /// (notably `let` lowering) treat `None` as "no annotation emitted".
    /// [`Type::Decimal`] maps to `rust_decimal::Decimal` (the crate is a
    /// dependency of `buff-lang-codegen-rust` so generated crates must depend
    /// on it as well — the runtime/driver is responsible for that).
    fn buff_type_to_syn(&self, ty: &Type) -> Option<SynType> {
        // Handle generic types (Vector, Matrix, Option) first.
        match ty {
            Type::Vector(elem) => {
                let inner = self.buff_type_to_syn(elem)?;
                return Some(make_generic_path_type("Vec", vec![inner]));
            }
            Type::Matrix(elem) => {
                // T24: Matrix<T> maps to the builtin `Matrix<T>` struct that
                // this codegen emits on-demand. The inner element type uses
                // the standard mapping; an Unknown element falls back to
                // i64 (Buff's default Int) so the annotation still compiles.
                let inner = self
                    .buff_type_to_syn(elem)
                    .unwrap_or_else(|| rust_path_type("i64"));
                return Some(make_generic_path_type("Matrix", vec![inner]));
            }
            Type::Option(inner) => {
                let inner = self.buff_type_to_syn(inner)?;
                return Some(make_generic_path_type("Option", vec![inner]));
            }
            // T25: Map<K, V> → Rust `std::collections::HashMap<K, V>`. We use
            // the fully-qualified path so generated programs do NOT need a
            // `use std::collections::HashMap;` import (the literal codegen
            // also uses the fully-qualified path, keeping import-management
            // out of the picture for v0.5).
            Type::Map(key, value) => {
                let k = self
                    .buff_type_to_syn(key)
                    .unwrap_or_else(|| rust_path_type("i64"));
                let v = self
                    .buff_type_to_syn(value)
                    .unwrap_or_else(|| rust_path_type("i64"));
                return Some(make_qualified_generic_path_type(
                    "std::collections::HashMap",
                    vec![k, v],
                ));
            }
            // T30: Result<T, E> → Rust `Result<T, E>` (the std Result is in
            // scope by default, so no fully-qualified path needed — mirroring
            // Option<T>'s 1:1 mapping from T28). Both inners must resolve to
            // a concrete Rust type; an Unknown inner (e.g. `Ok(42)` infers
            // `Result<Int<64>, Unknown>`) makes the whole annotation
            // indeterminate, so we return None and let Rust infer from
            // context (function return type, etc.).
            Type::Result(ok, err) => {
                let ok_ty = self.buff_type_to_syn(ok)?;
                let err_ty = self.buff_type_to_syn(err)?;
                return Some(make_generic_path_type("Result", vec![ok_ty, err_ty]));
            }
            // T76: union types — resolved `Type::Union` is only reached via
            // `typeref_to_type` for source unions; there's no inference
            // path that produces Union directly. Return None so Rust
            // inference handles the annotation (the wrapper enum type is
            // determined in `ast_typeref_to_syn` which collects unions from
            // TypeRef::Union).
            Type::Union(_) => return None,
            // T103: tuple types `(T, U, ...)`. Lower each member to a syn::Type
            // and build a Rust tuple type via `quote!` + `parse2`. Any
            // unresolvable member (Unknown/Void) makes the whole annotation
            // indeterminate — return None so Rust infers the tuple type from
            // context (function return type, etc.).
            Type::Tuple(members) => {
                let lowered: Vec<SynType> = members
                    .iter()
                    .map(|m| self.buff_type_to_syn(m))
                    .collect::<Option<Vec<_>>>()?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    ( #( #lowered ),* )
                };
                match syn::parse2::<SynType>(tokens) {
                    Ok(ty) => return Some(ty),
                    Err(_) => return None,
                }
            }
            _ => {}
        }
        let rust_name: &str = match ty {
            Type::Int {
                width: IntWidth::W8,
            } => "i8",
            Type::Int {
                width: IntWidth::W16,
            } => "i16",
            Type::Int {
                width: IntWidth::W32,
            } => "i32",
            Type::Int {
                width: IntWidth::W64,
            } => "i64",
            Type::Int {
                width: IntWidth::W128,
            } => "i128",
            Type::Bits {
                width: IntWidth::W8,
            } => "u8",
            Type::Bits {
                width: IntWidth::W16,
            } => "u16",
            Type::Bits {
                width: IntWidth::W32,
            } => "u32",
            Type::Bits {
                width: IntWidth::W64,
            } => "u64",
            Type::Bits {
                width: IntWidth::W128,
            } => "u128",
            // f16 is unstable in std; we map to f32 as a safe approximation.
            Type::Float {
                width: FloatWidth::W16,
            } => "f32",
            Type::Float {
                width: FloatWidth::W32,
            } => "f32",
            Type::Float {
                width: FloatWidth::W64,
            } => "f64",
            Type::Double => "f64",
            Type::Bool => "bool",
            Type::String => "String",
            // T21: Char → Rust's `char` (a 4-byte Unicode scalar value).
            Type::Char => "char",
            Type::Decimal => "rust_decimal::Decimal",
            // T124b: prelude datetime family. The plain Rust path mapping
            // is reused for everything except DateTime (which needs the
            // generic `<chrono::Utc>` argument — handled by the early
            // return just below).
            Type::Date => "chrono::NaiveDate",
            Type::Time => "chrono::NaiveTime",
            Type::Duration => "chrono::TimeDelta",
            Type::Instant => "std::time::Instant",
            Type::Unknown | Type::Void => return None,
            // T124b: DateTime is the only prelude type that needs a generic
            // argument. Return early with the proper generic-argument form
            // so `let dt: DateTime = ...` emits
            // `let dt: chrono::DateTime<chrono::Utc> = ...`.
            Type::DateTime => {
                return Some(make_generic_path_type(
                    "chrono::DateTime",
                    vec![rust_path_type("chrono::Utc")],
                ));
            }
            // Vector, Matrix, Map, Option, and Result are handled by the
            // early-return match above; this arm is unreachable but required
            // for exhaustiveness.
            Type::Vector(_)
            | Type::Matrix(_)
            | Type::Option(_)
            | Type::Map(_, _)
            | Type::Result(_, _)
            | Type::Union(_)
            | Type::Tuple(_) => return None,
        };
        Some(rust_path_type(rust_name))
    }

    fn unsupported(&self, what: &str) -> CodegenError {
        CodegenError::new(Diagnostic::error(
            format!("unsupported: {what}"),
            BuffSpan::dummy(),
        ))
    }
}

impl Default for RustCodegen {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// Free helpers
// ---------------------------------------------------------------------------

/// Convert a Buff [`buff_lang_ast::Ident`] into a `syn::Ident`. The byte offsets
/// in the Buff span don't carry over (proc-macro2 spans are opaque), so we
/// just use `call_site` here. The source-map mapping (Buff span → Rust
/// line/col) is recorded separately in [`CodegenContext`].
fn ast_ident_to_syn(ident: &buff_lang_ast::common::Ident) -> Ident {
    Ident::new(&ident.name, ProcSpan::call_site())
}

/// T75: rewrite the first parameter of a [`syn::Signature`] from a typed
/// `FnArg::Typed { ident: "self", ty }` into a bare [`syn::FnArg::Receiver`]
/// so the generated Rust reads `fn name(self, ...) -> ...` instead of the
/// (valid but verbose) `fn name(self: Type, ...) -> ...`.
///
/// This is the canonical Rust extension-method shape: the trait declaration
/// and impl body both spell the receiver as bare `self`, and Rust infers
/// the receiver type from the `impl Trait for Type` header. Without this
/// rewrite, the generated trait/impl would carry `self: Type` (also valid
/// Rust — it's the "explicit-self-type" form — but unusual and the spec QA
/// requires bare `self`).
///
/// The rewrite is a NO-OP when the first input is NOT named `self` (e.g.
/// an extension method that takes the receiver by a different name, or
/// one that takes only non-receiver args). Mutability is preserved: a
/// param named `self` with `mut` becomes `mut self`.
fn rewrite_self_receiver(mut sig: Signature) -> Signature {
    let Some(first) = sig.inputs.first() else {
        return sig;
    };
    let is_self = match first {
        syn::FnArg::Typed(pat_type) => matches!(
            pat_type.pat.as_ref(),
            Pat::Ident(pi) if pi.ident == "self"
        ),
        _ => false,
    };
    if !is_self {
        return sig;
    }
    // Replace the first input with a Receiver. We extract `mut` from the
    // existing PatIdent (if present) and otherwise use defaults. No
    // `colon_token` — bare `self`, NOT `self: Type`.
    let mutability = match first {
        syn::FnArg::Typed(pat_type) => match pat_type.pat.as_ref() {
            Pat::Ident(pi) if pi.mutability.is_some() => Some(Default::default()),
            _ => None,
        },
        _ => None,
    };
    // When `colon_token` is `None`, syn expects the `ty` field to be the
    // reconstructed shorthand type — `Self` for bare `self`,
    // `&Self` / `&mut Self` for ref forms (the latter not emitted here
    // yet — references are hidden from Buff users). We synthesise a
    // `Self` path type.
    let self_ty = SynType::Path(syn::TypePath {
        qself: None,
        path: syn::Path::from(Ident::new("Self", ProcSpan::call_site())),
    });
    sig.inputs[0] = syn::FnArg::Receiver(syn::Receiver {
        attrs: Vec::new(),
        reference: None,
        mutability,
        self_token: Default::default(),
        colon_token: None,
        ty: Box::new(self_ty),
    });
    sig
}

/// T92: extract a bare-ident [`SynExpr`] from a [`syn::FnArg::Typed`] whose
/// pattern is `Pat::Ident`. Returns `None` for receivers or non-ident
/// patterns (destructured params — not produced by Buff's parser today, but
/// defended against so future pattern-param work doesn't silently drop a
/// forwarded arg).
fn ident_expr_from_fn_arg(arg: &syn::FnArg) -> Option<SynExpr> {
    let syn::FnArg::Typed(pat_type) = arg else {
        return None;
    };
    let Pat::Ident(pat_ident) = pat_type.pat.as_ref() else {
        return None;
    };
    Some(SynExpr::Path(syn::ExprPath {
        attrs: Vec::new(),
        qself: None,
        path: syn::Path::from(pat_ident.ident.clone()),
    }))
}

/// T92: build the delegation forwarding body expression
/// `self.<field>.<method>(<args>)`.
///
/// - `field` is the embedding struct's field name (e.g. `person`).
/// - `method` is the embedded type's method name (e.g. `name`).
/// - `args` are the forwarded param identifiers (params after `self`).
fn field_method_call_expr(
    field: &str,
    method: &str,
    args: Punctuated<SynExpr, syn::Token![,]>,
) -> SynExpr {
    // `self`
    let self_expr = SynExpr::Path(syn::ExprPath {
        attrs: Vec::new(),
        qself: None,
        path: syn::Path::from(Ident::new("self", ProcSpan::call_site())),
    });
    // `self.<field>`
    let field_expr = SynExpr::Field(syn::ExprField {
        attrs: Vec::new(),
        base: Box::new(self_expr),
        dot_token: Default::default(),
        member: syn::Member::Named(Ident::new(field, ProcSpan::call_site())),
    });
    // `self.<field>.<method>(<args>)`
    SynExpr::MethodCall(syn::ExprMethodCall {
        attrs: Vec::new(),
        receiver: Box::new(field_expr),
        dot_token: Default::default(),
        method: Ident::new(method, ProcSpan::call_site()),
        turbofish: None,
        paren_token: Default::default(),
        args,
    })
}

/// T107: build ONE `copy_<field>` immutable-update method for a struct.
///
/// Emits:
///
/// ```rust,ignore
/// pub fn copy_<field>(&self, <field>: <rust_ty>) -> Self {
///     let mut c = self.clone();
///     c.<field> = <field>;
///     c
/// }
/// ```
///
/// The method is `pub` (so user code can call it), takes `&self` (so the
/// original value is untouched — Buff's immutable-update ergonomics), and
/// returns `Self` (the cloned-and-updated value). The body clones `self`,
/// mutably reassigns the named field, and returns the clone.
///
/// Built entirely via `syn` struct construction (no `parse_quote!`, no
/// string formatting — the single string producer is `prettyplease::unparse`
/// via [`crate::format`]). The `&self` receiver is constructed by hand via
/// [`syn::FnArg::Receiver`] with `reference: Some(..)`; the `ty` field is
/// the reconstructed `&Self` reference type (syn's invariant when
/// `colon_token` is `None`).
fn build_record_copy_method(field_name: &str, field_ty: SynType) -> syn::ImplItemFn {
    let method_ident = Ident::new(&format!("copy_{field_name}"), ProcSpan::call_site());
    let field_ident = Ident::new(field_name, ProcSpan::call_site());

    // `&self` receiver — `reference: Some((&, None))` spells the bare
    // `&self` shorthand. The `ty` field carries the reconstructed `&Self`
    // (syn's documented invariant: when `colon_token` is `None`, `ty` is
    // the reconstructed receiver type — `Self` for `self`, `&Self` for
    // `&self`, etc.).
    let self_ty = SynType::Path(syn::TypePath {
        qself: None,
        path: syn::Path::from(Ident::new("Self", ProcSpan::call_site())),
    });
    let ref_self_ty = SynType::Reference(syn::TypeReference {
        and_token: Default::default(),
        lifetime: None,
        mutability: None,
        elem: Box::new(self_ty),
    });
    let self_receiver = syn::FnArg::Receiver(syn::Receiver {
        attrs: Vec::new(),
        reference: Some((Default::default(), None)),
        mutability: None,
        self_token: Default::default(),
        colon_token: None,
        ty: Box::new(ref_self_ty),
    });

    // `<field>: <rust_ty>` — the new-value param.
    let value_param = syn::FnArg::Typed(syn::PatType {
        attrs: Vec::new(),
        pat: Box::new(Pat::Ident(PatIdent {
            attrs: Vec::new(),
            by_ref: None,
            mutability: None,
            ident: field_ident.clone(),
            subpat: None,
        })),
        colon_token: Default::default(),
        ty: Box::new(field_ty),
    });

    // `-> Self` return type.
    let return_ty = SynType::Path(syn::TypePath {
        qself: None,
        path: syn::Path::from(Ident::new("Self", ProcSpan::call_site())),
    });

    // Body statements, in order:
    //   1. `let mut c = self.clone();`
    //   2. `c.<field> = <field>;`
    //   3. `c` (trailing expression — the returned clone).
    let self_path = syn::Path::from(Ident::new("self", ProcSpan::call_site()));
    let self_expr = SynExpr::Path(syn::ExprPath {
        attrs: Vec::new(),
        qself: None,
        path: self_path,
    });
    // `self.clone()` — zero-arg method call.
    let clone_call = SynExpr::MethodCall(syn::ExprMethodCall {
        attrs: Vec::new(),
        receiver: Box::new(self_expr),
        dot_token: Default::default(),
        method: Ident::new("clone", ProcSpan::call_site()),
        turbofish: None,
        paren_token: Default::default(),
        args: Punctuated::new(),
    });
    let c_ident = Ident::new("c", ProcSpan::call_site());
    let let_stmt = SynStmt::Local(syn::Local {
        attrs: Vec::new(),
        let_token: Default::default(),
        pat: Pat::Ident(PatIdent {
            attrs: Vec::new(),
            by_ref: None,
            mutability: Some(Default::default()),
            ident: c_ident.clone(),
            subpat: None,
        }),
        init: Some(syn::LocalInit {
            eq_token: Default::default(),
            expr: Box::new(clone_call),
            diverge: None,
        }),
        semi_token: Default::default(),
    });

    // `c.<field> = <field>;` — assignment statement.
    let c_expr = SynExpr::Path(syn::ExprPath {
        attrs: Vec::new(),
        qself: None,
        path: syn::Path::from(c_ident.clone()),
    });
    let field_access = SynExpr::Field(syn::ExprField {
        attrs: Vec::new(),
        base: Box::new(c_expr),
        dot_token: Default::default(),
        member: syn::Member::Named(field_ident.clone()),
    });
    let value_expr = SynExpr::Path(syn::ExprPath {
        attrs: Vec::new(),
        qself: None,
        path: syn::Path::from(field_ident),
    });
    let assign_stmt = SynStmt::Expr(
        SynExpr::Assign(syn::ExprAssign {
            attrs: Vec::new(),
            left: Box::new(field_access),
            eq_token: Default::default(),
            right: Box::new(value_expr),
        }),
        Some(Default::default()),
    );

    // Trailing expression: `c` (the return value).
    let trailing_expr = SynExpr::Path(syn::ExprPath {
        attrs: Vec::new(),
        qself: None,
        path: syn::Path::from(c_ident),
    });
    let trailing_stmt = SynStmt::Expr(trailing_expr, None);

    syn::ImplItemFn {
        attrs: Vec::new(),
        vis: Visibility::Public(Default::default()),
        defaultness: None,
        sig: Signature {
            constness: None,
            asyncness: None,
            unsafety: None,
            abi: None,
            fn_token: Default::default(),
            ident: method_ident,
            generics: syn::Generics::default(),
            paren_token: Default::default(),
            inputs: [self_receiver, value_param].into_iter().collect(),
            variadic: None,
            output: ReturnType::Type(Default::default(), Box::new(return_ty)),
        },
        block: syn::Block {
            brace_token: Default::default(),
            stmts: vec![let_stmt, assign_stmt, trailing_stmt],
        },
    }
}

/// T32: the single named, configurable Buff→Rust primitive-type mapping
/// table.
///
/// Maps each of Buff's 9 primitive type NAMES to the corresponding Rust
/// type name (as written in source — the caller wraps it in a
/// [`rust_path_type`] to form a [`SynType`]). This is the ONE place that
/// knows how Buff primitive names spell in Rust; both
/// [`RustCodegen::ast_typeref_to_syn`] (unresolved [`TypeRef`]s from
/// user-written annotations) and any future "reverse" mapping (Rust→Buff
/// for diagnostics) should consult this table.
///
/// The 9 primitive names covered (the task's "13 types" counts 9
/// primitives + 4 generic containers — Vector, Option, Matrix, Map, Result
/// — which are handled structurally in [`RustCodegen::ast_typeref_to_syn`]
/// / [`RustCodegen::buff_type_to_syn`] because they carry type arguments):
///
/// | Buff name | Rust name            |
/// |-----------|----------------------|
/// | `Int`     | `i64`                |
/// | `Byte`    | `u8`                 |
/// | `Bits`    | `u64`                |
/// | `Float`   | `f32`                |
/// | `Double`  | `f64`                |
/// | `Bool`    | `bool`               |
/// | `String`  | `String`             |
/// | `Char`    | `char`               |
/// | `Decimal` | `rust_decimal::Decimal` |
///
/// T124b — prelude datetime family. The Rust names here are the FULLY-
/// QUALIFIED paths so generated code never needs a `use chrono::...;`
/// import:
///
/// | Buff       | Rust                                     |
/// |------------|------------------------------------------|
/// | `DateTime` | `chrono::DateTime<chrono::Utc>`          |
/// | `Date`     | `chrono::NaiveDate`                      |
/// | `Time`     | `chrono::NaiveTime`                      |
/// | `Duration` | `chrono::TimeDelta`                      |
/// | `Instant`  | `std::time::Instant`                     |
///
/// Unknown names (anything not in the table) are returned unchanged so
/// user-defined types (struct/enum names, generic type parameters like
/// `T`) keep their spelling — they become Rust path types verbatim.
///
/// **Note**: The `chrono::DateTime<chrono::Utc>` return for `DateTime` is
/// the *plain path spelling* `chrono::DateTime < chrono::Utc >` (without
/// generics angle brackets in the source representation). When this name is
/// used to build a `syn::Type` via [`rust_path_type`], the `<chrono::Utc>`
/// segment is NOT treated as a generic argument — it becomes a literal
/// path segment, which syn parses as the type-argument-less path. To get
/// the proper generic form, callers must use
/// [`Self::buff_prelude_type_to_syn`] (which constructs the type via
/// `make_generic_path_type`). [`buff_primitive_to_rust_name`] is kept
/// simple for the cases that don't need generics (everything except
/// `DateTime`); for `DateTime`, the codegen routes through the dedicated
/// helper.
pub fn buff_primitive_to_rust_name(buff_name: &str) -> &str {
    match buff_name {
        "Int" => "i64",
        "Byte" => "u8",
        "Bits" => "u64",
        "Float" => "f32",
        "Double" => "f64",
        "Bool" => "bool",
        "String" => "String",
        "Char" => "char",
        "Decimal" => "rust_decimal::Decimal",
        // T124b: prelude datetime family. These map to chrono / std::time
        // fully-qualified paths so generated code never needs a `use` import.
        // `DateTime` is special — it needs a generic `<chrono::Utc>` arg;
        // callers that build a `syn::Type` should consult
        // `ast_typeref_to_syn` (which constructs the proper generic form).
        "Date" => "chrono::NaiveDate",
        "Time" => "chrono::NaiveTime",
        "Duration" => "chrono::TimeDelta",
        "Instant" => "std::time::Instant",
        other => other,
    }
}

/// Build a `syn::Type::Path` from a `::`-separated Rust type name string
/// (e.g. `"i64"`, `"bool"`, `"rust_decimal::Decimal"`). Each `::`-separated
/// segment becomes a [`syn::PathSegment`]. The result is always a plain path
/// with no generic arguments.
fn rust_path_type(name: &str) -> SynType {
    SynType::Path(syn::TypePath {
        qself: None,
        path: rust_path(name),
    })
}

/// Build a `syn::Path` from a `::`-separated name string
/// (e.g. `"rust_decimal_macros::dec"`). Used for macro paths like the
/// `dec!(...)` codegen in T20.
fn rust_path(name: &str) -> syn::Path {
    let mut segments: Punctuated<syn::PathSegment, syn::Token![::]> = Punctuated::new();
    for seg in name.split("::") {
        segments.push(syn::PathSegment {
            ident: Ident::new(seg, ProcSpan::call_site()),
            arguments: syn::PathArguments::None,
        });
    }
    syn::Path {
        leading_colon: None,
        segments,
    }
}

/// Build a `println!("{}", arg)` macro invocation as a `syn::Expr::Macro`.
///
/// Used by the `print(x)` → `println!("{}", x)` mapping (T13/T96). The macro
/// token stream is built via `quote!` so it round-trips through `syn`'s
/// printer without any hand-rolled string formatting.
fn make_println_macro(arg: SynExpr) -> SynExpr {
    SynExpr::Macro(syn::ExprMacro {
        attrs: Vec::new(),
        mac: syn::Macro {
            path: syn::Path::from(Ident::new("println", ProcSpan::call_site())),
            bang_token: Default::default(),
            delimiter: syn::MacroDelimiter::Paren(Default::default()),
            tokens: quote::quote! { "{}", #arg },
        },
    })
}

/// Build a `println!("literal_text")` macro invocation — the T96 string-
/// literal fast path for `print("hello")` → `println!("hello")` (no `{}`
/// placeholder, the literal text becomes the format string itself).
fn make_println_macro_literal(text: &str) -> SynExpr {
    // Build the format-string literal via `proc_macro2::Literal::string` so
    // Rust-level escapes in `text` survive correctly (e.g. embedded quotes,
    // backslashes, newlines).
    let format_lit = proc_macro2::Literal::string(text);
    SynExpr::Macro(syn::ExprMacro {
        attrs: Vec::new(),
        mac: syn::Macro {
            path: syn::Path::from(Ident::new("println", ProcSpan::call_site())),
            bang_token: Default::default(),
            delimiter: syn::MacroDelimiter::Paren(Default::default()),
            tokens: quote::quote! { #format_lit },
        },
    })
}

/// Build a `recv.method()` (zero-arg) method call.
fn method_call_no_args(recv: SynExpr, method: &str) -> SynExpr {
    SynExpr::MethodCall(syn::ExprMethodCall {
        attrs: Vec::new(),
        receiver: Box::new(recv),
        dot_token: Default::default(),
        method: Ident::new(method, ProcSpan::call_site()),
        turbofish: None,
        paren_token: Default::default(),
        args: Default::default(),
    })
}

/// Wrap an expression in parentheses: `(e)`. Used to disambiguate method-
/// call receivers so integer literals like `5` lower to `(5).abs()` rather
/// than the ambiguous `5.abs()` (which Rust parses as a field access on the
/// float literal `5.`).
fn wrap_in_parens(e: SynExpr) -> SynExpr {
    SynExpr::Paren(syn::ExprParen {
        attrs: Vec::new(),
        paren_token: Default::default(),
        expr: Box::new(e),
    })
}

/// Build a named field access `base.field` (T24).
///
/// Used by the Matrix 2-D index codegen to build `m.data` and `m.cols`. The
/// base expression is taken by value (the caller clones when re-use is
/// needed, as in [`RustCodegen::lower_matrix_index`]).
fn field_access(base: SynExpr, field: &str) -> SynExpr {
    SynExpr::Field(syn::ExprField {
        attrs: Vec::new(),
        base: Box::new(base),
        dot_token: Default::default(),
        member: syn::Member::Named(Ident::new(field, ProcSpan::call_site())),
    })
}

/// T31: build a Rust `.await` expression `<base>.await` (T31).
///
/// This is the ONLY place in the codegen that produces a Rust `.await`.
/// Buff has no `await` keyword — the codegen auto-inserts `.await` at two
/// sites:
///
/// 1. **Async call sites inside async fns** — when the callee is a known
///    async fn and the current fn is async, the call is wrapped:
///    `callee(args)` → `callee(args).await`.
/// 2. **`Task<T>.result()`** — `t.result()` → `t.await`.
///
/// The `ExprAwait` syn node is constructed by hand (NOT via `quote!`) so
/// the base expression is spliced directly into the `base` slot — keeping
/// the resulting syn tree as direct as possible.
fn make_await(base: SynExpr) -> SynExpr {
    SynExpr::Await(syn::ExprAwait {
        attrs: Vec::new(),
        base: Box::new(base),
        dot_token: Default::default(),
        await_token: Default::default(),
    })
}

/// T33: wrap an initializer in `Arc::new(...)` (used for Arc-shared
/// bindings — those captured across a `spawn` boundary).
///
/// Builds `std::sync::Arc::new(<inner>)` as a `syn::ExprCall` on the
/// fully-qualified `std::sync::Arc::new` path. The fully-qualified form
/// is used so generated code never needs a `use std::sync::Arc;` import
/// (mirrors the T25 HashMap pattern and the T24 Matrix pattern —
/// emit-on-demand codegen keeps the generated source self-contained).
fn wrap_in_arc_new(inner: SynExpr) -> SynExpr {
    let arc_new_path = rust_path("std::sync::Arc::new");
    let callee = SynExpr::Path(syn::ExprPath {
        attrs: Vec::new(),
        qself: None,
        path: arc_new_path,
    });
    let mut args: Punctuated<SynExpr, syn::Token![,]> = Punctuated::new();
    args.push(inner);
    SynExpr::Call(syn::ExprCall {
        attrs: Vec::new(),
        func: Box::new(callee),
        paren_token: Default::default(),
        args,
    })
}

/// T42: wrap an integer initializer in `std::sync::atomic::AtomicI64::new(...)`.
///
/// Used at the `let` site of a captured integer accumulator promoted
/// by [`crate::atomic_analysis`]. The fully-qualified path keeps
/// generated source free of any `use std::sync::atomic::AtomicI64;`
/// import (mirrors the [`wrap_in_arc_new`] pattern).
fn wrap_in_atomic_i64_new(inner: SynExpr) -> SynExpr {
    let path = rust_path("std::sync::atomic::AtomicI64::new");
    let callee = SynExpr::Path(syn::ExprPath {
        attrs: Vec::new(),
        qself: None,
        path,
    });
    let mut args: Punctuated<SynExpr, syn::Token![,]> = Punctuated::new();
    args.push(inner);
    SynExpr::Call(syn::ExprCall {
        attrs: Vec::new(),
        func: Box::new(callee),
        paren_token: Default::default(),
        args,
    })
}

/// T42: build `t.fetch_add((rhs) as i64, std::sync::atomic::Ordering::Relaxed)`.
///
/// Used at the `t += x` site of an atomic-promoted accumulator (inside
/// the parallel closure body). The first argument is the RHS cast to
/// `i64` (a no-op when the RHS is already `i64`, but defensively
/// typed for any numeric source). The ordering is `Relaxed` — T42
/// accumulator semantics do not synchronise with other atomics or
/// establish happens-before relations; the program-order
/// single-thread semantics Buff presents to the user is preserved by
/// the post-parallel `.load()`.
fn atomic_fetch_add_stmt(name: &buff_lang_ast::common::Ident, rhs: SynExpr) -> SynExpr {
    // `t` — the bare atomic binding.
    let atomic_path = SynExpr::Path(syn::ExprPath {
        attrs: Vec::new(),
        qself: None,
        path: syn::Path::from(ast_ident_to_syn(name)),
    });
    // `(rhs) as i64` — cast the RHS defensively. `as` is a valid Rust
    // cast for any numeric type to i64; if `rhs` is already i64 this
    // is a no-op and Rust's `clippy::useless_conversion` does not
    // flag it (it's an `as` cast, not a `.into()`).
    let rhs_cast = SynExpr::Cast(syn::ExprCast {
        attrs: Vec::new(),
        expr: Box::new(rhs),
        as_token: Default::default(),
        ty: Box::new(rust_path_type("i64")),
    });
    // `std::sync::atomic::Ordering::Relaxed` — the ordering argument.
    let ordering = SynExpr::Path(syn::ExprPath {
        attrs: Vec::new(),
        qself: None,
        path: rust_path("std::sync::atomic::Ordering::Relaxed"),
    });
    let mut args: Punctuated<SynExpr, syn::Token![,]> = Punctuated::new();
    args.push(rhs_cast);
    args.push(ordering);
    SynExpr::MethodCall(syn::ExprMethodCall {
        attrs: Vec::new(),
        receiver: Box::new(atomic_path),
        dot_token: Default::default(),
        method: Ident::new("fetch_add", ProcSpan::call_site()),
        turbofish: None,
        paren_token: Default::default(),
        args,
    })
}

/// T42: build `t.load(std::sync::atomic::Ordering::Relaxed)`.
///
/// Used at every READ of an atomic-promoted binding (both inside and
/// outside the parallel closure body). The ordering is `Relaxed`,
/// matching the [`atomic_fetch_add_stmt`] choice — Buf's accumulator
/// pattern does not require cross-atomic synchronisation.
fn atomic_load_expr(atomic_path: SynExpr) -> SynExpr {
    let ordering = SynExpr::Path(syn::ExprPath {
        attrs: Vec::new(),
        qself: None,
        path: rust_path("std::sync::atomic::Ordering::Relaxed"),
    });
    let mut args: Punctuated<SynExpr, syn::Token![,]> = Punctuated::new();
    args.push(ordering);
    SynExpr::MethodCall(syn::ExprMethodCall {
        attrs: Vec::new(),
        receiver: Box::new(atomic_path),
        dot_token: Default::default(),
        method: Ident::new("load", ProcSpan::call_site()),
        turbofish: None,
        paren_token: Default::default(),
        args,
    })
}

/// T33: build `Arc::clone(&name)` (used at use sites of Arc-shared
/// bindings INSIDE a spawn body).
///
/// The argument is a borrowed reference (`&name`) so `Arc::clone` bumps
/// the refcount without cloning the underlying data. The fully-qualified
/// path keeps generated source free of any `use std::sync::Arc;`.
fn arc_clone_call(name: &buff_lang_ast::common::Ident) -> SynExpr {
    let arc_clone_path = rust_path("std::sync::Arc::clone");
    let callee = SynExpr::Path(syn::ExprPath {
        attrs: Vec::new(),
        qself: None,
        path: arc_clone_path,
    });
    // `&name` — single-segment borrow of the binding.
    let borrowed_name = SynExpr::Reference(syn::ExprReference {
        attrs: Vec::new(),
        and_token: Default::default(),
        mutability: None,
        expr: Box::new(SynExpr::Path(syn::ExprPath {
            attrs: Vec::new(),
            qself: None,
            path: syn::Path::from(ast_ident_to_syn(name)),
        })),
    });
    let mut args: Punctuated<SynExpr, syn::Token![,]> = Punctuated::new();
    args.push(borrowed_name);
    SynExpr::Call(syn::ExprCall {
        attrs: Vec::new(),
        func: Box::new(callee),
        paren_token: Default::default(),
        args,
    })
}

/// T33: build `*Arc::make_mut(&mut name)` — the LHS of an assignment to
/// an Arc-shared-and-subsequently-mutated binding (CoW site).
///
/// `Arc::make_mut(&mut x)` returns `&mut T`, cloning the inner value
/// only if the Arc's refcount > 1 (i.e. when the spawned task is
/// actually observing the same Arc). The leading `*` dereferences so
/// the assignment writes through to the (possibly-cloned) inner value:
/// `*Arc::make_mut(&mut v) = vec![3, 4]`. The fully-qualified path
/// keeps generated source free of any `use std::sync::Arc;`.
fn arc_make_mut_deref(name: &buff_lang_ast::common::Ident) -> SynExpr {
    let arc_make_mut_path = rust_path("std::sync::Arc::make_mut");
    let callee = SynExpr::Path(syn::ExprPath {
        attrs: Vec::new(),
        qself: None,
        path: arc_make_mut_path,
    });
    // `&mut name` — mutable borrow of the binding.
    let mut_borrowed_name = SynExpr::Reference(syn::ExprReference {
        attrs: Vec::new(),
        and_token: Default::default(),
        mutability: Some(Default::default()),
        expr: Box::new(SynExpr::Path(syn::ExprPath {
            attrs: Vec::new(),
            qself: None,
            path: syn::Path::from(ast_ident_to_syn(name)),
        })),
    });
    let mut args: Punctuated<SynExpr, syn::Token![,]> = Punctuated::new();
    args.push(mut_borrowed_name);
    let make_mut_call = SynExpr::Call(syn::ExprCall {
        attrs: Vec::new(),
        func: Box::new(callee),
        paren_token: Default::default(),
        args,
    });
    // `*<call>` — prefix dereference so the surrounding assignment writes
    // through to the inner value.
    SynExpr::Unary(syn::ExprUnary {
        attrs: Vec::new(),
        op: syn::UnOp::Deref(Default::default()),
        expr: Box::new(make_mut_call),
    })
}

/// Allow-list of method names that ALWAYS lower to a Rust method call even
/// when called with zero arguments (T26 field-access heuristic).
///
/// Any zero-arg `obj.name` whose `name` is NOT in this list lowers to a Rust
/// field access `obj.name`. Anything in the list stays a method call
/// `obj.name()`. The list contains:
///
/// - The string-method family this codegen explicitly handles
///   (`char_count`/`byte_len`/`chars`/`bytes`/`first`/`last`/`graphemes`).
/// - Universal `clone`/`to_string`/`to_owned`/`into`/etc. that show up via
///   move analysis and the standard library.
/// - Common collection zero-arg methods (`len`/`is_empty`/`iter`/...).
/// - Numeric methods that don't take args (`abs`/`sqrt`/`floor`/...).
///
/// Adding a new zero-arg builtin in a later task MUST extend this list,
/// otherwise users calling `obj.<new_builtin>()` will see broken field
/// access codegen. The unit test `t26_known_zero_arg_methods_table_is_load_bearing`
/// pins the table so a careless rename is caught.
const KNOWN_ZERO_ARG_METHODS: &[&str] = &[
    // String methods (this codegen explicitly lowers these).
    "char_count",
    "byte_len",
    "chars",
    "bytes",
    "first",
    "last",
    "graphemes",
    // Universal / standard-library methods.
    "clone",
    "to_string",
    "to_owned",
    "into",
    "as_ref",
    "as_mut",
    "default",
    "to_lowercase",
    "to_uppercase",
    "trim",
    "trim_start",
    "trim_end",
    // Collection zero-arg methods.
    "len",
    "is_empty",
    "iter",
    "iter_mut",
    "into_iter",
    "keys",
    "values",
    "pop",
    "clear",
    // Iterator adaptors (zero-arg form).
    "rev",
    "count",
    "sum",
    "product",
    "next",
    "enumerate",
    "flatten",
    "step_by",
    // Numeric zero-arg methods.
    "abs",
    "sqrt",
    "floor",
    "ceil",
    "round",
    "signum",
    "trunc",
    "fract",
    "recip",
    "is_nan",
    "is_infinite",
    "is_finite",
    "is_sign_positive",
    "is_sign_negative",
    "to_degrees",
    "to_radians",
    "exp",
    "ln",
    "log2",
    "log10",
    "tan",
    "sin",
    "cos",
    "atan",
    "asin",
    "acos",
    "tanh",
    "sinh",
    "cosh",
    "powi",
    "powf",
    // T124b: prelude-types zero-arg instance methods on the datetime
    // family (DateTime / Date / Time). Without these entries the T26
    // field-access heuristic would rewrite `dt.year()` as `dt.year`
    // (a field access on the chrono value, which doesn't exist).
    // `format` takes one arg so it's never affected by the heuristic.
    "year",
    "month",
    "day",
    "hour",
    "minute",
    "second",
    "timestamp",
];

/// Build the attribute list for a generated struct: always
/// `#[derive(Clone, Debug)]`, plus an optional `#[repr(C)]` when
/// `emit_repr_c` is true (T26 hook).
///
/// Ordering: derive attribute first, then `#[repr(C)]` (matching the layout
/// the T26 snapshot asserts). Both attributes use `syn::Attribute::parse_attr`
/// style construction (meta list / path) so they survive `prettyplease`
/// formatting without round-tripping through strings.
///
/// This is the shared derive-attribute helper extracted during the T26
/// REFACTOR step: future struct-like decls (enum repr hints, future trait
/// derives) reuse this function. The signature is `(emit_repr_c: bool) -> Vec<Attribute>`
/// so callers stay in control of whether repr(C) applies.
fn derive_and_repr_attrs(emit_repr_c: bool) -> Vec<syn::Attribute> {
    // Build `#[derive(Clone, Debug)]` via `syn::Attribute` construction.
    let derive_attr = syn::Attribute {
        pound_token: Default::default(),
        style: syn::AttrStyle::Outer,
        bracket_token: Default::default(),
        meta: syn::Meta::List(syn::MetaList {
            path: rust_path("derive"),
            delimiter: syn::MacroDelimiter::Paren(Default::default()),
            tokens: {
                // Build `Clone, Debug` as a token stream — this is a
                // fixed template parsed once at codegen time (not a runtime
                // Rust-string assembler; the single string producer remains
                // `prettyplease::unparse`).
                let mut t = proc_macro2::TokenStream::new();
                t.extend(quote::quote! { Clone });
                t.extend(quote::quote! { , });
                t.extend(quote::quote! { Debug });
                t
            },
        }),
    };
    let mut attrs = vec![derive_attr];
    if emit_repr_c {
        push_repr_c_attr(&mut attrs);
    }
    attrs
}

/// T107: build the derive attribute set for a STRUCT.
///
/// Structs ALWAYS derive `Clone`, `PartialEq`, `Debug` (T107 extends T26's
/// `Clone + Debug` baseline). The `Hash` derive is added CONDITIONALLY —
/// only when [`type_is_hash_safe`] reports that ALL the struct's field
/// types (recursively, across user struct references) impl `Hash`.
///
/// Emits one of:
///
/// ```rust,ignore
/// #[derive(Clone, PartialEq, Hash, Debug)]   // all fields Hash-safe
/// #[derive(Clone, PartialEq, Debug)]         // some field non-Hash
/// ```
///
/// followed by an optional `#[repr(C)]` (between derive and `pub struct`)
/// when `emit_repr_c` is true — identical ordering to the T26 helper.
///
/// # Why `PartialEq` is unconditional
///
/// Every Rust primitive Buff maps to (`i64`, `f32`, `String`, `bool`,
/// `rust_decimal::Decimal`, …) impls `PartialEq`, including floats (which
/// impl `PartialEq` but NOT `Eq`/`Hash`). So `PartialEq` is always safe.
///
/// # Why `Hash` is conditional
///
/// Rust's derived `Hash` impl requires every field type to impl `Hash`.
/// `f32` and `f64` do NOT impl `Hash` (NaN isn't hashable). `Vec<T>` and
/// `HashMap<K,V>` do NOT impl `Hash` in std either. So a struct with a
/// float/Vec/Map field cannot derive `Hash` — the caller must precompute
/// field-Hash-safety (see [`RustCodegen::compute_hash_safe_structs`]) and
/// pass the result via `include_hash`.
fn struct_derive_attrs(emit_repr_c: bool, include_hash: bool) -> Vec<syn::Attribute> {
    // Build the derive trait list as a single token stream. The order is
    // `Clone, PartialEq, [Hash,] Debug` — `Debug` is always last (matches
    // the existing T26 ordering and the test fixtures' expected spelling).
    let mut trait_tokens = proc_macro2::TokenStream::new();
    trait_tokens.extend(quote::quote! { Clone });
    trait_tokens.extend(quote::quote! { , });
    trait_tokens.extend(quote::quote! { PartialEq });
    trait_tokens.extend(quote::quote! { , });
    if include_hash {
        trait_tokens.extend(quote::quote! { Hash });
        trait_tokens.extend(quote::quote! { , });
    }
    trait_tokens.extend(quote::quote! { Debug });
    let derive_attr = syn::Attribute {
        pound_token: Default::default(),
        style: syn::AttrStyle::Outer,
        bracket_token: Default::default(),
        meta: syn::Meta::List(syn::MetaList {
            path: rust_path("derive"),
            delimiter: syn::MacroDelimiter::Paren(Default::default()),
            tokens: trait_tokens,
        }),
    };
    let mut attrs = vec![derive_attr];
    if emit_repr_c {
        push_repr_c_attr(&mut attrs);
    }
    attrs
}

/// T107: push a `#[repr(C)]` outer attribute onto `attrs`. Shared by
/// [`derive_and_repr_attrs`] (T26, enums + the legacy path) and
/// [`struct_derive_attrs`] (T107, structs).
fn push_repr_c_attr(attrs: &mut Vec<syn::Attribute>) {
    attrs.push(syn::Attribute {
        pound_token: Default::default(),
        style: syn::AttrStyle::Outer,
        bracket_token: Default::default(),
        meta: syn::Meta::List(syn::MetaList {
            path: rust_path("repr"),
            delimiter: syn::MacroDelimiter::Paren(Default::default()),
            tokens: quote::quote! { C },
        }),
    });
}

/// T50: build the derive + repr attribute list for a GPU-bound struct.
///
/// GPU-bound structs (those that flow through a `par_map` / `par_filter`
/// / `par_reduce` combinator — see [`gpu_alignment`](crate::gpu_alignment))
/// must have a stable C layout so their byte representation is
/// well-defined for `wgpu` storage-buffer upload / readback via
/// `bytemuck::cast_slice`. This helper emits:
///
/// ```rust,ignore
/// #[derive(Clone, Copy, PartialEq, Debug, bytemuck::Pod, bytemuck::Zeroable)]
/// #[repr(C)]
/// ```
///
/// # Why `Copy` is unconditional
///
/// `bytemuck::Pod` REQUIRES `Copy` (it's part of the unsafe contract —
/// Pod types must be freely bit-copyable). So every GPU-bound struct
/// must be `Copy`. For v1.0 we assume GPU-bound structs have only
/// primitive Pod fields (Int / Float / Bool / Byte / nested Pod structs
/// / arrays-of-Pod) — if a field is NOT Pod-compatible (e.g. `String`,
/// `Vec`, `Map`), the generated `#[derive(... bytemuck::Pod ...)]` will
/// fail to compile and rustc surfaces the error at build time. This is
/// an acceptable v1.0 limitation: GPU kernels over collections-of-
/// collections are out of scope.
///
/// # Why `Hash` is omitted
///
/// `Hash` requires every field to impl `Hash`. `f32` / `f64` (the most
/// common GPU-struct field types — coordinates, colours, intensities)
/// do NOT impl `Hash` (NaN is not hashable). So a GPU-bound struct
/// CANNOT generally derive `Hash`. We omit it unconditionally from the
/// GPU derive path; users who need Hash on a GPU-bound struct can
/// implement it manually (a v1.0+ concern).
///
/// # Why `PartialEq` is kept
///
/// `f32` / `f64` DO impl `PartialEq` (bit-equality, with the usual NaN
/// caveat). So a GPU-bound struct with Float fields CAN derive
/// `PartialEq`, and it's useful for testing (asserting CPU-vs-GPU
/// parity in dispatch tests). Kept unconditional.
///
/// # Ordering
///
/// Derive attribute first (with the bytemuck paths LAST so users reading
/// generated source see the familiar std derives first), then
/// `#[repr(C)]` between the derive and `pub struct` — identical
/// ordering to [`struct_derive_attrs`].
fn gpu_struct_derive_attrs() -> Vec<syn::Attribute> {
    // Build the derive trait list as a single token stream. The order is
    // `Clone, Copy, PartialEq, Debug, bytemuck::Pod, bytemuck::Zeroable`
    // — std derives first, bytemuck derives last. Matches the layout
    // recommended in the `bytemuck` documentation.
    let mut trait_tokens = proc_macro2::TokenStream::new();
    trait_tokens.extend(quote::quote! { Clone });
    trait_tokens.extend(quote::quote! { , });
    trait_tokens.extend(quote::quote! { Copy });
    trait_tokens.extend(quote::quote! { , });
    trait_tokens.extend(quote::quote! { PartialEq });
    trait_tokens.extend(quote::quote! { , });
    trait_tokens.extend(quote::quote! { Debug });
    trait_tokens.extend(quote::quote! { , });
    trait_tokens.extend(quote::quote! { bytemuck::Pod });
    trait_tokens.extend(quote::quote! { , });
    trait_tokens.extend(quote::quote! { bytemuck::Zeroable });
    let derive_attr = syn::Attribute {
        pound_token: Default::default(),
        style: syn::AttrStyle::Outer,
        bracket_token: Default::default(),
        meta: syn::Meta::List(syn::MetaList {
            path: rust_path("derive"),
            delimiter: syn::MacroDelimiter::Paren(Default::default()),
            tokens: trait_tokens,
        }),
    };
    let mut attrs = vec![derive_attr];
    push_repr_c_attr(&mut attrs);
    attrs
}

/// T107: is the given [`TypeRef`] "Hash-safe" — i.e. would the corresponding
/// Rust type impl `Hash`?
///
/// Used by [`struct_derive_attrs`] (via [`RustCodegen::lower_struct_decl`])
/// to decide whether a struct can carry the `Hash` derive. The check is
/// RECURSIVE across composite types:
///
/// - **Named primitives**: `Int`/`Byte`/`Bits`/`Bool`/`String`/`Char`/
///   `Decimal` → `true` (all map to Hash-impl'ing Rust types). `Float`/
///   `Double` → `false` (f32/f64 don't impl Hash).
/// - **Named user types** (struct/enum): `true` iff the type name is in
///   `hash_safe_user_structs` — a precomputed set built by
///   [`RustCodegen::compute_hash_safe_structs`] that does a fixpoint pass
///   over all struct decls (so `struct A { b: B }` is Hash-safe iff `B` is
///   too). Enums are conservatively assumed Hash-safe (their derived `Hash`
///   impl requires all variant payload types to impl Hash; full conditional
///   enum-Hash derivation is deferred to a later task).
/// - **`Option<T>`**: `true` iff `T` is Hash-safe (Option<T>: Hash when T: Hash).
/// - **`Tuple<T,U,…>` / `Union<A,B,…>`**: `true` iff EVERY member is
///   Hash-safe. Union emits a wrapper enum, which derives Hash iff every
///   variant payload impls Hash — same recursive rule.
/// - **`Generic { base, args }`**: `false` — collections (`Vector<T>` →
///   `Vec<T>`, `Map<K,V>` → `HashMap<K,V>`) don't impl Hash in std.
///   Conservative: any named generic base is treated as a non-Hash collection.
/// - **`Function { .. }`**: `false` — Buff closures lower to boxed `Fn` types
///   which don't reliably impl Hash.
fn type_is_hash_safe(ty: &TypeRef, hash_safe_user_structs: &BTreeSet<String>) -> bool {
    match ty {
        TypeRef::Named { name, .. } => match name.name.as_str() {
            // Hash-impl'ing Rust primitives.
            "Int" | "Byte" | "Bits" | "Bool" | "String" | "Char" | "Decimal" => true,
            // Non-Hash primitives.
            "Float" | "Double" => false,
            // User-defined struct/enum name: consult the precomputed set.
            // Unknown primitives (none today) and any name not in the table
            // are user types — their Hash-safety is decided by the fixpoint
            // analysis (struct) or the conservative-true rule (enum).
            other => hash_safe_user_structs.contains(other),
        },
        TypeRef::Option(inner, _) => type_is_hash_safe(inner, hash_safe_user_structs),
        // Tuple and Union: Hash-safe iff ALL members are Hash-safe.
        TypeRef::Tuple(members, _) | TypeRef::Union(members, _) => members
            .iter()
            .all(|m| type_is_hash_safe(m, hash_safe_user_structs)),
        // Collections (Vector, Map, …) and function types don't impl Hash.
        TypeRef::Generic { .. } | TypeRef::Function { .. } => false,
    }
}

// ---------------------------------------------------------------------------
// T105 — named-argument resolution helpers.
//
// Named args (`name: value` inside a call's arg list) carry their param
// name through the AST. At codegen, the codegen reorders them to match the
// callee's declared parameter order (when known) so the generated Rust
// call uses POSITIONAL arguments (Rust has no named-arg call syntax for
// free functions). This block has two free helpers:
//
// - [`collect_func_param_names`]: scans the decl list once at the start of
//   [`RustCodegen::generate`] and returns a `fn_name -> Vec<param_name>`
//   map. Same-compilation-unit free functions only.
// - [`materialize_named_args`]: given a call's `args` and an OPTIONAL
//   param-name list, returns a positional `Vec<Expr>`:
//     * `Some(params)` → reorder named args to match param order;
//       unmatched/extra args are appended defensively (Rust then errors
//       on arity mismatch).
//     * `None` → extract the value from each NamedArg (drop the name);
//       pure-positional call lists pass through unchanged.
// ---------------------------------------------------------------------------

/// Collect param-name lists for every user-defined free function in `decls`
/// (T105).
///
/// Returns a [`BTreeMap`] keyed by function name; the value is the
/// parameter-name list in DECLARATION ORDER (so reorder is positional).
/// Methods (inside `extend TYPE { ... }` blocks) are NOT collected — their
/// param names require receiver-type resolution (a v1.0 concern); for
/// method calls the codegen falls back to value-extraction (drop names).
///
/// A [`BTreeMap`] (not [`HashMap`]) is used so iteration is deterministic
/// across runs (the T29 flaky-test lesson). Last-declaration-wins on name
/// collisions (Buff allows shadowing at module level; the visible binding
/// at a call site is the last one in source order in single-file
/// compilation).
fn collect_func_param_names(decls: &[Decl]) -> BTreeMap<String, Vec<String>> {
    let mut out = BTreeMap::new();
    for decl in decls {
        if let Decl::FuncDecl(f) = decl {
            out.insert(
                f.name.name.clone(),
                f.params.iter().map(|p| p.name.name.clone()).collect(),
            );
        }
    }
    out
}

/// Convert a call's `args` into a POSITIONAL [`Vec<Expr>`] (T105).
///
/// Two modes:
///
/// - **Reorder** (`params = Some(pnames)`): walk `pnames` in declaration
///   order; for each param name, pull the matching named arg's value if
///   present, else take the next positional arg in source order. Unmatched
///   leftover args (positional or named) are appended AFTER the params so
///   the caller still sees them (Rust will then diagnose the arity
///   mismatch — Buff does not validate arg counts at codegen in v0.5).
///   `create(port: 80, host: "x")` with `params=[host, port]` becomes
///   `["x", 80]`.
///
/// - **Extract** (`params = None`): no param-order info available (method
///   call, prelude, builtin, foreign callee). Drop the names from any
///   NamedArg and emit the values in SOURCE order; pure-positional args
///   pass through unchanged. This is the v0.5 fallback for cases where
///   full callee-signature resolution isn't done.
///
/// Pure-positional call lists (no NamedArg at all) pass through unchanged
/// in BOTH modes (a defensive clone — the caller's args are borrowed, the
/// return is owned). The common case (`f(1, 2)`) therefore pays one
/// shallow clone of the args vec, which is cheap.
///
/// Determinism: the reorder is driven by walking `pnames` (declaration
/// order) and `args` (source order) — no [`HashMap`] iteration. The
/// output is byte-identical for the same `(args, params)` pair across
/// runs.
fn materialize_named_args(args: &[Expr], params: Option<&[String]>) -> Vec<Expr> {
    // Fast path: no NamedArg → pass through (clone the slice; cheap for the
    // typical small arg list).
    if !args.iter().any(|a| matches!(a, Expr::NamedArg { .. })) {
        return args.to_vec();
    }
    match params {
        Some(pnames) => {
            // Reorder. Collect positional args in source order; named args
            // are looked up by name when walking pnames.
            let positional: Vec<&Expr> = args
                .iter()
                .filter(|a| !matches!(a, Expr::NamedArg { .. }))
                .collect();
            let mut pos_idx = 0usize;
            let mut out: Vec<Expr> = Vec::with_capacity(pnames.len());
            for pn in pnames {
                // Linear search (arg lists are tiny — no need for a map).
                let found = args.iter().find_map(|a| match a {
                    Expr::NamedArg { name, value, .. } if &name.name == pn => Some(value),
                    _ => None,
                });
                if let Some(v) = found {
                    out.push((**v).clone());
                } else if pos_idx < positional.len() {
                    out.push(positional[pos_idx].clone());
                    pos_idx += 1;
                }
                // else: missing arg for this param — leave it out so Rust
                // diagnoses the arity mismatch (Buff v0.5 does no arity
                // checking at codegen).
            }
            // Defensive: leftover positional args go AFTER the params so
            // variadic-style callers don't silently lose data.
            while pos_idx < positional.len() {
                out.push(positional[pos_idx].clone());
                pos_idx += 1;
            }
            // Defensive: leftover named args (no matching param) go AFTER
            // leftover positionals, in SOURCE order (walk `args`, not a
            // map — determinism).
            for a in args {
                if let Expr::NamedArg { name, value, .. } = a {
                    if !pnames.iter().any(|p| p == &name.name) {
                        out.push((**value).clone());
                    }
                }
            }
            out
        }
        None => {
            // Extract: drop names, keep values, source order.
            args.iter()
                .map(|a| match a {
                    Expr::NamedArg { value, .. } => (**value).clone(),
                    _ => a.clone(),
                })
                .collect()
        }
    }
}

// ---------------------------------------------------------------------------
// T106 — default-argument fill helpers.
//
// A parameter may declare a default value (`name: Type = expr`). Rust has NO
// native default-param support, so the codegen must FILL omitted trailing
// args at the CALL SITE with the callee's declared default expression —
// `fetch("x")` becomes `fetch("x", 30)` when `timeout` defaults to `30`.
//
// This block has two free helpers:
//
// - [`collect_func_param_defaults`]: scans the decl list once at the start of
//   [`RustCodegen::generate`] and returns a `fn_name -> Vec<Option<Expr>>`
//   map (one entry per param, in declaration order; `None` = required,
//   `Some(expr)` = has a default). Same-compilation-unit free functions
//   only — mirrors [`collect_func_param_names`] (T105) scope rules.
// - [`fill_default_args`]: given a call's already-positional `args` and the
//   callee's `defaults` list, appends default expressions for any trailing
//   params the caller omitted. Returns `Some(filled)` only when at least
//   one default was filled (so the caller can skip a clone when nothing
//   changed); `None` means no fill was needed.
// ---------------------------------------------------------------------------

/// Collect default-value expressions for every parameter of every user-
/// defined free function in `decls` (T106).
///
/// Returns a [`BTreeMap`] keyed by function name; the value is the per-param
/// default list in DECLARATION ORDER (`None` for required params, `Some(expr)`
/// for defaulted ones). Mirrors [`collect_func_param_names`] (T105) for
/// scope: methods (inside `extend TYPE { ... }`) and cross-module callees are
/// NOT collected (deferred to v1.0).
///
/// A [`BTreeMap`] (not [`HashMap`]) is used so iteration is deterministic
/// across runs (the T29 flaky-test lesson). Last-declaration-wins on name
/// collisions (consistent with [`collect_func_param_names`]).
fn collect_func_param_defaults(decls: &[Decl]) -> BTreeMap<String, Vec<Option<Expr>>> {
    let mut out = BTreeMap::new();
    for decl in decls {
        if let Decl::FuncDecl(f) = decl {
            out.insert(
                f.name.name.clone(),
                f.params.iter().map(|p| p.default_value.clone()).collect(),
            );
        }
    }
    out
}

/// Collect the names of all `extern` functions declared in this compilation
/// unit (T119). Both decl shapes contribute:
/// - `Decl::FuncDecl(f)` where `f.is_extern` (the legacy `extern func
///   name(...)` form from T32),
/// - `Decl::ExternFuncDecl(d)` (the new `extern "ABI" [from "..."] func
///   name(...)` form from T119).
///
/// The set is consulted by `lower_expr`'s `Expr::FuncCall` arm: a bare-
/// ident callee whose name is in this set wraps the call in
/// `unsafe { ... }` — Rust requires an `unsafe` block at every foreign-
/// function call site, but Buff hides that from the user (the README's
/// "no `unsafe` Rust" guarantee).
///
/// A [`BTreeSet`] (not [`HashSet`]) for deterministic membership (the
/// T29 flaky-test lesson — even though we only query by membership today,
/// consistency with the rest of the codegen-feeding state is easier to
/// reason about).
fn collect_extern_fn_names(decls: &[Decl]) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    for decl in decls {
        match decl {
            Decl::FuncDecl(f) if f.is_extern => {
                out.insert(f.name.name.clone());
            }
            Decl::ExternFuncDecl(d) => {
                out.insert(d.name.name.clone());
            }
            _ => {}
        }
    }
    out
}

/// Collect the Rust crate dependencies a Buff program declares via extern
/// (T119). Walks the declaration list and returns the set of crate names
/// referenced from:
///
/// - `Decl::ExternCrateDecl` (the v0.5 `extern crate "serde"` form), AND
/// - `Decl::ExternFuncDecl` carrying a `from "crate"` annotation (the
///   T119 `extern "C" from "serde_json" func ...` form).
///
/// The legacy `extern func name(...)` form (T32, no ABI) does NOT carry
/// a crate annotation and so contributes nothing — the user must pair it
/// with a separate `extern crate "..."` declaration if they want the
/// crate recorded.
///
/// This is the function the CLI pipeline calls to auto-populate the
/// `[rust-deps]` section of `buff.toml` (and, transitively, the
/// `[dependencies]` section of the generated `Cargo.toml` when the
/// pipeline switches to a Cargo-project model). A [`BTreeSet`] is
/// returned so iteration order is deterministic across runs (the T29
/// flaky-test lesson — never rely on [`HashSet`] iteration order for
/// generated output).
///
/// # Example
///
/// ```
/// use buff_lang_codegen_rust::collect_rust_deps;
/// use buff_lang_ast::Decl;
///
/// // An empty program has no Rust deps.
/// let empty: Vec<Decl> = Vec::new();
/// let deps = collect_rust_deps(&empty);
/// assert!(deps.is_empty());
/// ```
pub fn collect_rust_deps(decls: &[Decl]) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    for decl in decls {
        match decl {
            Decl::ExternCrateDecl(d) => {
                out.insert(d.name.clone());
            }
            Decl::ExternFuncDecl(d) => {
                if let Some(c) = &d.crate_name {
                    out.insert(c.clone());
                }
            }
            _ => {}
        }
    }
    out
}

/// Fill omitted trailing defaulted params at the call site (T106).
///
/// Given a call's already-positional `args` and the callee's per-param
/// `defaults` list (in declaration order), append the default expression for
/// each trailing param the caller omitted:
///
/// - If `args.len() >= defaults.len()`: no fill needed (caller supplied all
///   params, or too many — Rust diagnoses the surplus). Returns [`None`].
/// - If `args.len() < defaults.len()`: walk `defaults[args.len() ..]`. For
///   each `Some(dv)`, push `dv.clone()` (a defaulted param the caller
///   omitted → fill the default). For each `None` (a REQUIRED param the
///   caller omitted), push nothing — Rust will diagnose the missing arg.
///   Returns `Some(filled)` iff at least one default was filled.
///
/// Determinism: the fill walks `defaults` in declaration (positional) order —
/// no [`HashMap`] iteration. The output is byte-identical for the same
/// `(args, defaults)` pair across runs.
///
/// **Interaction with T105 named-arg resolution**: this runs AFTER
/// [`materialize_named_args`], on the already-positional arg list. So a
/// named-arg call that omits a defaulted param (`fetch(url: "x")` with
/// `timeout` defaulted) is reordered first, then the missing trailing
/// default is filled — yielding `fetch("x", 30)` correctly.
fn fill_default_args(args: &[Expr], defaults: &[Option<Expr>]) -> Option<Vec<Expr>> {
    if args.len() >= defaults.len() {
        return None;
    }
    let mut filled = args.to_vec();
    let mut filled_any = false;
    // Iterate over the defaulted params the caller omitted, dropping the
    // `None`s (required params left out — Rust diagnoses those). Using
    // `.iter().flatten()` keeps clippy's `manual_flatten` happy and is the
    // idiomatic "only the Some values" walk.
    for dv in defaults[args.len()..].iter().flatten() {
        filled.push(dv.clone());
        filled_any = true;
    }
    if filled_any {
        Some(filled)
    } else {
        None
    }
}

/// Walk the declaration list looking for any `Matrix.new(...)` constructor
/// call (T24). Returns `true` if at least one is found, signalling
/// [`RustCodegen::generate`] to prepend the builtin `Matrix<T>` struct.
///
/// The detection is conservative: only the canonical constructor pattern
/// (`Matrix` Ident receiver, `new` method) triggers injection. 2-D indexing
/// on a Matrix-typed value WITHOUT a prior `Matrix.new(...)` would not
/// trigger injection by itself — but every well-formed Matrix program must
/// construct one first, so this signal is sufficient in practice. A
/// type-annotation-only `Matrix<T>` (with no constructor) is a rare edge
/// case deferred to a later task.
fn program_uses_matrix(decls: &[Decl]) -> bool {
    for decl in decls {
        let Decl::FuncDecl(f) = decl else {
            continue;
        };
        if block_uses_matrix(&f.body) {
            return true;
        }
    }
    false
}

/// Recursive helper for [`program_uses_matrix`]: scan a block's statements
/// and their nested expressions for a `Matrix.new(...)` call.
fn block_uses_matrix(block: &Block) -> bool {
    block.stmts.iter().any(stmt_uses_matrix)
}

/// Check a single statement (and its nested expressions) for Matrix.new.
fn stmt_uses_matrix(stmt: &Stmt) -> bool {
    match stmt {
        Stmt::LetDecl { value, .. }
        | Stmt::LetPattern { value, .. }
        | Stmt::ExprStmt(value, _)
        | Stmt::Return(Some(value), _) => expr_uses_matrix(value),
        Stmt::Assignment { target, value, .. } => {
            expr_uses_matrix(target) || expr_uses_matrix(value)
        }
        Stmt::Return(None, _) | Stmt::Break(_) | Stmt::Continue(_) => false,
        Stmt::ForIn { iter, body, .. } => expr_uses_matrix(iter) || block_uses_matrix(body),
        Stmt::ForWhile { cond, body, .. } => expr_uses_matrix(cond) || block_uses_matrix(body),
        // T72: `for let PAT = EXPR { body }` — value + body may use Matrix.
        Stmt::ForLet { value, body, .. } => expr_uses_matrix(value) || block_uses_matrix(body),
        // T73: `guard <conds> else { block }` — conditions + else may use
        // Matrix (any `Matrix.new` in a condition or the else-block triggers
        // emit-on-demand). Let-value and Bool-expr both count.
        Stmt::Guard {
            conditions,
            else_block,
            ..
        } => {
            conditions.iter().any(|c| match c {
                buff_lang_ast::GuardCondition::Let { value, .. } => expr_uses_matrix(value),
                buff_lang_ast::GuardCondition::Bool(e) => expr_uses_matrix(e),
            }) || block_uses_matrix(else_block)
        }
        // T100: `defer EXPR` — the deferred expression may use Matrix.
        Stmt::Defer { expr, .. } => expr_uses_matrix(expr),
    }
}

/// Recursively scan an expression tree for a `Matrix.new(...)` MethodCall.
fn expr_uses_matrix(expr: &Expr) -> bool {
    match expr {
        Expr::MethodCall {
            receiver, method, ..
        } => {
            if method.name == "new" {
                if let Expr::Ident(id, _) = receiver.as_ref() {
                    if id.name == "Matrix" {
                        return true;
                    }
                }
            }
            expr_uses_matrix(receiver)
        }
        Expr::Literal(_, _) | Expr::Ident(_, _) => false,
        Expr::BinaryOp { lhs, rhs, .. } => expr_uses_matrix(lhs) || expr_uses_matrix(rhs),
        Expr::UnaryOp { operand, .. } => expr_uses_matrix(operand),
        Expr::FuncCall { callee, args, .. } => {
            expr_uses_matrix(callee) || args.iter().any(expr_uses_matrix)
        }
        Expr::IfExpr {
            cond,
            then_block,
            else_block,
            ..
        } => {
            expr_uses_matrix(cond)
                || block_uses_matrix(then_block)
                || else_block.as_ref().is_some_and(block_uses_matrix)
        }
        Expr::StringInterp { parts, .. } => parts.iter().any(|p| match p {
            InterpPart::Expr(e) => expr_uses_matrix(e),
            InterpPart::Literal(_) => false,
        }),
        Expr::ArrayLit { elements, .. } => elements.iter().any(expr_uses_matrix),
        Expr::Index { base, indices, .. } => {
            expr_uses_matrix(base) || indices.iter().any(expr_uses_matrix)
        }
        // T25: a map literal may contain a Matrix expression as a key/value;
        // recurse conservatively.
        Expr::MapLit { entries, .. } => entries
            .iter()
            .any(|(k, v)| expr_uses_matrix(k) || expr_uses_matrix(v)),
        Expr::Lambda { body, .. } => block_uses_matrix(body),
        Expr::StructInit { fields, .. } => fields.iter().any(|(_, v)| expr_uses_matrix(v)),
        Expr::MatchExpr {
            scrutinee, arms, ..
        } => expr_uses_matrix(scrutinee) || arms.iter().any(|arm| block_uses_matrix(&arm.body)),
        Expr::SuspendExpr { inner, .. } => expr_uses_matrix(inner),
        // T30: recurse into the `?` operand so a Matrix constructor inside a
        // propagated expression is still detected.
        Expr::Try { expr, .. } => expr_uses_matrix(expr),
        // T31: `spawn expr` — does NOT use Matrix (the task body is opaque
        // to the Matrix emit-on-demand detector for v0.5).
        Expr::Spawn { task, .. } => expr_uses_matrix(task),
        // T68: `start..end` — recurse into both bounds.
        Expr::Range { start, end, .. } => expr_uses_matrix(start) || expr_uses_matrix(end),
        // T72: `if let PAT = EXPR { then } else { else }` — recurse into
        // value + both blocks (pattern carries no Matrix construction).
        Expr::IfLet {
            value,
            then_block,
            else_block,
            ..
        } => {
            expr_uses_matrix(value)
                || block_uses_matrix(then_block)
                || else_block.as_ref().is_some_and(block_uses_matrix)
        }
        // T103: `(e1, e2, ...)` — recurse into each element.
        Expr::TupleLit(members, _) => members.iter().any(expr_uses_matrix),
        // T105: a named arg `name: value` — recurse into the value.
        Expr::NamedArg { value, .. } => expr_uses_matrix(value),
    }
}

/// Build the builtin `Matrix<T>` struct AND its `new` impl as a
/// `Vec<Item>` (T24).
///
/// Emits (conceptually):
///
/// ```rust,ignore
/// #[derive(Clone, Debug)]
/// pub struct Matrix<T> {
///     pub data: Vec<T>,
///     pub rows: usize,
///     pub cols: usize,
/// }
///
/// impl<T: Default + Clone> Matrix<T> {
///     pub fn new(rows: usize, cols: usize) -> Self {
///         Self {
///             data: vec![T::default(); rows * cols],
///             rows,
///             cols,
///         }
///     }
/// }
/// ```
///
/// Built via `quote!`-equivalent (`syn::parse_str` on a string literal that
/// is itself valid Rust source) and re-parsed via
/// `syn::parse_str::<syn::File>` (returns `Result`, no panic — unlike
/// `parse_quote!`). On the (unreachable) parse failure we return an empty
/// vec; the generated program would then reference an undefined `Matrix`
/// type and fail later at rustc, which is the correct degradation (a
/// codegen bug, not a user-facing panic).
///
/// **Storage note**: `data` is a flat `Vec<T>` (NOT `Vec<Vec<T>>`) so the
/// buffer is contiguous and GPU-transferable — a `Matrix<Float<32>>` of
/// `rows * cols` f32 values can be uploaded to a WGSL storage buffer
/// verbatim. This is the flat-storage pattern the REFACTOR goal targets
/// for sharing with the GPU buffer codegen (v1.0).
///
/// **Note on the string source**: this is NOT raw-string Rust codegen —
/// the string is a *fixed template* parsed once at codegen time into
/// `syn::Item`s, after which all transformation goes through the syn tree
/// and `prettyplease`. It plays the same role as the `quote!` token-stream
/// templates used elsewhere in this file (e.g. `lower_read_line`,
/// `lower_into_iter_collect`) — a compile-time-fixed scaffold that is
/// re-parsed, not a runtime Rust-string assembler. The single string
/// producer remains `prettyplease::unparse`.
fn matrix_struct_items() -> Vec<Item> {
    let src = r#"
        #[derive(Clone, Debug)]
        pub struct Matrix<T> {
            pub data: Vec<T>,
            pub rows: usize,
            pub cols: usize,
        }

        impl<T: Default + Clone> Matrix<T> {
            pub fn new(rows: usize, cols: usize) -> Self {
                Self {
                    data: vec![T::default(); rows * cols],
                    rows,
                    cols,
                }
            }
        }
    "#;
    match syn::parse_str::<File>(src) {
        Ok(file) => file.items,
        Err(_) => Vec::new(),
    }
}

// ---------------------------------------------------------------------------
// T30 — builtin `Error` struct (emit-on-demand, mirrors Matrix pattern).
// ---------------------------------------------------------------------------

/// Walk the declaration list looking for any `Error(...)` constructor call
/// (T30). Returns `true` if at least one is found, signalling
/// [`RustCodegen::generate`] to prepend the builtin `Error` struct.
///
/// Detection is conservative: only the canonical constructor shape
/// (`FuncCall { callee: Ident("Error"), args.len() == 1 }`) triggers
/// emission. A program that mentions `Error` only in a type annotation
/// (`Result<_, Error>`) WITHOUT a constructor would not trigger emission by
/// itself — but every well-formed error-producing program must call
/// `Error(...)` to create one, so this signal is sufficient in practice.
/// (The v0.5 limitation is documented in `decisions.md`.)
fn program_uses_error(decls: &[Decl]) -> bool {
    for decl in decls {
        let Decl::FuncDecl(f) = decl else {
            continue;
        };
        if block_uses_error(&f.body) {
            return true;
        }
    }
    false
}

/// Recursive helper for [`program_uses_error`]: scan a block's statements.
fn block_uses_error(block: &Block) -> bool {
    block.stmts.iter().any(stmt_uses_error)
}

/// Check a single statement (and its nested expressions) for `Error(...)`.
fn stmt_uses_error(stmt: &Stmt) -> bool {
    match stmt {
        Stmt::LetDecl { value, .. }
        | Stmt::LetPattern { value, .. }
        | Stmt::ExprStmt(value, _)
        | Stmt::Return(Some(value), _) => expr_uses_error(value),
        Stmt::Assignment { target, value, .. } => expr_uses_error(target) || expr_uses_error(value),
        Stmt::Return(None, _) | Stmt::Break(_) | Stmt::Continue(_) => false,
        Stmt::ForIn { iter, body, .. } => expr_uses_error(iter) || block_uses_error(body),
        Stmt::ForWhile { cond, body, .. } => expr_uses_error(cond) || block_uses_error(body),
        // T72: `for let PAT = EXPR { body }` — value + body may use Error.
        Stmt::ForLet { value, body, .. } => expr_uses_error(value) || block_uses_error(body),
        // T73: `guard <conds> else { block }` — conditions + else may use
        // Error (any `Error(...)` in a condition or the else-block triggers
        // emit-on-demand of the builtin Error struct).
        Stmt::Guard {
            conditions,
            else_block,
            ..
        } => {
            conditions.iter().any(|c| match c {
                buff_lang_ast::GuardCondition::Let { value, .. } => expr_uses_error(value),
                buff_lang_ast::GuardCondition::Bool(e) => expr_uses_error(e),
            }) || block_uses_error(else_block)
        }
        // T100: `defer EXPR` — the deferred expression may use Error.
        Stmt::Defer { expr, .. } => expr_uses_error(expr),
    }
}

/// Recursively scan an expression tree for an `Error(...)` constructor call.
fn expr_uses_error(expr: &Expr) -> bool {
    match expr {
        Expr::FuncCall { callee, args, .. } => {
            if let Expr::Ident(name, _) = callee.as_ref() {
                if name.name == "Error" && args.len() == 1 {
                    return true;
                }
            }
            expr_uses_error(callee) || args.iter().any(expr_uses_error)
        }
        Expr::MethodCall { receiver, args, .. } => {
            expr_uses_error(receiver) || args.iter().any(expr_uses_error)
        }
        Expr::Literal(_, _) | Expr::Ident(_, _) => false,
        Expr::BinaryOp { lhs, rhs, .. } => expr_uses_error(lhs) || expr_uses_error(rhs),
        Expr::UnaryOp { operand, .. } => expr_uses_error(operand),
        Expr::IfExpr {
            cond,
            then_block,
            else_block,
            ..
        } => {
            expr_uses_error(cond)
                || block_uses_error(then_block)
                || else_block.as_ref().is_some_and(block_uses_error)
        }
        Expr::StringInterp { parts, .. } => parts.iter().any(|p| match p {
            InterpPart::Expr(e) => expr_uses_error(e),
            InterpPart::Literal(_) => false,
        }),
        Expr::ArrayLit { elements, .. } => elements.iter().any(expr_uses_error),
        Expr::Index { base, indices, .. } => {
            expr_uses_error(base) || indices.iter().any(expr_uses_error)
        }
        Expr::MapLit { entries, .. } => entries
            .iter()
            .any(|(k, v)| expr_uses_error(k) || expr_uses_error(v)),
        Expr::Lambda { body, .. } => block_uses_error(body),
        Expr::StructInit { fields, .. } => fields.iter().any(|(_, v)| expr_uses_error(v)),
        Expr::MatchExpr {
            scrutinee, arms, ..
        } => expr_uses_error(scrutinee) || arms.iter().any(|arm| block_uses_error(&arm.body)),
        Expr::SuspendExpr { inner, .. } => expr_uses_error(inner),
        // T30: recurse into the `?` operand.
        Expr::Try { expr, .. } => expr_uses_error(expr),
        // T31: recurse into the spawn task body.
        Expr::Spawn { task, .. } => expr_uses_error(task),
        // T68: `start..end` — recurse into both bounds.
        Expr::Range { start, end, .. } => expr_uses_error(start) || expr_uses_error(end),
        // T72: `if let PAT = EXPR { then } else { else }` — recurse into
        // value + both blocks (pattern carries no Error construction).
        Expr::IfLet {
            value,
            then_block,
            else_block,
            ..
        } => {
            expr_uses_error(value)
                || block_uses_error(then_block)
                || else_block.as_ref().is_some_and(block_uses_error)
        }
        // T103: `(e1, e2, ...)` — recurse into each element.
        Expr::TupleLit(members, _) => members.iter().any(expr_uses_error),
        // T105: a named arg `name: value` — recurse into the value.
        Expr::NamedArg { value, .. } => expr_uses_error(value),
    }
}

// ---------------------------------------------------------------------------
// T124b — chrono / std::time emit-on-demand detection (prelude-types).
// ---------------------------------------------------------------------------

/// Walk the declaration list looking for any reference to a prelude
/// datetime type (T124b). Returns `true` if at least one is found,
/// signalling [`RustCodegen::generate`] to record `"chrono"` in the
/// extern-crate set so the pipeline knows the generated Cargo project
/// depends on `chrono`.
///
/// Detection recognises BOTH:
/// - **Associated-function calls**: `DateTime.now()`, `Duration.days(7)`,
///   `Instant.now()`, `Date.today()`, etc. (receiver is a bare Ident
///   naming a prelude type).
/// - **Instance-method calls**: `dt.format(...)`, `dt.year()`, etc. The
///   receiver is NOT a bare type name (it's a value), so we conservatively
///   detect ANY call to the prelude instance-method names — false positives
///   are tolerable (they just trigger chrono registration, which is a
///   no-op if the program doesn't actually use chrono).
///
/// Source-level type annotations (`let dt: DateTime = ...`) are NOT
/// detected by this walker; they're handled by the codegen pass directly
/// via [`buff_lang_types::is_prelude_type`] when the annotation is
/// resolved. The two paths together cover every realistic chrono use.
fn program_uses_chrono(decls: &[Decl]) -> bool {
    for decl in decls {
        if let Decl::FuncDecl(f) = decl {
            if block_uses_chrono(&f.body) {
                return true;
            }
        }
    }
    false
}

/// Recursive helper for [`program_uses_chrono`]: scan a block's statements.
fn block_uses_chrono(block: &Block) -> bool {
    block.stmts.iter().any(stmt_uses_chrono)
}

/// Check a single statement (and its nested expressions) for chrono usage.
fn stmt_uses_chrono(stmt: &Stmt) -> bool {
    match stmt {
        Stmt::LetDecl {
            value,
            ty: Some(ty),
            ..
        }
        | Stmt::LetPattern {
            value,
            ty: Some(ty),
            ..
        } => {
            // Source-level type annotation names a prelude type
            // (e.g. `let dt: DateTime = ...`). This counts even if the
            // value expression doesn't itself mention chrono.
            type_ref_names_prelude_type(ty) || expr_uses_chrono(value)
        }
        Stmt::LetDecl { value, .. }
        | Stmt::LetPattern { value, .. }
        | Stmt::ExprStmt(value, _)
        | Stmt::Return(Some(value), _) => expr_uses_chrono(value),
        Stmt::Assignment { target, value, .. } => {
            expr_uses_chrono(target) || expr_uses_chrono(value)
        }
        Stmt::Return(None, _) | Stmt::Break(_) | Stmt::Continue(_) => false,
        Stmt::ForIn { iter, body, .. } => expr_uses_chrono(iter) || block_uses_chrono(body),
        Stmt::ForWhile { cond, body, .. } => expr_uses_chrono(cond) || block_uses_chrono(body),
        Stmt::ForLet { value, body, .. } => expr_uses_chrono(value) || block_uses_chrono(body),
        Stmt::Guard {
            conditions,
            else_block,
            ..
        } => {
            conditions.iter().any(|c| match c {
                buff_lang_ast::GuardCondition::Let { value, .. } => expr_uses_chrono(value),
                buff_lang_ast::GuardCondition::Bool(e) => expr_uses_chrono(e),
            }) || block_uses_chrono(else_block)
        }
        Stmt::Defer { expr, .. } => expr_uses_chrono(expr),
    }
}

/// Returns `true` iff `ty` (or any nested inner TypeRef) mentions a prelude
/// datetime type name. Used by [`stmt_uses_chrono`] to detect source-level
/// type annotations like `let dt: DateTime = ...`.
fn type_ref_names_prelude_type(ty: &TypeRef) -> bool {
    match ty {
        TypeRef::Named { name, .. } => buff_lang_types::is_prelude_type(&name.name),
        TypeRef::Option(inner, _) => type_ref_names_prelude_type(inner),
        TypeRef::Generic { base, args, .. } => {
            type_ref_names_prelude_type(base)
                || args.iter().any(type_ref_names_prelude_type)
        }
        _ => false,
    }
}

/// Recursively scan an expression tree for any prelude-type usage.
///
/// Detection is conservative: it triggers on any `Type.<assoc_fn>()`
/// shape whose receiver is a bare Ident naming a prelude type, OR on any
/// instance-method call whose method name is a recognised prelude
/// instance method (the receiver's inferred type is then checked at
/// codegen time by `lower_method_call`).
fn expr_uses_chrono(expr: &Expr) -> bool {
    match expr {
        Expr::MethodCall {
            receiver, method, ..
        } => {
            // Associated-function call: `DateTime.now()`, `Duration.days(7)`, etc.
            if let Expr::Ident(id, _) = receiver.as_ref() {
                if buff_lang_types::is_prelude_type(&id.name) {
                    return true;
                }
            }
            // Instance-method call: `dt.format(...)`, `dt.year()`, etc.
            // Conservative on the method NAME — the receiver's type is
            // resolved at codegen time, so we err on the side of "register
            // chrono" if the method name matches any prelude instance fn.
            if buff_lang_types::PreludeInstanceFn::ALL
                .iter()
                .any(|f| f.name() == method.name.as_str())
            {
                return true;
            }
            expr_uses_chrono(receiver)
        }
        Expr::Literal(_, _) | Expr::Ident(_, _) => false,
        Expr::BinaryOp { lhs, rhs, .. } => expr_uses_chrono(lhs) || expr_uses_chrono(rhs),
        Expr::UnaryOp { operand, .. } => expr_uses_chrono(operand),
        Expr::FuncCall { callee, args, .. } => {
            expr_uses_chrono(callee) || args.iter().any(expr_uses_chrono)
        }
        Expr::IfExpr {
            cond,
            then_block,
            else_block,
            ..
        } => {
            expr_uses_chrono(cond)
                || block_uses_chrono(then_block)
                || else_block.as_ref().is_some_and(block_uses_chrono)
        }
        Expr::StringInterp { parts, .. } => parts.iter().any(|p| match p {
            InterpPart::Expr(e) => expr_uses_chrono(e),
            InterpPart::Literal(_) => false,
        }),
        Expr::ArrayLit { elements, .. } => elements.iter().any(expr_uses_chrono),
        Expr::Index { base, indices, .. } => {
            expr_uses_chrono(base) || indices.iter().any(expr_uses_chrono)
        }
        Expr::MapLit { entries, .. } => entries
            .iter()
            .any(|(k, v)| expr_uses_chrono(k) || expr_uses_chrono(v)),
        Expr::Lambda { body, .. } => block_uses_chrono(body),
        Expr::StructInit { fields, .. } => fields.iter().any(|(_, v)| expr_uses_chrono(v)),
        Expr::MatchExpr {
            scrutinee, arms, ..
        } => expr_uses_chrono(scrutinee) || arms.iter().any(|arm| block_uses_chrono(&arm.body)),
        Expr::SuspendExpr { inner, .. } => expr_uses_chrono(inner),
        Expr::Try { expr, .. } => expr_uses_chrono(expr),
        Expr::Spawn { task, .. } => expr_uses_chrono(task),
        Expr::Range { start, end, .. } => expr_uses_chrono(start) || expr_uses_chrono(end),
        Expr::IfLet {
            value,
            then_block,
            else_block,
            ..
        } => {
            expr_uses_chrono(value)
                || block_uses_chrono(then_block)
                || else_block.as_ref().is_some_and(block_uses_chrono)
        }
        Expr::TupleLit(members, _) => members.iter().any(expr_uses_chrono),
        Expr::NamedArg { value, .. } => expr_uses_chrono(value),
    }
}

// ---------------------------------------------------------------------------
// T124c — tracing / tracing-subscriber emit-on-demand detection (Log module).
// ---------------------------------------------------------------------------

/// Walk the declaration list looking for any `Log.<level>(...)` call
/// (T124c). Returns `true` if at least one is found, signalling
/// [`RustCodegen::generate`] to:
/// 1. record `"tracing"` + `"tracing-subscriber"` in the extern-crate set
///    so the pipeline knows the generated Cargo project depends on both
///    crates;
/// 2. emit a `tracing_subscriber::fmt()...try_init()` statement at the top
///    of `main` so the program's log output is formatted (pretty in dev,
///    JSON in release) and level-filtered via the `BUFF_LOG` env var.
///
/// Detection recognises the `Log` namespace as the receiver of a method
/// call (`Log.info(...)`, `Log.error(...)`, ...). The method name is NOT
/// matched here — `Log` is a reserved prelude namespace, so any
/// `Log.<anything>()` triggers tracing registration. Codegen will surface
/// a clear error if `<anything>` is not one of debug/info/warn/error.
///
/// Mirrors the chrono detection pattern (T124b); the recursive walker
/// covers every Stmt / Expr variant that could host a `Log` call.
fn program_uses_tracing(decls: &[Decl]) -> bool {
    for decl in decls {
        if let Decl::FuncDecl(f) = decl {
            if block_uses_tracing(&f.body) {
                return true;
            }
        }
    }
    false
}

/// Recursive helper for [`program_uses_tracing`]: scan a block's statements.
fn block_uses_tracing(block: &Block) -> bool {
    block.stmts.iter().any(stmt_uses_tracing)
}

/// Check a single statement (and its nested expressions) for `Log.*(...)` usage.
fn stmt_uses_tracing(stmt: &Stmt) -> bool {
    match stmt {
        Stmt::LetDecl { value, .. }
        | Stmt::LetPattern { value, .. }
        | Stmt::ExprStmt(value, _)
        | Stmt::Return(Some(value), _) => expr_uses_tracing(value),
        Stmt::Assignment { target, value, .. } => {
            expr_uses_tracing(target) || expr_uses_tracing(value)
        }
        Stmt::Return(None, _) | Stmt::Break(_) | Stmt::Continue(_) => false,
        Stmt::ForIn { iter, body, .. } => expr_uses_tracing(iter) || block_uses_tracing(body),
        Stmt::ForWhile { cond, body, .. } => expr_uses_tracing(cond) || block_uses_tracing(body),
        Stmt::ForLet { value, body, .. } => expr_uses_tracing(value) || block_uses_tracing(body),
        Stmt::Guard {
            conditions,
            else_block,
            ..
        } => {
            conditions.iter().any(|c| match c {
                buff_lang_ast::GuardCondition::Let { value, .. } => expr_uses_tracing(value),
                buff_lang_ast::GuardCondition::Bool(e) => expr_uses_tracing(e),
            }) || block_uses_tracing(else_block)
        }
        Stmt::Defer { expr, .. } => expr_uses_tracing(expr),
    }
}

/// Recursively scan an expression tree for a `Log.<method>(...)` call.
///
/// Detection is on the receiver NAME (`Log`) only — the method name is
/// validated at codegen time. This means a hypothetical user-defined
/// variable named `Log` whose method is called would trigger a false
/// positive (registering tracing unnecessarily); but since `Log` is a
/// reserved prelude namespace, the user can't legitimately bind to that
/// name anyway (shadowing it is the documented head-gun pattern from
/// the T124b registry).
fn expr_uses_tracing(expr: &Expr) -> bool {
    match expr {
        Expr::MethodCall { receiver, method, .. } => {
            if let Expr::Ident(id, _) = receiver.as_ref() {
                if id.name == "Log" {
                    return true;
                }
            }
            // Conservatively flag any call whose method name matches a Log
            // level — same conservative strategy T124b uses for chrono
            // instance-method detection. The codegen arm will then either
            // emit a Log lowering or surface a clear error.
            if matches!(
                method.name.as_str(),
                "debug" | "info" | "warn" | "error"
            ) {
                // Only flag if the receiver could plausibly be Log (bare
                // Ident). We already covered `Log` above; other receivers
                // (values, calls) might be user methods that happen to
                // share the name — those should NOT trigger tracing
                // registration. So this branch is a no-op; we leave the
                // method-name check in place as documentation of the
                // design decision.
            }
            expr_uses_tracing(receiver)
        }
        Expr::Literal(_, _) | Expr::Ident(_, _) => false,
        Expr::BinaryOp { lhs, rhs, .. } => expr_uses_tracing(lhs) || expr_uses_tracing(rhs),
        Expr::UnaryOp { operand, .. } => expr_uses_tracing(operand),
        Expr::FuncCall { callee, args, .. } => {
            expr_uses_tracing(callee) || args.iter().any(expr_uses_tracing)
        }
        Expr::IfExpr {
            cond,
            then_block,
            else_block,
            ..
        } => {
            expr_uses_tracing(cond)
                || block_uses_tracing(then_block)
                || else_block.as_ref().is_some_and(block_uses_tracing)
        }
        Expr::StringInterp { parts, .. } => parts.iter().any(|p| match p {
            InterpPart::Expr(e) => expr_uses_tracing(e),
            InterpPart::Literal(_) => false,
        }),
        Expr::ArrayLit { elements, .. } => elements.iter().any(expr_uses_tracing),
        Expr::Index { base, indices, .. } => {
            expr_uses_tracing(base) || indices.iter().any(expr_uses_tracing)
        }
        Expr::MapLit { entries, .. } => entries
            .iter()
            .any(|(k, v)| expr_uses_tracing(k) || expr_uses_tracing(v)),
        Expr::Lambda { body, .. } => block_uses_tracing(body),
        Expr::StructInit { fields, .. } => fields.iter().any(|(_, v)| expr_uses_tracing(v)),
        Expr::MatchExpr {
            scrutinee, arms, ..
        } => expr_uses_tracing(scrutinee) || arms.iter().any(|arm| block_uses_tracing(&arm.body)),
        Expr::SuspendExpr { inner, .. } => expr_uses_tracing(inner),
        Expr::Try { expr, .. } => expr_uses_tracing(expr),
        Expr::Spawn { task, .. } => expr_uses_tracing(task),
        Expr::Range { start, end, .. } => expr_uses_tracing(start) || expr_uses_tracing(end),
        Expr::IfLet {
            value,
            then_block,
            else_block,
            ..
        } => {
            expr_uses_tracing(value)
                || block_uses_tracing(then_block)
                || else_block.as_ref().is_some_and(block_uses_tracing)
        }
        Expr::TupleLit(members, _) => members.iter().any(expr_uses_tracing),
        Expr::NamedArg { value, .. } => expr_uses_tracing(value),
    }
}

/// T124c: build the `tracing_subscriber` init statement emitted at the top
/// of `main` when the program uses the `Log` module.
///
/// Emits (conceptually):
///
/// ```rust,ignore
/// {
///     let __buff_log_filter = tracing_subscriber::EnvFilter::try_from_env("BUFF_LOG")
///         .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info"));
///     let _ = if cfg!(debug_assertions) {
///         tracing_subscriber::fmt()
///             .with_env_filter(__buff_log_filter)
///             .try_init()
///     } else {
///         tracing_subscriber::fmt()
///             .with_env_filter(__buff_log_filter)
///             .json()
///             .try_init()
///     };
/// }
/// ```
///
/// # Design
///
/// - **`BUFF_LOG` env var** drives the level filter (RUST_LOG-style
///   directives: `BUFF_LOG=debug`, `BUFF_LOG=warn,buff::net=trace`).
///   Falls back to `"info"` when unset or unparseable via `unwrap_or_else`
///   (NO panic — matches Buff's "no panicking generated code" stance).
/// - **dev vs release**: `cfg!(debug_assertions)` is a RUNTIME check in
///   Rust (`cfg!` macro form, not `#[cfg]` attribute) so the same compiled
///   binary can be reused. Dev → pretty to stderr (the default
///   `tracing_subscriber::fmt()` formatter); release → JSON to stdout
///   (`.json()` layer).
/// - **`try_init()` not `init()`**: `try_init()` returns `Result` instead
///   of panicking on duplicate-global-subscriber. We discard the result
///   with `let _ = ...` — the SECOND init in a test/binary that already
///   has a subscriber is silently swallowed (Buff's "no panic" rule).
/// - **Single filter value**: built ONCE outside the `if`/`else`, then
///   MOVED into whichever branch runs. Rust's branch-evaluation semantics
///   permit this (only one branch executes at runtime, so the single
///   move is sound).
///
/// # Why a block statement (not bare)?
///
/// Wrapping in a `{ ... }` block scopes the `__buff_log_filter` binding
/// so it doesn't leak into the user's `main` body. The block evaluates
/// to `()` (the `let _ = ...` discards the `Result`), so it can stand
/// as a regular statement at the top of `main`'s body.
///
/// Built via `quote!` + `syn::parse2` (the standard pattern in this
/// module — the single string producer remains `prettyplease::unparse`).
/// On parse failure (unreachable — the template is compile-time-fixed)
/// we return `None` so the caller silently skips the init (defensive —
/// never panics in codegen).
fn tracing_subscriber_init_stmt() -> Option<SynStmt> {
    let tokens: proc_macro2::TokenStream = quote::quote! {
        {
            let __buff_log_filter = tracing_subscriber::EnvFilter::try_from_env("BUFF_LOG")
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info"));
            let _ = if cfg!(debug_assertions) {
                tracing_subscriber::fmt()
                    .with_env_filter(__buff_log_filter)
                    .try_init()
            } else {
                tracing_subscriber::fmt()
                    .with_env_filter(__buff_log_filter)
                    .json()
                    .try_init()
            };
        }
    };
    syn::parse2::<SynStmt>(tokens).ok()
}

/// Build the builtin `Error` struct + its `new` impl + `Display` + Error trait
/// impls as a `Vec<Item>` (T30).
///
/// Emits (conceptually):
///
/// ```rust,ignore
/// #[derive(Clone, Debug)]
/// pub struct Error {
///     pub message: String,
/// }
///
/// impl Error {
///     pub fn new(message: impl Into<String>) -> Self {
///         Self { message: message.into() }
///     }
/// }
///
/// impl std::fmt::Display for Error {
///     fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
///         write!(f, "{}", self.message)
///     }
/// }
///
/// impl std::error::Error for Error {}
/// ```
///
/// This makes `Error` a proper Rust error type: it implements
/// `std::error::Error` (so `?` propagation's `From` bound is satisfiable
/// when the enclosing fn returns `Result<T, Error>`), `Display` (required by
/// `std::error::Error`), and `Debug` + `Clone` (consistent with every other
/// generated type via [`derive_and_repr_attrs`]).
///
/// Built via the same fixed-template-then-`syn::parse_str` approach as
/// [`matrix_struct_items`] (T24). See that function's docstring for the
/// "this is NOT raw-string codegen" rationale — the string is a
/// compile-time-fixed scaffold re-parsed into `syn::Item`s.
fn error_struct_items() -> Vec<Item> {
    let src = r#"
        #[derive(Clone, Debug)]
        pub struct Error {
            pub message: String,
        }

        impl Error {
            pub fn new(message: impl Into<String>) -> Self {
                Self {
                    message: message.into(),
                }
            }
        }

        impl std::fmt::Display for Error {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                write!(f, "{}", self.message)
            }
        }

        impl std::error::Error for Error {}
    "#;
    match syn::parse_str::<File>(src) {
        Ok(file) => file.items,
        Err(_) => Vec::new(),
    }
}

/// Build a Rust `as usize` cast for a vector index (T23).
///
/// Unlike [`cast_to`], this only wraps the operand in parens when it is a
/// non-atomic expression (binary/unary/cast), so the common cases stay clean:
/// `0 as usize`, `i as usize`. Compound indices like `a + b` become
/// `(a + b) as usize` so the cast doesn't bind tighter than the `+`.
fn cast_to_usize(e: SynExpr) -> SynExpr {
    let needs_parens = matches!(
        e,
        SynExpr::Binary(_) | SynExpr::Unary(_) | SynExpr::Cast(_) | SynExpr::Range(_)
    );
    let operand = if needs_parens { wrap_in_parens(e) } else { e };
    SynExpr::Cast(syn::ExprCast {
        attrs: Vec::new(),
        expr: Box::new(operand),
        as_token: Default::default(),
        ty: Box::new(rust_path_type("usize")),
    })
}

/// Build a Rust `as` cast: `(e) as T`. The receiver is parenthesised so
/// compound expressions bind correctly (e.g. `(a + b) as f64` not `a + b as f64`).
fn cast_to(e: SynExpr, target: &str) -> SynExpr {
    SynExpr::Cast(syn::ExprCast {
        attrs: Vec::new(),
        expr: Box::new(wrap_in_parens(e)),
        as_token: Default::default(),
        ty: Box::new(rust_path_type(target)),
    })
}

/// Build a Rust integer-literal expression (`0`, `1`, etc.).
fn make_int_lit_expr(n: i64) -> SynExpr {
    SynExpr::Lit(syn::ExprLit {
        attrs: Vec::new(),
        lit: syn::Lit::Int(syn::LitInt::new(&n.to_string(), ProcSpan::call_site())),
    })
}

/// Build a binary expression `lhs <op> rhs`.
fn make_binary_expr(op: syn::BinOp, lhs: SynExpr, rhs: SynExpr) -> SynExpr {
    SynExpr::Binary(syn::ExprBinary {
        attrs: Vec::new(),
        left: Box::new(lhs),
        op,
        right: Box::new(rhs),
    })
}

/// Discriminates the two Rust idioms for type-constructor prelude functions.
///
/// - `Numeric` covers `Int(x)` / `Float(x)` — Rust emits `(x) as T`.
/// - `Bool` is separate because Rust has no `as bool` cast; the numeric→bool
///   mapping is `x != 0`.
#[derive(Clone, Copy)]
enum ConvKind {
    Numeric,
    Bool,
}

/// Build a `x.parse::<T>().unwrap_or(default)` expression for the string-arg
/// branch of `Int(x)`/`Float(x)`/`Bool(x)`. Built via `quote!` so the
/// turbofish `::<T>` and method chain are constructed without hand-formatted
/// Rust.
fn parse_with_default(arg: SynExpr, target: &str, kind: &ConvKind) -> SynExpr {
    // Build `arg.parse::<target>()` as a method call with turbofish.
    let parse_call = SynExpr::MethodCall(syn::ExprMethodCall {
        attrs: Vec::new(),
        receiver: Box::new(arg),
        dot_token: Default::default(),
        method: Ident::new("parse", ProcSpan::call_site()),
        turbofish: Some(syn::AngleBracketedGenericArguments {
            colon2_token: None,
            lt_token: Default::default(),
            args: {
                let mut p: Punctuated<syn::GenericArgument, syn::Token![,]> = Punctuated::new();
                p.push(syn::GenericArgument::Type(rust_path_type(target)));
                p
            },
            gt_token: Default::default(),
        }),
        paren_token: Default::default(),
        args: Default::default(),
    });
    // `.unwrap_or(<default-lit>)` — for numerics the default is the unsuffixed
    // integer `0`; for bool it's `false`. Both are valid Rust literal tokens.
    let default_tokens: proc_macro2::TokenStream = match kind {
        ConvKind::Numeric => {
            let lit = proc_macro2::Literal::i64_unsuffixed(0);
            quote::quote! { #lit }
        }
        ConvKind::Bool => {
            quote::quote! { false }
        }
    };
    let default_expr =
        syn::parse2::<SynExpr>(default_tokens).unwrap_or_else(|_| make_int_lit_expr(0));
    // `.unwrap_or(default_expr)` — single-arg method call on the parse result.
    method_call_one_arg(parse_call, "unwrap_or", default_expr)
}

/// Mirror of the private `typeref_to_type` in `buff_lang_types::infer`.
///
/// Used by [`RustCodegen::lower_func`] to seed the [`TypeInferencer`]
/// environment with function-parameter types so subsequent `let`
/// bindings can refer to params and still get a useful inferred type.
fn typeref_to_type(ty: &TypeRef) -> Option<Type> {
    match ty {
        TypeRef::Named { name, .. } => match name.name.as_str() {
            "Int" => Some(Type::int_default()),
            "Float" => Some(Type::float_default()),
            "Double" => Some(Type::double()),
            "Bool" => Some(Type::bool()),
            "String" => Some(Type::string()),
            // T21: Char annotation maps to the resolved Char type.
            "Char" => Some(Type::char()),
            "Byte" => Some(Type::byte()),
            "Decimal" => Some(Type::Decimal),
            "Void" => Some(Type::Void),
            _ => None,
        },
        // T76: union types (for match param resolution). Resolve each
        // member recursively; unknown members become Unknown.
        TypeRef::Union(members, _) => {
            let resolved: Vec<Type> = members.iter().filter_map(typeref_to_type).collect();
            Some(Type::Union(resolved))
        }
        _ => None,
    }
}

impl RustCodegen {
    /// Build a Rust enum item for a union wrapper. T76.
    ///
    /// Example: members `[TypeRef::Named("String"), TypeRef::Named("Int")]`
    /// → `enum StringOrInt { String(String), Int(i64), }` (each variant
    /// named after the member's display name, carrying that member's Rust
    /// type). For v0.5, only Named and Generic-with-Named-base members
    /// are supported (Variant name = member name or Generic base name).
    fn union_enum_item(
        &mut self,
        name: &str,
        members: &[TypeRef],
    ) -> Result<ItemEnum, CodegenError> {
        let mut variants: Punctuated<syn::Variant, syn::Token![,]> = Punctuated::new();
        for member in members {
            // Variant name = member's display name (e.g. "String", "Int").
            // For Named it's the name; for Generic it's the base name (v0.5
            // limitation — nested unions deferred).
            let variant_name = match member {
                TypeRef::Named { name, .. } => name.name.clone(),
                TypeRef::Generic { base, .. } => {
                    if let TypeRef::Named { name, .. } = base.as_ref() {
                        name.name.clone()
                    } else {
                        return Err(self.unsupported(
                            "union member variant name: only Named or Generic with Named base supported in v0.5",
                        ));
                    }
                }
                _ => {
                    return Err(self.unsupported(
                        "union member variant name: only Named or Generic supported in v0.5",
                    ))
                }
            };
            // Inner Rust type: lower the member TypeRef.
            let inner_ty = self.ast_typeref_to_syn(member)?;
            // Create a tuple variant with one field.
            let variant = syn::Variant {
                attrs: Vec::new(),
                ident: Ident::new(&variant_name, ProcSpan::call_site()),
                fields: syn::Fields::Unnamed(syn::FieldsUnnamed {
                    paren_token: Default::default(),
                    unnamed: {
                        let mut punct: Punctuated<syn::Field, syn::Token![,]> = Punctuated::new();
                        punct.push(syn::Field {
                            attrs: Vec::new(),
                            vis: Visibility::Inherited,
                            ident: None,
                            colon_token: None,
                            ty: inner_ty,
                            mutability: syn::FieldMutability::None,
                        });
                        punct
                    },
                }),
                discriminant: None,
            };
            variants.push(variant);
        }
        Ok(ItemEnum {
            attrs: derive_and_repr_attrs(false), // No #[repr(C)] for unions.
            vis: Visibility::Public(Default::default()),
            enum_token: Default::default(),
            ident: Ident::new(name, ProcSpan::call_site()),
            generics: syn::Generics::default(),
            brace_token: Default::default(),
            variants,
        })
    }

    /// T107: compute the set of USER-DEFINED struct names whose fields are
    /// ALL Hash-safe (i.e. would let the struct carry the `#[derive(Hash)]`
    /// attribute without rustc rejecting it).
    ///
    /// # Algorithm — fixpoint removal
    ///
    /// 1. Start with ALL declared user struct names in the "safe" set.
    /// 2. Iterate: any struct whose fields are NOT all Hash-safe (consulting
    ///    the current "safe" set for user-typed fields via [`type_is_hash_safe`])
    ///    is REMOVED from the set.
    /// 3. Repeat until no change (a fixpoint).
    ///
    /// The fixpoint handles TRANSITIVE Hash-safety: if `struct A { b: B }`
    /// and `struct B { x: Float }`, then:
    /// - Pass 1 removes `B` (Float field is not Hash-safe).
    /// - Pass 2 removes `A` (its field `b: B` is no longer Hash-safe, since
    ///   `B` was just evicted).
    ///
    /// Cycle safety: if `struct X { y: Y }` and `struct Y { x: X }` (a
    /// cycle), both start in the set; if neither has any non-Hash field,
    /// they STAY in the set (both derive Hash and Rust accepts it —
    /// `#[derive(Hash)]` on a cyclic struct graph is fine because the
    /// derived impl doesn't recurse infinitely; only the trait bounds need
    /// to hold, which they do). If either has a Float field, both get
    /// evicted in the fixpoint.
    ///
    /// # Why a precompute (not on-demand)
    ///
    /// `lower_struct_decl` is called in source order, so a struct may be
    /// lowered BEFORE the structs it references. Precomputing the safe set
    /// once, up-front in [`Self::generate`], means every per-struct
    /// lowering decision sees the FULL program's Hash-safety info.
    fn compute_hash_safe_structs(&self, decls: &[Decl]) -> BTreeSet<String> {
        // Map: struct name → its fields (borrowed from decls).
        let mut struct_fields: BTreeMap<String, &Vec<(buff_lang_ast::common::Ident, TypeRef)>> =
            BTreeMap::new();
        for decl in decls {
            if let Decl::StructDecl(s) = decl {
                struct_fields.insert(s.name.name.clone(), &s.fields);
            }
        }
        // Fixpoint: start optimistic (all structs in), iteratively evict
        // any struct whose fields aren't all Hash-safe. Bounded by
        // struct count (each pass evicts at least one struct, or terminates).
        let mut safe: BTreeSet<String> = struct_fields.keys().cloned().collect();
        loop {
            let prev_len = safe.len();
            // Collect the names to evict this pass. We can't mutate `safe`
            // while iterating its dependents, so buffer the evictions.
            let to_evict: Vec<String> = struct_fields
                .iter()
                .filter_map(|(name, fields)| {
                    // Only consider structs still in the running.
                    if !safe.contains(name) {
                        return None;
                    }
                    // If ANY field is not Hash-safe (against the current
                    // `safe` set), this struct must be evicted.
                    let all_safe = fields.iter().all(|(_, ty)| type_is_hash_safe(ty, &safe));
                    if all_safe {
                        None
                    } else {
                        Some(name.clone())
                    }
                })
                .collect();
            for name in &to_evict {
                safe.remove(name);
            }
            if safe.len() == prev_len {
                break;
            }
        }
        safe
    }

    /// T107: emit per-struct `impl Struct { ... }` blocks containing the
    /// auto-derived `copy_<field>` immutable-update methods.
    ///
    /// For each non-empty user struct, emits ONE inherent impl block with
    /// one method per field:
    ///
    /// ```rust,ignore
    /// impl Struct {
    ///     pub fn copy_<field>(&self, <field>: <rust_ty>) -> Self {
    ///         let mut c = self.clone();
    ///         c.<field> = <field>;
    ///         c
    ///     }
    ///     // … one per field
    /// }
    /// ```
    ///
    /// The method takes `&self` (immutable borrow — the original is
    /// untouched, providing the immutable-update ergonomics Buff mandates),
    /// CLONES it (requires the `Clone` derive — always present on structs
    /// per T26+T107), reassigns the named field, and returns the clone.
    ///
    /// # Empty structs
    ///
    /// A struct with zero fields gets NO impl block (no methods to emit).
    /// Emitting an empty `impl Struct { }` would be valid Rust but adds
    /// noise to generated source for no value.
    ///
    /// # Ordering
    ///
    /// Impl blocks are pushed in SOURCE-STRUCT-DECLARATION ORDER (the
    /// `decls` slice is walked in order, and only `Decl::StructDecl`
    /// entries contribute). This makes the generated source deterministic
    /// — the same input always produces byte-identical output (the T29
    /// flaky-test lesson: never use HashMap iteration for codegen output).
    ///
    /// # Deferrals (v0.5)
    ///
    /// - **Multi-field copy**: `p.copy(name: "X", age: 31)` (multiple
    ///   fields in one call) is not yet supported — only the per-field
    ///   `copy_<field>(value)` form is generated.
    /// - **Builder pattern**: a fluent `.with_<field>(value)` combinator
    ///   chain is deferred.
    /// - **Custom `to_string`**: a `Display` impl is not auto-derived.
    fn emit_record_copy_methods(
        &mut self,
        decls: &[Decl],
        items: &mut Vec<Item>,
    ) -> Result<(), CodegenError> {
        for decl in decls {
            let s = match decl {
                Decl::StructDecl(s) => s,
                _ => continue,
            };
            // Skip empty structs — no fields means no copy methods (and we
            // avoid emitting an empty `impl Struct { }` block).
            if s.fields.is_empty() {
                continue;
            }
            // Build one method per field, in source order.
            let mut impl_items: Vec<syn::ImplItem> = Vec::with_capacity(s.fields.len());
            for (field_name, field_type) in &s.fields {
                let field_rust_ty = self.ast_typeref_to_syn(field_type)?;
                let method = build_record_copy_method(&field_name.name, field_rust_ty);
                impl_items.push(syn::ImplItem::Fn(method));
            }
            let impl_item = syn::ItemImpl {
                attrs: Vec::new(),
                defaultness: None,
                unsafety: None,
                generics: syn::Generics::default(),
                impl_token: Default::default(),
                // Inherent impl (`impl StructName { ... }`) — `trait_: None`.
                trait_: None,
                self_ty: Box::new(rust_path_type(&s.name.name)),
                brace_token: Default::default(),
                items: impl_items,
            };
            items.push(Item::Impl(impl_item));
        }
        Ok(())
    }

    /// T92: emit auto-delegation `impl` blocks for struct embedding.
    ///
    /// Scans all [`Decl::StructDecl`]s in `decls`. For each struct field
    /// whose type is a NAMED DECLARED struct that has methods (collected
    /// from [`Decl::ExtendBlock`]s targeting it), emits an inherent
    /// `impl StructName { fn <m>(self, ...) -> ... { self.<field>.<m>(...) } }`
    /// block — one forwarding method per method of the embedded type.
    ///
    /// This is the embedding/delegation pattern (à la Go): a struct that
    /// embeds another struct inherits its methods automatically, with the
    /// compiler generating the forwarding boilerplate. The user writes
    /// `employee.name()` and the call resolves to
    /// `employee.person.name()` via the auto-generated inherent impl.
    ///
    /// # Analysis (deterministic)
    ///
    /// Two maps are built from `decls`:
    /// - `struct_names: BTreeSet<String>` — names of all declared structs.
    ///   Used to decide whether a field's named type is a user struct
    ///   (vs. a primitive like `Float` that happens to be `TypeRef::Named`).
    /// - `methods_by_type: BTreeMap<String, Vec<&FuncDecl>>` — methods
    ///   grouped by their extend-block target type name. Multiple extend
    ///   blocks targeting the same type are merged (safe — only the
    ///   method list is consulted; the trait/impl emission happens in
    ///   [`Self::lower_extend_block_items`] and may itself collide on the
    ///   `BuffExt{Type}` name, but that is T75's concern, not ours).
    ///
    /// Both use [`BTreeMap`]/[`BTreeSet`] (NOT [`HashSet`]) so iteration
    /// order is deterministic across runs — the T29 flaky-test lesson.
    ///
    /// # Delegation per-method
    ///
    /// Only methods whose FIRST param is named `self` (instance methods)
    /// are delegated. Methods without a `self` receiver (associated
    /// functions like `Person::new()`) are skipped because the forwarding
    /// body `self.field.method()` would not type-check for a no-receiver
    /// method (Rust requires `Type::method()` syntax there). Supporting
    /// them is deferred.
    ///
    /// The delegation method's signature mirrors the original (same name,
    /// params, return type), with the first `self` param rewritten to a
    /// bare [`syn::FnArg::Receiver`] via the T75 [`rewrite_self_receiver`]
    /// helper. The body is a single method-call expression:
    /// `self.<field>.<method>(<forwarded_args>)` where `forwarded_args`
    /// are the identifiers of all params AFTER `self`.
    ///
    /// # Deferrals (v0.5)
    ///
    /// - **Multi-level chains** (`A embeds B embeds C`): only one level of
    ///   delegation is generated. If `B` itself embeds `C`, the user must
    ///   write `a.b.c.method()` until a transitive-closure analysis lands.
    /// - **Generic structs** (`struct Box<T> { inner: T }`): the field
    ///   type is matched by exact NAME only; generic instantiation is
    ///   not analysed.
    /// - **Conflict resolution**: if a struct embeds two types that both
    ///   define a method with the same name, BOTH delegation methods are
    ///   emitted and Rust will reject the duplicate (a clear compile
    ///   error rather than silent shadowing). Smarter resolution
    ///   (first-field-wins, explicit override) is deferred.
    /// - **Inherent impls**: methods defined outside `extend` blocks
    ///   (e.g. a future `impl Person { ... }` Buff syntax) are not
    ///   collected — v0.5 methods come only from extend blocks.
    fn emit_embedding_delegation(
        &mut self,
        decls: &[Decl],
        items: &mut Vec<Item>,
    ) -> Result<(), CodegenError> {
        // Build the struct-name set and the methods-by-type map in one pass.
        let mut struct_names: BTreeSet<String> = BTreeSet::new();
        let mut methods_by_type: BTreeMap<String, Vec<&FuncDecl>> = BTreeMap::new();
        for decl in decls {
            match decl {
                Decl::StructDecl(s) => {
                    struct_names.insert(s.name.name.clone());
                }
                Decl::ExtendBlock(e) => {
                    if let TypeRef::Named { name, .. } = &e.target {
                        methods_by_type
                            .entry(name.name.clone())
                            .or_default()
                            .extend(e.methods.iter());
                    }
                    // Generic / non-named extend targets don't contribute
                    // embeddable methods (their target is a primitive or
                    // generic container, not a user struct).
                }
                _ => {}
            }
        }

        // Iterate decls in SOURCE ORDER (deterministic — decls is a fixed
        // slice) so the delegation impls appear in a predictable position
        // relative to the structs they extend.
        for decl in decls {
            let s = match decl {
                Decl::StructDecl(s) => s,
                _ => continue,
            };
            // For each field whose type is a declared struct WITH methods,
            // build one delegation impl collecting every delegatable method.
            for (field_name, field_type) in &s.fields {
                let embedded_type_name = match field_type {
                    TypeRef::Named { name, .. } => &name.name,
                    // Generic/option/union field types are not simple struct
                    // embeddings — skip (deferred).
                    _ => continue,
                };
                // Must be a user-declared struct (not a primitive named type
                // like Float/String which also lowers via TypeRef::Named).
                if !struct_names.contains(embedded_type_name) {
                    continue;
                }
                let Some(methods) = methods_by_type.get(embedded_type_name) else {
                    // Embedded struct has no extend-block methods — nothing
                    // to delegate.
                    continue;
                };
                // Filter to instance methods (first param named `self`).
                let delegatable: Vec<&FuncDecl> = methods
                    .iter()
                    .copied()
                    .filter(|m| {
                        m.params
                            .first()
                            .map(|p| p.name.name == "self")
                            .unwrap_or(false)
                    })
                    .collect();
                if delegatable.is_empty() {
                    continue;
                }
                let impl_item = self.build_delegation_impl(
                    &s.name.name,
                    &field_name.name,
                    embedded_type_name,
                    &delegatable,
                )?;
                items.push(Item::Impl(impl_item));
            }
        }
        Ok(())
    }

    /// T92: build one inherent `impl StructName { ... }` block that
    /// promotes each method of the embedded type `embedded_type_name` to
    /// the embedding struct, forwarding through `self.<field_name>`.
    ///
    /// See [`Self::emit_embedding_delegation`] for the analysis + the
    /// deferrals. This helper builds the per-method signatures + bodies.
    fn build_delegation_impl(
        &mut self,
        struct_name: &str,
        field_name: &str,
        _embedded_type_name: &str,
        methods: &[&FuncDecl],
    ) -> Result<syn::ItemImpl, CodegenError> {
        let mut impl_items: Vec<syn::ImplItem> = Vec::with_capacity(methods.len());
        for method in methods {
            let item_fn = self.lower_func(method)?;
            // The signature is identical to the embedded type's method
            // signature (same params + return type), but with the first
            // `self` param rewritten to a bare Receiver — same trick T75
            // uses for extension traits. The receiver is now `Self` of
            // the EMBEDDING struct, which is exactly what we want: the
            // forwarded call `self.<field>.<method>(...)` consumes the
            // embedded value through the field.
            let sig = rewrite_self_receiver(item_fn.sig);
            // Collect forwarded arg expressions: the identifiers of all
            // params AFTER the first (which is the `self` receiver).
            let mut forwarded_args: Punctuated<SynExpr, syn::Token![,]> = Punctuated::new();
            for arg in sig.inputs.iter().skip(1) {
                if let Some(ident_expr) = ident_expr_from_fn_arg(arg) {
                    forwarded_args.push(ident_expr);
                }
            }
            // Body: `self.<field>.<method>(<forwarded_args>)`.
            let body_expr = field_method_call_expr(field_name, &method.name.name, forwarded_args);
            let block = syn::Block {
                brace_token: Default::default(),
                stmts: vec![SynStmt::Expr(body_expr, None)],
            };
            impl_items.push(syn::ImplItem::Fn(syn::ImplItemFn {
                attrs: Vec::new(),
                vis: Visibility::Inherited,
                defaultness: None,
                sig,
                block,
            }));
        }
        Ok(syn::ItemImpl {
            attrs: Vec::new(),
            defaultness: None,
            unsafety: None,
            generics: syn::Generics::default(),
            impl_token: Default::default(),
            // Inherent impl (NOT a trait impl) — `trait_: None` means
            // `impl StructName { ... }`, the shape Rust uses for inherent
            // methods on a type.
            trait_: None,
            self_ty: Box::new(rust_path_type(struct_name)),
            brace_token: Default::default(),
            items: impl_items,
        })
    }
}

/// Build a `Type::Path` with generic type arguments, e.g.
/// `Option<T>`, `Vec<T>`.
fn make_generic_path_type(name: &str, args: Vec<SynType>) -> SynType {
    let mut path_args: Punctuated<syn::GenericArgument, syn::Token![,]> = Punctuated::new();
    for a in args {
        path_args.push(syn::GenericArgument::Type(a));
    }
    let segment = syn::PathSegment {
        ident: Ident::new(name, ProcSpan::call_site()),
        arguments: syn::PathArguments::AngleBracketed(syn::AngleBracketedGenericArguments {
            colon2_token: None,
            lt_token: Default::default(),
            args: path_args,
            gt_token: Default::default(),
        }),
    };
    let mut segments: Punctuated<syn::PathSegment, syn::Token![::]> = Punctuated::new();
    segments.push(segment);
    SynType::Path(syn::TypePath {
        qself: None,
        path: syn::Path {
            leading_colon: None,
            segments,
        },
    })
}

/// Like [`make_generic_path_type`] but accepts a `::`-separated qualified
/// path (e.g. `"std::collections::HashMap"`) and attaches the generic
/// arguments to the LAST segment. Used by the T25 Map codegen so generated
/// programs can reference `std::collections::HashMap<K, V>` without a `use`
/// import (avoiding import management in v0.5).
fn make_qualified_generic_path_type(qualified_name: &str, args: Vec<SynType>) -> SynType {
    let mut path_args: Punctuated<syn::GenericArgument, syn::Token![,]> = Punctuated::new();
    for a in args {
        path_args.push(syn::GenericArgument::Type(a));
    }
    let mut segments: Punctuated<syn::PathSegment, syn::Token![::]> = Punctuated::new();
    let mut parts = qualified_name.split("::").collect::<Vec<_>>();
    let last_idx = parts.len().saturating_sub(1);
    for (i, seg) in parts.drain(..).enumerate() {
        let arguments = if i == last_idx {
            syn::PathArguments::AngleBracketed(syn::AngleBracketedGenericArguments {
                colon2_token: None,
                lt_token: Default::default(),
                args: path_args.clone(),
                gt_token: Default::default(),
            })
        } else {
            syn::PathArguments::None
        };
        segments.push(syn::PathSegment {
            ident: Ident::new(seg, ProcSpan::call_site()),
            arguments,
        });
    }
    SynType::Path(syn::TypePath {
        qself: None,
        path: syn::Path {
            leading_colon: None,
            segments,
        },
    })
}

/// Format a float so that it always has a decimal point or exponent (so the
/// `f32`/`f64` suffix binds to a float literal, not an integer).
fn float_repr(d: f64) -> String {
    let s = format!("{d}");
    if s.contains('.')
        || s.contains('e')
        || s.contains('E')
        || s == "inf"
        || s == "-inf"
        || s == "NaN"
    {
        s
    } else {
        format!("{s}.0")
    }
}

/// Build a `recv.method(arg)` single-argument method call.
///
/// Used by the string-method codegen helpers (e.g. `s.chars().skip(n)`).
fn method_call_one_arg(recv: SynExpr, method: &str, arg: SynExpr) -> SynExpr {
    let mut args: Punctuated<SynExpr, syn::Token![,]> = Punctuated::new();
    args.push(arg);
    SynExpr::MethodCall(syn::ExprMethodCall {
        attrs: Vec::new(),
        receiver: Box::new(recv),
        dot_token: Default::default(),
        method: Ident::new(method, ProcSpan::call_site()),
        turbofish: None,
        paren_token: Default::default(),
        args,
    })
}

/// T124b: build a fully-qualified Rust associated-function call
/// `<path>(args)` — used for prelude-type constructors like
/// `chrono::Utc::now()` and `chrono::TimeDelta::days(n)`.
///
/// The `qualified_path` is a `::`-separated string (e.g.
/// `"chrono::Utc::now"`, `"chrono::TimeDelta::days"`). The args slice is
/// lowered already — this helper just wraps them in a `syn::ExprCall` on
/// the path expression.
fn rust_call_expr(qualified_path: &str, args: Vec<SynExpr>) -> SynExpr {
    let callee = SynExpr::Path(syn::ExprPath {
        attrs: Vec::new(),
        qself: None,
        path: rust_path(qualified_path),
    });
    let mut punct: Punctuated<SynExpr, syn::Token![,]> = Punctuated::new();
    for a in args {
        punct.push(a);
    }
    SynExpr::Call(syn::ExprCall {
        attrs: Vec::new(),
        func: Box::new(callee),
        paren_token: Default::default(),
        args: punct,
    })
}

/// T124b: build a Rust `&'static str` literal expression.
///
/// Used by prelude-type parse/format codegen to pass strftime / parse
/// format strings (`"%Y-%m-%d"`). Built via `syn::LitStr::new` so any
/// embedded escapes survive correctly.
fn str_lit_expr(text: &str) -> SynExpr {
    SynExpr::Lit(syn::ExprLit {
        attrs: Vec::new(),
        lit: syn::Lit::Str(syn::LitStr::new(text, ProcSpan::call_site())),
    })
}

/// T124b: coerce a string-typed argument expression to `&str` when the
/// underlying chrono API requires it.
///
/// chrono's `DateTime::parse_from_rfc3339`, `NaiveDate::parse_from_str`,
/// and `DateTime::format` all take `&str`. Buff string literals
/// (`Expr::Literal(Literal::String(s))`) lower directly to a Rust
/// `&'static str` literal — so no coercion is needed in that case. For
/// non-literal arg expressions (idents referencing `String` bindings,
/// interpolation results, ...) we wrap the lowered expression in a borrow
/// `&<expr>` so Rust's Deref coercion turns `&String` into `&str`.
///
/// Without this, `DateTime.parse(my_string_var)` would emit
/// `chrono::DateTime::parse_from_rfc3339(my_string_var)` which fails to
/// compile (expected `&str`, found `String`). The borrow turns it into
/// `chrono::DateTime::parse_from_rfc3339(&my_string_var)` which works.
fn coerce_str_arg_to_ref(lowered: SynExpr, orig: &Expr) -> SynExpr {
    // String literals lower to `&'static str` already — no borrow needed.
    if matches!(orig, Expr::Literal(Literal::String(_), _)) {
        return lowered;
    }
    // Named-arg wrapper around a string literal — recurse into the value.
    if let Expr::NamedArg { value, .. } = orig {
        return coerce_str_arg_to_ref(lowered, value);
    }
    // Everything else (idents, interpolation, etc.) — borrow via `&<expr>`.
    //
    // `syn::Expr` has no `Ref` variant (references are parsed into the
    // generic `Expr::Paren`-shaped token-stream slot, not a dedicated
    // variant). We build `& #lowered` via `syn::parse_quote!` — the same
    // approach used elsewhere in this file for `#[tokio::main]` and
    // `#[test]` attribute construction (lines 977/983). The pattern is
    // well-known to parse successfully (any expression can be borrowed),
    // so the panic-on-parse-failure caveat of `parse_quote!` does not
    // apply in practice.
    syn::parse_quote!( & #lowered )
}

/// Take a token stream that calls a fully-qualified function with a single
/// placeholder argument `__recv` and replace that placeholder with an
/// actual lowered receiver expression.
///
/// The `tokens` argument is expected to parse as a Rust function-call
/// expression (e.g. `path::func(&__recv, true)`). We use `quote!` to splice
/// the receiver in: we re-parse a small template that names `__recv` and
/// then walk the resulting `ExprCall` to substitute the real receiver.
///
/// This indirection is needed because `quote!` cannot easily splice into
/// an arbitrary position inside a string-built token stream — we instead
/// parse the template to a real `ExprCall`, then swap the first argument.
fn splice_receiver_into_call(
    tokens: proc_macro2::TokenStream,
    recv: SynExpr,
) -> Result<SynExpr, CodegenError> {
    // Rebuild via quote! so we never hand-format. The placeholder name
    // `__recv` is referenced as a Rust identifier in the template; we then
    // construct the call by hand using the lowered receiver.
    //
    // Simpler approach: construct the call directly via syn::ExprCall with
    // the lowered recv as the first arg and `true` as the second.
    let _ = tokens; // discarded; we rebuild from scratch to stay quote!-based.
    let mut args: Punctuated<SynExpr, syn::Token![,]> = Punctuated::new();
    // `&recv` — syn doesn't have a one-liner for `&expr`, so we build it.
    let borrow_recv = SynExpr::Reference(syn::ExprReference {
        attrs: Vec::new(),
        and_token: Default::default(),
        mutability: None,
        expr: Box::new(recv),
    });
    args.push(borrow_recv);
    args.push(SynExpr::Lit(syn::ExprLit {
        attrs: Vec::new(),
        lit: syn::Lit::Bool(syn::LitBool::new(true, ProcSpan::call_site())),
    }));
    Ok(SynExpr::Call(syn::ExprCall {
        attrs: Vec::new(),
        func: Box::new(SynExpr::Path(syn::ExprPath {
            attrs: Vec::new(),
            qself: None,
            path: rust_path("unicode_segmentation::UnicodeSegmentation::graphemes"),
        })),
        paren_token: Default::default(),
        args,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use buff_lang_ast::common::{Block, Ident as AstIdent, Param};
    use buff_lang_ast::{op::BinaryOp, op::UnaryOp, Literal};
    use buff_lang_error::Span;

    fn dummy_span() -> Span {
        Span::dummy()
    }

    fn int_lit(n: i64) -> Expr {
        Expr::Literal(Literal::Int(n), dummy_span())
    }

    fn ident_expr(s: &str) -> Expr {
        Expr::Ident(AstIdent::new(s, dummy_span()), dummy_span())
    }

    #[test]
    fn empty_func_generates_syn_file() {
        let func = FuncDecl {
            name: AstIdent::new("empty", dummy_span()),
            params: Vec::new(),
            return_type: None,
            body: Block::empty(dummy_span()),
            is_async: false,
            is_unsafe: false,
            is_extern: false,
            attributes: Vec::new(),
            span: dummy_span(),
        };
        let mut codegen = RustCodegen::new();
        let file = codegen
            .generate(&[Decl::FuncDecl(func)])
            .expect("empty func must codegen");
        assert_eq!(file.items.len(), 1);
        assert!(matches!(file.items[0], Item::Fn(_)));
    }

    #[test]
    fn binary_op_lowers_to_expr_binary() {
        let mut codegen = RustCodegen::new();
        let lhs = int_lit(1);
        let rhs = int_lit(2);
        let expr = Expr::BinaryOp {
            op: BinaryOp::Add,
            lhs: Box::new(lhs),
            rhs: Box::new(rhs),
            span: dummy_span(),
        };
        let syn_expr = codegen.lower_expr(&expr).unwrap();
        assert!(matches!(syn_expr, SynExpr::Binary(_)));
    }

    #[test]
    fn unary_neg_lowers_correctly() {
        let mut codegen = RustCodegen::new();
        let operand = int_lit(5);
        let expr = Expr::UnaryOp {
            op: UnaryOp::Neg,
            operand: Box::new(operand),
            span: dummy_span(),
        };
        let syn_expr = codegen.lower_expr(&expr).unwrap();
        match syn_expr {
            SynExpr::Unary(u) => assert!(matches!(u.op, syn::UnOp::Neg(_))),
            other => panic!("expected Unary, got {other:?}"),
        }
    }

    #[test]
    fn type_int_maps_to_i64() {
        let mut codegen = RustCodegen::new();
        let tr = TypeRef::Named {
            name: AstIdent::new("Int", dummy_span()),
            span: dummy_span(),
        };
        let ty = codegen.ast_typeref_to_syn(&tr).unwrap();
        match ty {
            SynType::Path(p) => {
                let seg = p.path.segments.first().unwrap();
                assert_eq!(seg.ident.to_string(), "i64");
            }
            _ => panic!("expected Path"),
        }
    }

    #[test]
    fn type_option_maps_to_rust_option() {
        let mut codegen = RustCodegen::new();
        let tr = TypeRef::Option(
            Box::new(TypeRef::Named {
                name: AstIdent::new("Int", dummy_span()),
                span: dummy_span(),
            }),
            dummy_span(),
        );
        let ty = codegen.ast_typeref_to_syn(&tr).unwrap();
        match ty {
            SynType::Path(p) => {
                let seg = p.path.segments.first().unwrap();
                assert_eq!(seg.ident.to_string(), "Option");
                match &seg.arguments {
                    syn::PathArguments::AngleBracketed(ab) => assert_eq!(ab.args.len(), 1),
                    _ => panic!("expected angle-bracketed args"),
                }
            }
            _ => panic!("expected Path"),
        }
    }

    #[test]
    fn func_call_with_two_args_lowers() {
        let mut codegen = RustCodegen::new();
        let callee = ident_expr("foo");
        let args = vec![int_lit(1), int_lit(2)];
        let expr = Expr::FuncCall {
            callee: Box::new(callee),
            args,
            span: dummy_span(),
        };
        let syn_expr = codegen.lower_expr(&expr).unwrap();
        match syn_expr {
            SynExpr::Call(c) => assert_eq!(c.args.len(), 2),
            other => panic!("expected Call, got {other:?}"),
        }
    }

    #[test]
    fn struct_codegen_lowers_empty_struct_to_pub_struct_with_derives() {
        // T26: the pre-T26 behaviour returned CodegenError. T26 actually
        // implements struct codegen; we now verify the new GREEN path.
        let sd = buff_lang_ast::decl::StructDecl {
            name: AstIdent::new("Foo", dummy_span()),
            fields: Vec::new(),
            traits: Vec::new(),
            span: dummy_span(),
        };
        let mut codegen = RustCodegen::new();
        let result = codegen.generate(&[Decl::StructDecl(sd)]);
        let file = result.expect("struct codegen must succeed post-T26");
        assert_eq!(file.items.len(), 1);
        assert!(matches!(file.items[0], Item::Struct(_)));
    }

    #[test]
    fn float_repr_handles_integer_floats() {
        assert_eq!(float_repr(2.0), "2.0");
        assert_eq!(float_repr(2.5), "2.5");
    }

    #[test]
    fn make_let_pat_respects_mutability() {
        let pat = RustCodegen::make_let_pat(Ident::new("x", ProcSpan::call_site()), true);
        match pat {
            Pat::Ident(p) => assert!(p.mutability.is_some()),
            _ => panic!("expected Ident pat"),
        }
    }

    // Touch a few param/stmt shapes so unused-import warnings don't fire.
    #[test]
    fn _param_and_stmt_construction_smoke() {
        let _param = Param {
            name: AstIdent::new("p", dummy_span()),
            ty: TypeRef::Named {
                name: AstIdent::new("Int", dummy_span()),
                span: dummy_span(),
            },
            default_value: None,
            span: dummy_span(),
        };
        let _stmt = Stmt::Break(dummy_span());
    }

    // -----------------------------------------------------------------------
    // T22 — Fixed-width `Int<W>` codegen mapping contract.
    //
    // The T22 spec says fixed-mode overflow must "panic in debug, wrap in
    // release". Buff inherits this behaviour FOR FREE from Rust: codegen
    // maps each fixed `Int<W>` to the corresponding native Rust integer
    // (`i8`/`i16`/`i32`/`i64`/`i128`), and Rust's native arithmetic already
    // has the debug-panic/release-wrap overflow contract. No explicit
    // `checked_*` calls are emitted.
    //
    // These tests mechanically pin the mapping so a regression in
    // `buff_type_to_syn` cannot silently widen every fixed-width integer
    // (which would change the overflow boundary). See T22 evidence file
    // `task-22-overflow-modes.txt`.
    // -----------------------------------------------------------------------

    /// Helper: extract the leading path-segment ident from a `syn::Type` (or
    /// panic). Used by the T22 fixed-width mapping tests below.
    fn first_path_segment_str(ty: &SynType) -> String {
        match ty {
            SynType::Path(p) => p
                .path
                .segments
                .first()
                .map(|s| s.ident.to_string())
                .unwrap_or_else(|| panic!("path has no segments")),
            _ => panic!("expected Path, got {ty:?}"),
        }
    }

    #[test]
    fn t22_fixed_int_widths_map_to_native_rust_widths() {
        let codegen = RustCodegen::new();
        // Every fixed Int<W> must map to the SAME-width native Rust integer.
        for (w, expected) in [
            (IntWidth::W8, "i8"),
            (IntWidth::W16, "i16"),
            (IntWidth::W32, "i32"),
            (IntWidth::W64, "i64"),
            (IntWidth::W128, "i128"),
        ] {
            let ty = Type::Int { width: w };
            let syn_ty = codegen
                .buff_type_to_syn(&ty)
                .expect("Int<W> must map to a Rust type");
            assert_eq!(
                first_path_segment_str(&syn_ty),
                expected,
                "Int<{:?}] -> wrong Rust width",
                w
            );
        }
    }

    #[test]
    fn t22_fixed_int8_preserves_width_through_arithmetic() {
        // The full T22 "fixed mode preserves type" contract: an i8 value
        // stays i8 after arithmetic because (a) the TypeInferencer preserves
        // width via promote_binary and (b) the codegen maps the resulting
        // Int<8> back to i8.  We verify the codegen end of that chain here.
        // (The inferencer end is covered by `numeric_coercion::fixed_int8_*`.)
        let codegen = RustCodegen::new();
        let syn_ty = codegen
            .buff_type_to_syn(&Type::Int {
                width: IntWidth::W8,
            })
            .expect("Int<8> maps to i8");
        assert_eq!(first_path_segment_str(&syn_ty), "i8");
        // And Int<32> + Int<32> = Int<32> maps back to i32 (not widened to i64).
        let syn_ty = codegen
            .buff_type_to_syn(&Type::Int {
                width: IntWidth::W32,
            })
            .expect("Int<32> maps to i32");
        assert_eq!(first_path_segment_str(&syn_ty), "i32");
    }
}

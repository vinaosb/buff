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
    Stmt, StructDecl as AstStructDecl, TypeParam, TypeRef,
};
use buff_lang_error::{CodegenError, Diagnostic, ErrorCode, Span as BuffSpan};
use buff_lang_types::{prelude::PreludeFn, FloatWidth, IntWidth, Type, TypeInferencer};

use crate::atomic_analysis::AtomicPromotions;
use crate::context::CodegenContext;
use crate::move_analysis::MoveAnalyzer;

// T105a: syn-construction helpers extracted to a child module. The child
// inherits this module's imports via `use super::*` (verbatim move, zero
// per-module import lists). Functions are `pub(super)` so the parent can
// reach them through the glob below.
mod syn_helpers;
use syn_helpers::*;
// Re-export so lib.rs's `pub use rust_codegen::buff_primitive_to_rust_name` still resolves.
pub use syn_helpers::buff_primitive_to_rust_name;
mod expr_lowering;
mod method_call_lowering;
mod decl_lowering;
mod type_lowering;
mod lowering_helpers;
use lowering_helpers::*;
mod conv_helpers;
use conv_helpers::*;
mod extern_crate_detection_extra;
use extern_crate_detection_extra::*;
mod extern_crate_detection;
use extern_crate_detection::*;
mod dependency_detection;
use dependency_detection::*;
// Re-export so lib.rs's `pub use rust_codegen::collect_rust_deps` still resolves.
pub use dependency_detection::collect_rust_deps;
mod derive_attrs;
use derive_attrs::*;

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
    /// T85: registry of USER-defined enum variants in this compilation
    /// unit. Maps `variant_name` (e.g. `"Red"`) → owning `enum_name`
    /// (e.g. `"Color"`). Populated by [`Self::generate`] BEFORE the main
    /// lowering loop, via the [`collect_user_enum_variants`] helper.
    ///
    /// Consulted by [`Self::lower_expr`]'s `Expr::Ident` arm and by
    /// [`Self::lower_pattern`]'s `Pattern::Ident` / `Pattern::Variant`
    /// arms so that a bare user-written `Red` (which the parser encodes
    /// as `Pattern::Ident("Red")` or `Pattern::Variant { enum_name: "",
    /// variant: "Red", .. }`) lowers to the fully-qualified Rust path
    /// `Color::Red`. Without this qualification rustc treats the bare
    /// `Red` in match-arms as a fresh binding pattern (silently shadowing
    /// the variant) and the bare `Red` in expression position as an
    /// unresolved identifier — both produce compile errors.
    ///
    /// Prelude enums (`Option`, `Result`) are EXCLUDED: their variants
    /// (`Some`/`None`/`Ok`/`Err`) live in the Rust prelude and MUST stay
    /// unqualified. Variant-name COLLISIONS (same name declared by two
    /// user enums) also remove the entry — ambiguous references are left
    /// unqualified so rustc produces the right diagnostic.
    ///
    /// A [`BTreeMap`] (not [`HashMap`]) for deterministic membership and
    /// iteration (the T29 flaky-test lesson — never rely on hash-seed-
    /// dependent iteration for codegen output).
    user_enum_variants: BTreeMap<String, String>,
    /// T86: depth of `return <expr>` operands we're currently lowering.
    /// Incremented by [`Self::lower_stmt`]'s `Stmt::Return` arm around
    /// the inner expression lowering so [`Self::lower_match_expr`] can
    /// detect it's operating in RETURN POSITION and strip the trailing
    /// `;` from each arm body block — without that strip, every arm
    /// body block lowers as `{ <expr>; }` whose Rust type is `()`
    /// (statement, not tail expression), making
    /// `return match n { A => 1, _ => 0 }` fail to typecheck against a
    /// non-`()` return type.
    ///
    /// The counter (not a boolean) is incremented by EVERY nested
    /// `return` so a `return match x { A => return 5, _ => 0 }` (whose
    /// inner arm body is itself a return) still works: the outer return
    /// sets depth=1; the inner return's match arm bodies consult depth
    /// (still ≥1) — but the inner return itself is fine because
    /// `Stmt::Return` always wraps in `SynStmt::Expr(_, Some(semi))`
    /// regardless of depth.
    ///
    /// Stays ≥1 for the ENTIRE expression tree under a return
    /// (intentionally): a `return if c { match x { ... } } else { 0 }`
    /// needs the INNER match's arm bodies stripped too, because the
    /// whole expression must yield the function's return type.
    return_position_depth: usize,
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
            user_enum_variants: BTreeMap::new(),
            return_position_depth: 0,
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
        // T124d: register the `regex` crate as an external dependency when
        // the program references the prelude `Regex` module
        // (`Regex.compile(p)`, `regex.match(...)`, `regex.find(...)`,
        // `regex.replace(...)`, `regex.captures(...)`). Generated code
        // uses fully-qualified `regex::Regex::...` paths so no `use` import
        // is emitted — but the recorded name signals to the pipeline /
        // build-driver that the generated Cargo project must declare
        // `regex` in `[dependencies]`.
        //
        // Mirrors the chrono/tracing registration pattern (T124b/T124c):
        // single-file `buff run` rustc path does NOT link this crate;
        // the codegen-only linking boundary is the accepted acceptance
        // criterion for v1.4 prelude modules. Cargo-project wiring is
        // deferred (snapshots + extern_crates set is the verifiable
        // contract).
        if program_uses_regex(decls) {
            self.extern_crates.insert("regex".to_string());
        }
        // T124e: register the `toml` crate as an external dependency when
        // the program references the prelude `Toml` namespace module
        // (`Toml.parse(s)` / `Toml.stringify(v)`). Generated code uses
        // fully-qualified `toml::from_str` / `toml::to_string` paths so
        // no `use` import is emitted — but the recorded name signals to
        // the pipeline / build-driver that the generated Cargo project
        // must declare `toml` in `[dependencies]`.
        //
        // Mirrors the chrono/tracing/regex registration pattern
        // (T124b/T124c/T124d): single-file `buff run` rustc path does
        // NOT link this crate; the codegen-only linking boundary is the
        // accepted acceptance criterion for v1.4 prelude modules.
        // Cargo-project wiring is deferred (snapshots + extern_crates
        // set is the verifiable contract).
        if program_uses_toml(decls) {
            self.extern_crates.insert("toml".to_string());
        }
        // T124f: register the `rand` crate as an external dependency when
        // the program references the prelude `Random` namespace module
        // (`Random.int(lo, hi)`, `Random.float()`, `Random.choice(v)`,
        // `Random.shuffle(v)`). Generated code uses fully-qualified
        // `rand::rng()` / `rand::seq::IndexedRandom::*` paths so
        // no `use` import is emitted - but the recorded name signals to
        // the pipeline / build-driver that the generated Cargo project
        // must declare `rand` in `[dependencies]`.
        //
        // NOTE: `Math` and `Strings` (also T124f) wrap Rust `std` only
        // (no extern crate needed) - they have NO `program_uses_X`
        // walker for that reason. `Sort` (`.sort()` / `.sort_by()` on
        // Buff's existing Vector type) is an instance method that lowers
        // to Rust's slice `.sort()` / `.sort_by()` (also std-only, no
        // extern crate).
        //
        // Mirrors the chrono/tracing/regex/toml registration pattern
        // (T124b/T124c/T124d/T124e): single-file `buff run` rustc path
        // does NOT link this crate; the codegen-only linking boundary
        // is the accepted acceptance criterion for v1.4 prelude modules.
        // Cargo-project wiring is deferred (snapshots + extern_crates
        // set is the verifiable contract).
        if program_uses_rand(decls) {
            self.extern_crates.insert("rand".to_string());
        }
        // T124g: register the `tokio` crate as an external dependency when
        // the program references the prelude `sleep(duration)` free fn.
        // Generated code lowers `sleep(d)` to
        // `tokio::time::sleep(<d>).await`, so any program using `sleep`
        // transitively depends on the `tokio` crate being in
        // `[dependencies]` (and on the enclosing fn being async —
        // `#[tokio::main]` is auto-stamped when `main` propagates to
        // async via the T31 walker; sleep() calls in non-async fns are a
        // rustc-level error, surfaced as a normal Rust diagnostic).
        //
        // NOTE: tokio is ALREADY used by the v1.0 async lowering
        // (`tokio::spawn`, `tokio::runtime::Runtime`, `#[tokio::main]`)
        // but the async pass does NOT register the crate in
        // `extern_crates` — single-file `buff run` rustc path never
        // linked tokio either, mirroring the chrono/tracing/regex/toml
        // codegen-only boundary. The walker here is the FIRST time
        // `tokio` enters `extern_crates`; the existing async codegen
        // paths don't need to start recording it because their generated
        // `tokio::*` paths compile iff tokio is in the (deferred)
        // Cargo project's `[dependencies]`, which is exactly what this
        // walker signals.
        //
        // Walker scope: NARROW (per the T124f gotcha that chrono was
        // originally over-broad). It flags ONLY a `FuncCall` whose
        // callee is the bare Ident `sleep` — NOT every async fn, NOT
        // every `tokio::*` path fragment in the lowering (the lowering
        // is a codegen-private concern; the walker is a USER-INTENT
        // detector). Same shape as the rand walker flagging
        // `Random.<method>(...)`.
        if program_uses_tokio(decls) {
            self.extern_crates.insert("tokio".to_string());
        }
        // T124h: register the FIVE web-module extern crates (Base64 /
        // Hex / URLEncode / UUID / URL) when the program references the
        // corresponding prelude modules. Each walker is NARROW - it
        // flags ONLY the specific receiver name (`Base64` / `Hex` /
        // `URLEncode` / `UUID` / `URL`), mirroring the rand / tokio
        // walker pattern (T124f / T124g). The chrono over-broad-walker
        // gotcha (T124f) is the cautionary tale: each walker stays
        // minimal so it doesn't over-trigger on unrelated code.
        //
        // Generated code uses fully-qualified paths so no `use` import
        // is emitted - but the recorded name signals to the pipeline /
        // build-driver that the generated Cargo project must declare
        // each crate in `[dependencies]`.
        //
        // Mirrors the chrono/tracing/regex/toml/rand/tokio registration
        // pattern (T124b/T124c/T124d/T124e/T124f/T124g): single-file
        // `buff run` rustc path does NOT link these crates; the
        // codegen-only linking boundary is the accepted acceptance
        // criterion for v1.4 prelude modules. Cargo-project wiring is
        // deferred (snapshots + extern_crates set is the verifiable
        // contract).
        if program_uses_base64(decls) {
            self.extern_crates.insert("base64".to_string());
        }
        if program_uses_hex(decls) {
            self.extern_crates.insert("hex".to_string());
        }
        if program_uses_percent_encoding(decls) {
            self.extern_crates.insert("percent-encoding".to_string());
        }
        if program_uses_uuid(decls) {
            self.extern_crates.insert("uuid".to_string());
        }
        if program_uses_url(decls) {
            self.extern_crates.insert("url".to_string());
        }
        // T124i: register the `serde_yml` and `csv` crates as external
        // dependencies when the program references the corresponding
        // prelude modules (`Yaml.parse(s)` / `Yaml.stringify(v)` /
        // `Csv.parse(s)` / `Csv.stringify(rows)`). Generated code uses
        // fully-qualified `serde_yml::from_str` / `serde_yml::to_string`
        // and `csv::ReaderBuilder::...` / `csv::Writer::from_writer`
        // paths so no `use` import is emitted - but the recorded name
        // signals to the pipeline / build-driver that the generated
        // Cargo project must declare each crate in `[dependencies]`.
        //
        // NOTE: `serde_yml` is the maintained fork of the
        // deprecated/archived `serde_yaml` crate (do NOT use
        // serde_yaml). The crate name is recorded as `serde_yml` (with
        // underscore) matching the path segments emitted by codegen.
        //
        // Mirrors the chrono/tracing/regex/toml/rand/tokio/base64/hex/
        // percent-encoding/uuid/url registration pattern
        // (T124b/T124c/T124d/T124e/T124f/T124g/T124h): single-file
        // `buff run` rustc path does NOT link these crates; the
        // codegen-only linking boundary is the accepted acceptance
        // criterion for v1.4 prelude modules. Cargo-project wiring is
        // deferred (snapshots + extern_crates set is the verifiable
        // contract).
        if program_uses_serde_yml(decls) {
            self.extern_crates.insert("serde_yml".to_string());
        }
        // T23: register the `serde_json` crate as an external dependency
        // when the program references the `Json` prelude namespace
        // (`Json.parse(s)` / `Json.stringify(v)`). Generated code uses
        // fully-qualified `serde_json::from_str` / `serde_json::to_string`
        // paths so no `use` import is emitted - but the recorded name
        // signals to the pipeline / build-driver that the generated
        // Cargo project must declare `serde_json` in `[dependencies]`.
        //
        // Mirrors the serde_yml / csv registration pattern (T124i):
        // single-file `buff run` rustc path does NOT link these crates;
        // the codegen-only linking boundary is the accepted acceptance
        // criterion for v1.4+ prelude modules.
        if program_uses_serde_json(decls) {
            self.extern_crates.insert("serde_json".to_string());
        }
        if program_uses_csv(decls) {
            self.extern_crates.insert("csv".to_string());
        }
        // T124j: register the `walkdir` + `tempfile` crates as external
        // dependencies when the program references the corresponding
        // prelude modules (`Dir.walk(p)` / `Tempfile.create()` /
        // `Tempfile.dir()`). Generated code uses fully-qualified
        // `walkdir::WalkDir::new(...)` and `tempfile::NamedTempFile::new()`
        // paths so no `use` import is emitted - but the recorded name
        // signals to the pipeline / build-driver that the generated
        // Cargo project must declare each crate in `[dependencies]`.
        //
        // NOTE: `Path` (value type) + `Dir.list/create/remove` +
        // `Path.exists` wrap `std::path::Path`/`PathBuf` + `std::fs::*`
        // (std-only - NO extern crate needed for those, mirroring the
        // Math/Strings/Args/Env stance from T124f/T124g). `Tempfile.dir`
        // uses `std::env::temp_dir()` (also std-only), but the narrow
        // walker still records `tempfile` for symmetry (any Tempfile.*
        // call flags the crate).
        //
        // Mirrors the chrono/tracing/regex/toml/rand/tokio/base64/hex/
        // percent-encoding/uuid/url/serde_yml/csv registration pattern
        // (T124b/T124c/T124d/T124e/T124f/T124g/T124h/T124i): single-
        // file `buff run` rustc path does NOT link these crates; the
        // codegen-only linking boundary is the accepted acceptance
        // criterion for v1.4 prelude modules. Cargo-project wiring is
        // deferred (snapshots + extern_crates set is the verifiable
        // contract).
        if program_uses_walkdir(decls) {
            self.extern_crates.insert("walkdir".to_string());
        }
        if program_uses_tempfile(decls) {
            self.extern_crates.insert("tempfile".to_string());
        }
        // T124k: register the `sha2` + `md5` + `hmac` + `hex` crates
        // as external dependencies when the program references the
        // corresponding prelude modules (`Hash.sha256(data)` /
        // `Hash.sha512(data)` / `Hash.md5(data)` /
        // `HMAC.sha256(key, data)`). Generated code uses
        // fully-qualified `sha2::Sha256::digest` / `sha2::Sha512::digest`
        // / `md5::compute` / `hmac::Hmac::<sha2::Sha256>::...` paths
        // (plus block-scoped `use sha2::Digest;` / `use hmac::Mac;`
        // for the trait methods) so no top-level `use` import is
        // emitted - but the recorded name signals to the pipeline /
        // build-driver that the generated Cargo project must declare
        // each crate in `[dependencies]`.
        //
        // NOTE: `Hash.sha256` / `Hash.sha512` both record `sha2`
        // (the SHA-2 family crate ships both digesters);
        // `Hash.md5` records `md5`; `HMAC.sha256` records `hmac`
        // + `sha2` (HMAC wraps `hmac::Hmac<sha2::Sha256>` so the
        // generated path needs both). `hex` is recorded alongside
        // each (the hex encoding is shared with T124h Hex module's
        // walker; re-recording is idempotent - extern_crates is a
        // BTreeSet).
        //
        // The narrow walkers (per-method) flag the SPECIFIC method
        // names - sha256/sha512 -> sha2; md5 -> md5; HMAC.sha256 ->
        // hmac + sha2. Mirrors the chrono-over-broad gotcha (T124f):
        // a generic `program_uses_namespace("Hash")` would
        // over-register (a program using only `Hash.md5` shouldn't
        // need `sha2`).
        //
        // Mirrors the chrono/tracing/regex/toml/rand/tokio/base64/hex/
        // percent-encoding/uuid/url/serde_yml/csv/walkdir/tempfile
        // registration pattern (T124b/T124c/T124d/T124e/T124f/T124g/
        // T124h/T124i/T124j): single-file `buff run` rustc path does
        // NOT link these crates; the codegen-only linking boundary is
        // the accepted acceptance criterion for v1.4 prelude modules.
        // Cargo-project wiring is deferred (snapshots + extern_crates
        // set is the verifiable contract).
        if program_uses_sha2(decls) {
            self.extern_crates.insert("sha2".to_string());
        }
        if program_uses_md5(decls) {
            self.extern_crates.insert("md5".to_string());
        }
        if program_uses_hmac(decls) {
            self.extern_crates.insert("hmac".to_string());
            // HMAC.sha256 lowers to `hmac::Hmac<sha2::Sha256>` so
            // the `sha2` crate is needed alongside `hmac`. Record
            // `sha2` here too (idempotent if the program also uses
            // Hash.sha256/sha512 - extern_crates is a BTreeSet).
            self.extern_crates.insert("sha2".to_string());
        }
        if program_uses_sha2(decls) || program_uses_md5(decls) || program_uses_hmac(decls) {
            // Every Hash.* / HMAC.* call emits a `hex::encode(...)`
            // for the digest / MAC bytes. Record `hex` alongside
            // (shared with T124h Hex module's walker; idempotent).
            self.extern_crates.insert("hex".to_string());
        }
        // T124l: register the `num_cpus` crate as an external
        // dependency when the program references `OS.cpus()`.
        // Generated code uses the fully-qualified
        // `num_cpus::get() as i64` path so no top-level `use`
        // import is emitted - but the recorded name signals to
        // the pipeline / build-driver that the generated Cargo
        // project must declare `num_cpus` in `[dependencies]`.
        //
        // NOTE: `OS.name` / `OS.arch` / `OS.hostname` use
        // `std::env::consts::{OS,ARCH}` + env-var hostname
        // fallback (std-only - NO extern crate needed, mirrors
        // the Math/Strings/Args/Env stance from T124f/T124g).
        // `Process.*` uses `std::process::*` (std-only - NO
        // extern crate needed, mirrors the Path/Dir.list stance
        // from T124j). The narrow `program_uses_num_cpus` walker
        // flags ONLY the `OS.cpus` method name (mirrors the
        // chrono-over-broad cautionary tale, T124f gotcha: a
        // generic `program_uses_namespace("OS")` would
        // over-register num_cpus for programs using only
        // `OS.name` / `OS.arch` / `OS.hostname`).
        //
        // Mirrors the chrono/tracing/regex/toml/rand/tokio/base64/
        // hex/percent-encoding/uuid/url/serde_yml/csv/walkdir/
        // tempfile/sha2/md5/hmac registration pattern: single-file
        // `buff run` rustc path does NOT link num_cpus; cargo-
        // project wiring is deferred (snapshots + extern_crates
        // set is the verifiable contract).
        if program_uses_num_cpus(decls) {
            self.extern_crates.insert("num_cpus".to_string());
        }
        // T124m: register the `tokio` crate for `TCP.*` / `UDP.*`
        // calls (idempotent with the existing tokio walker that
        // flags ONLY `sleep(...)` free-fn calls - the existing
        // walker does NOT fire on TCP.* / UDP.* / WebSocket.*
        // method calls). `WebSocket.*` records `tokio-tungstenite`
        // + `futures-util` via the narrow
        // `program_uses_tokio_tungstenite` walker; `tokio` is
        // pulled transitively by `tokio-tungstenite`'s dependency
        // on it (so a WebSocket-only program would have tokio in
        // its Cargo.lock even without an explicit tokio dep), but
        // we record tokio explicitly for clarity (mirrors how we
        // also record sha2 for HMAC.sha256 even though hmac pulls
        // it transitively).
        //
        // Generated code uses fully-qualified `tokio::net::*` /
        // `tokio::io::*` / `tokio_tungstenite::*` /
        // `futures_util::*` paths so no top-level `use` import is
        // emitted - but the recorded names signal to the pipeline
        // / build-driver that the generated Cargo project must
        // declare each crate in `[dependencies]`.
        //
        // Mirrors the chrono/tracing/regex/toml/rand/tokio/base64/
        // hex/percent-encoding/uuid/url/serde_yml/csv/walkdir/
        // tempfile/sha2/md5/hmac/num_cpus registration pattern
        // (T124b..T124l): single-file `buff run` rustc path does
        // NOT link these crates; cargo-project wiring is deferred
        // (snapshots + extern_crates set is the verifiable
        // contract). The `.await` calls surface a rustc-level
        // error if the enclosing function is not async (T31 walker
        // propagates async-ness ONLY through bare-Ident free-fn
        // calls, NOT method-call / namespace-assoc-fn calls, so
        // the enclosing-fn-async transformation is a deferral;
        // see issues.md).
        if program_uses_tcp(decls)
            || program_uses_udp(decls)
            || program_uses_tokio_tungstenite(decls)
        {
            self.extern_crates.insert("tokio".to_string());
        }
        if program_uses_tokio_tungstenite(decls) {
            self.extern_crates.insert("tokio-tungstenite".to_string());
            self.extern_crates.insert("futures-util".to_string());
        }
        // T2: register `buff-lang-runtime` (the in-tree runtime
        // abstraction crate) when the program references the
        // prelude `Channel` module (`Channel.new(...)`). Generated
        // code uses fully-qualified `buff_lang_runtime::Channel::new`
        // + `Sender` + `Receiver` paths so no top-level `use` import
        // is emitted - but the recorded name signals to the pipeline
        // / build-driver that the generated Cargo project must
        // declare `buff-lang-runtime` in `[dependencies]`. Also
        // records `tokio` transitively (the runtime crate wraps
        // `tokio::sync::mpsc` per Metis G6). Mirrors the chrono /
        // regex / toml / etc. registration pattern.
        if program_uses_namespace(decls, "Channel") {
            self.extern_crates.insert("buff-lang-runtime".to_string());
            self.extern_crates.insert("tokio".to_string());
        }
        // T9: register `buff-image` when the program references the
        // prelude `Image` module (`Image.from_path(...)` etc.). The
        // generated code uses fully-qualified `buff_image::Image::*`
        // paths so no top-level `use` import is emitted — but the
        // recorded name signals to the pipeline / build-driver that
        // the generated Cargo project must declare `buff-image` in
        // `[dependencies]`. Also records `image` transitively (the
        // wrapper crate wraps `image::DynamicImage`). Mirrors the
        // Channel / chrono / regex / toml registration pattern.
        if program_uses_namespace(decls, "Image") {
            self.extern_crates.insert("buff-image".to_string());
            self.extern_crates.insert("image".to_string());
        }
        // T37: register `buff-fake` when the program references the
        // prelude `Faker` module (`Faker.new()` / `Faker.with_locale()`
        // / `faker.name()` etc.). The generated code uses fully-
        // qualified `buff_fake::Faker::*` paths so no top-level `use`
        // import is emitted — but the recorded name signals to the
        // pipeline / build-driver that the generated Cargo project
        // must declare `buff-fake` in `[dependencies]`. Also records
        // `fake` transitively (the wrapper crate wraps the `fake`
        // crate). Mirrors the T9 Image registration pattern.
        if program_uses_namespace(decls, "Faker") {
            self.extern_crates.insert("buff-fake".to_string());
            self.extern_crates.insert("fake".to_string());
        }
        // T10: register `buff-audio` when the program references the
        // prelude `AudioBuffer` module (`AudioBuffer.from_path(...)`
        // etc.). Also records `hound` + `symphonia` transitively (the
        // wrapper crate wraps both for WAV / MP3 / FLAC / Vorbis
        // decode + WAV encode). Mirrors the T9 Image pattern.
        if program_uses_namespace(decls, "AudioBuffer") {
            self.extern_crates.insert("buff-audio".to_string());
            self.extern_crates.insert("hound".to_string());
            self.extern_crates.insert("symphonia".to_string());
        }
        // T17: register `buff-web` when the program references the
        // prelude `Web` module (`Web.new()` / `Web.bind(addr)` /
        // `web.get(...)` / etc.). Also records `axum` + `tokio` +
        // `serde_json` transitively (the wrapper crate wraps all
        // three for the HTTP server runtime + JSON codec). Mirrors
        // the T9 Image / T10 AudioBuffer pattern.
        if program_uses_namespace(decls, "Web") {
            self.extern_crates.insert("buff-web".to_string());
            self.extern_crates.insert("axum".to_string());
            self.extern_crates.insert("tokio".to_string());
            self.extern_crates.insert("serde_json".to_string());
        }
        // T20: register `buff-reactive` when the program references
        // any of the three reactive namespaces (`ReactiveSignal` /
        // `ReactiveComputed` / `ReactiveEffect`). Generated code uses
        // fully-qualified `buff_reactive::Signal::new` /
        // `buff_reactive::Computed::new` / `buff_reactive::Effect::new`
        // paths so no top-level `use` import is emitted — but the
        // recorded name signals to the pipeline / build-driver that
        // the generated Cargo project must declare `buff-reactive` in
        // `[dependencies]`. Mirrors the buff-image / buff-audio /
        // buff-dataframe / buff-audit pattern. The walker checks all
        // three namespaces because the user typically composes
        // Signal + Computed + Effect together; recording once for
        // any of them is sufficient (idempotent BTreeSet insert).
        if program_uses_namespace(decls, "ReactiveSignal")
            || program_uses_namespace(decls, "ReactiveComputed")
            || program_uses_namespace(decls, "ReactiveEffect")
        {
            self.extern_crates.insert("buff-reactive".to_string());
        }
        // T29: register `buff-validate` when the program references the
        // prelude `Validator` module (`Validator.new(...)` etc.). The
        // generated code uses fully-qualified `buff_validate::Validator::*`
        // paths so no top-level `use` import is emitted — but the
        // recorded name signals to the pipeline / build-driver that
        // the generated Cargo project must declare `buff-validate` in
        // `[dependencies]`. Also records `validator` (the upstream
        // validation crate whose trait methods we lower to) +
        // `serde_json` (for JSON Schema export) + `regex` (for
        // `with_regex` pattern compilation at rule-registration time).
        // Mirrors the Image / HttpClient registration pattern.
        if program_uses_namespace(decls, "Validator") {
            self.extern_crates.insert("buff-validate".to_string());
            self.extern_crates.insert("validator".to_string());
            self.extern_crates.insert("serde_json".to_string());
            self.extern_crates.insert("regex".to_string());
        }
        // T26: register `buff-audit` when the program references the
        // prelude `Audit` OR `Signature` modules (`Audit.scan(...)`
        // / `Signature.sign(...)` etc.). Also records
        // `ed25519-dalek` + `sha2` + `hex` + `rand` transitively
        // (the wrapper crate wraps all four: ed25519-dalek for
        // Ed25519 sign/verify, sha2 for the deferred manifest-hash
        // path, hex for sig/key encode/decode, rand for the OS
        // CSPRNG consumed by `Signature.keypair()`). Mirrors the
        // T9 Image / T10 Audio / T124k Hash+HMAC pattern. NO `ring`,
        // NO native-tls, NO cc-rs - ed25519-dalek is the canonical
        // pure-Rust Ed25519.
        if program_uses_namespace(decls, "Audit") || program_uses_namespace(decls, "Signature") {
            self.extern_crates.insert("buff-audit".to_string());
            self.extern_crates.insert("ed25519-dalek".to_string());
            self.extern_crates.insert("sha2".to_string());
            self.extern_crates.insert("hex".to_string());
            self.extern_crates.insert("rand".to_string());
        }
        // T27: register `buff-fuzz` when the program references either
        // the prelude `Fuzz` OR `Strategy` modules (`Fuzz.run(...)` /
        // `Strategy.int(...)` / etc.). Also records `proptest`
        // transitively (the wrapper crate wraps `proptest::test_runner::
        // TestRunner` + `proptest::strategy::Strategy` for the runtime
        // API). Mirrors the T9 Image / T10 Audio / T26 Audit pattern.
        // NO `cargo-fuzz`, NO `afl.rs`, NO cc-rs - proptest is pure-Rust
        // (matches the "no C library, no Docker" hard rule + the
        // "Windows host with no MSVC" constraint that pushed hand-rolled
        // lexer/parser).
        if program_uses_namespace(decls, "Fuzz") || program_uses_namespace(decls, "Strategy") {
            self.extern_crates.insert("buff-fuzz".to_string());
            self.extern_crates.insert("proptest".to_string());
        }
        // T18: register `buff-db` when the program references the
        // prelude `Database` module (`Database.connect(url)` etc.).
        // Also records `sqlx` + `tokio` transitively (the wrapper
        // crate wraps sqlx::any::AnyPool which needs a tokio runtime
        // via the `runtime-tokio-rustls` feature — NOT native-tls,
        // per workspace hard rule from AGENTS.md "Pure-Rust
        // preference"). Mirrors the T9 Image / T10 Audio / T26
        // Audit pattern. NO `diesel`, NO `libpq`, NO native-tls —
        // T18 task spec mandates sqlx-only.
        if program_uses_namespace(decls, "Database") {
            self.extern_crates.insert("buff-db".to_string());
            self.extern_crates.insert("sqlx".to_string());
            self.extern_crates.insert("tokio".to_string());
        }
        // T33: register `buff-http-client` + `reqwest` when the program
        // references the prelude `HttpClient` module (`HttpClient.new()` /
        // `client.get(url)` / etc.). Mirrors the T9 Image / T10 AudioBuffer
        // / T17 Web / T18 Database pattern.
        if program_uses_namespace(decls, "HttpClient") {
            self.extern_crates.insert("buff-http-client".to_string());
            self.extern_crates.insert("reqwest".to_string());
        }
        // T30: register `buff-config` + `figment` + `notify` when the
        // program references the prelude `Config` module (`Config.new()` /
        // `cfg.set_default(key, val)` / etc.). Namespace-only module
        // (mirror Log / Toml / Math / Random). The `figment` + `notify`
        // crates are the two external deps the wrapper crate wraps.
        if program_uses_namespace(decls, "Config") {
            self.extern_crates.insert("buff-config".to_string());
            self.extern_crates.insert("figment".to_string());
            self.extern_crates.insert("notify".to_string());
        }
        // T31 (frameworks): register `buff-cache` + `moka` when the
        // program references the prelude `Cache` module
        // (`Cache.new(max_capacity)` / `cache.get(k)` / etc.). Mirrors
        // the T9 Image / T10 AudioBuffer / T33 HttpClient pattern.
        // Distributed Redis backend deferred to v1.18+ — moka is the
        // only external dep the MVP wrapper crate wraps.
        if program_uses_namespace(decls, "Cache") {
            self.extern_crates.insert("buff-cache".to_string());
            self.extern_crates.insert("moka".to_string());
        }
        // T44: register `buff-i18n` when the program references the
        // I18n prelude type (`I18n.new(locale)` / `I18n.with_fallback`
        // / `i18n.add_resource` / `i18n.load` / `i18n.translate`).
        // Also records `fluent-bundle` + `unic-langid` transitively
        // (the upstream crates `buff-i18n` wraps). Distributed /
        // machine-translation backends explicitly forbidden by T44
        // spec — `fluent-bundle` + `unic-langid` are the only deps.
        if program_uses_namespace(decls, "I18n") {
            self.extern_crates.insert("buff-i18n".to_string());
            self.extern_crates.insert("fluent-bundle".to_string());
            self.extern_crates.insert("unic-langid".to_string());
        }
        // T34: register `buff-auth` when the program references any of
        // the four prelude auth modules (`JWT` / `OAuth2Client` /
        // `Password` / `Rbac`). Also records `jsonwebtoken` +
        // `argon2` + `oauth2` + `reqwest` transitively (the wrapper
        // crate wraps jsonwebtoken 10 with the pure-Rust `rust_crypto`
        // backend for HS256 JWT, argon2 0.5 for Argon2id password
        // hashing, oauth2 4 + reqwest rustls-tls for the OAuth2
        // auth-code flow). Mirrors the T9 Image / T10 Audio / T26
        // Audit / T18 Database pattern. NO `ring`, NO native-tls, NO
        // cc-rs — the T34 task spec explicitly forbids all three per
        // the "Windows host with no MSVC vcruntime.h" constraint.
        if program_uses_namespace(decls, "JWT")
            || program_uses_namespace(decls, "OAuth2Client")
            || program_uses_namespace(decls, "Password")
            || program_uses_namespace(decls, "Rbac")
        {
            self.extern_crates.insert("buff-auth".to_string());
            self.extern_crates.insert("jsonwebtoken".to_string());
            self.extern_crates.insert("argon2".to_string());
            self.extern_crates.insert("oauth2".to_string());
            self.extern_crates.insert("reqwest".to_string());
        }
        // T39: register `buff-archive` when the program references
        // the `Archive` namespace. Also records `zip` (deflate-only,
        // default-features disabled — pure-Rust), `tar` 0.4, `flate2`
        // 1.x (pure-Rust `miniz_oxide` backend), and `ruzstd` 0.8
        // (pure-Rust Zstd — NOT the canonical `zstd` crate which
        // wraps C libzstd via cc-rs, violating the "no C library"
        // hard rule). Mirrors the T9 Image / T17 Web / T18 Database
        // pattern. NO 7z, RAR, BZip2, encryption-at-rest — all
        // forbidden by the T39 task spec.
        if program_uses_namespace(decls, "Archive") {
            self.extern_crates.insert("buff-archive".to_string());
            self.extern_crates.insert("zip".to_string());
            self.extern_crates.insert("tar".to_string());
            self.extern_crates.insert("flate2".to_string());
            self.extern_crates.insert("ruzstd".to_string());
        }
        // T51: register `buff-msgpack` + `rmp-serde` + `serde_json`
        // when the program references the prelude `MsgPack` module
        // (`MsgPack.serialize(value)` / `MsgPack.deserialize(bytes)`).
        // The generated code uses fully-qualified `buff_msgpack::*`
        // paths so no top-level `use` import is emitted — but the
        // recorded name signals to the pipeline / build-driver that
        // the generated Cargo project must declare `buff-msgpack` in
        // `[dependencies]`. Also records `rmp-serde` + `serde_json`
        // transitively (the wrapper crate wraps both). Mirrors the
        // T9 Image / T39 Archive registration pattern.
        if program_uses_namespace(decls, "MsgPack") {
            self.extern_crates.insert("buff-msgpack".to_string());
            self.extern_crates.insert("rmp-serde".to_string());
            self.extern_crates.insert("serde_json".to_string());
        }
        // T52: register `buff-protobuf` + `prost` + `prost-types` +
        // `serde_json` when the program references the prelude
        // `Protobuf` OR `Message` modules (`Protobuf.serialize(value)`
        // / `Protobuf.deserialize(bytes)` / `Message.new(value)` /
        // `Message.from_bytes(bytes)` / `msg.byte_size()` / etc.).
        // The walker checks both namespaces because the user always
        // composes Protobuf + Message together (a Message value arises
        // only via Message.new which encodes via Protobuf.serialize
        // internally); recording once for either is sufficient
        // (idempotent BTreeSet insert). Also records `prost` +
        // `prost-types` + `serde_json` transitively (the wrapper crate
        // wraps all three). Mirrors the T51 MsgPack + T42 Email +
        // T50 Xml registration pattern.
        if program_uses_namespace(decls, "Protobuf") || program_uses_namespace(decls, "Message") {
            self.extern_crates.insert("buff-protobuf".to_string());
            self.extern_crates.insert("prost".to_string());
            self.extern_crates.insert("prost-types".to_string());
            self.extern_crates.insert("serde_json".to_string());
        }
        // T42: register `buff-email` + `lettre` + `handlebars` when
        // the program references the prelude `Email` OR `SmtpClient`
        // modules (`Email.new(from, to, subject)` /
        // `email.body(text)` / `email.html(tpl, ctx)` /
        // `email.attach(path)` / `SmtpClient.new(host, port, user,
        // pass)` / `client.send(email)`). The walker checks both
        // namespaces because the user always composes Email +
        // SmtpClient together; recording once for either is
        // sufficient (idempotent BTreeSet insert). Also records
        // `lettre` (the pure-Rust SMTP transport + message builder
        // via the `rustls` feature — NOT `native-tls` per AGENTS.md
        // hard rule) + `handlebars` (the templating engine shared
        // with T19 buff-template for `email.html(template, context)`
        // rendering). Mirrors the T9 Image / T18 Database / T34
        // buff-auth pattern.
        if program_uses_namespace(decls, "Email") || program_uses_namespace(decls, "SmtpClient") {
            self.extern_crates.insert("buff-email".to_string());
            self.extern_crates.insert("lettre".to_string());
            self.extern_crates.insert("handlebars".to_string());
        }
        // T43: register `buff-scrape` when the program references any
        // of the three prelude scrape namespaces (`Document.*` /
        // `Element.*` / `Crawler.*`). Also records `scraper`
        // transitively (the HTML parser + CSS selector engine wrapped
        // by `buff-scrape::Document` / `buff-scrape::Element`) and
        // `reqwest` transitively (the rustls-tls HTTP client wrapped
        // by `buff-scrape::Crawler`). The walker checks all three
        // namespaces because the user composes Document + Element +
        // Crawler together; recording once for any of them is
        // sufficient (idempotent BTreeSet insert). Mirrors the T9
        // Image / T18 Database / T34 buff-auth / T42 buff-email
        // pattern. Pure-Rust, CPU-only (no JS rendering, no
        // distributed crawling — both forbidden by T43 spec).
        if program_uses_namespace(decls, "Document")
            || program_uses_namespace(decls, "Element")
            || program_uses_namespace(decls, "Crawler")
        {
            self.extern_crates.insert("buff-scrape".to_string());
            self.extern_crates.insert("scraper".to_string());
            self.extern_crates.insert("reqwest".to_string());
        }
        // T46: register `buff-nlp` when the program references the
        // `Text` namespace (`Text.detect_language(text)` /
        // `Text.stem(word, algorithm)` / `Text.tokenize(text)` /
        // `Text.sentences(text)`). Also records `whatlang`
        // (pure-Rust trigram language identifier — 69+ languages),
        // `rust-stemmers` (pure-Rust Snowball stemmer for 18
        // languages — NOT a C binding), and `unicode-segmentation`
        // (already pinned for T124 String segmentation — pure-Rust
        // UAX #29 word + sentence segmentation). Mirrors the T9
        // Image / T18 Database / T34 buff-auth / T39 buff-archive
        // pattern. NO lemmatization, NO ML-based NER, NO embeddings
        // — all forbidden by the T46 task spec (v1.20+ work).
        if program_uses_namespace(decls, "Text") {
            self.extern_crates.insert("buff-nlp".to_string());
            self.extern_crates.insert("whatlang".to_string());
            self.extern_crates.insert("rust-stemmers".to_string());
            self.extern_crates
                .insert("unicode-segmentation".to_string());
        }
        // T45: register `buff-geo` when the program references any of
        // the three prelude geo namespaces (`Point.*` / `LineString.*`
        // / `Polygon.*`). Also records `geo` + `geo-types` transitively
        // (the wrapper crate wraps both for Euclidean distance / length
        // / area / Contains / Intersects algorithms). The walker checks
        // all three namespaces because the user typically composes
        // Point + LineString + Polygon together; recording once for any
        // of them is sufficient (idempotent BTreeSet insert). Mirrors
        // the T9 Image / T43 buff-scrape / T42 buff-email pattern.
        // Pure-Rust, CPU-only per Metis G7 lock (NO GPU dispatch).
        if program_uses_namespace(decls, "Point")
            || program_uses_namespace(decls, "LineString")
            || program_uses_namespace(decls, "Polygon")
        {
            self.extern_crates.insert("buff-geo".to_string());
            self.extern_crates.insert("geo".to_string());
            self.extern_crates.insert("geo-types".to_string());
        }
        // T54: register `buff-simd` when the program references the
        // `Simd` namespace (`Simd.splat(x)` / `Simd.from_slice(s)` /
        // `Simd.from_array(arr)` / `simd.add(other)` etc.). Also
        // records `wide` transitively (the pure-Rust portable SIMD
        // wrapper crate that `buff_simd::Simd` wraps — `wide::f32x4`).
        // Mirrors the T9 Image / T45 buff-geo / T43 buff-scrape pattern.
        // Pure-Rust, CPU-only per Metis G7 lock (NO GPU dispatch — GPU
        // SIMD is WGSL's job via `buff-lang-codegen-wgsl`); NO nightly
        // `std::simd`, NO runtime `is_x86_feature_detected!` detection
        // per T54 spec ("Must NOT" clause).
        if program_uses_namespace(decls, "Simd") {
            self.extern_crates.insert("buff-simd".to_string());
            self.extern_crates.insert("wide".to_string());
        }
        // T59: register `buff-actors` when the program references any
        // of the actor namespaces (`ActorSystem.*` / `ActorRef.*` /
        // `Supervisor.*` / `ChildSpec.*` / `RestartStrategy.*`).
        // Also records `crossbeam-channel` transitively (the
        // per-actor mailbox primitive). The MVP uses `std::thread`
        // (NOT `tokio`) for deterministic `JoinHandle::join` on
        // graceful shutdown; a future v1.18+ async variant would
        // also record `tokio`. Mirrors the T54 Simd walker pattern.
        if program_uses_namespace(decls, "ActorSystem")
            || program_uses_namespace(decls, "ActorRef")
            || program_uses_namespace(decls, "Supervisor")
            || program_uses_namespace(decls, "ChildSpec")
            || program_uses_namespace(decls, "RestartStrategy")
        {
            self.extern_crates.insert("buff-actors".to_string());
            self.extern_crates.insert("crossbeam-channel".to_string());
        }
        // T50: register `buff-xml` when the program references either
        // of the two prelude xml namespaces (`Xml.*` /
        // `XmlElement.*`). Also records `quick-xml` transitively (the
        // pure-Rust streaming XML parser wrapped by
        // `buff_xml::XmlDocument::from_str`). The walker checks both
        // namespaces because the user typically composes Xml +
        // XmlElement together (Xml.from_str returns XmlDocument,
        // whose .root() / .find() return XmlElement); recording once
        // for either is sufficient (idempotent BTreeSet insert).
        // Mirrors the T9 Image / T43 buff-scrape / T45 buff-geo
        // pattern. Pure-Rust, CPU-only.
        if program_uses_namespace(decls, "Xml") || program_uses_namespace(decls, "XmlElement") {
            self.extern_crates.insert("buff-xml".to_string());
            self.extern_crates.insert("quick-xml".to_string());
        }
        // T47: register `buff-chat` when the program references any of
        // the three prelude chat namespaces (`Bot.*` /
        // `ChatMessage.*` / `Platform.*`). Also records `serenity`
        // (Discord Gateway + HTTP API — pure-Rust via rustls_backend,
        // NOT native_tls_backend) + `teloxide` (Telegram Bot API —
        // pure-Rust via rustls, NOT native_tls) + `async-trait`
        // (serenity's EventHandler trait bridge) + `tokio` (the
        // multi-threaded runtime `Bot::start` builds internally)
        // transitively. The walker checks all three namespaces because
        // the user typically composes Bot + ChatMessage + Platform
        // together (Bot.new takes Platform; ChatMessage values arise
        // only inside handler closures); recording once for any of
        // them is sufficient (idempotent BTreeSet insert). Mirrors the
        // T9 Image / T43 buff-scrape / T45 buff-geo / T50 buff-xml
        // pattern. Pure-Rust, CPU-only; both serenity + teloxide use
        // rustls + ring (NO native-tls, NO cc-rs — matches the
        // "no C library, no Docker" hard rule from T126/T127).
        if program_uses_namespace(decls, "Bot")
            || program_uses_namespace(decls, "ChatMessage")
            || program_uses_namespace(decls, "Platform")
        {
            self.extern_crates.insert("buff-chat".to_string());
            self.extern_crates.insert("serenity".to_string());
            self.extern_crates.insert("teloxide".to_string());
            self.extern_crates.insert("async-trait".to_string());
            self.extern_crates.insert("tokio".to_string());
        }
        // T48: register `buff-web3` when the program references any of
        // the five prelude web3 namespaces (`Provider.*` / `Wallet.*` /
        // `ConnectedWallet.*` / `Contract.*` / `ContractMethod.*`).
        // Also records `ethers` (the upstream Ethereum RPC + signer
        // crate, with the `rustls` feature — NOT native-tls per
        // AGENTS.md hard rule), `tokio` (the multi-threaded runtime
        // shared via `buff_web3`'s OnceLock), `reqwest` (transitive
        // via ethers' Http provider — rustls-tls), `serde_json`
        // (transitive via ethers' ABI parser), and `hex` (transitive
        // via Wallet.sign_message hex-encoding + ContractMethod tx-
        // hash formatting). The walker checks all five namespaces
        // because the user always composes Provider + Wallet +
        // Contract together (a ContractMethod value arises only via
        // `contract.method(name)` which requires a Contract, which
        // requires either a Provider or ConnectedWallet); recording
        // once for any of them is sufficient (idempotent BTreeSet
        // insert). Mirrors the T9 Image / T18 Database / T34 buff-auth
        // / T42 buff-email / T47 buff-chat pattern. Pure-Rust, CPU-
        // only (network I/O never runs on the GPU path).
        if program_uses_namespace(decls, "Provider")
            || program_uses_namespace(decls, "Wallet")
            || program_uses_namespace(decls, "ConnectedWallet")
            || program_uses_namespace(decls, "Contract")
            || program_uses_namespace(decls, "ContractMethod")
        {
            self.extern_crates.insert("buff-web3".to_string());
            self.extern_crates.insert("ethers".to_string());
            self.extern_crates.insert("tokio".to_string());
            self.extern_crates.insert("reqwest".to_string());
            self.extern_crates.insert("serde_json".to_string());
            self.extern_crates.insert("hex".to_string());
        }
        // T49: register `buff-crypto-extras` when the program references
        // any of the five prelude crypto-extras namespaces (`AES.*` /
        // `RSA.*` / `ECDH.*` / `Argon2.*` / `RsaKeypair.*`). Also
        // records the upstream RustCrypto crates the wrapper consumes:
        // `aes-gcm` (AES-256-GCM AEAD), `rsa` (PKCS#1 v1.5 SHA-256
        // signatures), `p256` + `p384` (NIST ECDH key agreement),
        // `argon2` (raw Argon2id KDF — shared with T34 buff-auth's
        // PHC-string Password hashing), `sha2` (pulled transitively by
        // rsa + p256 + argon2; recorded explicitly for clarity),
        // `rand` (CSPRNG for nonce/key/salt generation), `signature`
        // (Verifier + RandomizedSigner traits used by the RSA path —
        // the rsa crate re-exports them but we record signature
        // explicitly for the extern_crates contract), and `hex` (for
        // hex-encoded test vectors + diagnostics). The walker checks
        // all five namespaces because the user typically composes
        // AES + RSA + ECDH + Argon2 + RsaKeypair together (a
        // RsaKeypair value arises only via `RSA.generate_keypair`);
        // recording once for any of them is sufficient (idempotent
        // BTreeSet insert). Mirrors the T9 Image / T43 buff-scrape /
        // T45 buff-geo / T50 buff-xml / T47 buff-chat / T48 buff-web3
        // pattern. Pure-Rust, CPU-only (NO ring, NO native-tls, NO
        // cc-rs — matches the AGENTS.md "no C library" hard rule).
        if program_uses_namespace(decls, "AES")
            || program_uses_namespace(decls, "RSA")
            || program_uses_namespace(decls, "ECDH")
            || program_uses_namespace(decls, "Argon2")
            || program_uses_namespace(decls, "RsaKeypair")
        {
            self.extern_crates.insert("buff-crypto-extras".to_string());
            self.extern_crates.insert("aes-gcm".to_string());
            self.extern_crates.insert("rsa".to_string());
            self.extern_crates.insert("p256".to_string());
            self.extern_crates.insert("p384".to_string());
            self.extern_crates.insert("argon2".to_string());
            self.extern_crates.insert("sha2".to_string());
            self.extern_crates.insert("rand".to_string());
            self.extern_crates.insert("signature".to_string());
            self.extern_crates.insert("hex".to_string());
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
        // T85: collect the registry of USER-defined enum variants
        // (variant_name → enum_name), EXCLUDING prelude enums
        // (`Option`/`Result`). Built BEFORE the main lowering loop so
        // [`Self::lower_expr`]'s `Expr::Ident` arm and
        // [`Self::lower_pattern`]'s `Pattern::Ident` / `Pattern::Variant`
        // arms can qualify bare variant references as `Enum::Variant`.
        // See [`collect_user_enum_variants`] for the collision rule.
        self.user_enum_variants = collect_user_enum_variants(decls);
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
                // T86: mark that we're lowering the operand of a
                // `return <expr>`. [`Self::lower_match_expr`] consults
                // this depth counter to strip the trailing `;` from
                // match arm body blocks (without the strip, every arm
                // body block has Rust type `()` and
                // `return match n { A => 1, _ => 0 }` fails to
                // typecheck against a non-`()` return type). The
                // counter (not a bool) is incremented so nested
                // returns inside match arms still work correctly.
                self.return_position_depth = self.return_position_depth.saturating_add(1);
                let lowered_inner = opt_expr
                    .as_ref()
                    .map(|expr| self.lower_expr(expr))
                    .transpose()?;
                self.return_position_depth = self.return_position_depth.saturating_sub(1);
                let return_expr = match lowered_inner {
                    Some(expr) => SynExpr::Return(syn::ExprReturn {
                        attrs: Vec::new(),
                        return_token: Default::default(),
                        expr: Some(Box::new(expr)),
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
                // T82: Map-index WRITE path. If the target is
                // `Expr::Index { base, indices: [key] }` and `base`
                // infers to `Map<K, V>`, lower `m[key] = value` to
                // `m.insert(key, value)` (Buff's "no panic on missing
                // keys" convention applies to WRITES too: insert
                // creates-or-replaces, never panics). Compound ops
                // (`+=`, `-=`, ...) on map entries are NOT supported
                // here — they'd require read-modify-write via
                // `entry().and_modify().or_insert()` and are deferred.
                // The check happens BEFORE the bare-Ident fast-path
                // so an `m[k] = v` is never confused with an Ident
                // assignment.
                if *op == buff_lang_ast::op::BinaryOp::Assign {
                    if let Expr::Index { base, indices, .. } = &target {
                        if indices.len() == 1 {
                            let base_ty = self
                                .type_inferencer
                                .infer_expr(base)
                                .unwrap_or(Type::Unknown);
                            if matches!(base_ty, Type::Map(..)) {
                                return self
                                    .lower_map_index_write(base, &indices[0], value);
                            }
                        }
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
            // T53: `comptime { body }` — lower the body as an inline
            // block. Surgical stub; T53 will replace this with the
            // comptime interpreter that evaluates the block at compile
            // time and substitutes the result.
            Stmt::ComptimeBlock { body, .. } => {
                let syn_block = self.lower_block(body)?;
                let expr_block = syn::ExprBlock {
                    attrs: Vec::new(),
                    label: None,
                    block: syn_block,
                };
                Ok(SynStmt::Expr(SynExpr::Block(expr_block), None))
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
                // T85: bare user-defined enum variant reference. If the
                // name resolves to a user-defined enum variant
                // (e.g. `Red` belongs to `enum Color`), emit the
                // fully-qualified Rust path `Color::Red`. Without this,
                // rustc rejects the bare `Red` as an unresolved
                // identifier. Prelude variants (`Some`/`None`/`Ok`/`Err`)
                // are EXCLUDED by [`collect_user_enum_variants`] and stay
                // unqualified (they're in Rust's prelude).
                //
                // This branch fires BEFORE the move-analyzer / atomic /
                // closure-bypass checks because an enum VARIANT is not a
                // variable — it has no ownership state, can't be atomic,
                // and is never captured by closures. Returning early
                // keeps the variant path pure (no spurious `.clone()`).
                if let Some(enum_name) = self.user_enum_variants.get(&name.name) {
                    return Ok(two_segment_path_expr(enum_name, &name.name));
                }
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
                            buff_lang_types::prelude_types::assoc_fn_lookup(&id.name, &method.name)
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
                    // T82: Map indexing READ path. If the base infers to
                    // `Map<K, V>`, lower `m[key]` to
                    // `m.get(&key).cloned().unwrap_or_default()` so a
                    // missing key returns the default for `V` (Buff's
                    // "no panic on missing keys" convention — the user
                    // never sees a Rust panic). Inference failure or
                    // non-Map base falls through to the Vector path
                    // (`m[key as usize]`).
                    let base_ty = self
                        .type_inferencer
                        .infer_expr(base)
                        .unwrap_or(Type::Unknown);
                    if matches!(base_ty, Type::Map(..)) {
                        return self.lower_map_index_read(base, &indices[0]);
                    }
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
            // T124g: input() / input(prompt) - read one line from stdin
            // (optionally printing a prompt first). The prompt is print!
            // (no newline) so the user's input appears on the same line.
            // Trailing newline from read_line is trimmed. Wraps
            // std::io::stdin + std::io::Write::flush (for the prompt).
            PreludeFn::Input => self.lower_input(args),

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
            // T124g: sleep(duration) - async-transparent sleep. Lowers to
            // `tokio::time::sleep(<duration>).await`. The `.await` is
            // unconditional (Buff has no `await` keyword - the codegen
            // inserts it). The enclosing fn MUST be async; the T31
            // propagation walker doesn't YET know about `sleep` (a
            // future task can teach it), so for now the user must
            // declare an async fn that calls sleep (or call sleep from
            // main, which is auto-stamped `#[tokio::main]` when
            // propagated). The codegen boundary is the established
            // "codegen-only linking" pattern (single-file rustc link of
            // tokio is deferred - same as chrono/regex/toml/rand).
            //
            // Duration arg: the canonical Buff form is
            // `sleep(Duration.seconds(N))` which would lower to
            // `tokio::time::sleep(chrono::TimeDelta::seconds(N)).await` -
            // BUT chrono::TimeDelta is NOT a std::time::Duration (which
            // tokio::time::sleep requires). To keep the surface
            // ergonomic AND the generated code self-contained, we
            // detect the `Duration.<unit>(N)` AST shape and lower it
            // directly to `std::time::Duration::from_<unit>(N)` (no
            // chrono dependency in the sleep path). Plain Int args are
            // treated as seconds (`from_secs`). Other arg shapes pass
            // through unchanged (user responsibility - useful for
            // `std::time::Duration::from_millis(100)` directly if the
            // user constructs it).
            PreludeFn::Sleep => self.lower_sleep(args),
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
            // T38: assertThat(value) → buff_assertions::assertThat(value)
            // The fluent assertion entry point. Lowers to the
            // buff_assertions crate's assertThat function, which returns
            // an AssertThat<T> wrapper with chainable methods.
            PreludeFn::AssertThat => {
                if args.len() != 1 {
                    return Err(self.unsupported(
                        "assertThat() expects exactly 1 argument (the value to assert on)",
                    ));
                }
                let value = self.lower_expr(&args[0])?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    buff_assertions::assertThat(#value)
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("assertThat() codegen parse: {e}")))
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
        // Lower exactly two args, erroring on arity mismatch. Returns
        // a 2-tuple so T34 (buff-auth) call sites that destructure
        // `(token, secret)` work without refactor. Mirrors `one_arg`.
        let two_args = |c: &mut Self| -> Result<(SynExpr, SynExpr), CodegenError> {
            if args.len() != 2 {
                return Err(c.unsupported(&format!(
                    "{}.{}() expects exactly 2 args, got {}",
                    ptype.name(),
                    pmethod.name(),
                    args.len()
                )));
            }
            let a0 = c.lower_expr(&args[0])?;
            let a1 = c.lower_expr(&args[1])?;
            Ok((a0, a1))
        };
        // Lower exactly N args, erroring on arity mismatch. Returns the
        // lowered args as a Vec so multi-arg prelude calls (Math.pow,
        // Math.min/max, Random.int, Strings.split/join/replace/...)
        // can destructure them positionally.
        let n_args = |c: &mut Self, n: usize| -> Result<Vec<SynExpr>, CodegenError> {
            if args.len() != n {
                return Err(c.unsupported(&format!(
                    "{}.{}() expects exactly {} arg(s), got {}",
                    ptype.name(),
                    pmethod.name(),
                    n,
                    args.len()
                )));
            }
            args.iter().map(|a| c.lower_expr(a)).collect()
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
            // T124d: Regex.compile(pattern) -> Regex. Mirrors the
            // DateTime.parse "unwrap_or(<default>)" pattern (T124b):
            // `regex::Regex::new` is fallible (`Result<Regex, Error>`),
            // but Buff's prelude-type ctor surface is infallible (no
            // Result return). An invalid pattern yields a never-matching
            // fallback regex `r"a^"` (provably valid syntax: an `a`
            // followed by start-of-string anchor — syntactically valid,
            // semantically never matches anything). The inner `.unwrap()`
            // on this known-valid literal is the established Rust idiom
            // for infallible fallback from a fallible ctor when no
            // const-fn constructor exists (regex has no `Regex::empty()`
            // or const constructor; chrono's `Utc::now()` is the
            // equivalent trusted call in the T124b precedent).
            (T::Regex, A::Compile) => {
                let arg = one_arg(self)?;
                let arg = coerce_str_arg_to_ref(arg, &args[0]);
                // regex::Regex::new(pattern).unwrap_or_else(|_| regex::Regex::new(r"a^").unwrap())
                let new_call = rust_call_expr("regex::Regex::new", vec![arg]);
                let fallback = rust_call_expr("regex::Regex::new", vec![str_lit_expr(r"a^")]);
                // Build `.unwrap_or_else(|_| <fallback>.unwrap())` via
                // quote! + parse2 (the closure arg is awkward to build
                // directly via syn::ExprMethodCall).
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    #new_call.unwrap_or_else(|_| #fallback.unwrap())
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Regex.compile codegen parse: {e}")))
            }
            // T124e: Toml.parse(s) -> Map<String, Unknown>. Mirrors the
            // Regex.compile "unwrap_or_else(<default>)" panic-free
            // pattern: `toml::from_str` is fallible
            // (`Result<T, toml::de::Error>`), but Buff's prelude-type
            // surface is infallible. A parse failure yields an empty
            // Map via `.unwrap_or_default()` (HashMap impls Default) —
            // NO panic, matching the T124b/T124d precedent.
            //
            // The turbofish `::<std::collections::HashMap<String,
            // toml::Value>>` makes the concrete return type explicit in
            // the generated Rust. Buff's inferred return type
            // (Map<String, Unknown>) is the looser surface contract;
            // the turbofish pins the runtime representation so the
            // generated Rust is fully typed without requiring a let-
            // binding's type annotation to drive inference.
            //
            // The turbofish path can NOT be built via `rust_call_expr`
            // (which splits on `::` and creates Idents —
            // `<std::collections::...>` is a turbofish arg, not a path
            // segment). We build the whole call via `quote!` instead.
            //
            // String literals lower to `&'static str` already (no
            // borrow needed); non-literal String-typed args get an `&`
            // via `coerce_str_arg_to_ref` so Rust's Deref coercion
            // turns `&String` into `&str` (the type `toml::from_str`
            // requires).
            (T::Toml, A::Parse) => {
                let arg = one_arg(self)?;
                let arg = coerce_str_arg_to_ref(arg, &args[0]);
                // toml::from_str::<HashMap<String, toml::Value>>(s)
                //     .unwrap_or_default()
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    toml::from_str::<std::collections::HashMap<String, toml::Value>>(#arg)
                        .unwrap_or_default()
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Toml.parse codegen parse: {e}")))
            }
            // T124e: Toml.stringify(v) -> String. Mirrors the
            // Toml.parse panic-free pattern: `toml::to_string` is
            // fallible (`Result<String, toml::ser::Error>`), but Buff
            // surfaces it as infallible. A serialization failure yields
            // the empty string via `.unwrap_or_default()` (String
            // impls Default) — NO panic.
            //
            // The arg is taken by `&v` because `toml::to_string`
            // requires `&impl Serialize`. Any value the user passes (a
            // Map<String, ?>, a struct, ...) is borrowed — the Rust
            // Serialize bound is checked at the rustc level (a value
            // that doesn't impl Serialize surfaces as a regular Rust
            // compile error, not a Buff codegen error).
            //
            // Built via `quote!` directly so the `& #arg` borrow is a
            // real syn::ExprRef (not a path-segment hack).
            (T::Toml, A::Stringify) => {
                let arg = one_arg(self)?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    toml::to_string(&#arg).unwrap_or_default()
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Toml.stringify codegen parse: {e}")))
            }
            // T124f: Math module - all 11 assoc fns wrap Rust's `f64`
            // methods. The arg is cast to `f64` first (so an Int arg
            // like `Math.sqrt(16)` works as well as a Float arg like
            // `Math.sqrt(2.0)`) - this matches the spec's acceptance
            // criterion `Math.sqrt(16) -> 4.0`. The cast is wrapped in
            // parens via `cast_to` so compound expressions bind
            // correctly: `Math.sqrt(a + b)` -> `((a + b) as f64).sqrt()`.
            //
            // Math uses only Rust `std` (NO extern crate needed) -
            // every `f64` method is on the primitive type directly.
            //
            // UNARY MATH METHODS (1 arg): sqrt / sin / cos / tan / abs
            // / floor / ceil / round. Each lowers to `(<arg> as f64).<method>()`.
            // We use a single shared code path: build the method call
            // via `quote!` so the cast + method-chain is one
            // well-formed `syn::Expr`.
            (T::Math, A::Sqrt) => lower_math_unary(one_arg(self)?, "sqrt"),
            (T::Math, A::Sin) => lower_math_unary(one_arg(self)?, "sin"),
            (T::Math, A::Cos) => lower_math_unary(one_arg(self)?, "cos"),
            (T::Math, A::Tan) => lower_math_unary(one_arg(self)?, "tan"),
            (T::Math, A::Abs) => lower_math_unary(one_arg(self)?, "abs"),
            (T::Math, A::Floor) => lower_math_unary(one_arg(self)?, "floor"),
            (T::Math, A::Ceil) => lower_math_unary(one_arg(self)?, "ceil"),
            (T::Math, A::Round) => lower_math_unary(one_arg(self)?, "round"),
            // BINARY MATH METHODS (2 args): pow / min / max.
            // - `Math.pow(base, exp)` -> `((base as f64).powf(exp as f64))`.
            //   Both args cast to f64 because `f64::powf` takes `f64`.
            // - `Math.min(a, b)` / `Math.max(a, b)` -> `(a as f64).min(b as f64)`.
            //   Both args cast to f64 for symmetry with pow (Rust's
            //   `f64::min` / `f64::max` take `f64`).
            (T::Math, A::Pow) => {
                let args = n_args(self, 2)?;
                let (base, exp) = (args[0].clone(), args[1].clone());
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    (#base as f64).powf(#exp as f64)
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Math.pow codegen parse: {e}")))
            }
            (T::Math, A::Min) => lower_math_binary(n_args(self, 2)?, "min"),
            (T::Math, A::Max) => lower_math_binary(n_args(self, 2)?, "max"),
            // T124f: Random module - 4 assoc fns wrapping the `rand`
            // crate (0.9 API). All use `rand::rng()` to obtain
            // a thread-local RNG (NOT cryptographically secure - the
            // plan defers CSPRNG to a future Hash/Crypto module).
            //
            // `Random.int(min, max)` -> `rand::rng().random_range(min..=max)`.
            // The inclusive range `min..=max` matches the spec's
            // acceptance criterion `Random.int(1, 10)` returns int in
            // [1, 10] (NOT [1, 11)). `random_range` is the rand 0.9 API
            // (rand 0.8 called it `gen_range`).
            (T::Random, A::Int) => {
                let args = n_args(self, 2)?;
                let (lo, hi) = (args[0].clone(), args[1].clone());
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    rand::rng().random_range(#lo..=#hi)
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Random.int codegen parse: {e}")))
            }
            // `Random.float()` -> `rand::rng().random::<f64>()`.
            // Returns f64 in `[0, 1)`. Zero args. Uses `random::<f64>()`
            // (rand 0.9 API; rand 0.8 called it `gen::<f64>()`).
            (T::Random, A::Float) => {
                no_args(self)?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    rand::rng().random::<f64>()
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Random.float codegen parse: {e}")))
            }
            // `Random.choice(vec)` -> `rand::seq::IndexedRandom::choose(
            //   &vec, &mut rand::rng()).cloned()`.
            //
            // Returns `Option<T>` (None on empty input - NEVER panics,
            // matching Buff's "no panicking generated code" rule). The
            // fully-qualified `IndexedRandom::choose` path avoids needing
            // a `use rand::seq::IndexedRandom;` import in the generated
            // crate. The `.cloned()` lifts `Option<&T>` to `Option<T>`
            // so the user gets an owned value (Buff hides references).
            //
            // Acceptance: `Random.choice([1, 2, 3])` returns `Option<Int>`
            // (Some(1) / Some(2) / Some(3) at random; never None on a
            // non-empty input).
            (T::Random, A::Choice) => {
                let arg = one_arg(self)?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    rand::seq::IndexedRandom::choose(&#arg, &mut rand::rng()).cloned()
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Random.choice codegen parse: {e}")))
            }
            // `Random.shuffle(vec)` -> `{ let mut __v = vec;
            //   rand::seq::IndexedRandom::shuffle(&mut __v, &mut
            //   rand::rng()); __v }`.
            //
            // Returns a NEW shuffled Vector (the input is consumed -
            // the codegen makes a `let mut` binding internally and
            // returns it; in Buff's move-by-default world this is the
            // natural ownership transfer). The fully-qualified
            // `IndexedRandom::shuffle` path avoids needing a `use` import.
            //
            // Built via `quote!` + parse2 because the block expression
            // (`{ let mut __v = ...; ...; __v }`) is awkward to build
            // via direct syn node construction. The result is a single
            // `syn::Expr::Block` that evaluates to the shuffled Vec.
            //
            // NOTE: the local binding name `__v` is deliberately
            // underscore-prefixed to avoid colliding with user vars in
            // the surrounding scope (Buff's identifier namespace
            // convention reserves `__`-prefixed names for codegen-
            // introduced temporaries - mirrors the `__recv` placeholder
            // pattern used in `splice_receiver_into_call`).
            (T::Random, A::Shuffle) => {
                let arg = one_arg(self)?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    {
                        let mut __v = #arg;
                        rand::seq::IndexedRandom::shuffle(&mut __v, &mut rand::rng());
                        __v
                    }
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Random.shuffle codegen parse: {e}")))
            }
            // T124f: Strings module - 8 assoc fns wrapping Rust's `str`
            // / `String` methods as functional module calls. Strings
            // uses only Rust `std` (NO extern crate needed).
            //
            // Each arg of String type is borrowed via
            // `coerce_str_arg_to_ref` so Rust's Deref coercion turns
            // `&String` into `&str` (the type `str` methods take).
            // String literals lower to `&'static str` already (no
            // borrow needed).
            //
            // `Strings.split(text, sep)` ->
            //   `text.split(sep).map(|s| s.to_string()).collect::<Vec<String>>()`.
            //
            // The `.map(|s| s.to_string())` lifts `&str` to `String`
            // (Buff hides references from users). The turbofish
            // `::<Vec<String>>` pins the concrete return type so the
            // generated Rust is fully typed without a let-binding
            // annotation.
            (T::Strings, A::Split) => {
                let lowered = n_args(self, 2)?;
                let raw_text = lowered[0].clone();
                let raw_sep = lowered[1].clone();
                let text = coerce_str_arg_to_ref(raw_text, &args[0]);
                let sep = coerce_str_arg_to_ref(raw_sep, &args[1]);
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    #text.split(#sep).map(|s| s.to_string()).collect::<Vec<String>>()
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Strings.split codegen parse: {e}")))
            }
            // `Strings.join(vec, sep)` -> `vec.join(&sep)`.
            // The sep is borrowed via `&` so both `'static str` and
            // `String` inputs satisfy `Vec::<String>::join`'s `&str`
            // bound. The vec itself is taken by value (`Vec::<String>::join`
            // takes `&self`, so the codegen auto-borrows via
            // `method_call_one_arg` which produces `vec.join(&sep)`).
            (T::Strings, A::Join) => {
                let lowered = n_args(self, 2)?;
                let vec_e = lowered[0].clone();
                let sep_e = lowered[1].clone();
                // Borrow the sep via `&` to satisfy `&str` bound on
                // `Vec::<String>::join(&self, sep: &str)`. The vec is
                // method-called directly (auto-borrowed by Rust's
                // method-call sugar).
                let sep_borrowed = syn::Expr::Reference(syn::ExprReference {
                    attrs: Vec::new(),
                    and_token: Default::default(),
                    expr: Box::new(sep_e),
                    mutability: None,
                });
                Ok(method_call_one_arg(vec_e, "join", sep_borrowed))
            }
            // `Strings.trim(text)` -> `text.trim().to_string()`.
            // `.trim()` returns `&str`; `.to_string()` lifts to `String`.
            (T::Strings, A::Trim) => {
                let arg = one_arg(self)?;
                let arg_ref = coerce_str_arg_to_ref(arg, &args[0]);
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    #arg_ref.trim().to_string()
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Strings.trim codegen parse: {e}")))
            }
            // `Strings.replace(text, from, to)` -> `text.replace(from, to)`.
            // Rust's `str::replace` takes `&str, &str` (or a Pattern)
            // and returns a NEW `String`. We borrow each arg via
            // `coerce_str_arg_to_ref` so `&String` derefs to `&str`.
            (T::Strings, A::Replace) => {
                let lowered = n_args(self, 3)?;
                let raw_text = lowered[0].clone();
                let raw_from = lowered[1].clone();
                let raw_to = lowered[2].clone();
                let text = coerce_str_arg_to_ref(raw_text, &args[0]);
                let from = coerce_str_arg_to_ref(raw_from, &args[1]);
                let to = coerce_str_arg_to_ref(raw_to, &args[2]);
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    #text.replace(#from, #to)
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Strings.replace codegen parse: {e}")))
            }
            // `Strings.contains(text, substr)` -> `text.contains(substr)`.
            // Returns Bool. The substr arg is borrowed via `&` so both
            // `'static str` and `String` inputs satisfy `str::contains`'s
            // Pattern bound (str: Pattern works directly).
            (T::Strings, A::Contains) => {
                let lowered = n_args(self, 2)?;
                let text_ref = coerce_str_arg_to_ref(lowered[0].clone(), &args[0]);
                let sep_ref = coerce_str_arg_to_ref(lowered[1].clone(), &args[1]);
                Ok(method_call_one_arg(text_ref, "contains", sep_ref))
            }
            // `Strings.starts_with(text, prefix)` -> `text.starts_with(prefix)`.
            // Same shape as `contains`.
            (T::Strings, A::StartsWith) => {
                let lowered = n_args(self, 2)?;
                let text_ref = coerce_str_arg_to_ref(lowered[0].clone(), &args[0]);
                let pre_ref = coerce_str_arg_to_ref(lowered[1].clone(), &args[1]);
                Ok(method_call_one_arg(text_ref, "starts_with", pre_ref))
            }
            // `Strings.to_uppercase(text)` -> `text.to_uppercase().to_string()`.
            // `.to_uppercase()` returns `String` already (Rust's
            // `str::to_uppercase` -> `String`), so the extra
            // `.to_string()` is a no-op for type but keeps the
            // generated code uniform with `trim` (both produce
            // `String`). Belt-and-suspenders: if a future Rust version
            // changes `to_uppercase` to return `Cow<str>` or similar,
            // the explicit `.to_string()` keeps Buff's surface stable.
            (T::Strings, A::ToUppercase) => {
                let arg = one_arg(self)?;
                let arg_ref = coerce_str_arg_to_ref(arg, &args[0]);
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    #arg_ref.to_uppercase().to_string()
                };
                syn::parse2(tokens).map_err(|e| {
                    self.unsupported(&format!("Strings.to_uppercase codegen parse: {e}"))
                })
            }
            // `Strings.to_lowercase(text)` -> `text.to_lowercase().to_string()`.
            // Same shape as `to_uppercase`.
            (T::Strings, A::ToLowercase) => {
                let arg = one_arg(self)?;
                let arg_ref = coerce_str_arg_to_ref(arg, &args[0]);
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    #arg_ref.to_lowercase().to_string()
                };
                syn::parse2(tokens).map_err(|e| {
                    self.unsupported(&format!("Strings.to_lowercase codegen parse: {e}"))
                })
            }
            // T124g: Args module - 2 assoc fns wrapping Rust's
            // `std::env::args` iterator. Args uses only Rust `std` (NO
            // extern crate needed).
            //
            // `Args.list()` -> `std::env::args().collect::<Vec<String>>()`.
            // Zero args. The turbofish `::<Vec<String>>` pins the
            // concrete return type so the generated Rust is fully typed
            // without a let-binding annotation (mirrors the Strings.split
            // turbofish pattern from T124f).
            (T::Args, A::List) => {
                no_args(self)?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    std::env::args().collect::<Vec<String>>()
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Args.list codegen parse: {e}")))
            }
            // `Args.get(i)` -> `std::env::args().nth(i).unwrap_or_default()`.
            // One Int arg. The `.unwrap_or_default()` yields the empty
            // String on out-of-bounds (NEVER panics - matching Buff's
            // "no panicking generated code" rule, mirroring the
            // Toml.parse / Regex.compile unwrap_or-panic-free pattern).
            (T::Args, A::Get) => {
                let arg = one_arg(self)?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    std::env::args().nth(#arg).unwrap_or_default()
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Args.get codegen parse: {e}")))
            }
            // T124g: Env module - 3 assoc fns wrapping Rust's
            // `std::env::var` / `set_var`. Env uses only Rust `std` (NO
            // extern crate needed).
            //
            // `Env.get("KEY")` -> `std::env::var(k).ok()`. One String
            // arg. Returns Option<String> (None when unset OR invalid
            // UTF-8 - both folded into None). Same `Get` variant as
            // Args.get but dispatched on (Env, Get); the codegen here
            // differs from (Args, Get) because the semantics differ
            // (var lookup vs positional arg). Mirrors the (DateTime,
            // Parse) / (Date, Parse) / (Toml, Parse) overload-by-type
            // pattern.
            //
            // String literals lower to `&'static str` already (no
            // borrow needed); non-literal String args get an `&` via
            // `coerce_str_arg_to_ref` so Rust's Deref coercion turns
            // `&String` into `&str` (the type `std::env::var` takes).
            (T::Env, A::Get) => {
                let arg = one_arg(self)?;
                let arg = coerce_str_arg_to_ref(arg, &args[0]);
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    std::env::var(#arg).ok()
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Env.get codegen parse: {e}")))
            }
            // `Env.set("KEY", "value")` -> `unsafe { std::env::set_var(k, v); }`.
            // Two String args. Returns Void. NOTE: `std::env::set_var`
            // is `unsafe` in Rust Edition 2024 (the edition the generated
            // code targets). The `unsafe { ... }` wrapper satisfies the
            // edition requirement.
            //
            // Both args are borrowed via `&` so Rust's Deref coercion
            // turns `&String` into `&str` (the type `set_var` takes).
            // The result is wrapped in a block `{ unsafe { ... }; }` so
            // the expression yields `()` (the call itself returns `()`
            // so the block is technically redundant, but uniform with
            // other Void-returning prelude calls avoids special-case
            // handling in expression-statement position).
            (T::Env, A::Set) => {
                let lowered = n_args(self, 2)?;
                let k = coerce_str_arg_to_ref(lowered[0].clone(), &args[0]);
                let v = coerce_str_arg_to_ref(lowered[1].clone(), &args[1]);
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    unsafe { std::env::set_var(#k, #v); }
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Env.set codegen parse: {e}")))
            }
            // `Env.has("KEY")` -> `std::env::var(k).is_ok()`. One String
            // arg. Returns Bool. Same borrow coercion as Env.get.
            (T::Env, A::Has) => {
                let arg = one_arg(self)?;
                let arg = coerce_str_arg_to_ref(arg, &args[0]);
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    std::env::var(#arg).is_ok()
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Env.has codegen parse: {e}")))
            }
            // T124h: Base64 module - 2 assoc fns wrapping the `base64`
            // Rust crate (STANDARD engine via the `Engine` trait).
            //
            // `Base64.encode(bytes)` -> String. Wraps
            // `base64::Engine::encode(&base64::engine::general_purpose::STANDARD,
            // bytes)` (UFCS form so the `Engine` trait need not be in
            // scope at the call site - generated code requires NO `use
            // base64::Engine as _;` import). The arg is the byte source
            // (Vector<Byte> / &[u8] / anything `AsRef<[u8]>`).
            (T::Base64, A::Encode) => {
                let arg = one_arg(self)?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    base64::Engine::encode(
                        &base64::engine::general_purpose::STANDARD,
                        #arg,
                    )
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Base64.encode codegen parse: {e}")))
            }
            // `Base64.decode(s)` -> Vector<Byte>. Wraps
            // `base64::Engine::decode(&general_purpose::STANDARD, s)
            // .unwrap_or_default()` (empty Vec on decode failure - NEVER
            // panics, matching Buff's "no panicking generated code" rule).
            // UFCS form so the `Engine` trait need not be in scope.
            //
            // The arg is borrowed via `&` so Rust's Deref coercion turns
            // `&String` into `&[u8]` (via `String`'s `AsRef<[u8]>` impl)
            // - the type `Engine::decode`'s generic bound accepts.
            (T::Base64, A::Decode) => {
                let arg = one_arg(self)?;
                let arg_ref = coerce_str_arg_to_ref(arg, &args[0]);
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    base64::Engine::decode(
                        &base64::engine::general_purpose::STANDARD,
                        #arg_ref,
                    ).unwrap_or_default()
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Base64.decode codegen parse: {e}")))
            }
            // T124h: Hex module - 2 assoc fns wrapping the `hex` Rust
            // crate (free functions, no trait import needed).
            //
            // `Hex.encode(bytes)` -> String. Wraps `hex::encode(bytes)`.
            // The arg is the byte source (`AsRef<[u8]>`).
            (T::Hex, A::Encode) => {
                let arg = one_arg(self)?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    hex::encode(#arg)
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Hex.encode codegen parse: {e}")))
            }
            // `Hex.decode(s)` -> Vector<Byte>. Wraps
            // `hex::decode(s).unwrap_or_default()` (empty Vec on decode
            // failure - NEVER panics). The arg is borrowed via `&` so
            // Rust's Deref coercion turns `&String` into `&str` (the
            // type `hex::decode` accepts via `AsRef<[u8]>` on `&str`).
            (T::Hex, A::Decode) => {
                let arg = one_arg(self)?;
                let arg_ref = coerce_str_arg_to_ref(arg, &args[0]);
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    hex::decode(#arg_ref).unwrap_or_default()
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Hex.decode codegen parse: {e}")))
            }
            // T124h: URLEncode module - 2 assoc fns wrapping the
            // `percent-encoding` Rust crate.
            //
            // `URLEncode.encode(s)` -> String. Wraps
            // `percent_encoding::utf8_percent_encode(s,
            // percent_encoding::NON_ALPHANUMERIC).to_string()`. The
            // `NON_ALPHANUMERIC` AsciiSet encodes everything that's not
            // an ASCII letter or digit (the canonical "encode special
            // characters" choice for safe URL embedding).
            //
            // The arg is borrowed via `&` so Rust's Deref coercion turns
            // `&String` into `&str` (the type `utf8_percent_encode`
            // takes).
            (T::URLEncode, A::Encode) => {
                let arg = one_arg(self)?;
                let arg_ref = coerce_str_arg_to_ref(arg, &args[0]);
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    percent_encoding::utf8_percent_encode(
                        #arg_ref,
                        percent_encoding::NON_ALPHANUMERIC,
                    ).to_string()
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("URLEncode.encode codegen parse: {e}")))
            }
            // `URLEncode.decode(s)` -> String. Wraps
            // `percent_encoding::percent_decode_str(s).decode_utf8_lossy()
            // .into_owned()`. Invalid UTF-8 sequences become U+FFFD
            // REPLACEMENT CHARACTER (lossy decode - NEVER panics).
            //
            // The arg is borrowed via `&` so Rust's Deref coercion turns
            // `&String` into `&str` (the type `percent_decode_str`
            // takes).
            (T::URLEncode, A::Decode) => {
                let arg = one_arg(self)?;
                let arg_ref = coerce_str_arg_to_ref(arg, &args[0]);
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    percent_encoding::percent_decode_str(#arg_ref)
                        .decode_utf8_lossy()
                        .into_owned()
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("URLEncode.decode codegen parse: {e}")))
            }
            // T124h: UUID module - 3 assoc fns wrapping the `uuid` Rust
            // crate. Surface return types are String / String / Bool
            // (NOT a `Uuid` value type) - Buff surfaces UUIDs as their
            // canonical hyphen-separated String form.
            //
            // `UUID.v4()` -> String. Wraps
            // `uuid::Uuid::new_v4().to_string()` (requires the `v4`
            // feature on the `uuid` crate, configured at the workspace
            // level).
            (T::UUID, A::V4) => {
                no_args(self)?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    uuid::Uuid::new_v4().to_string()
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("UUID.v4 codegen parse: {e}")))
            }
            // `UUID.v7()` -> String. Wraps
            // `uuid::Uuid::now_v7().to_string()` (requires the `v7`
            // feature on the `uuid` crate). Distinct from v4 in
            // generation algorithm (v7 is timestamp-prefixed for sort
            // stability) but identical surface type.
            (T::UUID, A::V7) => {
                no_args(self)?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    uuid::Uuid::now_v7().to_string()
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("UUID.v7 codegen parse: {e}")))
            }
            // `UUID.parse(s)` -> Bool. Wraps
            // `uuid::Uuid::parse_str(s).is_ok()` (validation only).
            // Reuses the shared `Parse` variant (5th overload for Parse,
            // after DateTime / Date / Toml / URL). The arg is borrowed
            // via `&` so Rust's Deref coercion turns `&String` into
            // `&str` (the type `Uuid::parse_str` takes).
            (T::UUID, A::Parse) => {
                let arg = one_arg(self)?;
                let arg_ref = coerce_str_arg_to_ref(arg, &args[0]);
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    uuid::Uuid::parse_str(#arg_ref).is_ok()
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("UUID.parse codegen parse: {e}")))
            }
            // T124h: URL module - 1 assoc fn (parse) wrapping the `url`
            // Rust crate. URL is a runtime-value type (NOT namespace-
            // only like Base64/Hex/URLEncode/UUID) - `URL.parse(s)`
            // returns a `URL` value carrying the four instance
            // accessors `.scheme` / `.host` / `.path` / `.query(key)`.
            //
            // `URL.parse(s)` -> URL. Wraps
            // `url::Url::parse(s).unwrap_or_else(|_| url::Url::parse("about:blank").unwrap())`.
            // The `about:blank` fallback is always parseable (it's a
            // reserved URL scheme per RFC 3986), so the inner `.unwrap()`
            // is infallible at runtime (matches Regex.compile's `r"a^"`
            // fallback stance from T124d). NEVER panics on malformed
            // input - falls back to a benign placeholder URL.
            //
            // The arg is borrowed via `&` so Rust's Deref coercion turns
            // `&String` into `&str` (the type `Url::parse` takes).
            (T::URL, A::Parse) => {
                let arg = one_arg(self)?;
                let arg_ref = coerce_str_arg_to_ref(arg, &args[0]);
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    url::Url::parse(#arg_ref)
                        .unwrap_or_else(|_| url::Url::parse("about:blank").unwrap())
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("URL.parse codegen parse: {e}")))
            }
            // T124i: Yaml.parse(s) -> Map<String, Unknown>. Mirrors the
            // Toml.parse codegen arm (T124e) line-for-line, swapping
            // `toml::` paths for `serde_yml::` paths. The maintained
            // fork `serde_yml` is API-compatible with the deprecated
            // `serde_yaml` (`from_str::<T>(s) -> Result<T, Error>` and
            // `to_string(&v) -> Result<String, Error>`).
            //
            // The turbofish pins the concrete return type
            // `HashMap<String, serde_yml::Value>` so the generated Rust
            // is fully typed without requiring a let-binding annotation.
            // Buff's inferred surface return type is the looser
            // `Map<String, Unknown>` (the Unknown value type reflects
            // YAML's heterogeneous value space, mirroring the
            // Toml.parse Unknown-value stance from T124e).
            //
            // The turbofish path CANNOT be built via `rust_call_expr`
            // (which splits on `::` and creates Idents - the
            // `<std::collections::...>` turbofish arg is not a path
            // segment). Built via `quote!` exactly like Toml.parse.
            //
            // String literals lower to `&'static str` already; non-
            // literal String args get an `&` via `coerce_str_arg_to_ref`
            // so Rust's Deref coercion turns `&String` into `&str`
            // (the type `serde_yml::from_str` requires).
            //
            // `.unwrap_or_default()` (HashMap impls Default) is the
            // panic-free fallback: a parse failure yields an empty
            // Map, NEVER a panic (mirrors Toml.parse / Regex.compile).
            (T::Yaml, A::Parse) => {
                let arg = one_arg(self)?;
                let arg = coerce_str_arg_to_ref(arg, &args[0]);
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    serde_yml::from_str::<std::collections::HashMap<String, serde_yml::Value>>(#arg)
                        .unwrap_or_default()
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Yaml.parse codegen parse: {e}")))
            }
            // T124i: Yaml.stringify(v) -> String. Mirrors the
            // Toml.stringify codegen arm (T124e) line-for-line,
            // swapping `toml::to_string` for `serde_yml::to_string`.
            // Both APIs are structurally identical: take `&impl
            // Serialize`, return `Result<String, _>`. The arg is
            // borrowed via `&v` so Rust's serde-Serialize bound is
            // satisfied for any Map<String, ?> / Serialize-implementing
            // value.
            //
            // `.unwrap_or_default()` (String impls Default) is the
            // panic-free fallback: a serialization failure yields the
            // empty String, NEVER a panic.
            //
            // Built via `quote!` directly so the `& #arg` borrow is a
            // real syn::ExprRef (not a path-segment hack).
            (T::Yaml, A::Stringify) => {
                let arg = one_arg(self)?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    serde_yml::to_string(&#arg).unwrap_or_default()
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Yaml.stringify codegen parse: {e}")))
            }
            // T23: Json.parse(s) -> Map<String, Unknown>. Mirrors the
            // Yaml.parse / Toml.parse codegen arms exactly, swapping
            // `serde_yml::from_str` / `toml::from_str` for
            // `serde_json::from_str`. The turbofish pins the concrete
            // `HashMap<String, serde_json::Value>` so the generated
            // Rust is fully typed. `.unwrap_or_default()` is the
            // panic-free fallback (empty Map on parse failure).
            (T::Json, A::Parse) => {
                let arg = one_arg(self)?;
                let arg = coerce_str_arg_to_ref(arg, &args[0]);
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    serde_json::from_str::<std::collections::HashMap<String, serde_json::Value>>(#arg)
                        .unwrap_or_default()
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Json.parse codegen parse: {e}")))
            }
            // T23: Json.stringify(v) -> String. Mirrors the
            // Yaml.stringify / Toml.stringify codegen arms exactly,
            // swapping `serde_yml::to_string` / `toml::to_string` for
            // `serde_json::to_string`. The arg is borrowed via `&v`
            // so Rust's serde-Serialize bound is satisfied.
            // `.unwrap_or_default()` is the panic-free fallback
            // (empty String on serialization failure).
            (T::Json, A::Stringify) => {
                let arg = one_arg(self)?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    serde_json::to_string(&#arg).unwrap_or_default()
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Json.stringify codegen parse: {e}")))
            }
            // T124i: Csv.parse(s) -> Vector<Vector<String>>. Differs
            // from Yaml/Toml in surface type (uniform rows of Strings
            // vs heterogeneous Map) so the codegen is bespoke rather
            // than a 1:1 mirror. Wraps the `csv` crate's
            // `ReaderBuilder` + `records()` iterator.
            //
            // The generated block expression:
            //   {
            //     let mut __rdr = csv::ReaderBuilder::new()
            //         .has_headers(false)
            //         .from_reader(s.as_bytes());
            //     __rdr.records()
            //         .filter_map(|r| r.ok())
            //         .map(|r| r.iter().map(|f| f.to_string()).collect::<Vec<String>>())
            //         .collect::<Vec<Vec<String>>>()
            //   }
            //
            // Key design choices (mirror the Yaml/Toml panic-free
            // stance from T124e):
            // - `.has_headers(false)`: per spec, Csv.parse surfaces
            //   EVERY row uniformly (including the header row). CSV
            //   has no inherent type information - the header is just
            //   the first row of Strings. Disabling header handling
            //   means the reader doesn't consume the first row.
            // - `.filter_map(|r| r.ok())`: malformed rows are SKIPPED
            //   (not surfaced as panics or errors). Matches the
            //   "no panicking generated code" Buff rule. A CSV with
            //   a malformed row yields the well-formed rows before
            //   the bad one.
            // - `.map(|r| r.iter().map(|f| f.to_string()).collect::<
            //   Vec<String>>())`: each `csv::StringRecord` becomes a
            //   `Vec<String>` (Buff surfaces every cell as text -
            //   there is no CSV typing).
            // - The final `.collect::<Vec<Vec<String>>>()` pins the
            //   turbofish to Buff's `Vector<Vector<String>>` surface
            //   type so the generated Rust is fully typed.
            // - The whole block is wrapped in `{ ... }` so it
            //   evaluates to the collected Vec (a single syn::Expr::Block).
            // - The `__rdr` local binding name is `__`-prefixed to
            //   avoid colliding with user vars (mirrors the
            //   `__recv` / `__v` codegen-temporary convention from
            //   T124f Random.shuffle / splice_receiver_into_call).
            //
            // The arg is borrowed via `&` for `as_bytes()` so
            // non-literal String args get a `&String.as_bytes()` (via
            // Deref coercion) rather than `String.as_bytes()` (which
            // would be a no-op borrow on owned). String literals lower
            // to `&'static str` already.
            (T::Csv, A::Parse) => {
                let arg = one_arg(self)?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    {
                        let mut __rdr = csv::ReaderBuilder::new()
                            .has_headers(false)
                            .from_reader(#arg.as_bytes());
                        __rdr.records()
                            .filter_map(|r| r.ok())
                            .map(|r| r.iter().map(|f| f.to_string()).collect::<Vec<String>>())
                            .collect::<Vec<Vec<String>>>()
                    }
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Csv.parse codegen parse: {e}")))
            }
            // T124i: Csv.stringify(rows) -> String. Wraps the `csv`
            // crate's `Writer` over an in-memory `Vec<u8>` buffer.
            // The generated block expression:
            //   {
            //     let mut __wtr = csv::Writer::from_writer(Vec::<u8>::new());
            //     for __row in &rows {
            //         __wtr.write_record(__row.clone()).ok();
            //     }
            //     String::from_utf8(__wtr.into_inner().unwrap_or_default())
            //         .unwrap_or_default()
            //   }
            //
            // Key design choices (mirror the Yaml/Toml panic-free
            // stance):
            // - `csv::Writer::from_writer(Vec::<u8>::new())`: write
            //   to an in-memory buffer (no file I/O). The turbofish
            //   `Vec::<u8>::new()` pins the writer's type parameter
            //   so Rust's inference doesn't need a let-binding
            //   annotation.
            // - `for __row in &rows`: iterate by reference so the
            //   input `Vec<Vec<String>>` is NOT consumed (Buff's
            //   move-by-default model would otherwise move the arg;
            //   borrowing lets the caller keep using the rows Vec).
            // - `__wtr.write_record(__row.clone()).ok();`: write
            //   each row. The `.clone()` lifts `&Vec<String>` to
            //   `Vec<String>` (write_record takes an owned iterator
            //   item via AsRef<[u8]> - the clone is the cheapest
            //   way to satisfy the bound without bespoke iterator
            //   plumbing). `.ok()` discards the Result - a single
            //   row write failure is NOT surfaced as a panic
            //   (matches the "no panicking generated code" rule);
            //   the row is simply omitted from the output.
            // - `__wtr.into_inner().unwrap_or_default()`: extract
            //   the underlying `Vec<u8>` writer. `into_inner` is
            //   fallible (`Result<W, csv::Error>`) only if a previous
            //   write was buffered and panicked; in practice it
            //   succeeds. `.unwrap_or_default()` yields an empty
            //   Vec<u8> on the (extremely unlikely) failure path -
            //   NEVER a panic.
            // - `String::from_utf8(...).unwrap_or_default()`: lift
            //   the byte buffer to String. Invalid UTF-8 yields the
            //   empty String (lossy - NEVER panics, mirrors the
            //   URLEncode.decode lossy stance from T124h).
            // - `__wtr` / `__row` are `__`-prefixed to avoid colliding
            //   with user vars (mirrors the `__recv` / `__v` / `__rdr`
            //   codegen-temporary convention).
            (T::Csv, A::Stringify) => {
                let arg = one_arg(self)?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    {
                        let mut __wtr = csv::Writer::from_writer(Vec::<u8>::new());
                        for __row in &#arg {
                            __wtr.write_record(__row.clone()).ok();
                        }
                        String::from_utf8(__wtr.into_inner().unwrap_or_default())
                            .unwrap_or_default()
                    }
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Csv.stringify codegen parse: {e}")))
            }
            // T24: File I/O assoc fns. File is namespace-only (mirrors
            // Log / Toml / Math). All four fns lower to std::fs::* with
            // `.unwrap_or_default()` so the Buff surface is always
            // infallible — NEVER panics, matching Buff's "no panicking
            // generated code" rule. NO extern crate needed (std-only,
            // mirroring Math / Strings / Args / Env).
            //
            // `File.read(path)` -> String. Wraps
            // `std::fs::read_to_string(p).unwrap_or_default()`.
            (T::File, A::Read) => {
                let arg = one_arg(self)?;
                let arg = coerce_str_arg_to_ref(arg, &args[0]);
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    std::fs::read_to_string(#arg).unwrap_or_default()
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("File.read codegen parse: {e}")))
            }
            // `File.write(path, content)` -> Void. Wraps
            // `std::fs::write(p, c).unwrap_or_default()`.
            (T::File, A::Write) => {
                let path_arg = self.lower_expr(&args[0])?;
                let path_arg = coerce_str_arg_to_ref(path_arg, &args[0]);
                let content_arg = self.lower_expr(&args[1])?;
                let content_arg = coerce_str_arg_to_ref(content_arg, &args[1]);
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    std::fs::write(#path_arg, #content_arg).unwrap_or_default()
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("File.write codegen parse: {e}")))
            }
            // `File.exists(path)` -> Bool. Wraps
            // `std::path::Path::new(p).exists()`.
            (T::File, A::Exists) => {
                let arg = one_arg(self)?;
                let arg = coerce_str_arg_to_ref(arg, &args[0]);
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    std::path::Path::new(#arg).exists()
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("File.exists codegen parse: {e}")))
            }
            // `File.append(path, content)` -> Void. Wraps
            // `std::fs::OpenOptions::new().append(true).open(p)
            // .and_then(|mut f| std::io::Write::write_all(&mut f, c.as_bytes()))
            // .unwrap_or_default()`.
            (T::File, A::Append) => {
                let path_arg = self.lower_expr(&args[0])?;
                let path_arg = coerce_str_arg_to_ref(path_arg, &args[0]);
                let content_arg = self.lower_expr(&args[1])?;
                let content_arg = coerce_str_arg_to_ref(content_arg, &args[1]);
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    std::fs::OpenOptions::new()
                        .append(true)
                        .open(#path_arg)
                        .and_then(|mut f| std::io::Write::write_all(&mut f, #content_arg.as_bytes()))
                        .unwrap_or_default()
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("File.append codegen parse: {e}")))
            }
            // T124j: Path module - 1 assoc fn (join) wrapping
            // `std::path::PathBuf` (std-only - NO extern crate
            // needed). Path is a runtime-value type (NOT namespace-
            // only like Dir/Tempfile) - `Path.join(a, b, ...)`
            // returns a `Path` value carrying the four instance
            // methods `.parent()` / `.extension()` / `.basename()` /
            // `.exists()`.
            //
            // `Path.join(a, b, ...)` -> Path. Wraps a chained
            // `std::path::PathBuf::from(a).join(b).join(c)...` for
            // any number of args >= 1. A single-arg `Path.join(a)`
            // returns `PathBuf::from(a)` (the no-op join). The
            // `PathBuf::from` constructor accepts any `AsRef<Path>`
            // - String / &str / PathBuf values all satisfy the bound
            // via Rust's std blanket impls.
            //
            // The arg sequence is lowered once and chained into a
            // single expression tree via folding (the first arg
            // becomes the PathBuf root, each subsequent arg becomes
            // a `.join(...)` call wrapping the accumulator). This
            // shape is required because quote! can't splice a Vec
            // into a chain of method calls without an explicit
            // fold (the `#(#args).*` repetition would emit them as
            // a flat sequence, not a nested call chain).
            //
            // Same shared `Join` variant as Strings.join (T124f).
            (T::Path, A::Join) => {
                if args.is_empty() {
                    return Err(self
                        .unsupported("Path.join() expects at least 1 arg (the path head), got 0"));
                }
                // Lower each arg once (avoids re-evaluating side
                // effects). The first arg becomes the PathBuf root;
                // each subsequent arg becomes a `.join(...)` call.
                let lowered: Vec<SynExpr> = args
                    .iter()
                    .map(|a| self.lower_expr(a))
                    .collect::<Result<Vec<_>, _>>()?;
                let mut iter = lowered.into_iter();
                let head = iter.next().expect("non-empty (checked above)");
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    std::path::PathBuf::from(#head)
                };
                let mut acc = syn::parse2::<SynExpr>(tokens)
                    .map_err(|e| self.unsupported(&format!("Path.join head codegen parse: {e}")))?;
                for next in iter {
                    acc = method_call_one_arg(acc, "join", next);
                }
                Ok(acc)
            }
            // T124j: Dir module - 4 assoc fns (list/create/remove/walk)
            // wrapping `std::fs::*` (std-only - NO extern crate needed
            // for list/create/remove) and the `walkdir` Rust crate (for
            // walk - the walkdir crate is recorded in `extern_crates`).
            // Dir is namespace-only (mirrors Log/Toml/Yaml/Csv) - every
            // call returns a value, NEVER a Dir value type.
            //
            // `Dir.list(path)` -> Vector<String>. Wraps
            // `std::fs::read_dir(p).filter_map(|e| e.ok()).map(|e|
            // e.file_name().to_string_lossy().into_owned())
            // .collect::<Vec<String>>()`. Skips inaccessible entries
            // via `.filter_map(|e| e.ok())` - NEVER panics (mirrors
            // the Csv.parse panic-free stance from T124i). Returns
            // entry NAMES (NOT paths) - the surface mirrors shell
            // `ls` / Python `os.listdir`.
            //
            // Same shared `List` variant as Args.list (T124g).
            (T::Dir, A::List) => {
                let arg = one_arg(self)?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    std::fs::read_dir(#arg)
                        .map(|entries| {
                            entries
                                .filter_map(|e| e.ok())
                                .map(|e| e.file_name().to_string_lossy().into_owned())
                                .collect::<Vec<String>>()
                        })
                        .unwrap_or_default()
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Dir.list codegen parse: {e}")))
            }
            // `Dir.create(path)` -> Void. Wraps
            // `std::fs::create_dir_all(p).ok()` (creates the
            // directory + any missing parents - mirrors `mkdir -p`;
            // discards errors via `.ok()` - NEVER panics). Same
            // shared `Create` variant as Tempfile.create.
            (T::Dir, A::Create) => {
                let arg = one_arg(self)?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    std::fs::create_dir_all(#arg).ok()
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Dir.create codegen parse: {e}")))
            }
            // `Dir.remove(path)` -> Void. Wraps
            // `std::fs::remove_dir_all(p).ok()` (removes the
            // directory tree recursively; discards errors via
            // `.ok()` - NEVER panics, mirroring the Dir.create
            // stance).
            (T::Dir, A::Remove) => {
                let arg = one_arg(self)?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    std::fs::remove_dir_all(#arg).ok()
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Dir.remove codegen parse: {e}")))
            }
            // `Dir.walk(path)` -> Vector<Path>. Wraps
            // `walkdir::WalkDir::new(p).into_iter().filter_map(|e|
            // e.ok()).map(|e| e.path().to_path_buf())
            // .collect::<Vec<std::path::PathBuf>>()`. Skips
            // inaccessible entries via `.filter_map(|e| e.ok())` -
            // NEVER panics (mirrors the Csv.parse panic-free stance
            // from T124i). The walkdir crate is recorded in
            // `extern_crates` when a Buff program uses Dir.walk.
            (T::Dir, A::Walk) => {
                let arg = one_arg(self)?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    walkdir::WalkDir::new(#arg)
                        .into_iter()
                        .filter_map(|e| e.ok())
                        .map(|e| e.path().to_path_buf())
                        .collect::<Vec<std::path::PathBuf>>()
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Dir.walk codegen parse: {e}")))
            }
            // T124j: Tempfile module - 2 assoc fns (create/dir)
            // wrapping the `tempfile` Rust crate + `std::env::temp_dir`.
            // Tempfile is namespace-only (mirrors Log/Toml/Yaml/Csv/
            // Dir). The `tempfile` crate is recorded in `extern_crates`
            // when a Buff program uses Tempfile.create/dir.
            //
            // `Tempfile.create()` -> Path. Wraps
            // `tempfile::NamedTempFile::new().map(|f|
            // f.into_temp_path().keep().unwrap_or_default())
            // .unwrap_or_default()`. The `into_temp_path().keep()`
            // chain persists the temp file's path beyond the
            // NamedTempFile's drop (the file becomes a regular file
            // the user can write/read/delete like any other). Both
            // inner `.unwrap_or_default()` calls handle the
            // potential PathPersistError / io::Error paths -
            // panic-free (empty PathBuf on failure - NEVER panics,
            // mirrors the Regex.compile / URL.parse infallible-ctor
            // stance from T124d/T124h).
            //
            // Same shared `Create` variant as Dir.create.
            (T::Tempfile, A::Create) => {
                if !args.is_empty() {
                    return Err(self.unsupported(&format!(
                        "Tempfile.create() takes no arguments, got {}",
                        args.len()
                    )));
                }
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    tempfile::NamedTempFile::new()
                        .map(|f| f.into_temp_path().keep().unwrap_or_default())
                        .unwrap_or_default()
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Tempfile.create codegen parse: {e}")))
            }
            // `Tempfile.dir()` -> Path. Wraps `std::env::temp_dir()`
            // (the `tempfile::env::temp_dir()` is a re-export of the
            // std fn; we splice the std path directly so NO extern
            // crate is needed for this call alone - but the narrow
            // walker records `tempfile` for symmetry with
            // Tempfile.create).
            (T::Tempfile, A::Dir) => {
                if !args.is_empty() {
                    return Err(self.unsupported(&format!(
                        "Tempfile.dir() takes no arguments, got {}",
                        args.len()
                    )));
                }
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    std::env::temp_dir()
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Tempfile.dir codegen parse: {e}")))
            }
            // T124k: Hash module - 3 assoc fns wrapping the `sha2`
            // (SHA-256 / SHA-512) + `md5` RustCrypto crates. Hash is
            // namespace-only (mirrors Log/Toml/Base64/Hex/Yaml/Csv/
            // Dir/Tempfile). The `sha2` / `md5` crates are recorded
            // in `extern_crates` when a Buff program uses Hash.*
            // (the narrow walkers flag the specific method names -
            // sha256/sha512 -> sha2, md5 -> md5); the `hex` crate is
            // recorded alongside (shared with T124h Hex module's
            // walker).
            //
            // `Hash.sha256(data)` -> String. Wraps
            // `{ use sha2::Digest; hex::encode(sha2::Sha256::digest
            // (<d>.as_bytes())) }`. The block-scoped `use` brings
            // the `Digest` trait's `digest` method into scope WITHOUT
            // polluting the caller's namespace (`digest` is a trait
            // method, NOT an inherent method on `Sha256` - so the
            // `use` is required for the path-syntax call
            // `sha2::Sha256::digest(...)` to resolve).
            //
            // The arg accepts String OR Vector<Byte> (anything
            // `AsRef<[u8]>` at the codegen layer); `.as_bytes()`
            // gives `&[u8]` for both String (str::as_bytes) and
            // Vec<u8> (slice::as_bytes via [u8] identity). The
            // returned `GenericArray<u8, U32>` is accepted by
            // `hex::encode` via its `AsRef<[u8]>` bound.
            //
            // Sanity check: `Hash.sha256("hello")` =
            // `2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824`.
            (T::Hash, A::Sha256) => {
                let arg = one_arg(self)?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    {
                        use sha2::Digest;
                        hex::encode(sha2::Sha256::digest(#arg.as_bytes()))
                    }
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Hash.sha256 codegen parse: {e}")))
            }
            // `Hash.sha512(data)` -> String. Same shape as sha256
            // but `Sha512`. Returns the 128-char lowercase hex
            // String. Block-scoped `use sha2::Digest;` for the trait
            // method (same rationale as sha256).
            (T::Hash, A::Sha512) => {
                let arg = one_arg(self)?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    {
                        use sha2::Digest;
                        hex::encode(sha2::Sha512::digest(#arg.as_bytes()))
                    }
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Hash.sha512 codegen parse: {e}")))
            }
            // `Hash.md5(data)` -> String. Wraps
            // `hex::encode(md5::compute(<d>.as_bytes()).0)` (the
            // `.0` accesses the inner `[u8; 16]` of the
            // `md5::Digest` tuple struct; `hex::encode` accepts it
            // via `AsRef<[u8]>` on `[u8; N]` arrays). NO `use`
            // needed - `md5::compute` is a free function (not a
            // trait method) and `.0` is a public field access (not
            // a trait method either). Returns the 32-char lowercase
            // hex String. **MD5 is CRYPTOGRAPHICALLY BROKEN** -
            // exposed for checksum compatibility only; NEVER use
            // for security.
            (T::Hash, A::Md5) => {
                let arg = one_arg(self)?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    hex::encode(md5::compute(#arg.as_bytes()).0)
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Hash.md5 codegen parse: {e}")))
            }
            // T124k: HMAC module - 1 assoc fn wrapping the `hmac` +
            // `sha2` RustCrypto crates. HMAC is namespace-only
            // (mirrors Hash / Log / Toml / Base64 / Hex / ...). The
            // `hmac` + `sha2` crates are recorded in `extern_crates`
            // when a Buff program uses `HMAC.sha256` (the
            // `hmac::Hmac<sha2::Sha256>` path needs BOTH); the
            // `hex` crate is recorded alongside.
            //
            // `HMAC.sha256(key, data)` -> String. Wraps
            // `{ use hmac::Mac; hmac::Hmac::<sha2::Sha256>
            // ::new_from_slice(<k>.as_bytes()).map(|mut mac| {
            // mac.update(<d>.as_bytes()); hex::encode(mac.finalize()
            // .into_bytes()) }).unwrap_or_default() }`. Block-scoped
            // `use hmac::Mac;` brings the `Mac` trait's `update` /
            // `finalize` methods into scope (they're trait methods,
            // NOT inherent on `Hmac`).
            //
            // `new_from_slice` returns `Result<Hmac<Sha256>,
            // MacError>` and accepts ANY key length (HMAC has no
            // fixed key size); the `.map(...).unwrap_or_default()`
            // collapses the Err branch to an empty String - NEVER
            // panics, matching Buff's "no panicking generated code"
            // rule (mirrors Base64.decode / Hex.decode / Csv.parse's
            // panic-free stance).
            //
            // Both args accept String OR Vector<Byte> (anything
            // `AsRef<[u8]>`); `.as_bytes()` gives `&[u8]` for both.
            // The `mac.finalize().into_bytes()` returns a
            // `GenericArray<u8, U32>` (32 bytes for SHA-256) that
            // `hex::encode` accepts via `AsRef<[u8]>`.
            //
            // Same shared `Sha256` variant as `Hash.sha256`;
            // dispatched on the (HMAC, Sha256) pair.
            (T::HMAC, A::Sha256) => {
                let mut lowered = n_args(self, 2)?;
                let key = lowered.remove(0);
                let data = lowered.remove(0);
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    {
                        use hmac::Mac;
                        hmac::Hmac::<sha2::Sha256>::new_from_slice(#key.as_bytes())
                            .map(|mut mac| {
                                mac.update(#data.as_bytes());
                                hex::encode(mac.finalize().into_bytes())
                            })
                            .unwrap_or_default()
                    }
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("HMAC.sha256 codegen parse: {e}")))
            }
            // T124l: Process module - 2 assoc fns wrapping
            // `std::process::Command` (spawn) + `std::process::exit`
            // (the side-effecting terminal call). Process is a
            // runtime-value type (mirrors Regex T124d / URL T124h /
            // Path T124j). `Process.*` uses ONLY `std::process` - NO
            // extern crate recorded (mirrors Path/Dir.list stance
            // from T124j).
            //
            // `Process.spawn(command, args)` -> Process. Wraps
            // `std::process::Command::new(<cmd>).args(<args>).spawn()
            // .ok()` (the `.ok()` collapses a spawn failure to None
            // - NEVER panics, matching Buff's "no panicking generated
            // code" rule). The command + args are passed SEPARATELY
            // (NOT through a shell) so there's NO shell-injection
            // vector (the spec's safety stance). The returned value
            // is `Option<std::process::Child>` - the codegen
            // instance-method lowerings (.wait / .id) chain `.map()
            // .unwrap_or_default()` through the Option so spawn-
            // failure is observable as default Int (0) without
            // panicking.
            //
            // The generated Rust type is `Option<std::process::
            // Child>`, surfaced in Buff as `Process` (see the
            // `buff_type_to_syn` arm + the [`Type::Process`] doc).
            (T::Process, A::Spawn) => {
                let mut lowered = n_args(self, 2)?;
                let cmd = lowered.remove(0);
                let args_expr = lowered.remove(0);
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    std::process::Command::new(#cmd)
                        .args(#args_expr)
                        .spawn()
                        .ok()
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Process.spawn codegen parse: {e}")))
            }
            // `Process.exit(code)` -> Void. Wraps
            // `std::process::exit(<code> as i32)`. The call NEVER
            // returns (it terminates the program immediately). NOTE:
            // Rust's `std::process::exit` does NOT run destructors;
            // the Buff surface inherits that behavior (the spec
            // calls this out as the "exit yourself" primitive,
            // distinct from signal-based shutdown which is explicitly
            // out-of-scope). The `as i32` cast narrows Buff's
            // default `Int<64>` to the OS's `i32` exit-code width.
            (T::Process, A::Exit) => {
                let arg = one_arg(self)?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    std::process::exit(#arg as i32)
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Process.exit codegen parse: {e}")))
            }
            // T124l: OS module - 4 assoc fns wrapping
            // `std::env::consts::{OS,ARCH}` + env-var hostname +
            // `num_cpus::get`. OS is namespace-only (mirrors Log /
            // Toml / Math / Strings / Args / Env / Hash / HMAC).
            //
            // `OS.name()` -> String. Wraps
            // `std::env::consts::OS.to_string()` (compile-time
            // const - one of `linux` / `macos` / `windows` /
            // `freebsd` / ...). Zero args.
            (T::OS, A::Name) => {
                no_args(self)?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    std::env::consts::OS.to_string()
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("OS.name codegen parse: {e}")))
            }
            // `OS.arch()` -> String. Wraps
            // `std::env::consts::ARCH.to_string()` (compile-time
            // const - one of `x86_64` / `aarch64` / `x86` / ...).
            // Zero args.
            (T::OS, A::Arch) => {
                no_args(self)?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    std::env::consts::ARCH.to_string()
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("OS.arch codegen parse: {e}")))
            }
            // `OS.hostname()` -> String. Wraps
            // `std::env::var("COMPUTERNAME").or_else(|_|
            // std::env::var("HOSTNAME")).unwrap_or_default()` (empty
            // String when neither env var is set - NEVER panics).
            // The bare-minimum env-var approach covering Windows
            // (COMPUTERNAME) + Unix (HOSTNAME). NO `hostname` crate
            // added (the spec explicitly forbids it - the codegen-
            // only linking boundary limit is observed: this call
            // alone needs NO extern crate). Zero args.
            (T::OS, A::Hostname) => {
                no_args(self)?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    std::env::var("COMPUTERNAME")
                        .or_else(|_| std::env::var("HOSTNAME"))
                        .unwrap_or_default()
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("OS.hostname codegen parse: {e}")))
            }
            // `OS.cpus()` -> Int. Wraps `num_cpus::get() as i64`.
            // The `num_cpus` crate is recorded in codegen
            // `extern_crates` when a Buff program uses `OS.cpus`
            // (the narrow `program_uses_num_cpus` walker flags ONLY
            // the `cpus` method name - `name` / `arch` / `hostname`
            // use std only and record NO extern crate). Zero args.
            (T::OS, A::Cpus) => {
                no_args(self)?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    num_cpus::get() as i64
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("OS.cpus codegen parse: {e}")))
            }
            // T124m: TCP / UDP / WebSocket networking assoc fns.
            // Each wraps an async tokio / tokio-tungstenite connect
            // / bind call via `.await.ok()` (panic-free - a connect
            // or bind failure collapses to `None`). The returned
            // value (Connection / Socket / WsConnection) is the
            // receiver for the corresponding instance methods.
            //
            // Same codegen-only-linking-boundary stance as the
            // other tokio / tokio-tungstenite lowerings: single-
            // file `buff run` rustc path does NOT link tokio (the
            // `.await` calls surface a rustc-level error if the
            // enclosing function is not async - the T31 walker
            // propagates async-ness ONLY through bare-Ident free-
            // function calls, NOT method-call / namespace-assoc-fn
            // calls, so the enclosing-fn-async transformation is a
            // deferral; see issues.md).
            //
            // `TCP.connect(host, port) -> Connection`. Wraps
            // `tokio::net::TcpStream::connect(format!("{}:{}",
            // h, p)).await.ok()` (two args: String host, Int port;
            // the format! builds the `"host:port"` SocketAddr
            // string tokio's connect accepts). The `.ok()`
            // collapses a connect failure to `None` - NEVER panics.
            // The `tokio` crate is recorded in codegen
            // `extern_crates` (idempotent with the existing tokio
            // walker from T124g - any sleep() call OR TCP.* /
            // UDP.* call flags `tokio`).
            (T::TCP, A::Connect) => {
                let mut lowered = n_args(self, 2)?;
                let host = lowered.remove(0);
                let port = lowered.remove(0);
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    tokio::net::TcpStream::connect(format!("{}:{}", #host, #port))
                        .await
                        .ok()
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("TCP.connect codegen parse: {e}")))
            }
            // `UDP.bind(host, port) -> Socket`. Wraps
            // `tokio::net::UdpSocket::bind(format!("{}:{}", h,
            // p)).await.ok()` (two args: String host, Int port).
            // The `.ok()` collapses a bind failure to `None` -
            // NEVER panics.
            (T::UDP, A::Bind) => {
                let mut lowered = n_args(self, 2)?;
                let host = lowered.remove(0);
                let port = lowered.remove(0);
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    tokio::net::UdpSocket::bind(format!("{}:{}", #host, #port))
                        .await
                        .ok()
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("UDP.bind codegen parse: {e}")))
            }
            // `WebSocket.connect(url) -> WsConnection`. Wraps
            // `tokio_tungstenite::connect_async(url).await.ok()
            // .map(|(ws, _)| ws)` (one arg: String url). The
            // `.ok().map(...)` chain collapses a connect failure to
            // `None` and unwraps the `(WebSocketStream, Response)`
            // tuple tokio-tungstenite's connect_async returns -
            // NEVER panics. The `tokio-tungstenite` + `futures-
            // util` crates are recorded in codegen `extern_crates`
            // via the narrow `program_uses_tokio_tungstenite`
            // walker. Same shared `Connect` variant as
            // `TCP.connect(host, port)`; dispatched on the
            // (WebSocket, Connect) pair (mirrors `Parse` shared
            // between DateTime / Date / Toml / URL / UUID).
            (T::WebSocket, A::Connect) => {
                let url = one_arg(self)?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    tokio_tungstenite::connect_async(#url)
                        .await
                        .ok()
                        .map(|(ws, _)| ws)
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("WebSocket.connect codegen parse: {e}")))
            }
            // T2: Channel.new(buf_size) -> (Sender<T>, Receiver<T>).
            // Wraps `buff_lang_runtime::Channel::new(buf_size)` which
            // internally calls `tokio::sync::mpsc::channel(buf_size)`
            // and returns the `(Sender<T>, Receiver<T>)` tuple directly.
            // The runtime hides tokio behind the abstraction per Metis G6.
            // NO turbofish at the call site - Rust's type inference
            // derives T from subsequent `sender.send(value)` /
            // `receiver.recv()` usage (the user never writes the
            // turbofish in Buff source; Buff does not expose explicit
            // generic-type-argument syntax on method calls).
            //
            // One arg (Int buf_size). The codegen does NOT cast the
            // arg to usize (Rust's type inference derives usize from
            // the `tokio::sync::mpsc::channel(buffer: usize)` signature;
            // an untyped Int literal in Buff lowers to `i64`, which
            // Rust's `Into<usize>` would not satisfy without a cast).
            // For the literal case the user typically writes a small
            // positive integer (`Channel.new(10)`, `Channel.new(100)`)
            // which `i64` -> `usize` infers cleanly on most hosts. If
            // a user passes a negative or huge value, Rust surfaces a
            // normal overflow diagnostic at compile time (we do NOT
            // silently coerce).
            (T::Channel, A::New) => {
                let arg = one_arg(self)?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    buff_lang_runtime::Channel::new(#arg as usize)
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Channel.new codegen parse: {e}")))
            }
            // T7: DataFrame.from_csv(path) -> DataFrame. Wraps
            // `buff_dataframe::DataFrame::from_csv(path)
            // .unwrap_or_default()` (panic-free on file-not-found /
            // parse failure — DataFrame impls Default as the empty
            // frame, matching Buff's "no panicking generated code"
            // rule). Records `buff-dataframe` in extern_crates via
            // the narrow `program_uses_namespace("DataFrame")` walker.
            (T::DataFrame, A::FromCsv) => {
                let arg = one_arg(self)?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    buff_dataframe::DataFrame::from_csv(#arg).unwrap_or_default()
                };
                syn::parse2(tokens).map_err(|e| {
                    self.unsupported(&format!("DataFrame.from_csv codegen parse: {e}"))
                })
            }
            // T7: DataFrame.from_json(path) -> DataFrame. Same shape
            // as FromCsv — panic-free via `unwrap_or_default()`.
            // Reads JSON-lines (one JSON object per line).
            (T::DataFrame, A::FromJson) => {
                let arg = one_arg(self)?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    buff_dataframe::DataFrame::from_json(#arg).unwrap_or_default()
                };
                syn::parse2(tokens).map_err(|e| {
                    self.unsupported(&format!("DataFrame.from_json codegen parse: {e}"))
                })
            }
            // T9: Image.from_path(path) -> Image. Wraps
            // `buff_image::Image::from_path(arg).unwrap_or_default()`
            // (panic-free on file-not-found / decode failure / codec
            // panic — Image impls Default as a 1x1 transparent pixel,
            // matching Buff's "no panicking generated code" rule).
            // Records `buff-image` + `image` in extern_crates via the
            // `program_uses_namespace("Image")` walker.
            (T::Image, A::FromPath) => {
                let arg = one_arg(self)?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    buff_image::Image::from_path(#arg).unwrap_or_default()
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Image.from_path codegen parse: {e}")))
            }
            // T9: Image.from_bytes(bytes) -> Image. Same shape as
            // FromPath — panic-free via `unwrap_or_default()`. The
            // arg is a `Vector<Byte>` on the Buff surface (Vec<u8>
            // after codegen lowering); the codegen passes it by ref
            // to `buff_image::Image::from_bytes(&bytes)`.
            (T::Image, A::FromBytes) => {
                let arg = one_arg(self)?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    buff_image::Image::from_bytes(&#arg).unwrap_or_default()
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Image.from_bytes codegen parse: {e}")))
            }
            // T37: Faker.new() -> Faker. Wraps
            // `buff_fake::Faker::new()` (default locale en-US, random
            // seed). Infallible — no unwrap_or_default needed. Records
            // `buff-fake` + `fake` in extern_crates via the
            // `program_uses_namespace("Faker")` walker.
            (T::Faker, A::New) => {
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    buff_fake::Faker::new()
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Faker.new codegen parse: {e}")))
            }
            // T37: Faker.with_locale(locale) -> Faker. One arg (String
            // locale, either "en-US" or "pt-BR"). Wraps
            // `buff_fake::Faker::with_locale(locale)`.
            (T::Faker, A::WithLocale) => {
                let arg = one_arg(self)?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    buff_fake::Faker::with_locale(match #arg.as_str() {
                        "pt-BR" => buff_fake::FakerLocale::PtBr,
                        _ => buff_fake::FakerLocale::EnUs,
                    })
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Faker.with_locale codegen parse: {e}")))
            }
            // T37: Faker.with_seed(locale, seed) -> Faker. Two args
            // (String locale, Int seed). Wraps
            // `buff_fake::Faker::with_seed(locale, seed)`.
            (T::Faker, A::WithSeed) => {
                if args.len() != 2 {
                    return Err(self.unsupported(&format!(
                        "Faker.with_seed expects exactly 2 args (locale, seed), got {}",
                        args.len()
                    )));
                }
                let locale = self.lower_expr(&args[0])?;
                let seed = self.lower_expr(&args[1])?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    buff_fake::Faker::with_seed(match #locale.as_str() {
                        "pt-BR" => buff_fake::FakerLocale::PtBr,
                        _ => buff_fake::FakerLocale::EnUs,
                    }, #seed as u64)
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Faker.with_seed codegen parse: {e}")))
            }
            // T10: AudioBuffer.from_path(path) -> AudioBuffer. Wraps
            // `buff_audio::AudioBuffer::from_path(arg)
            // .unwrap_or_default()` (panic-free on file-not-found /
            // decode failure / codec panic — AudioBuffer impls Default
            // as an empty 44100Hz mono buffer, matching Buff's "no
            // panicking generated code" rule). Records `buff-audio` +
            // `hound` + `symphonia` in extern_crates via the
            // `program_uses_namespace("AudioBuffer")` walker.
            (T::Audio, A::FromPath) => {
                let arg = one_arg(self)?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    buff_audio::AudioBuffer::from_path(#arg).unwrap_or_default()
                };
                syn::parse2(tokens).map_err(|e| {
                    self.unsupported(&format!("AudioBuffer.from_path codegen parse: {e}"))
                })
            }
            // T10: AudioBuffer.from_samples(samples, sample_rate,
            // channels) -> AudioBuffer. Three args (Vec<f32>, u32,
            // u16). The Buff surface passes (Vector<Float>, Int,
            // Int); codegen casts Int -> u32 / u16 at the call site
            // (mirrors the i64 -> usize cast in Channel.new).
            // Panic-free via `unwrap_or_default()` (AudioBuffer
            // impls Default — invalid params collapse to empty).
            (T::Audio, A::FromSamples) => {
                if args.len() != 3 {
                    return Err(self.unsupported(&format!(
                        "from_samples() expects exactly 3 args (samples, sample_rate, channels), got {}",
                        args.len()
                    )));
                }
                let samples = self.lower_expr(&args[0])?;
                let sample_rate = self.lower_expr(&args[1])?;
                let channels = self.lower_expr(&args[2])?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    buff_audio::AudioBuffer::from_samples(#samples, #sample_rate as u32, #channels as u16)
                        .unwrap_or_default()
                };
                syn::parse2(tokens).map_err(|e| {
                    self.unsupported(&format!("AudioBuffer.from_samples codegen parse: {e}"))
                })
            }
            // T26: Audit.scan(path) -> Vector<String>. One arg (String
            // / Path). Wraps `buff_audit::scan(&arg)
            // .unwrap_or_default()` (panic-free on io / advisory-DB
            // failure - empty Vec, matching Buff's "no panicking
            // generated code" rule). Records `buff-audit` +
            // `ed25519-dalek` + `sha2` + `hex` + `rand` in
            // extern_crates via the narrow
            // `program_uses_namespace("Audit")` walker.
            (T::Audit, A::Scan) => {
                let arg = one_arg(self)?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    buff_audit::scan(#arg).unwrap_or_default()
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Audit.scan codegen parse: {e}")))
            }
            // T26: Audit.list() -> Vector<String>. Zero args. Wraps
            // `buff_audit::known_advisories()` (infallible - returns
            // the static `advisory_db::ALL` ID list). Records the
            // same extern_crates set as Audit.scan. Reuses the
            // existing T124g `List` variant (shared between Args.list
            // / Env.list / Audit.list - same shared-variant pattern
            // as Parse / Get / Encode).
            (T::Audit, A::List) => {
                no_args(self)?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    buff_audit::known_advisories()
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Audit.list codegen parse: {e}")))
            }
            // T26: Signature.sign(data, secret_hex) -> String. Two
            // args (Vector<Byte>, String). Wraps `buff_audit::sign
            // (&data, &secret_hex).unwrap_or_default()` (panic-free
            // on bad-key / bad-hex - empty String). The `&#data` is
            // `&Vec<u8>` which Rust auto-derefs to `&[u8]` at the
            // call site. Records the same extern_crates set as
            // Audit.* via the `program_uses_namespace("Signature")`
            // walker.
            (T::Signature, A::Sign) => {
                let mut lowered = n_args(self, 2)?;
                let data = lowered.remove(0);
                let secret_hex = lowered.remove(0);
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    buff_audit::sign(&#data, &#secret_hex).unwrap_or_default()
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Signature.sign codegen parse: {e}")))
            }
            // T26: Signature.verify(data, sig_hex, public_hex) ->
            // Bool. Three args. Wraps `buff_audit::verify(...).unwrap_
            // or(false)` (the unwrap_or(false) is the contract: bad
            // signature, bad key, OR bad hex all collapse to false -
            // NEVER panics, NEVER errors. The T26 task spec mandates
            // the bool return so a future `buff add --no-verify`
            // bypass layers cleanly).
            (T::Signature, A::Verify) => {
                let mut lowered = n_args(self, 3)?;
                let data = lowered.remove(0);
                let sig_hex = lowered.remove(0);
                let public_hex = lowered.remove(0);
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    buff_audit::verify(&#data, &#sig_hex, &#public_hex).unwrap_or(false)
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Signature.verify codegen parse: {e}")))
            }
            // T26: Signature.keypair() -> (String, String). Zero
            // args. Wraps `buff_audit::keypair()
            // .unwrap_or_default()` (the unwrap_or_default collapses
            // a Panic error to two empty Strings - NEVER panics).
            // Used by `buff publish --sign` to mint a fresh signing
            // identity per package release.
            (T::Signature, A::Keypair) => {
                no_args(self)?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    buff_audit::keypair().unwrap_or_default()
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Signature.keypair codegen parse: {e}")))
            }
            // T27: Fuzz.run(strategy, iterations, closure) -> Void.
            // Three args (Strategy, Int, closure). Wraps
            // `buff_fuzz::run(&strategy, iterations as u32, |n: i64| closure_body(n))`.
            // The lowered call returns FuzzSummary; the codegen
            // discards it via `let _ = buff_fuzz::run(...)` so the
            // Buff surface stays Void-only. Records `buff-fuzz` +
            // `proptest` in extern_crates via the narrow
            // `program_uses_namespace("Fuzz")` walker.
            //
            // The `Run` variant is Fuzz-only - no other prelude type
            // exposes a method named `run` today.
            (T::Fuzz, A::Run) => {
                if args.len() != 3 {
                    return Err(self.unsupported(&format!(
                        "Fuzz.run() expects exactly 3 args (strategy, iterations, closure), got {}",
                        args.len()
                    )));
                }
                let strategy = self.lower_expr(&args[0])?;
                let iterations = self.lower_expr(&args[1])?;
                let closure = self.lower_expr(&args[2])?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    let _ = buff_fuzz::run(&#strategy, #iterations as u32, #closure);
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Fuzz.run codegen parse: {e}")))
            }
            // T27: Strategy.int(min, max) -> Strategy. Two args (Int, Int).
            // Wraps `buff_fuzz::Strategy::int(min, max)`. The `Int`
            // variant is shared with `Random.int(min, max)` (T124f),
            // dispatched on the (Strategy, Int) pair.
            (T::Strategy, A::Int) => {
                if args.len() != 2 {
                    return Err(self.unsupported(&format!(
                        "Strategy.int() expects exactly 2 args (min, max), got {}",
                        args.len()
                    )));
                }
                let min = self.lower_expr(&args[0])?;
                let max = self.lower_expr(&args[1])?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    buff_fuzz::Strategy::int(#min, #max)
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Strategy.int codegen parse: {e}")))
            }
            // T27: Strategy.float(min, max) -> Strategy. Two args (Float, Float).
            // Wraps `buff_fuzz::Strategy::float(min, max)`. The `Float`
            // variant is shared with `Random.float()` (T124f), dispatched
            // on the (Strategy, Float) pair.
            (T::Strategy, A::Float) => {
                if args.len() != 2 {
                    return Err(self.unsupported(&format!(
                        "Strategy.float() expects exactly 2 args (min, max), got {}",
                        args.len()
                    )));
                }
                let min = self.lower_expr(&args[0])?;
                let max = self.lower_expr(&args[1])?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    buff_fuzz::Strategy::float(#min as f64, #max as f64)
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Strategy.float codegen parse: {e}")))
            }
            // T27: Strategy.bool() -> Strategy. Zero args. Wraps
            // `buff_fuzz::Strategy::bool()`.
            (T::Strategy, A::Bool) => {
                no_args(self)?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    buff_fuzz::Strategy::bool()
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Strategy.bool codegen parse: {e}")))
            }
            // T27: Strategy.string(max_len) -> Strategy. One arg (Int).
            // Wraps `buff_fuzz::Strategy::string(max_len)`.
            (T::Strategy, A::String) => {
                let arg = one_arg(self)?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    buff_fuzz::Strategy::string(#arg as usize)
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Strategy.string codegen parse: {e}")))
            }
            // T27: Strategy.bytes(max_len) -> Strategy. One arg (Int).
            // Wraps `buff_fuzz::Strategy::bytes(max_len)`.
            (T::Strategy, A::Bytes) => {
                let arg = one_arg(self)?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    buff_fuzz::Strategy::bytes(#arg as usize)
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Strategy.bytes codegen parse: {e}")))
            }
            // T20: ReactiveSignal.new(value) -> Signal<T>. One arg (T).
            // Wraps `buff_reactive::Signal::new(value)` (infallible).
            (T::ReactiveSignal, A::New) => {
                let arg = one_arg(self)?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    buff_reactive::Signal::new(#arg)
                };
                syn::parse2(tokens).map_err(|e| {
                    self.unsupported(&format!("ReactiveSignal.new codegen parse: {e}"))
                })
            }
            // T20: ReactiveComputed.new(fn) -> Computed<T>. One arg
            // (closure `Fn() -> T`). Wraps `buff_reactive::Computed::new(fn)`.
            (T::ReactiveComputed, A::New) => {
                let arg = one_arg(self)?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    buff_reactive::Computed::new(#arg)
                };
                syn::parse2(tokens).map_err(|e| {
                    self.unsupported(&format!("ReactiveComputed.new codegen parse: {e}"))
                })
            }
            // T20: ReactiveEffect.new(fn) -> Effect. One arg (closure
            // `Fn() -> Void`). Wraps `buff_reactive::Effect::new(fn)`.
            (T::ReactiveEffect, A::New) => {
                let arg = one_arg(self)?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    buff_reactive::Effect::new(#arg)
                };
                syn::parse2(tokens).map_err(|e| {
                    self.unsupported(&format!("ReactiveEffect.new codegen parse: {e}"))
                })
            }
            // T18: Database.connect(url) -> Pool (forward-declared as
            // `Type::Unknown` in prelude_types.rs; the buff-db crate's
            // `Pool` type IS the runtime value). Wraps
            // `buff_db::Pool::connect(&url).await?` (the `?` propagates
            // `DbError` per Buff's R3 error-mapping contract — the
            // Buff user's surrounding fn must return
            // `Result<T, buff_db::DbError>` so the `?` splices
            // cleanly; the Buff `?` operator is the standard error-
            // propagation idiom, mirroring `regex::Regex::new(p)?`
            // from T124d and `buff_image::Image::from_path(p)?` from
            // T9). The `.await` is auto-inserted by the T31 async-
            // propagation pass when the surrounding fn is async
            // (Buff has no `await` keyword). Records `buff-db` +
            // `sqlx` + `tokio` in extern_crates via the narrow
            // `program_uses_namespace("Database")` walker. Same
            // shared `Connect` variant as `TCP.connect(host, port)`
            // / `WebSocket.connect(url)`; dispatched on the
            // (Database, Connect) pair (mirrors `Parse` shared
            // between DateTime / Date / Toml / URL / UUID).
            //
            // One arg (String url — e.g. `"sqlite::memory:"` or
            // `"postgres://user:pass@host/db"`). The codegen does NOT
            // cast the arg — it passes the owned `String` directly to
            // `Pool::connect(url: &str)` (Rust's deref coercion lifts
            // `String` to `&str` automatically). The returned `Pool`
            // value is the receiver for the deferred `.query()` /
            // `.execute()` / `.begin()` instance methods (a sibling
            // task adds `Type::Pool` + instance-method lowering arms).
            (T::Database, A::Connect) => {
                let url = one_arg(self)?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    buff_db::Pool::connect(&#url).await?
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Database.connect codegen parse: {e}")))
            }
            // T17: Web.new() -> Web. Zero args. Wraps
            // `buff_web::Web::new()` (infallible - returns an empty
            // Web with no routes / no middleware / no bind addr).
            // Records `buff-web` + `axum` + `tokio` + `serde_json` in
            // extern_crates via the `program_uses_namespace("Web")`
            // walker. Dispatch on (PreludeType::Web, New) - mirrors
            // the (Channel, New) precedent (Channel.new also returns
            // a runtime value via a zero-arg ctor).
            (T::Web, A::New) => {
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    buff_web::Web::new()
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Web.new codegen parse: {e}")))
            }
            // T17: Web.bind(addr) -> Web. One arg (String). Wraps
            // `buff_web::Web::bind(addr)` (infallible - returns an
            // empty Web with the bind addr preset; the user adds
            // routes via web.get / web.post / ... and starts serving
            // via web.run()).
            //
            // Same shared `Bind` variant as `UDP.bind(host, port)`
            // (T124m) - dispatched on (Web, Bind) pair (mirrors
            // `Parse` shared between DateTime / Date / Toml / URL /
            // UUID, `Connect` shared between TCP / WebSocket).
            (T::Web, A::Bind) => {
                let arg = one_arg(self)?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    buff_web::Web::bind(#arg)
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Web.bind codegen parse: {e}")))
            }
            // T33: HttpClient.new() -> HttpClient. Zero args. Wraps
            // `buff_http_client::HttpClient::new()` (infallible -
            // returns a new client with default settings). Records
            // `buff-http-client` + `reqwest` in extern_crates via the
            // `program_uses_namespace("HttpClient")` walker. Dispatch
            // on (PreludeType::HttpClient, New) - mirrors the (Web,
            // New) / (Channel, New) precedent.
            (T::HttpClient, A::New) => {
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    buff_http_client::HttpClient::new()
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("HttpClient.new codegen parse: {e}")))
            }
            // T29: Validator.new() -> Validator. Zero args. Wraps
            // `buff_validate::Validator::new()` (infallible - returns
            // an empty rule set). Records `buff-validate` +
            // `validator` + `serde_json` + `regex` in extern_crates
            // via the `program_uses_namespace("Validator")` walker.
            // Dispatch on (PreludeType::Validator, New) - mirrors the
            // (HttpClient, New) / (Channel, New) precedent.
            (T::Validator, A::New) => {
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    buff_validate::Validator::new()
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Validator.new codegen parse: {e}")))
            }
            // T42: Email.new(from, to, subject) -> Email. Three args
            // (String from, String to, String subject). Wraps
            // `buff_email::Email::new(&from, &to, &subject)?` (the
            // `?` propagates EmailError::InvalidAddress per Buff's
            // R3 error-mapping contract). Records `buff-email` +
            // `lettre` + `handlebars` in extern_crates via the
            // `program_uses_namespace("Email")` walker. Dispatch on
            // (PreludeType::Email, New) - mirrors the (Validator,
            // New) / (HttpClient, New) / (Cache, New) precedent.
            (T::Email, A::New) => {
                if args.len() != 3 {
                    return Err(self.unsupported(&format!(
                        "Email.new() expects exactly 3 args (from, to, subject), got {}",
                        args.len()
                    )));
                }
                let from = self.lower_expr(&args[0])?;
                let to = self.lower_expr(&args[1])?;
                let subject = self.lower_expr(&args[2])?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    buff_email::Email::new(&#from, &#to, &#subject)?
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Email.new codegen parse: {e}")))
            }
            // T42: SmtpClient.new(host, port, username, password) ->
            // SmtpClient. Four args (String host, Int port, String
            // username, String password). Wraps
            // `buff_email::SmtpClient::new(&host, port as u16, &user,
            // &pass)?` (the `?` propagates EmailError::InvalidRelay).
            // Records `buff-email` + `lettre` in extern_crates
            // (shared walker with Email). Dispatch on
            // (PreludeType::SmtpClient, New).
            (T::SmtpClient, A::New) => {
                if args.len() != 4 {
                    return Err(self.unsupported(&format!(
                        "SmtpClient.new() expects exactly 4 args (host, port, username, password), got {}",
                        args.len()
                    )));
                }
                let host = self.lower_expr(&args[0])?;
                let port = self.lower_expr(&args[1])?;
                let username = self.lower_expr(&args[2])?;
                let password = self.lower_expr(&args[3])?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    buff_email::SmtpClient::new(&#host, #port as u16, &#username, &#password)?
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("SmtpClient.new codegen parse: {e}")))
            }
            // T31: Cache.new(max_capacity) -> Cache. One arg (Int).
            // Wraps `buff_cache::Cache::new(max_capacity as u64)
            // .unwrap_or_default()` (panic-free on zero-capacity —
            // Cache impls Default as a 1024-capacity empty cache,
            // matching Buff's "no panicking generated code" rule).
            // Records `buff-cache` + `moka` in extern_crates via the
            // `program_uses_namespace("Cache")` walker.
            (T::Cache, A::New) => {
                let arg = one_arg(self)?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    buff_cache::Cache::new(#arg as u64).unwrap_or_default()
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Cache.new codegen parse: {e}")))
            }
            // T44: I18n.new(locale) -> I18n. One arg (String). Wraps
            // `buff_i18n::I18n::new(&locale).unwrap_or_default()`
            // (panic-free on invalid locale — I18n impls Default as
            // an empty English catalog, matching Buff's "no
            // panicking generated code" rule + the Image / Cache /
            // Document precedent). Records `buff-i18n` +
            // `fluent-bundle` + `unic-langid` in extern_crates via
            // the `program_uses_namespace("I18n")` walker.
            (T::I18n, A::New) => {
                let arg = one_arg(self)?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    buff_i18n::I18n::new(&#arg).unwrap_or_default()
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("I18n.new codegen parse: {e}")))
            }
            // T44: I18n.with_fallback(locale, fallback) -> I18n. Two
            // args (String locale, String fallback). Wraps
            // `buff_i18n::I18n::with_fallback(&locale, &fallback)
            // .unwrap_or_default()` (panic-free).
            (T::I18n, A::WithFallback) => {
                if args.len() != 2 {
                    return Err(self.unsupported(&format!(
                        "with_fallback() expects exactly 2 args (locale, fallback), got {}",
                        args.len()
                    )));
                }
                let locale = self.lower_expr(&args[0])?;
                let fallback = self.lower_expr(&args[1])?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    buff_i18n::I18n::with_fallback(&#locale, &#fallback).unwrap_or_default()
                };
                syn::parse2(tokens).map_err(|e| {
                    self.unsupported(&format!("I18n.with_fallback codegen parse: {e}"))
                })
            }
            // T43: Document.from_html(html) -> Document. One arg
            // (String). Wraps `buff_scrape::Document::from_html(&html)
            // .unwrap_or_default()` (panic-free on empty input —
            // Document impls Default as `<html></html>`, matching
            // Buff's "no panicking generated code" rule; mirrors the
            // Image.from_path `unwrap_or_default()` precedent). Records
            // `buff-scrape` + `scraper` in extern_crates via the
            // `program_uses_namespace("Document")` walker.
            (T::Document, A::FromHtml) => {
                let arg = one_arg(self)?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    buff_scrape::Document::from_html(&#arg).unwrap_or_default()
                };
                syn::parse2(tokens).map_err(|e| {
                    self.unsupported(&format!("Document.from_html codegen parse: {e}"))
                })
            }
            // T43: Crawler.new(seed_url) -> Crawler. One arg (String).
            // Wraps `buff_scrape::Crawler::new(&seed)
            // .unwrap_or_default()` (panic-free on empty seed —
            // Crawler impls Default as an about:blank-seeded client).
            // Records `buff-scrape` + `reqwest` in extern_crates via
            // the `program_uses_namespace("Crawler")` walker.
            (T::Crawler, A::New) => {
                let arg = one_arg(self)?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    buff_scrape::Crawler::new(&#arg).unwrap_or_default()
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Crawler.new codegen parse: {e}")))
            }
            // T30: Config module — namespace-only (no runtime value).
            // `Config.new()` creates a `buff_config::Config` and stores
            // it in a thread-local static so subsequent method calls
            // (`cfg.set_default`, `cfg.load_file`, etc.) operate on the
            // same instance. The codegen emits a lazy-static pattern
            // (one Config per thread — no Mutex contention for the
            // common single-threaded case). Mirrors the Log / Toml /
            // Math namespace-only pattern.
            (T::Config, A::New) => {
                no_args(self)?;
                let tokens: proc_macro2::TokenStream = quote::quote! {{
                    static CONFIG: std::sync::LazyLock<buff_config::Config> =
                        std::sync::LazyLock::new(buff_config::Config::new);
                    &*CONFIG
                }};
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Config.new codegen parse: {e}")))
            }
            // `Config.set_default(key, val)` -> Void. Two args.
            (T::Config, A::SetDefault) => {
                if args.len() != 2 {
                    return Err(self.unsupported(&format!(
                        "set_default() expects exactly 2 args (key, value), got {}",
                        args.len()
                    )));
                }
                let key = self.lower_expr(&args[0])?;
                let val = self.lower_expr(&args[1])?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    CONFIG.set_default(#key, #val)
                };
                syn::parse2(tokens).map_err(|e| {
                    self.unsupported(&format!("Config.set_default codegen parse: {e}"))
                })
            }
            // `Config.load_file(path)` -> Void. One arg.
            (T::Config, A::LoadFile) => {
                let arg = one_arg(self)?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    CONFIG.load_file(#arg).unwrap_or_default()
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Config.load_file codegen parse: {e}")))
            }
            // `Config.load_env(prefix)` -> Void. One arg.
            (T::Config, A::LoadEnv) => {
                let arg = one_arg(self)?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    CONFIG.load_env(#arg)
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Config.load_env codegen parse: {e}")))
            }
            // `Config.load_args(args)` -> Void. One arg (Vector<String>).
            (T::Config, A::LoadArgs) => {
                let arg = one_arg(self)?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    CONFIG.load_args(&#arg)
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Config.load_args codegen parse: {e}")))
            }
            // `Config.get(key)` -> Option<String>. One arg. Reuses the
            // shared `Get` variant (also used by Args.get / Env.get).
            (T::Config, A::Get) => {
                let arg = one_arg(self)?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    CONFIG.get(#arg)
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Config.get codegen parse: {e}")))
            }
            // `Config.get_int(key)` -> Option<Int>. One arg.
            (T::Config, A::GetInt) => {
                let arg = one_arg(self)?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    CONFIG.get_int(#arg)
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Config.get_int codegen parse: {e}")))
            }
            // `Config.get_float(key)` -> Option<Float>. One arg.
            (T::Config, A::GetFloat) => {
                let arg = one_arg(self)?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    CONFIG.get_float(#arg)
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Config.get_float codegen parse: {e}")))
            }
            // `Config.get_bool(key)` -> Option<Bool>. One arg.
            (T::Config, A::GetBool) => {
                let arg = one_arg(self)?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    CONFIG.get_bool(#arg)
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Config.get_bool codegen parse: {e}")))
            }
            // `Config.watch(path, callback)` -> Void. Two args.
            (T::Config, A::Watch) => {
                if args.len() != 2 {
                    return Err(self.unsupported(&format!(
                        "watch() expects exactly 2 args (path, callback), got {}",
                        args.len()
                    )));
                }
                let path = self.lower_expr(&args[0])?;
                let cb = self.lower_expr(&args[1])?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    CONFIG.watch(#path, #cb).unwrap_or_default()
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Config.watch codegen parse: {e}")))
            }
            // T34: buff-auth assoc fns. The 5 (type, method) pairs
            // below cover the MVP surface: JWT.encode / JWT.decode /
            // Password.hash / Password.verify / OAuth2Client.new /
            // Rbac.new. The instance-method forms
            // (client.authorization_url / client.exchange_code /
            // policy.add / policy.enforce) are deferred to the sibling
            // task that adds Type::OAuth2Client / Type::Rbac — mirrors
            // the T17 Web (web.get / web.listen) + T18 Database
            // (pool.query / pool.execute) forward-declaration
            // precedent. Records `buff-auth` + `jsonwebtoken` +
            // `argon2` + `oauth2` + `reqwest` in extern_crates via the
            // shared `program_uses_namespace("JWT"|"OAuth2Client"|
            // "Password"|"Rbac")` walker.
            //
            // JWT.encode(claims, secret) -> String. Two args
            // (Map<String, Unknown>, String). Wraps
            // `buff_auth::jwt_encode(&claims_obj, &secret)
            // .unwrap_or_default()` (panic-free — empty String on
            // encode failure, NEVER panics). The codegen serialises
            // the Buff Map<String, Unknown> arg to a serde_json Value
            // and extracts the inner Map<String, Value> via
            // `.as_object().cloned()` (the Buff Map<String, Unknown>
            // lowers to a `std::collections::HashMap<String, ?>`
            // whose serde_json serialisation round-trips through
            // Map<String, Value>).
            (T::Jwt, A::Encode) => {
                if args.len() != 2 {
                    return Err(self.unsupported(&format!(
                        "JWT.encode() expects exactly 2 args (claims, secret), got {}",
                        args.len()
                    )));
                }
                let claims = self.lower_expr(&args[0])?;
                let secret = self.lower_expr(&args[1])?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    buff_auth::jwt_encode(
                        &serde_json::to_value(&#claims)
                            .ok()
                            .and_then(|v| v.as_object().cloned())
                            .unwrap_or_default(),
                        &#secret,
                    ).unwrap_or_default()
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("JWT.encode codegen parse: {e}")))
            }
            // JWT.decode(token, secret) -> Map<String, Unknown>. Two
            // args (String, String). Wraps
            // `buff_auth::jwt_decode(&token, &secret).unwrap_or_default()`
            // (panic-free — invalid signature / malformed token /
            // expired all collapse to an empty Map, NEVER panics).
            (T::Jwt, A::Decode) => {
                let (token, secret) = two_args(self)?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    buff_auth::jwt_decode(#token, #secret)
                        .unwrap_or_default()
                        .into_iter().collect()
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("JWT.decode codegen parse: {e}")))
            }
            // Password.hash(plain) -> String. One arg (String). Wraps
            // `buff_auth::password_hash(plain).unwrap_or_default()`
            // (panic-free — empty String on hash failure, NEVER
            // panics).
            (T::Password, A::PasswordHash) => {
                let arg = one_arg(self)?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    buff_auth::password_hash(#arg).unwrap_or_default()
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Password.hash codegen parse: {e}")))
            }
            // Password.verify(plain, phc_hash) -> Bool. Two args
            // (String, String). Wraps
            // `buff_auth::password_verify(plain, hash).unwrap_or(false)`
            // (panic-free — false on mismatch or hash-format failure,
            // NEVER panics). Mirrors the T26 Signature.verify lowering
            // stance: verification failure is Ok(false), NOT an error.
            (T::Password, A::PasswordVerify) => {
                let (plain, hash) = two_args(self)?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    buff_auth::password_verify(#plain, #hash).unwrap_or(false)
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Password.verify codegen parse: {e}")))
            }
            // T39: Archive.compress_dir(input_dir, output_path) -> Void.
            // Two args. Wraps `buff_archive::Archive::compress_dir(
            // input_dir, output_path, buff_archive::Format::from_path(
            // std::path::Path::new(&output_path)).unwrap_or(
            // buff_archive::Format::Zip))?` (the `?` propagates
            // `ArchiveError` per Buff's R3 error-mapping contract; the
            // format is auto-detected from the output_path extension —
            // `.zip` → Zip, `.tar.gz` → Gz, `.tar.zst` → Zstd, etc.,
            // matching the cross-language convention of `tar -czf
            // x.tar.gz src/`). Records `buff-archive` + `zip` + `tar`
            // + `flate2` + `ruzstd` in extern_crates via the
            // `program_uses_namespace("Archive")` walker.
            (T::Archive, A::CompressDir) => {
                let (input_dir, output_path) = two_args(self)?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    buff_archive::Archive::compress_dir(
                        #input_dir,
                        #output_path,
                        buff_archive::Format::from_path(std::path::Path::new(&#output_path))
                            .unwrap_or(buff_archive::Format::Zip),
                    )?
                };
                syn::parse2(tokens).map_err(|e| {
                    self.unsupported(&format!("Archive.compress_dir codegen parse: {e}"))
                })
            }
            // T39: Archive.extract(archive_path, output_dir) -> Void.
            // Two args. Wraps `buff_archive::Archive::extract(
            // archive_path, output_dir)?` (the format is auto-detected
            // from the file's extension inside the wrapper). Records
            // `buff-archive` + `zip` + `tar` + `flate2` + `ruzstd` in
            // extern_crates via the `program_uses_namespace("Archive")`
            // walker.
            (T::Archive, A::Extract) => {
                let (archive_path, output_dir) = two_args(self)?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    buff_archive::Archive::extract(#archive_path, #output_dir)?
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Archive.extract codegen parse: {e}")))
            }
            // T51: MsgPack.serialize(value) -> Vector<Byte>. Wraps
            // `buff_msgpack::serialize(&value).unwrap_or_default()`
            // (empty Vec on serialize failure — NEVER panics, matching
            // Buff's "no panicking generated code" rule). Records
            // `buff-msgpack` + `rmp-serde` + `serde_json` in
            // extern_crates via the
            // `program_uses_namespace("MsgPack")` walker.
            (T::MsgPack, A::Serialize) => {
                let arg = one_arg(self)?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    buff_msgpack::serialize(&#arg).unwrap_or_default()
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("MsgPack.serialize codegen parse: {e}")))
            }
            // T51: MsgPack.deserialize(bytes) -> Value. Wraps
            // `buff_msgpack::deserialize(&bytes).unwrap_or_default()`
            // (returns `serde_json::Value::Null` on deserialize failure
            // — `Value` impls `Default`; NEVER panics). The arg is a
            // `Vector<Byte>` on the Buff surface (Vec<u8> after
            // codegen lowering); the codegen passes it by ref.
            (T::MsgPack, A::Deserialize) => {
                let arg = one_arg(self)?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    buff_msgpack::deserialize(&#arg).unwrap_or_default()
                };
                syn::parse2(tokens).map_err(|e| {
                    self.unsupported(&format!("MsgPack.deserialize codegen parse: {e}"))
                })
            }
            // T51: MsgPack.roundtrip(value) -> Option<Value>. Wraps
            // `buff_msgpack::roundtrip(&value)` directly — the
            // runtime fn already returns `Option<serde_json::Value>`
            // (None on either step failing). NEVER panics.
            (T::MsgPack, A::Roundtrip) => {
                let arg = one_arg(self)?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    buff_msgpack::roundtrip(&#arg)
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("MsgPack.roundtrip codegen parse: {e}")))
            }
            // T52: Protobuf.serialize(value) -> Vector<Byte>. Wraps
            // `buff_protobuf::serialize(&value).unwrap_or_default()`
            // (empty Vec on serialize failure — NEVER panics, matching
            // Buff's "no panicking generated code" rule). Records
            // `buff-protobuf` + `prost` + `prost-types` + `serde_json`
            // in extern_crates via the
            // `program_uses_namespace("Protobuf")` /
            // `program_uses_namespace("Message")` walker. Mirrors T51
            // MsgPack.serialize 1:1.
            (T::Protobuf, A::Serialize) => {
                let arg = one_arg(self)?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    buff_protobuf::serialize(&#arg).unwrap_or_default()
                };
                syn::parse2(tokens).map_err(|e| {
                    self.unsupported(&format!("Protobuf.serialize codegen parse: {e}"))
                })
            }
            // T52: Protobuf.deserialize(bytes) -> Value. Wraps
            // `buff_protobuf::deserialize(&bytes).unwrap_or_default()`
            // (returns `serde_json::Value::Null` on deserialize failure
            // — `Value` impls `Default`; NEVER panics). The arg is a
            // `Vector<Byte>` on the Buff surface (Vec<u8> after
            // codegen lowering); the codegen passes it by ref. Mirrors
            // T51 MsgPack.deserialize 1:1.
            (T::Protobuf, A::Deserialize) => {
                let arg = one_arg(self)?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    buff_protobuf::deserialize(&#arg).unwrap_or_default()
                };
                syn::parse2(tokens).map_err(|e| {
                    self.unsupported(&format!("Protobuf.deserialize codegen parse: {e}"))
                })
            }
            // T52: Protobuf.roundtrip(value) -> Option<Value>. Wraps
            // `buff_protobuf::roundtrip(&value)` directly — the
            // runtime fn already returns `Option<serde_json::Value>`
            // (None on either step failing). NEVER panics. Mirrors T51
            // MsgPack.roundtrip 1:1.
            (T::Protobuf, A::Roundtrip) => {
                let arg = one_arg(self)?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    buff_protobuf::roundtrip(&#arg)
                };
                syn::parse2(tokens).map_err(|e| {
                    self.unsupported(&format!("Protobuf.roundtrip codegen parse: {e}"))
                })
            }
            // T52: Message.new(value) -> Message. One arg (the value
            // to encode). Wraps `buff_protobuf::Message::new(&value)
            // .unwrap_or_default()` (panic-free on encode failure —
            // Message impls Default as an empty-payload message).
            // `New` is shared with Channel.new / Faker.new /
            // Crawler.new / Point.new / XmlElement.new — dispatched
            // on the (Message, New) pair. Records `buff-protobuf` +
            // `prost` + `prost-types` + `serde_json` in extern_crates
            // via the `program_uses_namespace("Message")` walker.
            (T::Message, A::New) => {
                let arg = one_arg(self)?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    buff_protobuf::Message::new(&#arg).unwrap_or_default()
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Message.new codegen parse: {e}")))
            }
            // T52: Message.from_bytes(bytes) -> Message. One arg
            // (Vector<Byte>). Wraps
            // `buff_protobuf::Message::from_bytes(bytes)
            // .unwrap_or_default()` (panic-free on decode failure /
            // empty buffer — Message impls Default). `FromBytes` is
            // shared with Image.from_bytes — dispatched on the
            // (Message, FromBytes) pair.
            (T::Message, A::FromBytes) => {
                let arg = one_arg(self)?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    buff_protobuf::Message::from_bytes(#arg).unwrap_or_default()
                };
                syn::parse2(tokens).map_err(|e| {
                    self.unsupported(&format!("Message.from_bytes codegen parse: {e}"))
                })
            }
            // T52: Message.decode(bytes) -> Message. Class-method
            // alias for Message.from_bytes — same lowering shape but
            // takes a `&[u8]` (Bytes ref) instead of `Vec<u8>` (owned
            // bytes). The codegen splices the arg directly (Buff's
            // `Vector<Byte>` lowers to `Vec<u8>` which deref-coerces
            // to `&[u8]` for the underlying `Message::decode(&[u8])`
            // Rust signature). `Decode` is shared with Base64.decode /
            // Hex.decode / URLEncode.decode — dispatched on the
            // (Message, Decode) pair.
            (T::Message, A::Decode) => {
                let arg = one_arg(self)?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    buff_protobuf::Message::decode(&#arg).unwrap_or_default()
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Message.decode codegen parse: {e}")))
            }
            // T47: Bot.new(platform, token) -> Bot. Two args (Platform,
            // String). Wraps `buff_chat::Bot::new(platform, token)
            // .unwrap_or_default()` (panic-free on construction
            // failure — Bot impls Default as an empty Discord bot,
            // added in the T47 MVP commit). `New` is shared with
            // Channel.new / Faker.new / Point.new / XmlElement.new /
            // Message.new (T52) — dispatched on the (Bot, New) pair.
            // Records `buff-chat` + `serenity` + `teloxide` +
            // `async-trait` + `tokio` in extern_crates via the
            // `program_uses_namespace("Bot")` walker.
            (T::Bot, A::New) => {
                if args.len() != 2 {
                    return Err(self.unsupported(&format!(
                        "Bot.new expects exactly 2 args (platform, token), got {}",
                        args.len()
                    )));
                }
                let platform = self.lower_expr(&args[0])?;
                let token = self.lower_expr(&args[1])?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    buff_chat::Bot::new(#platform, (#token).to_string()).unwrap_or_default()
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Bot.new codegen parse: {e}")))
            }
            // T47: ChatMessage.new(text, channel, author, platform,
            // is_dm) -> ChatMessage. Five args (String, String,
            // String, Platform, Bool). Wraps
            // `buff_chat::Message::new(text, channel, author,
            // platform, is_dm)` directly (infallible — the underlying
            // Message::new ctor has no failure mode). The token-
            // coercion `(#x).to_string()` on each String arg handles
            // Buff's `&str`-from-literal lowering (the codegen inserts
            // the coercion so the user can pass either String or &str
            // — the wrapper ctor takes owned `String` per FFI guide
            // R5). Records `buff-chat` + `serenity` + `teloxide` +
            // `async-trait` + `tokio` in extern_crates via the
            // `program_uses_namespace("ChatMessage")` walker.
            (T::ChatMessage, A::New) => {
                if args.len() != 5 {
                    return Err(self.unsupported(&format!(
                        "ChatMessage.new expects exactly 5 args (text, channel, author, platform, is_dm), got {}",
                        args.len()
                    )));
                }
                let text = self.lower_expr(&args[0])?;
                let channel = self.lower_expr(&args[1])?;
                let author = self.lower_expr(&args[2])?;
                let platform = self.lower_expr(&args[3])?;
                let is_dm = self.lower_expr(&args[4])?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    buff_chat::Message::new(
                        (#text).to_string(),
                        (#channel).to_string(),
                        (#author).to_string(),
                        #platform,
                        #is_dm,
                    )
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("ChatMessage.new codegen parse: {e}")))
            }
            // T50: Xml.from_str(xml) -> XmlDocument. One arg (String).
            // Wraps `buff_xml::XmlDocument::from_str(&xml)
            // .unwrap_or_default()` (panic-free on empty/parse failure —
            // XmlDocument impls Default as a root-only document). Records
            // `buff-xml` + `quick-xml` in extern_crates via the
            // `program_uses_namespace("Xml")` walker.
            (T::Xml, A::FromStr) => {
                let arg = one_arg(self)?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    buff_xml::XmlDocument::from_str(&#arg).unwrap_or_default()
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Xml.from_str codegen parse: {e}")))
            }
            // T50: XmlElement.new(name, text, attrs) -> XmlElement.
            // Three args (String, String, Map<String,String>). Wraps
            // `buff_xml::XmlElement::new(&name, &text, attrs_vec)`
            // where `attrs_vec` is built by `.into_iter().map(|(k, v)|
            // (k.to_string(), v.to_string())).collect::<Vec<(String,
            // String)>>()` — the conversion accepts any IntoIterator
            // yielding string-like tuples (Buff Map literal codegens
            // to `HashMap<&str, &str>`; user-passed `HashMap<String,
            // String>` / `Vec<(String, String)>` also work). Infallible
            // (the wrapper ctor never fails — no `?` / unwrap_or_default
            // needed). Records `buff-xml` + `quick-xml` in extern_crates
            // via the `program_uses_namespace("XmlElement")` walker.
            (T::XmlElement, A::New) => {
                if args.len() != 3 {
                    return Err(self.unsupported(&format!(
                        "XmlElement.new expects exactly 3 args (name, text, attrs), got {}",
                        args.len()
                    )));
                }
                let name = self.lower_expr(&args[0])?;
                let text = self.lower_expr(&args[1])?;
                let attrs = self.lower_expr(&args[2])?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    buff_xml::XmlElement::new(
                        &(#name).to_string(),
                        &(#text).to_string(),
                        (#attrs)
                            .into_iter()
                            .map(|(k, v)| (
                                std::string::ToString::to_string(&k),
                                std::string::ToString::to_string(&v)
                            ))
                            .collect::<Vec<(String, String)>>(),
                    )
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("XmlElement.new codegen parse: {e}")))
            }
            // T45: Point.new(x, y) -> Point. Two args (Float, Float).
            // Wraps `buff_geo::Point::new(x, y)` (infallible — the
            // underlying geo_types::Point::new never fails). Records
            // `buff-geo` + `geo` + `geo-types` in extern_crates via the
            // `program_uses_namespace("Point")` walker.
            (T::Point, A::New) => {
                if args.len() != 2 {
                    return Err(self.unsupported(&format!(
                        "Point.new expects exactly 2 args (x, y), got {}",
                        args.len()
                    )));
                }
                let x = self.lower_expr(&args[0])?;
                let y = self.lower_expr(&args[1])?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    buff_geo::Point::new(#x, #y)
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Point.new codegen parse: {e}")))
            }
            // T45: LineString.new(points) -> LineString. One arg
            // (Vector<Point>). Wraps
            // `buff_geo::LineString::new(points).unwrap_or_default()`
            // (panic-free on empty input — LineString impls Default).
            (T::LineString, A::New) => {
                let arg = one_arg(self)?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    buff_geo::LineString::new(#arg).unwrap_or_default()
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("LineString.new codegen parse: {e}")))
            }
            // T45: LineString.from_coords(flat) -> LineString. One arg
            // (Vector<Float>). Wraps
            // `buff_geo::LineString::from_coords(coords).unwrap_or_default()`
            // (panic-free on empty / odd-length input).
            (T::LineString, A::FromCoords) => {
                let arg = one_arg(self)?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    buff_geo::LineString::from_coords(#arg).unwrap_or_default()
                };
                syn::parse2(tokens).map_err(|e| {
                    self.unsupported(&format!("LineString.from_coords codegen parse: {e}"))
                })
            }
            // T45: Polygon.new(ring) -> Polygon. One arg (Vector<Point>).
            // Wraps `buff_geo::Polygon::new(ring).unwrap_or_default()`
            // (panic-free on degenerate input — Polygon impls Default).
            (T::Polygon, A::New) => {
                let arg = one_arg(self)?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    buff_geo::Polygon::new(#arg).unwrap_or_default()
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Polygon.new codegen parse: {e}")))
            }
            // T45: Polygon.from_coords(flat) -> Polygon. One arg
            // (Vector<Float>). Wraps
            // `buff_geo::Polygon::from_coords(coords).unwrap_or_default()`
            // (panic-free on empty / odd-length / degenerate input).
            (T::Polygon, A::FromCoords) => {
                let arg = one_arg(self)?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    buff_geo::Polygon::from_coords(#arg).unwrap_or_default()
                };
                syn::parse2(tokens).map_err(|e| {
                    self.unsupported(&format!("Polygon.from_coords codegen parse: {e}"))
                })
            }
            // T54: buff-simd constructors. Each lowers to the matching
            // `buff_simd::Simd::*` associated function. `Simd.splat(x)`
            // and `Simd.from_array(arr)` are infallible (wrap the
            // underlying `wide::f32x4` ctors directly). `Simd.from_slice
            // (slice)` is fallible in Rust (returns
            // `Result<_, SimdError>`) but surfaces as infallible on the
            // Buff side via `.unwrap_or_default()` (Simd impls Default
            // as `splat(0.0)`). Records `buff-simd` + `wide` in
            // extern_crates via the `program_uses_namespace("Simd")`
            // walker.
            //
            // `Simd.splat(x)` -> Simd. One arg (Float). Wraps
            // `buff_simd::Simd::splat(x)`.
            (T::Simd, A::Splat) => {
                if args.len() != 1 {
                    return Err(self.unsupported(&format!(
                        "Simd.splat expects exactly 1 arg (x: Float), got {}",
                        args.len()
                    )));
                }
                let x = self.lower_expr(&args[0])?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    buff_simd::Simd::splat(#x as f32)
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Simd.splat codegen parse: {e}")))
            }
            // `Simd.from_slice(slice)` -> Simd. One arg (Vector<Float>).
            // Wraps
            // `buff_simd::Simd::from_slice(&slice).unwrap_or_default()`
            // (panic-free on too-short / non-finite input).
            (T::Simd, A::FromSlice) => {
                let arg = one_arg(self)?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    buff_simd::Simd::from_slice(&#arg).unwrap_or_default()
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Simd.from_slice codegen parse: {e}")))
            }
            // `Simd.from_array(arr)` -> Simd. One arg (Vector<Float>).
            // Wraps `buff_simd::Simd::from_array(...)` via the slice
            // path (the wrapper accepts a slice; codegen passes the
            // array as a slice reference). Infallible via
            // `.unwrap_or_default()`.
            (T::Simd, A::FromArray) => {
                let arg = one_arg(self)?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    buff_simd::Simd::from_slice(&#arg).unwrap_or_default()
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Simd.from_array codegen parse: {e}")))
            }
            // T46: Text.detect_language(text) -> Option<Language>. One
            // arg (String). Wraps `buff_nlp::Text::detect_language(&text)`
            // directly — the wrapper already returns Option<Language>
            // (None on empty input / detection failure). NEVER panics
            // (the wrapper uses catch_unwind per FFI guide R6). Records
            // `buff-nlp` + `whatlang` + `rust-stemmers` +
            // `unicode-segmentation` in extern_crates via the
            // `program_uses_namespace("Text")` walker.
            (T::Text, A::DetectLanguage) => {
                let arg = one_arg(self)?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    buff_nlp::Text::detect_language(&#arg)
                };
                syn::parse2(tokens).map_err(|e| {
                    self.unsupported(&format!("Text.detect_language codegen parse: {e}"))
                })
            }
            // T46: Text.stem(word, algorithm) -> String. Two args
            // (String word, String algorithm — lowercase Snowball name
            // like "english" / "portuguese"). Wraps
            // `buff_nlp::Text::stem(&word,
            // buff_nlp::StemAlgorithm::from_code(&algorithm)
            // .unwrap_or(buff_nlp::StemAlgorithm::English))?` — the
            // `?` propagates `NlpError` per Buff's R3 error-mapping
            // contract; unknown algorithm names fall back to English
            // (defensive, never silently corrupts). The wrapper uses
            // catch_unwind per FFI guide R6.
            (T::Text, A::Stem) => {
                if args.len() != 2 {
                    return Err(self.unsupported(&format!(
                        "Text.stem expects exactly 2 args (word, algorithm), got {}",
                        args.len()
                    )));
                }
                let word = self.lower_expr(&args[0])?;
                let algorithm = self.lower_expr(&args[1])?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    buff_nlp::Text::stem(
                        &#word,
                        buff_nlp::StemAlgorithm::from_code(&#algorithm)
                            .unwrap_or(buff_nlp::StemAlgorithm::English),
                    )?
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Text.stem codegen parse: {e}")))
            }
            // T46: Text.tokenize(text) -> Vector<String>. One arg
            // (String). Wraps `buff_nlp::Text::tokenize(&text)` —
            // pure iterator over UAX #29 word boundaries (no panic
            // vectors; catch_unwind omitted per the lib.rs note).
            (T::Text, A::Tokenize) => {
                let arg = one_arg(self)?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    buff_nlp::Text::tokenize(&#arg)
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Text.tokenize codegen parse: {e}")))
            }
            // T46: Text.sentences(text) -> Vector<String>. One arg
            // (String). Wraps `buff_nlp::Text::sentences(&text)` —
            // pure iterator over UAX #29 sentence boundaries.
            (T::Text, A::Sentences) => {
                let arg = one_arg(self)?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    buff_nlp::Text::sentences(&#arg)
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Text.sentences codegen parse: {e}")))
            }
            // T48: Provider.new(rpc_url) -> Provider. One arg (String).
            // Wraps `buff_web3::Provider::new(&url).unwrap_or_default()`
            // (panic-free — Provider impls Default as a localhost-pointed
            // no-op provider; the codegen-lowered `.unwrap_or_default()`
            // collapses `Web3Error::InvalidUrl` / `Web3Error::Panic` to
            // the default Provider per Buff's "no panicking generated
            // code" rule). Records `buff-web3` + `ethers` + `tokio` +
            // `reqwest` + `serde_json` + `hex` in extern_crates via the
            // `program_uses_namespace("Provider")` walker.
            (T::Provider, A::New) => {
                let arg = one_arg(self)?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    buff_web3::Provider::new(&#arg).unwrap_or_default()
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Provider.new codegen parse: {e}")))
            }
            // T48: Wallet.from_private_key(key) -> Wallet. One arg
            // (String — accepts `0x`-prefixed or bare 64-char hex).
            // Wraps `buff_web3::Wallet::from_private_key(&key)
            // .unwrap_or_default()` (panic-free — Wallet impls Default
            // as a "burner" wallet derived from a fixed test key,
            // NEVER use on mainnet; the codegen-lowered
            // `.unwrap_or_default()` collapses
            // `Web3Error::InvalidPrivateKey` / `Web3Error::Panic` to
            // the default Wallet). Records `buff-web3` + `ethers` +
            // `tokio` + `reqwest` + `serde_json` + `hex` in
            // extern_crates via the `program_uses_namespace("Wallet")`
            // walker.
            (T::Wallet, A::FromPrivateKey) => {
                let arg = one_arg(self)?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    buff_web3::Wallet::from_private_key(&#arg).unwrap_or_default()
                };
                syn::parse2(tokens).map_err(|e| {
                    self.unsupported(&format!("Wallet.from_private_key codegen parse: {e}"))
                })
            }
            // T48: Contract.new(address, abi_json, client) -> Contract.
            // Three args (String address, String abi JSON,
            // Provider|ConnectedWallet client). Wraps
            // `buff_web3::Contract::new(&addr, &abi, #client)
            // .unwrap_or_default()` (panic-free — Contract impls
            // Default as a zero-address + empty-ABI + read-only
            // contract; the codegen-lowered `.unwrap_or_default()`
            // collapses `Web3Error::InvalidAddress` /
            // `Web3Error::InvalidAbi` / `Web3Error::Panic` to the
            // default Contract). The `client` arg is spliced directly
            // — the buff_web3 `IntoClient` trait accepts both Provider
            // (read-only) and ConnectedWallet (signing). Records
            // `buff-web3` + `ethers` + `tokio` + `reqwest` +
            // `serde_json` + `hex` in extern_crates via the
            // `program_uses_namespace("Contract")` walker.
            (T::Contract, A::New) => {
                if args.len() != 3 {
                    return Err(self.unsupported(&format!(
                        "Contract.new expects exactly 3 args (address, abi_json, client), got {}",
                        args.len()
                    )));
                }
                let address = self.lower_expr(&args[0])?;
                let abi = self.lower_expr(&args[1])?;
                let client = self.lower_expr(&args[2])?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    buff_web3::Contract::new(&#address, &#abi, #client).unwrap_or_default()
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Contract.new codegen parse: {e}")))
            }
            // T49: AES.generate_key() -> Vector<Byte>. Zero args.
            // Wraps `buff_crypto_extras::aes_gcm_api::generate_key()`
            // (infallible — returns Vec<u8> directly via
            // `Aes256Gcm::generate_key(&mut OsRng)`; OsRng::fill_bytes
            // is infallible on all platforms Buff supports). Records
            // `buff-crypto-extras` + 8 RustCrypto crates in
            // extern_crates via the
            // `program_uses_namespace("AES")` walker.
            (T::AES, A::GenerateKey) => {
                no_args(self)?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    buff_crypto_extras::aes_gcm_api::generate_key()
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("AES.generate_key codegen parse: {e}")))
            }
            // T49: AES.generate_nonce() -> Vector<Byte>. Zero args.
            // Wraps `buff_crypto_extras::aes_gcm_api::generate_nonce()`
            // (infallible — returns the 12-byte GCM nonce via
            // `Aes256Gcm::generate_nonce(&mut OsRng)`).
            (T::AES, A::GenerateNonce) => {
                no_args(self)?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    buff_crypto_extras::aes_gcm_api::generate_nonce()
                };
                syn::parse2(tokens).map_err(|e| {
                    self.unsupported(&format!("AES.generate_nonce codegen parse: {e}"))
                })
            }
            // T49: AES.encrypt(key, nonce, plaintext) -> Vector<Byte>.
            // Three args. Wraps `buff_crypto_extras::aes_gcm_api::
            // encrypt(&key, &nonce, &plaintext).unwrap_or_default()`
            // (empty Vec on any failure — wrong key/nonce length,
            // AES engine error, panic — NEVER panics, matching
            // Buff's "no panicking generated code" rule). The args
            // are spliced by reference so the underlying `&[u8]`
            // bounds are satisfied for both Vec<u8> and slices.
            (T::AES, A::Encrypt) => {
                let mut lowered = n_args(self, 3)?;
                let key = lowered.remove(0);
                let nonce = lowered.remove(0);
                let plaintext = lowered.remove(0);
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    buff_crypto_extras::aes_gcm_api::encrypt(#key.as_slice(), #nonce.as_slice(), #plaintext.as_slice()).unwrap_or_default()
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("AES.encrypt codegen parse: {e}")))
            }
            // T49: AES.decrypt(key, nonce, ciphertext) -> Vector<Byte>.
            // Three args. Wraps `buff_crypto_extras::aes_gcm_api::
            // decrypt(&key, &nonce, &ciphertext).unwrap_or_default()`
            // (empty Vec on auth-tag mismatch / wrong key / wrong
            // nonce length / panic — NEVER panics). Same shape as
            // AES.encrypt.
            (T::AES, A::Decrypt) => {
                let mut lowered = n_args(self, 3)?;
                let key = lowered.remove(0);
                let nonce = lowered.remove(0);
                let ciphertext = lowered.remove(0);
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    buff_crypto_extras::aes_gcm_api::decrypt(#key.as_slice(), #nonce.as_slice(), #ciphertext.as_slice()).unwrap_or_default()
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("AES.decrypt codegen parse: {e}")))
            }
            // T49: RSA.generate_keypair(bits) -> RsaKeypair. One arg
            // (Int). Wraps `buff_crypto_extras::rsa_api::generate_keypair
            // (bits as usize).unwrap_or_default()` (panic-free — the
            // wrapper crate's RsaKeypair impls Default as the
            // empty-PEM-string fallback; the codegen-lowered
            // `.unwrap_or_default()` collapses CryptoError::
            // InvalidLength / Panic to the default RsaKeypair per
            // Buff's "no panicking generated code" rule). The `as
            // usize` lifts Buff's Int<64> to the usize Rust expects.
            // Computationally expensive (~100ms for 2048-bit, ~1s
            // for 4096-bit).
            (T::RSA, A::GenerateKeypair) => {
                let arg = one_arg(self)?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    buff_crypto_extras::rsa_api::generate_keypair(#arg as usize).unwrap_or_default()
                };
                syn::parse2(tokens).map_err(|e| {
                    self.unsupported(&format!("RSA.generate_keypair codegen parse: {e}"))
                })
            }
            // T49: RSA.sign(private_pem, data) -> Vector<Byte>. Two
            // args (String, Vector<Byte>). Wraps
            // `buff_crypto_extras::rsa_api::sign(&private_pem,
            // data.as_slice()).unwrap_or_default()` (empty Vec on
            // malformed PEM / sign engine failure / panic — NEVER
            // panics; the empty-Vec fallback is the correct
            // user-facing behavior since RSA.verify will return
            // false for any non-matching signature, including an
            // empty one).
            (T::RSA, A::Sign) => {
                let (private_pem, data) = two_args(self)?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    buff_crypto_extras::rsa_api::sign(&#private_pem, #data.as_slice()).unwrap_or_default()
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("RSA.sign codegen parse: {e}")))
            }
            // T49: RSA.verify(public_pem, data, signature) -> Bool.
            // Three args. Wraps `buff_crypto_extras::rsa_api::verify(
            // &public_pem, data.as_slice(), signature.as_slice())`
            // (the wrapper already returns `bool` — false on any
            // failure: signature mismatch, malformed PEM, invalid
            // signature bytes, or panic — mirrors T26 Signature.
            // verify + T34 Password.verify stance so a future
            // verify_allow policy can layer cleanly). NO
            // `.unwrap_or_default()` needed (the wrapper collapses
            // all failures to `false` itself).
            (T::RSA, A::Verify) => {
                let mut lowered = n_args(self, 3)?;
                let public_pem = lowered.remove(0);
                let data = lowered.remove(0);
                let signature = lowered.remove(0);
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    buff_crypto_extras::rsa_api::verify(&#public_pem, #data.as_slice(), #signature.as_slice())
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("RSA.verify codegen parse: {e}")))
            }
            // T49: ECDH.generate_private() -> Vector<Byte>. Zero
            // args. Wraps `buff_crypto_extras::ecdh_api::
            // p256_generate_private()` (infallible — returns the
            // 32-byte P-256 scalar via `P256Secret::random(&mut
            // OsRng)`).
            (T::ECDH, A::GeneratePrivate) => {
                no_args(self)?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    buff_crypto_extras::ecdh_api::p256_generate_private()
                };
                syn::parse2(tokens).map_err(|e| {
                    self.unsupported(&format!("ECDH.generate_private codegen parse: {e}"))
                })
            }
            // T49: ECDH.public_from_private(private) -> Vector<Byte>.
            // One arg (Vector<Byte>). Wraps
            // `buff_crypto_extras::ecdh_api::p256_public_from_private
            // (private.as_slice()).unwrap_or_default()` (empty Vec
            // on wrong length / invalid scalar / panic — NEVER
            // panics).
            (T::ECDH, A::PublicFromPrivate) => {
                let arg = one_arg(self)?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    buff_crypto_extras::ecdh_api::p256_public_from_private(#arg.as_slice()).unwrap_or_default()
                };
                syn::parse2(tokens).map_err(|e| {
                    self.unsupported(&format!("ECDH.public_from_private codegen parse: {e}"))
                })
            }
            // T49: ECDH.derive_shared(private, public) ->
            // Vector<Byte>. Two args. Wraps
            // `buff_crypto_extras::ecdh_api::p256_derive_shared(
            // private.as_slice(), public.as_slice())
            // .unwrap_or_default()` (empty Vec on wrong length /
            // invalid point / cofactor edge case / panic — NEVER
            // panics).
            (T::ECDH, A::DeriveShared) => {
                let (private, public) = two_args(self)?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    buff_crypto_extras::ecdh_api::p256_derive_shared(#private.as_slice(), #public.as_slice()).unwrap_or_default()
                };
                syn::parse2(tokens).map_err(|e| {
                    self.unsupported(&format!("ECDH.derive_shared codegen parse: {e}"))
                })
            }
            // T49: Argon2.generate_salt() -> Vector<Byte>. Zero
            // args. Wraps `buff_crypto_extras::argon2_api::
            // generate_salt()` (infallible — fills a 16-byte Vec
            // via `rand::rng().fill_bytes`).
            (T::Argon2, A::GenerateSalt) => {
                no_args(self)?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    buff_crypto_extras::argon2_api::generate_salt()
                };
                syn::parse2(tokens).map_err(|e| {
                    self.unsupported(&format!("Argon2.generate_salt codegen parse: {e}"))
                })
            }
            // T49: Argon2.derive_key(password, salt) -> Vector<Byte>.
            // Two args (String, Vector<Byte>). Wraps
            // `buff_crypto_extras::argon2_api::derive_key(&password,
            // salt.as_slice()).unwrap_or_default()` (empty Vec on
            // wrong salt length / Argon2 engine failure / panic —
            // NEVER panics).
            (T::Argon2, A::DeriveKey) => {
                let (password, salt) = two_args(self)?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    buff_crypto_extras::argon2_api::derive_key(&#password, #salt.as_slice()).unwrap_or_default()
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Argon2.derive_key codegen parse: {e}")))
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
    fn lower_prelude_type_assoc_const(
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
    fn lower_prelude_type_instance_fn(
        &mut self,
        recv_ty: &Type,
        pmethod: buff_lang_types::PreludeInstanceFn,
        receiver: &Expr,
        args: &[Expr],
    ) -> Result<SynExpr, CodegenError> {
        use buff_lang_types::PreludeInstanceFn as M;
        let recv = self.lower_expr(receiver)?;
        // Defensive `one_arg` closure for instance methods that take a
        // single positional arg (added by T44 buff-i18n backfill —
        // also unblocks T42/T43 sibling code that already used the
        // name without defining it locally). Mirrors the
        // `lower_prelude_type_assoc_fn::one_arg` closure shape.
        let pmethod_name = pmethod.name();
        let one_arg = |c: &mut Self| -> Result<SynExpr, CodegenError> {
            if args.len() != 1 {
                return Err(c.unsupported(&format!(
                    "{}() expects exactly 1 arg, got {}",
                    pmethod_name,
                    args.len()
                )));
            }
            c.lower_expr(&args[0])
        };
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
            // T124d: Regex instance methods.
            //
            // `regex.match(text)` -> Option<String>. Wraps the bool
            // result of `regex::Regex::is_match` into an Option that
            // carries the original text on match (so it composes with
            // Buff's Option-handling surface identically to `find`).
            M::Match => {
                if args.len() != 1 {
                    return Err(self.unsupported(&format!(
                        "match() expects exactly 1 arg (the text to search), got {}",
                        args.len()
                    )));
                }
                let text = self.lower_expr(&args[0])?;
                let text_ref = coerce_str_arg_to_ref(text.clone(), &args[0]);
                // if recv.is_match(text) { Some(text.to_string()) } else { None }
                // Built via quote! so the if/else shape is a real syn::ExprIf.
                let is_match_call = method_call_one_arg(recv, "is_match", text_ref);
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    if #is_match_call {
                        Some(#text.to_string())
                    } else {
                        None
                    }
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Regex.match codegen parse: {e}")))
            }
            // `regex.find(text)` -> Option<String>. Lowers to
            // `recv.find(text).map(|m| m.as_str().to_string())`.
            //
            // T50: the `!matches!(recv_ty, Type::Xml)` guard excludes
            // XmlDocument — its `.find()` returns `Result<&XmlElement,
            // XmlError>` (not `Option<regex::Match>`), so the
            // `.map(|m| m.as_str().to_string())` shape is wrong for
            // it. The Xml-specific arm (`M::Find if matches!(recv_ty,
            // Type::Xml)` below) handles XmlDocument via
            // `XmlDocument::find(&recv, &arg).ok().cloned()`. Without
            // this guard the Xml arm would be shadowed (unreachable).
            M::Find if !matches!(recv_ty, Type::Xml) => {
                if args.len() != 1 {
                    return Err(self.unsupported(&format!(
                        "find() expects exactly 1 arg (the text to search), got {}",
                        args.len()
                    )));
                }
                let text = self.lower_expr(&args[0])?;
                let text_ref = coerce_str_arg_to_ref(text, &args[0]);
                let find_call = method_call_one_arg(recv, "find", text_ref);
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    #find_call.map(|m| m.as_str().to_string())
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Regex.find codegen parse: {e}")))
            }
            // `regex.replace(text, repl)` -> String. Lowers to
            // `recv.replace_all(text, repl).to_string()`.
            // `replace_all` (not `replace`) gives the "replace ALL
            // matches" semantics the task spec requires:
            // `regex.replace("a1b2","\\d","X") == "aXbX"`.
            M::Replace => {
                if args.len() != 2 {
                    return Err(self.unsupported(&format!(
                        "replace() expects exactly 2 args (text, replacement), got {}",
                        args.len()
                    )));
                }
                let text = self.lower_expr(&args[0])?;
                let repl = self.lower_expr(&args[1])?;
                let text_ref = coerce_str_arg_to_ref(text, &args[0]);
                let repl_ref = coerce_str_arg_to_ref(repl, &args[1]);
                // recv.replace_all(text, repl).to_string()
                let mut call_args: Punctuated<SynExpr, syn::Token![,]> = Punctuated::new();
                call_args.push(text_ref);
                call_args.push(repl_ref);
                let replace_call = SynExpr::MethodCall(syn::ExprMethodCall {
                    attrs: Vec::new(),
                    receiver: Box::new(recv),
                    dot_token: Default::default(),
                    method: Ident::new("replace_all", ProcSpan::call_site()),
                    turbofish: None,
                    paren_token: Default::default(),
                    args: call_args,
                });
                Ok(method_call_no_args(replace_call, "to_string"))
            }
            // `regex.captures(text)` -> Map<String, String>. Lowers to a
            // block expression that:
            //   1. Calls `recv.captures(text)` (returns Option<Captures>).
            //   2. Builds a `std::collections::HashMap<String, String>`.
            //   3. Iterates `caps.iter()` in INDEX order (numbered
            //      groups: "0" = full match, "1" = first group, ...).
            //   4. Iterates `recv.capture_names().flatten()` for NAMED
            //      groups (source-declaration order).
            //   5. Returns the populated map (or empty map on no match).
            //
            // DETERMINISTIC codegen: the generated Rust source is the
            // same for every Buff source with the same shape (the
            // closure + iteration structure is fixed; only the receiver
            // and text args vary). Runtime iteration order of the
            // resulting HashMap is NOT deterministic — but that's a
            // Rust HashMap property, not a codegen concern (lookups by
            // key still work regardless of iteration order). The
            // group-index iteration order IS deterministic at runtime,
            // so populating from numbered-first then named preserves
            // source-declaration order if the user dumps the map.
            M::Captures => {
                if args.len() != 1 {
                    return Err(self.unsupported(&format!(
                        "captures() expects exactly 1 arg (the text to search), got {}",
                        args.len()
                    )));
                }
                let text = self.lower_expr(&args[0])?;
                let text_ref = coerce_str_arg_to_ref(text.clone(), &args[0]);
                // Bind the captures result so we can iterate it twice
                // (once for numbered, once for named). Use an explicit
                // `let __buff_caps = recv.captures(text);` to avoid
                // re-evaluating the receiver (which may have side effects).
                let caps_call = method_call_one_arg(recv.clone(), "captures", text_ref);
                // `recv.capture_names()` is needed for named-group
                // iteration. We call it on the receiver, NOT on a
                // borrow — `capture_names` takes `&self` so this works.
                let capture_names_call = method_call_no_args(recv, "capture_names");
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    {
                        let __buff_caps = #caps_call;
                        let mut __buff_map: std::collections::HashMap<String, String> =
                            std::collections::HashMap::new();
                        if let Some(__buff_c) = __buff_caps {
                            for (__buff_i, __buff_opt) in __buff_c.iter().enumerate() {
                                if let Some(__buff_m) = __buff_opt {
                                    __buff_map.insert(
                                        __buff_i.to_string(),
                                        __buff_m.as_str().to_string(),
                                    );
                                }
                            }
                            for __buff_name in #capture_names_call.flatten() {
                                if let Some(__buff_m) = __buff_c.name(__buff_name) {
                                    __buff_map.insert(
                                        __buff_name.to_string(),
                                        __buff_m.as_str().to_string(),
                                    );
                                }
                            }
                        }
                        __buff_map
                    }
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Regex.captures codegen parse: {e}")))
            }
            // T124h: URL instance accessors.
            //
            // `url.scheme` -> String. Wraps `url::Url::scheme().to_string()`.
            // The `.to_string()` lifts `&str` to `String` (Buff hides
            // references from users). Zero args.
            M::Scheme => {
                if !args.is_empty() {
                    return Err(
                        self.unsupported(&format!("scheme takes no arguments, got {}", args.len()))
                    );
                }
                let scheme_call = method_call_no_args(recv, "scheme");
                Ok(method_call_no_args(scheme_call, "to_string"))
            }
            // `url.host` -> String (empty when the URL has no host - NEVER
            // panics). Wraps
            // `url::Url::host_str().unwrap_or_default().to_string()`.
            // `host_str()` returns `Option<&str>` (None when the URL has
            // no host - e.g. `mailto:` URLs); `.unwrap_or_default()`
            // yields `&str` (the `""` when None); `.to_string()` lifts to
            // owned `String`.
            M::Host => {
                if !args.is_empty() {
                    return Err(
                        self.unsupported(&format!("host takes no arguments, got {}", args.len()))
                    );
                }
                let host_call = method_call_no_args(recv, "host_str");
                let default = method_call_no_args(host_call, "unwrap_or_default");
                Ok(method_call_no_args(default, "to_string"))
            }
            // `url.path` -> String. Wraps `url::Url::path().to_string()`.
            // The `.to_string()` lifts `&str` to `String`.
            M::Path => {
                if !args.is_empty() {
                    return Err(
                        self.unsupported(&format!("path takes no arguments, got {}", args.len()))
                    );
                }
                let path_call = method_call_no_args(recv, "path");
                Ok(method_call_no_args(path_call, "to_string"))
            }
            // `url.query(key)` -> Option<String>. Wraps a block:
            // ```
            // {
            //     let __buff_key = (key).to_string();
            //     recv.query_pairs()
            //         .find(|(k, _)| *k == __buff_key)
            //         .map(|(_, v)| v.into_owned())
            // }
            // ```
            // The `to_string()` on the key normalises both `&str`
            // literals and `String` idents to owned `String` so the
            // closure's `*k == __buff_key` comparison (where `*k:
            // Cow<str>` and `__buff_key: String`) type-checks via
            // `impl PartialEq<String> for Cow<'_, str>`. `.into_owned()`
            // lifts the matched `Cow<str>` value to owned `String`.
            // Returns `None` when the key is absent (find returns None) -
            // NEVER panics.
            //
            // The block-bind is required because `key` may have side
            // effects (function call) or move semantics (variable) that
            // would otherwise be re-evaluated on every closure call.
            // Binding once to `__buff_key` makes the lookup O(1) in
            // key-construction cost.
            M::Query => {
                if args.len() != 1 {
                    return Err(self.unsupported(&format!(
                        "query(key) expects exactly 1 arg (the key), got {}",
                        args.len()
                    )));
                }
                let key = self.lower_expr(&args[0])?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    {
                        let __buff_key = (#key).to_string();
                        #recv.query_pairs()
                            .find(|(k, _)| *k == __buff_key)
                            .map(|(_, v)| v.into_owned())
                    }
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("URL.query codegen parse: {e}")))
            }
            // T124j: Path instance methods. Each lowers to a
            // fully-qualified `std::path::Path` method (Buff hides
            // references from users; the underlying Rust accessors
            // return `Option<&Path>` / `Option<&OsStr>` / `Option<&OsStr>`
            // / `bool`).
            //
            // `path.parent()` -> Option<Path>. Wraps
            // `recv.parent().map(|p| p.to_path_buf())`. The
            // `.to_path_buf()` lifts `&Path` to owned `PathBuf`
            // (Buff surfaces owned values). Zero args. Returns None
            // when the path has no parent (e.g. `/` or a bare
            // filename) - NEVER panics.
            M::Parent => {
                if !args.is_empty() {
                    return Err(self
                        .unsupported(&format!("parent() takes no arguments, got {}", args.len())));
                }
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    #recv.parent().map(|p| p.to_path_buf())
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Path.parent codegen parse: {e}")))
            }
            // `path.extension()` -> Option<String>. Wraps
            // `recv.extension().map(|e| e.to_string())`. The
            // `.to_string()` lifts `&OsStr` to owned `String` (may
            // panic if the OsStr is non-UTF-8 - but std's
            // `OsStr::to_string` (via Display) is lossy-panic-free,
            // it returns the replacement char for non-UTF-8 bytes,
            // matching Buff's "no panicking generated code" rule).
            // Zero args. Returns None when there's no extension.
            M::Extension => {
                if !args.is_empty() {
                    return Err(self.unsupported(&format!(
                        "extension() takes no arguments, got {}",
                        args.len()
                    )));
                }
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    #recv.extension().map(|e| e.to_string())
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Path.extension codegen parse: {e}")))
            }
            // `path.basename()` -> String. Wraps `recv.file_name()
            // .and_then(|n| n.to_str()).unwrap_or_default().to_string()`.
            // The `.and_then(|n| n.to_str())` handles non-UTF-8
            // filenames lossy-ly (returns None - which falls through
            // to the empty String default - rather than panicking).
            // Zero args. Empty String when the path terminates in
            // `..` or `/` (file_name returns None for those).
            M::Basename => {
                if !args.is_empty() {
                    return Err(self.unsupported(&format!(
                        "basename() takes no arguments, got {}",
                        args.len()
                    )));
                }
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    #recv
                        .file_name()
                        .and_then(|n| n.to_str())
                        .unwrap_or_default()
                        .to_string()
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Path.basename codegen parse: {e}")))
            }
            // `path.exists()` -> Bool. Wraps `recv.exists()` (the
            // underlying std method is infallible - returns `false`
            // on permission errors, never panics). Zero args.
            M::Exists => {
                if !args.is_empty() {
                    return Err(self
                        .unsupported(&format!("exists() takes no arguments, got {}", args.len())));
                }
                Ok(method_call_no_args(recv, "exists"))
            }
            // T124l: Process instance methods. Each lowers to a
            // fully-qualified `std::process::Child` method chained
            // through the `Option<Child>` wrapper the codegen adds
            // at spawn time (`Process.spawn` -> `Command::spawn().ok()`).
            // The Option-wrapper layer keeps the calls panic-free
            // even when spawn failed - the Option collapses to a
            // default Int (0) via `.map(...).unwrap_or_default()`.
            //
            // `process.wait() -> Int`. Wraps
            // `recv.map(|mut c| c.wait().map(|s| s.code()
            // .unwrap_or_default()).unwrap_or_default())
            // .unwrap_or_default()` (the outer Option handles the
            // spawn-failed case; the middle Result handles wait()
            // failure; the inner Option handles signal-terminated
            // processes that have no exit code - all collapse to
            // `0` via `unwrap_or_default()`, NEVER panics). Zero
            // args. The `mut c` binding is required because
            // `Child::wait` takes `&mut self`.
            M::Wait => {
                if !args.is_empty() {
                    return Err(
                        self.unsupported(&format!("wait() takes no arguments, got {}", args.len()))
                    );
                }
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    #recv
                        .map(|mut c| {
                            c.wait()
                                .map(|s| s.code().unwrap_or_default())
                                .unwrap_or_default()
                        })
                        .unwrap_or_default()
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Process.wait codegen parse: {e}")))
            }
            // `process.id() -> Int`. Wraps
            // `recv.map(|c| c.id() as i64).unwrap_or_default()`
            // (0 when the spawn failed or the process has already
            // exited and been reaped - NEVER panics). Zero args.
            // The `as i64` cast widens Rust's `u32` pid to Buff's
            // default `Int<64>` width.
            M::Id => {
                if !args.is_empty() {
                    return Err(
                        self.unsupported(&format!("id() takes no arguments, got {}", args.len()))
                    );
                }
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    #recv
                        .map(|c| c.id() as i64)
                        .unwrap_or_default()
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Process.id codegen parse: {e}")))
            }
            // T124m: TCP-Connection / UDP-Socket / WebSocket-
            // WsConnection instance methods. Each lowers to a
            // fully-qualified tokio / futures-util async method
            // chained through the `Option<...>` wrapper the
            // codegen adds at connect / bind time. The Option-
            // wrapper layer keeps the calls panic-free even when
            // connect / bind failed - the Option's None branch is
            // a no-op (send / close), an empty Vec (recv), or an
            // empty String (ws.recv).
            //
            // All networking instance methods emit `.await` per
            // the tokio / futures-util async API. Buff has NO
            // `await` keyword - the `.await` is purely a codegen
            // concern, snapshot-verified only (single-file `buff
            // run` rustc path does NOT link tokio; the T31 async
            // walker propagates async-ness ONLY through bare-Ident
            // free-fn calls, NOT method-call / namespace-assoc-fn
            // calls, so the enclosing-fn-async transformation is
            // a deferral; see issues.md).
            //
            // Connection.send(data) -> Void. Wraps
            // `{ use tokio::io::AsyncWriteExt; if let Some(mut s)
            // = recv { s.write_all(d.as_bytes()).await.ok(); } }`
            // (block-scoped trait import; `.ok()` discards the
            // write result; Option None branch is a no-op - NEVER
            // panics). One arg (String).
            M::Send if matches!(recv_ty, Type::Connection) => {
                if args.len() != 1 {
                    return Err(self.unsupported(&format!(
                        "send() expects exactly 1 arg (the data String), got {}",
                        args.len()
                    )));
                }
                let data = self.lower_expr(&args[0])?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    {
                        use tokio::io::AsyncWriteExt;
                        if let Some(mut s) = #recv {
                            s.write_all(#data.as_bytes()).await.ok();
                        }
                    }
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Connection.send codegen parse: {e}")))
            }
            // Connection.recv() -> Vector<Byte>. Wraps
            // `{ use tokio::io::AsyncReadExt; let mut buf =
            // Vec::new(); if let Some(mut s) = recv { let _ =
            // s.read(&mut buf).await; } buf }` (returns empty Vec
            // on EOF / error / connect-failed - NEVER panics).
            // Zero args. Returns Vec<u8>.
            M::Recv if matches!(recv_ty, Type::Connection) => {
                if !args.is_empty() {
                    return Err(
                        self.unsupported(&format!("recv() takes no arguments, got {}", args.len()))
                    );
                }
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    {
                        use tokio::io::AsyncReadExt;
                        let mut buf: Vec<u8> = Vec::new();
                        if let Some(mut s) = #recv {
                            let _ = s.read(&mut buf).await;
                        }
                        buf
                    }
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Connection.recv codegen parse: {e}")))
            }
            // Connection.close() -> Void. Wraps
            // `{ use tokio::io::AsyncWriteExt; if let Some(mut s)
            // = recv { s.shutdown().await.ok(); } }` (graceful
            // shutdown of the write side; Option None branch is a
            // no-op - NEVER panics). Zero args. Same `Close`
            // variant dispatched on WsConnection (different
            // lowering - SinkExt::close).
            M::Close if matches!(recv_ty, Type::Connection) => {
                if !args.is_empty() {
                    return Err(self
                        .unsupported(&format!("close() takes no arguments, got {}", args.len())));
                }
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    {
                        use tokio::io::AsyncWriteExt;
                        if let Some(mut s) = #recv {
                            s.shutdown().await.ok();
                        }
                    }
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Connection.close codegen parse: {e}")))
            }
            // Socket.send_to(data, addr) -> Void. Wraps
            // `{ if let Some(s) = recv { s.send_to(d.as_bytes(),
            // a).await.ok(); } }` (Option None branch is a no-op -
            // NEVER panics). Two args (String data, String addr).
            M::SendTo => {
                if args.len() != 2 {
                    return Err(self.unsupported(&format!(
                        "send_to() expects exactly 2 args (data, addr), got {}",
                        args.len()
                    )));
                }
                let data = self.lower_expr(&args[0])?;
                let addr = self.lower_expr(&args[1])?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    {
                        if let Some(s) = #recv {
                            s.send_to(#data.as_bytes(), #addr).await.ok();
                        }
                    }
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Socket.send_to codegen parse: {e}")))
            }
            // Socket.recv_from() -> Tuple. Returns
            // `(Vector<Byte>, String)` (datagram bytes + sender
            // addr). Wraps `{ let mut buf = vec![0u8; 65535]; if
            // let Some(s) = recv { return s.recv_from(&mut buf)
            // .await.ok().map(|(n, addr)| (buf[..n].to_vec(),
            // addr.to_string())); } (Vec::new(), String::new()) }`
            // (returns empty tuple on connect-failed / recv error
            // - NEVER panics). Zero args. The 65535 buffer size is
            // the max UDP datagram payload.
            M::RecvFrom => {
                if !args.is_empty() {
                    return Err(self.unsupported(&format!(
                        "recv_from() takes no arguments, got {}",
                        args.len()
                    )));
                }
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    {
                        let mut buf = vec![0u8; 65535];
                        if let Some(s) = #recv {
                            return s
                                .recv_from(&mut buf)
                                .await
                                .ok()
                                .map(|(n, addr)| (buf[..n].to_vec(), addr.to_string()));
                        }
                        (Vec::new(), String::new())
                    }
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Socket.recv_from codegen parse: {e}")))
            }
            // WsConnection.send(text) -> Void. Wraps
            // `{ use futures_util::SinkExt; if let Some(mut s) =
            // recv { s.send(tokio_tungstenite::tungstenite::
            // Message::Text(t)).await.ok(); } }` (block-scoped
            // trait import; `.ok()` discards the send result;
            // Option None branch is a no-op - NEVER panics). One
            // arg (String text). Same `Send` variant as
            // Connection.send (TCP); dispatched on the
            // (WsConnection, Send) pair.
            M::Send if matches!(recv_ty, Type::WsConnection) => {
                if args.len() != 1 {
                    return Err(self.unsupported(&format!(
                        "send() expects exactly 1 arg (the text String), got {}",
                        args.len()
                    )));
                }
                let text = self.lower_expr(&args[0])?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    {
                        use futures_util::SinkExt;
                        if let Some(mut s) = #recv {
                            s.send(tokio_tungstenite::tungstenite::Message::Text(#text))
                                .await
                                .ok();
                        }
                    }
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("WsConnection.send codegen parse: {e}")))
            }
            // WsConnection.recv() -> String. Wraps
            // `{ use futures_util::StreamExt; if let Some(mut s)
            // = recv { while let Some(Ok(msg)) = s.next().await {
            // if let tokio_tungstenite::tungstenite::Message::
            // Text(t) = msg { return t; } } } String::new() }`
            // (returns empty String on connect-failed / closed /
            // non-text message - NEVER panics). Zero args. The
            // while loop drains non-text frames (Binary / Ping /
            // Pong) until a Text frame arrives or the stream
            // closes; the explicit `return` exits the block on
            // the first Text frame. Distinct from Connection.recv
            // (TCP) which returns Vector<Byte>.
            M::Recv if matches!(recv_ty, Type::WsConnection) => {
                if !args.is_empty() {
                    return Err(
                        self.unsupported(&format!("recv() takes no arguments, got {}", args.len()))
                    );
                }
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    {
                        use futures_util::StreamExt;
                        if let Some(mut s) = #recv {
                            while let Some(Ok(msg)) = s.next().await {
                                if let tokio_tungstenite::tungstenite::Message::Text(t) = msg {
                                    return t;
                                }
                            }
                        }
                        String::new()
                    }
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("WsConnection.recv codegen parse: {e}")))
            }
            // WsConnection.close() -> Void. Wraps
            // `{ use futures_util::SinkExt; if let Some(mut s) =
            // recv { s.close(None).await.ok(); } }` (sends a Close
            // frame; Option None branch is a no-op - NEVER
            // panics). Zero args. Same `Close` variant as
            // Connection.close (TCP); dispatched on the
            // (WsConnection, Close) pair (different lowering -
            // SinkExt::close vs AsyncWriteExt::shutdown).
            M::Close if matches!(recv_ty, Type::WsConnection) => {
                if !args.is_empty() {
                    return Err(self
                        .unsupported(&format!("close() takes no arguments, got {}", args.len())));
                }
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    {
                        use futures_util::SinkExt;
                        if let Some(mut s) = #recv {
                            s.close(None).await.ok();
                        }
                    }
                };
                syn::parse2(tokens).map_err(|e| {
                    self.unsupported(&format!("WsConnection.close codegen parse: {e}"))
                })
            }
            // T2: Channel-Sender.send(value) -> Void. Wraps
            // `runtime_sender.send(value).await.ok()` (the .ok()
            // discards the Result<(), RuntimeError>; the user-facing
            // surface is Void in MVP - v1.18+ may surface the
            // Result). One arg (the value to send). Auto-await per
            // T31 - the codegen emits .await at the call site.
            M::Send if matches!(recv_ty, Type::Sender) => {
                if args.len() != 1 {
                    return Err(self.unsupported(&format!(
                        "send() expects exactly 1 arg (the value to send), got {}",
                        args.len()
                    )));
                }
                let value = self.lower_expr(&args[0])?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    {
                        #recv.send(#value).await.ok();
                    }
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Sender.send codegen parse: {e}")))
            }
            // T2: Channel-Receiver.recv() -> Option<T>. Wraps
            // `runtime_receiver.recv().await` (returns Option<T>:
            // Some(value) when a value arrives; None when all senders
            // were dropped - the canonical channel-closed semantic).
            // Zero args. Auto-await per T31.
            M::Recv if matches!(recv_ty, Type::Receiver) => {
                if !args.is_empty() {
                    return Err(
                        self.unsupported(&format!("recv() takes no arguments, got {}", args.len()))
                    );
                }
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    #recv.recv().await
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Receiver.recv codegen parse: {e}")))
            }
            // T2: Channel-Receiver.close() -> Void. Wraps
            // `runtime_receiver.close()` (sync - NOT async; returns
            // immediately after marking the receiver closed). Zero
            // args. Idempotent (mirrors tokio mpsc::Receiver::close).
            M::Close if matches!(recv_ty, Type::Receiver) => {
                if !args.is_empty() {
                    return Err(self
                        .unsupported(&format!("close() takes no arguments, got {}", args.len())));
                }
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    {
                        #recv.close();
                    }
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Receiver.close codegen parse: {e}")))
            }
            // T124m: Send / Recv / Close on a non-(Connection /
            // WsConnection / Sender / Receiver / SmtpClient /
            // ContractMethod) receiver type fall through to a clear
            // error (mirrors the unreachable defensive arm in
            // lower_prelude_type_assoc_fn). The registry's
            // instance_fn_lookup already rejected the (type,
            // method) pair before reaching this point; this arm
            // is the safety net for future runtime-value types
            // that might also expose `send` / `recv` / `close`
            // methods. T42 (SmtpClient.send) + T48
            // (ContractMethod.send) have their own dedicated arms
            // LATER in this match — the guard below excludes them
            // from this wildcard so they reach their dedicated
            // arms (without the guard, this wildcard would catch
            // them first as dead code).
            M::Send | M::Recv | M::Close
                if !matches!(
                    recv_ty,
                    Type::SmtpClient | Type::ContractMethod
                ) =>
            {
                Err(self.unsupported(&format!(
                    "{recv_ty}.{:?}() is not a recognised prelude instance method",
                    pmethod
                )))
            }
            // T7: DataFrame instance methods. Each chainable method
            // returns `buff_dataframe::DataFrame` so the user can
            // chain `df.select(cols).filter(pred).head(10)`. The
            // codegen records `buff-dataframe` in extern_crates via
            // the narrow `program_uses_namespace("DataFrame")` walker
            // (matches the buff-image / buff-audio precedent). All
            // DataFrame methods panic-free at the codegen layer via
            // `unwrap_or_default()` (DataFrame impls Default as the
            // empty frame).
            //
            // `df.select(cols)` -> DataFrame. One arg (Vector<String>
            // of column names). The codegen splat-converts the Vec
            // to a `&[&str]` slice via `.iter().map(|s| s.as_str())
            // .collect::<Vec<&str>>()` so the buff_dataframe API
            // takes the slice directly.
            M::Select if matches!(recv_ty, Type::DataFrame) => {
                if args.len() != 1 {
                    return Err(self.unsupported(&format!(
                        "select() expects exactly 1 arg (Vector<String> of column names), got {}",
                        args.len()
                    )));
                }
                let cols = self.lower_expr(&args[0])?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    {
                        let __cols: Vec<String> = #cols;
                        let __slice: Vec<&str> = __cols.iter().map(|s| s.as_str()).collect();
                        #recv.select(&__slice).unwrap_or_default()
                    }
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("DataFrame.select codegen parse: {e}")))
            }
            // `df.filter(predicate)` -> DataFrame. One arg (a lambda
            // `|row| -> Bool`). The codegen passes the user closure
            // directly to `buff_dataframe::DataFrame::filter`, which
            // expects `Fn(&RowView<'_>) -> bool`. The user closure's
            // param is a RowView — Buff's codegen surfaces RowView as
            // an opaque value (the user writes `row.get_int("age")`
            // etc.; the codegen passes &RowView by reference).
            M::Filter if matches!(recv_ty, Type::DataFrame) => {
                if args.len() != 1 {
                    return Err(self.unsupported(&format!(
                        "filter() expects exactly 1 arg (a closure predicate), got {}",
                        args.len()
                    )));
                }
                let pred = self.lower_expr(&args[0])?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    #recv.filter(|__row| (#pred)(__row)).unwrap_or_default()
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("DataFrame.filter codegen parse: {e}")))
            }
            // `df.sort(col)` -> DataFrame. One arg (String column
            // name). Ascending lexicographic sort by the column's
            // cells.
            M::Sort if matches!(recv_ty, Type::DataFrame) => {
                if args.len() != 1 {
                    return Err(self.unsupported(&format!(
                        "sort() expects exactly 1 arg (the column name), got {}",
                        args.len()
                    )));
                }
                let col = self.lower_expr(&args[0])?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    #recv.sort(#col.as_str()).unwrap_or_default()
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("DataFrame.sort codegen parse: {e}")))
            }
            // `df.head(n)` -> DataFrame. One arg (Int). First n rows.
            M::Head if matches!(recv_ty, Type::DataFrame) => {
                if args.len() != 1 {
                    return Err(self.unsupported(&format!(
                        "head() expects exactly 1 arg (the row count), got {}",
                        args.len()
                    )));
                }
                let n = self.lower_expr(&args[0])?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    #recv.head((#n).max(0) as usize)
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("DataFrame.head codegen parse: {e}")))
            }
            // `df.len()` -> Int. Zero args. Row count.
            M::Len if matches!(recv_ty, Type::DataFrame) => {
                if !args.is_empty() {
                    return Err(self
                        .unsupported(&format!("len() takes no arguments, got {}", args.len())));
                }
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    (#recv.len() as i64)
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("DataFrame.len codegen parse: {e}")))
            }
            // `df.join(other, on)` -> DataFrame. Two args (DataFrame
            // other, String on-column). Inner equi-join.
            M::Join if matches!(recv_ty, Type::DataFrame) => {
                if args.len() != 2 {
                    return Err(self.unsupported(&format!(
                        "join() expects exactly 2 args (other DataFrame, on-column), got {}",
                        args.len()
                    )));
                }
                let other = self.lower_expr(&args[0])?;
                let on = self.lower_expr(&args[1])?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    #recv.join(&#other, #on.as_str()).unwrap_or_default()
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("DataFrame.join codegen parse: {e}")))
            }
            // `df.group_by(col)` -> DataFrame. One arg (String column
            // name). Returns a DataFrame (the GroupBy intermediate is
            // collapsed into a DataFrame via `into_inner()` so
            // subsequent `.agg(...)` calls dispatch on the DataFrame
            // receiver — a true GroupBy intermediate type would
            // require a second Type variant + display arm + codegen
            // path; deferred to v1.18+).
            M::GroupBy if matches!(recv_ty, Type::DataFrame) => {
                if args.len() != 1 {
                    return Err(self.unsupported(&format!(
                        "group_by() expects exactly 1 arg (the column name), got {}",
                        args.len()
                    )));
                }
                let col = self.lower_expr(&args[0])?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    #recv.group_by(#col.as_str())
                        .map(|__gb| __gb.into_df())
                        .unwrap_or_default()
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("DataFrame.group_by codegen parse: {e}")))
            }
            // `df.agg(col, op)` -> DataFrame. Two args (String column
            // name, String aggregation op — "sum"/"mean"/"min"/"max"/
            // "count"). Returns a per-group aggregate DataFrame.
            M::Agg if matches!(recv_ty, Type::DataFrame) => {
                if args.len() != 2 {
                    return Err(self.unsupported(&format!(
                        "agg() expects exactly 2 args (column name, op string), got {}",
                        args.len()
                    )));
                }
                let col = self.lower_expr(&args[0])?;
                let op = self.lower_expr(&args[1])?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    {
                        let __col: String = #col;
                        let __op_str: String = #op;
                        let __op = buff_dataframe::AggOp::parse(__op_str.as_str())
                            .unwrap_or(buff_dataframe::AggOp::Count);
                        #recv.agg(__col.as_str(), __op)
                    }
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("DataFrame.agg codegen parse: {e}")))
            }
            // `df.to_table_string()` -> String. Zero args. Fixed-width
            // pretty-printer. Infallible (no `unwrap_or_default()` wrap
            // needed — `to_table_string` returns String directly).
            M::ToTableString if matches!(recv_ty, Type::DataFrame) => {
                if !args.is_empty() {
                    return Err(self.unsupported(&format!(
                        "to_table_string() takes no arguments, got {}",
                        args.len()
                    )));
                }
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    #recv.to_table_string()
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("DataFrame.to_table_string codegen parse: {e}")))
            }
            // Non-DataFrame receiver with a DataFrame-only method
            // (Select/Filter/Sort/Head/GroupBy/Agg/ToTableString) falls
            // through to a clear error (mirrors the Send/Recv/Close
            // safety net). T43 excludes Type::Document / Type::Element
            // from the M::Select catch-all so the buff-scrape arms
            // below fire first (mirrors how the `M::Len` arm excludes
            // Type::Cache).
            M::Select if !matches!(recv_ty, Type::Document | Type::Element)
                => Err(self.unsupported(&format!(
                    "{recv_ty}.select() is not a recognised prelude instance method",
                ))),
            M::Filter | M::Sort | M::Head | M::GroupBy | M::Agg | M::ToTableString => {
                Err(self.unsupported(&format!(
                    "{recv_ty}.{:?}() is not a recognised prelude instance method",
                    pmethod
                )))
            }
            // `Len` is shared between DataFrame.len (above) and
            // future Vector.len / Map.len / Series.len / Cache.len —
            // dispatched on receiver type. Non-DataFrame / non-Cache
            // receivers fall through to the existing method-resolution
            // path. T31 (Cache) arm lives below; the guard on this
            // arm skips Cache so the Cache-specific arm fires first.
            M::Len if !matches!(recv_ty, Type::Cache) => Err(self.unsupported(&format!(
                "{recv_ty}.len() is not a recognised prelude instance method",
            ))),
            // T9: Image instance methods. Each filter returning a new
            // Image (grayscale / resize / crop / blur) lowers to
            // `buff_image::Image::<method>` and is panic-free via
            // `unwrap_or_default()` (Image impls Default as a 1x1
            // transparent pixel — added in the same T9 finish commit
            // as this codegen arm). The codegen records `buff-image`
            // + `image` in extern_crates via the
            // `program_uses_namespace("Image")` walker.
            //
            // `img.width()` -> Int. Zero args. Wraps `recv.width() as
            // i64` (the `as i64` lifts u32 to Buff's Int width).
            M::Width if matches!(recv_ty, Type::Image) => {
                if !args.is_empty() {
                    return Err(self
                        .unsupported(&format!("width() takes no arguments, got {}", args.len())));
                }
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    (#recv.width() as i64)
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Image.width codegen parse: {e}")))
            }
            // `img.height()` -> Int. Zero args. Wraps `recv.height()
            // as i64`.
            M::Height if matches!(recv_ty, Type::Image) => {
                if !args.is_empty() {
                    return Err(self.unsupported(&format!(
                        "height() takes no arguments, got {}",
                        args.len()
                    )));
                }
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    (#recv.height() as i64)
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Image.height codegen parse: {e}")))
            }
            // `img.pixel_format()` -> PixelFormat. Zero args. Wraps
            // `recv.format()` (renamed on the Buff surface to avoid a
            // clash with DateTime.format — distinct variant, distinct
            // semantics). Returns Type::Unknown at the type-checker
            // layer; codegen emits the bare call and Rust infers
            // `buff_image::PixelFormat`.
            M::PixelFormat if matches!(recv_ty, Type::Image) => {
                if !args.is_empty() {
                    return Err(self.unsupported(&format!(
                        "pixel_format() takes no arguments, got {}",
                        args.len()
                    )));
                }
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    #recv.format()
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Image.pixel_format codegen parse: {e}")))
            }
            // `img.get_pixel(x, y)` -> Color. Two args. Bounds-
            // checked; the codegen lowers to `recv.get_pixel(x as u32,
            // y as u32).unwrap_or_default()` (Color impls Default as
            // black — panic-free on out-of-bounds coords).
            M::GetPixel if matches!(recv_ty, Type::Image) => {
                if args.len() != 2 {
                    return Err(self.unsupported(&format!(
                        "get_pixel() expects exactly 2 args (x, y), got {}",
                        args.len()
                    )));
                }
                let x = self.lower_expr(&args[0])?;
                let y = self.lower_expr(&args[1])?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    #recv.get_pixel(#x as u32, #y as u32).unwrap_or_default()
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Image.get_pixel codegen parse: {e}")))
            }
            // `img.set_pixel(x, y, color)` -> Void. Three args. In-
            // place mutation. Panic-free via `unwrap_or_default()` (()
            // impls Default — out-of-bounds coords are a no-op).
            M::SetPixel if matches!(recv_ty, Type::Image) => {
                if args.len() != 3 {
                    return Err(self.unsupported(&format!(
                        "set_pixel() expects exactly 3 args (x, y, color), got {}",
                        args.len()
                    )));
                }
                let x = self.lower_expr(&args[0])?;
                let y = self.lower_expr(&args[1])?;
                let color = self.lower_expr(&args[2])?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    #recv.set_pixel(#x as u32, #y as u32, #color).unwrap_or_default()
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Image.set_pixel codegen parse: {e}")))
            }
            // `img.grayscale()` -> Image. Zero args. Consumes self,
            // returns a new Image. Infallible (no `unwrap_or_default`
            // wrap needed — `grayscale` returns Image directly).
            M::Grayscale if matches!(recv_ty, Type::Image) => {
                if !args.is_empty() {
                    return Err(self.unsupported(&format!(
                        "grayscale() takes no arguments, got {}",
                        args.len()
                    )));
                }
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    #recv.grayscale()
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Image.grayscale codegen parse: {e}")))
            }
            // `img.invert()` -> Void. Zero args. In-place channel
            // inversion. Infallible.
            M::Invert if matches!(recv_ty, Type::Image) => {
                if !args.is_empty() {
                    return Err(self.unsupported(&format!(
                        "invert() takes no arguments, got {}",
                        args.len()
                    )));
                }
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    #recv.invert()
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Image.invert codegen parse: {e}")))
            }
            // `img.resize(w, h)` -> Image. Two args (Int w, Int h).
            // Lanczos3 resize. Panic-free via `unwrap_or_default()`
            // (Image impls Default — invalid dims collapse to 1x1).
            M::Resize if matches!(recv_ty, Type::Image) => {
                if args.len() != 2 {
                    return Err(self.unsupported(&format!(
                        "resize() expects exactly 2 args (w, h), got {}",
                        args.len()
                    )));
                }
                let w = self.lower_expr(&args[0])?;
                let h = self.lower_expr(&args[1])?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    #recv.resize(#w as u32, #h as u32).unwrap_or_default()
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Image.resize codegen parse: {e}")))
            }
            // `img.crop(x, y, w, h)` -> Image. Four args. Bounds-
            // checked. Panic-free via `unwrap_or_default()`.
            M::Crop if matches!(recv_ty, Type::Image) => {
                if args.len() != 4 {
                    return Err(self.unsupported(&format!(
                        "crop() expects exactly 4 args (x, y, w, h), got {}",
                        args.len()
                    )));
                }
                let x = self.lower_expr(&args[0])?;
                let y = self.lower_expr(&args[1])?;
                let w = self.lower_expr(&args[2])?;
                let h = self.lower_expr(&args[3])?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    #recv.crop(#x as u32, #y as u32, #w as u32, #h as u32).unwrap_or_default()
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Image.crop codegen parse: {e}")))
            }
            // `img.blur(sigma)` -> Image. One arg (Float). Gaussian
            // blur. Infallible (returns Image directly).
            M::Blur if matches!(recv_ty, Type::Image) => {
                if args.len() != 1 {
                    return Err(self.unsupported(&format!(
                        "blur() expects exactly 1 arg (sigma), got {}",
                        args.len()
                    )));
                }
                let sigma = self.lower_expr(&args[0])?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    #recv.blur(#sigma as f32)
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Image.blur codegen parse: {e}")))
            }
            // T45: buff-geo instance methods. Each lowers to the
            // matching `buff_geo::{Point, LineString, Polygon}` method.
            // All infallible at the codegen layer (the wrapper crate's
            // methods return f64 / bool directly — no unwrap_or_default
            // needed). Records `buff-geo` + `geo` + `geo-types` in
            // extern_crates via the `program_uses_namespace("Point")` /
            // ("LineString") / ("Polygon") walkers.
            //
            // `point.x()` -> Float. Zero args. Wraps `recv.x()`.
            M::X if matches!(recv_ty, Type::Point) => {
                if !args.is_empty() {
                    return Err(self.unsupported(&format!(
                        "x() takes no arguments, got {}",
                        args.len()
                    )));
                }
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    #recv.x()
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Point.x codegen parse: {e}")))
            }
            // `point.y()` -> Float. Zero args. Wraps `recv.y()`.
            M::Y if matches!(recv_ty, Type::Point) => {
                if !args.is_empty() {
                    return Err(self.unsupported(&format!(
                        "y() takes no arguments, got {}",
                        args.len()
                    )));
                }
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    #recv.y()
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Point.y codegen parse: {e}")))
            }
            // `point.distance_to(other)` -> Float. One arg (Point).
            // Wraps `recv.distance_to(other)`.
            M::DistanceTo if matches!(recv_ty, Type::Point) => {
                if args.len() != 1 {
                    return Err(self.unsupported(&format!(
                        "distance_to() expects exactly 1 arg (other Point), got {}",
                        args.len()
                    )));
                }
                let other = self.lower_expr(&args[0])?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    #recv.distance_to(#other)
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Point.distance_to codegen parse: {e}")))
            }
            // `line_string.length()` -> Float. Zero args. Wraps
            // `recv.length()`.
            M::Length if matches!(recv_ty, Type::LineString) => {
                if !args.is_empty() {
                    return Err(self.unsupported(&format!(
                        "length() takes no arguments, got {}",
                        args.len()
                    )));
                }
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    #recv.length()
                };
                syn::parse2(tokens).map_err(|e| {
                    self.unsupported(&format!("LineString.length codegen parse: {e}"))
                })
            }
            // `polygon.area()` -> Float. Zero args. Wraps `recv.area()`.
            M::Area if matches!(recv_ty, Type::Polygon) => {
                if !args.is_empty() {
                    return Err(self.unsupported(&format!(
                        "area() takes no arguments, got {}",
                        args.len()
                    )));
                }
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    #recv.area()
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Polygon.area codegen parse: {e}")))
            }
            // `polygon.contains(point)` -> Bool. One arg (Point).
            // Wraps `recv.contains(point)`. Shared `Contains` variant —
            // dispatched on (Polygon, Contains) pair (same variant as
            // Cache.contains, different receiver type).
            M::Contains if matches!(recv_ty, Type::Polygon) => {
                if args.len() != 1 {
                    return Err(self.unsupported(&format!(
                        "contains() expects exactly 1 arg (Point), got {}",
                        args.len()
                    )));
                }
                let pt = self.lower_expr(&args[0])?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    #recv.contains(#pt)
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Polygon.contains codegen parse: {e}")))
            }
            // `polygon.intersects(other)` -> Bool. One arg (Polygon).
            // Wraps `recv.intersects(&other)` (panic-free via
            // catch_unwind inside the wrapper per FFI guide R6).
            M::Intersects if matches!(recv_ty, Type::Polygon) => {
                if args.len() != 1 {
                    return Err(self.unsupported(&format!(
                        "intersects() expects exactly 1 arg (other Polygon), got {}",
                        args.len()
                    )));
                }
                let other = self.lower_expr(&args[0])?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    #recv.intersects(&#other)
                };
                syn::parse2(tokens).map_err(|e| {
                    self.unsupported(&format!("Polygon.intersects codegen parse: {e}"))
                })
            }
            // T54: buff-simd instance methods. Each lowers to the
            // matching `buff_simd::Simd` method. The 4 lane-wise binary
            // ops (add/sub/mul/div) each take one Simd arg and return
            // Simd. The 3 horizontal reductions (sum/min/max) take no
            // args and return f32. The extract (to_vec) takes no args
            // and returns Vec<f32>. All infallible at the codegen layer
            // (the wrapper crate's methods return Simd / f32 / Vec<f32>
            // directly — no unwrap_or_default needed). Records
            // `buff-simd` + `wide` in extern_crates via the
            // `program_uses_namespace("Simd")` walker.
            //
            // `simd.add(other)` -> Simd. One arg (Simd). Wraps
            // `recv.add(other)`.
            M::Add if matches!(recv_ty, Type::Simd) => {
                if args.len() != 1 {
                    return Err(self.unsupported(&format!(
                        "add() expects exactly 1 arg (other Simd), got {}",
                        args.len()
                    )));
                }
                let other = self.lower_expr(&args[0])?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    #recv.add(#other)
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Simd.add codegen parse: {e}")))
            }
            // `simd.sub(other)` -> Simd. One arg (Simd). Wraps
            // `recv.sub(other)`.
            M::Sub if matches!(recv_ty, Type::Simd) => {
                if args.len() != 1 {
                    return Err(self.unsupported(&format!(
                        "sub() expects exactly 1 arg (other Simd), got {}",
                        args.len()
                    )));
                }
                let other = self.lower_expr(&args[0])?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    #recv.sub(#other)
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Simd.sub codegen parse: {e}")))
            }
            // `simd.mul(other)` -> Simd. One arg (Simd). Wraps
            // `recv.mul(other)`.
            M::Mul if matches!(recv_ty, Type::Simd) => {
                if args.len() != 1 {
                    return Err(self.unsupported(&format!(
                        "mul() expects exactly 1 arg (other Simd), got {}",
                        args.len()
                    )));
                }
                let other = self.lower_expr(&args[0])?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    #recv.mul(#other)
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Simd.mul codegen parse: {e}")))
            }
            // `simd.div(other)` -> Simd. One arg (Simd). Wraps
            // `recv.div(other)`.
            M::Div if matches!(recv_ty, Type::Simd) => {
                if args.len() != 1 {
                    return Err(self.unsupported(&format!(
                        "div() expects exactly 1 arg (other Simd), got {}",
                        args.len()
                    )));
                }
                let other = self.lower_expr(&args[0])?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    #recv.div(#other)
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Simd.div codegen parse: {e}")))
            }
            // `simd.sum()` -> Float. Zero args. Wraps `recv.sum()`.
            M::Sum if matches!(recv_ty, Type::Simd) => {
                if !args.is_empty() {
                    return Err(self.unsupported(&format!(
                        "sum() takes no arguments, got {}",
                        args.len()
                    )));
                }
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    #recv.sum()
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Simd.sum codegen parse: {e}")))
            }
            // `simd.min()` -> Float. Zero args. Wraps `recv.min()`.
            M::Min if matches!(recv_ty, Type::Simd) => {
                if !args.is_empty() {
                    return Err(self.unsupported(&format!(
                        "min() takes no arguments, got {}",
                        args.len()
                    )));
                }
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    #recv.min()
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Simd.min codegen parse: {e}")))
            }
            // `simd.max()` -> Float. Zero args. Wraps `recv.max()`.
            M::Max if matches!(recv_ty, Type::Simd) => {
                if !args.is_empty() {
                    return Err(self.unsupported(&format!(
                        "max() takes no arguments, got {}",
                        args.len()
                    )));
                }
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    #recv.max()
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Simd.max codegen parse: {e}")))
            }
            // `simd.to_vec()` -> Vector<Float>. Zero args. Wraps
            // `recv.to_vec()`.
            M::ToVec if matches!(recv_ty, Type::Simd) => {
                if !args.is_empty() {
                    return Err(self.unsupported(&format!(
                        "to_vec() takes no arguments, got {}",
                        args.len()
                    )));
                }
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    #recv.to_vec()
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Simd.to_vec codegen parse: {e}")))
            }
            // T46: buff-nlp Language instance methods. Both infallible
            // — the `buff_nlp::Language::code` / `name` methods return
            // owned String directly (cloning the inner `&'static str`
            // per FFI guide R5). Records `buff-nlp` + `whatlang` +
            // `rust-stemmers` + `unicode-segmentation` in extern_crates
            // via the `program_uses_namespace("Text")` walker (the
            // walker fires on Text.* calls; Language values arise only
            // as Text.detect_language return values, so a program that
            // uses lang.code() always also uses Text.* — the walker
            // registers buff-nlp correctly either way).
            //
            // `language.code()` -> String (ISO 639-3). Zero args.
            M::Code if matches!(recv_ty, Type::Language) => {
                if !args.is_empty() {
                    return Err(self.unsupported(&format!(
                        "code() takes no arguments, got {}",
                        args.len()
                    )));
                }
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    #recv.code()
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Language.code codegen parse: {e}")))
            }
            // `language.name()` -> String (English name). Zero args.
            // `Name` is shared with `faker.name()` (Faker arm below) —
            // dispatched on receiver type.
            M::Name if matches!(recv_ty, Type::Language) => {
                if !args.is_empty() {
                    return Err(self.unsupported(&format!(
                        "name() takes no arguments, got {}",
                        args.len()
                    )));
                }
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    #recv.name()
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Language.name codegen parse: {e}")))
            }
            // T52: buff-protobuf Message instance methods. All
            // infallible — `buff_protobuf::Message::byte_size` /
            // `type_url` / `encode` return `usize` / `&str` / `&[u8]`
            // directly (no Result wrapper). `payload` is fallible in
            // Rust (returns `Result<Value, ProtobufError>`) so the
            // codegen wraps with `.unwrap_or_default()` (Value::Null
            // on decode failure — panic-free via
            // `.unwrap_or_default()`, NOT bare `.unwrap()`). Records
            // `buff-protobuf` + `prost` + `prost-types` + `serde_json`
            // in extern_crates via the
            // `program_uses_namespace("Message")` walker.
            //
            // `message.byte_size()` -> Int. Zero args. Wraps
            // `recv.byte_size() as i64` (the underlying Rust method
            // returns `usize`; the cast lifts to Buff's `Int<64>`).
            M::ByteSize if matches!(recv_ty, Type::Message) => {
                if !args.is_empty() {
                    return Err(self.unsupported(&format!(
                        "byte_size() takes no arguments, got {}",
                        args.len()
                    )));
                }
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    (#recv.byte_size() as i64)
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Message.byte_size codegen parse: {e}")))
            }
            // `message.type_url()` -> String. Zero args. Wraps
            // `recv.type_url().to_string()` (the underlying Rust
            // method returns `&str`; the `.to_string()` lifts to
            // owned String per FFI guide R2 — Buff surfaces owned
            // values).
            M::TypeUrl if matches!(recv_ty, Type::Message) => {
                if !args.is_empty() {
                    return Err(self.unsupported(&format!(
                        "type_url() takes no arguments, got {}",
                        args.len()
                    )));
                }
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    #recv.type_url().to_string()
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Message.type_url codegen parse: {e}")))
            }
            // `message.payload()` -> Value. Zero args. Wraps
            // `recv.payload().unwrap_or_default()` (Value::Null on
            // decode failure — panic-free via `.unwrap_or_default()`,
            // NOT bare `.unwrap()`).
            M::Payload if matches!(recv_ty, Type::Message) => {
                if !args.is_empty() {
                    return Err(self.unsupported(&format!(
                        "payload() takes no arguments, got {}",
                        args.len()
                    )));
                }
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    #recv.payload().unwrap_or_default()
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Message.payload codegen parse: {e}")))
            }
            // `message.encode()` -> Vector<Byte>. Zero args. Wraps
            // `recv.encode().to_vec()` (the underlying Rust method
            // returns `&[u8]`; the `.to_vec()` lifts to owned `Vec<u8>`
            // per FFI guide R2 — Buff surfaces owned values). Distinct
            // from `PreludeAssocFn::Encode` (the Base64.encode /
            // Hex.encode *associated-function* shape) — this Encode is
            // an *instance method* on a Message value (different enum,
            // different dispatch table).
            M::Encode if matches!(recv_ty, Type::Message) => {
                if !args.is_empty() {
                    return Err(self.unsupported(&format!(
                        "encode() takes no arguments, got {}",
                        args.len()
                    )));
                }
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    #recv.encode().to_vec()
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Message.encode codegen parse: {e}")))
            }
            // T47: buff-chat Bot instance methods. The Bot wrapper's
            // methods all panic-free via `.unwrap_or(())` (command /
            // on_message / start / stop / dispatch — return Result in
            // Rust but collapse to Void at the Buff surface per FFI
            // guide R3) or infallible directly (is_running /
            // command_count / has_message_handler / platform — return
            // bool / usize / Platform directly). Records `buff-chat` +
            // `serenity` + `teloxide` + `async-trait` + `tokio` in
            // extern_crates via the `program_uses_namespace("Bot")`
            // walker.
            //
            // `bot.command(name, handler)` -> Void. Two args (String
            // name, closure handler). The closure is spliced directly
            // — Rust coerces `|msg| ...` to the `F: Fn(Message) +
            // Send + Sync + 'static` bound on `Bot::command` (no
            // Arc::new / Box::new wrap needed; the wrapper ctor does
            // the Arc-sharing internally). `.unwrap_or(())` collapses
            // ChatError::EmptyCommandName / DuplicateCommand to Void
            // (silently swallowed at the Buff surface).
            M::Command if matches!(recv_ty, Type::Bot) => {
                if args.len() != 2 {
                    return Err(self.unsupported(&format!(
                        "command() expects exactly 2 args (name, handler), got {}",
                        args.len()
                    )));
                }
                let name = self.lower_expr(&args[0])?;
                let handler = self.lower_expr(&args[1])?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    #recv.command(&(#name).to_string(), move |msg| #handler).unwrap_or(())
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Bot.command codegen parse: {e}")))
            }
            // `bot.on_message(handler)` -> Void. One arg (closure
            // handler). Same closure-splice shape as `command` minus
            // the name arg. `.unwrap_or(())` collapses registration
            // failure to Void.
            M::OnMessage if matches!(recv_ty, Type::Bot) => {
                if args.len() != 1 {
                    return Err(self.unsupported(&format!(
                        "on_message() expects exactly 1 arg (handler), got {}",
                        args.len()
                    )));
                }
                let handler = self.lower_expr(&args[0])?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    #recv.on_message(move |msg| #handler).unwrap_or(())
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Bot.on_message codegen parse: {e}")))
            }
            // `bot.start()` -> Void. Zero args. Blocks on the platform
            // event loop. `.unwrap_or(())` collapses ChatError to Void
            // (AlreadyRunning / AlreadyInRuntime / Connect / Runtime).
            M::Start if matches!(recv_ty, Type::Bot) => {
                if !args.is_empty() {
                    return Err(self.unsupported(&format!(
                        "start() takes no arguments, got {}",
                        args.len()
                    )));
                }
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    #recv.start().unwrap_or(())
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Bot.start codegen parse: {e}")))
            }
            // `bot.stop()` -> Void. Zero args. Cooperative shutdown
            // (AtomicBool flag). `.unwrap_or(())` collapses
            // ChatError::NotRunning to Void.
            M::Stop if matches!(recv_ty, Type::Bot) => {
                if !args.is_empty() {
                    return Err(self.unsupported(&format!(
                        "stop() takes no arguments, got {}",
                        args.len()
                    )));
                }
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    #recv.stop().unwrap_or(())
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Bot.stop codegen parse: {e}")))
            }
            // `bot.dispatch(msg)` -> Void. One arg (ChatMessage). The
            // public testing entry — exercises the handler routing
            // without a live network connection (T47 "mock API").
            M::Dispatch if matches!(recv_ty, Type::Bot) => {
                if args.len() != 1 {
                    return Err(self.unsupported(&format!(
                        "dispatch() expects exactly 1 arg (Message), got {}",
                        args.len()
                    )));
                }
                let msg = self.lower_expr(&args[0])?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    #recv.dispatch(#msg).unwrap_or(())
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Bot.dispatch codegen parse: {e}")))
            }
            // `bot.is_running()` -> Bool. Zero args. Infallible (the
            // wrapper returns false on poisoned lock, never panics).
            M::IsRunning if matches!(recv_ty, Type::Bot) => {
                if !args.is_empty() {
                    return Err(self.unsupported(&format!(
                        "is_running() takes no arguments, got {}",
                        args.len()
                    )));
                }
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    #recv.is_running()
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Bot.is_running codegen parse: {e}")))
            }
            // `bot.command_count()` -> Int. Zero args. The underlying
            // Rust method returns `usize`; the `as i64` lifts to
            // Buff's `Int<64>`.
            M::CommandCount if matches!(recv_ty, Type::Bot) => {
                if !args.is_empty() {
                    return Err(self.unsupported(&format!(
                        "command_count() takes no arguments, got {}",
                        args.len()
                    )));
                }
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    #recv.command_count() as i64
                };
                syn::parse2(tokens).map_err(|e| {
                    self.unsupported(&format!("Bot.command_count codegen parse: {e}"))
                })
            }
            // `bot.has_message_handler()` -> Bool. Zero args.
            M::HasMessageHandler if matches!(recv_ty, Type::Bot) => {
                if !args.is_empty() {
                    return Err(self.unsupported(&format!(
                        "has_message_handler() takes no arguments, got {}",
                        args.len()
                    )));
                }
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    #recv.has_message_handler()
                };
                syn::parse2(tokens).map_err(|e| {
                    self.unsupported(&format!("Bot.has_message_handler codegen parse: {e}"))
                })
            }
            // `bot.platform()` -> Platform. Zero args. Infallible (Copy
            // value, never panics). Shared `Platform` variant —
            // dispatched on the (Bot, Platform) pair (same variant as
            // ChatMessage.platform, different receiver type).
            M::Platform if matches!(recv_ty, Type::Bot) => {
                if !args.is_empty() {
                    return Err(self.unsupported(&format!(
                        "platform() takes no arguments, got {}",
                        args.len()
                    )));
                }
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    #recv.platform()
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Bot.platform codegen parse: {e}")))
            }
            // T47: buff-chat ChatMessage instance methods. All
            // infallible — the `buff_chat::Message` methods return
            // `&str` / Platform / bool directly (the codegen wraps
            // &str returns in `.to_string()` so Buff surfaces owned
            // String values per FFI guide R2). Records `buff-chat` +
            // `serenity` + `teloxide` + `async-trait` + `tokio` in
            // extern_crates via the `program_uses_namespace
            // ("ChatMessage")` walker.
            //
            // `msg.text()` -> String. Zero args. Shared `Text`
            // variant — dispatched on the (ChatMessage, Text) pair
            // (same variant as Document / Element / XmlElement.text,
            // different receiver type).
            M::Text if matches!(recv_ty, Type::ChatMessage) => {
                if !args.is_empty() {
                    return Err(self.unsupported(&format!(
                        "text() takes no arguments, got {}",
                        args.len()
                    )));
                }
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    #recv.text().to_string()
                };
                syn::parse2(tokens).map_err(|e| {
                    self.unsupported(&format!("ChatMessage.text codegen parse: {e}"))
                })
            }
            // `msg.channel()` -> String. Zero args.
            M::Channel if matches!(recv_ty, Type::ChatMessage) => {
                if !args.is_empty() {
                    return Err(self.unsupported(&format!(
                        "channel() takes no arguments, got {}",
                        args.len()
                    )));
                }
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    #recv.channel().to_string()
                };
                syn::parse2(tokens).map_err(|e| {
                    self.unsupported(&format!("ChatMessage.channel codegen parse: {e}"))
                })
            }
            // `msg.author()` -> String. Zero args.
            M::Author if matches!(recv_ty, Type::ChatMessage) => {
                if !args.is_empty() {
                    return Err(self.unsupported(&format!(
                        "author() takes no arguments, got {}",
                        args.len()
                    )));
                }
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    #recv.author().to_string()
                };
                syn::parse2(tokens).map_err(|e| {
                    self.unsupported(&format!("ChatMessage.author codegen parse: {e}"))
                })
            }
            // `msg.platform()` -> Platform. Zero args. Shared
            // `Platform` variant — dispatched on the
            // (ChatMessage, Platform) pair (same variant as
            // Bot.platform, different receiver type).
            M::Platform if matches!(recv_ty, Type::ChatMessage) => {
                if !args.is_empty() {
                    return Err(self.unsupported(&format!(
                        "platform() takes no arguments, got {}",
                        args.len()
                    )));
                }
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    #recv.platform()
                };
                syn::parse2(tokens).map_err(|e| {
                    self.unsupported(&format!("ChatMessage.platform codegen parse: {e}"))
                })
            }
            // `msg.is_dm()` -> Bool. Zero args.
            M::IsDm if matches!(recv_ty, Type::ChatMessage) => {
                if !args.is_empty() {
                    return Err(self.unsupported(&format!(
                        "is_dm() takes no arguments, got {}",
                        args.len()
                    )));
                }
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    #recv.is_dm()
                };
                syn::parse2(tokens).map_err(|e| {
                    self.unsupported(&format!("ChatMessage.is_dm codegen parse: {e}"))
                })
            }
            // T47: buff-chat Platform instance methods. Both
            // infallible — the `buff_chat::Platform::is_discord` /
            // `is_telegram` methods return `bool` directly (Copy
            // value). Records `buff-chat` + `serenity` + `teloxide` +
            // `async-trait` + `tokio` in extern_crates via the
            // `program_uses_namespace("Platform")` walker.
            //
            // `platform.is_discord()` -> Bool. Zero args.
            M::IsDiscord if matches!(recv_ty, Type::Platform) => {
                if !args.is_empty() {
                    return Err(self.unsupported(&format!(
                        "is_discord() takes no arguments, got {}",
                        args.len()
                    )));
                }
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    #recv.is_discord()
                };
                syn::parse2(tokens).map_err(|e| {
                    self.unsupported(&format!("Platform.is_discord codegen parse: {e}"))
                })
            }
            // `platform.is_telegram()` -> Bool. Zero args.
            M::IsTelegram if matches!(recv_ty, Type::Platform) => {
                if !args.is_empty() {
                    return Err(self.unsupported(&format!(
                        "is_telegram() takes no arguments, got {}",
                        args.len()
                    )));
                }
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    #recv.is_telegram()
                };
                syn::parse2(tokens).map_err(|e| {
                    self.unsupported(&format!("Platform.is_telegram codegen parse: {e}"))
                })
            }
            // T37: Faker instance methods. All infallible — the
            // `buff_fake::Faker` methods return owned String / i64
            // directly (no unwrap_or_default needed). Records
            // `buff-fake` + `fake` in extern_crates via the
            // `program_uses_namespace("Faker")` walker.
            //
            // `faker.name()` -> String. Zero args.
            M::Name if matches!(recv_ty, Type::Faker) => {
                if !args.is_empty() {
                    return Err(self.unsupported(&format!(
                        "name() takes no arguments, got {}", args.len()
                    )));
                }
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    #recv.name()
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Faker.name codegen parse: {e}")))
            }
            // `faker.email()` -> String. Zero args.
            M::Email if matches!(recv_ty, Type::Faker) => {
                if !args.is_empty() {
                    return Err(self.unsupported(&format!(
                        "email() takes no arguments, got {}", args.len()
                    )));
                }
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    #recv.email()
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Faker.email codegen parse: {e}")))
            }
            // `faker.address()` -> String. Zero args.
            M::Address if matches!(recv_ty, Type::Faker) => {
                if !args.is_empty() {
                    return Err(self.unsupported(&format!(
                        "address() takes no arguments, got {}", args.len()
                    )));
                }
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    #recv.address()
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Faker.address codegen parse: {e}")))
            }
            // `faker.phone()` -> String. Zero args.
            M::Phone if matches!(recv_ty, Type::Faker) => {
                if !args.is_empty() {
                    return Err(self.unsupported(&format!(
                        "phone() takes no arguments, got {}", args.len()
                    )));
                }
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    #recv.phone()
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Faker.phone codegen parse: {e}")))
            }
            // `faker.uuid()` -> String. Zero args.
            M::Uuid if matches!(recv_ty, Type::Faker) => {
                if !args.is_empty() {
                    return Err(self.unsupported(&format!(
                        "uuid() takes no arguments, got {}", args.len()
                    )));
                }
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    #recv.uuid()
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Faker.uuid codegen parse: {e}")))
            }
            // `faker.lorem(words)` -> String. One arg (Int word_count).
            M::Lorem if matches!(recv_ty, Type::Faker) => {
                if args.len() != 1 {
                    return Err(self.unsupported(&format!(
                        "lorem() expects exactly 1 arg (word_count), got {}", args.len()
                    )));
                }
                let word_count = self.lower_expr(&args[0])?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    #recv.lorem(#word_count as usize)
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Faker.lorem codegen parse: {e}")))
            }
            // `faker.int(min, max)` -> Int. Two args (Int min, Int max).
            M::FakerInt if matches!(recv_ty, Type::Faker) => {
                if args.len() != 2 {
                    return Err(self.unsupported(&format!(
                        "int() expects exactly 2 args (min, max), got {}", args.len()
                    )));
                }
                let min = self.lower_expr(&args[0])?;
                let max = self.lower_expr(&args[1])?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    #recv.int(#min, #max)
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Faker.int codegen parse: {e}")))
            }
            // `faker.datetime(start, end)` -> String. Two args (String
            // start, String end). Wraps `recv.datetime(&start, &end)
            // .unwrap_or_default()` (panic-free — empty string on error).
            M::FakerDatetime if matches!(recv_ty, Type::Faker) => {
                if args.len() != 2 {
                    return Err(self.unsupported(&format!(
                        "datetime() expects exactly 2 args (start, end), got {}", args.len()
                    )));
                }
                let start = self.lower_expr(&args[0])?;
                let end = self.lower_expr(&args[1])?;
                let start_ref = coerce_str_arg_to_ref(start, &args[0]);
                let end_ref = coerce_str_arg_to_ref(end, &args[1]);
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    #recv.datetime(#start_ref, #end_ref).unwrap_or_default()
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Faker.datetime codegen parse: {e}")))
            }
            // T31: Cache instance methods. Each method lowers to
            // `buff_cache::Cache::<method>`. The codegen records
            // `buff-cache` + `moka` in extern_crates via the
            // `program_uses_namespace("Cache")` walker.
            //
            // `cache.get(key)` -> String?. One arg (String). Wraps
            // `recv.get(&key)` (returns Option<String> natively).
            M::Get if matches!(recv_ty, Type::Cache) => {
                if args.len() != 1 {
                    return Err(self.unsupported(&format!(
                        "get() expects exactly 1 arg (key), got {}",
                        args.len()
                    )));
                }
                let key = self.lower_expr(&args[0])?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    #recv.get(&#key)
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Cache.get codegen parse: {e}")))
            }
            // `cache.set(key, value)` -> Void. Two args. Wraps
            // `recv.set(key, value)` (the no-TTL overload).
            M::Set if matches!(recv_ty, Type::Cache) => {
                if args.len() != 2 {
                    return Err(self.unsupported(&format!(
                        "set() expects exactly 2 args (key, value), got {}",
                        args.len()
                    )));
                }
                let key = self.lower_expr(&args[0])?;
                let value = self.lower_expr(&args[1])?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    #recv.set(#key, #value)
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Cache.set codegen parse: {e}")))
            }
            // `cache.set(key, value, ttl)` -> Void. Three args. Wraps
            // `recv.set_with_ttl(key, value, ttl)`. The Buff surface
            // uses the same `set` method name (arity-based dispatch
            // via the codegen's arg-count check); the underlying
            // Rust method is `set_with_ttl` to avoid overload
            // ambiguity.
            M::SetTtl if matches!(recv_ty, Type::Cache) => {
                if args.len() != 3 {
                    return Err(self.unsupported(&format!(
                        "set(key, value, ttl) expects exactly 3 args, got {}",
                        args.len()
                    )));
                }
                let key = self.lower_expr(&args[0])?;
                let value = self.lower_expr(&args[1])?;
                let ttl = self.lower_expr(&args[2])?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    #recv.set_with_ttl(#key, #value, #ttl)
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Cache.set(ttl) codegen parse: {e}")))
            }
            // `cache.delete(key)` -> Void. One arg. Wraps
            // `recv.delete(&key)`.
            M::Delete if matches!(recv_ty, Type::Cache) => {
                if args.len() != 1 {
                    return Err(self.unsupported(&format!(
                        "delete() expects exactly 1 arg (key), got {}",
                        args.len()
                    )));
                }
                let key = self.lower_expr(&args[0])?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    #recv.delete(&#key)
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Cache.delete codegen parse: {e}")))
            }
            // `cache.contains(key)` -> Bool. One arg. Wraps
            // `recv.contains(&key)`.
            M::Contains if matches!(recv_ty, Type::Cache) => {
                if args.len() != 1 {
                    return Err(self.unsupported(&format!(
                        "contains() expects exactly 1 arg (key), got {}",
                        args.len()
                    )));
                }
                let key = self.lower_expr(&args[0])?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    #recv.contains(&#key)
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Cache.contains codegen parse: {e}")))
            }
            // `cache.clear()` -> Void. Zero args. Wraps
            // `recv.clear()`.
            M::Clear if matches!(recv_ty, Type::Cache) => {
                if !args.is_empty() {
                    return Err(self.unsupported(&format!(
                        "clear() takes no arguments, got {}",
                        args.len()
                    )));
                }
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    #recv.clear()
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Cache.clear codegen parse: {e}")))
            }
            // `cache.len()` -> Int. Zero args. Wraps `recv.len() as
            // i64` (the `as i64` lifts u64 to Buff's Int width).
            M::Len if matches!(recv_ty, Type::Cache) => {
                if !args.is_empty() {
                    return Err(self.unsupported(&format!(
                        "len() takes no arguments, got {}",
                        args.len()
                    )));
                }
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    (#recv.len() as i64)
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Cache.len codegen parse: {e}")))
            }
            // T44 MVP: I18n instance methods. AddResource / Load are
            // 2-arg / 1-arg Void methods wrapped in `.unwrap_or(())`
            // for panic-free codegen (mirrors the Cache.set / SetTtl
            // stance). Translate is a 1-arg String method. The 7
            // deferred methods (SetFallback / AvailableLocales /
            // CurrentLocale / FallbackLocale / TranslateWithArgs /
            // HasMessage / Warnings) are available on the
            // `buff_i18n::I18n` Rust type but codegen-wiring is
            // deferred to a follow-up.
            M::AddResource if matches!(recv_ty, Type::I18n) => {
                if args.len() != 2 {
                    return Err(self.unsupported(&format!(
                        "add_resource() expects exactly 2 args (locale, ftl), got {}",
                        args.len()
                    )));
                }
                let locale = self.lower_expr(&args[0])?;
                let ftl = self.lower_expr(&args[1])?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    #recv.add_resource(&#locale, &#ftl).unwrap_or(())
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("I18n.add_resource codegen parse: {e}")))
            }
            // `i18n.load(locale)` -> Void. One arg (String). Wraps
            // `recv.load(&locale).unwrap_or(())` (panic-free on
            // LocaleNotLoaded — no-op).
            M::Load if matches!(recv_ty, Type::I18n) => {
                if args.len() != 1 {
                    return Err(self.unsupported(&format!(
                        "load() expects exactly 1 arg (locale), got {}",
                        args.len()
                    )));
                }
                let locale = self.lower_expr(&args[0])?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    #recv.load(&#locale).unwrap_or(())
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("I18n.load codegen parse: {e}")))
            }
            // `i18n.translate(key)` -> String. One arg (String).
            // Wraps `recv.translate(&key)` (current → fallback → key
            // string contract — NEVER panics).
            M::Translate if matches!(recv_ty, Type::I18n) => {
                if args.len() != 1 {
                    return Err(self.unsupported(&format!(
                        "translate() expects exactly 1 arg (key), got {}",
                        args.len()
                    )));
                }
                let key = self.lower_expr(&args[0])?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    #recv.translate(&#key)
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("I18n.translate codegen parse: {e}")))
            }
            // T43: buff-scrape instance methods. Each method lowers to
            // `buff_scrape::{Document, Element, Crawler}::<method>`. The
            // codegen records `buff-scrape` + `scraper` (for Document /
            // Element) or `reqwest` (for Crawler) in extern_crates via
            // the `program_uses_namespace("Document" / "Element" /
            // "Crawler")` walker. All methods are panic-free at the
            // codegen layer (Document/Element/Crawler all impl Default;
            // fallible ops lower to `unwrap_or_default()` or `?`
            // depending on whether the receiver itself is consumed).
            //
            // ---- Document instance methods (4) -----------------
            // `doc.select(css)` -> Vector<Element>. One arg (String).
            // Wraps `buff_scrape::Document::select(&recv, &css)
            // .unwrap_or_default()` (panic-free on invalid CSS — empty
            // Vec returned; mirrors the Image.from_path panic-free
            // pattern).
            M::Select if matches!(recv_ty, Type::Document) => {
                let arg = one_arg(self)?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    buff_scrape::Document::select(&#recv, &#arg).unwrap_or_default()
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Document.select codegen parse: {e}")))
            }
            // `doc.text()` -> String. Zero args. Wraps `recv.text()`.
            M::Text if matches!(recv_ty, Type::Document) => {
                if !args.is_empty() {
                    return Err(self.unsupported(&format!(
                        "text() takes no arguments, got {}",
                        args.len()
                    )));
                }
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    #recv.text()
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Document.text codegen parse: {e}")))
            }
            // `doc.html()` -> String. Zero args. Wraps `recv.html()`.
            // Shared `Html` variant — distinct from (Email, Html)
            // (two-arg template builder).
            M::Html if matches!(recv_ty, Type::Document) => {
                if !args.is_empty() {
                    return Err(self.unsupported(&format!(
                        "html() takes no arguments, got {}",
                        args.len()
                    )));
                }
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    #recv.html()
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Document.html codegen parse: {e}")))
            }
            // `doc.title()` -> String?. Zero args. Wraps `recv.title()`.
            M::Title if matches!(recv_ty, Type::Document) => {
                if !args.is_empty() {
                    return Err(self.unsupported(&format!(
                        "title() takes no arguments, got {}",
                        args.len()
                    )));
                }
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    #recv.title()
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Document.title codegen parse: {e}")))
            }
            // ---- Element instance methods (5) -----------------
            // `el.select(css)` -> Vector<Element>. One arg (String).
            // Wraps `Element::select(&recv, &css).unwrap_or_default()`.
            M::Select if matches!(recv_ty, Type::Element) => {
                let arg = one_arg(self)?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    buff_scrape::Element::select(&#recv, &#arg).unwrap_or_default()
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Element.select codegen parse: {e}")))
            }
            // `el.text()` -> String. Zero args. Wraps `recv.text()`.
            M::Text if matches!(recv_ty, Type::Element) => {
                if !args.is_empty() {
                    return Err(self.unsupported(&format!(
                        "text() takes no arguments, got {}",
                        args.len()
                    )));
                }
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    #recv.text()
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Element.text codegen parse: {e}")))
            }
            // `el.attr(name)` -> String?. One arg (String). Wraps
            // `recv.attr(&name)`.
            M::Attr if matches!(recv_ty, Type::Element) => {
                let arg = one_arg(self)?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    #recv.attr(&#arg)
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Element.attr codegen parse: {e}")))
            }
            // `el.html()` -> String. Zero args. Wraps `recv.html()`.
            M::Html if matches!(recv_ty, Type::Element) => {
                if !args.is_empty() {
                    return Err(self.unsupported(&format!(
                        "html() takes no arguments, got {}",
                        args.len()
                    )));
                }
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    #recv.html()
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Element.html codegen parse: {e}")))
            }
            // `el.inner_html()` -> String. Zero args. Wraps
            // `recv.inner_html()`.
            M::InnerHtml if matches!(recv_ty, Type::Element) => {
                if !args.is_empty() {
                    return Err(self.unsupported(&format!(
                        "inner_html() takes no arguments, got {}",
                        args.len()
                    )));
                }
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    #recv.inner_html()
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Element.inner_html codegen parse: {e}")))
            }
            // ---- Crawler instance methods (4) -----------------
            // `crawler.seed()` -> String. Zero args. Wraps `recv.seed()`.
            M::Seed if matches!(recv_ty, Type::Crawler) => {
                if !args.is_empty() {
                    return Err(self.unsupported(&format!(
                        "seed() takes no arguments, got {}",
                        args.len()
                    )));
                }
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    #recv.seed()
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Crawler.seed codegen parse: {e}")))
            }
            // `crawler.fetch(url)` -> Document. One arg (String URL).
            // Wraps `Crawler::fetch(&recv, &url).unwrap_or_default()`
            // (panic-free on network / HTTP error — Document impls
            // Default as `<html></html>`; matches the Image.from_path
            // `unwrap_or_default()` panic-free pattern).
            M::Fetch if matches!(recv_ty, Type::Crawler) => {
                let arg = one_arg(self)?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    buff_scrape::Crawler::fetch(&#recv, &#arg).unwrap_or_default()
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Crawler.fetch codegen parse: {e}")))
            }
            // `crawler.crawl(max_pages)` -> Vector<String>. One arg
            // (Int). Wraps `Crawler::crawl(&recv, max_pages as i64)
            // .unwrap_or_default()` (panic-free on network error —
            // returns whatever was crawled before the failure).
            M::Crawl if matches!(recv_ty, Type::Crawler) => {
                let arg = one_arg(self)?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    buff_scrape::Crawler::crawl(&#recv, #arg as i64).unwrap_or_default()
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Crawler.crawl codegen parse: {e}")))
            }
            // `crawler.robots_allows(url)` -> Bool. One arg (String URL).
            // Wraps `Crawler::robots_allows(&recv, &url)` (infallible —
            // returns `true` on robots.txt fetch failure per the Robots
            // Exclusion Protocol fail-open guidance).
            M::RobotsAllows if matches!(recv_ty, Type::Crawler) => {
                let arg = one_arg(self)?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    buff_scrape::Crawler::robots_allows(&#recv, &#arg)
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Crawler.robots_allows codegen parse: {e}")))
            }
            // T50: Xml instance methods. Each method lowers to
            // `buff_xml::XmlDocument::<method>`. The codegen records
            // `buff-xml` + `quick-xml` in extern_crates via the
            // `program_uses_namespace("Xml")` walker. All methods
            // panic-free at the codegen layer.
            //
            // `doc.root()` -> XmlDocument (opaque). Zero args.
            M::Root if matches!(recv_ty, Type::Xml) => {
                if !args.is_empty() {
                    return Err(self.unsupported(&format!(
                        "root() takes no arguments, got {}",
                        args.len()
                    )));
                }
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    buff_xml::XmlDocument::root(&#recv).clone()
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Xml.root codegen parse: {e}")))
            }
            // `doc.find(xpath)` -> Option<XmlDocument>. One arg (String).
            M::Find if matches!(recv_ty, Type::Xml) => {
                let arg = one_arg(self)?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    buff_xml::XmlDocument::find(&#recv, &#arg).ok().cloned()
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Xml.find codegen parse: {e}")))
            }
            // `doc.to_string()` -> String. Zero args.
            M::ToString if matches!(recv_ty, Type::Xml) => {
                if !args.is_empty() {
                    return Err(self.unsupported(&format!(
                        "to_string() takes no arguments, got {}",
                        args.len()
                    )));
                }
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    buff_xml::XmlDocument::to_string(&#recv).unwrap_or_default()
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Xml.to_string codegen parse: {e}")))
            }
            // T50: XmlElement instance methods. Each method lowers to
            // the matching `buff_xml::XmlElement` method. The wrapper
            // crate's methods return `&str` / `Option<&str>` /
            // `&[XmlElement]` — the codegen lifts these to owned Buff
            // values (`String` / `Option<String>` / `Vec<XmlElement>`)
            // per FFI guide R2 (Buff surfaces only owned values).
            // Records `buff-xml` + `quick-xml` in extern_crates via
            // the `program_uses_namespace("XmlElement")` walker.
            //
            // `el.name()` -> String. Zero args. Wraps
            // `recv.name().to_string()` (lifts `&str` -> `String`).
            M::Name if matches!(recv_ty, Type::XmlElement) => {
                if !args.is_empty() {
                    return Err(self.unsupported(&format!(
                        "name() takes no arguments, got {}",
                        args.len()
                    )));
                }
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    #recv.name().to_string()
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("XmlElement.name codegen parse: {e}")))
            }
            // `el.text()` -> Option<String>. Zero args. Wraps
            // `recv.text().map(|s| s.to_string())` (lifts
            // `Option<&str>` -> `Option<String>`).
            M::Text if matches!(recv_ty, Type::XmlElement) => {
                if !args.is_empty() {
                    return Err(self.unsupported(&format!(
                        "text() takes no arguments, got {}",
                        args.len()
                    )));
                }
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    #recv.text().map(|s| s.to_string())
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("XmlElement.text codegen parse: {e}")))
            }
            // `el.attr(name)` -> Option<String>. One arg (String).
            // Wraps `recv.attr(&name).map(|s| s.to_string())` (lifts
            // `Option<&str>` -> `Option<String>`).
            M::Attr if matches!(recv_ty, Type::XmlElement) => {
                let arg = one_arg(self)?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    #recv.attr(&#arg).map(|s| s.to_string())
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("XmlElement.attr codegen parse: {e}")))
            }
            // `el.children()` -> Vector<XmlElement>. Zero args. Wraps
            // `recv.children().to_vec()` (lifts `&[XmlElement]` ->
            // `Vec<XmlElement>`).
            M::Children if matches!(recv_ty, Type::XmlElement) => {
                if !args.is_empty() {
                    return Err(self.unsupported(&format!(
                        "children() takes no arguments, got {}",
                        args.len()
                    )));
                }
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    #recv.children().to_vec()
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("XmlElement.children codegen parse: {e}")))
            }
            // T10: AudioBuffer instance methods. Each method lowers
            // to `buff_audio::AudioBuffer::<method>`. The codegen
            // records `buff-audio` + `hound` + `symphonia` in
            // extern_crates via the `program_uses_namespace
            // ("AudioBuffer")` walker. All methods panic-free at the
            // codegen layer (slice via `unwrap_or_default()`;
            // AudioBuffer impls Default — added in the same T10
            // finish commit as this codegen arm).
            //
            // `buf.samples()` -> Vector<Float>. Zero args. Wraps
            // `recv.samples().to_vec()` (the `.to_vec()` lifts `&[f32]`
            // to `Vec<f32>` — Buff surfaces owned values).
            M::Samples if matches!(recv_ty, Type::Audio) => {
                if !args.is_empty() {
                    return Err(self.unsupported(&format!(
                        "samples() takes no arguments, got {}",
                        args.len()
                    )));
                }
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    #recv.samples().to_vec()
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("AudioBuffer.samples codegen parse: {e}")))
            }
            // `buf.sample_rate()` -> Int. Zero args. Wraps
            // `recv.sample_rate() as i64`.
            M::SampleRate if matches!(recv_ty, Type::Audio) => {
                if !args.is_empty() {
                    return Err(self.unsupported(&format!(
                        "sample_rate() takes no arguments, got {}",
                        args.len()
                    )));
                }
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    (#recv.sample_rate() as i64)
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("AudioBuffer.sample_rate codegen parse: {e}")))
            }
            // `buf.channels()` -> Int. Zero args. Wraps
            // `recv.channels() as i64`.
            M::Channels if matches!(recv_ty, Type::Audio) => {
                if !args.is_empty() {
                    return Err(self.unsupported(&format!(
                        "channels() takes no arguments, got {}",
                        args.len()
                    )));
                }
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    (#recv.channels() as i64)
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("AudioBuffer.channels codegen parse: {e}")))
            }
            // `buf.frames()` -> Int. Zero args. Wraps `recv.frames()
            // as i64`.
            M::Frames if matches!(recv_ty, Type::Audio) => {
                if !args.is_empty() {
                    return Err(self.unsupported(&format!(
                        "frames() takes no arguments, got {}",
                        args.len()
                    )));
                }
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    (#recv.frames() as i64)
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("AudioBuffer.frames codegen parse: {e}")))
            }
            // `buf.duration_secs()` -> Float. Zero args. Wraps
            // `recv.duration_secs()` (already f64).
            M::DurationSecs if matches!(recv_ty, Type::Audio) => {
                if !args.is_empty() {
                    return Err(self.unsupported(&format!(
                        "duration_secs() takes no arguments, got {}",
                        args.len()
                    )));
                }
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    #recv.duration_secs()
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("AudioBuffer.duration_secs codegen parse: {e}")))
            }
            // `buf.amplify(factor)` -> Void. One arg (Float). In-place
            // scale. Infallible.
            M::Amplify if matches!(recv_ty, Type::Audio) => {
                if args.len() != 1 {
                    return Err(self.unsupported(&format!(
                        "amplify() expects exactly 1 arg (factor), got {}",
                        args.len()
                    )));
                }
                let factor = self.lower_expr(&args[0])?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    #recv.amplify(#factor as f32)
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("AudioBuffer.amplify codegen parse: {e}")))
            }
            // `buf.normalize(target)` -> Void. One arg (Float). In-
            // place peak-normalize. Infallible (zero-sample buffer is
            // a no-op).
            M::Normalize if matches!(recv_ty, Type::Audio) => {
                if args.len() != 1 {
                    return Err(self.unsupported(&format!(
                        "normalize() expects exactly 1 arg (target), got {}",
                        args.len()
                    )));
                }
                let target = self.lower_expr(&args[0])?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    #recv.normalize(#target as f32)
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("AudioBuffer.normalize codegen parse: {e}")))
            }
            // `buf.mix(other)` -> Void. One arg (AudioBuffer). Sample-
            // wise add. Panic-free via `unwrap_or_default()` (rate /
            // channel mismatch is a no-op).
            M::Mix if matches!(recv_ty, Type::Audio) => {
                if args.len() != 1 {
                    return Err(self.unsupported(&format!(
                        "mix() expects exactly 1 arg (other AudioBuffer), got {}",
                        args.len()
                    )));
                }
                let other = self.lower_expr(&args[0])?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    #recv.mix(&#other).unwrap_or_default()
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("AudioBuffer.mix codegen parse: {e}")))
            }
            // `buf.slice(start_sec, end_sec)` -> AudioBuffer. Two args
            // (Float, Float). Returns a new AudioBuffer for the time
            // window. Panic-free via `unwrap_or_default()`
            // (AudioBuffer impls Default — invalid endpoints collapse
            // to empty 44100Hz mono).
            M::Slice if matches!(recv_ty, Type::Audio) => {
                if args.len() != 2 {
                    return Err(self.unsupported(&format!(
                        "slice() expects exactly 2 args (start_sec, end_sec), got {}",
                        args.len()
                    )));
                }
                let start = self.lower_expr(&args[0])?;
                let end = self.lower_expr(&args[1])?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    #recv.slice(#start as f64, #end as f64).unwrap_or_default()
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("AudioBuffer.slice codegen parse: {e}")))
            }
            // `buf.summarize()` -> AudioSummary. Zero args. Returns a
            // statistics snapshot. Infallible (returns AudioSummary
            // directly — no `unwrap_or_default()` needed). The return
            // type is Type::Unknown at the type-checker layer; codegen
            // emits the bare call and Rust infers
            // `buff_audio::AudioSummary`.
            M::Summarize if matches!(recv_ty, Type::Audio) => {
                if !args.is_empty() {
                    return Err(self.unsupported(&format!(
                        "summarize() takes no arguments, got {}",
                        args.len()
                    )));
                }
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    #recv.summarize()
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("AudioBuffer.summarize codegen parse: {e}")))
            }
            // `Save` is shared between Image.save (above) and
            // AudioBuffer.save (below) — dispatched on receiver type
            // (mirrors `Send` shared between Connection / WsConnection
            // and `Format` shared between DateTime / Date / Time).
            //
            // `img.save(path)` -> Void. One arg (String / Path).
            // Writes to disk. Panic-free via `unwrap_or_default()` (()
            // impls Default — I/O failure is a no-op).
            M::Save if matches!(recv_ty, Type::Image) => {
                if args.len() != 1 {
                    return Err(self.unsupported(&format!(
                        "save() expects exactly 1 arg (path), got {}",
                        args.len()
                    )));
                }
                let path = self.lower_expr(&args[0])?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    #recv.save(#path).unwrap_or_default()
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Image.save codegen parse: {e}")))
            }
            // `buf.save(path)` -> Void. One arg (String / Path). WAV
            // encode. Panic-free via `unwrap_or_default()`.
            M::Save if matches!(recv_ty, Type::Audio) => {
                if args.len() != 1 {
                    return Err(self.unsupported(&format!(
                        "save() expects exactly 1 arg (path), got {}",
                        args.len()
                    )));
                }
                let path = self.lower_expr(&args[0])?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    #recv.save(#path).unwrap_or_default()
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("AudioBuffer.save codegen parse: {e}")))
            }
            // T19: Template.render(context_json) -> String. One arg
            // (String — a JSON object). Wraps
            // `buff_template::Template::render(&self, &ctx)
            // .unwrap_or_default()` (panic-free on render failure —
            // missing variable / partial error collapses to empty
            // string, matching Buff's "no panicking generated code"
            // rule).
            M::Render if matches!(recv_ty, Type::Template) => {
                if args.len() != 1 {
                    return Err(self.unsupported(&format!(
                        "render() expects exactly 1 arg (context_json), got {}",
                        args.len()
                    )));
                }
                let ctx = self.lower_expr(&args[0])?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    #recv.render(#ctx).unwrap_or_default()
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Template.render codegen parse: {e}")))
            }
            // Non-Image / Non-Audio receiver with an Image-only /
            // Audio-only method (Width / Height / PixelFormat /
            // GetPixel / SetPixel / Grayscale / Invert / Resize /
            // Crop / Blur / Samples / SampleRate / Channels / Frames
            // / DurationSecs / Amplify / Normalize / Mix / Slice /
            // Summarize / Save) falls through to a clear error
            // (mirrors the Select / Filter / Sort / Head / GroupBy /
            // Agg / ToTableString safety net above).
            M::Width
            | M::Height
            | M::PixelFormat
            | M::GetPixel
            | M::SetPixel
            | M::Grayscale
            | M::Invert
            | M::Resize
            | M::Crop
            | M::Blur
            | M::Samples
            | M::SampleRate
            | M::Channels
            | M::Frames
            | M::DurationSecs
            | M::Amplify
            | M::Normalize
            | M::Mix
            | M::Slice
            | M::Summarize
            | M::Render
            | M::Save
            // T31: Cache-only methods. Non-Cache receiver with one of
            // these methods falls through to a clear error (mirrors
            // the Image / Audio / Template safety nets above).
            | M::SetTtl
            | M::Delete
            | M::Contains
            | M::Clear => Err(self.unsupported(&format!(
                "{recv_ty}.{:?}() is not a recognised prelude instance method",
                pmethod
            ))),
            // T20: Reactive instance methods on Type::Unknown receivers.
            // Dispatched when type inference resolves the receiver to
            // Type::Unknown (the forward-declaration contract for
            // ReactiveSignal.new / ReactiveComputed.new /
            // ReactiveEffect.new return values — coordinated
            // Type::ReactiveSignal / ReactiveComputed / ReactiveEffect
            // variants in ty.rs are follow-up sibling tasks OUTSIDE
            // the T20 shared zone). Each arm emits the method call
            // directly; Rust's method resolution finds the matching
            // `buff_reactive::Signal::get` / `set` / `update` /
            // `Computed::get` / `Effect::run` method.
            M::Get if matches!(recv_ty, Type::Unknown) => {
                if !args.is_empty() {
                    return Err(self.unsupported(&format!(
                        "get() takes no arguments, got {}",
                        args.len()
                    )));
                }
                Ok(method_call_no_args(recv, "get"))
            }
            M::Set if matches!(recv_ty, Type::Unknown) => {
                if args.len() != 1 {
                    return Err(self.unsupported(&format!(
                        "set() expects exactly 1 arg (the new value), got {}",
                        args.len()
                    )));
                }
                let value = self.lower_expr(&args[0])?;
                Ok(method_call_one_arg(recv, "set", value))
            }
            M::Update if matches!(recv_ty, Type::Unknown) => {
                if args.len() != 1 {
                    return Err(self.unsupported(&format!(
                        "update() expects exactly 1 arg (the mutating closure), got {}",
                        args.len()
                    )));
                }
                let closure = self.lower_expr(&args[0])?;
                // `Signal::update` returns `Result<(), ReactiveError>`;
                // discard via `.ok()` so generated code is panic-free
                // and infallible at the Buff surface (mirrors the
                // DataFrame / Image unwrap_or_default stance).
                let call = method_call_one_arg(recv, "update", closure);
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    #call.ok()
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Signal.update codegen parse: {e}")))
            }
            M::Invalidate if matches!(recv_ty, Type::Unknown) => {
                if !args.is_empty() {
                    return Err(self.unsupported(&format!(
                        "invalidate() takes no arguments, got {}",
                        args.len()
                    )));
                }
                Ok(method_call_no_args(recv, "invalidate"))
            }
            // T29: Validator instance methods. Records `buff-validate`
            // + `validator` + `serde_json` + `regex` in extern_crates
            // via the `program_uses_namespace("Validator")` walker.
            //
            // The five builder methods (with_*) consume self and
            // return Self — Buff's "no visible references" stance
            // mirrors the axum `Router::route` pattern. Each call
            // lowers to `recv.with_xxx(arg)` (the buff-validate
            // surface takes the args by value).
            //
            // `validator.with_email(field)` -> Validator. One arg.
            M::WithEmail if matches!(recv_ty, Type::Validator) => {
                if args.len() != 1 {
                    return Err(self.unsupported(&format!(
                        "with_email() expects exactly 1 arg (field), got {}",
                        args.len()
                    )));
                }
                let field = self.lower_expr(&args[0])?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    #recv.with_email(#field)
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Validator.with_email codegen parse: {e}")))
            }
            // `validator.with_url(field)` -> Validator. One arg.
            M::WithUrl if matches!(recv_ty, Type::Validator) => {
                if args.len() != 1 {
                    return Err(self.unsupported(&format!(
                        "with_url() expects exactly 1 arg (field), got {}",
                        args.len()
                    )));
                }
                let field = self.lower_expr(&args[0])?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    #recv.with_url(#field)
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Validator.with_url codegen parse: {e}")))
            }
            // `validator.with_length(field, min, max)` -> Validator.
            // Three args. Panic-free via `unwrap_or_default()`
            // (Validator impls Default as an empty rule set —
            // InvalidRuleConfig surfaces as no-op clone).
            M::WithLength if matches!(recv_ty, Type::Validator) => {
                if args.len() != 3 {
                    return Err(self.unsupported(&format!(
                        "with_length() expects exactly 3 args (field, min, max), got {}",
                        args.len()
                    )));
                }
                let field = self.lower_expr(&args[0])?;
                let min = self.lower_expr(&args[1])?;
                let max = self.lower_expr(&args[2])?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    #recv.with_length(#field, #min as u64, #max as u64).unwrap_or_default()
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Validator.with_length codegen parse: {e}")))
            }
            // `validator.with_range(field, min, max)` -> Validator.
            // Three args. Panic-free via `unwrap_or_default()`.
            M::WithRange if matches!(recv_ty, Type::Validator) => {
                if args.len() != 3 {
                    return Err(self.unsupported(&format!(
                        "with_range() expects exactly 3 args (field, min, max), got {}",
                        args.len()
                    )));
                }
                let field = self.lower_expr(&args[0])?;
                let min = self.lower_expr(&args[1])?;
                let max = self.lower_expr(&args[2])?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    #recv.with_range(#field, #min as i64, #max as i64).unwrap_or_default()
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Validator.with_range codegen parse: {e}")))
            }
            // `validator.with_regex(field, pattern)` -> Validator.
            // Two args. Panic-free via `unwrap_or_default()` (a
            // malformed pattern surfaces as no-op clone — the
            // underlying buff-validate surface compiles the regex
            // eagerly at registration).
            M::WithRegex if matches!(recv_ty, Type::Validator) => {
                if args.len() != 2 {
                    return Err(self.unsupported(&format!(
                        "with_regex() expects exactly 2 args (field, pattern), got {}",
                        args.len()
                    )));
                }
                let field = self.lower_expr(&args[0])?;
                let pattern = self.lower_expr(&args[1])?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    #recv.with_regex(#field, #pattern).unwrap_or_default()
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Validator.with_regex codegen parse: {e}")))
            }
            // `validator.validate(input)` -> Result<Void, String>.
            // One arg (Map<String, String>). Wraps
            // `recv.validate(&input).map_err(|e| e.to_string())` so
            // the Buff `?` operator propagates a string error.
            M::Validate if matches!(recv_ty, Type::Validator) => {
                if args.len() != 1 {
                    return Err(self.unsupported(&format!(
                        "validate() expects exactly 1 arg (input), got {}",
                        args.len()
                    )));
                }
                let input = self.lower_expr(&args[0])?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    #recv.validate(#input).map_err(|e| e.to_string())
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Validator.validate codegen parse: {e}")))
            }
            // `validator.to_json_schema()` -> String. Zero args.
            // Wraps `recv.to_json_schema()`.
            M::ToJsonSchema if matches!(recv_ty, Type::Validator) => {
                if !args.is_empty() {
                    return Err(self.unsupported(&format!(
                        "to_json_schema() takes no arguments, got {}",
                        args.len()
                    )));
                }
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    #recv.to_json_schema()
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Validator.to_json_schema codegen parse: {e}")))
            }
            // T42: Email builder methods. Each consumes self and
            // returns a new Email (Buff "no visible references"
            // stance — mirrors Validator with_* + HttpClient.new).
            // `email.body(text)` -> Email. One arg (String plain).
            // Wraps `recv.body(&text)?` (the `?` propagates
            // EmailError::Panic — only failure mode for a string
            // setter per the catch_unwind contract).
            M::Body if matches!(recv_ty, Type::Email) => {
                if args.len() != 1 {
                    return Err(self.unsupported(&format!(
                        "body() expects exactly 1 arg (text), got {}",
                        args.len()
                    )));
                }
                let text = self.lower_expr(&args[0])?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    #recv.body(&#text)?
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Email.body codegen parse: {e}")))
            }
            // `email.html(template, context_json)` -> Email. Two args
            // (String handlebars template, String JSON context).
            // Wraps `recv.html(&template, &ctx)?` (the `?` propagates
            // EmailError::TemplateParse / TemplateRender).
            M::Html if matches!(recv_ty, Type::Email) => {
                if args.len() != 2 {
                    return Err(self.unsupported(&format!(
                        "html() expects exactly 2 args (template, context_json), got {}",
                        args.len()
                    )));
                }
                let template = self.lower_expr(&args[0])?;
                let context = self.lower_expr(&args[1])?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    #recv.html(&#template, &#context)?
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Email.html codegen parse: {e}")))
            }
            // `email.attach(path)` -> Email. One arg (String path).
            // Wraps `recv.attach(&path)?` (panic-free — file is NOT
            // read at builder time; EmailError surfaces at send time
            // via the build_message MIME-assembly path).
            M::Attach if matches!(recv_ty, Type::Email) => {
                if args.len() != 1 {
                    return Err(self.unsupported(&format!(
                        "attach() expects exactly 1 arg (path), got {}",
                        args.len()
                    )));
                }
                let path = self.lower_expr(&args[0])?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    #recv.attach(&#path)?
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Email.attach codegen parse: {e}")))
            }
            // T42: SmtpClient action method. The single send method
            // is dispatched on (Type::SmtpClient, Send) — shares the
            // Send variant with TCP / WebSocket / Sender. Returns
            // Void (the codegen discards the Result via
            // unwrap_or_default panic-free — invalid email / SMTP
            // failure is a no-op at the Buff surface, matching the
            // Image save / Cache set precedent).
            // `client.send(email)` -> Void. One arg (Email). Wraps
            // `recv.send(&email).unwrap_or_default()`.
            M::Send if matches!(recv_ty, Type::SmtpClient) => {
                if args.len() != 1 {
                    return Err(self.unsupported(&format!(
                        "send() expects exactly 1 arg (email), got {}",
                        args.len()
                    )));
                }
                let email = self.lower_expr(&args[0])?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    #recv.send(&#email).unwrap_or_default()
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("SmtpClient.send codegen parse: {e}")))
            }
            // T48: buff-web3 instance methods. Each lowers to a
            // fully-qualified `buff_web3::*` method chained with
            // `.unwrap_or_default()` / `as i64` (panic-free — mirrors
            // T9 Image / T45 Point / T47 Bot / T52 Message). The
            // shared `Address` variant covers Wallet.address /
            // ConnectedWallet.address / Contract.address; the shared
            // `Connect` variant covers Wallet.connect; the shared
            // `Send` variant covers ContractMethod.send.
            //
            // `provider.chain_id()` -> Int. Zero args. Wraps
            // `recv.chain_id().unwrap_or_default() as i64`.
            M::ChainId if matches!(recv_ty, Type::Provider) => {
                if !args.is_empty() {
                    return Err(self.unsupported(&format!(
                        "chain_id() takes no arguments, got {}",
                        args.len()
                    )));
                }
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    #recv.chain_id().unwrap_or_default() as i64
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Provider.chain_id codegen parse: {e}")))
            }
            // `provider.block_number()` -> Int. Zero args.
            M::BlockNumber if matches!(recv_ty, Type::Provider) => {
                if !args.is_empty() {
                    return Err(self.unsupported(&format!(
                        "block_number() takes no arguments, got {}",
                        args.len()
                    )));
                }
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    #recv.block_number().unwrap_or_default() as i64
                };
                syn::parse2(tokens).map_err(|e| {
                    self.unsupported(&format!("Provider.block_number codegen parse: {e}"))
                })
            }
            // `provider.get_balance(address)` -> Int. One arg (String).
            M::GetBalance if matches!(recv_ty, Type::Provider) => {
                if args.len() != 1 {
                    return Err(self.unsupported(&format!(
                        "get_balance() expects exactly 1 arg (address), got {}",
                        args.len()
                    )));
                }
                let address = self.lower_expr(&args[0])?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    #recv.get_balance(&#address).unwrap_or_default() as i64
                };
                syn::parse2(tokens).map_err(|e| {
                    self.unsupported(&format!("Provider.get_balance codegen parse: {e}"))
                })
            }
            // `provider.get_nonce(address)` -> Int. One arg (String).
            M::GetNonce if matches!(recv_ty, Type::Provider) => {
                if args.len() != 1 {
                    return Err(self.unsupported(&format!(
                        "get_nonce() expects exactly 1 arg (address), got {}",
                        args.len()
                    )));
                }
                let address = self.lower_expr(&args[0])?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    #recv.get_nonce(&#address).unwrap_or_default() as i64
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Provider.get_nonce codegen parse: {e}")))
            }
            // `provider.wait_for_tx(tx_hash)` -> String. One arg (String).
            M::WaitForTx if matches!(recv_ty, Type::Provider) => {
                if args.len() != 1 {
                    return Err(self.unsupported(&format!(
                        "wait_for_tx() expects exactly 1 arg (tx_hash), got {}",
                        args.len()
                    )));
                }
                let hash = self.lower_expr(&args[0])?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    #recv.wait_for_tx(&#hash).unwrap_or_default()
                };
                syn::parse2(tokens).map_err(|e| {
                    self.unsupported(&format!("Provider.wait_for_tx codegen parse: {e}"))
                })
            }
            // `wallet.address()` -> String. Zero args. Shared `Address`
            // variant dispatched on (Wallet, Address) / (ConnectedWallet,
            // Address) / (Contract, Address) pairs. Infallible (the
            // underlying buff_web3 methods return String directly).
            M::Address if matches!(recv_ty, Type::Wallet | Type::ConnectedWallet | Type::Contract) => {
                if !args.is_empty() {
                    return Err(self.unsupported(&format!(
                        "address() takes no arguments, got {}",
                        args.len()
                    )));
                }
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    #recv.address()
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("web3 .address codegen parse: {e}")))
            }
            // `wallet.connect(provider)` -> ConnectedWallet. One arg
            // (Provider). Infallible (returns ConnectedWallet directly —
            // no failure mode). Wraps `recv.connect(#provider)` (move
            // semantics — consumes self). Shared `Connect` variant
            // dispatched on (Wallet, Connect) — distinct lowering from
            // TCP.connect / WebSocket.connect.
            M::Connect if matches!(recv_ty, Type::Wallet) => {
                if args.len() != 1 {
                    return Err(self.unsupported(&format!(
                        "connect() expects exactly 1 arg (provider), got {}",
                        args.len()
                    )));
                }
                let provider = self.lower_expr(&args[0])?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    #recv.connect(#provider)
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Wallet.connect codegen parse: {e}")))
            }
            // `wallet.sign_message(message)` -> String. One arg (String).
            // Wraps `recv.sign_message(&msg).unwrap_or_default()` (panic-
            // free — Web3Error::Rpc collapses to String::default()).
            M::SignMessage if matches!(recv_ty, Type::Wallet) => {
                if args.len() != 1 {
                    return Err(self.unsupported(&format!(
                        "sign_message() expects exactly 1 arg (message), got {}",
                        args.len()
                    )));
                }
                let message = self.lower_expr(&args[0])?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    #recv.sign_message(&#message).unwrap_or_default()
                };
                syn::parse2(tokens).map_err(|e| {
                    self.unsupported(&format!("Wallet.sign_message codegen parse: {e}"))
                })
            }
            // `contract.method(name)` -> ContractMethod. One arg (String).
            // Wraps `recv.method(&name).unwrap_or_default()` (panic-free —
            // MethodNotFound / InvalidAbi collapses to a default
            // ContractMethod whose .call() / .send() return Default).
            M::Method if matches!(recv_ty, Type::Contract) => {
                if args.len() != 1 {
                    return Err(self.unsupported(&format!(
                        "method() expects exactly 1 arg (name), got {}",
                        args.len()
                    )));
                }
                let name = self.lower_expr(&args[0])?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    #recv.method(&#name).unwrap_or_default()
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Contract.method codegen parse: {e}")))
            }
            // `m.arg(name, value)` -> ContractMethod. Two args (String
            // name, String value). The name is currently IGNORED at the
            // wire layer (ethers::abi::Token doesn't carry names for
            // non-tuple inputs); future tuple support may consume it.
            // The value is spliced as `ethers::abi::Token::String`.
            // Chainable — consumes self, returns Self (mirrors
            // Validator.with_* / Email.body builder pattern).
            M::Arg if matches!(recv_ty, Type::ContractMethod) => {
                if args.len() != 2 {
                    return Err(self.unsupported(&format!(
                        "arg() expects exactly 2 args (name, value), got {}",
                        args.len()
                    )));
                }
                let value = self.lower_expr(&args[1])?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    #recv.arg(ethers::abi::Token::String((#value).to_string()))
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("ContractMethod.arg codegen parse: {e}")))
            }
            // `m.args(values)` -> ContractMethod. One arg (Vector<String>).
            // Each value spliced as `ethers::abi::Token::String`.
            // Chainable — consumes self, returns Self.
            M::Args if matches!(recv_ty, Type::ContractMethod) => {
                if args.len() != 1 {
                    return Err(self.unsupported(&format!(
                        "args() expects exactly 1 arg (values), got {}",
                        args.len()
                    )));
                }
                let values = self.lower_expr(&args[0])?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    #recv.args((#values).into_iter().map(|v| ethers::abi::Token::String(v)))
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("ContractMethod.args codegen parse: {e}")))
            }
            // `m.call()` -> String. Zero args. Wraps `recv.call()
            // .unwrap_or_default()` (panic-free — Web3Error::Rpc /
            // AbiDecode collapses to String::default()).
            M::Call if matches!(recv_ty, Type::ContractMethod) => {
                if !args.is_empty() {
                    return Err(self.unsupported(&format!(
                        "call() takes no arguments, got {}",
                        args.len()
                    )));
                }
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    #recv.call().unwrap_or_default()
                };
                syn::parse2(tokens).map_err(|e| {
                    self.unsupported(&format!("ContractMethod.call codegen parse: {e}"))
                })
            }
            // `m.send()` -> String. Zero args. Wraps `recv.send()
            // .unwrap_or_default()` (panic-free — Web3Error::Rpc /
            // WalletNotConnected collapses to String::default()). Shared
            // `Send` variant dispatched on (ContractMethod, Send) —
            // distinct lowering from Connection.send /
            // WsConnection.send / Sender.send / SmtpClient.send.
            M::Send if matches!(recv_ty, Type::ContractMethod) => {
                if !args.is_empty() {
                    return Err(self.unsupported(&format!(
                        "send() takes no arguments, got {}",
                        args.len()
                    )));
                }
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    #recv.send().unwrap_or_default()
                };
                syn::parse2(tokens).map_err(|e| {
                    self.unsupported(&format!("ContractMethod.send codegen parse: {e}"))
                })
            }
            // T49: RsaKeypair.public_pem() -> String. Zero args.
            // Wraps `recv.public_pem.clone()` (the underlying field
            // is `String`; `.clone()` lifts `&String` to owned
            // `String` per Buff's "hide references from users" rule).
            // Infallible (no failure mode — the field is always
            // populated when constructed via RSA.generate_keypair).
            // Shared `PublicPem` variant dispatched on
            // (RsaKeypair, PublicPem).
            M::PublicPem if matches!(recv_ty, Type::RsaKeypair) => {
                if !args.is_empty() {
                    return Err(self.unsupported(&format!(
                        "public_pem() takes no arguments, got {}",
                        args.len()
                    )));
                }
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    #recv.public_pem.clone()
                };
                syn::parse2(tokens).map_err(|e| {
                    self.unsupported(&format!("RsaKeypair.public_pem codegen parse: {e}"))
                })
            }
            // T49: RsaKeypair.private_pem() -> String. Zero args.
            // Wraps `recv.private_pem.clone()`. Same shape as
            // public_pem above.
            M::PrivatePem if matches!(recv_ty, Type::RsaKeypair) => {
                if !args.is_empty() {
                    return Err(self.unsupported(&format!(
                        "private_pem() takes no arguments, got {}",
                        args.len()
                    )));
                }
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    #recv.private_pem.clone()
                };
                syn::parse2(tokens).map_err(|e| {
                    self.unsupported(&format!("RsaKeypair.private_pem codegen parse: {e}"))
                })
            }
            // T31 (gap-fill): wildcard for instance-method variants
            // whose codegen arms haven't been written yet (T11 Signal
            // / Spectrum / Window, T12 ECS, T17 Web route_*, T20
            // Reactive Update/Invalidate, T26 Audit, T29 Validator
            // with_*). The variant exists in the PreludeInstanceFn
            // enum but no `lower_*` arm handles it — surfaces as a
            // clear "unsupported" error instead of a compile break.
            // As sibling tasks complete their codegen wiring, they
            // add explicit arms ABOVE this wildcard.
            _ => Err(self.unsupported(&format!(
                "{recv_ty}.{:?}() codegen not yet implemented",
                pmethod
            ))),
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

    fn unsupported(&self, what: &str) -> CodegenError {
        CodegenError::new(
            Diagnostic::error(format!("unsupported: {what}"), BuffSpan::dummy())
                .with_code(ErrorCode::UnsupportedCodegen),
        )
    }
}

impl Default for RustCodegen {
    fn default() -> Self {
        Self::new()
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
        let func = FuncDecl { name: AstIdent::new("empty", dummy_span()),
        params: Vec::new(),
        return_type: None,
        body: Block::empty(dummy_span()),
        is_async: false,
        is_unsafe: false,
        is_extern: false, attributes: Vec::new(), type_params: Vec::new(), span: dummy_span(), };
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
            is_comptime: false,
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

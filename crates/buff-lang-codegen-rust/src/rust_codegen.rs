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
mod decl_lowering;
mod expr_lowering;
mod lowering_helpers;
mod method_call_lowering;
mod prelude_fns;
mod prelude_types;
mod type_lowering;
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
        // T89: register the `rust_decimal` crate as an external dependency
        // when the program references the prelude `Decimal` type
        // (`Decimal.new(s)` / `Decimal.from_float(f)` / `d.add(other)` /
        // `d.mul(other)` / `d.to_string()`). Generated code uses fully-
        // qualified `rust_decimal::Decimal::from_str` /
        // `rust_decimal::Decimal::from_f64` paths so no `use` import is
        // emitted — but the recorded name signals to the pipeline /
        // build-driver that the generated Cargo project must declare
        // `rust_decimal` in `[dependencies]`.
        if program_uses_namespace(decls, "Decimal") {
            self.extern_crates.insert("rust_decimal".to_string());
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
        // T25: register the `reqwest` crate as an external dependency
        // when the program references the `Http` prelude namespace
        // (`Http.get(url)` / `Http.post(url, body)`). Generated code
        // uses fully-qualified `reqwest::blocking::get` /
        // `reqwest::blocking::Client::new().post(...)` paths so no
        // `use` import is emitted.
        if program_uses_namespace(decls, "Http") {
            self.extern_crates.insert("reqwest".to_string());
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
                                return self.lower_map_index_write(base, &indices[0], value);
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
            Stmt::While { cond, body, .. } => {
                // BUG-9: `while cond { body }` — identical lowering to
                // ForWhile (Rust `while cond { body }`).
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
                    // T70: `__buff_pin` is the `@pin` desugar sentinel. The
                    // parser rewrites `@pin let x = expr` into
                    // `let x = __buff_pin(expr)`; we lower this to
                    // `std::hint::black_box(expr)`, which prevents rustc/LLVM
                    // from eliminating, moving, or reordering the binding
                    // (useful for memory-mapped I/O and hardware registers).
                    // The sentinel is intercepted BEFORE the prelude lookup
                    // so it never reaches the normal function-call path
                    // (where it would emit a call to a non-existent
                    // `__buff_pin` fn). `std::hint::black_box` is in std —
                    // no extern crate dependency.
                    if name.name == "__buff_pin" && args_ref.len() == 1 {
                        let inner = self.lower_expr(&args_ref[0])?;
                        let mut call_args: Punctuated<SynExpr, syn::Token![,]> = Punctuated::new();
                        call_args.push(inner);
                        return Ok(SynExpr::Call(syn::ExprCall {
                            attrs: Vec::new(),
                            func: Box::new(SynExpr::Path(syn::ExprPath {
                                attrs: Vec::new(),
                                qself: None,
                                path: rust_path("std::hint::black_box"),
                            })),
                            paren_token: Default::default(),
                            args: call_args,
                        }));
                    }
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
                    // Self-host: data variant constructor qualification.
                    // If the callee name matches a user-defined enum variant,
                    // generate EnumName::VariantName(args) instead of bare
                    // VariantName(args). This enables enum variants WITH data
                    // like Circle(5) → Shape::Circle(5).
                    if let Some(enum_name) = self.user_enum_variants.get(&name.name) {
                        let qualified = format!("{}::{}", enum_name, name.name);
                        let callee_expr = SynExpr::Path(syn::ExprPath {
                            attrs: Vec::new(),
                            qself: None,
                            path: rust_path(&qualified),
                        });
                        let mut call_args: Punctuated<SynExpr, syn::Token![,]> = Punctuated::new();
                        for arg in args_ref {
                            call_args.push(self.lower_expr(arg)?);
                        }
                        return Ok(SynExpr::Call(syn::ExprCall {
                            attrs: Vec::new(),
                            func: Box::new(callee_expr),
                            paren_token: Default::default(),
                            args: call_args,
                        }));
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
            // Guard on Type::Regex so the String.Replace arm below
            // (dispatched on Type::String) is reachable (T73).
            M::Replace if matches!(recv_ty, Type::Regex) => {
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
            M::Len if !matches!(recv_ty, Type::Cache | Type::String) => Err(self.unsupported(&format!(
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
            // T89: Decimal instance methods. Dispatched on
            // (Type::Decimal, variant) pairs. Reuses the existing
            // Add / Mul / ToString variants (shared with Simd / Xml).
            //
            // `d.add(other)` -> Decimal. One arg (Decimal). Wraps
            // `recv + other` (Rust's `Add` trait on `rust_decimal::Decimal`).
            M::Add if matches!(recv_ty, Type::Decimal) => {
                if args.len() != 1 {
                    return Err(self.unsupported(&format!(
                        "add() expects exactly 1 arg (other Decimal), got {}",
                        args.len()
                    )));
                }
                let other = self.lower_expr(&args[0])?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    #recv + #other
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Decimal.add codegen parse: {e}")))
            }
            // `d.mul(other)` -> Decimal. One arg (Decimal). Wraps
            // `recv * other` (Rust's `Mul` trait on `rust_decimal::Decimal`).
            M::Mul if matches!(recv_ty, Type::Decimal) => {
                if args.len() != 1 {
                    return Err(self.unsupported(&format!(
                        "mul() expects exactly 1 arg (other Decimal), got {}",
                        args.len()
                    )));
                }
                let other = self.lower_expr(&args[0])?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    #recv * #other
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Decimal.mul codegen parse: {e}")))
            }
            // `d.to_string()` -> String. Zero args. Wraps
            // `recv.to_string()` (rust_decimal::Decimal implements Display).
            M::ToString if matches!(recv_ty, Type::Decimal) => {
                if !args.is_empty() {
                    return Err(self.unsupported(&format!(
                        "to_string() takes no arguments, got {}",
                        args.len()
                    )));
                }
                Ok(method_call_no_args(recv, "to_string"))
            }
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
            // T84: `(0..10).contains(5)` -> Bool. One arg. Wraps
            // `recv.contains(&item)` — Rust's `std::ops::Range` /
            // `RangeInclusive` both implement `RangeBounds<T>` whose
            // `contains` takes `&T`. The receiver type is `Type::Range`
            // (the lazy integer range produced by `..` / `..=`). O(1)
            // — never materialises the range.
            M::Contains if matches!(recv_ty, Type::Range(_)) => {
                if args.len() != 1 {
                    return Err(self.unsupported(&format!(
                        "contains() expects exactly 1 arg (item), got {}",
                        args.len()
                    )));
                }
                let item = self.lower_expr(&args[0])?;
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    #recv.contains(&#item)
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Range.contains codegen parse: {e}")))
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
            // T73: String instance methods — dispatched on (Type::String,
            // variant) pairs. These lower to Rust's str methods directly.
            // Methods returning `&str` chain `.to_string()` to produce an
            // owned String (Buff hides references from users).
            //
            // `s.split(sep)` -> Vector<String>. Wraps
            // `s.split(sep).map(|s| s.to_string()).collect::<Vec<String>>()`.
            M::Split if matches!(recv_ty, Type::String) => {
                let sep = one_arg(self)?;
                let sep_ref = coerce_str_arg_to_ref(sep, &args[0]);
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    #recv.split(#sep_ref).map(|s| s.to_string()).collect::<Vec<String>>()
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("String.split codegen parse: {e}")))
            }
            // `s.trim()` -> String. Wraps `s.trim().to_string()`.
            M::Trim if matches!(recv_ty, Type::String) => {
                if !args.is_empty() {
                    return Err(self.unsupported(&format!(
                        "trim() takes no arguments, got {}",
                        args.len()
                    )));
                }
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    #recv.trim().to_string()
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("String.trim codegen parse: {e}")))
            }
            // `s.starts_with(prefix)` -> Bool. Wraps `s.starts_with(prefix)`.
            M::StartsWith if matches!(recv_ty, Type::String) => {
                let prefix = one_arg(self)?;
                let prefix_ref = coerce_str_arg_to_ref(prefix, &args[0]);
                Ok(method_call_one_arg(recv, "starts_with", prefix_ref))
            }
            // `s.ends_with(suffix)` -> Bool. Wraps `s.ends_with(suffix)`.
            M::EndsWith if matches!(recv_ty, Type::String) => {
                let suffix = one_arg(self)?;
                let suffix_ref = coerce_str_arg_to_ref(suffix, &args[0]);
                Ok(method_call_one_arg(recv, "ends_with", suffix_ref))
            }
            // `s.to_upper()` -> String. Wraps `s.to_uppercase().to_string()`.
            // `.to_uppercase()` returns a `String` in Rust, but we chain
            // `.to_string()` for consistency (the source `&str` method
            // signature returns `String` directly; chaining `.to_string()`
            // on a `String` is a no-op clone, but the generated code is
            // simple and the optimizer removes the redundant `to_string`).
            M::ToUppercase if matches!(recv_ty, Type::String) => {
                if !args.is_empty() {
                    return Err(self.unsupported(&format!(
                        "to_upper() takes no arguments, got {}",
                        args.len()
                    )));
                }
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    #recv.to_uppercase().to_string()
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("String.to_uppercase codegen parse: {e}")))
            }
            // `s.to_lower()` -> String. Wraps `s.to_lowercase().to_string()`.
            M::ToLowercase if matches!(recv_ty, Type::String) => {
                if !args.is_empty() {
                    return Err(self.unsupported(&format!(
                        "to_lower() takes no arguments, got {}",
                        args.len()
                    )));
                }
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    #recv.to_lowercase().to_string()
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("String.to_lowercase codegen parse: {e}")))
            }
            // `s.replace(from, to)` -> String (reuses existing Replace
            // variant — dispatched on Type::String). Wraps
            // `s.replace(from, to)`. Two args (String, String).
            M::Replace if matches!(recv_ty, Type::String) => {
                if args.len() != 2 {
                    return Err(self.unsupported(&format!(
                        "replace() expects exactly 2 args (from, to), got {}",
                        args.len()
                    )));
                }
                let from = self.lower_expr(&args[0])?;
                let to = self.lower_expr(&args[1])?;
                let from_ref = coerce_str_arg_to_ref(from, &args[0]);
                let to_ref = coerce_str_arg_to_ref(to, &args[1]);
                let mut call_args: Punctuated<SynExpr, syn::Token![,]> = Punctuated::new();
                call_args.push(from_ref);
                call_args.push(to_ref);
                let replace_call = SynExpr::MethodCall(syn::ExprMethodCall {
                    attrs: Vec::new(),
                    receiver: Box::new(recv),
                    dot_token: Default::default(),
                    method: Ident::new("replace", ProcSpan::call_site()),
                    turbofish: None,
                    paren_token: Default::default(),
                    args: call_args,
                });
                Ok(replace_call)
            }
            // `s.contains(sub)` -> Bool (reuses existing Contains variant
            // — dispatched on Type::String). Wraps `s.contains(sub)`.
            M::Contains if matches!(recv_ty, Type::String) => {
                let sub = one_arg(self)?;
                let sub_ref = coerce_str_arg_to_ref(sub, &args[0]);
                Ok(method_call_one_arg(recv, "contains", sub_ref))
            }
            // `s.len()` -> Int (reuses existing Len variant — dispatched on
            // Type::String). Zero args. Wraps `recv.len() as i64`.
            M::Len if matches!(recv_ty, Type::String) => {
                if !args.is_empty() {
                    return Err(self.unsupported(&format!(
                        "len() takes no arguments, got {}",
                        args.len()
                    )));
                }
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    #recv.len() as i64
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("String.len codegen parse: {e}")))
            }
            // T80: Http response instance methods. Dispatch on
            // `(Int, String)` tuple (the return type of Http.get / post / put / delete).
            //
            // `response.status()` -> Int. Wraps `recv.0` (tuple field access).
            M::ResponseStatus => {
                if !matches!(recv_ty, Type::Tuple(_)) {
                    return Err(self.unsupported(&format!(
                        "status() requires a response tuple, got {recv_ty}"
                    )));
                }
                if !args.is_empty() {
                    return Err(self.unsupported(&format!(
                        "status() takes no arguments, got {}",
                        args.len()
                    )));
                }
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    #recv.0
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Response.status codegen parse: {e}")))
            }
            // `response.body()` -> String. Wraps `recv.1.clone()`.
            M::ResponseBody => {
                if !matches!(recv_ty, Type::Tuple(_)) {
                    return Err(self.unsupported(&format!(
                        "body() requires a response tuple, got {recv_ty}"
                    )));
                }
                if !args.is_empty() {
                    return Err(self.unsupported(&format!(
                        "body() takes no arguments, got {}",
                        args.len()
                    )));
                }
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    #recv.1.clone()
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Response.body codegen parse: {e}")))
            }
            // `response.json()` -> Map<String, Unknown>. Wraps
            // `serde_json::from_str::<HashMap<String, serde_json::Value>>(&recv.1).unwrap_or_default()`.
            M::ResponseJson => {
                if !matches!(recv_ty, Type::Tuple(_)) {
                    return Err(self.unsupported(&format!(
                        "json() requires a response tuple, got {recv_ty}"
                    )));
                }
                if !args.is_empty() {
                    return Err(self.unsupported(&format!(
                        "json() takes no arguments, got {}",
                        args.len()
                    )));
                }
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    serde_json::from_str::<std::collections::HashMap<String, serde_json::Value>>(&#recv.1)
                        .unwrap_or_default()
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Response.json codegen parse: {e}")))
            }
            // `response.headers()` -> Map<String, String>. Returns empty Map
            // (headers not available from body-only response).
            M::ResponseHeaders => {
                if !matches!(recv_ty, Type::Tuple(_)) {
                    return Err(self.unsupported(&format!(
                        "headers() requires a response tuple, got {recv_ty}"
                    )));
                }
                if !args.is_empty() {
                    return Err(self.unsupported(&format!(
                        "headers() takes no arguments, got {}",
                        args.len()
                    )));
                }
                let tokens: proc_macro2::TokenStream = quote::quote! {
                    std::collections::HashMap::<String, String>::new()
                };
                syn::parse2(tokens)
                    .map_err(|e| self.unsupported(&format!("Response.headers codegen parse: {e}")))
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
        let func = FuncDecl {
            name: AstIdent::new("empty", dummy_span()),
            params: Vec::new(),
            return_type: None,
            body: Block::empty(dummy_span()),
            is_async: false,
            is_unsafe: false,
            is_extern: false,
            attributes: Vec::new(),
            type_params: Vec::new(),
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
            type_params: Vec::new(),
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

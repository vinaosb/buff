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
mod analysis;
mod decl_lowering;
mod expr_lowering;
mod lowering_helpers;
mod method_call_lowering;
mod prelude_fns;
mod prelude_lowering;
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
mod extern_crate_registration;
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
        // Analysis pre-passes (ITER-38: moved verbatim to the
        // `rust_codegen/analysis.rs` child module): T42 atomic
        // promotion first, then T41/T42 race detection (its
        // exemption predicate consults the T42 promotions set).
        self.analyze_atomic_promotions(decls);
        self.analyze_parallel_races(decls)?;
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
        // Extern-crate registration (ITER-44: moved verbatim to the
        // `rust_codegen/extern_crate_registration.rs` child module):
        // program-shape detection walks (`program_uses_*`) record
        // every Rust crate the generated program depends on into
        // `self.extern_crates`.
        self.register_extern_crates(decls);
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
        self.analyze_gpu_alignment(decls);
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
            // T53/ITER-53B: a top-level `comptime:` block evaluates at
            // compile time — its `let` bindings become user-visible Rust
            // `const` items, emitted BEFORE any function so every later
            // item can reference them. Multi-item by design (one const
            // per binding), following the ExtendBlock multi-item
            // precedent of extending `items` rather than pushing one.
            if let Decl::ComptimeDecl(d) = decl {
                items.extend(crate::comptime::lower_comptime_decl(d, decls)?);
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

/// T32/ITER-56B: run ONLY the extern-crate registration walkers over a
/// program and return the dependency set - no codegen. This is the cheap
/// companion for callers that already hold generated Rust (the T55
/// cache-hit path) or that aggregate deps alongside
/// [`generate_rust`](self::generate_rust).
#[allow(clippy::items_after_test_module)] // appended at EOF (ITER-56B); relocating mid-file would shuffle ~2.4k lines for style only - mirrors the buff-lang-check lib.rs precedent.
pub fn extern_crates_for(decls: &[Decl]) -> std::collections::BTreeSet<String> {
    let mut codegen = RustCodegen::new();
    codegen.register_extern_crates(decls);
    codegen.extern_crates().clone()
}

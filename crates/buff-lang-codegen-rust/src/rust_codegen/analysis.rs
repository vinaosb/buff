//! ITER-38 - analysis PRE-PASS seam: the race/atomic/gpu_alignment
//! pre-passes of `RustCodegen::generate` (mechanically extracted from
//! rust_codegen.rs).
//!
//! Verbatim move of the generate() pre-pass blocks (T42 atomic
//! promotion + T41/T42 race detection at the top of generate(); T50
//! GPU-bound struct detection right before the main lowering loop)
//! plus the two atomic consultation helpers (`is_atomic_var` /
//! `atomic_initial_value`) into this child module so the parent file
//! shrinks. Every method is pub(super): the only call sites are the
//! parent generate() pre-pass sequence and the parent lower_stmt /
//! lower_expr lowering arms. The analyses themselves live in the
//! crate-level modules (`crate::atomic_analysis`,
//! `crate::race_analysis`, `crate::gpu_alignment`); this seam is the
//! RustCodegen orchestration that runs them BEFORE the main lowering
//! loop and consults their results DURING it. The parent declares
//! only `mod analysis;` (inherent methods resolve by type, no `use`
//! needed). Child inherits parent imports via `use super::*` and may
//! access parent private fields (descendant privacy).

use super::*;

impl RustCodegen {
    /// T42: is `name` an atomic-promotable capture in the function
    /// currently being lowered? Consulted by the `LetDecl`,
    /// `Assignment`, and `Expr::Ident` lowering arms to decide whether
    /// to emit `AtomicI64::new` / `fetch_add` / `load` lowering.
    pub(super) fn is_atomic_var(&self, name: &str) -> bool {
        self.current_atomic_set.contains_key(name)
    }

    /// T42: the integer initial value to which the atomic-promoted
    /// binding was declared (`let mut t = N`). Unused at the call sites
    /// today (we lower the existing initializer expression directly
    /// rather than re-materialising the literal), but kept for
    /// future-proofing and for assertion-style tests.
    #[allow(dead_code)]
    pub(super) fn atomic_initial_value(&self, name: &str) -> Option<i64> {
        self.current_atomic_set.get(name).copied()
    }

    /// T42 atomic-promotion pre-pass (verbatim block moved from the top
    /// of `RustCodegen::generate`).
    pub(super) fn analyze_atomic_promotions(&mut self, decls: &[Decl]) {
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
    }

    /// T41/T42 race-detection pre-pass (verbatim block moved from the
    /// top of `RustCodegen::generate`). Runs after
    /// `analyze_atomic_promotions` so the exemption predicate can
    /// consult the promotions set.
    pub(super) fn analyze_parallel_races(&mut self, decls: &[Decl]) -> Result<(), CodegenError> {
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
        Ok(())
    }

    /// T50 GPU-bound struct pre-pass (verbatim block moved from just
    /// before the main lowering loop of `RustCodegen::generate`).
    pub(super) fn analyze_gpu_alignment(&mut self, decls: &[Decl]) {
        // T50: compute the set of user-defined structs that participate
        // in a parallel combinator pipeline (par_map / par_filter /
        // par_reduce). Must run BEFORE the main lowering loop so
        // `lower_struct_decl` can emit `#[repr(C)]` + bytemuck derives
        // for those structs (GPU-upload-safe layout) without affecting
        // non-GPU-bound structs. Detection rule: closure param type
        // annotation OR struct init inside the parallel closure body.
        // See `gpu_alignment` module docs for the full rationale.
        self.gpu_bound_structs = crate::gpu_alignment::gpu_bound_structs(decls);
    }
}

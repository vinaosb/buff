//! GPU-bound struct detection (T50).
//!
//! When Buff lowers a parallel-combinator call (`par_map` / `par_filter` /
//! `par_reduce`) the runtime may dispatch the closure to a GPU compute
//! shader (T40 decides; T45 executes). For the GPU upload to be sound,
//! the elements flowing through the parallel pipeline MUST have a
//! well-defined memory layout — Rust's default `#[repr(Rust)]` is
//! unspecified, so a `Vec<Point>` uploaded as a raw byte buffer to a
//! `wgpu` storage buffer would expose whatever padding / field-reordering
//! rustc happened to pick. That is undefined behaviour at the GPU side.
//!
//! The fix is mechanical: any user struct that participates in a parallel
//! combinator pipeline is emitted with `#[repr(C)]` (stable C layout —
//! no hidden padding between declared fields, fields in declaration
//! order) plus `#[derive(Copy, bytemuck::Pod, bytemuck::Zeroable)]` so
//! the generated Rust can `bytemuck::cast_slice::<Point, u8>(&vec)` for
//! the GPU upload + readback.
//!
//! ## Detection rule (v1.0)
//!
//! A user-defined struct (declared via `struct Name { ... }`) is
//! **GPU-bound** iff at least ONE of the following holds at any parallel
//! combinator call site in the program:
//!
//! 1. **Closure parameter annotation** — the closure passed to the
//!    `par_*` combinator has a parameter whose [`TypeRef`] names the
//!    struct. This is the "struct flows in" direction:
//!    `v.par_map({ p: Point => p.x })`. Buff rarely requires parameter
//!    type annotations (the inferencer fills them), but when present
//!    they're a definitive signal that the user intends `Point` to be
//!    the element type of `v`.
//!
//! 2. **Struct construction inside the parallel closure body** — the
//!    closure body contains a `Expr::StructInit { type_name: <Name> }`
//!    expression. This is the "struct flows out" direction:
//!    `v.par_map({ x => Point { x: x, y: 0.0 } })`. The closure
//!    produces a fresh struct value per element, so the output
//!    `Vec<Point>` is the GPU-resident collection.
//!
//! Both signals are structural (no type-inference needed) and
//! deterministic (BTreeSet, source-order walk). A struct that fails
//! BOTH checks — including structs used only inside sequential `.map`
//! or `.filter` calls — is left untouched: its codegen is byte-identical
//! to the pre-T50 output (no `#[repr(C)]`, no Pod derive).
//!
//! ## Why not the receiver type?
//!
//! The receiver `v.par_map(...)` carries no type annotation in the AST.
//! Resolving "what is the element type of `v`" requires the full
//! type-inference fixpoint (T12's `TypeInferencer`) plus cross-statement
//! propagation — far beyond the scope of a codegen-time pre-pass.
//! Signals (1) and (2) above cover the same intent without the
//! inference cost: if the user writes `v.par_map({ p: Point => ... })`
//! they're telling us `v` is a `Vec<Point>`; if they write
//! `v.par_map({ x => Point { ... } })` they're telling us the result
//! is. Either way we know `Point` is GPU-bound.
//!
//! ## What this file owns
//!
//! - [`gpu_bound_structs`] — the program-wide detector. Returns a
//!   deterministic [`BTreeSet<String>`] of user-struct names that
//!   flow through a parallel combinator. Consumed by
//!   [`crate::rust_codegen::RustCodegen::generate`] BEFORE per-decl
//!   lowering starts, so [`RustCodegen::lower_struct_decl`] can
//!   consult the set when deciding whether to emit `#[repr(C)]` +
//!   bytemuck derives.
//! - [`is_parallel_combinator`] — exported predicate (mirrors
//!   [`crate::race_analysis::PARALLEL_COMBINATORS`]) so tests + the
//!   walker share a single source of truth.
//!
//! ## Determinism
//!
//! Every collection is a [`BTreeSet`] / [`Vec`] (per the T29 flaky-test
//! lesson — never HashMap/HashSet in codegen-feeding analyses). Walkers
//! visit AST nodes in source order. The same input always yields the
//! same output set; iteration of the set (when consulted) is ascending
//! by struct name, so codegen decisions are reproducible across runs.
//!
//! ## Non-GPU-bound structs are unchanged
//!
//! A struct whose name is NOT in the gpu-bound set is lowered exactly as
//! before T50 — same `#[derive(...)]` list, no `#[repr(C)]`. The
//! existing struct-codegen snapshot tests (T26 / T107) continue to pass
//! byte-identically. Only structs that participate in a parallel
//! combinator see new attributes, and they get a STRICT SUPERSET of the
//! old attribute list (more derives + repr(C)) — never a regression.

use std::collections::BTreeSet;

use buff_lang_ast::{common::Block, decl::FuncDecl, Decl, Expr, Stmt, TypeRef};

/// Re-export the parallel-combinator names so this module is the single
/// source of truth for "is this method a GPU-dispatch trigger?". Mirrors
/// [`crate::race_analysis::PARALLEL_COMBINATORS`] (T41) — duplicated here
/// rather than re-imported so the GPU-alignment module has no upstream
/// dependency on the race detector (the two analyses are conceptually
/// independent: race detection rejects unsafe closures; gpu-alignment
/// decorates structs. Decoupling them keeps each analysis testable in
/// isolation).
const GPU_DISPATCH_COMBINATORS: &[&str] = &["par_map", "par_filter", "par_reduce"];

/// Predicate form of [`GPU_DISPATCH_COMBINATORS`] — is `name` one of the
/// parallel combinators whose closure may be GPU-dispatched?
///
/// Public so tests can verify the predicate without hard-coding the
/// list. Kept in sync with [`crate::race_analysis::PARALLEL_COMBINATORS`]
/// by convention (the two lists MUST match; if they ever diverge,
/// `tests/gpu_alignment_tests.rs::gpu_alignment_combinator_list_matches_race_analysis`
/// will fail).
fn is_parallel_combinator(name: &str) -> bool {
    GPU_DISPATCH_COMBINATORS.contains(&name)
}

/// Detect the set of user-defined struct names that flow through a
/// parallel combinator (`par_map` / `par_filter` / `par_reduce`) in the
/// program `decls`. See the [module docs](self) for the detection rule.
///
/// Returns a [`BTreeSet<String>`] (deterministic; ascending iteration).
/// Empty when the program has no parallel combinators OR no user structs
/// participating in them.
///
/// # Walk order
///
/// Functions are visited in `decls` source order; statements within each
/// function body in source order; sub-expressions depth-first. The
/// returned SET is unordered by construction (BTreeSet sorts on
/// iteration), but the walk's source-order discipline guarantees the
/// same input program always yields the same set.
pub fn gpu_bound_structs(decls: &[Decl]) -> BTreeSet<String> {
    // Build the universe of user-defined struct names. We only ever
    // mark USER structs as gpu-bound — builtin types (Int, Float, etc.)
    // have their own fixed lowering and don't take user attributes.
    let user_structs: BTreeSet<String> = decls
        .iter()
        .filter_map(|d| match d {
            Decl::StructDecl(s) => Some(s.name.name.clone()),
            _ => None,
        })
        .collect();

    let mut found: BTreeSet<String> = BTreeSet::new();
    for decl in decls {
        if let Decl::FuncDecl(func) = decl {
            analyze_func(func, &user_structs, &mut found);
        }
        // Other top-level decls (struct/enum/trait/extend/import/etc.)
        // don't contain executable parallel-combinator call sites in
        // their DECL forms (methods inside extend blocks DO, but T75's
        // extend-block lowering walks them as separate FuncDecls via
        // the per-method lowering — so this outer loop catches them
        // once the AST is in canonical form). Conservative + correct.
    }
    found
}

/// Walk one function body collecting gpu-bound struct names into
/// `found`. Mirrors the walker structure of
/// [`crate::race_analysis::analyze_func`] but collects rather than
/// rejects.
fn analyze_func(func: &FuncDecl, user_structs: &BTreeSet<String>, found: &mut BTreeSet<String>) {
    walk_block(&func.body, false, user_structs, found);
}

/// Walk a [`Block`].
///
/// `in_parallel_closure: true` iff this block is the body of a closure
/// that was passed as an argument to a parallel combinator
/// (`par_map`/`par_filter`/`par_reduce`). When `true`, [`Expr::StructInit`]
/// nodes encountered here trigger Signal 2 (struct construction inside a
/// parallel closure body). When `false`, StructInit nodes do NOT
/// trigger — a top-level `Point { x: 0.0 }` outside any parallel
/// closure is not a GPU-bound signal.
fn walk_block(
    block: &Block,
    in_parallel_closure: bool,
    user_structs: &BTreeSet<String>,
    found: &mut BTreeSet<String>,
) {
    for stmt in &block.stmts {
        walk_stmt(stmt, in_parallel_closure, user_structs, found);
    }
}

fn walk_stmt(
    stmt: &Stmt,
    in_parallel_closure: bool,
    user_structs: &BTreeSet<String>,
    found: &mut BTreeSet<String>,
) {
    use buff_lang_ast::GuardCondition;
    match stmt {
        Stmt::LetDecl { value, .. } => walk_expr(value, in_parallel_closure, user_structs, found),
        Stmt::LetPattern { value, .. } => {
            walk_expr(value, in_parallel_closure, user_structs, found)
        }
        Stmt::Assignment { target, value, .. } => {
            walk_expr(target, in_parallel_closure, user_structs, found);
            walk_expr(value, in_parallel_closure, user_structs, found);
        }
        Stmt::ExprStmt(e, _) => walk_expr(e, in_parallel_closure, user_structs, found),
        Stmt::Return(Some(v), _) => walk_expr(v, in_parallel_closure, user_structs, found),
        Stmt::Return(None, _) | Stmt::Break(_) | Stmt::Continue(_) => {}
        Stmt::ForIn { iter, body, .. } => {
            walk_expr(iter, in_parallel_closure, user_structs, found);
            // The body of a for-loop INSIDE a parallel closure is still
            // part of the parallel closure's body — Signal 2 fires here.
            walk_block(body, in_parallel_closure, user_structs, found);
        }
        Stmt::ForWhile { cond, body, .. } => {
            walk_expr(cond, in_parallel_closure, user_structs, found);
            walk_block(body, in_parallel_closure, user_structs, found);
        }
        Stmt::While { cond, body, .. } => {
            walk_expr(cond, in_parallel_closure, user_structs, found);
            walk_block(body, in_parallel_closure, user_structs, found);
        }
        Stmt::ForLet { value, body, .. } => {
            walk_expr(value, in_parallel_closure, user_structs, found);
            walk_block(body, in_parallel_closure, user_structs, found);
        }
        Stmt::Guard {
            conditions,
            else_block,
            ..
        } => {
            for cond in conditions {
                match cond {
                    GuardCondition::Let { value, .. } => {
                        walk_expr(value, in_parallel_closure, user_structs, found);
                    }
                    GuardCondition::Bool(e) => {
                        walk_expr(e, in_parallel_closure, user_structs, found);
                    }
                }
            }
            walk_block(else_block, in_parallel_closure, user_structs, found);
        }
        Stmt::Defer { expr, .. } => walk_expr(expr, in_parallel_closure, user_structs, found),
        Stmt::ComptimeBlock { .. } => {}
    }
}

fn walk_expr(
    expr: &Expr,
    in_parallel_closure: bool,
    user_structs: &BTreeSet<String>,
    found: &mut BTreeSet<String>,
) {
    match expr {
        Expr::MethodCall {
            receiver,
            method,
            args,
            ..
        } => {
            // If this is a GPU-dispatch combinator, inspect each lambda
            // argument for the two gpu-bound signals. The closure body
            // walk passes `in_parallel_closure = true` so any StructInit
            // inside the body triggers Signal 2.
            //
            // This MUST happen BEFORE the generic recursion into `args`:
            // the call-site context (`par_*`) is what makes the inner
            // struct references gpu-bound.
            if is_parallel_combinator(&method.name) {
                for arg in args {
                    let inner = unwrap_named_arg(arg);
                    if let Expr::Lambda { params, body, .. } = inner {
                        // Signal 1: param type annotation names a user struct.
                        for p in params {
                            collect_named_user_structs(&p.ty, user_structs, found);
                        }
                        // Signal 2: walk the closure body with the
                        // parallel-closure flag ON so StructInit nodes
                        // here trigger collection.
                        walk_block(body, true, user_structs, found);
                    }
                }
            }
            // Recurse regardless — parallel combinators may be chained
            // (e.g. `u.par_filter({...}).par_map({...})`), and the
            // receiver itself may contain more parallel calls. The
            // outer recursion passes `in_parallel_closure` unchanged
            // EXCEPT when entering a non-parallel Lambda body (see
            // below — that resets the flag, since a closure that's not
            // a par_* arg is NOT gpu-bound-on-its-own).
            walk_expr(receiver, in_parallel_closure, user_structs, found);
            for arg in args {
                walk_expr(arg, in_parallel_closure, user_structs, found);
            }
        }
        Expr::BinaryOp { lhs, rhs, .. } => {
            walk_expr(lhs, in_parallel_closure, user_structs, found);
            walk_expr(rhs, in_parallel_closure, user_structs, found);
        }
        Expr::UnaryOp { operand, .. } => {
            walk_expr(operand, in_parallel_closure, user_structs, found)
        }
        Expr::FuncCall { callee, args, .. } => {
            walk_expr(callee, in_parallel_closure, user_structs, found);
            for a in args {
                walk_expr(a, in_parallel_closure, user_structs, found);
            }
        }
        Expr::IfExpr {
            cond,
            then_block,
            else_block,
            ..
        } => {
            walk_expr(cond, in_parallel_closure, user_structs, found);
            walk_block(then_block, in_parallel_closure, user_structs, found);
            if let Some(eb) = else_block {
                walk_block(eb, in_parallel_closure, user_structs, found);
            }
        }
        // A Lambda OUTSIDE a parallel-combinator argument position is
        // not gpu-bound on its own — the gpu-bound signal is "this
        // closure is the arg to a par_* call". Entering its body
        // RESETS the parallel-closure flag (a nested closure inside a
        // par_* closure body is NOT itself a parallel closure unless
        // IT is also passed to a par_* call). However, we still
        // recurse into its body in case the closure itself contains a
        // nested par_* call (rare but possible: a closure that does
        // its own parallel dispatch internally).
        Expr::Lambda { body, .. } => walk_block(body, false, user_structs, found),
        Expr::StructInit {
            type_name, fields, ..
        } => {
            // Signal 2: struct construction INSIDE a parallel closure
            // body. The `in_parallel_closure` flag is set by the
            // MethodCall arm's `walk_block(body, true, ...)` call when
            // it dispatches into a par_* closure body. Top-level
            // StructInit (or StructInit inside a non-parallel closure)
            // does NOT trigger — `in_parallel_closure` is false there.
            if in_parallel_closure && user_structs.contains(&type_name.name) {
                found.insert(type_name.name.clone());
            }
            for (_, v) in fields {
                walk_expr(v, in_parallel_closure, user_structs, found);
            }
        }
        Expr::MatchExpr {
            scrutinee, arms, ..
        } => {
            walk_expr(scrutinee, in_parallel_closure, user_structs, found);
            for arm in arms {
                walk_block(&arm.body, in_parallel_closure, user_structs, found);
            }
        }
        Expr::SuspendExpr { inner, .. } => {
            walk_expr(inner, in_parallel_closure, user_structs, found)
        }
        Expr::ArrayLit { elements, .. } => {
            for e in elements {
                walk_expr(e, in_parallel_closure, user_structs, found);
            }
        }
        Expr::Index { base, indices, .. } => {
            walk_expr(base, in_parallel_closure, user_structs, found);
            for i in indices {
                walk_expr(i, in_parallel_closure, user_structs, found);
            }
        }
        Expr::StringInterp { parts, .. } => {
            for part in parts {
                if let buff_lang_ast::InterpPart::Expr(e, _) = part {
                    walk_expr(e, in_parallel_closure, user_structs, found);
                }
            }
        }
        Expr::MapLit { entries, .. } => {
            for (k, v) in entries {
                walk_expr(k, in_parallel_closure, user_structs, found);
                walk_expr(v, in_parallel_closure, user_structs, found);
            }
        }
        Expr::Try { expr, .. } => walk_expr(expr, in_parallel_closure, user_structs, found),
        Expr::Spawn { task, .. } => walk_expr(task, in_parallel_closure, user_structs, found),
        Expr::Range { start, end, .. } => {
            walk_expr(start, in_parallel_closure, user_structs, found);
            walk_expr(end, in_parallel_closure, user_structs, found);
        }
        Expr::IfLet {
            value,
            then_block,
            else_block,
            ..
        } => {
            walk_expr(value, in_parallel_closure, user_structs, found);
            walk_block(then_block, in_parallel_closure, user_structs, found);
            if let Some(eb) = else_block {
                walk_block(eb, in_parallel_closure, user_structs, found);
            }
        }
        Expr::TupleLit(members, _) => {
            for m in members {
                walk_expr(m, in_parallel_closure, user_structs, found);
            }
        }
        Expr::NamedArg { value, .. } => walk_expr(value, in_parallel_closure, user_structs, found),
        // Leaves — no recursion needed.
        Expr::Literal(_, _) | Expr::Ident(_, _) => {}
    }
}

/// Walk a [`TypeRef`] tree collecting every [`TypeRef::Named`] whose
/// `name` matches a user-defined struct. Used on closure-param type
/// annotations (Signal 1).
///
/// Recurses into generic args, Option inner, Tuple members, Union
/// members, and Function params/return — a struct name embedded
/// anywhere in a param's annotation counts (e.g. `Vector<Point>` →
/// `Point` is gpu-bound because the user wrote it in the annotation).
fn collect_named_user_structs(
    ty: &TypeRef,
    user_structs: &BTreeSet<String>,
    found: &mut BTreeSet<String>,
) {
    match ty {
        TypeRef::Named { name, .. } => {
            if user_structs.contains(&name.name) {
                found.insert(name.name.clone());
            }
        }
        TypeRef::Generic { base, args, .. } => {
            collect_named_user_structs(base, user_structs, found);
            for a in args {
                collect_named_user_structs(a, user_structs, found);
            }
        }
        TypeRef::Option(inner, _) => collect_named_user_structs(inner, user_structs, found),
        TypeRef::Function {
            params,
            return_type,
            ..
        } => {
            for p in params {
                collect_named_user_structs(p, user_structs, found);
            }
            collect_named_user_structs(return_type, user_structs, found);
        }
        // DR-020 / P2.1a: trait objects name a trait, not a user struct.
        // No recursion needed — the trait_name does not refer to a struct
        // that could be GPU-aligned. The boxed-dyn representation is also
        // fundamentally incompatible with #[repr(C)] + Pod/Zeroable.
        TypeRef::TraitObject { .. } => {}
        TypeRef::Union(members, _) => {
            for m in members {
                collect_named_user_structs(m, user_structs, found);
            }
        }
        TypeRef::Tuple(members, _) => {
            for m in members {
                collect_named_user_structs(m, user_structs, found);
            }
        }
    }
}

/// A `name: value` named arg may wrap a closure. The detector needs to
/// see THROUGH the NamedArg wrapper to find the underlying Lambda.
/// Mirrors [`crate::race_analysis::unwrap_named_arg`].
fn unwrap_named_arg(arg: &Expr) -> &Expr {
    match arg {
        Expr::NamedArg { value, .. } => value,
        other => other,
    }
}

// =====================================================================
// Inline smoke tests — exercise the detector on synthesised ASTs so a
// quick `cargo test -p buff-lang-codegen-rust --lib gpu_alignment`
// catches regressions without spinning up an integration binary.
// Full behavioural coverage lives in
// `tests/gpu_alignment_tests.rs` (12+ integration tests).
// =====================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use buff_lang_ast::common::{Block, Ident, Param};
    use buff_lang_ast::decl::{FuncDecl, StructDecl};
    use buff_lang_ast::Literal;
    use buff_lang_error::Span;

    fn span() -> Span {
        Span::dummy()
    }
    fn ident(s: &str) -> Ident {
        Ident::new(s, span())
    }
    fn ident_expr(s: &str) -> Expr {
        Expr::Ident(ident(s), span())
    }
    fn named_ty(s: &str) -> TypeRef {
        TypeRef::Named {
            name: ident(s),
            span: span(),
        }
    }
    fn placeholder_ty() -> TypeRef {
        named_ty("_")
    }
    fn struct_decl(name: &str, fields: Vec<(&str, &str)>) -> Decl {
        Decl::StructDecl(StructDecl {
            name: ident(name),
            fields: fields
                .into_iter()
                .map(|(n, t)| (ident(n), named_ty(t)))
                .collect(),
            traits: Vec::new(),
            type_params: Vec::new(),
            span: span(),
        })
    }
    fn empty_func_with_stmts(name: &str, stmts: Vec<Stmt>) -> Decl {
        Decl::FuncDecl(FuncDecl {
            name: ident(name),
            params: Vec::new(),
            return_type: None,
            body: Block {
                stmts,
                span: span(),
            },
            is_async: false,
            is_unsafe: false,
            is_extern: false,
            attributes: Vec::new(),
            type_params: Vec::new(),
            span: span(),
        })
    }
    fn closure_with_typed_param(
        param_name: &str,
        param_ty: TypeRef,
        body_stmts: Vec<Stmt>,
    ) -> Expr {
        Expr::Lambda {
            params: vec![Param {
                name: ident(param_name),
                ty: param_ty,
                default_value: None,
                is_comptime: false,
                span: span(),
            }],
            body: Block {
                stmts: body_stmts,
                span: span(),
            },
            return_type: None,
            span: span(),
        }
    }
    fn method_call(receiver: Expr, method: &str, args: Vec<Expr>) -> Expr {
        Expr::MethodCall {
            receiver: Box::new(receiver),
            method: ident(method),
            args,
            span: span(),
        }
    }
    fn expr_stmt(e: Expr) -> Stmt {
        Stmt::ExprStmt(e, span())
    }
    fn struct_init(type_name: &str, fields: Vec<(&str, Expr)>) -> Expr {
        Expr::StructInit {
            type_name: ident(type_name),
            fields: fields.into_iter().map(|(n, v)| (ident(n), v)).collect(),
            span: span(),
        }
    }

    #[test]
    fn gpu_alignment_combinator_predicate_recognises_par_combinators() {
        assert!(is_parallel_combinator("par_map"));
        assert!(is_parallel_combinator("par_filter"));
        assert!(is_parallel_combinator("par_reduce"));
        assert!(!is_parallel_combinator("map"));
        assert!(!is_parallel_combinator("filter"));
        assert!(!is_parallel_combinator("reduce"));
    }

    #[test]
    fn gpu_alignment_detector_finds_struct_via_closure_param_annotation() {
        // struct Point { x: Float, y: Float }
        // func f() {
        //   v.par_map({ p: Point => p.x })
        // }
        let point = struct_decl("Point", vec![("x", "Float"), ("y", "Float")]);
        let closure =
            closure_with_typed_param("p", named_ty("Point"), vec![expr_stmt(ident_expr("p"))]);
        let body = vec![expr_stmt(method_call(
            ident_expr("v"),
            "par_map",
            vec![closure],
        ))];
        let func = empty_func_with_stmts("f", body);

        let found = gpu_bound_structs(&[point, func]);
        assert!(found.contains("Point"));
    }

    #[test]
    fn gpu_alignment_detector_finds_struct_via_struct_init_in_closure_body() {
        // struct Point { x: Float, y: Float }
        // func f() {
        //   v.par_map({ x => Point { x: x, y: 0.0 } })
        // }
        let point = struct_decl("Point", vec![("x", "Float"), ("y", "Float")]);
        let closure = closure_with_typed_param(
            "x",
            placeholder_ty(),
            vec![expr_stmt(struct_init(
                "Point",
                vec![
                    ("x", ident_expr("x")),
                    ("y", Expr::Literal(Literal::Float(0.0), span())),
                ],
            ))],
        );
        let body = vec![expr_stmt(method_call(
            ident_expr("v"),
            "par_map",
            vec![closure],
        ))];
        let func = empty_func_with_stmts("f", body);

        let found = gpu_bound_structs(&[point, func]);
        assert!(found.contains("Point"));
    }

    #[test]
    fn gpu_alignment_detector_sequential_map_does_not_trigger() {
        // struct Point { x: Float }
        // func f() {
        //   v.map({ p: Point => p.x })    // sequential .map — NOT gpu-bound
        // }
        let point = struct_decl("Point", vec![("x", "Float")]);
        let closure =
            closure_with_typed_param("p", named_ty("Point"), vec![expr_stmt(ident_expr("p"))]);
        let body = vec![expr_stmt(method_call(
            ident_expr("v"),
            "map",
            vec![closure],
        ))];
        let func = empty_func_with_stmts("f", body);

        let found = gpu_bound_structs(&[point, func]);
        assert!(
            !found.contains("Point"),
            "sequential .map must NOT trigger gpu-bound detection; got {found:?}"
        );
    }

    #[test]
    fn gpu_alignment_detector_empty_when_no_parallel_calls() {
        let point = struct_decl("Point", vec![("x", "Float")]);
        let func = empty_func_with_stmts("f", vec![expr_stmt(ident_expr("v"))]);
        let found = gpu_bound_structs(&[point, func]);
        assert!(found.is_empty());
    }
}

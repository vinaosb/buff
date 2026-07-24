//! T105a - named-arg/default/extern-fn dependency collectors (mechanically extracted from rust_codegen.rs).
//!
//! Verbatim move - no logic changes. Child module of rust_codegen so it
//! inherits the parent imports via use super::* (zero per-module import lists).
//! Functions are pub(super) so the parent reaches them through the glob below.

use super::*;


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
pub(super) fn collect_func_param_names(decls: &[Decl]) -> BTreeMap<String, Vec<String>> {
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

/// T85: collect the registry of USER-defined enum variants in this
/// compilation unit (T85 — fixes the bare-`Red`-vs-`Color::Red` bug).
///
/// Returns a [`BTreeMap`] keyed by VARIANT NAME with the owning ENUM NAME
/// as the value. The map is consulted by `lower_expr` / `lower_pattern`
/// so that bare variant references (`Red`) lower to the qualified Rust
/// path (`Color::Red`) — without this, rustc treats a bare `Red` in a
/// match arm as a fresh binding pattern and a bare `Red` in expression
/// position as an unresolved identifier.
///
/// # Exclusion rules
///
/// - **Prelude enums** (`Option`, `Result`): their variants
///   (`Some`/`None`/`Ok`/`Err`) live in the Rust prelude and MUST stay
///   unqualified (`Some(5)` not `Option::Some(5)`). The user can still
///   declare an `enum Option` / `enum Result` (it shadows the prelude
///   within their crate) but Buff stays out of the way — prelude-name
///   shadowing is a footgun the user opted into.
/// - **Collisions**: if the SAME variant name appears in two or more
///   user-defined enums (e.g. `enum A { X }` and `enum B { X }`), the
///   entry is REMOVED from the map. An ambiguous bare reference cannot
///   be auto-qualified; rustc will produce the right "ambiguous" error
///   and the user must write `A::X` / `B::X` explicitly in Buff source.
///
/// Determinism: a [`BTreeMap`] (not [`HashMap`]) is used so iteration
/// order is independent of hash seed (the T29 flaky-test lesson).
pub(super) fn collect_user_enum_variants(decls: &[Decl]) -> BTreeMap<String, String> {
    // Rust prelude enums whose variants must NOT be auto-qualified.
    const PRELUDE_ENUMS: &[&str] = &["Option", "Result"];
    // Two-pass: first collect (variant -> enum) pairs into a Vec so we
    // can detect collisions; then fold into the BTreeMap, removing any
    // key that appears more than once. A Vec-of-tuples (not a HashMap)
    // keeps the input order deterministic for the collision scan.
    let mut pairs: Vec<(String, String)> = Vec::new();
    for decl in decls {
        if let Decl::EnumDecl(e) = decl {
            if PRELUDE_ENUMS.contains(&e.name.name.as_str()) {
                continue;
            }
            for v in &e.variants {
                pairs.push((v.name.name.clone(), e.name.name.clone()));
            }
        }
    }
    let mut out: BTreeMap<String, String> = BTreeMap::new();
    for (variant, enum_name) in &pairs {
        out.insert(variant.clone(), enum_name.clone());
    }
    // Collision removal: any variant name appearing in 2+ distinct enums
    // is ambiguous and must not be auto-qualified.
    let mut collisions: BTreeSet<String> = BTreeSet::new();
    for (i, (v1, e1)) in pairs.iter().enumerate() {
        for (v2, e2) in pairs.iter().skip(i + 1) {
            if v1 == v2 && e1 != e2 {
                collisions.insert(v1.clone());
            }
        }
    }
    for c in &collisions {
        out.remove(c);
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
pub(super) fn materialize_named_args(args: &[Expr], params: Option<&[String]>) -> Vec<Expr> {
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
pub(super) fn collect_func_param_defaults(decls: &[Decl]) -> BTreeMap<String, Vec<Option<Expr>>> {
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
pub(super) fn collect_extern_fn_names(decls: &[Decl]) -> BTreeSet<String> {
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
pub(super) fn fill_default_args(args: &[Expr], defaults: &[Option<Expr>]) -> Option<Vec<Expr>> {
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


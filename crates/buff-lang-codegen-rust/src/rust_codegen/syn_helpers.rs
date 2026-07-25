//! T105a - syn-construction helpers (mechanically extracted from rust_codegen.rs).
//!
//! Leaf helpers that build syn AST fragments: identifiers, two-segment
//! enum paths, method-call expressions, attribute wrappers, atomic/Arc
//! scaffolding, and the KNOWN_ZERO_ARG_METHODS table. Verbatim move -
//! no logic changes. Child module of rust_codegen so it inherits the
//! parent imports via use super::* (zero per-module import lists).

use super::*;

// ---------------------------------------------------------------------------
// Free helpers
// ---------------------------------------------------------------------------

/// Convert a Buff [`buff_lang_ast::Ident`] into a `syn::Ident`. The byte offsets
/// in the Buff span don't carry over (proc-macro2 spans are opaque), so we
/// just use `call_site` here. The source-map mapping (Buff span → Rust
/// line/col) is recorded separately in [`CodegenContext`].
pub(super) fn ast_ident_to_syn(ident: &buff_lang_ast::common::Ident) -> Ident {
    Ident::new(&ident.name, ProcSpan::call_site())
}

/// T85: build a two-segment `EnumName::VariantName` [`syn::Path`].
///
/// Used by [`RustCodegen::lower_expr`] (the `Expr::Ident` arm) and
/// [`RustCodegen::lower_pattern`] (the `Pattern::Ident` /
/// `Pattern::Variant` arms) to qualify bare user-defined enum variant
/// references. The path is built explicitly (not via `parse_quote!`,
/// which is banned in non-test code per the crate's hard rules).
///
/// Determinism: the path is built from two [`Ident`]s joined by a single
/// `::` separator — byte-identical for the same `(enum, variant)` pair.
pub(super) fn two_segment_path(enum_name: &str, variant_name: &str) -> syn::Path {
    syn::Path {
        leading_colon: None,
        segments: std::iter::once(syn::PathSegment {
            ident: Ident::new(enum_name, ProcSpan::call_site()),
            arguments: syn::PathArguments::None,
        })
        .chain(std::iter::once(syn::PathSegment {
            ident: Ident::new(variant_name, ProcSpan::call_site()),
            arguments: syn::PathArguments::None,
        }))
        .collect(),
    }
}

/// T85: build a two-segment `EnumName::VariantName` [`SynExpr::Path`].
///
/// Thin wrapper around [`two_segment_path`] that wraps the path in an
/// [`syn::ExprPath`] for use as a Rust expression. Used by
/// [`RustCodegen::lower_expr`]'s `Expr::Ident` arm.
pub(super) fn two_segment_path_expr(enum_name: &str, variant_name: &str) -> SynExpr {
    SynExpr::Path(syn::ExprPath {
        attrs: Vec::new(),
        qself: None,
        path: two_segment_path(enum_name, variant_name),
    })
}

/// T86: strip the trailing `;` on the LAST statement of a [`syn::Block`]
/// iff that statement is a [`SynStmt::Expr`] with `Some(semi)`.
///
/// Used by [`RustCodegen::lower_match_expr`] when the match is in return
/// position: the parser wraps each arm body as a one-statement
/// `Block { stmts: [Stmt::ExprStmt(e)] }`, which [`Self::lower_block`]
/// (correctly, for general blocks) emits as `{ e; }` (statement with
/// semi → block type `()`). For an arm body that must YIELD a value
/// (so the surrounding `return match n { ... }` typechecks against the
/// function's declared return type), we strip the trailing `;` on the
/// last expression-statement so it becomes a tail expression
/// (`{ e }` → block yields the value of `e`).
///
/// NO-OP when:
/// - the block is empty (`{}`),
/// - the last statement is not a [`SynStmt::Expr`] (e.g. it's a
///   `Local` let-binding or a Semi with `None` already),
/// - the last statement is already a tail expression (`None` semi).
///
/// Only the LAST statement is touched — interior statements keep their
/// semis (they MUST be statements; only the tail position can be an
/// expression in Rust).
pub(super) fn strip_trailing_semi_on_last_expr_stmt(block: &mut syn::Block) {
    let Some(last) = block.stmts.last_mut() else {
        return;
    };
    if let SynStmt::Expr(_, semi) = last {
        *semi = None;
    }
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
pub(super) fn rewrite_self_receiver(mut sig: Signature) -> Signature {
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
pub(super) fn ident_expr_from_fn_arg(arg: &syn::FnArg) -> Option<SynExpr> {
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
pub(super) fn field_method_call_expr(
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
pub(super) fn build_record_copy_method(field_name: &str, field_ty: SynType) -> syn::ImplItemFn {
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
        // T124d: Regex prelude type. Plain `regex::Regex` path; no generic
        // argument. Generated code uses the fully-qualified path so no
        // `use` import is needed (mirrors the chrono family pattern).
        "Regex" => "regex::Regex",
        other => other,
    }
}

/// Build a `syn::Type::Path` from a `::`-separated Rust type name string
/// (e.g. `"i64"`, `"bool"`, `"rust_decimal::Decimal"`). Each `::`-separated
/// segment becomes a [`syn::PathSegment`]. The result is always a plain path
/// with no generic arguments.
pub(super) fn rust_path_type(name: &str) -> SynType {
    SynType::Path(syn::TypePath {
        qself: None,
        path: rust_path(name),
    })
}

/// Build a `syn::Path` from a `::`-separated name string
/// (e.g. `"rust_decimal_macros::dec"`). Used for macro paths like the
/// `dec!(...)` codegen in T20.
pub(super) fn rust_path(name: &str) -> syn::Path {
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
pub(super) fn make_println_macro(arg: SynExpr) -> SynExpr {
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
pub(super) fn make_println_macro_literal(text: &str) -> SynExpr {
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
pub(super) fn method_call_no_args(recv: SynExpr, method: &str) -> SynExpr {
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
pub(super) fn wrap_in_parens(e: SynExpr) -> SynExpr {
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
pub(super) fn field_access(base: SynExpr, field: &str) -> SynExpr {
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
pub(super) fn make_await(base: SynExpr) -> SynExpr {
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
pub(super) fn wrap_in_arc_new(inner: SynExpr) -> SynExpr {
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
pub(super) fn wrap_in_atomic_i64_new(inner: SynExpr) -> SynExpr {
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
pub(super) fn atomic_fetch_add_stmt(name: &buff_lang_ast::common::Ident, rhs: SynExpr) -> SynExpr {
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
pub(super) fn atomic_load_expr(atomic_path: SynExpr) -> SynExpr {
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
pub(super) fn arc_clone_call(name: &buff_lang_ast::common::Ident) -> SynExpr {
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
pub(super) fn arc_make_mut_deref(name: &buff_lang_ast::common::Ident) -> SynExpr {
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
pub(super) const KNOWN_ZERO_ARG_METHODS: &[&str] = &[
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
    // T124f: Vector zero-arg instance method `sort()`. Without this
    // entry the T26 field-access heuristic would rewrite `vec.sort()`
    // as `vec.sort` (a field access on the Vec, which doesn't exist).
    // `sort_by` takes one arg so it's never affected by the heuristic.
    "sort",
    // T124h: URL zero-arg instance accessors (`scheme` / `host` /
    // `path`). Without these entries the T26 field-access heuristic
    // would rewrite `url.scheme` as a Rust field access on the
    // `url::Url` value (which doesn't exist - the underlying Rust
    // methods are `.scheme()` / `.host_str()` / `.path()`).
    // `query` takes one arg so it's never affected by the heuristic.
    "scheme",
    "host",
    "path",
    // T124j: Path zero-arg instance methods (`parent` / `extension`
    // / `basename` / `exists`). Without these entries the T26
    // field-access heuristic would rewrite `path.parent()` as a
    // Rust field access on the `std::path::PathBuf` value (which
    // doesn't exist - the underlying Rust methods are `.parent()` /
    // `.extension()` / `.file_name()` / `.exists()`).
    "parent",
    "extension",
    "basename",
    "exists",
    // T124l: Process zero-arg instance methods (`wait` / `id`).
    // Without these entries the T26 field-access heuristic would
    // rewrite `process.wait()` as a Rust field access on the
    // `Option<std::process::Child>` value (which doesn't exist -
    // the underlying Rust methods are `.wait()` / `.id()` on the
    // inner `Child`, accessed via the Option's `.map(...)`).
    "wait",
    "id",
    // T124m: Networking zero-arg instance methods (`recv` /
    // `close` / `recv_from`). Without these entries the T26
    // field-access heuristic would rewrite `conn.recv()` /
    // `conn.close()` / `sock.recv_from()` as Rust field accesses
    // on the `Option<tokio::net::*>` values (which don't exist -
    // the underlying Rust methods are async `recv()` / `close()`
    // / `recv_from()` on the inner TcpStream / UdpSocket /
    // WebSocketStream, accessed via `if let Some(mut s) = ...`).
    // `send` / `send_to` take args so they're never affected by
    // the heuristic.
    "recv",
    "close",
    "recv_from",
    // T7: DataFrame zero-arg instance method `to_table_string()`.
    // Without this entry the T26 field-access heuristic would rewrite
    // `df.to_table_string()` as `df.to_table_string` (a Rust field
    // access on the `buff_dataframe::DataFrame` value, which doesn't
    // exist - the underlying Rust method is `.to_table_string()`
    // returning a fixed-width formatted String). The other DataFrame
    // methods (`select`/`filter`/`sort`/`head`/`join`/`group_by`/
    // `agg`) all take args so they're never affected by the heuristic;
    // `len` is already covered above as a universal collection method.
    "to_table_string",
    // T9: Image zero-arg instance methods (`width` / `height` /
    // `pixel_format` / `grayscale` / `invert`). Without these entries
    // the T26 field-access heuristic would rewrite `img.width()` as
    // `img.width` (a field access on the `buff_image::Image` value,
    // which doesn't exist - the underlying Rust methods are `.width()`
    // / `.height()` / `.format()` / `.grayscale()` / `.invert()` on
    // the inner `image::DynamicImage`). `get_pixel` / `set_pixel` /
    // `save` / `resize` / `crop` / `blur` all take args so they're
    // never affected by the heuristic.
    "width",
    "height",
    "pixel_format",
    "grayscale",
    "invert",
    // T10: AudioBuffer zero-arg instance methods (`samples` /
    // `sample_rate` / `channels` / `frames` / `duration_secs` /
    // `summarize`). Without these entries the T26 field-access
    // heuristic would rewrite `buf.samples()` as `buf.samples` (a
    // field access on the `buff_audio::AudioBuffer` value, which
    // doesn't exist - the underlying Rust methods are `.samples()` /
    // `.sample_rate()` / `.channels()` / `.frames()` /
    // `.duration_secs()` / `.summarize()`). `save` / `amplify` /
    // `normalize` / `mix` / `slice` all take args so they're never
    // affected by the heuristic.
    "samples",
    "sample_rate",
    "channels",
    "frames",
    "duration_secs",
    "summarize",
    // T71: Lazy iterator zero-arg methods (`lazy` / `collect` / `count`).
    // Without these entries the T26 field-access heuristic would rewrite
    // `vec.lazy()` / `iter.collect()` / `iter.count()` as Rust field
    // accesses (which don't exist on Vec / iterator adapters).
    "lazy",
    "collect",
    "count",
];

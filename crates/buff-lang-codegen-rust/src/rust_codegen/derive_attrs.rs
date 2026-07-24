//! T105a - derive/repr attribute builders (mechanically extracted from rust_codegen.rs).
//!
//! Verbatim move - no logic changes. Child module of rust_codegen so it
//! inherits the parent imports via use super::* (zero per-module import lists).
//! Functions are pub(super) so the parent reaches them through the glob below.

use super::*;


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
pub(super) fn derive_and_repr_attrs(emit_repr_c: bool) -> Vec<syn::Attribute> {
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
pub(super) fn struct_derive_attrs(emit_repr_c: bool, include_hash: bool) -> Vec<syn::Attribute> {
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
pub(super) fn push_repr_c_attr(attrs: &mut Vec<syn::Attribute>) {
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
pub(super) fn gpu_struct_derive_attrs() -> Vec<syn::Attribute> {
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
pub(super) fn type_is_hash_safe(ty: &TypeRef, hash_safe_user_structs: &BTreeSet<String>) -> bool {
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


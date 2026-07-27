# DR-020: dyn Trait Language Extension

**Date:** 2026-07-27
**Status:** ACCEPTED
**Supersedes:** None
**Governing authority:** T110 (`buff-direction-speed-moat-selfhost.md`)
**Originating spike:** S1 (`spike-multi-dispatch.md`, verdict `NEEDS_DYN_TRAIT`)
**Blocks:** P2.1a-e (self-host-completion-roadmap Phase 2 implementation)

---

## Context

The 10 target crates identified in DR-014 for self-host-frontend porting
have minimal trait-object usage: 2 cases total across all 10 crates,
both refactorable to generics or eliminable by design. On paper, the
existing `multi_dispatch` feature (T58, shipped v1.19) should be
sufficient.

However, the S1 spike (`spike-multi-dispatch.md`) revealed that
multi_dispatch is **non-functional end-to-end**. The dispatcher exists
and is consulted by `buff check`, but the codegen layer in
`buff-lang-codegen-rust` has zero references to `MultiDispatchTable`.
All 5 multi-dispatch examples in `examples/` fail with rustc `E0428`
(duplicate definition) when run via `buff run`. T58 was declared shipped
in v1.19 but the codegen mangling pass never landed.

Beyond the bug, multi_dispatch has a fundamental limitation: it is
compile-time only and cannot express heterogeneous collections
(`Vec<Box<dyn Trait>>`). While the 10 target crates today contain zero
such collections, the porting plan should not assume zero future demand.

Three independent reasons from S1 section 5 drive this decision, each
sufficient on its own:

1. **multi_dispatch codegen is broken.** Relying on it for the port
   would require a codegen fix first. `dyn Trait` is a cleaner,
   single-feature addition.
2. **multi_dispatch fundamentally cannot express heterogeneous
   collections.** `dyn Trait` covers both single-callback and
   heterogeneous-collection cases uniformly.
3. **Direct port is cheaper than refactor.** Even if multi_dispatch
   worked perfectly, adding `dyn Trait` costs one language extension
   plus one vtable-aware codegen pass, and it eliminates fragile
   refactoring of callback patterns. The port becomes mechanical
   translation instead of semantic redesign.

## Decision

Add `dyn Trait` to Buff as a language extension, consuming extension
cap slot 1 of max 3.

## Syntax

Two forms, matching Rust:

- `Box<dyn Trait>` -- owned trait object (heap-allocated)
- `&dyn Trait` -- borrowed trait object (reference)

`dyn` is **not** a reserved keyword. It lexes as `Ident` and is
recognized contextually in type position. This avoids breaking any
existing Buff code that uses `dyn` as a variable name, per the
Stability Promise.

## Type System Foundation

**Key reuse: no new `Type` variant needed.**

`Type::DynamicDispatch(Box<Type>)` already exists at
`crates/buff-lang-types/src/ty.rs:1192` (T68, shipped v1.19), with a
constructor at line 1424 (`Type::dynamic_dispatch(trait_ty)`) and a
`Display` impl at line 2735 that formats as `Box<dyn {trait_ty}>`.

What P2.1a will add is a new **AST-level** type reference variant:

```
TypeRef::TraitObject { trait_name: Ident, lifetime: Option<String>, span: Span }
```

This goes in `crates/buff-lang-ast/src/ty.rs`. The mapping from AST to
Type (P2.1c) is straightforward:

```
TypeRef::TraitObject { trait_name, .. }
  -> Type::DynamicDispatch(Box::new(Type::User { name: trait_name, args: vec![] }))
```

## Codegen Approach

**Key reuse: the lowering already exists.**

`buff_type_to_syn()` in
`crates/buff-lang-codegen-rust/src/rust_codegen/type_lowering.rs:240`
already handles `Type::DynamicDispatch`:

```rust
Type::DynamicDispatch(trait_ty) => {
    let inner = self.buff_type_to_syn(trait_ty)?;
    let tokens: proc_macro2::TokenStream = quote::quote! {
        Box<dyn #inner>
    };
    // ...
}
```

This emits Rust `Box<dyn Trait>` via `quote!`. Zero transformation
needed, since Rust natively supports `dyn Trait`.

What P2.1c will add is a parallel arm in `ast_typeref_to_syn()` for
`TypeRef::TraitObject` (the direct AST-to-syn path that bypasses the
Type intermediate when the parser produces a trait-object type
annotation directly).

## Prelude Implications

None for MVP. Existing prelude functions don't need trait-object
overloads. Buff's native `Error` type can be used instead of
`std::error::Error`, which eliminates one of the 2 trait-object usages
found in the target crates (the `fn source()` return type in
`buff-lang-error/src/error.rs:91`).

## Autoboxing Rules

- **Owned form** (`Box<dyn Trait>`): the user explicitly writes the
  `Box`. No implicit autoboxing.
- **Borrowed form** (`&dyn Trait`): only allowed as a function parameter
  type, not as a `let`-binding type. This is an MVP simplicity
  restriction to avoid lifetime-inference complexity in local
  bindings.

## Extension Cap Slot 1 Usage

This DR consumes extension cap slot 1. The current state from
`.sisyphus/evidence/extensions-counter.json` is `used: 0, max: 3`.

Slot 1 was previously "reserved" for `multiple_dispatch_test` (per S1
status). That reservation is superseded by this DR. multi_dispatch is
now confirmed non-functional per S1 section 4, so reserving a slot for
its test infrastructure no longer makes sense. The counter update
(`used: 0 -> 1`) happens atomically in P2.1a alongside the AST
change.

After this DR: 1 of 3 slots used, 2 remaining for future needs.

## Error Codes

Two new error codes, assigned in the E12xx (type) range. These are
**STABLE FOREVER** per conventions section 19: no renumbering, reusing,
or silently removing.

| Code | Name | Trigger |
|---|---|---|
| `E1213` | `TraitObjectUndefinedType` | `dyn` references a name that is not a known trait |
| `E1214` | `TraitObjectUnsupportedLifetime` | Lifetime annotation other than `'static` (MVP restriction) |

## Alternatives Considered

### 1. multi_dispatch only (REJECTED)

Non-functional end-to-end per S1 section 4. The codegen layer has zero
integration with the dispatcher; all 5 multi-dispatch examples fail
with E0428. Even if fixed, multi_dispatch cannot express heterogeneous
collections.

### 2. Generics + closures (REJECTED)

Would require redesigning trait hierarchies as free-function groups,
which is a fragile refactor rather than a mechanical port. The 2
callback sites in the target crates would need manual rewriting, and
the approach wouldn't cover heterogeneous collections at all.

### 3. C ABI shims (REJECTED)

Would require writing `#[no_mangle] extern "C"` wrappers for every trait
method. Massive complexity for minimal gain, and it violates Buff's
"no C library" rule (see T119 extern ABI policy).

### 4. dyn Trait (ACCEPTED)

Cleanest, smallest surface area: one `TypeRef` variant in the AST, one
parser recognition rule (contextual `dyn` in type position), one
`typeref_to_type()` arm, one codegen arm (already exists). Direct port
becomes mechanical translation from Rust syntax to Buff syntax.

## Consequences

### Positive

- The self-host port becomes mechanical (Rust syntax to Buff syntax)
  for the 10 target crates in DR-014.
- Both actual trait-object usages port directly: `LexCallback`
  callback in the lexer, and `std::error::Error::source` (the latter
  eliminated entirely by using Buff-native errors).
- Future heterogeneous collections (`Vector<Box<dyn Trait>>`) work
  uniformly with a single language feature.
- The existing `Type::DynamicDispatch` variant and codegen lowering
  (shipped v1.19) are reused with zero modification.

### Negative

- Extension cap slot 1 of 3 is consumed. Two slots remain for future
  language extensions.
- The `dyn` contextual-keyword recognition adds parser complexity,
  though it is bounded (only in type position after `Box<` or `&`).
- The borrowed form (`&dyn Trait`) has an MVP restriction (parameters
  only, no let-bindings) that may confuse users who expect full Rust
  parity.

## References

- DR-014 (`.sisyphus/decisions/selfhost-feasibility.md`) -- 10 target
  crates inventory and trait-object usage count
- S1 spike (`.sisyphus/evidence/spike-multi-dispatch.md`) -- full
  motivation, multi_dispatch broken-state analysis, 3 verdict reasons
- T110 (`.sisyphus/decisions/buff-direction-speed-moat-selfhost.md`) --
  strategic direction and self-host scope
- T68 -- `Type::DynamicDispatch` (ty.rs:1192, constructor at :1424,
  v1.19)
- T75 -- trait/impl syntax (existing language foundation)
- T119 -- extern ABI policy (alternative C-shim path, rejected)
- Plan section Extension Cap Enforcement
  (`.sisyphus/plans/self-host-completion-roadmap.md`) -- cap mechanism
- Stability Promise (`.sisyphus/decisions/stability-promise.md`) --
  `dyn` not reserved as keyword

# Enums

## What Rust pain does Buff avoid?

Rust enums have a few pain points:

1. **`TypeName::Variant` qualification** -- inside a match arm you write
   `Shape::Circle(r)`, not just `Circle(r)`. This adds noise, especially in
   nested matches.
2. **`#[derive]` macros** -- same as structs, you need manual derives.
3. **`&self` on methods** -- enum methods require explicit reference receivers.

## The Buff equivalent

Buff enums use **unqualified variant names** in match arms. No `Shape::Circle`,
just `Circle`. Derives are automatic. No `impl` block needed for basic matching.

## Status

**Parser gap** -- enum declaration codegen is verified (see
`crates/buff-lang-codegen-rust/tests/enum_codegen.rs`), but the CLI's
single-file parser does NOT yet accept `enum` declarations at the top level.
Additionally, matching on user-defined enum values is a codegen gap:
variants are emitted unqualified (`Circle` instead of `Shape::Circle`).
Only the built-in `Option` and `Result` enums work end-to-end.
To verify enum codegen: `cargo test -p buff-lang-codegen-rust --test enum_codegen`.

## Key differences

| Rust | Buff |
|---|---|
| `match s { Shape::Circle(r) => ... }` | `match s { Circle(r) => ... }` |
| `#[derive(Clone, Debug)]` on every enum | Automatic |
| `impl Shape { fn area(&self) }` | `func area(s: Shape)` (standalone) |
| Variant names must be qualified in code | Unqualified everywhere |

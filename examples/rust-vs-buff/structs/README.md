# Structs

## What Rust pain does Buff avoid?

Rust structs require boilerplate and ownership awareness:

1. **`#[derive]` macros** -- you must manually add `#[derive(Clone, Debug)]`
   (or `PartialEq, Hash, Eq`) to every struct for basic usability.
2. **`pub` on every field** -- fields are private by default, forcing `pub`
   on each field in most cases.
3. **`&self` receivers** -- methods require explicit `&self` or `&mut self`,
   making the signature more verbose.
4. **`.clone()` for field access** -- accessing a `String` field moves it out
   unless you clone first.
5. **`impl` blocks** -- methods require a separate `impl StructName { }` block.

## The Buff equivalent

Buff structs need no derives, no `pub`, no `impl`, no `self`. The compiler
adds derives automatically, makes fields public, and handles ownership.

## Status

**Parser gap** -- struct codegen is fully verified in
`crates/buff-lang-codegen-rust/tests/struct_codegen.rs`, and the generated
Rust re-parses as valid `syn::File`. However, the CLI's single-file parser
does NOT yet accept `struct` declarations at the top level (it only accepts
`func` declarations). This is a parser limitation, not a codegen limitation.
The `.buff` file above comments out the struct syntax and uses a map as
a workaround. To verify struct codegen: `cargo test -p buff-lang-codegen-rust
--test struct_codegen`.

## Key differences

| Rust | Buff |
|---|---|
| `#[derive(Clone, Debug)]` | Automatic |
| `pub name: String` | `name: String` (public by default) |
| `impl Point { fn x(&self) -> f32 }` | Just `p.x` (field access) |
| `person.name.clone()` | `person.name` (compiler clones if needed) |
| `let mut p = Point { x: 1.0, y: 2.0 }` | `let p = Point { x: 1.0, y: 2.0 }` |

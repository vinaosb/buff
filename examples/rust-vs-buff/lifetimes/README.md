# Lifetimes

## What Rust pain does Buff avoid?

Rust requires **explicit lifetime annotations** (`'a`, `'b`) on functions that
return references. When a function takes `&self` and returns `&str`, the
compiler demands that you tie the output lifetime to the input. Structs that
hold references need lifetime parameters on the struct itself.

This creates a significant learning curve. Common lifetime pain points:

1. **Function signatures cluttered with `'a`** -- `fn title<'a>(&'a self) -> &'a str`
   for what should be a simple accessor.
2. **Struct lifetime parameters** -- `struct Wrapper<'a> { data: &'a str }`
   infects every type that holds a reference.
3. **Lifetime elision fails** -- when the compiler cannot automatically elide
   lifetimes, you must reason about them manually.

## The Buff equivalent

Buff has **no references** and **no lifetimes**. Functions take and return
**owned values**. The compiler emits clones or Arc where sharing is needed.
A `func title(a: Article) -> String` simply returns a new String; no lifetime
annotation required.

## Status

**Parser gap** -- struct codegen is fully verified in
`crates/buff-lang-codegen-rust/tests/struct_codegen.rs`, and the generated
Rust re-parses as valid `syn::File`. However, the CLI's single-file parser
does NOT yet accept `struct` declarations at the top level. This is a parser
limitation. The `.buff` file above comments out the struct syntax and uses
print statements as a workaround.

## Key differences

| Rust | Buff |
|---|---|
| `fn title<'a>(&'a self) -> &'a str` | `func title(a: Article) -> String` |
| `struct Wrapper<'a> { data: &'a str }` | Not needed -- owned values |
| `&self`, `&str`, `&[T]` everywhere | No `&` -- owned by default |
| "lifetime may not live long enough" errors | Cannot happen |

# Collections

## What Rust pain does Buff avoid?

Rust's collection types have some ergonomic gaps:

1. **HashMap has no `[]` operator** -- you cannot write `m["key"]` to look up
   a value. You must use `m.get(&key)` which returns `Option<&V>`, requiring
   a match or `unwrap()`.
2. **`use std::collections::HashMap`** -- every file using HashMap needs this
   import. In Buff, Map is a builtin type.
3. **Type annotations on HashMap** -- `HashMap::new()` doesn't know the key
   and value types without inference from `.insert()` calls or explicit
   type parameters.
4. **`&key` for lookup** -- HashMap requires a reference to the key for
   `.get()` and `.insert()`.

## The Buff equivalent

Buff's `Vector<T>` and `Map<K,V>` are builtin types with literal syntax.
No imports, no type annotations. Vector supports `[]` literal syntax,
`.push()`, `.pop()`, `.len()`, `.map()`, and indexing. Map uses `{k: v}`
literal syntax.

## Key differences

| Rust | Buff |
|---|---|
| `use std::collections::HashMap` | No imports needed |
| `HashMap::new()` then `.insert()` | `{1: 10, 2: 20}` literal |
| `m.get(&key)` returning `Option<&V>` | Direct lookup (codegen gap) |
| `vec![1, 2, 3]` | `[1, 2, 3]` |
| `.iter().map(\|x\| x).collect()` | `.map({ x => x })` |

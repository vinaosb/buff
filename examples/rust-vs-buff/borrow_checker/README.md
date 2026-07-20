# Borrow Checker

## What Rust pain does Buff avoid?

In Rust, every value has an **owner**. When you pass a value to a function
or assign it to a new variable, ownership **moves**. The original binding
becomes unusable. You must explicitly add `&` for borrowing, `.clone()` for
copying, or redesign your code with lifetimes.

This leads to three common frustrations:

1. **"borrowed after move" errors** -- using a value after it was moved.
2. **Explicit `.clone()` litter** -- adding `.clone()` everywhere to satisfy
   the borrow checker, hurting readability and hiding intent.
3. **Reference annotation overhead** -- `&`, `&mut`, lifetime `'a` annotations
   on function signatures, even for simple operations.

## The Buff equivalent

Buff has **no visible references** (`&`, `&mut`), **no lifetime annotations**
(`'a`), and **no manual `clone()` calls**. The compiler generates owned-by-default
Rust code and inserts clones where needed. You write `v[0]`, the compiler
decides whether to pass by value or insert `.clone()`.

## Key differences

| Rust | Buff |
|---|---|
| `fn first(v: &Vec<i32>) -> i32` | `func first(v: Vector<Int>) -> Int` |
| `v.clone()` to reuse after move | Just use `v` again |
| `&v` to borrow without moving | Not needed -- no references |
| `'a` lifetime annotations | Not needed -- no lifetimes |
| `value borrowed after move` error | Cannot happen -- compiler manages it |

Note: the Buff example avoids passing vectors to functions because `Vector<Int>`
is not yet supported as a function parameter type. The example demonstrates
reuse-after-move at the variable level, which is where the pain is most visible.

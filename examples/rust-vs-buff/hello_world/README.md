# Hello World

## What Rust pain does Buff avoid?

The simplest possible program already shows differences:

1. **`println!` macro** -- Rust's print function is a macro (`println!`),
   not a regular function. Macros use `!` suffix and have special syntax
   rules. `print()` in Buff is a regular function call.
2. **Semicolons** -- every Rust statement ends with `;`. Buff uses
   indentation-based blocks (the offside rule) instead.
3. **`fn` keyword** -- Rust uses `fn` for function declarations. Buff
   uses `func` (a frozen keyword among 25).
4. **String type** -- Rust's `"Hello"` is `&str` (a borrowed slice), not
   `String` (an owned heap string). Buff treats string literals as
   owned values.

## Key differences

| Rust | Buff |
|---|---|
| `fn main() { println!("Hello!"); }` | `func main(): print("Hello!")` |
| `println!` macro | `print()` function |
| Semicolons on every statement | No semicolons (indentation-based) |
| `&str` vs `String` distinction | No visible reference types |

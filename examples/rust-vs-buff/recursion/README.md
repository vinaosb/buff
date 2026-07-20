# Recursion

## What Rust pain does Buff avoid?

Rust's recursion is relatively clean, but has minor friction:

1. **Multiple integer types** -- Rust has `u64`, `i64`, `u32`, `i32`, `usize`,
   `isize`, etc. Choosing the right one requires thought. Buff uses `Int`
   for integer values (mapped to `i64` in generated Rust).
2. **`println!` with format args** -- `println!("fib(10) = {}", fibonacci(10))`
   requires the format macro syntax. Buff's `print()` takes a single value.
3. **Semicolons and braces** -- every statement needs `;`, every function
   body needs `{ }`. Buff uses indentation-based blocks.

## The Buff equivalent

Buff recursion works the same way: `func name(params) -> Ret:`, with
indentation-based bodies. `Int` handles all integer needs. `print()` outputs
a single value without format syntax.

## Key differences

| Rust | Buff |
|---|---|
| `fn fib(n: u64) -> u64 { ... }` | `func fib(n: Int) -> Int:` |
| `println!("fib(10) = {}", fib(10))` | `print(fib(10))` |
| `u64`, `i32`, `usize` choices | `Int` (single integer type) |
| `{ ... }` braces on every block | Indentation-based blocks |
| Semicolons required | No semicolons |

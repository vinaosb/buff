+++
title = "Your first program"
weight = 20
+++

# Your first program

This page walks through three short Buff programs — `ola.buff` (hello world),
`fibonacci.buff` (recursion + arithmetic), and `error_handling.buff`
(`Result<T,E>` + the `?` operator). Each one runs end-to-end with `buff run`.

## Hello world

```buff
func main():
    print("Olá, Buff!")
```

Save it as `ola.buff` and run:

```bash
buff run ola.buff
# → Olá, Buff!
```

Notes:

- Indentation defines the function body — **no braces** for control flow.
- `func main()` is the entry point, just like Rust.
- `print` is a *prelude* function — it's available in every Buff file without
  an `import`. The compiler lowers it to `println!("{}", ...)` behind the
  scenes.

## Recursion and arithmetic

```buff
func fib(n: Int) -> Int:
    if n < 2:
        return n
    return fib(n - 1) + fib(n - 2)

func main():
    let n = 10
    print(fib(n))
```

Run it:

```bash
buff run fibonacci.buff
# → 55
```

What's happening here:

- `func fib(n: Int) -> Int` declares a function with one typed parameter and
  an explicit return type. Buff also supports type inference on `let`
  bindings, so `let n = 10` infers `Int` without an annotation.
- `if n < 2:` is a 4-space-indented block. Buff's lexer implements the
  offside rule — the body is everything dedented below the header.
- `fib(n - 1) + fib(n - 2)` is plain arithmetic. The generated Rust is
  identical to what you'd hand-write.

## Errors and `Result<T, E>`

Buff's error model maps onto Rust's `std::result::Result`. There is no
`try` / `catch` keyword — errors are values.

```buff
func half(n: Int) -> Result<Int, Error>:
    if n < 2:
        return Error("input too small")
    return Ok(n / 2)

func add_one(n: Int) -> Result<Int, Error>:
    let h = half(n)?
    return Ok(h + 1)

func main():
    let good = add_one(10)
    match good { Ok(v) => print(v), Err(_) => print(0) }
```

Run it:

```bash
buff run error_handling.buff
# → 6
# → 0
```

Highlights:

- `return Error("msg")` lowers to `return Err(Error::new("msg"))` — the
  builtin `Error` struct + `std::error::Error` impl are emitted on demand.
- The `?` operator propagates an error early: `half(n)?` unwraps the `Ok`
  value on success, or returns the `Err` from `add_one` immediately on
  failure.
- `match` arms use the `{ Pat => body, Pat => body }` form. Brace blocks are
  reserved for *data* (struct literals, maps, lambdas, match arms) — never
  for control flow.

## Standalone typecheck

Before generating code, you can ask Buff to just type-check the file:

```bash
buff check ola.buff
buff check error_handling.buff
```

`buff check` runs the lexer → parser → type inferencer without invoking
codegen or `rustc`. It's fast (no linking) and is what the LSP server uses
to surface diagnostics in your editor.

## Next steps

- Read the [syntax reference](../../language/syntax/) to learn the layout
  rules, keywords, and reserved symbols.
- Browse [`examples/`][examples] in the repo for 20+ runnable programs
  covering collections, closures, async, extern, FFI, and more.
- Scaffold a real project with [`buff new`](../project-structure/).

[examples]: https://github.com/buff-lang/buff/tree/master/examples

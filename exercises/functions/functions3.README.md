# Recursion in Buff

A Buff function can call itself just like any other function. The pattern is identical to Rust/Python: a base case that returns a constant, and a recursive case that calls the function with a smaller input.

```buff
func factorial(n: Int) -> Int:
    if n <= 1:
        return 1
    return n * factorial(n - 1)
```

Note the syntax: `func NAME(PARAM: TYPE) -> RETURNTYPE:` followed by an indented body. The `return` keyword is mandatory when returning a value from inside an `if` branch — Buff does not implicitly return the last expression the way Rust does.

## Your task

Define `factorial(n: Int) -> Int` above `main` using the recursive formula, then call `print(factorial(5))`. The expected output is `120`.

**Hint:** the base case is `if n <= 1: return 1`. The recursive case is `return n * factorial(n - 1)`. Both inside the indented function body.

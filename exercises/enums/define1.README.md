# Defining Enums in Buff

An **enum** is a type with a fixed set of named variants. Buff uses the brace form (consistent with struct-init / map / closure — braces are *data* in Buff):

```buff
enum Status { Pending, Active, Done }

enum Shape {
    Circle(Float),
    Rect(Float, Float),
    Point,
}
```

- Variants are separated by commas (trailing comma allowed).
- A **unit variant** is just a name: `Pending`.
- A **data-carrying variant** looks like a function call: `Circle(Float)` carries one `Float`; `Rect(Float, Float)` carries two.
- Enums can be generic: `enum Result<T, E> { Ok(T), Err(E) }` (this is how Buff's built-in `Result` is shaped).

Once defined, an enum value is constructed by writing the variant name like a function call: `Status.Active` or just `Active` (Buff resolves the parent enum).

## Your task

Define `enum Status { Pending, Active, Done }` at the top of the file (above `main`). Then in `main`, the solution constructs each variant — but you only need the declaration. Replace the TODO comment with the enum header line.

**Hint:** the syntax is exactly `enum Status { Pending, Active, Done }` on one line. Braces are mandatory (Buff's enum follows the data-brace convention).

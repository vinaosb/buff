# Matching Enums with Data

When an enum variant carries data, you destructure it inside a `match` arm with the `Variant(binding)` pattern:

```buff
enum Shape { Circle(Float), Rect(Float, Float), Point }

func area(s: Shape) -> Float:
    return match s {
        Circle(r) => 3.14 * r * r,
        Rect(w, h) => w * h,
        Point => 0,
    }
```

- `Circle(r)` binds the inner `Float` to the name `r` inside that arm.
- `Rect(w, h)` binds two payloads to `w` and `h`.
- Unit variants use the bare name: `Point`.
- Match must be **exhaustive** — every variant of `Shape` needs an arm.

The whole expression evaluates to whichever arm body runs, so `return match ...` returns the body of the matched arm.

## Your task

1. Define `enum Shape { Circle(Float), Rect(Float, Float), Point }` above `main`.
2. Define `func area(s: Shape) -> Float:` whose body is `return match s { ... }`.
3. In `main`, print `area(Circle(2))` (≈ 12.56), `area(Rect(3, 4))` (12), and `area(Point)` (0).

**Hint:** the variant patterns are `Circle(r) => 3.14 * r * r`, `Rect(w, h) => w * h`, and `Point => 0`. Arms separated by commas inside the `match SHAPE { ... }` braces.

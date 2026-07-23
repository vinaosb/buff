+++
title = "Type errors (E12xx)"
weight = 53
+++

# Type errors (`E12xx`)

The type-checker (`buff-lang-types`) runs inside codegen *and* standalone
via `buff check` (T55). When operands are incompatible, a binding's type
does not match its value, or a name is not in scope, it emits an `E12xx`
error.

## Codes

| Code   | Variant                      | Trigger                                         |
|--------|------------------------------|-------------------------------------------------|
| `E1201`| `UndefinedVariable`          | name not found in scope                         |
| `E1202`| `BinaryOpTypeMismatch`       | `Int + String`, `Int < Bool`, etc.              |
| `E1203`| `AssignTypeMismatch`         | `let x: Int = "hi"`                             |
| `E1204`| `InvalidUnaryOperand`        | `!5`, `-Bool`, etc.                             |
| `E1205`| `IfConditionMustBeBool`      | `if 5:` (condition is not `Bool`)               |
| `E1206`| `IfBranchTypeMismatch`       | `if`/`else` arms return different types         |
| `E1207`| `NonExhaustiveMatch`         | `match` missing an arm                          |
| `E1208`| `PreferGpuOnRecursiveFunction`| `@prefer(gpu)` on a recursive fn               |
| `E1209`| `ModuleError`                | bad `import`, cycle, missing export             |
| `E1210`| `ComptimeEvaluationFailed`   | comptime block did not reduce to a constant     |
| `E1211`| `ComptimeIoForbidden`        | I/O inside a `comptime {}` block                |
| `E1212`| `ComptimeReflectionForbidden`| reflection beyond type info at comptime         |

## Suggestions (T63)

`E1201` (undefined variable) is the most common type error, and it almost
always means a typo. When the unknown name is within Levenshtein distance
2 of a prelude builtin (free fn or prelude type), the diagnostic carries
a `help:` note:

```text
[Error] error[E1201]: undefined variable: pritn
  |
3 | let x = pritn("hi")
  |         ^^^^^
  |
  help: did you mean `print`?
```

The candidate set is the implicit prelude (`print`, `println`, `abs`,
`Int`, `String`, `DateTime`, …). The suggestion engine lives in
`buff-lang-error::suggest` and is deterministic (ties broken
alphabetically).

## `buff check` common-mistake linter (T63)

In addition to type errors, `buff check` runs a common-mistake linter that
catches:

- **Wrong case** — `Print(...)` warns `function names are lowercase, did
  you mean \`print\`?`.
- **Near-miss typos** — `prin(...)` warns `help: did you mean \`print\`?`
  even before the type-checker fires.

These are warnings (exit 0 by default); pass `--deny-warnings` / `-D` to
promote them to errors.

## Rustc → Buff mapping (T63)

When the generated Rust fails to compile, the error mapper classifies the
`rustc` message into the closest `E12xx` code:

| rustc                          | Buff code  |
|--------------------------------|------------|
| `cannot find value/function`   | `E1201`    |
| `mismatched types`             | `E1203`    |
| `cannot multiply/add types`    | `E1202`    |
| `expected bool`                | `E1205`    |
| `if and else incompatible`     | `E1206`    |
| `non-exhaustive patterns`      | `E1207`    |

## Example

```text
[Error] error[E1203]: cannot assign String to Int
  |
1 | let x: Int = "hello"
  |              ^^^^^^^
  |
  help: use Int.from(...) to convert, or change the annotation
```

+++
title = "Attributes"
weight = 50
+++

# Attributes

Attributes are compiler hints prefixed with `@`. They never change
semantics on their own — they nudge code generation, runtime behavior, or
analysis passes. An attribute the compiler doesn't recognize is a warning,
not an error.

## `@prefer(gpu)` — GPU dispatch hint

Tells the runtime that a hot loop should prefer GPU dispatch when hardware
is available. **Never breaks** when no GPU is present — it's a hint, not
a requirement.

```buff
@prefer(gpu)
func matrix_multiply(a: Matrix<f32>, b: Matrix<f32>) -> Matrix<f32>:
    ...
```

Without the hint, the compiler uses an arithmetic-intensity threshold:
loops above the threshold default to GPU, below stay on CPU. `@prefer(gpu)`
overrides the threshold upward; `@prefer(cpu)` overrides it downward.

The runtime emits both paths (Rayon CPU + WGSL GPU) and selects at
execution time, falling back to CPU if the device has no suitable GPU.

## `@ui` — Dioxus component marker

Marks a function as a Dioxus UI component for the `.buffhtml` SFC pipeline.
Used in `crates/buff-lang-cli/templates/desktop/src/main.buff`:

```buff
@ui
func app() -> Element:
    return <div>{ "hello" }</div>
```

The `@ui` attribute triggers the RSX codegen path, which emits a Dioxus
`#[component]` proc-macro on the generated Rust.

## Comptime attributes

Buff has a small compile-time-evaluation subsystem (v1.x). Functions and
constants marked with comptime attributes are evaluated by the compiler
during type-checking, not at runtime:

| Attribute | Effect |
|---|---|
| `@comptime` | Function is evaluated at compile time when arguments are constants |
| `@unroll` | Suggests the compiler unroll a `for` loop |
| `@parallel` | Suggests a loop should be parallelized via Rayon |

```buff
@comptime
func factorial(n: Int) -> Int:
    if n <= 1:
        return 1
    return n * factorial(n - 1)

const TABLE = @comptime [factorial(i) for i in 0..10]
```

These are advisory. The compiler may ignore them if it can't prove
soundness (comptime functions must terminate, must not allocate, must not
call FFI).

## Diagnostic and analysis attributes

| Attribute | Effect |
|---|---|
| `@allow(reason)` | Suppress a specific lint on this item |
| `@deprecated(msg)` | Mark item as deprecated (convention §9) |
| `@test` | Mark function as a unit test (run via `buff test`) |
| `@bench` | Mark function as a benchmark (run via `buff bench`) |

```buff
@deprecated("use fetch_v2 instead")
func fetch(url: String) -> Result<String, Error>:
    ...
```

## Layout / repr attributes (FFI)

For FFI interop with C, you may need to pin the in-memory layout of a
struct:

```buff
@repr(C)
struct Point:
    x: f32
    y: f32
```

This lowers to `#[repr(C)]` on the generated Rust struct. Without it, the
default is Rust's layout (which the optimizer may reorder). See
[`crates/buff-lang-ffi-guide/GUIDE.md`][ffi] for the 6 hard rules on
`extern` wrapper crates.

[ffi]: https://github.com/buff-lang/buff/blob/master/crates/buff-lang-ffi-guide/GUIDE.md

## Custom attributes

Buff does not (yet) support user-defined proc-macros. Attributes are
baked into the compiler. The full list lives in
`crates/buff-lang-parser/src/attributes.rs`; if you need one that isn't
there, open an issue describing the use case.

## Naming convention

Attribute names are lowercase with hyphens or single words:

- `@prefer(gpu)` ✓
- `@comptime` ✓
- `@Prefer(GPU)` ✗ (rejected by the parser)

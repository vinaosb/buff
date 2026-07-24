# Chapter 4 — GPU Compute

This is the chapter that makes Buff unlike Go, Python, or Java. The same
function you write once can:

- run in parallel across all your CPU cores via Rayon, **and**
- dispatch to the GPU as a compiled [WGSL][wgsl] compute shader,

and the runtime picks the right path at execution time. You write neither a
lock, nor a thread, nor a shader, nor a single line of CUDA.

[wgsl]: https://www.w3.org/TR/WGSL/

By the end of this chapter you'll understand:

- the **arithmetic intensity threshold** that decides CPU vs GPU automatically,
- the `@prefer(gpu)` and `@force(gpu)` dispatch hints (and `@prefer(cpu)` to
  opt out),
- the WGSL "one-parameter numeric lambda" subset that lowers to a shader,
- the stable binding contract between `buff-lang-codegen-wgsl` and
  `buff-lang-runtime`,
- why GPU dispatch **never breaks** when no GPU is present (automatic CPU
  fallback),
- the runtime error codes (`E1401`–`E1406`) and how to interpret them.

## 4.1 The model in one picture

```
        you write:
            @prefer(gpu)
            func normalize(data: Vector<Float>) -> Vector<Float>:
                return data.map({ x => (x - mean) / stddev })
                                │
        buff-lang-codegen-wgsl              buff-lang-codegen-rust
        lowers the lambda                  lowers the whole fn to
        to a WGSL shader:                  a Rayon parallel path:
                                │                       │
                                ▼                       ▼
                  @compute shader                    rayon::par_iter
                  in a .wgsl string                  CPU impl in Rust
                                │                       │
                                └──────────┬────────────┘
                                           ▼
                                buff-lang-runtime
                                picks at execution time:
                                  • GPU present + data big enough?  → GPU
                                  • otherwise?                       → CPU
                                           │
                                           ▼
                                  native result
```

You write one function. The compiler emits *both* implementations. The runtime
decides.

## 4.2 The CPU path — Rayon parallelism 🟢

Even without any GPU involvement, Buff's data-parallel combinators run in
parallel across all your CPU cores. The `.map(...)` combinator on a
`Vector<T>` lowers to Rayon's `par_iter().map(...).collect()`:

```buff
func main():
    let data = [1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0]
    let squared = data.map({ x => x * x })
    print(squared[0])
    print(squared[7])
```

This already fans out across cores. On an 8-core machine the eight multiplications
literally run on eight cores simultaneously. You wrote no threads, no `Arc`,
no `Mutex`, no `rayon::prelude::*`. The compiler inserted all of it.

> See also: [`examples/closures.buff`](../../examples/closures.buff) and
> [`examples/rust-vs-buff/closures/closures.buff`](../../examples/rust-vs-buff/closures/closures.buff)
> for end-to-end `.map()` examples.

## 4.3 The `@prefer(gpu)` hint 🔶

To *suggest* the GPU (the runtime still picks CPU if no GPU is present or the
data is too small), annotate a function with `@prefer(gpu)`:

```buff
@prefer(gpu)
func mandelbrot_row(xs: Vector<Float>, cy: Float) -> Vector<Float>:
    return xs.map({ x => mandel_at(x, cy) })

func mandel_at(x: Float, cy: Float) -> Float:
    let mut zx = 0.0
    let mut zy = 0.0
    let mut i = 0
    let max = 256
    while i < max and zx * zx + zy * zy < 4.0:
        let xt = zx * zx - zy * zy + x
        zy = 2.0 * zx * zy + cy
        zx = xt
        i = i + 1
    return Float(i) / Float(max)
```

`@prefer(gpu)` is a *hint*, never a hard requirement. Its semantics:

| Condition | Runtime decision |
|---|---|
| GPU present + data exceeds the arithmetic-intensity threshold | Dispatch to GPU. |
| GPU present but data too small (threshold not met) | Dispatch to CPU (Rayon). |
| No GPU adapter available | Dispatch to CPU (Rayon). |
| GPU dispatch fails at runtime (`E1401`–`E1406`) | Fall back to CPU (Rayon) if possible, else surface the error. |

This is the core of Buff's "heterogeneous computing without the pain" promise:
**the hint never breaks your program**. The same binary runs unchanged on a
laptop with integrated graphics, on a server with no GPU, and on a workstation
with an RTX 4090 — and uses the best path each one offers.

> 🔶 The `@prefer(gpu)` attribute parses, type-checks, and the codegen path
> emits both implementations. End-to-end GPU dispatch from `buff run` requires
> the wgpu runtime linkage wired through the CLI's single-file pipeline; the
> generated Rust is correct and re-parses via `syn`. The CPU fallback path
> runs today.

## 4.4 `@force(gpu)` — the strict variant

When you *know* a function is pointless on the CPU (say, a shader that needs
thousands of cores to hit a deadline), `@force(gpu)` makes GPU dispatch
mandatory. If no GPU is present, the function aborts with `E1404` ("no GPU
adapter is available on this host") rather than silently falling back:

```buff
@force(gpu)
func large_matrix_softmax(rows: Vector<Float>) -> Vector<Float>:
    return rows.map({ x => exp(x) })   // simplified; real softmax needs the max
```

Use `@force(gpu)` sparingly. The default `@prefer(gpu)` is almost always what
you want — it's the "do the right thing" option. `@force` is for the narrow
case where falling back to CPU would be *worse* than failing loudly (e.g. a
real-time rendering step that would drop below the frame budget on CPU).

The opposite opt-out is `@prefer(cpu)`:

```buff
@prefer(cpu)
func cheap_sum(xs: Vector<Int>) -> Int:
    return xs.fold(0, { acc, x => acc + x })
```

`@prefer(cpu)` skips GPU codegen entirely — no WGSL shader is emitted for this
function. Use it when you know the GPU can't help (small data, branchy code,
non-numeric types) and you want to keep the binary free of the wgpu runtime
linkage.

## 4.5 The arithmetic intensity threshold

How does the runtime decide between CPU and GPU for a `@prefer(gpu)` function?
Two factors:

1. **Arithmetic intensity** — ratio of floating-point operations to bytes
   loaded from memory. A function that does one multiply per element is
   *memory-bound*; the GPU's bandwidth advantage is small. A function that
   does fifty operations per element is *compute-bound*; the GPU wins big.
2. **Data size** — small arrays (< ~4 KB) don't amortize the GPU dispatch
   overhead (kernel compilation, buffer upload, result download).

When both factors favour the GPU, the runtime dispatches to it. Otherwise, it
falls back to the Rayon CPU path. You can tune this in the future via a runtime
API; for now the thresholds are sensible defaults.

This is why `@prefer(gpu)` is a *hint* — the runtime has more information at
execution time (data size, GPU presence) than the compiler has at compile time.

## 4.6 The WGSL subset — what lowers to a shader

Not every function can become a WGSL shader. The shader-emission path in
`buff-lang-codegen-wgsl` accepts a deliberately small subset:

> **The map-lambda rule.** The GPU-eligible part of a function is a single
> `.map({ x => <expr> })` call where `<expr>` is a single expression (no
> statements) over one numeric parameter.

Concretely, the body expression may contain:

- **Literals** — `1.0`, `42`, `true`, `0xFF` (must be `f32`/`f16`/`i32`/`u32`).
- **The parameter** — referenced by name (`x` in `{ x => ... }`).
- **Binary operators** — all 18: `+ - * / %`, `== != < <= > >=`, `&& ||`,
  `& | ^ << >>`.
- **Unary operators** — `-x`, `!b`, `~i`.

Everything else — function calls, method calls, struct init, `match`,
indexing, multi-statement bodies, nested lambdas, free variables, `f64`,
`Decimal`, `String` — is *rejected* by the WGSL codegen and falls back to CPU.

This is intentional. The WGSL subset is small enough to emit correct shaders
for *with confidence*; the CPU path handles everything else. Examples:

| Lambda | GPU? | Why |
|---|---|---|
| `{ x => x * 2.0 }` | ✅ | one binary op on the parameter |
| `{ x => x * x + 1.0 }` | ✅ | nested binary ops |
| `{ x => abs(x) }` | ❌ | function call — CPU fallback |
| `{ x => x.foo() }` | ❌ | method call — CPU fallback |
| `{ x => x[0] }` | ❌ | indexing — CPU fallback |
| `{ x => if x > 0.0 then x else 0.0 }` | ❌ | not a single expression — CPU fallback |
| `{ x, y => x + y }` | ❌ | two parameters — CPU fallback |
| `{ x => format!("{}", x) }` | ❌ | `String` type — CPU fallback |

When the WGSL path rejects a lambda, you get a clear diagnostic — never a
silent miscompilation. The CPU fallback produces identical results; only the
speed differs.

## 4.7 The generated shader

For a GPU-eligible lambda like `{ x => x * x }`, `buff-lang-codegen-wgsl`
emits a complete WGSL `@compute` shader. The template is stable (it must match
the runtime's hardcoded `wgpu::BindGroupLayout`):

```wgsl
// (header comment — fixed-width, no timestamps for determinism)

@group(0) @binding(0) var<storage, read> input: array<f32>;
@group(0) @binding(1) var<storage, read_write> output: array<f32>;

@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let i = gid.x;
    if (i >= arrayLength(&input)) {
        return;
    }
    let x = input[i];
    output[i] = x * x;
}
```

The defaults are:

| Knob | Default | Notes |
|---|---|---|
| `workgroup_size` | `64` | 2 warps of 32 on NVIDIA, 4 wavefronts of 16 on AMD. Valid range 1..=1024 per WGSL spec. |
| `element_type` | `f32` | `f16`/`i32`/`u32` also supported. `f64` rejected (no WGSL support). |
| `@group` | `0` | both bindings on group 0 |
| `binding_input` | `0` | read-only storage buffer |
| `binding_output` | `1` | read-write storage buffer |

The shader unconditionally:

1. Reads `gid.x` as the element index.
2. Bounds-checks against `arrayLength(&input)` and early-returns if out of
   range.
3. Loads `input[i]` into a `let x` named after the lambda parameter.
4. Evaluates the lowered body expression.
5. Writes the result to `output[i]`.

The **binding contract** between `buff-lang-codegen-wgsl` and
`buff-lang-runtime` is this exact layout. Both crates must stay in sync; the
AGENTS.md files in both cross-reference each other to enforce it.

> **Why `format!()` for WGSL?** This is the *one* exception to Buff's "no
> raw-string codegen" hard rule. WGSL has no `syn` equivalent, and the
> `naga`/`wgpu` parsers panic on invalid input rather than returning structured
> errors. The exception is documented inline in
> [`crates/buff-lang-codegen-wgsl/src/shader.rs`](../../crates/buff-lang-codegen-wgsl/src/shader.rs).
> Every *other* crate in the workspace builds Rust via `syn`/`quote` and
> formats via `prettyplease`.

## 4.8 The runtime — Rayon + wgpu + tokio

`buff-lang-runtime` is the host for heterogeneous compute. It pulls together
three Rust runtimes:

| Runtime | Role |
|---|---|
| [`rayon`](https://crates.io/crates/rayon) | CPU parallelism (data-parallel iterators). Always available. |
| [`wgpu`](https://crates.io/crates/wgpu) | GPU dispatch (compiles WGSL shaders, manages buffers). Optional — falls back to Rayon if absent. |
| [`tokio`](https://crates.io/crates/tokio) | Async I/O (networking, timers). Only linked when your program uses `async` or networking primitives. |

The `--minimal` build flag ([Chapter 2 §2.8](./chapter-2.md)) is interesting
here: it strips the GPU and Rayon linkages from binaries that don't use them.
A program with no `@prefer(gpu)` and no `.map(...)` calls produces a binary
with *neither* wgpu *nor* rayon linked — keeping the size budget under 5 MB.
See [`examples/minimal_compute.buff`](../../examples/minimal_compute.buff):

```buff
func sum_squares(n: Int) -> Int:
    let mut total = 0
    let mut i = 1
    for i <= n:
        total = total + i * i
        i = i + 1
    return total

func main():
    let result = sum_squares(100)
    print("sum of squares 1..=100:")
    print(result)
```

This uses a `while`-style loop (no `.map`), so the generated Rust uses pure
`std` iterators. No Rayon, no wgpu — the smallest possible compute binary.

## 4.9 Error codes `E1401`–`E1406`

When GPU dispatch fails, the runtime surfaces one of six error codes. None of
them are fatal if a CPU fallback exists; they become fatal only when
`@force(gpu)` was used or no fallback is possible.

| Code | Meaning | Common cause |
|---|---|---|
| `E1401` | GPU dispatch failed and no CPU fallback was available | `@force(gpu)` + a runtime GPU error |
| `E1402` | GPU shader execution fault | a bug in the generated WGSL (report it!) |
| `E1403` | GPU adapter or device initialization failed | driver issue, or the adapter vanished mid-run |
| `E1404` | no GPU adapter is available on this host | headless server, or `@force(gpu)` on a laptop without drivers |
| `E1405` | input exceeds the VRAM tiling budget | dataset too large for the GPU's memory; chunk it |
| `E1406` | WGSL shader rejected by the GPU pipeline compiler | rare; usually a driver/GPU mismatch |

These codes are **stable forever** (see [Chapter 8](./chapter-8.md) and the
conventions doc §19). You can match on them in your program and react — e.g.
fall back to a streaming CPU implementation when `E1405` fires.

## 4.10 Putting it together — a GPU-eligible pipeline

Here's a realistic shape: normalize a large vector on the GPU when possible,
fall back to CPU otherwise. The whole function is one line of Buff:

```buff
@prefer(gpu)
func normalize(data: Vector<Float>, mean: Float, stddev: Float) -> Vector<Float>:
    return data.map({ x => (x - mean) / stddev })

func main():
    // Build a large dataset.
    let mut samples: Vector<Float> = []
    let mut i = 0
    while i < 1000000:
        samples.push(Float(i) * 0.001)
        i = i + 1

    let mean = 500.0
    let stddev = 250.0
    let normalized = normalize(samples, mean, stddev)
    print("first:", normalized[0])
    print("last:", normalized[999999])
```

On a workstation with a GPU, the million-element `map` dispatches to the GPU
and finishes in milliseconds. On a headless server, it dispatches to Rayon
and fans across cores. On a single-core CI runner, it runs sequentially. The
source code is identical in all three cases.

The lambda `{ x => (x - mean) / stddev }` *references free variables* `mean`
and `stddev` — which the WGSL subset table in §4.6 says is rejected. That's
correct: the WGSL codegen rejects this lambda, and the function falls back to
the CPU (Rayon) path. To get the GPU path, capture the constants inline:

```buff
@prefer(gpu)
func normalize_gpu(data: Vector<Float>) -> Vector<Float>:
    // mean=500.0, stddev=250.0 inlined — now the lambda is GPU-eligible.
    return data.map({ x => (x - 500.0) / 250.0 })
```

This is the typical workflow: write the clear version first (CPU fallback is
fine), then — if profiling shows the GPU would help — refactor to inline the
constants so the lambda becomes GPU-eligible. The CPU version stays as a
fallback for hosts without a GPU.

## 4.11 Recap

- Buff emits **both** a Rayon CPU path and a WGSL GPU path for eligible
  functions; the runtime picks at execution time.
- `@prefer(gpu)` is a *hint* — never breaks when no GPU is present.
- `@force(gpu)` makes GPU dispatch mandatory; fails with `E1404` if absent.
- `@prefer(cpu)` skips GPU codegen entirely.
- The GPU-eligible subset is **single-parameter numeric `.map` lambdas with a
  single-expression body**. Everything else falls back to CPU.
- The WGSL binding layout (`@group(0) @binding(0)` input, `@binding(1)`
  output, workgroup 64) is a stable contract between codegen and runtime.
- Error codes `E1401`–`E1406` are stable forever and matchable in your
  program.
- `buff build --minimal` strips the Rayon and wgpu linkages from binaries
  that don't use them.

---

*Next: [Chapter 5 — Build a UI App](./chapter-5.md)*

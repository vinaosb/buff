# buff-lang-codegen-wgsl

Lowers single-parameter numeric map lambdas (`{x => x * 2.0}`) to WGSL compute shaders for GPU dispatch. T44.

## OVERVIEW

Input: a Buff `Expr::Lambda` with exactly one numeric parameter and a single-expression body. Output: a complete WGSL `@compute` shader string with stable storage-buffer bindings.

Limited GPU subset. CPU fallback in `buff-lang-runtime` handles everything this crate rejects: multi-statement bodies, function calls, struct init, match, indexing, nested lambdas, free variables, and non-WGSL-native types (f64, Decimal, String, etc.).

## STRUCTURE (5 src files)

| File | Lines | Role |
|------|-------|------|
| `lib.rs` | 478 | Entry points: `generate_wgsl(&lambda)`, `generate_wgsl_with_options(&lambda, &opts)`, `WgslCodegen::with_options(opts).generate(&lambda)`. Lambda extraction + param type resolution. `WgslOptions` struct, `WgslCodegen` state. |
| `lower.rs` | 374 | AST-to-WGSL expression lowering. `lower_expr(expr, param_name) -> String`. Supports: `Literal` (Float/Int/Bool/Byte), `Ident` (param ref only), `BinaryOp` (18 ops), `UnaryOp` (Neg/Not/BitNot). Parenthesizes nested BinaryOps for precedence safety. |
| `shader.rs` | 241 | `render_shader(opts, param_name, body_wgsl) -> String`. Shader template with stable bindings. `WgslOptions` defaults: workgroup_size=64, element_type=f32, group=0, bindings 0/1. **Uses `format!()` for WGSL output** (project rule exception, see below). |
| `ty.rs` | 277 | `WgslScalarType` (F32/F16/I32/U32). `filter_buff_type_name()` maps Buff type names. `filter_literal()` rejects non-WGSL literals. `resolve_param_type()` resolves AST type annotations. |
| `error.rs` | 96 | `WgslError`: UnsupportedType{ty,hint}, UnsupportedExpr{detail}, NotMapLambda{got}, InvalidLambdaBody{count,hint}. All `Clone + PartialEq + Eq` for test assertions. |

## TESTS

`tests/wgsl_codegen_tests.rs` (627 lines, 20+ categories): full-shader snapshots, arithmetic ops (18 binary + 3 unary), rejection paths (Double, free variables, multi-statement, non-lambda), determinism, type filtering, entry API equivalence, options round-trip. Plus inline `#[cfg(test)]` units in each src file.

Snapshot tests enforce byte-identical output for the same input.

## WHERE TO LOOK

| Task | File(s) |
|------|---------|
| Add a new binary operator | `lower.rs` (add to `SUPPORTED_BINARY_TOKENS` table) |
| Change shader template | `shader.rs` |
| Add a new WGSL scalar type | `ty.rs` + `lower.rs` |
| Tune error messages | `error.rs` |
| Change default options | `shader.rs` (`WgslOptions::default`) |
| Change lambda validation | `lib.rs` (`extract_map_lambda`, `extract_single_expr_body`) |

## CONVENTIONS

### HARD RULE EXCEPTION: `format!()` for WGSL

This crate uses `format!()` for WGSL emission (`lib.rs`/`shader.rs`). The project hard rule says "no raw-string codegen" but WGSL has no `syn` equivalent and `naga`/`wgpu` parsers panic on invalid input rather than returning structured errors. The exception is documented inline in source. All other crates in the workspace use `syn`/`quote`/`prettyplease`.

### Type policy

WGSL-native scalars only: `f32`, `f16`, `i32`, `u32`. Rejects `f64` (no WGSL support, canonical RED-spec rejection), `Decimal` (CPU-only by policy), `i64` (no WGSL support), `Int<8>`/`Int<16>` (deferred auto-widen). Rejection errors are explicit and point users toward CPU fallback.

### Determinism

Byte-identical output for the same `(lambda, options)`. The shader header comment is fixed-width and does not embed timestamps or paths. No `HashMap`/`HashSet`.

### Binding contract

Stable layout matching `buff-lang-runtime`:

```
@group(0) @binding(0) var<storage, read> input: array<T>;
@group(0) @binding(1) var<storage, read_write> output: array<T>;
@compute @workgroup_size(64)   // default, configurable 1..=1024
fn main(@builtin(global_invocation_id) gid: vec3<u32>) { ... }
```

## WHAT THIS CRATE DOES NOT DO

Statements, function calls, method calls, struct operations, match expressions, indexing, multi-statement lambda bodies, nested lambdas, free variables, `enable f16;` directive emission, shader compilation/validation (T45's job). CPU fallback in `buff-lang-runtime` handles all of these.

## DEPS

`buff-lang-ast` (workspace), `buff-lang-error` (workspace), `thiserror` (workspace). No `syn`/`quote`/`prettyplease` (this crate emits raw strings, not Rust).

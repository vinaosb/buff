# buff-simd

First-class SIMD types for the Buff language (Mojo-inspired). Pure-Rust MVP wrapping the [`wide`](https://crates.io/crates/wide) crate's portable stable-SIMD types. NO nightly `std::simd`, NO runtime `is_x86_feature_detected!` detection (compile-time target features only), NO GPU dispatch (that's WGSL's job per Metis G7 lock).

**Status: experimental** (T54 v1.19 language evolution wave).

## STRUCTURE

```
buff-simd/
├── Cargo.toml            # wide + thiserror deps; criterion dev-dep for bench
├── src/
│   ├── lib.rs            # Simd struct + 14 methods + dot free fn (~290 LOC)
│   └── error.rs          # SimdError enum (~35 LOC)
├── tests/
│   └── core.rs           # 17 unit tests + 4 insta snapshots
├── benches/
│   └── dot_product.rs    # criterion bench: scalar vs explicit-SIMD dot product
└── examples/
    ├── simd_basic.rs     # splat + add + sum smoke test
    ├── simd_dot_product.rs # 4-lane dot product vs scalar
    ├── simd_math.rs      # lane-wise mul/div + horizontal min/max
    └── simd_image.rs     # pixel-channel SIMD (4 channels = RGBA = f32x4)
```

## WHERE TO LOOK

| Task | File |
|---|---|
| Add a new SIMD operation | `src/lib.rs` (add method) + `crates/buff-lang-types/src/prelude_types.rs` (`PreludeInstanceFn` + `instance_fn_return_type`) + `crates/buff-lang-codegen-rust/src/rust_codegen.rs::lower_prelude_type_instance_fn` |
| Add a new constructor | `src/lib.rs` + `prelude_types.rs` (`PreludeAssocFn` + `assoc_fn_return_type`) + `rust_codegen.rs::lower_prelude_type_assoc_fn` |
| Add a new error variant | `src/error.rs` |
| Widen to 8/16 lanes (f32x8/f32x16) | `src/lib.rs` (swap `f32x4` → `f32x8` + bump `LANES`) + `prelude_types.rs` (generic `N` plumbing — deferred to v1.20+) |
| Change the benchmark | `benches/dot_product.rs` |

## PUBLIC API

### `Simd` — 4-lane `f32x4` SIMD register (the concrete `Simd<Float, 4>`)

| Method | Signature | Notes |
|---|---|---|
| `Simd::splat` | `(x: f32) -> Simd` | Broadcast scalar to all 4 lanes. Infallible. |
| `Simd::from_slice` | `(&[f32]) -> Result<Simd, SimdError>` | Length-checked (needs >=4). Rejects non-finite. |
| `Simd::from_array` | `([f32; 4]) -> Simd` | Infallible. |
| `simd.add` | `(self, Simd) -> Simd` | Lane-wise `+`. |
| `simd.sub` | `(self, Simd) -> Simd` | Lane-wise `-`. |
| `simd.mul` | `(self, Simd) -> Simd` | Lane-wise `*`. |
| `simd.div` | `(self, Simd) -> Simd` | Lane-wise `/`. |
| `simd.sum` | `(self) -> f32` | Horizontal sum. |
| `simd.min` | `(self) -> f32` | Horizontal min (smallest lane). |
| `simd.max` | `(self) -> f32` | Horizontal max (largest lane). |
| `simd.lane_min` | `(self, Simd) -> Simd` | Element-wise min vs another Simd. |
| `simd.lane_max` | `(self, Simd) -> Simd` | Element-wise max vs another Simd. |
| `simd.to_vec` | `(self) -> Vec<f32>` | Extract 4 lanes. |
| `simd.to_array` | `(self) -> [f32; 4]` | Extract to fixed array. |

### Free functions

| Fn | Signature | Notes |
|---|---|---|
| `dot` | `(a: Simd, b: Simd) -> f32` | 4-lane dot product = `a.mul(b).sum()`. |

## CONVENTIONS

- **Pure-Rust only**: wraps `wide` (pure-Rust portable SIMD — no `cc-rs`, no assembler shims, no nightly `std::simd`). Matches the "no C library, no Docker" hard rule.
- **Compile-time target features only**: NO runtime `is_x86_feature_detected!` detection (per T54 spec "Must NOT" clause). `wide` selects the best register width at compile time via `#[cfg(target_feature)]`.
- **CPU-only per Metis G7 lock**: NO GPU dispatch. GPU SIMD is WGSL's job (`buff-lang-codegen-wgsl`).
- **Panic-free**: no `unwrap` / `expect` / `panic!` in non-test code. The only fallible entry point is `Simd::from_slice` (returns `Result`); codegen wraps it in `.unwrap_or_default()` (Default = `splat(0.0)`).
- **Fixed 4 lanes for MVP**: the conceptual `Simd<T, N>` is realised as `Simd<Float, 4>` (one 128-bit SSE/NEON register). Wider registers (`f32x8` = AVX, `f32x16` = AVX-512) + generic `N` parameter deferred to v1.20+.

## CODEGEN INTEGRATION

The Buff surface (`Simd.splat(x)` / `Simd.from_slice(s)` / `simd.add(other)` / `simd.sum()` etc.) is wired in:

- **Type variant**: `Type::Simd` in `crates/buff-lang-types/src/ty.rs`
- **Prelude registry**: `PreludeType::Simd` + `PreludeAssocFn::{Splat, FromSlice, FromArray}` + `PreludeInstanceFn::{Add, Sub, Mul, Div, Sum, Min, Max, ToVec}` in `crates/buff-lang-types/src/prelude_types.rs`
- **Lowering**: `lower_prelude_type_assoc_fn` (3 ctors) + `lower_prelude_type_instance_fn` (8 methods) in `crates/buff-lang-codegen-rust/src/rust_codegen.rs`
- **Extern crates**: `buff-simd` + `wide` registered via `program_uses_namespace("Simd")`
- **Tests**: `crates/buff-lang-codegen-rust/tests/simd_codegen.rs`

## DEFERRED

- Generic `Simd<T, N>` type-parameter plumbing (T = f32/i32/u32, N = 4/8/16) — v1.20+
- Wider registers: `f32x8` (AVX 256-bit), `f32x16` (AVX-512 512-bit) — v1.20+
- Integer SIMD types (`i32x4`, `u32x4`, `i64x2`) — v1.20+
- Gather / scatter / shuffle / mask operations — v1.20+
- Matrix multiply tile (8x8 f32x8 blocks) for buff-tensor (T8) — v1.20+
- Fused multiply-add (`fma`) — v1.20+ (needs target-feature gate)

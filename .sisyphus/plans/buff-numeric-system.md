# Buff Extended Numeric Type System — Complete Specification

## Core Philosophy (FINAL — Simplified)
"Explicit width = fixed and predictable. Auto width = flexible, grows and shrinks as needed."

**Two modes only**:
- `Int<32>` (explicit width) = FIXED. Never grows. Checked overflow (panic debug, wrap release).
- `Int` (no width) = FLEXIBLE. Compiler tracks value range, picks smallest fitting width. Grows on overflow, shrinks when range narrows.

---

## Tier 1: Standard Numeric Types (Everyday Use)

### Int — Signed Integer with Auto-Width
```
let x = 42                  // Int<64> (safe default for arithmetic)
let small: Int<8> = 100     // explicit narrow
let temperatures = [20, 25] // Vector<Int<8>> (auto-detected: all fit i8)
```
Widths: `<8>`, `<16>`, `<32>`, `<64>` (default), `<128>`

### Bits — Unsigned Integer with Auto-Width  
```
let mask: Bits<8> = 0xFF
let flags = [0, 1, 2, 3]   // Vector<Bits<8>> (auto-detected)
```
Widths: `<8>` (alias: Byte), `<16>`, `<32>`, `<64>` (default), `<128>`

### Float — IEEE 754 Floating Point
```
let pi = 3.14              // Float<32> (default, GPU-native)
let precise: Float<64> = 3.141592653589  // alias: Double
```
Widths: `<16>` (half), `<32>` (default), `<64>` (alias: Double)

### Decimal — Exact Fixed-Point
```
let price = 99.90m         // Decimal (128-bit, for finance)
```
Single format: 128-bit, always CPU (not GPU)

---

## Tier 2: AI/ML Quantization Formats

### Quantized Floats
```
// Brain Float (AI training — wider range, less precision)
let training_weight: BFloat16 = 0.156

// FP8 variants (NVIDIA Hopper)
let inference_a: Float<FP8_E4M3> = ...  // precision-focused
let inference_b: Float<FP8_E5M2> = ...  // range-focused

// Extreme compression
let tiny: Float<FP4> = ...     // 4-bit float
let qlora: Float<NF4> = ...    // NormalFloat 4-bit
```

| Format | Bits | Exp | Mantissa | Range | Precision | Use Case |
|--------|------|-----|----------|-------|-----------|----------|
| `Float<16>` | 16 | 5 | 10 | ±65504 | ~3 decimal | General AI, GPU-native |
| `BFloat16` | 16 | 8 | 7 | ±3.4×10³⁸ | ~2 decimal | AI training (wide range) |
| `Float<FP8_E4M3>` | 8 | 4 | 3 | ±448 | ~1 decimal | NVIDIA Hopper inference |
| `Float<FP8_E5M2>` | 8 | 5 | 2 | ±57344 | ~0.5 decimal | Wide-range inference |
| `Float<FP4>` | 4 | 2 | 1 | ±4 | ~0.5 decimal | Extreme compression |
| `Float<NF4>` | 4 | — | — | — | — | QLoRA fine-tuning |

### Quantization API
```
let model = Matrix.load("llama.bin")     // Float<32> by default

// Quantize to different formats
let fast = model.quantize(.FP8_E4M3)     // 4x smaller, NVIDIA H100 native
let tiny = model.quantize(.NF4)          // 8x smaller, QLoRA
let mini = model.quantize(.INT4)         // 8x smaller, uniform

// Dequantize when needed
let restored = fast.dequantize()         // Back to Float<32>

// Runtime checks: if target GPU doesn't support FP8, auto-fallback to FP16
```

---

## Tier 3: Trits — First-Class Ternary

### Trit Type
```
let t: Trit = -1          // or 0, or +1
let zero: Trit = 0
let positive: Trit = +1

// Only 3 valid values — compiler enforces
let invalid: Trit = 2     // COMPILE ERROR: Trit must be -1, 0, or 1
```

### Trit Arithmetic (SPECIAL — no multiplication needed!)
```
let a: Trit = -1
let b: Trit = +1

let sum = a + b           // Trit sum: -1 + 1 = 0
let product = a * b       // Trit product: just sign logic! (-1)*(+1) = -1
                          // NO hardware multiplier needed!
```

**Trit multiplication truth table**:
```
  ×  | -1    0   +1
-----|----------------
 -1  | +1    0   -1
  0  |  0    0    0
 +1  | -1    0   +1
// This is just: sign(a) × sign(b). Zero propagates. NO multiply instruction!
```

### Trits<N> — Packed Ternary Storage
```
// 5 trits pack into 1 byte (3^5 = 243 < 256)
let weights: Vector<Trit> = load_bitnet_model()  // logical ternary
let packed: Trits<5> = ...                        // 5 trits in 1 byte

// BitNet b1.58 model loading
let model = Matrix.load("bitnet.bin").quantize(.Trit)
```

| Storage | Trits per byte | Efficiency |
|---------|---------------|------------|
| 1 trit (unpacked) | 1 (wastes 2/3) | 33% |
| 2 trits | 2 (3^2=9 < 256) | 67% |
| 5 trits (optimal) | 5 (3^5=243 < 256) | 100% efficient |
| 5 trits → 1 byte | 5 | Theoretical max |

### Trits as Future-Ready
- **Emulation now**: Bit-packing in WGSL/Rust shaders
- **Native future**: When ternary hardware arrives, compiler generates native ops
- **Abstract API**: User writes same code regardless of hardware support
```
// This code is IDENTICAL whether running on:
// - Current GPU (emulated via bit-packing)
// - Future ternary processor (native)
let output = model.forward(input)  // model is quantized to .Trit
```

---

## Auto-Sizing Arithmetic Rules (MANDATORY)

The compiler MUST track output widths for ALL operations:

### Integer Operations
```
let a: Int<8> = 100
let b: Int<8> = 28

let sum = a + b          // 128 → needs Int<16> (i8 max = 127)
let diff = a - b         // 72 → fits Int<8>
let product = a * b      // 2800 → needs Int<16> (i8 max = 127)
let quotient = a / b     // 3 → fits Int<8>
let shifted = a << 4     // 1600 → needs Int<16> (original 8 + shift 4 = 12 bits)
```

### Width Propagation Rules

| Operation | Formula | Example |
|-----------|---------|---------|
| `Int<W1> + Int<W2>` | `Int<max(W1,W2)+1>` | i8 + i8 → i16 (carry) |
| `Int<W1> - Int<W2>` | `Int<max(W1,W2)+1>` | i8 - i8 → i16 (borrow) |
| `Int<W1> * Int<W2>` | `Int<W1+W2>` | i8 × i8 → i16 |
| `Int<W> << n` | `Int<W+n>` | i8 << 4 → i12→i16 |
| `Int<W> >> n` | `Int<W>` | i8 >> 4 → i8 (truncate) |
| `Bits<W1> & Bits<W2>` | `Bits<max(W1,W2)>` | u8 & u16 → u16 |
| `Bits<W1> \| Bits<W2>` | `Bits<max(W1,W2)>` | u8 \| u16 → u16 |
| `Bits<W1> ^ Bits<W2>` | `Bits<max(W1,W2)>` | u8 ^ u16 → u16 |

### Float Operations (Precision Preservation)
```
let a: Float<16> = 1.5
let b: Float<16> = 2.3

let sum = a + b           // Float<16> (same precision, OK)
let product = a * b       // Float<32>! (promote to prevent precision loss)

// Mixing widths
let mixed = Float<32>(pi) + Float<64>(e)  // Float<64> (wider wins)
```

| Operation | Result Precision | Rationale |
|-----------|-----------------|-----------|
| `Float<W> + Float<W>` | `Float<W>` | Same precision, no loss |
| `Float<W> * Float<W>` | `Float<min(W*2, 64)>` | Prevent mantissa loss |
| `Float<W1> OP Float<W2>` | `Float<max(W1,W2)>` | Wider precision wins |
| `Float<W> + Int<X>` | `Float<W>` | Int converts to float |
| `Decimal OP Decimal` | `Decimal` | Exact, no change |

### Quantized Format Arithmetic
```
// Quantized types promote to standard for arithmetic, then re-quantize
let a: Float<FP8_E4M3> = ...
let b: Float<FP8_E4M3> = ...
let result = a + b        // Promotes to Float<32>, computes, result is Float<32>
                          // User must explicitly re-quantize: result.quantize(.FP8_E4M3)
```

### Mixed-Format Rules
```
let bf16: BFloat16 = 0.5
let fp32: Float<32> = 1.5
let result = bf16 + fp32  // Float<32> (BFloat16 promotes to f32 for safety)

let fp8: Float<FP8_E4M3> = ...
let fp4: Float<FP4> = ...
let mixed = fp8 + fp4     // COMPILE ERROR or Float<32>? 
                          // Rule: quantized promotes to Float<32> for mixed ops
```

---

## GPU Compatibility Matrix (WGSL-Native Policy)

**Policy**: GPU dispatch accepts ONLY WGSL-native types. Everything else: CPU (Rayon) or auto-convert with warning.

| Type | WGSL Native? | Action | Notes |
|------|-------------|--------|-------|
| `Float<32>` | ✅ Native | Direct dispatch | Default GPU type, zero overhead |
| `Float<16>` | ✅ Native (modern) | Direct dispatch (check `enable f16`) | Fallback to f32 if unsupported |
| `Int<32>` | ✅ Native | Direct dispatch | Zero overhead |
| `Bits<32>` | ✅ Native | Direct dispatch | Zero overhead |
| `Float<64>` | ❌ | Auto-convert → f32 | Precision warning if lossy |
| `Int<64>` | ❌ | Auto-convert → i32 | Overflow check (static + runtime) |
| `Int<8>`, `Int<16>` | ❌ | Auto-convert → i32 | Widened (safe, no data loss) |
| `Bits<8>`, `Bits<16>` | ❌ | Auto-convert → u32 | Widened (safe) |
| `Bits<64>` | ❌ | Auto-convert → u32 | Overflow check |
| `Decimal` | ❌ | CPU fallback (Rayon) | Not numeric for GPU |
| `BFloat16` | ❌ | **DEFERRED v2.0** | Not in WGSL spec |
| `Float<FP8_*>` | ❌ | **DEFERRED v2.0** | Not in WGSL spec |
| `Float<FP4>` | ❌ | **DEFERRED v2.0** | Not in WGSL spec |
| `Float<NF4>` | ❌ | **DEFERRED v2.0** | Not in WGSL spec |
| `Trit` | ❌ | **DEFERRED v2.0** | Not in WGSL spec |

> **DEFERRED formats** (BFloat16, FP8, FP4, NF4, Trit) are documented in this spec for future reference.
> They will be implemented in v2.0 either when: (a) WGSL adds native support, OR (b) Buff has robust bit-packing emulation in WGSL shaders.
> Until then, users working with these formats use CPU parallel (Rayon) which handles them natively.

---

## Summary: Complete Numeric Tower

```
                    ┌──────────────────────────────────────────┐
                    │           NUMERIC TYPE SYSTEM             │
                    └──────────────────────────────────────────┘
                                      │
                    ┌─────────────────┴─────────────────┐
                    │                                   │
               ┌────▼────┐                        ┌─────▼─────┐
               │ INTEGER │                        │  FLOATING │
               └────┬────┘                        │   POINT   │
                    │                              └─────┬─────┘
           ┌────────┴────────┐                     ┌─────┴─────┐
      ┌────▼────┐      ┌─────▼─────┐         ┌─────▼────┐ ┌───▼────┐
      │  Int<X> │      │  Bits<X>  │         │ Float<X> │ │Decimal │
      │ signed  │      │ unsigned  │         │ IEEE 754 │ │ 128bit │
      │ auto-w  │      │ auto-w    │         │ 16/32/64 │ │ fixed  │
      └────┬────┘      └─────┬─────┘         └─────┬────┘ └────────┘
           │                  │                     │
           │            ┌─────▼─────┐        ┌──────▼──────┐
           │            │   Trit    │        │  Quantized  │
           │            │  (-1,0,1) │        │   Floats    │
           │            │  Trits<N> │        │ BFloat16    │
           │            │  ternary  │        │ FP8_E4M3    │
           │            └───────────┘        │ FP8_E5M2    │
           │                                 │ FP4, NF4   │
           │                                 └─────────────┘
           │
    ┌──────▼──────┐
    │ AUTO-SIZING │
    │ Arithmetic  │
    │ +: max+1    │
    │ *: W1+W2   │
    │ <<: W+n    │
    └─────────────┘
```

# buff-dsp

Signal-processing MVP for the Buff language: FFT, biquad filters, spectral ops, and window functions. CPU-only (Metis G7). Wraps the pure-Rust `rustfft`, `realfft`, and `apodize` crates behind a safe, owned-`Vec<f64>` surface that complies with the FFI guide (`crates/buff-lang-ffi-guide/GUIDE.md`).

## STRUCTURE

```
buff-dsp/
├── Cargo.toml           # workspace deps: rustfft 6.2, realfft 3.4, apodize 1.0
├── src/
│   └── lib.rs           # ~700 LOC — Signal / Spectrum / Window / Complex + 25 pub fns
├── examples/
│   ├── fft_peak.rs      # QA scenario 1: 440 Hz sine → FFT peak near bin 440
│   ├── hann_shape.rs    # QA scenario 2: hann(8) reference vector
│   └── lowpass.rs       # QA scenario 3: lowpass attenuates 5 kHz more than 100 Hz
├── tests/
│   ├── fft_proptest.rs  # proptest: fft → ifft roundtrip ≈ identity
│   ├── filters.rs       # biquad correctness (lowpass / highpass / bandpass)
│   ├── windows.rs       # hann / hamming / blackman shape properties
│   ├── spectrum.rs      # Spectrum::freqs / magnitudes / phases
│   └── snapshots/       # 5+ insta snapshots
├── AGENTS.md            # this file
└── README.md            # crate overview
```

## WHERE TO LOOK

| Task | File |
|---|---|
| Add a new DSP op | `src/lib.rs` (extend the 25-fn cap carefully — see below) |
| Add a new window type | `src/lib.rs::WindowKind` enum + `compute_window` arm + new `Window::*` ctor |
| Add a new filter | `src/lib.rs::Biquad` (add a new ctor like `Biquad::notch`) + new `Signal::notch` method |
| Change FFT backend | `src/lib.rs::externs` module (the ONLY place `rustfft`/`realfft`/`apodize` are touched) |
| Audit FFI safety | Cross-reference `crates/buff-lang-ffi-guide/GUIDE.md` rules R1-R6 |
| Add a snapshot test | `tests/snapshots/<name>.snap` + new `#[test]` in any tests/ file |

## CONVENTIONS (this crate only)

- **25-public-function CAP** (T11 spec hard limit). The current count is enumerated in `src/lib.rs` crate-level docs. Adding a 26th fn requires T11 spec amendment.
- **`#![forbid(unsafe_code)]`** at the crate root. There is NO `unsafe` anywhere — `catch_unwind` is the only panic-isolation mechanism (per GUIDE.md R6).
- **No `unwrap`/`expect`/`panic!`/`todo!`/`unimplemented!`** in non-test code (project-wide rule from README).
- **Every externs function returns `Option<Vec<_>>`** — `None` = panic caught, `Some(Vec::new())` = empty/degenerate input, `Some(non_empty)` = success. Callers decide the fallback.
- **Plain-data public types** (`Signal`, `Spectrum`, `Window`, `Complex`) — no traits, no lifetimes, no generics on the public surface. `Complex` is a plain struct (NOT `num_complex::Complex64`) so `rustfft`'s internals don't leak (R1).
- **Biquad coefficients follow the RBJ Audio EQ Cookbook** — see `src/lib.rs::Biquad::{lowpass, highpass, bandpass}` doc comments for the formulas.
- **CPU-only** (Metis G7). GPU FFT would live in a sibling crate (`buff-lang-codegen-wgsl`).
- **No real-time streaming** (deferred to v1.18+). `Signal` is `Vec`-backed, not `Stream`-backed.
- **No adaptive filters** (LMS, RLS — deferred to v1.18+).

## FFI GUIDE COMPLIANCE

Per `crates/buff-lang-ffi-guide/GUIDE.md`:

- **R1 (no raw pointers)**: ✓ — every public type is `Vec<f64>` / `Vec<Complex>` / `u32` / `f64`.
- **R2 (Rust owns heap)**: ✓ — `Signal::from_vec` takes ownership of the input `Vec<f64>`.
- **R3 (error mapping)**: ✓ — every public fn is infallible; bad inputs yield empty results (never panics). `externs` catches panics and signals via `Option`.
- **R4 (Send + 'static)**: ✓ — `Signal`/`Spectrum`/`Window`/`Complex` are all `Send + Sync` (verified by the `static_assertions`-style compile check inherent in their plain-data shape).
- **R5 (no lifetimes)**: ✓ — every accessor returns an owned `Vec` or borrows for the call duration only.
- **R6 (panic boundary)**: ✓ — every externs fn body is wrapped in `std::panic::catch_unwind(AssertUnwindSafe(...))`.

## RELATIONSHIP TO OTHER CRATES

| Crate | Relationship |
|---|---|
| `buff-lang-ffi-guide` | Authoritative FFI rules. buff-dsp complies with all 6. |
| `buff-lang-codegen-rust` | Codegen layer that lowers `Signal.*` / `Window.*` Buff calls into `buff_dsp::Signal::*` Rust paths. |
| `buff-lang-types` | Prelude-type registry — `Signal` / `Window` / `Spectrum` are registered there so the type inferencer recognises them. |
| `rustfft` / `realfft` / `apodize` | The three extern targets. Only touched inside `src/lib.rs::externs`. |

## TESTING

- **proptest** (`tests/fft_proptest.rs`): FFT→IFFT roundtrip is approximately the identity for arbitrary real signals.
- **Unit smoke tests** (`src/lib.rs::tests`): empty signals, DC spectrum, Hann reference vector, lowpass attenuation.
- **Insta snapshots** (`tests/snapshots/*.snap`): frozen reference vectors for Hann / Hamming / Blackman / freqs / magnitudes.
- **QA scenarios** (3 examples): match the T11 spec acceptance scenarios verbatim.

CI runs `cargo test -p buff-dsp` on all 3 OSes. Locally on Windows the MSVC `msvcrt.lib` issue (per AGENTS.md root) may block test linking — use `cargo check --tests -p buff-dsp` to verify the test code type-checks.

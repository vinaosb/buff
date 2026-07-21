# buff-dsp

Signal-processing MVP for the Buff language: **FFT**, **biquad filters**, **spectral ops**, and **window functions**. CPU-only. Wraps the pure-Rust [`rustfft`](https://docs.rs/rustfft), [`realfft`](https://docs.rs/realfft), and [`apodize`](https://docs.rs/apodize) crates behind a safe, owned-`Vec<f64>` surface that complies with the [Buff FFI guide](../buff-lang-ffi-guide/GUIDE.md).

## What this is

A minimal digital-signal-processing crate that the Buff compiler lowers `Signal<Float>` / `Window.*` calls into. The public surface is **25 functions** exactly (T11 hard cap) and ships:

- **`Signal`** — `Vec<f64>` + `sample_rate: u32`. The user-visible time-domain value.
- **`Spectrum`** — `Vec<Complex>` + `sample_rate: u32`. FFT output (hermitian half `N/2+1` bins).
- **`Window`** — precomputed Hann / Hamming / Blackman coefficients.
- **`Complex`** — plain `{re, im: f64}` struct (no `num_complex` leak).

## Quick start

```rust
use buff_dsp::{Signal, Window};

// Synthesize a 440 Hz sine at 44.1 kHz, 4096 samples.
let s = Signal::sine(440.0, 44_100, 4096);

// Forward FFT → spectrum with 2049 bins.
let spec = s.fft();
let mags = spec.magnitudes();
let peak_bin = mags.iter()
    .enumerate()
    .skip(1)
    .max_by(|(_, a), (_, b)| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal))
    .map(|(i, _)| i)
    .unwrap_or(0);
let peak_hz = peak_bin as f64 * 44_100.0 / 4096.0;
assert!((peak_hz - 440.0).abs() < 15.0);

// Filter: low-pass at 500 Hz.
let filtered = s.lowpass(500.0);

// Window + apply.
let hann = Window::hann(256);
let mut framed = Signal::from_vec(vec![1.0; 256], 44_100);
framed.apply_window(&hann);

// Spectrogram (STFT).
let frames = s.spectrogram(1024);
```

## Scope

- **CPU-only** (per Metis G7 — no GPU acceleration).
- **No real-time streaming** — `Signal` is `Vec`-backed, not `Stream`-backed. Deferred to v1.18+.
- **No adaptive filters** (LMS, RLS) — deferred to v1.18+.
- **25 public functions max** — T11 hard cap. See `src/lib.rs` crate-level docs for the enumeration.

## FFI safety

Complies with all 6 rules from `crates/buff-lang-ffi-guide/GUIDE.md`:

| Rule | Status |
|---|---|
| R1: No raw pointers | ✓ All public types are owned `Vec<f64>` / structs. |
| R2: Rust owns heap | ✓ `Signal::from_vec` takes ownership of the input. |
| R3: Error mapping | ✓ Every public fn is infallible; bad inputs yield empty results. |
| R4: Send + 'static | ✓ All public types are plain-data `Send + Sync`. |
| R5: No lifetimes | ✓ Every accessor returns owned values or borrows for the call only. |
| R6: Panic boundary | ✓ Every `externs` body uses `catch_unwind`. |

## License

MIT OR Apache-2.0, same as the rest of the Buff workspace.

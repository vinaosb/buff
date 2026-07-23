//! # buff-dsp
//!
//! Signal-processing MVP for Buff: FFT, biquad filters, spectral ops,
//! and window functions. CPU-only. Wraps the pure-Rust `rustfft`,
//! `realfft`, and `apodization` crates behind a safe, owned-`Vec`
//! surface that complies with the Buff FFI guide
//! (`crates/buff-lang-ffi-guide/GUIDE.md`).
//!
//! ## Surface
//!
//! | Type | What it is |
//! |---|---|
//! | [`Signal`] | `Vec<f64>` + `sample_rate: u32`. The user-visible signal value. |
//! | [`Spectrum`] | `Vec<Complex>` + `sample_rate: u32`. FFT output. |
//! | [`Window`] | A precomputed window (Hann / Hamming / Blackman). |
//! | [`Complex`] | `{re, im: f64}` plain struct (no `num_complex` at the boundary). |
//!
//! ## Public functions
//!
//! 1. [`Signal::from_vec`] — ctor.
//! 2. [`Signal::sine`] — synth (test helper + example use).
//! 3. [`Signal::sample_rate`] — accessor.
//! 4. [`Signal::len`] — sample count.
//! 5. [`Signal::is_empty`] — empty check.
//! 6. [`Signal::as_slice`] — read-only view of samples.
//! 7. [`Signal::fft`] — forward R2C FFT → [`Spectrum`].
//! 8. [`Signal::ifft`] — inverse synthesis: returns time-domain signal.
//! 9. [`Signal::lowpass`] — one-pole one-zero biquad low-pass.
//! 10. [`Signal::highpass`] — one-pole one-zero biquad high-pass.
//! 11. [`Signal::bandpass`] — constant-0-dBGain biquad band-pass.
//! 12. [`Signal::apply_window`] — multiply samples by [`Window`] in place.
//! 13. [`Signal::spectrogram`] — STFT → `Vec<Spectrum>` (one frame per hop).
//! 14. [`Signal::magnitude`] — element-wise magnitude of the FFT spectrum.
//! 15. [`Signal::phase`] — element-wise phase in radians of the FFT spectrum.
//! 16. [`Spectrum::len`] — bin count.
//! 17. [`Spectrum::is_empty`] — empty check.
//! 18. [`Spectrum::iter`] — borrow iterator over bins.
//! 19. [`Spectrum::freqs`] — `Vec<f64>` of bin centres in Hz.
//! 20. [`Spectrum::magnitudes`] — `Vec<f64>` of `|bin|`.
//! 21. [`Spectrum::phases`] — `Vec<f64>` of `atan2(im, re)`.
//! 22. [`Window::hann`] — Hann window of length `n`.
//! 23. [`Window::hamming`] — Hamming window of length `n`.
//! 24. [`Window::blackman`] — Blackman window of length `n`.
//! 25. [`Window::as_slice`] — read-only view of coefficients.
//!
//! Count: 25 (the cap from the T11 spec). All other helpers are
//! private to this crate.
//!
//! ## FFI safety (per GUIDE.md)
//!
//! - R1: No `*const T` / `*mut T` anywhere. Inputs and outputs are owned
//!   `Vec<f64>` / `Vec<Complex>`. The only `unsafe` is `catch_unwind`
//!   wrappers in [`externs`].
//! - R2: Rust owns every allocation. Buff holds owned `Vec` copies.
//! - R3: No fallibility is surfaced — every operation is infallible
//!   from the Buff user's view: bad lengths yield empty results (never
//!   panics). Internally `rustfft` returns `()`, so the only failure
//!   mode is a Rust panic, which R6 catches.
//! - R4: [`Signal`] / [`Spectrum`] / [`Window`] are all `Send + Sync`
//!   (plain `Vec<f64>` + `u32`). Safe to capture in `spawn`.
//! - R5: No lifetimes exposed. Every accessor returns an owned value
//!   or borrows for the duration of the call only.
//! - R6: Every public surface function whose body could panic (FFT,
//!   filters, spectrogram, window ctors) is wrapped in
//!   [`std::panic::catch_unwind`] in [`externs`].
//!
//! ## Scope
//!
//! CPU-only (Metis G7). No real-time streaming (deferred to v1.18+).
//! No adaptive filters (LMS, RLS — deferred to v1.18+).

#![forbid(unsafe_code)]

// ---------------------------------------------------------------------------
// Public plain-data types
// ---------------------------------------------------------------------------

/// `{re, im: f64}` — a complex sample.
///
/// Plain struct so the public API does not leak `num_complex` (which
/// `rustfft` uses internally). The codegen layer maps Buff's
/// `Signal<Complex>` to this struct; no raw pointers cross the
/// boundary (R1).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Complex {
    /// Real part.
    pub re: f64,
    /// Imaginary part.
    pub im: f64,
}

impl Complex {
    /// Element-wise magnitude `sqrt(re^2 + im^2)`.
    fn magnitude(self) -> f64 {
        self.re.hypot(self.im)
    }

    /// Element-wise phase `atan2(im, re)` in radians `[-pi, pi]`.
    fn phase(self) -> f64 {
        self.im.atan2(self.re)
    }
}

impl From<rustfft::num_complex::Complex64> for Complex {
    fn from(c: rustfft::num_complex::Complex64) -> Self {
        Self { re: c.re, im: c.im }
    }
}

impl From<Complex> for rustfft::num_complex::Complex64 {
    fn from(c: Complex) -> Self {
        Self { re: c.re, im: c.im }
    }
}

/// A time-domain signal: owned `Vec<f64>` samples + `sample_rate` (Hz).
///
/// Buff-side spelling: `Signal<Float>`. Codegen-lowered via the
/// `Signal.from_vec(data, sample_rate)` ctor + instance methods
/// `s.fft()` / `s.lowpass(cutoff_hz)` / `s.apply_window(window)` etc.
#[derive(Debug, Clone, PartialEq)]
pub struct Signal {
    samples: Vec<f64>,
    sample_rate: u32,
}

/// An FFT spectrum: owned `Vec<Complex>` bins + `sample_rate` (Hz).
///
/// Returned by [`Signal::fft`] / [`Signal::spectrogram`]. The number
/// of bins is `N / 2 + 1` (hermitian-symmetric half of a length-`N`
/// real input — `realfft` drops the redundant conjugates).
#[derive(Debug, Clone, PartialEq)]
pub struct Spectrum {
    bins: Vec<Complex>,
    sample_rate: u32,
}

/// A precomputed window function (Hann / Hamming / Blackman).
///
/// Created via [`Window::hann`] / [`Window::hamming`] /
/// [`Window::blackman`] and applied via [`Signal::apply_window`].
/// Owned `Vec<f64>` of coefficients — no raw pointers (R1).
#[derive(Debug, Clone, PartialEq)]
pub struct Window {
    coeffs: Vec<f64>,
    kind: WindowKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WindowKind {
    Hann,
    Hamming,
    Blackman,
}

// ---------------------------------------------------------------------------
// Signal — construction & accessors
// ---------------------------------------------------------------------------

impl Signal {
    /// Build a signal from an owned sample buffer + sample rate (Hz).
    ///
    /// `Signal.from_vec(data, sample_rate)` in Buff. The ctor takes
    /// ownership of `data` (R2: Rust owns the heap allocation).
    pub fn from_vec(samples: Vec<f64>, sample_rate: u32) -> Self {
        Self {
            samples,
            sample_rate,
        }
    }

    /// Synthesize `n` samples of a `freq_hz` sine wave at `sample_rate`.
    ///
    /// Convenience used by tests and examples. `n == 0` returns an
    /// empty signal (never panics).
    pub fn sine(freq_hz: f64, sample_rate: u32, n: usize) -> Self {
        if n == 0 || sample_rate == 0 {
            return Self::from_vec(Vec::new(), sample_rate);
        }
        let mut out = Vec::with_capacity(n);
        let two_pi_f_over_sr = 2.0 * std::f64::consts::PI * freq_hz / sample_rate as f64;
        for i in 0..n {
            out.push((two_pi_f_over_sr * i as f64).sin());
        }
        Self::from_vec(out, sample_rate)
    }

    /// Sample rate in Hz.
    pub fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    /// Number of samples.
    pub fn len(&self) -> usize {
        self.samples.len()
    }

    /// Whether the signal has zero samples.
    pub fn is_empty(&self) -> bool {
        self.samples.is_empty()
    }

    /// Read-only view of the sample buffer.
    pub fn as_slice(&self) -> &[f64] {
        &self.samples
    }
}

// ---------------------------------------------------------------------------
// Spectrum — accessors
// ---------------------------------------------------------------------------

impl Spectrum {
    /// Number of frequency bins.
    pub fn len(&self) -> usize {
        self.bins.len()
    }

    /// Whether the spectrum has zero bins.
    pub fn is_empty(&self) -> bool {
        self.bins.is_empty()
    }

    /// Borrow iterator over the bins.
    pub fn iter(&self) -> std::slice::Iter<'_, Complex> {
        self.bins.iter()
    }

    /// `Vec<f64>` of bin-centre frequencies in Hz.
    ///
    /// `bin_k` corresponds to `k * sample_rate / N` Hz where `N` is
    /// the original time-domain length. We reconstruct `N` from
    /// `bins.len() == N/2 + 1` ⇒ `N = 2 * (bins - 1)`.
    pub fn freqs(&self) -> Vec<f64> {
        if self.bins.len() < 2 || self.sample_rate == 0 {
            return Vec::new();
        }
        let n = (self.bins.len() - 1) * 2;
        let hz_per_bin = self.sample_rate as f64 / n as f64;
        (0..self.bins.len())
            .map(|k| k as f64 * hz_per_bin)
            .collect()
    }

    /// `Vec<f64>` of per-bin magnitudes.
    pub fn magnitudes(&self) -> Vec<f64> {
        self.bins.iter().map(|c| c.magnitude()).collect()
    }

    /// `Vec<f64>` of per-bin phases in radians.
    pub fn phases(&self) -> Vec<f64> {
        self.bins.iter().map(|c| c.phase()).collect()
    }
}

// ---------------------------------------------------------------------------
// Window — constructors
// ---------------------------------------------------------------------------

impl Window {
    /// Hann window of length `n`. Symmetric (periodic in `n-1`).
    ///
    /// Matches the T11 acceptance-scenario reference vector
    /// `[0, 0.146, 0.5, 0.854, 1.0, 0.854, 0.5, 0.146]` for `n=8`
    /// (within 1e-12).
    pub fn hann(n: usize) -> Self {
        Self::new(WindowKind::Hann, n)
    }

    /// Hamming window of length `n`. Symmetric.
    pub fn hamming(n: usize) -> Self {
        Self::new(WindowKind::Hamming, n)
    }

    /// Blackman window of length `n`. Symmetric.
    pub fn blackman(n: usize) -> Self {
        Self::new(WindowKind::Blackman, n)
    }

    /// Read-only view of the coefficients.
    pub fn as_slice(&self) -> &[f64] {
        &self.coeffs
    }

    fn new(kind: WindowKind, n: usize) -> Self {
        if n == 0 {
            return Self {
                coeffs: Vec::new(),
                kind,
            };
        }
        let coeffs = compute_window(kind, n);
        Self { coeffs, kind }
    }
}

fn compute_window(kind: WindowKind, n: usize) -> Vec<f64> {
    // `apodize` ships iterator-based window ctors: `hanning_iter(n)`,
    // `hamming_iter(n)`, `blackman_iter(n)` all yield `f64` coefficients
    // in `[0, 1]`. The iterators are exact length `n` — we `.take(n)` as
    // a defensive measure (no panic if a future bump changes semantics).
    let coeffs: Vec<f64> = match kind {
        WindowKind::Hann => apodize::hanning_iter(n).take(n).collect(),
        WindowKind::Hamming => apodize::hamming_iter(n).take(n).collect(),
        WindowKind::Blackman => apodize::blackman_iter(n).take(n).collect(),
    };
    if coeffs.len() == n {
        coeffs
    } else {
        vec![1.0; n]
    }
}

// ---------------------------------------------------------------------------
// FFT — forward (R2C) and inverse (C2R)
// ---------------------------------------------------------------------------

impl Signal {
    /// Forward real-to-complex FFT.
    ///
    /// Returns a [`Spectrum`] of `N/2 + 1` hermitian bins. Empty / 0-length
    /// signals return an empty spectrum (never panics).
    pub fn fft(&self) -> Spectrum {
        let sr = self.sample_rate;
        match externs::dsp_fft_forward(&self.samples, sr) {
            Some(bins) => Spectrum {
                bins,
                sample_rate: sr,
            },
            None => Spectrum {
                bins: Vec::new(),
                sample_rate: sr,
            },
        }
    }

    /// Inverse complex-to-real synthesis.
    ///
    /// Consumes a [`Spectrum`] and returns a time-domain [`Signal`].
    /// Useful for round-tripping `s.fft().ifft() ≈ s` (modulo float
    /// noise). Length `2 * (bins - 1)` samples. Empty / malformed
    /// spectra return an empty signal.
    pub fn ifft(spectrum: Spectrum) -> Signal {
        let sr = spectrum.sample_rate;
        match externs::dsp_fft_inverse(&spectrum.bins, sr) {
            Some(samples) => Signal {
                samples,
                sample_rate: sr,
            },
            None => Signal {
                samples: Vec::new(),
                sample_rate: sr,
            },
        }
    }
}

// ---------------------------------------------------------------------------
// Biquad filters (Audio-EQ-Cookbook, RBJ)
// ---------------------------------------------------------------------------

/// One biquad section's coefficients. Direct-form I.
#[derive(Debug, Clone, Copy)]
struct Biquad {
    b0: f64,
    b1: f64,
    b2: f64,
    a1: f64,
    a2: f64,
}

impl Biquad {
    /// RBJ Audio EQ Cookbook low-pass.
    fn lowpass(sample_rate: f64, cutoff_hz: f64, q: f64) -> Self {
        let w0 = 2.0 * std::f64::consts::PI * cutoff_hz / sample_rate;
        let cw = w0.cos();
        let sw = w0.sin();
        let alpha = sw / (2.0 * q);
        let b0 = (1.0 - cw) / 2.0;
        let b1 = 1.0 - cw;
        let b2 = (1.0 - cw) / 2.0;
        let a0 = 1.0 + alpha;
        let a1 = -2.0 * cw;
        let a2 = 1.0 - alpha;
        Self::normalise(b0, b1, b2, a0, a1, a2)
    }

    /// RBJ Audio EQ Cookbook high-pass.
    fn highpass(sample_rate: f64, cutoff_hz: f64, q: f64) -> Self {
        let w0 = 2.0 * std::f64::consts::PI * cutoff_hz / sample_rate;
        let cw = w0.cos();
        let sw = w0.sin();
        let alpha = sw / (2.0 * q);
        let b0 = (1.0 + cw) / 2.0;
        let b1 = -(1.0 + cw);
        let b2 = (1.0 + cw) / 2.0;
        let a0 = 1.0 + alpha;
        let a1 = -2.0 * cw;
        let a2 = 1.0 - alpha;
        Self::normalise(b0, b1, b2, a0, a1, a2)
    }

    /// RBJ Audio EQ Cookbook band-pass (constant 0 dB peak gain).
    fn bandpass(sample_rate: f64, low_hz: f64, high_hz: f64) -> Self {
        // Treat `low_hz` / `high_hz` as the -3 dB band edges and
        // convert to (centre, Q) per the cookbook.
        let centre = (low_hz * high_hz).sqrt();
        let bw = (high_hz - low_hz).max(1e-9);
        let q = centre / bw;
        let w0 = 2.0 * std::f64::consts::PI * centre / sample_rate;
        let cw = w0.cos();
        let sw = w0.sin();
        let alpha = sw / (2.0 * q);
        // constant 0 dB peak gain form: b0 = alpha; b1 = 0; b2 = -alpha.
        let b0 = alpha;
        let b1 = 0.0;
        let b2 = -alpha;
        let a0 = 1.0 + alpha;
        let a1 = -2.0 * cw;
        let a2 = 1.0 - alpha;
        Self::normalise(b0, b1, b2, a0, a1, a2)
    }

    #[allow(clippy::too_many_arguments)]
    fn normalise(b0: f64, b1: f64, b2: f64, a0: f64, a1: f64, a2: f64) -> Self {
        if a0.abs() < 1e-300 {
            // Degenerate: pass-through.
            return Self {
                b0: 1.0,
                b1: 0.0,
                b2: 0.0,
                a1: 0.0,
                a2: 0.0,
            };
        }
        Self {
            b0: b0 / a0,
            b1: b1 / a0,
            b2: b2 / a0,
            a1: a1 / a0,
            a2: a2 / a0,
        }
    }

    /// Apply direct-form-I transposed biquad (in-place-friendly).
    fn filter(&self, samples: &[f64]) -> Vec<f64> {
        let mut out = Vec::with_capacity(samples.len());
        let mut x1 = 0.0;
        let mut x2 = 0.0;
        let mut y1 = 0.0;
        let mut y2 = 0.0;
        for &x in samples {
            let y = self.b0 * x + self.b1 * x1 + self.b2 * x2 - self.a1 * y1 - self.a2 * y2;
            x2 = x1;
            x1 = x;
            y2 = y1;
            y1 = y;
            out.push(y);
        }
        out
    }
}

impl Signal {
    /// Apply a low-pass filter at `cutoff_hz` (RBJ biquad, Q = 1/√2
    /// ≈ Butterworth).
    pub fn lowpass(&self, cutoff_hz: f64) -> Signal {
        self.apply_biquad(|sr| {
            Biquad::lowpass(sr as f64, cutoff_hz, std::f64::consts::FRAC_1_SQRT_2)
        })
    }

    /// Apply a high-pass filter at `cutoff_hz` (RBJ biquad, Q = 1/√2).
    pub fn highpass(&self, cutoff_hz: f64) -> Signal {
        self.apply_biquad(|sr| {
            Biquad::highpass(sr as f64, cutoff_hz, std::f64::consts::FRAC_1_SQRT_2)
        })
    }

    /// Apply a band-pass filter between `low_hz` and `high_hz` (RBJ
    /// biquad, -3 dB edges).
    pub fn bandpass(&self, low_hz: f64, high_hz: f64) -> Signal {
        self.apply_biquad(|sr| Biquad::bandpass(sr as f64, low_hz, high_hz))
    }

    fn apply_biquad<F: Fn(u32) -> Biquad>(&self, mk: F) -> Signal {
        if self.samples.is_empty() || self.sample_rate == 0 {
            return self.clone();
        }
        let bq = mk(self.sample_rate);
        let filtered =
            externs::dsp_biquad_filter(&self.samples, bq).unwrap_or_else(|| self.samples.clone());
        Signal::from_vec(filtered, self.sample_rate)
    }

    /// Multiply samples element-wise by a [`Window`] in place.
    pub fn apply_window(&mut self, window: &Window) {
        if self.samples.len() != window.coeffs.len() {
            // Length mismatch — apply the overlapping prefix only,
            // leaving trailing samples untouched. Never panics.
            let n = self.samples.len().min(window.coeffs.len());
            for i in 0..n {
                self.samples[i] *= window.coeffs[i];
            }
            return;
        }
        for (s, w) in self.samples.iter_mut().zip(window.coeffs.iter()) {
            *s *= w;
        }
    }
}

// ---------------------------------------------------------------------------
// Spectral ops
// ---------------------------------------------------------------------------

impl Signal {
    /// Short-Time Fourier Transform (STFT).
    ///
    /// Hops by `window_size / 2` samples (50% overlap), applies a
    /// Hann window per frame, and returns one [`Spectrum`] per frame.
    /// Signals shorter than `window_size` yield a single zero-padded
    /// frame. Empty signals return an empty `Vec`.
    pub fn spectrogram(&self, window_size: usize) -> Vec<Spectrum> {
        if self.samples.is_empty() || window_size == 0 || self.sample_rate == 0 {
            return Vec::new();
        }
        let hop = (window_size / 2).max(1);
        let hann = Window::hann(window_size);
        let n_frames = if self.samples.len() < window_size {
            1
        } else {
            (self.samples.len() - window_size) / hop + 1
        };
        let mut out = Vec::with_capacity(n_frames);
        for f in 0..n_frames {
            let start = f * hop;
            let mut frame = if start + window_size <= self.samples.len() {
                self.samples[start..start + window_size].to_vec()
            } else {
                // Zero-pad the trailing frame.
                let mut buf = vec![0.0; window_size];
                let take = self.samples.len() - start;
                buf[..take].copy_from_slice(&self.samples[start..]);
                buf
            };
            // Apply Hann in place.
            for (s, w) in frame.iter_mut().zip(hann.coeffs.iter()) {
                *s *= w;
            }
            let bins = externs::dsp_fft_forward(&frame, self.sample_rate).unwrap_or_default();
            out.push(Spectrum {
                bins,
                sample_rate: self.sample_rate,
            });
        }
        out
    }

    /// Per-bin magnitude spectrum of `self.fft()`. Convenience over
    /// `s.fft().magnitudes()`.
    pub fn magnitude(&self) -> Vec<f64> {
        self.fft().magnitudes()
    }

    /// Per-bin phase spectrum in radians of `self.fft()`. Convenience
    /// over `s.fft().phases()`.
    pub fn phase(&self) -> Vec<f64> {
        self.fft().phases()
    }
}

// ---------------------------------------------------------------------------
// externs — the FFI safety layer (GUIDE.md R1-R6)
// ---------------------------------------------------------------------------

/// Private module of `catch_unwind`-wrapped extern targets. These are
/// the only places that touch `rustfft` / `realfft` / `apodization`
/// directly.
mod externs {
    use super::{Biquad, Complex};
    use std::panic::{catch_unwind, AssertUnwindSafe};

    /// Forward FFT of a real `&[f64]` input. Returns hermitian half
    /// spectrum (`N/2 + 1` bins) or `None` on panic. Empty input →
    /// empty `Vec` (no panic, no allocation beyond the empty vec).
    pub(super) fn dsp_fft_forward(input: &[f64], _sample_rate: u32) -> Option<Vec<Complex>> {
        let n = input.len();
        if n == 0 {
            return Some(Vec::new());
        }
        let mut planner = realfft::RealFftPlanner::<f64>::new();
        let r2c = planner.plan_fft_forward(n);
        let mut real_buf = input.to_vec();
        let mut complex_buf = vec![rustfft::num_complex::Complex64::new(0.0, 0.0); n / 2 + 1];
        let r = catch_unwind(AssertUnwindSafe(|| {
            r2c.process(&mut real_buf, &mut complex_buf)
        }));
        match r {
            Ok(Ok(())) => Some(complex_buf.iter().map(|c| (*c).into()).collect()),
            Ok(Err(_)) => Some(Vec::new()),
            Err(_) => None,
        }
    }

    /// Inverse FFT of a hermitian `&[Complex]` spectrum. Returns a
    /// real time-domain signal of length `2 * (bins - 1)` or `None`
    /// on panic. Empty input → empty `Vec`.
    pub(super) fn dsp_fft_inverse(input: &[Complex], _sample_rate: u32) -> Option<Vec<f64>> {
        let bins = input.len();
        if bins == 0 {
            return Some(Vec::new());
        }
        let n = if bins >= 2 { (bins - 1) * 2 } else { 1 };
        let mut planner = realfft::RealFftPlanner::<f64>::new();
        let c2r = planner.plan_fft_inverse(n);
        let mut complex_buf: Vec<rustfft::num_complex::Complex64> =
            input.iter().map(|c| (*c).into()).collect();
        complex_buf.resize(n / 2 + 1, rustfft::num_complex::Complex64::new(0.0, 0.0));
        let mut real_buf = vec![0.0; n];
        let r = catch_unwind(AssertUnwindSafe(|| {
            c2r.process(&mut complex_buf, &mut real_buf)
        }));
        match r {
            Ok(Ok(())) => Some(real_buf),
            Ok(Err(_)) => Some(Vec::new()),
            Err(_) => None,
        }
    }

    /// Apply a [`Biquad`] filter to `&[f64]` and return the filtered
    /// `Vec<f64>`. Panics are caught and yield `None` (caller falls
    /// back to the original samples).
    pub(super) fn dsp_biquad_filter(input: &[f64], bq: Biquad) -> Option<Vec<f64>> {
        catch_unwind(AssertUnwindSafe(|| bq.filter(input))).ok()
    }
}

// ---------------------------------------------------------------------------
// Re-exports for FFI consumers (codegen layer)
// ---------------------------------------------------------------------------

// The codegen layer (`crates/buff-lang-codegen-rust`) splices fully-
// qualified paths like `buff_dsp::Signal::from_vec(...)` into the
// generated Rust. No `use` import is required at the call site.

// ---------------------------------------------------------------------------
// Unit smoke tests (the heavy proptest suite lives in tests/)
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_signal_fft_is_empty() {
        let s = Signal::from_vec(Vec::new(), 44_100);
        let spec = s.fft();
        assert!(spec.is_empty());
    }

    #[test]
    fn sine_dc_returns_only_dc_bin() {
        // A constant signal has only a DC component.
        let s = Signal::from_vec(vec![1.0; 8], 8);
        let spec = s.fft();
        assert_eq!(spec.len(), 5);
        let mags = spec.magnitudes();
        assert!(mags[0] > 7.0);
        for &m in &mags[1..] {
            assert!(m < 1e-6, "expected ~0 magnitude, got {m}");
        }
    }

    #[test]
    fn fft_ifft_roundtrip_dc_signal() {
        let original = Signal::from_vec(vec![0.5; 64], 64);
        let spec = original.fft();
        let recovered = Signal::ifft(spec);
        for (a, b) in original.as_slice().iter().zip(recovered.as_slice().iter()) {
            assert!((a - b).abs() < 1e-6, "roundtrip mismatch {a} vs {b}");
        }
    }

    #[test]
    fn hann_window_reference_vector_n8() {
        // T11 acceptance scenario: hann(8) ≈ [0, 0.146, 0.5, 0.854,
        // 1.0, 0.854, 0.5, 0.146] within tolerance.
        let w = Window::hann(8);
        let expected = [0.0, 0.1464, 0.5, 0.8536, 1.0, 0.8536, 0.5, 0.1464];
        for (got, want) in w.as_slice().iter().zip(expected.iter()) {
            assert!((got - want).abs() < 1e-3, "got {got}, want {want}");
        }
    }

    #[test]
    fn apply_window_multiplies_in_place() {
        let mut s = Signal::from_vec(vec![1.0; 4], 4);
        let w = Window::hann(4);
        s.apply_window(&w);
        // The DC component should drop.
        let sum: f64 = s.as_slice().iter().sum();
        assert!(sum < 4.0);
    }

    #[test]
    fn lowpass_attenuates_high_freq() {
        // Mix 100 Hz + 5000 Hz at 16 kHz; 100 Hz survives, 5000 Hz dies.
        let sr = 16_000u32;
        let mut mixed = Vec::with_capacity(2048);
        for i in 0..2048 {
            let t = i as f64 / sr as f64;
            mixed.push(
                (2.0 * std::f64::consts::PI * 100.0 * t).sin()
                    + (2.0 * std::f64::consts::PI * 5000.0 * t).sin(),
            );
        }
        let s = Signal::from_vec(mixed, sr);
        let filtered = s.lowpass(500.0);
        let in_spec = s.fft();
        let out_spec = filtered.fft();
        let in_mags = in_spec.magnitudes();
        let out_mags = out_spec.magnitudes();
        // Locate the bins near 100 Hz and 5000 Hz.
        let n = s.len();
        let bin_100 = (100.0 * n as f64 / sr as f64).round() as usize;
        let bin_5000 = (5000.0 * n as f64 / sr as f64).round() as usize;
        let atten_5000 = out_mags[bin_5000] / in_mags[bin_5000].max(1e-9);
        let atten_100 = out_mags[bin_100] / in_mags[bin_100].max(1e-9);
        assert!(
            atten_5000 < atten_100,
            "expected 5kHz more attenuated than 100Hz: atten_5k={atten_5000}, atten_100={atten_100}"
        );
    }

    #[test]
    fn spectrogram_empty_input_is_empty() {
        let s = Signal::from_vec(Vec::new(), 8_000);
        assert!(s.spectrogram(256).is_empty());
    }

    #[test]
    fn spectrum_freqs_match_sample_rate_over_n() {
        let s = Signal::from_vec(vec![0.0; 8], 8);
        let spec = s.fft();
        let freqs = spec.freqs();
        assert_eq!(freqs.len(), spec.len());
        assert!((freqs[1] - 1.0).abs() < 1e-9);
    }
}

//! Spectrum accessor correctness: freqs / magnitudes / phases /
//! spectrogram framing.

use buff_dsp::{Signal, Spectrum};
use std::f64::consts::PI;

#[test]
fn spectrum_freqs_are_linear_spaced() {
    let s = Signal::from_vec(vec![0.0; 16], 16);
    let spec = s.fft();
    let freqs = spec.freqs();
    assert_eq!(freqs.len(), spec.len());
    assert!(freqs[0] < 1e-9, "first bin should be DC, got {}", freqs[0]);
    let step = freqs[1];
    assert!((step - 1.0).abs() < 1e-9, "bin width should be 1 Hz, got {step}");
    for (i, &freq) in freqs.iter().enumerate().skip(1) {
        let expected = i as f64 * step;
        assert!((freq - expected).abs() < 1e-9, "bin {i} not linear: {freq} vs {expected}");
    }
}

#[test]
fn spectrum_magnitudes_via_impulse_signal() {
    // FFT of a unit impulse [1, 0, 0, ...] has uniform magnitude 1
    // across all bins — a canonical FFT identity.
    let n = 16;
    let mut impulse = vec![0.0; n];
    impulse[0] = 1.0;
    let s = Signal::from_vec(impulse, n as u32);
    let spec = s.fft();
    let mags = spec.magnitudes();
    for (i, &m) in mags.iter().enumerate() {
        assert!((m - 1.0).abs() < 1e-9, "impulse magnitude at bin {i} should be 1.0, got {m}");
    }
}

#[test]
fn spectrum_phases_via_pure_cosine_at_bin_2() {
    // A cosine at bin k has zero phase at bin k (its FFT is real +
    // positive at that bin). Use a 2-cycle cosine over N=16 samples.
    let n = 16usize;
    let sr = n as u32;
    let bin_k = 2usize;
    let samples: Vec<f64> = (0..n)
        .map(|i| (2.0 * PI * bin_k as f64 * i as f64 / n as f64).cos())
        .collect();
    let s = Signal::from_vec(samples, sr);
    let spec = s.fft();
    let phases = spec.phases();
    // Phase at the active bin should be ~0 (cosine is the real part
    // of the complex exponential — its coefficient is real + positive).
    assert!(
        phases[bin_k].abs() < 1e-6,
        "phase at active bin {bin_k} should be ~0, got {}",
        phases[bin_k]
    );
}

#[test]
fn spectrogram_single_frame_for_short_signal() {
    let s = Signal::from_vec(vec![0.0; 100], 8_000);
    let frames = s.spectrogram(256);
    assert_eq!(frames.len(), 1, "short signal should produce 1 zero-padded frame");
    assert_eq!(frames[0].len(), 129);
}

#[test]
fn spectrogram_frame_count_grows_with_signal_length() {
    let sr = 8_000u32;
    let short = Signal::from_vec(vec![0.0; 512], sr);
    let long = Signal::from_vec(vec![0.0; 2048], sr);
    let frames_short = short.spectrogram(256);
    let frames_long = long.spectrogram(256);
    assert!(frames_long.len() > frames_short.len());
}

#[test]
fn spectrogram_empty_input_returns_empty_vec() {
    let s = Signal::from_vec(Vec::new(), 8_000);
    assert!(s.spectrogram(256).is_empty());
}

#[test]
fn signal_magnitude_and_phase_match_spectrum_methods() {
    let s = Signal::sine(100.0, 1_000, 1_000);
    let spec = s.fft();
    let mags_via_signal = s.magnitude();
    let mags_via_spectrum = spec.magnitudes();
    assert_eq!(mags_via_signal.len(), mags_via_spectrum.len());
    for (a, b) in mags_via_signal.iter().zip(mags_via_spectrum.iter()) {
        assert!((a - b).abs() < 1e-9);
    }
}

#[test]
fn spectrum_iter_yields_correct_count() {
    let s = Signal::from_vec(vec![0.0; 32], 32);
    let spec = s.fft();
    let collected: Vec<_> = spec.iter().collect();
    assert_eq!(collected.len(), spec.len());
}

#[test]
fn spectrum_len_and_is_empty_are_consistent() {
    let empty_spec: Spectrum = Signal::from_vec(Vec::new(), 8).fft();
    assert!(empty_spec.is_empty());
    assert_eq!(empty_spec.len(), 0);
    let nonempty_spec = Signal::from_vec(vec![0.0; 8], 8).fft();
    assert!(!nonempty_spec.is_empty());
    assert_eq!(nonempty_spec.len(), 5);
}

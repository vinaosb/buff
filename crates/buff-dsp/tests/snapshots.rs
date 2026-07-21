//! Snapshot tests using insta. Frozen reference vectors for:
//!   1. `hann(8)` coefficients
//!   2. `hamming(8)` coefficients
//!   3. `blackman(8)` coefficients
//!   4. DC-signal FFT magnitudes (length 8)
//!   5. Impulse-signal FFT magnitudes (length 16, all should be 1.0)
//!   6. Spectrum freqs for N=8 / sr=8

#![cfg(test)]

use buff_dsp::Signal;
use std::format;

fn format_f64_slice(xs: &[f64]) -> String {
    xs.iter()
        .map(|x| format!("{:>10.6}", x))
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn snap_hann_window_n8() {
    let w = buff_dsp::Window::hann(8);
    insta::assert_snapshot!("hann_n8", format_f64_slice(w.as_slice()));
}

#[test]
fn snap_hamming_window_n8() {
    let w = buff_dsp::Window::hamming(8);
    insta::assert_snapshot!("hamming_n8", format_f64_slice(w.as_slice()));
}

#[test]
fn snap_blackman_window_n8() {
    let w = buff_dsp::Window::blackman(8);
    insta::assert_snapshot!("blackman_n8", format_f64_slice(w.as_slice()));
}

#[test]
fn snap_dc_signal_fft_magnitudes_n8() {
    let s = Signal::from_vec(vec![1.0; 8], 8);
    let mags = s.fft().magnitudes();
    insta::assert_snapshot!("dc_fft_mags_n8", format_f64_slice(&mags));
}

#[test]
fn snap_impulse_fft_magnitudes_n16() {
    let mut impulse = vec![0.0; 16];
    impulse[0] = 1.0;
    let s = Signal::from_vec(impulse, 16);
    let mags = s.fft().magnitudes();
    insta::assert_snapshot!("impulse_fft_mags_n16", format_f64_slice(&mags));
}

#[test]
fn snap_spectrum_freqs_n8_sr8() {
    let s = Signal::from_vec(vec![0.0; 8], 8);
    let freqs = s.fft().freqs();
    insta::assert_snapshot!("spectrum_freqs_n8_sr8", format_f64_slice(&freqs));
}

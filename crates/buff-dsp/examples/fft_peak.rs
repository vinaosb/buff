//! T11 QA scenario 1: FFT of a pure 440 Hz sine wave produces a
//! peaked spectrum near bin 440.
//!
//! Generates 1 second of 440 Hz sine at 44100 Hz sample rate,
//! computes the FFT, and asserts the peak magnitude occurs near
//! bin 440 (within tolerance).
//!
//! Run: `cargo run --example fft_peak -p buff-dsp`

use buff_dsp::Signal;

fn main() {
    let sample_rate = 44_100u32;
    let n = sample_rate as usize;
    let signal = Signal::sine(440.0, sample_rate, n);

    let spectrum = signal.fft();
    let mags = spectrum.magnitudes();

    let peak_bin = mags
        .iter()
        .enumerate()
        .skip(1)
        .max_by(|(_, a), (_, b)| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal))
        .map(|(i, _)| i)
        .unwrap_or(0);

    let bin_hz = sample_rate as f64 / n as f64;
    let peak_hz = peak_bin as f64 * bin_hz;

    println!("sample_rate  = {sample_rate} Hz");
    println!("n_samples    = {n}");
    println!("n_bins       = {}", spectrum.len());
    println!("peak_bin     = {peak_bin}");
    println!("peak_hz      = {peak_hz:.2}");
    println!("bin_width_hz = {bin_hz:.4}");

    let tolerance_bins = (10.0 * bin_hz).round() as usize;
    let distance = peak_bin.saturating_sub(440);
    assert!(
        distance <= tolerance_bins,
        "expected peak near bin 440 (±{tolerance_bins}), got bin {peak_bin} ({peak_hz:.2} Hz)"
    );

    println!("PASS: FFT identifies the 440 Hz component within tolerance.");
}

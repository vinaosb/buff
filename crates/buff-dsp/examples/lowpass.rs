//! T11 QA scenario 3: Lowpass filter attenuates high frequencies.
//!
//! Mixes 100 Hz + 5000 Hz sines at 16 kHz, applies a low-pass at
//! 500 Hz, then FFTs both input and output. Asserts the 5000 Hz
//! bin is attenuated more than the 100 Hz bin.
//!
//! Run: `cargo run --example lowpass -p buff-dsp`

use buff_dsp::Signal;
use std::f64::consts::PI;

fn main() {
    let sr = 16_000u32;
    let n = 4_096usize;

    let mut mixed = Vec::with_capacity(n);
    for i in 0..n {
        let t = i as f64 / sr as f64;
        let low = (2.0 * PI * 100.0 * t).sin();
        let high = (2.0 * PI * 5_000.0 * t).sin();
        mixed.push(low + high);
    }
    let signal = Signal::from_vec(mixed, sr);
    let filtered = signal.lowpass(500.0);

    let in_mags = signal.fft().magnitudes();
    let out_mags = filtered.fft().magnitudes();

    let bin_100 = (100.0 * n as f64 / sr as f64).round() as usize;
    let bin_5000 = (5_000.0 * n as f64 / sr as f64).round() as usize;

    let atten_100 = out_mags[bin_100] / in_mags[bin_100].max(1e-9);
    let atten_5000 = out_mags[bin_5000] / in_mags[bin_5000].max(1e-9);

    println!("sample_rate  = {sr} Hz");
    println!("n_samples    = {n}");
    println!(
        "bin_100      = {bin_100}  ({:.2} Hz)",
        bin_100 as f64 * sr as f64 / n as f64
    );
    println!(
        "bin_5000     = {bin_5000} ({:.2} Hz)",
        bin_5000 as f64 * sr as f64 / n as f64
    );
    println!("atten_100    = {atten_100:.4}");
    println!("atten_5000   = {atten_5000:.4}");

    assert!(
        atten_5000 < atten_100,
        "expected 5000 Hz bin attenuated more than 100 Hz bin \
         (atten_5000={atten_5000:.4} < atten_100={atten_100:.4})"
    );
    println!("PASS: lowpass attenuates 5000 Hz more than 100 Hz.");
}

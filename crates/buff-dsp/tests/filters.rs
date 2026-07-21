//! Biquad filter correctness. The three filters (lowpass / highpass /
//! bandpass) share the RBJ Audio EQ Cookbook coefficients — we verify
//! their behaviour by checking the gain at DC, Nyquist, and the
//! passband centre.

use buff_dsp::Signal;
use std::f64::consts::PI;

#[test]
fn lowpass_passes_dc_attenuates_nyquist() {
    // A low-pass at 100 Hz with sr=1000 should pass DC (gain≈1) and
    // heavily attenuate Nyquist (500 Hz).
    let sr = 1_000u32;
    let dc = Signal::from_vec(vec![1.0; 64], sr);
    let nyq = Signal::sine(sr as f64 / 2.0 * 0.95, sr, 64);
    let dc_out = dc.lowpass(100.0);
    let nyq_out = nyq.lowpass(100.0);
    let dc_gain = rms(dc_out.as_slice()) / rms(dc.as_slice()).max(1e-9);
    let nyq_gain = rms(nyq_out.as_slice()) / rms(nyq.as_slice()).max(1e-9);
    assert!(dc_gain > 0.99, "lowpass should pass DC: dc_gain={dc_gain}");
    assert!(nyq_gain < 0.5, "lowpass should attenuate Nyquist: nyq_gain={nyq_gain}");
}

#[test]
fn highpass_attenuates_dc_passes_nyquist() {
    let sr = 1_000u32;
    let dc = Signal::from_vec(vec![1.0; 64], sr);
    let nyq = Signal::sine(sr as f64 / 2.0 * 0.95, sr, 64);
    let dc_out = dc.highpass(100.0);
    let nyq_out = nyq.highpass(100.0);
    let dc_gain = rms(dc_out.as_slice()) / rms(dc.as_slice()).max(1e-9);
    let nyq_gain = rms(nyq_out.as_slice()) / rms(nyq.as_slice()).max(1e-9);
    assert!(dc_gain < 0.5, "highpass should attenuate DC: dc_gain={dc_gain}");
    assert!(nyq_gain > 0.5, "highpass should pass Nyquist: nyq_gain={nyq_gain}");
}

#[test]
fn bandpass_passes_centre_attenuates_outside() {
    let sr = 4_000u32;
    let n = 256;
    let centre = 400.0_f64;
    let outside = 50.0_f64;
    let sig_centre = Signal::sine(centre, sr, n);
    let sig_outside = Signal::sine(outside, sr, n);
    let bp_centre = sig_centre.bandpass(300.0, 500.0);
    let bp_outside = sig_outside.bandpass(300.0, 500.0);
    let gain_centre = rms(bp_centre.as_slice()) / rms(sig_centre.as_slice()).max(1e-9);
    let gain_outside = rms(bp_outside.as_slice()) / rms(sig_outside.as_slice()).max(1e-9);
    assert!(
        gain_centre > gain_outside,
        "bandpass should pass centre {centre} Hz more than outside {outside} Hz: gc={gain_centre} go={gain_outside}"
    );
}

#[test]
fn lowpass_preserves_length_and_sample_rate() {
    let s = Signal::from_vec(vec![0.0; 128], 8_000);
    let out = s.lowpass(1_000.0);
    assert_eq!(out.len(), 128);
    assert_eq!(out.sample_rate(), 8_000);
}

#[test]
fn empty_signal_lowpass_is_empty() {
    let s = Signal::from_vec(Vec::new(), 8_000);
    let out = s.lowpass(1_000.0);
    assert!(out.is_empty());
}

#[test]
fn lowpass_cascade_attenuates_more() {
    // Two stacked low-passes attenuate high frequencies more than one.
    let sr = 4_000u32;
    let n = 256;
    let sig = Signal::sine(1_500.0, sr, n);
    let one_pass = sig.lowpass(500.0);
    let two_pass = one_pass.lowpass(500.0);
    let gain_one = rms(one_pass.as_slice()) / rms(sig.as_slice()).max(1e-9);
    let gain_two = rms(two_pass.as_slice()) / rms(sig.as_slice()).max(1e-9);
    assert!(gain_two < gain_one, "two passes should attenuate more: g1={gain_one} g2={gain_two}");
}

fn rms(xs: &[f64]) -> f64 {
    if xs.is_empty() {
        return 0.0;
    }
    let sum_sq: f64 = xs.iter().map(|x| x * x).sum();
    (sum_sq / xs.len() as f64).sqrt()
}

#[test]
fn avoid_unused_pi_warning() {
    let _ = PI;
}

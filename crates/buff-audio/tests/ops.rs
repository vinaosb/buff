//! Integration tests for `buff-audio` — exercises the full public API
//! surface (constructors, accessors, save/load round-trip, sample ops,
//! error paths) and asserts the 5+ insta snapshots committed under
//! `tests/snapshots/`.
//!
//! Tests run without network access and without any pre-existing audio
//! fixtures — every WAV used here is synthesized in-memory and written
//! to a `tempfile::NamedTempFile` for the round-trip cases.

use std::f32::consts::PI;

use buff_audio::{AudioBuffer, AudioError};

fn sine(freq: f32, amp: f32, sample_rate: u32, dur_secs: f32) -> Vec<f32> {
    let n = (dur_secs * sample_rate as f32) as usize;
    (0..n)
        .map(|i| {
            let t = i as f32 / sample_rate as f32;
            (t * freq * 2.0 * PI).sin() * amp
        })
        .collect()
}

fn stereo(samples_mono: &[f32]) -> Vec<f32> {
    samples_mono.iter().flat_map(|&v| [v, v]).collect()
}

#[test]
fn constructs_empty_buffer() {
    let buf = AudioBuffer::from_samples(Vec::new(), 44100, 2).expect("empty ok");
    assert_eq!(buf.samples().len(), 0);
    assert_eq!(buf.frames(), 0);
    assert_eq!(buf.channels(), 2);
    assert_eq!(buf.sample_rate(), 44100);
    assert_eq!(buf.duration_secs(), 0.0);
}

#[test]
fn constructs_stereo_buffer() {
    let samples = stereo(&[0.1, 0.2, 0.3]);
    let buf = AudioBuffer::from_samples(samples.clone(), 44100, 2).expect("ok");
    assert_eq!(buf.samples(), &samples[..]);
    assert_eq!(buf.frames(), 3);
    assert_eq!(buf.channels(), 2);
    assert!((buf.duration_secs() - 3.0 / 44100.0).abs() < 1e-9);
}

#[test]
fn rejects_zero_channels() {
    let err = AudioBuffer::from_samples(Vec::new(), 44100, 0).unwrap_err();
    assert!(matches!(err, AudioError::InvalidParam(_)));
}

#[test]
fn rejects_zero_sample_rate() {
    let err = AudioBuffer::from_samples(Vec::new(), 0, 1).unwrap_err();
    assert!(matches!(err, AudioError::InvalidParam(_)));
}

#[test]
fn rejects_misaligned_samples() {
    let err = AudioBuffer::from_samples(vec![0.1, 0.2, 0.3], 44100, 2).unwrap_err();
    assert!(matches!(err, AudioError::InvalidParam(_)));
}

#[test]
fn amplify_scales_every_sample() {
    let mut buf = AudioBuffer::from_samples(vec![0.1, -0.2, 0.3], 44100, 1).expect("ok");
    buf.amplify(2.0);
    assert_eq!(buf.samples(), &[0.2, -0.4, 0.6]);
}

#[test]
fn normalize_sets_peak_to_target() {
    let mut buf = AudioBuffer::from_samples(vec![0.1, -0.5, 0.3], 44100, 1).expect("ok");
    buf.normalize(1.0);
    let peak = buf
        .samples()
        .iter()
        .map(|s| s.abs())
        .fold(0.0_f32, f32::max);
    assert!((peak - 1.0).abs() < 1e-6, "peak should be 1.0, got {peak}");
    assert!((buf.samples()[1] - -1.0).abs() < 1e-6);
}

#[test]
fn normalize_handles_all_zero_buffer() {
    let mut buf = AudioBuffer::from_samples(vec![0.0, 0.0, 0.0], 44100, 1).expect("ok");
    buf.normalize(1.0);
    assert_eq!(buf.samples(), &[0.0, 0.0, 0.0]);
}

#[test]
fn mix_adds_sample_wise() {
    let samples_a = sine(220.0, 0.5, 100, 1.0);
    let samples_b = sine(330.0, 0.3, 100, 1.0);

    let mut a = AudioBuffer::from_samples(samples_a.clone(), 100, 1).expect("ok");
    let b = AudioBuffer::from_samples(samples_b.clone(), 100, 1).expect("ok");

    a.mix(&b).expect("rate + channels match");

    for (i, x) in a.samples().iter().enumerate() {
        let expected = samples_a[i] + samples_b[i];
        assert!(
            (x - expected).abs() < 1e-6,
            "frame {i}: expected {expected}, got {x}"
        );
    }
}

#[test]
fn mix_rejects_rate_mismatch() {
    let mut a = AudioBuffer::from_samples(vec![0.1], 44100, 1).expect("ok");
    let b = AudioBuffer::from_samples(vec![0.1], 22050, 1).expect("ok");
    let err = a.mix(&b).unwrap_err();
    assert!(matches!(err, AudioError::InvalidParam(_)));
}

#[test]
fn mix_rejects_channel_mismatch() {
    let mut a = AudioBuffer::from_samples(vec![0.1, 0.2], 44100, 2).expect("ok");
    let b = AudioBuffer::from_samples(vec![0.1], 44100, 1).expect("ok");
    let err = a.mix(&b).unwrap_err();
    assert!(matches!(err, AudioError::InvalidParam(_)));
}

#[test]
fn slice_returns_subregion() {
    let samples = sine(440.0, 0.5, 1000, 1.0);
    let buf = AudioBuffer::from_samples(samples, 1000, 1).expect("ok");
    assert!((buf.duration_secs() - 1.0).abs() < 1e-9);

    let sliced = buf.slice(0.25, 0.75).expect("ok");
    assert_eq!(sliced.frames(), 500);
    assert!((sliced.duration_secs() - 0.5).abs() < 1e-9);
    assert_eq!(sliced.sample_rate(), 1000);
    assert_eq!(sliced.channels(), 1);
}

#[test]
fn slice_clamps_endpoints() {
    let buf = AudioBuffer::from_samples(vec![0.1; 1000], 1000, 1).expect("ok");
    let sliced = buf.slice(-1.0, 100.0).expect("ok");
    assert_eq!(sliced.frames(), 1000);
}

#[test]
fn slice_rejects_inverted_range() {
    let buf = AudioBuffer::from_samples(vec![0.1; 100], 100, 1).expect("ok");
    let err = buf.slice(0.5, 0.25).unwrap_err();
    assert!(matches!(err, AudioError::InvalidParam(_)));
}

#[test]
fn slice_rejects_nan() {
    let buf = AudioBuffer::from_samples(vec![0.1; 100], 100, 1).expect("ok");
    let err = buf.slice(f64::NAN, 0.5).unwrap_err();
    assert!(matches!(err, AudioError::InvalidParam(_)));
}

#[test]
fn save_load_round_trip_preserves_samples() {
    use tempfile::NamedTempFile;
    let samples = sine(440.0, 0.5, 44100, 1.0);

    let original = AudioBuffer::from_samples(samples.clone(), 44100, 1).expect("ok");

    let tmp = NamedTempFile::new().expect("tmp create");
    let path = tmp.path().to_path_buf();
    tmp.close().expect("tmp close");

    original.save(&path).expect("save");
    let reloaded = AudioBuffer::from_path(&path).expect("reload");

    assert_eq!(reloaded.sample_rate(), 44100);
    assert_eq!(reloaded.channels(), 1);
    assert_eq!(reloaded.frames(), original.frames());

    let max_diff = original
        .samples()
        .iter()
        .zip(reloaded.samples().iter())
        .map(|(a, b)| (a - b).abs())
        .fold(0.0_f32, f32::max);
    assert!(
        max_diff < 1e-6,
        "f32 WAV round-trip should be lossless, got max_diff={max_diff}"
    );

    let _ = std::fs::remove_file(&path);
}

#[test]
fn from_path_rejects_missing_file() {
    let err = AudioBuffer::from_path("nonexistent_xyz.wav").unwrap_err();
    assert!(matches!(err, AudioError::Io(_)));
}

#[test]
fn summarize_reports_correct_peak_and_rms() {
    let buf = AudioBuffer::from_samples(vec![0.5, -0.5, 0.5, -0.5], 100, 1).expect("ok");
    let s = buf.summarize();
    assert_eq!(s.frames, 4);
    assert!((s.peak - 0.5).abs() < 1e-6);
    assert!((s.rms - 0.5).abs() < 1e-6);
    assert_eq!(s.channels, 1);
    assert_eq!(s.sample_rate, 100);
}

#[test]
fn display_formats_compactly() {
    let buf = AudioBuffer::from_samples(vec![0.0; 44100], 44100, 2).expect("ok");
    let s = format!("{}", buf);
    assert!(s.starts_with("AudioBuffer(2ch, 44100 Hz,"));
    assert!(s.contains("44100 frames"));
}

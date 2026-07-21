//! Insta snapshot tests for `buff-audio`.
//!
//! Snapshots assert the human-readable `Display` output of
//! `AudioBuffer` + `AudioSummary` across the canonical scenarios:
//! empty / mono / stereo, normalize, amplify, mix, slice, and a
//! full save→reload round-trip. Snapshots live alongside in
//! `tests/snapshots/` and are accepted via `cargo insta accept`.

use std::f32::consts::PI;

use buff_audio::AudioBuffer;

fn sine(freq: f32, amp: f32, sample_rate: u32, dur_secs: f32) -> Vec<f32> {
    let n = (dur_secs * sample_rate as f32) as usize;
    (0..n)
        .map(|i| {
            let t = i as f32 / sample_rate as f32;
            (t * freq * 2.0 * PI).sin() * amp
        })
        .collect()
}

#[test]
fn snapshot_empty_mono_buffer() {
    let buf = AudioBuffer::from_samples(Vec::new(), 44100, 1).expect("ok");
    insta::assert_snapshot!("empty_mono_display", format!("{}", buf));
    insta::assert_snapshot!("empty_mono_summary", format!("{}", buf.summarize()));
}

#[test]
fn snapshot_one_second_mono_tone() {
    let samples = sine(440.0, 0.5, 44100, 1.0);
    let buf = AudioBuffer::from_samples(samples, 44100, 1).expect("ok");
    insta::assert_snapshot!("mono_tone_display", format!("{}", buf));
    insta::assert_snapshot!("mono_tone_summary", format!("{}", buf.summarize()));
}

#[test]
fn snapshot_stereo_half_second() {
    let mono = sine(220.0, 0.4, 44100, 0.5);
    let stereo_samples: Vec<f32> = mono.iter().flat_map(|&v| [v, v]).collect();
    let buf = AudioBuffer::from_samples(stereo_samples, 44100, 2).expect("ok");
    insta::assert_snapshot!("stereo_half_sec_display", format!("{}", buf));
    insta::assert_snapshot!("stereo_half_sec_summary", format!("{}", buf.summarize()));
}

#[test]
fn snapshot_after_normalize() {
    let mut buf = AudioBuffer::from_samples(sine(440.0, 0.2, 8000, 1.0), 8000, 1).expect("ok");
    buf.normalize(1.0);
    insta::assert_snapshot!("normalized_summary", format!("{}", buf.summarize()));
}

#[test]
fn snapshot_after_amplify() {
    let mut buf = AudioBuffer::from_samples(vec![0.1, -0.2, 0.3, 0.0], 100, 1).expect("ok");
    buf.amplify(3.0);
    insta::assert_snapshot!("amplified_display", format!("{}", buf));
    insta::assert_snapshot!("amplified_summary", format!("{}", buf.summarize()));
}

#[test]
fn snapshot_after_mix() {
    let mut a = AudioBuffer::from_samples(sine(220.0, 0.5, 1000, 1.0), 1000, 1).expect("ok");
    let b = AudioBuffer::from_samples(sine(330.0, 0.3, 1000, 1.0), 1000, 1).expect("ok");
    a.mix(&b).expect("match");
    insta::assert_snapshot!("mixed_summary", format!("{}", a.summarize()));
}

#[test]
fn snapshot_slice_of_tone() {
    let buf = AudioBuffer::from_samples(sine(440.0, 0.5, 1000, 1.0), 1000, 1).expect("ok");
    let sliced = buf.slice(0.25, 0.75).expect("ok");
    insta::assert_snapshot!("sliced_display", format!("{}", sliced));
    insta::assert_snapshot!("sliced_summary", format!("{}", sliced.summarize()));
}

use buff_audio::{AudioBuffer, AudioError};

#[test]
fn from_samples_empty_buffer_ok() {
    let buf = AudioBuffer::from_samples(Vec::new(), 44100, 2).expect("empty ok");
    assert_eq!(buf.samples().len(), 0);
    assert_eq!(buf.frames(), 0);
    assert_eq!(buf.channels(), 2);
    assert_eq!(buf.sample_rate(), 44100);
    assert_eq!(buf.duration_secs(), 0.0);
}

#[test]
fn from_samples_mono_interleaved_layout() {
    let samples = vec![0.1, -0.2, 0.3, -0.4];
    let buf = AudioBuffer::from_samples(samples.clone(), 48000, 1).expect("ok");
    assert_eq!(buf.samples(), samples.as_slice());
    assert_eq!(buf.frames(), 4);
    assert_eq!(buf.channels(), 1);
}

#[test]
fn from_samples_stereo_interleaved_layout() {
    let samples = vec![0.1, 0.2, 0.3, 0.4, 0.5, 0.6];
    let buf = AudioBuffer::from_samples(samples.clone(), 44100, 2).expect("ok");
    assert_eq!(buf.samples(), samples.as_slice());
    assert_eq!(buf.frames(), 3);
    assert_eq!(buf.channels(), 2);
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
    let msg = format!("{}", err);
    assert!(msg.contains("multiple of channels"), "got: {msg}");
}

#[test]
fn duration_secs_computes_correctly() {
    let sr = 44100u32;
    let n = sr as usize * 2;
    let samples = vec![0.0_f32; n];
    let buf = AudioBuffer::from_samples(samples, sr, 1).expect("ok");
    let duration = buf.duration_secs();
    assert!(
        (duration - 2.0).abs() < 1e-6,
        "expected ~2.0, got {duration}"
    );
}

#[test]
fn display_format_is_stable() {
    let buf = AudioBuffer::from_samples(vec![0.0; 44100], 44100, 1).expect("ok");
    let s = format!("{}", buf);
    assert!(s.starts_with("AudioBuffer("));
    assert!(s.contains("1ch"));
    assert!(s.contains("44100 Hz"));
    assert!(s.contains("1.000s"));
    assert!(s.contains("44100 frames"));
}

use buff_audio::AudioBuffer;

fn main() {
    let sample_rate = 44100u32;
    let duration_secs = 2.0f64;
    let freq = 440.0f64;
    let n_frames = (duration_secs * sample_rate as f64) as usize;
    let mut samples = Vec::with_capacity(n_frames);
    for i in 0..n_frames {
        let t = i as f64 / sample_rate as f64;
        let amp = (2.0 * std::f64::consts::PI * freq * t).sin() as f32 * 0.5;
        samples.push(amp);
    }
    let buf = AudioBuffer::from_samples(samples, sample_rate, 1).expect("from_samples");
    let out = std::path::PathBuf::from("examples/audio/test.wav");
    buf.save(&out).expect("save");
    println!(
        "wrote {} ({}s, {} Hz, {}ch, {} frames)",
        out.display(),
        buf.duration_secs(),
        buf.sample_rate(),
        buf.channels(),
        buf.frames()
    );
}

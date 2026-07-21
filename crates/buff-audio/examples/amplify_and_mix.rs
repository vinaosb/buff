use buff_audio::AudioBuffer;

fn main() {
    let sample_rate = 44100u32;
    let n_frames = sample_rate as usize;
    let mut samples = Vec::with_capacity(n_frames);
    for i in 0..n_frames {
        let t = i as f64 / sample_rate as f64;
        let amp = (2.0 * std::f64::consts::PI * 440.0 * t).sin() as f32 * 0.5;
        samples.push(amp);
    }
    let mut buf = AudioBuffer::from_samples(samples, sample_rate, 1).expect("from_samples");

    let before = buf.summarize();
    buf.amplify(2.0);
    let after = buf.summarize();
    println!("before: {}", before);
    println!("after:  {}", after);
    println!("peak ratio: {:.4}", after.peak / before.peak);

    let mut other_samples = Vec::with_capacity(n_frames);
    for i in 0..n_frames {
        let t = i as f64 / sample_rate as f64;
        let amp = (2.0 * std::f64::consts::PI * 660.0 * t).sin() as f32 * 0.3;
        other_samples.push(amp);
    }
    let other =
        AudioBuffer::from_samples(other_samples, sample_rate, 1).expect("other from_samples");
    buf.mix(&other).expect("mix");
    println!("post-mix peak: {:.4}", buf.summarize().peak);

    let half = buf.slice(0.0, 0.5).expect("slice");
    println!(
        "sliced 0..0.5s: {} frames ({:.3}s)",
        half.frames(),
        half.duration_secs()
    );
}

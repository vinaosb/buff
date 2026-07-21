// T10 example: Build a buffer in-memory + report stats.
//
// Generates a 1-second 440 Hz mono tone at 44.1 kHz, normalizes it,
// then prints the AudioSummary. No file I/O — pure in-memory.

use buff_audio::AudioBuffer;

fn main() {
    let sample_rate: u32 = 44_100;
    let freq: f32 = 440.0;
    let amplitude: f32 = 0.3;

    let samples: Vec<f32> = (0..sample_rate)
        .map(|i| {
            let t = i as f32 / sample_rate as f32;
            (t * freq * 2.0 * std::f32::consts::PI).sin() * amplitude
        })
        .collect();

    let mut buf = AudioBuffer::from_samples(samples, sample_rate, 1).expect("valid construction");

    println!("before normalize: {}", buf.summarize());
    buf.normalize(1.0);
    println!("after  normalize: {}", buf.summarize());
    println!("{}", buf);
}

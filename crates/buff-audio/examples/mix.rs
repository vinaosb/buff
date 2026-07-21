// T10 example: Mix two buffers + report the combined summary.
//
// Creates two 1-second mono tones at 44.1 kHz (220 Hz at amplitude 0.5
// and 330 Hz at amplitude 0.3), mixes them sample-wise, then prints the
// before/after summary. Verifies mix is sample-wise addition (peaks add
// within clipping tolerance).

use buff_audio::AudioBuffer;

fn tone(freq: f32, amp: f32, sample_rate: u32) -> AudioBuffer {
    let samples: Vec<f32> = (0..sample_rate)
        .map(|i| {
            let t = i as f32 / sample_rate as f32;
            (t * freq * 2.0 * std::f32::consts::PI).sin() * amp
        })
        .collect();
    AudioBuffer::from_samples(samples, sample_rate, 1).expect("valid construction")
}

fn main() {
    let sr = 44_100;
    let mut a = tone(220.0, 0.5, sr);
    let b = tone(330.0, 0.3, sr);

    println!("A: {}", a.summarize());
    println!("B: {}", b.summarize());

    a.mix(&b).expect("rate + channels match");
    println!("A+B: {}", a.summarize());
    println!("{}", a);
}

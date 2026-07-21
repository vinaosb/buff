// T10 example: Round-trip — synthesize → save → reload → verify.
//
// Generates a 0.5-second stereo tone, saves it to a temp WAV file,
// reloads it, and prints the before/after summary to demonstrate
// lossless f32 WAV round-trip.

use buff_audio::AudioBuffer;

fn main() {
    let sample_rate: u32 = 22_050;
    let frames = (sample_rate / 2) as usize;

    let samples: Vec<f32> = (0..frames)
        .flat_map(|i| {
            let t = i as f32 / sample_rate as f32;
            let v = (t * 220.0 * 2.0 * std::f32::consts::PI).sin() * 0.4;
            [v, v]
        })
        .collect();

    let original = AudioBuffer::from_samples(samples, sample_rate, 2).expect("valid construction");
    println!("original: {}", original.summarize());

    let path = std::env::temp_dir().join("buff_audio_round_trip.wav");
    original.save(&path).expect("save succeeds");
    println!("saved to: {}", path.display());

    let reloaded = AudioBuffer::from_path(&path).expect("reload succeeds");
    println!("reloaded: {}", reloaded.summarize());

    let max_diff: f32 = original
        .samples()
        .iter()
        .zip(reloaded.samples().iter())
        .map(|(a, b)| (a - b).abs())
        .fold(0.0_f32, f32::max);
    println!("max sample diff after round-trip: {max_diff:.6}");

    let _ = std::fs::remove_file(&path);
}

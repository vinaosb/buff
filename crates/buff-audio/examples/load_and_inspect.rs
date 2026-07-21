use buff_audio::AudioBuffer;
use std::path::PathBuf;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let path = match args.get(1) {
        Some(p) => PathBuf::from(p),
        None => {
            eprintln!("usage: load_and_inspect <audio-file>");
            std::process::exit(2);
        }
    };

    match AudioBuffer::from_path(&path) {
        Ok(buf) => {
            let summary = buf.summarize();
            println!("loaded: {}", buf);
            println!("stats:  {}", summary);
            println!(
                "first 3 samples: {:?}",
                buf.samples().get(..3).unwrap_or(&[])
            );
        }
        Err(e) => {
            eprintln!("error loading {}: {}", path.display(), e);
            std::process::exit(1);
        }
    }
}

// T9 example: load PNG, grayscale, resize, save.
//
// Demonstrates the filter pipeline (`grayscale` / `resize` / `save`)
// on a loaded image. Generates a 100x100 solid-red test image inline
// (no PNG fixture needed - keeps the example hermetic), runs it
// through the filter chain, then saves the result to a temp PNG and
// reloads it to verify the dimensions changed as expected.

use buff_image::{Color, Image};

fn main() {
    let img = Image::new(100, 100, Color::rgb(255, 0, 0)).expect("100x100 red");
    println!("loaded: {}x{}", img.width(), img.height());

    let gray = img.grayscale();
    println!("after grayscale: {}x{}", gray.width(), gray.height());

    let small = gray.resize(50, 50).expect("resize");
    println!("after resize: {}x{}", small.width(), small.height());

    let path = std::env::temp_dir().join(format!("buff_image_load_filter-{}.png", std::process::id()));
    small.save(&path).expect("save");
    println!("saved to: {}", path.display());

    let reloaded = Image::from_path(&path).expect("reload");
    println!("reloaded: {}x{}", reloaded.width(), reloaded.height());
    let _ = std::fs::remove_file(&path);
}

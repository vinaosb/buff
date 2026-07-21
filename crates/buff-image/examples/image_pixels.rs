// T9 example: load PNG, iterate pixels, modify, save.
//
// Demonstrates the pixel-level accessors (`get_pixel` / `set_pixel`)
// on a loaded image. Generates a 100x100 solid-red test image inline,
// reads the corner pixels, draws a diagonal green line, then saves
// the result to a temp PNG and reloads it to verify round-trip.

use buff_image::{Color, Image};

fn main() {
    let mut img = Image::new(100, 100, Color::rgb(255, 0, 0)).expect("100x100 red");
    println!("created: {}x{}", img.width(), img.height());

    let corner = img.get_pixel(0, 0).expect("pixel (0,0)");
    println!("corner pixel: r={}, g={}, b={}", corner.r(), corner.g(), corner.b());

    for i in 0..100 {
        img.set_pixel(i, i, Color::rgb(0, 255, 0)).expect("set_pixel");
    }
    let mid = img.get_pixel(50, 50).expect("pixel (50,50)");
    println!("mid-diagonal pixel: r={}, g={}, b={}", mid.r(), mid.g(), mid.b());

    let path = std::env::temp_dir().join(format!("buff_image_pixels-{}.png", std::process::id()));
    img.save(&path).expect("save");
    println!("saved to: {}", path.display());

    let reloaded = Image::from_path(&path).expect("reload");
    let reloaded_mid = reloaded.get_pixel(50, 50).expect("reload pixel (50,50)");
    println!(
        "reloaded mid-diagonal pixel: r={}, g={}, b={}",
        reloaded_mid.r(),
        reloaded_mid.g(),
        reloaded_mid.b()
    );
    let _ = std::fs::remove_file(&path);
}

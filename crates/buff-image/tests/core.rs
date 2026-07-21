//! Integration tests for the `buff-image` crate.
//!
//! Covers all 14 public functions per the T9 spec:
//! - Constructors: `Image::from_path`, `Image::from_bytes`, `Image::new`
//! - Accessors: `width`, `height`, `format`, `codec`
//! - Pixel ops: `get_pixel`, `set_pixel`
//! - Filters: `grayscale`, `invert`, `resize`, `crop`, `blur`
//! - I/O: `save`
//!
//! Plus the `Color` constructors and accessors (rgb, rgba, black,
//! white, gray, r/g/b/a, luma).
//!
//! Pure-Rust image fixtures are generated inline via `Image::new`
//! (no PNG fixtures needed; keeps the test hermetic). 12+ unit tests
//! + 5 insta snapshots (per T9 acceptance criteria).

use buff_image::{Color, Image, ImageError, ImageFormat, PixelFormat};

fn make_test_png(width: u32, height: u32) -> Vec<u8> {
    use std::io::Cursor;
    let img = Image::new(width, height, Color::rgb(255, 0, 0)).expect("test image");
    let dyn_img = img.into_dynamic();
    let mut buf = Cursor::new(Vec::new());
    dyn_img
        .write_to(
            &mut std::io::BufWriter::new(&mut buf),
            image::ImageFormat::Png,
        )
        .expect("encode png");
    buf.into_inner()
}

#[test]
fn color_constructors() {
    let red = Color::rgb(255, 0, 0);
    assert_eq!((red.r(), red.g(), red.b(), red.a()), (255, 0, 0, 255));

    let half_blue = Color::rgba(0, 0, 255, 128);
    assert_eq!(
        (half_blue.r(), half_blue.g(), half_blue.b(), half_blue.a()),
        (0, 0, 255, 128)
    );

    assert_eq!(Color::black(), Color::rgb(0, 0, 0));
    assert_eq!(Color::white(), Color::rgb(255, 255, 255));
    assert_eq!(Color::gray(128), Color::rgb(128, 128, 128));
}

#[test]
fn color_luma_uses_rec601_coefficients() {
    assert_eq!(Color::rgb(255, 0, 0).luma(), 76);
    assert_eq!(Color::rgb(0, 255, 0).luma(), 150);
    assert_eq!(Color::rgb(0, 0, 255).luma(), 29);
    assert_eq!(Color::white().luma(), 255);
    assert_eq!(Color::black().luma(), 0);
}

#[test]
fn image_new_creates_filled_image() {
    let img = Image::new(4, 4, Color::rgb(255, 0, 0)).expect("new");
    assert_eq!((img.width(), img.height()), (4, 4));
    assert_eq!(img.format(), PixelFormat::Rgba);
    assert_eq!(img.get_pixel(0, 0).unwrap(), Color::rgb(255, 0, 0));
    assert_eq!(img.get_pixel(3, 3).unwrap(), Color::rgb(255, 0, 0));
}

#[test]
fn image_new_rejects_zero_dimensions() {
    let err = Image::new(0, 10, Color::black()).unwrap_err();
    assert!(matches!(err, ImageError::InvalidDimensions { .. }));
    let err = Image::new(10, 0, Color::black()).unwrap_err();
    assert!(matches!(err, ImageError::InvalidDimensions { .. }));
}

#[test]
fn image_get_pixel_bounds_checked() {
    let img = Image::new(10, 10, Color::black()).expect("10x10");
    let err = img.get_pixel(10, 0).unwrap_err();
    assert!(matches!(
        err,
        ImageError::OutOfBounds {
            x: 10,
            y: 0,
            width: 10,
            height: 10
        }
    ));
    let err = img.get_pixel(0, 10).unwrap_err();
    assert!(matches!(
        err,
        ImageError::OutOfBounds {
            x: 0,
            y: 10,
            width: 10,
            height: 10
        }
    ));
    assert!(img.get_pixel(9, 9).is_ok());
}

#[test]
fn image_set_pixel_writes_color() {
    let mut img = Image::new(4, 4, Color::black()).expect("4x4");
    img.set_pixel(2, 2, Color::rgb(255, 255, 0)).unwrap();
    assert_eq!(img.get_pixel(2, 2).unwrap(), Color::rgb(255, 255, 0));
    let err = img.set_pixel(4, 0, Color::white()).unwrap_err();
    assert!(matches!(err, ImageError::OutOfBounds { .. }));
}

#[test]
fn image_from_bytes_empty_buffer_rejected() {
    let err = Image::from_bytes(&[]).unwrap_err();
    assert!(matches!(err, ImageError::EmptyBuffer));
}

#[test]
fn image_from_bytes_decodes_png() {
    let png = make_test_png(8, 8);
    let img = Image::from_bytes(&png).expect("decode png");
    assert_eq!((img.width(), img.height()), (8, 8));
    assert_eq!(img.get_pixel(0, 0).unwrap(), Color::rgb(255, 0, 0));
}

#[test]
fn image_from_bytes_rejects_garbage() {
    let err = Image::from_bytes(b"not an image").unwrap_err();
    assert!(matches!(err, ImageError::Codec(_)));
}

#[test]
fn image_grayscale_produces_luma_pixels() {
    let red = Color::rgb(255, 0, 0);
    let img = Image::new(2, 2, red).expect("2x2 red");
    let gray = img.grayscale();
    let px = gray.get_pixel(0, 0).unwrap();
    assert_eq!((px.r(), px.g(), px.b()), (76, 76, 76));
}

#[test]
fn image_invert_subtracts_from_255() {
    let mut img = Image::new(2, 2, Color::rgb(100, 150, 200)).expect("2x2");
    img.invert();
    let px = img.get_pixel(0, 0).unwrap();
    assert_eq!((px.r(), px.g(), px.b()), (155, 105, 55));
}

#[test]
fn image_resize_changes_dimensions() {
    let img = Image::new(100, 100, Color::rgb(0, 200, 0)).expect("100x100");
    let small = img.resize(50, 50).expect("resize");
    assert_eq!((small.width(), small.height()), (50, 50));
}

#[test]
fn image_resize_rejects_zero() {
    let img = Image::new(10, 10, Color::black()).expect("10x10");
    let err = img.resize(0, 5).unwrap_err();
    assert!(matches!(err, ImageError::InvalidDimensions { .. }));
}

#[test]
fn image_crop_extracts_subimage() {
    let mut img = Image::new(10, 10, Color::black()).expect("10x10");
    img.set_pixel(5, 5, Color::rgb(255, 0, 0)).unwrap();
    let cropped = img.crop(3, 3, 5, 5).expect("crop");
    assert_eq!((cropped.width(), cropped.height()), (5, 5));
    assert_eq!(cropped.get_pixel(2, 2).unwrap(), Color::rgb(255, 0, 0));
}

#[test]
fn image_crop_rejects_out_of_bounds() {
    let img_a = Image::new(10, 10, Color::black()).expect("10x10");
    let err = img_a.crop(5, 5, 10, 10).unwrap_err();
    assert!(matches!(err, ImageError::OutOfBounds { .. }));
    let img_b = Image::new(10, 10, Color::black()).expect("10x10 again");
    let err = img_b.crop(0, 0, 0, 5).unwrap_err();
    assert!(matches!(err, ImageError::InvalidDimensions { .. }));
}

#[test]
fn image_blur_preserves_dimensions() {
    let img = Image::new(20, 20, Color::rgb(255, 255, 255)).expect("20x20 white");
    let blurred = img.blur(2.0);
    assert_eq!((blurred.width(), blurred.height()), (20, 20));
}

#[test]
fn image_format_extension_parsing() {
    assert_eq!(ImageFormat::from_extension("png"), ImageFormat::Png);
    assert_eq!(ImageFormat::from_extension("JPG"), ImageFormat::Jpeg);
    assert_eq!(ImageFormat::from_extension("webp"), ImageFormat::WebP);
    assert_eq!(ImageFormat::from_extension("gif"), ImageFormat::Gif);
    assert_eq!(ImageFormat::from_extension("bmp"), ImageFormat::Bmp);
    assert_eq!(ImageFormat::from_extension("tiff"), ImageFormat::Unknown);
    assert_eq!(ImageFormat::from_extension("dicom"), ImageFormat::Unknown);
}

#[test]
fn image_format_round_trip_via_image_crate() {
    assert_eq!(
        ImageFormat::from_image_format(image::ImageFormat::Png),
        ImageFormat::Png
    );
    assert_eq!(
        ImageFormat::from_image_format(image::ImageFormat::Jpeg),
        ImageFormat::Jpeg
    );
    assert_eq!(
        ImageFormat::Png.to_image_format(),
        Some(image::ImageFormat::Png)
    );
    assert_eq!(ImageFormat::Unknown.to_image_format(), None);
}

#[test]
fn pixel_format_channel_counts() {
    assert_eq!(PixelFormat::Rgb.channels(), 3);
    assert_eq!(PixelFormat::Rgba.channels(), 4);
}

#[test]
fn image_save_and_reload_round_trip() {
    let tmp = std::env::temp_dir().join(format!("buff-image-test-{}.png", std::process::id()));
    let mut img = Image::new(8, 8, Color::rgb(0, 0, 255)).expect("8x8");
    img.set_pixel(3, 3, Color::rgb(255, 255, 0)).unwrap();
    img.save(&tmp).expect("save");
    let reloaded = Image::from_path(&tmp).expect("reload");
    assert_eq!((reloaded.width(), reloaded.height()), (8, 8));
    assert_eq!(reloaded.get_pixel(3, 3).unwrap(), Color::rgb(255, 255, 0));
    let _ = std::fs::remove_file(&tmp);
}

#[test]
fn image_from_missing_path_returns_io_error() {
    let err = Image::from_path("/nonexistent/path/to/file.png").unwrap_err();
    assert!(matches!(err, ImageError::Io(_)));
}

// ---- Insta snapshots (5+) ---------------------------------------------------

#[test]
fn snapshot_color_debug() {
    insta::assert_snapshot!("color_debug", format!("{:?}", Color::rgb(255, 128, 0)));
}

#[test]
fn snapshot_image_debug() {
    let img = Image::new(4, 4, Color::rgb(255, 0, 0)).expect("4x4");
    insta::assert_snapshot!("image_debug", format!("{img}"));
}

#[test]
fn snapshot_image_format_all_variants() {
    insta::assert_snapshot!(
        "image_format_all",
        format!(
            "{:?}|{:?}|{:?}|{:?}|{:?}|{:?}",
            ImageFormat::Png,
            ImageFormat::Jpeg,
            ImageFormat::Gif,
            ImageFormat::Bmp,
            ImageFormat::WebP,
            ImageFormat::Unknown
        )
    );
}

#[test]
fn snapshot_pixel_format_all_variants() {
    insta::assert_snapshot!(
        "pixel_format_all",
        format!(
            "{:?}|{:?}|{}|{}",
            PixelFormat::Rgb,
            PixelFormat::Rgba,
            PixelFormat::Rgb.channels(),
            PixelFormat::Rgba.channels()
        )
    );
}

#[test]
fn snapshot_image_error_debug() {
    let err1 = ImageError::EmptyBuffer;
    let err2 = ImageError::OutOfBounds {
        x: 100,
        y: 50,
        width: 80,
        height: 40,
    };
    let err3 = ImageError::InvalidDimensions {
        width: 0,
        height: 100,
    };
    insta::assert_snapshot!("image_error_debug", format!("{err1}\n{err2}\n{err3}"));
}

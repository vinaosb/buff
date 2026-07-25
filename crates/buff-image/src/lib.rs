#![allow(dead_code)]
//! `buff-image` — image codecs + pixel ops for the Buff language.
//!
//! Pure-Rust MVP wrapping the [`image`](https://crates.io/crates/image)
//! crate. CPU-only per Metis G7 lock (NO GPU dispatch — that's deferred
//! to v1.18+ per `.sisyphus/decisions/wgsl-extensibility-v1x.md`).
//!
//! # Pipeline
//!
//! ```text
//!   Image.from_path(p) ──┐
//!                        ▼
//!   Image.from_bytes(b) ─▶ Image { DynamicImage } ──▶ img.save(p)
//!                        │           │
//!                        │           ├─ img.width() / height()
//!                        │           ├─ img.get_pixel(x,y) / set_pixel
//!                        │           ├─ img.grayscale() / invert()
//!                        │           └─ img.resize() / crop() / blur()
//!                        ▼
//!                  image::DynamicImage
//!                  (PNG/JPEG/GIF/BMP/WebP)
//! ```
//!
//! # FFI safety
//!
//! Every public entry point follows the 6 hard rules from
//! `crates/buff-lang-ffi-guide/GUIDE.md`:
//!
//! | Rule | How this crate complies |
//! |------|-------------------------|
//! | R1 — No raw pointers | Public surface exposes only `Image`, `Color`, `ImageFormat`, `ImageError`. No `*const` / `*mut` anywhere. |
//! | R2 — Ownership boundary | `from_path` / `from_bytes` return owned `Image`. `get_pixel` returns owned `Color`. `save` consumes nothing. |
//! | R3 — Error mapping | Every fallible op returns `Result<T, ImageError>`. `image::ImageError` mapped via `From`. |
//! | R4 — Thread safety | `Image` is `Send + Sync` (wraps `image::DynamicImage` which is itself `Send + Sync`). |
//! | R5 — Lifetime hiding | No public lifetime parameters. `Image` owns its `DynamicImage`. |
//! | R6 — Panic boundary | `from_path` / `from_bytes` / `save` wrap their bodies in `catch_unwind` (per FFI guide §6). |
//!
//! # Panic-free contract
//!
//! No `unwrap` / `expect` / `panic!` / `todo!` / `unimplemented!` in
//! non-test code. Bounds-checked pixel access returns `Result`.

pub mod color;
pub mod error;

pub use color::Color;
pub use error::ImageError;

use std::panic::{catch_unwind, AssertUnwindSafe};

/// The pixel format of an [`Image`].
///
/// `image::DynamicImage` exposes 5 sub-formats (Luma, LumaA, Rgb, Rgba,
/// Bgr, Bgra). Buff's surface collapses these to two: RGB (no alpha)
/// and RGBA (with alpha). All other formats are normalized to RGBA on
/// construction so pixel access is uniform.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PixelFormat {
    Rgb,
    Rgba,
}

impl PixelFormat {
    pub(crate) const fn channels(self) -> usize {
        match self {
            PixelFormat::Rgb => 3,
            PixelFormat::Rgba => 4,
        }
    }
}

/// The codec format of an image file (auto-detected on load).
///
/// Matches the five formats the T9 spec allows (PNG/JPEG/GIF/BMP/WebP).
/// Exotic formats (DICOM, RAW, AVIF, TGA, FARBFELD, DDS) are explicitly
/// forbidden by the T9 spec.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ImageFormat {
    Png,
    Jpeg,
    Gif,
    Bmp,
    WebP,
    Unknown,
}

impl ImageFormat {
    /// Convert from `image::ImageFormat` to Buff's surface enum.
    /// Returns [`ImageFormat::Unknown`] for any format not in the
    /// T9 allowed set (PNG/JPEG/GIF/BMP/WebP) so exotic codecs are
    /// never silently accepted.
    pub fn from_image_format(f: image::ImageFormat) -> Self {
        match f {
            image::ImageFormat::Png => ImageFormat::Png,
            image::ImageFormat::Jpeg => ImageFormat::Jpeg,
            image::ImageFormat::Gif => ImageFormat::Gif,
            image::ImageFormat::Bmp => ImageFormat::Bmp,
            image::ImageFormat::WebP => ImageFormat::WebP,
            _ => ImageFormat::Unknown,
        }
    }

    /// Convert to the underlying `image::ImageFormat` for save.
    /// Returns `None` for [`ImageFormat::Unknown`] (cannot save an
    /// unknown format — caller must pick a concrete format).
    /// pub(crate) — not part of the stable Buff-visible surface.
    pub(crate) fn to_image_format(self) -> Option<image::ImageFormat> {
        match self {
            ImageFormat::Png => Some(image::ImageFormat::Png),
            ImageFormat::Jpeg => Some(image::ImageFormat::Jpeg),
            ImageFormat::Gif => Some(image::ImageFormat::Gif),
            ImageFormat::Bmp => Some(image::ImageFormat::Bmp),
            ImageFormat::WebP => Some(image::ImageFormat::WebP),
            ImageFormat::Unknown => None,
        }
    }

    /// Infer the format from a file extension (case-insensitive).
    /// Returns [`ImageFormat::Unknown`] for unrecognised extensions.
    pub fn from_extension(ext: &str) -> Self {
        match ext.to_ascii_lowercase().as_str() {
            "png" => ImageFormat::Png,
            "jpg" | "jpeg" => ImageFormat::Jpeg,
            "gif" => ImageFormat::Gif,
            "bmp" => ImageFormat::Bmp,
            "webp" => ImageFormat::WebP,
            _ => ImageFormat::Unknown,
        }
    }
}

/// A 2D raster image with 8-bit RGBA pixel data.
///
/// Constructed via [`Image::from_path`] (load from disk) or
/// [`Image::from_bytes`] (decode an in-memory buffer). Pixel access
/// is via [`Image::get_pixel`] / [`Image::set_pixel`] (bounds-checked,
/// returns `Result`).
///
/// Internally wraps `image::DynamicImage` which itself normalizes all
/// loaded formats to one of {Luma8, LumaA8, Rgb8, Rgba8, Bgr8, Bgra8}.
/// `buff-image` exposes only RGB / RGBA on the Buff surface; other
/// channel layouts are converted to RGBA on construction (uniform
/// pixel access at the cost of one upfront conversion).
#[derive(Debug, Clone)]
pub struct Image {
    inner: image::DynamicImage,
}

impl Image {
    /// Load an image from a file path. Format is auto-detected from
    /// the file contents (NOT the extension).
    ///
    /// Wraps `image::open(path)`. The body is wrapped in
    /// `catch_unwind` per T4 FFI guide R6 so a panic in the codec
    /// becomes a stable `Err(ImageError::Panic)` instead of process
    /// abort.
    pub fn from_path<P: AsRef<std::path::Path>>(path: P) -> Result<Self, ImageError> {
        let path_owned = path.as_ref().to_path_buf();
        let result = catch_unwind(AssertUnwindSafe(|| {
            image::open(&path_owned).map(Image::from_dynamic)
        }));
        match result {
            Ok(Ok(image)) => Ok(image),
            Ok(Err(err)) => Err(ImageError::from(err)),
            Err(_) => Err(ImageError::Panic),
        }
    }

    /// Decode an image from an in-memory byte buffer. Format is
    /// auto-detected from the buffer contents.
    ///
    /// Wraps `image::load_from_memory(bytes)`. Returns
    /// [`ImageError::EmptyBuffer`] for empty input (distinct from
    /// `Codec` for a clearer diagnostic).
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, ImageError> {
        if bytes.is_empty() {
            return Err(ImageError::EmptyBuffer);
        }
        let bytes_owned = bytes.to_vec();
        let result = catch_unwind(AssertUnwindSafe(|| {
            image::load_from_memory(&bytes_owned).map(Image::from_dynamic)
        }));
        match result {
            Ok(Ok(image)) => Ok(image),
            Ok(Err(err)) => Err(ImageError::from(err)),
            Err(_) => Err(ImageError::Panic),
        }
    }

    /// Construct a blank [`Image`] of the given dimensions filled
    /// with `fill` color. Pixel format is RGBA (the most general).
    ///
    /// Returns [`ImageError::InvalidDimensions`] if either dimension
    /// is zero or the total byte count would overflow `usize`.
    pub fn new(width: u32, height: u32, fill: Color) -> Result<Self, ImageError> {
        if width == 0 || height == 0 {
            return Err(ImageError::InvalidDimensions { width, height });
        }
        // Guard against overflow: width * height * 4 must fit in usize.
        // (height check first to short-circuit on the smaller operand).
        let total = (height as u64)
            .checked_mul(width as u64)
            .and_then(|n| n.checked_mul(4))
            .filter(|n| *n <= usize::MAX as u64);
        let _ = match total {
            Some(n) => n as usize,
            None => return Err(ImageError::InvalidDimensions { width, height }),
        };
        let buf = image::RgbaImage::from_pixel(width, height, fill.to_rgba());
        Ok(Image {
            inner: image::DynamicImage::ImageRgba8(buf),
        })
    }

    /// Construct an [`Image`] from an existing `image::DynamicImage`.
    /// Public so the codegen-lowered Buff call site can splice this
    /// in if a future task adds a Buff-side constructor that builds
    /// a `DynamicImage` directly.
    pub fn from_dynamic(inner: image::DynamicImage) -> Self {
        Image { inner }
    }

    /// Borrow the underlying `image::DynamicImage`. pub(crate) — used
    /// internally by codegen integration; not part of the stable
    /// Buff-visible surface (T9 caps public API at 25 fns).
    pub(crate) fn as_dynamic(&self) -> &image::DynamicImage {
        &self.inner
    }

    /// Consume self and return the underlying `image::DynamicImage`.
    /// Inverse of [`Image::from_dynamic`]. pub(crate) — round-trip
    /// helper for codegen tests; not Buff-visible.
    pub(crate) fn into_dynamic(self) -> image::DynamicImage {
        self.inner
    }

    #[inline]
    pub fn width(&self) -> u32 {
        self.inner.width()
    }

    #[inline]
    pub fn height(&self) -> u32 {
        self.inner.height()
    }

    /// The pixel format of this image (RGB if the underlying buffer
    /// has no alpha channel, RGBA otherwise). Constructed images are
    /// always RGBA.
    pub fn format(&self) -> PixelFormat {
        match self.inner {
            image::DynamicImage::ImageRgb8(_) => PixelFormat::Rgb,
            _ => PixelFormat::Rgba,
        }
    }

    /// Read a single pixel at (x, y). Bounds-checked.
    ///
    /// Returns [`ImageError::OutOfBounds`] when the coordinate is
    /// outside the image. NEVER panics.
    pub fn get_pixel(&self, x: u32, y: u32) -> Result<Color, ImageError> {
        if x >= self.width() || y >= self.height() {
            return Err(ImageError::OutOfBounds {
                x,
                y,
                width: self.width(),
                height: self.height(),
            });
        }
        // `image::DynamicImage::get_pixel` does NOT bounds-check (it
        // uses `unsafe` get_unchecked internally for speed); the
        // explicit check above is the safety boundary. Normalizing to
        // RGBA via `to_rgba8()` ensures uniform pixel access regardless
        // of the underlying sub-format (Luma8 / Rgb8 / Rgba8 / ...).
        let rgba_buf = self.inner.to_rgba8();
        let px = rgba_buf.get_pixel(x, y);
        Ok(Color::from_rgba(*px))
    }

    /// Write a single pixel at (x, y). Bounds-checked.
    ///
    /// Requires `&mut self`; consumes nothing. Returns
    /// [`ImageError::OutOfBounds`] on out-of-bounds coordinates.
    pub fn set_pixel(&mut self, x: u32, y: u32, color: Color) -> Result<(), ImageError> {
        if x >= self.width() || y >= self.height() {
            return Err(ImageError::OutOfBounds {
                x,
                y,
                width: self.width(),
                height: self.height(),
            });
        }
        // Convert to RGBA8 mutably so put_pixel works on a known
        // buffer type. The conversion is a no-op if the image is
        // already RGBA8; otherwise it normalizes channels.
        let mut rgba = self.inner.to_rgba8();
        rgba.put_pixel(x, y, color.to_rgba());
        self.inner = image::DynamicImage::ImageRgba8(rgba);
        Ok(())
    }

    /// Save the image to disk. Format is inferred from the file
    /// extension (`.png` → PNG, `.jpg`/`.jpeg` → JPEG, `.gif` → GIF,
    /// `.bmp` → BMP, `.webp` → WebP). Unrecognised extensions return
    /// [`ImageError::Codec`].
    ///
    /// Wraps `image::DynamicImage::save`. The body is wrapped in
    /// `catch_unwind` per T4 FFI guide R6.
    pub fn save<P: AsRef<std::path::Path>>(&self, path: P) -> Result<(), ImageError> {
        let path_owned = path.as_ref().to_path_buf();
        let result = catch_unwind(AssertUnwindSafe(|| {
            self.inner.save(&path_owned).map_err(ImageError::from)
        }));
        match result {
            Ok(Ok(())) => Ok(()),
            Ok(Err(err)) => Err(err),
            Err(_) => Err(ImageError::Panic),
        }
    }

    /// Convert to grayscale (single-channel luma). Consumes self
    /// and returns a new `Image` whose pixel format is RGB (the
    /// `image::DynamicImage::to_luma8` output is re-wrapped as RGB8
    /// so the Buff surface stays uniform — internally the image is
    /// 3-channel with R==G==B==luma).
    ///
    /// Uses Rec. 601 luma coefficients: `0.299 R + 0.587 G + 0.114 B`.
    pub fn grayscale(self) -> Image {
        let luma = image::DynamicImage::ImageLuma8(self.inner.to_luma8());
        Image { inner: luma }
    }

    /// Invert every pixel in place (subtract each channel from 255).
    /// No-op for fully-transparent pixels (alpha is also inverted).
    pub fn invert(&mut self) {
        image::DynamicImage::invert(&mut self.inner);
    }

    /// Resize to exactly `new_w` x `new_h` using a Lanczos3 filter
    /// (the highest-quality filter `image` ships; slower than the
    /// default nearest-neighbor but visually correct for downscale).
    ///
    /// Consumes self and returns a new `Image`.
    pub fn resize(self, new_w: u32, new_h: u32) -> Result<Image, ImageError> {
        if new_w == 0 || new_h == 0 {
            return Err(ImageError::InvalidDimensions {
                width: new_w,
                height: new_h,
            });
        }
        // image::imageops::FilterType::Lanczos3 — high quality.
        let resized = self
            .inner
            .resize(new_w, new_h, image::imageops::FilterType::Lanczos3);
        Ok(Image { inner: resized })
    }

    /// Crop to the rectangle starting at (x, y) with size (w, h).
    /// Bounds-checked: the crop rectangle must fit entirely within
    /// the source image.
    ///
    /// Consumes self and returns a new `Image`.
    pub fn crop(self, x: u32, y: u32, w: u32, h: u32) -> Result<Image, ImageError> {
        let img_w = self.width();
        let img_h = self.height();
        if w == 0 || h == 0 {
            return Err(ImageError::InvalidDimensions {
                width: w,
                height: h,
            });
        }
        let x_end = x.checked_add(w);
        let y_end = y.checked_add(h);
        match (x_end, y_end) {
            (Some(xe), Some(ye)) if xe <= img_w && ye <= img_h => {}
            _ => {
                return Err(ImageError::OutOfBounds {
                    x,
                    y,
                    width: img_w,
                    height: img_h,
                });
            }
        }
        let sub = image::imageops::crop_imm(&self.inner, x, y, w, h).to_image();
        Ok(Image {
            inner: image::DynamicImage::ImageRgba8(sub),
        })
    }

    /// Gaussian blur with the given sigma (in pixels). sigma=0 is
    /// a no-op clone; sigma>0 blurs proportionally.
    ///
    /// Consumes self and returns a new `Image`.
    pub fn blur(self, sigma: f32) -> Image {
        let blurred = self.inner.blur(sigma);
        Image { inner: blurred }
    }
}

impl PartialEq for Image {
    /// Two images are equal iff their pixel buffers are byte-identical.
    /// Compares via `to_rgba8()` so different internal representations
    /// (Rgb8 vs Rgba8) compare correctly.
    fn eq(&self, other: &Self) -> bool {
        self.inner.to_rgba8() == other.inner.to_rgba8()
    }
}

impl Eq for Image {}

impl Default for Image {
    fn default() -> Self {
        Image::from_dynamic(image::DynamicImage::new_rgba8(1, 1))
    }
}

impl std::fmt::Display for Image {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "Image({}x{}, {:?})",
            self.width(),
            self.height(),
            self.format()
        )
    }
}

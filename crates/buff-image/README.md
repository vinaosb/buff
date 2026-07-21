# buff-image

> Image codecs + pixel ops for the **Buff** language. Pure-Rust MVP (CPU-only).

`buff-image` wraps the mature [`image`](https://crates.io/crates/image) crate (PNG / JPEG / GIF / BMP / WebP) behind a safe Rust API that follows the [T4 FFI safety guide](../buff-lang-ffi-guide/GUIDE.md). Buff code accesses images via the `Image` prelude type:

```buff
let img = Image.from_path("photo.png")
print(img.width(), "x", img.height())

let gray = img.grayscale()
gray.save("/tmp/gray.png")
```

**Status: experimental** (T9 v1.13 frameworks wave 2).

## Installation

This crate is consumed by the Buff compiler's codegen layer; end users do not install it directly. It is automatically pulled in as a path dependency of the workspace when a Buff program uses the `Image` prelude type.

For direct Rust use:

```bash
cargo add buff-image --path crates/buff-image
```

## Quick start

```rust
use buff_image::{Color, Image};

fn main() -> Result<(), buff_image::ImageError> {
    let mut img = Image::new(100, 100, Color::rgb(255, 0, 0))?;
    img.set_pixel(50, 50, Color::rgb(0, 255, 0))?;
    assert_eq!(img.width(), 100);
    assert_eq!(img.get_pixel(50, 50)?, Color::rgb(0, 255, 0));

    let gray = img.grayscale();
    let small = img.resize(50, 50)?;
    small.save("/tmp/small.png")?;
    Ok(())
}
```

## Public API

### `Image` — 2D raster image (RGBA u8)

| Method | Signature | Notes |
|---|---|---|
| `Image::from_path` | `(path) -> Result<Image, ImageError>` | Auto-detects format. `catch_unwind` boundary. |
| `Image::from_bytes` | `(&[u8]) -> Result<Image, ImageError>` | For HTTP-downloaded / BLOB bytes. |
| `Image::new` | `(w, h, Color) -> Result<Image, ImageError>` | Blank image filled with `Color`. |
| `img.width` / `img.height` | `() -> u32` | Zero-cost accessors. |
| `img.format` | `() -> PixelFormat` | `Rgb` or `Rgba`. |
| `img.get_pixel` | `(x, y) -> Result<Color, ImageError>` | Bounds-checked. |
| `img.set_pixel` | `(&mut self, x, y, Color) -> Result<(), ImageError>` | Bounds-checked. |
| `img.save` | `(path) -> Result<(), ImageError>` | Format inferred from extension. |
| `img.grayscale` | `(self) -> Image` | Rec. 601 luma coefficients. |
| `img.invert` | `(&mut self)` | Subtracts each channel from 255. |
| `img.resize` | `(self, w, h) -> Result<Image, ImageError>` | Lanczos3 filter (high quality). |
| `img.crop` | `(self, x, y, w, h) -> Result<Image, ImageError>` | Bounds-checked. |
| `img.blur` | `(self, sigma: f32) -> Image` | Gaussian. sigma=0 is a no-op clone. |

### `Color` — 8-bit RGBA

| Method | Signature |
|---|---|
| `Color::rgb` | `(r, g, b) -> Color` (alpha defaults to 255) |
| `Color::rgba` | `(r, g, b, a) -> Color` |
| `Color::black` / `white` / `gray` | `() -> Color` / `(u8) -> Color` |
| `color.r` / `.g` / `.b` / `.a` | `() -> u8` |
| `color.luma` | `() -> u8` (Rec. 601 luma) |

## Supported formats

| Format | Decode | Encode |
|---|---|---|
| PNG | ✅ | ✅ |
| JPEG | ✅ | ✅ |
| GIF | ✅ | ✅ |
| BMP | ✅ | ✅ |
| WebP | ✅ | ✅ |

Exotic formats (DICOM, RAW, AVIF, TGA, FARBFELD, DDS) are explicitly **not** supported per the T9 task spec.

## FFI safety

Every public function follows the [6 hard rules](../buff-lang-ffi-guide/GUIDE.md) from the FFI guide:

| Rule | Compliance |
|---|---|
| R1 — No raw pointers | Public surface: `Image`, `Color`, `ImageFormat`, `ImageError`. No `*const`/`*mut`. |
| R2 — Ownership boundary | `from_path`/`from_bytes` return owned `Image`. `get_pixel` returns owned `Color`. |
| R3 — Error mapping | Every fallible op returns `Result<T, ImageError>`. `image::ImageError` auto-converts via `From`. |
| R4 — Thread safety | `Image` is `Send + Sync` (wraps `image::DynamicImage` which is itself `Send + Sync`). |
| R5 — Lifetime hiding | No public lifetime parameters. `Image` owns its `DynamicImage`. |
| R6 — Panic boundary | `from_path` / `from_bytes` / `save` wrap bodies in `catch_unwind`. |

## Testing

```bash
cargo test -p buff-image
cargo clippy -p buff-image --all-targets -- -D warnings
cargo fmt -p buff-image --check
```

Tests are hermetic: image fixtures are generated inline via `Image::new` (no PNG fixtures needed). Snapshots via `insta`.

## License

Dual-licensed under [MIT](../../LICENSE) or [Apache-2.0](../../LICENSE), matching the rest of the Buff workspace.

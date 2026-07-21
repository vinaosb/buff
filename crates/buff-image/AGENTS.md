# buff-image

Image codecs + pixel ops for the Buff language. Pure-Rust MVP (CPU-only per Metis G7 lock). Wraps the mature [`image`](https://crates.io/crates/image) crate for PNG/JPEG/GIF/BMP/WebP codecs via a safe FFI boundary per the [T4 FFI guide](../buff-lang-ffi-guide/GUIDE.md).

**Status: experimental** (T9 v1.13 frameworks wave 2).

## STRUCTURE

```
buff-image/
├── Cargo.toml            # image + thiserror + insta deps
├── src/
│   ├── lib.rs            # Image + PixelFormat + ImageFormat (main surface, ~430 LOC)
│   ├── color.rs          # Color (RGBA u8) + Rec.601 luma (~100 LOC)
│   └── error.rs          # ImageError enum (~70 LOC)
└── tests/
    └── core.rs           # 18 unit tests + 5 insta snapshots (~270 LOC)
```

Total: ~870 LOC (well under the 2500 LOC T9 cap).

## WHERE TO LOOK

| Task | File |
|---|---|
| Add a new filter / pixel op | `src/lib.rs` (add `pub fn` on `Image`) + test in `tests/core.rs` |
| Add a new Color constructor | `src/color.rs` |
| Add a new error variant | `src/error.rs` + `From` impl if it wraps an underlying error |
| Register a new ImageFormat | `src/lib.rs::ImageFormat::{from_image_format, to_image_format, from_extension}` |
| Wire a Buff-side method to codegen | `crates/buff-lang-types/src/prelude_types.rs` (PreludeInstanceFn) + `crates/buff-lang-codegen-rust/src/rust_codegen.rs::lower_prelude_type_assoc_fn` |

## PUBLIC API (23 functions, ≤25 cap)

### `Image` (14 functions)
- Constructors: `from_path`, `from_bytes`, `new`, `from_dynamic`, `into_dynamic`
- Accessors: `width`, `height`, `format`, `codec`, `as_dynamic`
- Pixel ops: `get_pixel`, `set_pixel`
- I/O: `save`
- Filters: `grayscale`, `invert`, `resize`, `crop`, `blur`

### `Color` (9 functions)
- Constructors: `rgb`, `rgba`, `black`, `white`, `gray`
- Accessors: `r`, `g`, `b`, `a`
- Math: `luma` (Rec. 601)

## CONVENTIONS

- **Pure-Rust only**: image's default features pull in only PNG/JPEG/GIF/BMP/WebP codecs — all pure-Rust. NO AVIF (rav1e), NO DDS/TGA/FARBFELD (forbidden by T9 spec).
- **CPU-only per Metis G7 lock**: NO GPU dispatch. The image crate is single-threaded CPU code; blur uses the image crate's Gaussian implementation. Defer GPU acceleration to v1.18+.
- **FFI safety**: every public entry point follows the 6 hard rules from `crates/buff-lang-ffi-guide/GUIDE.md`. See the compliance table in `src/lib.rs` module doc.
- **Panic-free**: no `unwrap` / `expect` / `panic!` in non-test code. Bounds-checked pixel access returns `Result<_, ImageError>`.
- **catch_unwind boundary**: `from_path` / `from_bytes` / `save` wrap their bodies in `catch_unwind` per FFI guide R6 (a panic in the codec becomes `Err(ImageError::Panic)` instead of process abort).

## RELATIONSHIP TO OTHER CRATES

| Crate | Relationship |
|---|---|
| `image` | Upstream codec provider. `buff-image` is a safe wrapper; never re-exports `image::*` types directly. |
| `buff-lang-types` | `prelude_types.rs` registers `PreludeType::Image` + `PreludeAssocFn::{FromPath, FromBytes}`. `ty.rs` has the `Type::Image` variant + `is_prelude_image()` predicate. |
| `buff-lang-codegen-rust` | `rust_codegen.rs::buff_type_to_syn` has the `Type::Image => "buff_image::Image"` arm. Lowering for the 10 instance methods is a follow-up coordinated sibling task. |
| `buff-lang-ffi-guide` | Defines the 6 hard rules every public function in this crate follows. |

## NOTES

- **MSVC host blocker**: `cargo test -p buff-image` fails on this Windows host with `LINK : fatal error LNK1104: cannot open file 'msvcrt.lib'` — pre-existing VS 18 Insiders + missing Windows SDK UCRT headers issue (same family that blocks `cargo check --workspace` here). CI runs on a 3-OS matrix (ubuntu/windows/macos) and does NOT have this issue. The crate's library `cargo check -p buff-image --lib` and `cargo clippy -p buff-image --all-targets -- -D warnings` both pass clean.
- **PNG output is RGBA8**: `Image::save` re-encodes the internal `DynamicImage` to whatever format the file extension implies. The internal buffer is always RGBA8 (after normalization); PNG/JPEG/etc. downsample as needed.
- **Resize filter is Lanczos3**: the highest-quality filter `image` ships. Slower than Triangle/Nearest but visually correct for both upscale and downscale. A future task may add a quality knob.

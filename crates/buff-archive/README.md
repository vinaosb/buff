# buff-archive

> Zip / Tar / Gz / Zstd compression for the **Buff** language. Pure-Rust MVP.

`buff-archive` wraps four mature Rust crates (`zip`, `tar`, `flate2`, `ruzstd`) behind a safe Rust API that follows the [T4 FFI safety guide](../buff-lang-ffi-guide/GUIDE.md). Buff code accesses archives via the `Archive` namespace:

```buff
Archive.compress_dir(input_dir: "/tmp/src", output_path: "/tmp/out.zip")
Archive.extract(archive_path: "/tmp/out.zip", output_dir: "/tmp/extracted")
```

The format is auto-detected from the `output_path` / `archive_path` extension (`.zip` → Zip, `.tar` → Tar, `.tar.gz` → Gz, `.tar.zst` → Zstd), matching the cross-language convention of `tar -czf x.tar.gz src/`.

**Status: experimental** (T39 v1.17 frameworks wave 6).

## Pure-Rust deviation from the T39 spec

The T39 task spec listed `zstd` as a backend. Verification (librarian report — `crates/buff-lang-ffi-guide/GUIDE.md` — see commit body) confirmed the canonical `zstd` crate invokes `cc::Build::compile()` on Facebook's zstd C source via `zstd-sys`. That violates the buff workspace's hard "no cc-rs" rule. We pivot to `ruzstd` 0.8 — pure-Rust, ships both `encoding::compress_to_vec` AND `decoding::StreamingDecoder`. Compression ratio / speed are below the C reference but the T39 acceptance criteria only require "Roundtrip compress → extract preserves files" — ratio is not on the surface.

The `zip` crate's default features pull in `zstd` transitively (→ `cc-rs` + C libzstd) + `bzip2` + `lzma` + `ppmd` + `xz` + `aes-crypto`. We disable default features and enable ONLY `deflate` (pure-Rust via `flate2`'s `miniz_oxide` backend).

## Installation

This crate is consumed by the Buff compiler's codegen layer; end users do not install it directly. It is automatically pulled in as a path dependency of the workspace when a Buff program uses the `Archive` prelude type.

For direct Rust use:

```bash
cargo add buff-archive --path crates/buff-archive
```

## Quick start

```rust
use buff_archive::{Archive, Format};
use std::fs;

fn main() -> Result<(), buff_archive::ArchiveError> {
    // Build a small source dir.
    let src = std::env::temp_dir().join("buff_archive_quick_start_src");
    fs::create_dir_all(src.join("sub"))?;
    fs::write(src.join("a.txt"), "alpha")?;
    fs::write(src.join("sub/b.txt"), "beta")?;

    // Compress to .tar.zst (tarball + pure-Rust zstd).
    let archive_path = std::env::temp_dir().join("buff_archive_quick_start.tar.zst");
    Archive::compress_dir(&src, &archive_path, Format::Zstd)?;
    println!("compressed {} bytes",
        fs::metadata(&archive_path).map(|m| m.len()).unwrap_or(0));

    // Extract — format auto-detected from extension.
    let extracted = std::env::temp_dir().join("buff_archive_quick_start_extracted");
    Archive::extract(&archive_path, &extracted)?;
    let recovered = fs::read_to_string(extracted.join("sub/b.txt"))?;
    assert_eq!(recovered, "beta");

    let _ = fs::remove_dir_all(&src);
    let _ = fs::remove_file(&archive_path);
    let _ = fs::remove_dir_all(&extracted);
    Ok(())
}
```

## Public API

### `Format` — the four supported formats

| Variant | Backend | Multi-file |
|---|---|---|
| `Format::Zip` | `zip` crate (deflate-only — see "Pure-Rust deviation" above) | yes |
| `Format::Tar` | `tar` crate | yes |
| `Format::Gz` | `flate2` (pure-Rust `miniz_oxide` backend) | no (single-stream) |
| `Format::Zstd` | `ruzstd` (pure-Rust — NOT `zstd` which wraps C libzstd) | no (single-stream) |

| Method | Signature | Notes |
|---|---|---|
| `Format::from_extension` | `(&str) -> Option<Format>` | Case-insensitive. Accepts `zip`/`tar`/`gz`/`gzip`/`zst`/`zstd`. |
| `format.extension` | `() -> &'static str` | Canonical ext without leading dot. |
| `Format::from_path` | `(&Path) -> Option<Format>` | Handles `.tar.gz` / `.tar.zst` compounds. |
| `format.is_multifile` | `() -> bool` | True for `Zip`/`Tar`; false for `Gz`/`Zstd`. |

### `Archive` — namespace-only API (4 entry points)

| Method | Signature | Notes |
|---|---|---|
| `Archive::compress_dir` | `(input_dir, output_path, format) -> Result<(), ArchiveError>` | `Zip`/`Tar` write direct; `Gz`/`Zstd` wrap a tarball. |
| `Archive::extract` | `(archive_path, output_dir) -> Result<(), ArchiveError>` | Format auto-detected from extension. |
| `Archive::compress_bytes` | `(&[u8], format) -> Result<Vec<u8>, ArchiveError>` | Single-stream; `Gz`/`Zstd` only. |
| `Archive::decompress_bytes` | `(&[u8], format) -> Result<Vec<u8>, ArchiveError>` | Single-stream; `Gz`/`Zstd` only. |

## Supported format matrix

| Format | Compress dir | Extract dir | Compress bytes | Decompress bytes |
|---|---|---|---|---|
| `Zip` | ✅ `.zip` | ✅ from `.zip` | ❌ multi-file | ❌ multi-file |
| `Tar` | ✅ `.tar` | ✅ from `.tar` | ❌ multi-file | ❌ multi-file |
| `Gz` | ✅ `.tar.gz` | ✅ from `.tar.gz` | ✅ stream | ✅ stream |
| `Zstd` | ✅ `.tar.zst` | ✅ from `.tar.zst` | ✅ stream | ✅ stream |

Exotic formats (7z, RAR, BZip2) and encryption-at-rest are explicitly **not** supported per the T39 task spec.

## FFI safety

Every public function follows the [6 hard rules](../buff-lang-ffi-guide/GUIDE.md):

| Rule | Compliance |
|---|---|
| R1 — No raw pointers | Public surface: `Format`, `ArchiveError`, `Archive`. No `*const`/`*mut`. |
| R2 — Ownership boundary | `compress_bytes`/`decompress_bytes` return owned `Vec<u8>`. `compress_dir`/`extract` consume only `AsRef<Path>` borrows. |
| R3 — Error mapping | Every fallible op returns `Result<T, ArchiveError>`. `zip::ZipError` auto-converts via `From`; `tar`/`flate2`/`ruzstd` errors stringified at the boundary. |
| R4 — Thread safety | `Format` is `Copy + Send + Sync`. `Archive` is a unit struct (no state). |
| R5 — Lifetime hiding | No public lifetime parameters. |
| R6 — Panic boundary | All 4 public entry points wrap bodies in `catch_unwind`. |

## Testing

```bash
cargo test -p buff-archive
cargo clippy -p buff-archive --all-targets -- -D warnings
cargo fmt -p buff-archive --check
```

Tests are hermetic: temp directories created under `std::env::temp_dir()` with a unique per-process prefix; cleaned up best-effort at end. Snapshots via `insta`.

## License

Dual-licensed under [MIT](../../LICENSE) or [Apache-2.0](../../LICENSE), matching the rest of the Buff workspace.

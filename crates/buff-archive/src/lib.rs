//! `buff-archive` — Zip/Tar/Gz/Zstd compression for the Buff language.
//!
//! Pure-Rust MVP wrapping four mature Rust crates via a safe FFI
//! boundary per the [T4 FFI guide](../buff-lang-ffi-guide/GUIDE.md).
//!
//! | Format | Backend crate | Mode |
//! |--------|---------------|------|
//! | `Zip`  | `zip` (deflate only — see root `Cargo.toml` rationale) | multi-file archive |
//! | `Tar`  | `tar` | multi-file archive (uncompressed) |
//! | `Gz`   | `flate2` (pure-Rust `miniz_oxide` backend) | single-stream codec |
//! | `Zstd` | `ruzstd` (pure-Rust — NOT `zstd` which wraps C libzstd) | single-stream codec |
//!
//! # Pipeline
//!
//! ```text
//!   Directory ──┬──▶ Archive.compress_dir(out, Zip)  ──▶ .zip
//!               ├──▶ Archive.compress_dir(out, Tar)  ──▶ .tar
//!               ├──▶ Archive.compress_dir(out, Gz)   ──▶ .tar.gz  (tar inside gzip)
//!               └──▶ Archive.compress_dir(out, Zstd) ──▶ .tar.zst (tar inside zstd)
//!
//!   .zip / .tar / .tar.gz / .tar.zst ──▶ Archive.extract(dest)  (auto-detected)
//!
//!   bytes ──▶ Archive.compress_bytes(Gz | Zstd)   ──▶ compressed bytes
//!   compressed bytes ──▶ Archive.decompress_bytes(Gz | Zstd) ──▶ bytes
//! ```
//!
//! # FFI safety
//!
//! Every public entry point follows the 6 hard rules from
//! `crates/buff-lang-ffi-guide/GUIDE.md`:
//!
//! | Rule | How this crate complies |
//! |------|-------------------------|
//! | R1 — No raw pointers | Public surface exposes only `Format` + `ArchiveError` + the namespace marker `Archive`. No `*const` / `*mut` anywhere. |
//! | R2 — Ownership boundary | `compress_bytes` / `decompress_bytes` return owned `Vec<u8>`. `compress_dir` / `extract` consume only `AsRef<Path>` borrows for path lookup. |
//! | R3 — Error mapping | Every fallible op returns `Result<T, ArchiveError>`. Underlying `zip::ZipError` mapped via `From`; `tar` / `flate2` / `ruzstd` errors stringified at the boundary. |
//! | R4 — Thread safety | `Format` is `Copy + Send + Sync`. `Archive` is a unit struct (no state). |
//! | R5 — Lifetime hiding | No public lifetime parameters. All inputs are owned or borrowed paths; all outputs are owned. |
//! | R6 — Panic boundary | `compress_dir` / `extract` / `compress_bytes` / `decompress_bytes` wrap their bodies in `catch_unwind`. |
//!
//! # Panic-free contract
//!
//! No `unwrap` / `expect` / `panic!` / `todo!` / `unimplemented!` in
//! non-test code. Every fallible op returns `Result`.
//!
//! # Scope (per T39 spec)
//!
//! - **NO 7z, RAR, BZip2** — explicitly forbidden by the spec.
//! - **NO encryption-at-rest** — combine with T34 (`buff-auth`) if needed.
//! - **NO zstd-in-zip** — the `zip` crate's `zstd` feature would
//!   transitively pull `cc-rs` + C libzstd; we disable it. Deflate
//!   remains the dominant zip codec in the wild.

pub mod error;

pub use error::ArchiveError;

use std::io::{Read, Write};
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::Path;

/// The four compression / archive formats supported by [`Archive`].
///
/// `Zip` and `Tar` are multi-file archive formats (they carry many
/// entries with relative paths + metadata). `Gz` and `Zstd` are
/// single-stream codecs (one byte stream in, one compressed byte
/// stream out).
///
/// When `Gz` / `Zstd` are passed to [`Archive::compress_dir`], the
/// directory is first packed into an uncompressed TAR and the codec
/// is then applied to the resulting byte stream (producing a `.tar.gz`
/// / `.tar.zst` file). This mirrors the cross-language convention
/// (`tarfile` + `gzip` / `tarfile` + `zstandard` in Python; `tar -czf`
/// / `tar --zstd -cf` on the CLI).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Format {
    /// ZIP archive — multi-file, deflate-compressed per-entry. The
    /// dominant cross-platform archive format.
    Zip,
    /// TAR archive — multi-file, uncompressed. The Unix ecosystem's
    /// canonical archive format (usually paired with `Gz` or `Zstd`).
    Tar,
    /// Gzip single-stream codec (RFC 1952). Compresses one byte
    /// stream; pair with `Tar` for multi-file archives.
    Gz,
    /// Zstandard single-stream codec (RFC 8878). Modern high-ratio
    /// codec; pair with `Tar` for multi-file archives.
    Zstd,
}

impl Format {
    /// Parse a file extension (case-insensitive, no leading dot) into
    /// a [`Format`]. Returns `None` for unrecognised extensions or
    /// for extensions of formats this crate does not support (e.g.
    /// `rar`, `7z`, `bz2`).
    ///
    /// Accepted inputs:
    /// - `zip` → [`Format::Zip`]
    /// - `tar` → [`Format::Tar`]
    /// - `gz` / `gzip` → [`Format::Gz`]
    /// - `zst` / `zstd` → [`Format::Zstd`]
    pub fn from_extension(ext: &str) -> Option<Self> {
        match ext.to_ascii_lowercase().as_str() {
            "zip" => Some(Format::Zip),
            "tar" => Some(Format::Tar),
            "gz" | "gzip" => Some(Format::Gz),
            "zst" | "zstd" => Some(Format::Zstd),
            _ => None,
        }
    }

    /// The canonical file extension (without leading dot) for this
    /// format. Used by [`Format::from_path`] and by integration tests.
    pub const fn extension(self) -> &'static str {
        match self {
            Format::Zip => "zip",
            Format::Tar => "tar",
            Format::Gz => "gz",
            Format::Zstd => "zst",
        }
    }

    /// Infer the format from a file path's extension. Returns `None`
    /// if the extension is unrecognised, OR if the path's extension
    /// is `tgz` / `txz` / `taz` (the single-letter tar+codec
    /// compounds — caller should explicitly use `Tar` for the inner
    /// archive; `Archive::extract` special-cases `.tar.gz` / `.tar.zst`
    /// internally so the user does not need to disambiguate).
    pub fn from_path(path: &Path) -> Option<Self> {
        let ext = path.extension()?.to_str()?.to_ascii_lowercase();
        // `.tar.gz` / `.tar.zst` compound extensions: caller passes
        // the path to `Archive::extract` and the codec is detected
        // internally. `from_path` returns the codec (Gz / Zstd) so
        // the user-facing `Format.from_path("a.tar.gz")` returns Gz.
        if let Some(rest) = ext.strip_prefix("tar.") {
            return Format::from_extension(rest);
        }
        Format::from_extension(&ext)
    }

    /// Returns `true` for the multi-file archive formats (`Zip`,
    /// `Tar`). Returns `false` for the single-stream codecs (`Gz`,
    /// `Zstd`). Used internally to reject byte-level operations on
    /// multi-file formats with a clear error.
    pub const fn is_multifile(self) -> bool {
        matches!(self, Format::Zip | Format::Tar)
    }
}

impl std::fmt::Display for Format {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.extension())
    }
}

/// Namespace marker for the archive API. The struct itself carries
/// no state; all functionality is exposed via associated functions
/// ([`Archive::compress_dir`] / [`Archive::extract`] /
/// [`Archive::compress_bytes`] / [`Archive::decompress_bytes`]).
///
/// Mirrors the `Log` / `Toml` / `Config` namespace-only prelude-type
/// pattern — `Archive` is never instantiated.
pub struct Archive;

impl Archive {
    /// Compress a directory tree into an archive file.
    ///
    /// - `Zip`: writes a multi-file `.zip` (per-entry deflate).
    /// - `Tar`: writes an uncompressed `.tar`.
    /// - `Gz`: tars the dir then gzip-compresses the stream → `.tar.gz`.
    /// - `Zstd`: tars the dir then zstd-compresses the stream → `.tar.zst`.
    ///
    /// The output file's extension is the caller's responsibility —
    /// `Archive::extract` auto-detects the format from the file's
    /// extension, so callers should name the output consistently
    /// (`.zip` / `.tar` / `.tar.gz` / `.tar.zst`).
    ///
    /// `input_dir` must exist and be readable; `output_path`'s parent
    /// directory must exist and be writable.
    ///
    /// Wrapped in `catch_unwind` per T4 FFI guide R6.
    pub fn compress_dir<P, Q>(
        input_dir: P,
        output_path: Q,
        format: Format,
    ) -> Result<(), ArchiveError>
    where
        P: AsRef<Path>,
        Q: AsRef<Path>,
    {
        let input_dir = input_dir.as_ref().to_path_buf();
        let output_path = output_path.as_ref().to_path_buf();
        let result = catch_unwind(AssertUnwindSafe(|| {
            compress_dir_inner(&input_dir, &output_path, format)
        }));
        match result {
            Ok(Ok(())) => Ok(()),
            Ok(Err(err)) => Err(err),
            Err(_) => Err(ArchiveError::Panic),
        }
    }

    /// Extract an archive file into a destination directory.
    ///
    /// The format is auto-detected from the file's extension:
    /// - `.zip` → multi-file ZIP extraction.
    /// - `.tar` → multi-file TAR extraction.
    /// - `.tar.gz` / `.tgz` → Gzip-decode + TAR-extract.
    /// - `.tar.zst` / `.zst` → Zstd-decode + TAR-extract.
    ///
    /// `output_dir` is created (recursively) if it does not exist;
    /// this mirrors `tar::Archive::unpack`'s behaviour and the
    /// cross-language convention (`unzip -d`, `tar -C`).
    ///
    /// Wrapped in `catch_unwind` per T4 FFI guide R6.
    pub fn extract<P, Q>(archive_path: P, output_dir: Q) -> Result<(), ArchiveError>
    where
        P: AsRef<Path>,
        Q: AsRef<Path>,
    {
        let archive_path = archive_path.as_ref().to_path_buf();
        let output_dir = output_dir.as_ref().to_path_buf();
        let result = catch_unwind(AssertUnwindSafe(|| {
            extract_inner(&archive_path, &output_dir)
        }));
        match result {
            Ok(Ok(())) => Ok(()),
            Ok(Err(err)) => Err(err),
            Err(_) => Err(ArchiveError::Panic),
        }
    }

    /// Compress a byte stream with a single-stream codec.
    ///
    /// Only [`Format::Gz`] and [`Format::Zstd`] are accepted — `Zip`
    /// and `Tar` are multi-file archive formats and have no
    /// byte-stream-level definition. Passing them returns
    /// [`ArchiveError::UnsupportedForByteStream`].
    ///
    /// Wrapped in `catch_unwind` per T4 FFI guide R6.
    pub fn compress_bytes(bytes: &[u8], format: Format) -> Result<Vec<u8>, ArchiveError> {
        if bytes.is_empty() {
            return Err(ArchiveError::EmptyInput);
        }
        let bytes_owned = bytes.to_vec();
        let result = catch_unwind(AssertUnwindSafe(|| {
            compress_bytes_inner(&bytes_owned, format)
        }));
        match result {
            Ok(Ok(out)) => Ok(out),
            Ok(Err(err)) => Err(err),
            Err(_) => Err(ArchiveError::Panic),
        }
    }

    /// Decompress a byte stream produced by [`Archive::compress_bytes`].
    ///
    /// Only [`Format::Gz`] and [`Format::Zstd`] are accepted — `Zip`
    /// and `Tar` are multi-file archive formats and have no
    /// byte-stream-level definition. Passing them returns
    /// [`ArchiveError::UnsupportedForByteStream`].
    ///
    /// Wrapped in `catch_unwind` per T4 FFI guide R6.
    pub fn decompress_bytes(bytes: &[u8], format: Format) -> Result<Vec<u8>, ArchiveError> {
        if bytes.is_empty() {
            return Err(ArchiveError::EmptyInput);
        }
        let bytes_owned = bytes.to_vec();
        let result = catch_unwind(AssertUnwindSafe(|| {
            decompress_bytes_inner(&bytes_owned, format)
        }));
        match result {
            Ok(Ok(out)) => Ok(out),
            Ok(Err(err)) => Err(err),
            Err(_) => Err(ArchiveError::Panic),
        }
    }
}

// ---------------------------------------------------------------------------
// Internal helpers (crate-private). None of these use catch_unwind — the
// public entry points wrap the call sites.
// ---------------------------------------------------------------------------

fn compress_dir_inner(
    input_dir: &Path,
    output_path: &Path,
    format: Format,
) -> Result<(), ArchiveError> {
    if !input_dir.is_dir() {
        return Err(ArchiveError::Io(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            format!(
                "compress_dir: input not a directory: {}",
                input_dir.display()
            ),
        )));
    }
    match format {
        Format::Zip => write_zip(input_dir, output_path),
        Format::Tar => write_tar(input_dir, output_path, false, Format::Tar),
        Format::Gz => write_tar(input_dir, output_path, true, Format::Gz),
        Format::Zstd => write_tar(input_dir, output_path, true, Format::Zstd),
    }
}

fn extract_inner(archive_path: &Path, output_dir: &Path) -> Result<(), ArchiveError> {
    let format = Format::from_path(archive_path).ok_or_else(|| ArchiveError::UnknownFormat {
        path: archive_path.display().to_string(),
    })?;
    std::fs::create_dir_all(output_dir)?;
    match format {
        Format::Zip => read_zip(archive_path, output_dir),
        Format::Tar => read_tar(Box::new(std::fs::File::open(archive_path)?), output_dir),
        Format::Gz => {
            let file = std::fs::File::open(archive_path)?;
            let decoder = flate2::read::GzDecoder::new(file);
            read_tar(Box::new(decoder), output_dir)
        }
        Format::Zstd => {
            let file = std::fs::File::open(archive_path)?;
            let decoder = ruzstd::decoding::StreamingDecoder::new(file)
                .map_err(|e| ArchiveError::Zstd(e.to_string()))?;
            read_tar(Box::new(decoder), output_dir)
        }
    }
}

fn compress_bytes_inner(bytes: &[u8], format: Format) -> Result<Vec<u8>, ArchiveError> {
    match format {
        Format::Gz => {
            let mut encoder =
                flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
            encoder
                .write_all(bytes)
                .map_err(|e| ArchiveError::Gzip(e.to_string()))?;
            encoder
                .finish()
                .map_err(|e| ArchiveError::Gzip(e.to_string()))
        }
        Format::Zstd => Ok(ruzstd::encoding::compress_to_vec(
            bytes,
            ruzstd::encoding::CompressionLevel::Fastest,
        )),
        Format::Zip | Format::Tar => Err(ArchiveError::UnsupportedForByteStream { format }),
    }
}

fn decompress_bytes_inner(bytes: &[u8], format: Format) -> Result<Vec<u8>, ArchiveError> {
    match format {
        Format::Gz => {
            let mut decoder = flate2::read::GzDecoder::new(bytes);
            let mut out = Vec::new();
            decoder
                .read_to_end(&mut out)
                .map_err(|e| ArchiveError::Gzip(e.to_string()))?;
            Ok(out)
        }
        Format::Zstd => {
            let mut decoder = ruzstd::decoding::StreamingDecoder::new(bytes)
                .map_err(|e| ArchiveError::Zstd(e.to_string()))?;
            let mut out = Vec::new();
            decoder
                .read_to_end(&mut out)
                .map_err(|e| ArchiveError::Zstd(e.to_string()))?;
            Ok(out)
        }
        Format::Zip | Format::Tar => Err(ArchiveError::UnsupportedForByteStream { format }),
    }
}

/// Walk `input_dir` recursively, write every regular file as a
/// deflate-compressed entry into a new ZIP at `output_path`.
fn write_zip(input_dir: &Path, output_path: &Path) -> Result<(), ArchiveError> {
    let file = std::fs::File::create(output_path)?;
    let mut zip = zip::ZipWriter::new(file);
    let options = zip::write::SimpleFileOptions::default();

    let entries = collect_dir_entries(input_dir)?;
    for (relative_name, abs_path) in entries {
        if abs_path.is_file() {
            zip.start_file(&relative_name, options)?;
            let mut f = std::fs::File::open(&abs_path)?;
            std::io::copy(&mut f, &mut zip)?;
        }
    }
    zip.finish()?;
    Ok(())
}

/// Pack `input_dir` into a TAR. If `wrap_codec` is `true`, wrap the
/// tar byte stream in `codec` (Gz or Zstd) before writing to disk.
fn write_tar(
    input_dir: &Path,
    output_path: &Path,
    wrap_codec: bool,
    codec: Format,
) -> Result<(), ArchiveError> {
    let file = std::fs::File::create(output_path)?;
    match (wrap_codec, codec) {
        (false, _) => {
            let mut builder = tar::Builder::new(file);
            builder.append_dir_all(".", input_dir)?;
            builder.finish()?;
        }
        (true, Format::Gz) => {
            let encoder = flate2::write::GzEncoder::new(file, flate2::Compression::default());
            let mut builder = tar::Builder::new(encoder);
            builder.append_dir_all(".", input_dir)?;
            let encoder = builder.into_inner()?;
            encoder
                .finish()
                .map_err(|e| ArchiveError::Gzip(e.to_string()))?;
        }
        (true, Format::Zstd) => {
            // ruzstd's encoder writes to a Vec; we then copy the
            // compressed bytes to `file`. tar -> Vec<u8> -> ruzstd ->
            // file keeps the boundary simple.
            let mut tar_buf: Vec<u8> = Vec::new();
            {
                let mut builder = tar::Builder::new(&mut tar_buf);
                builder.append_dir_all(".", input_dir)?;
                builder.finish()?;
            }
            let compressed = ruzstd::encoding::compress_to_vec(
                &tar_buf,
                ruzstd::encoding::CompressionLevel::Fastest,
            );
            let mut file = file;
            file.write_all(&compressed)?;
        }
        // The other arms are unreachable: write_tar is only called
        // with (false, Tar) | (true, Gz) | (true, Zstd) from
        // compress_dir_inner. The match arms above cover all three.
        _ => {
            return Err(ArchiveError::Io(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                format!("write_tar: invalid codec combination: wrap={wrap_codec}, codec={codec:?}"),
            )));
        }
    }
    Ok(())
}

/// Open a ZIP archive and unpack every entry into `output_dir`.
/// Mirrors `zip::ZipArchive::extract` but explicit so we surface
/// errors through [`ArchiveError::Zip`].
fn read_zip(archive_path: &Path, output_dir: &Path) -> Result<(), ArchiveError> {
    let file = std::fs::File::open(archive_path)?;
    let mut archive = zip::ZipArchive::new(file)?;
    for i in 0..archive.len() {
        let mut entry = archive.by_index(i)?;
        let outpath = match entry.enclosed_name() {
            Some(p) => output_dir.join(p),
            None => continue,
        };
        if entry.is_dir() {
            std::fs::create_dir_all(&outpath)?;
        } else {
            if let Some(parent) = outpath.parent() {
                std::fs::create_dir_all(parent)?;
            }
            let mut outfile = std::fs::File::create(&outpath)?;
            std::io::copy(&mut entry, &mut outfile)?;
        }
    }
    Ok(())
}

/// Unpack a TAR byte stream into `output_dir`. The byte stream may
/// be a raw `File` (for `.tar`) OR a `StreamingDecoder` /
/// `GzDecoder` wrapping a `File` (for `.tar.zst` / `.tar.gz`).
fn read_tar(reader: Box<dyn Read>, output_dir: &Path) -> Result<(), ArchiveError> {
    let mut archive = tar::Archive::new(reader);
    archive.unpack(output_dir)?;
    Ok(())
}

/// Walk `dir` recursively, returning `(relative_path, absolute_path)`
/// pairs sorted by relative path (deterministic output — important
/// for reproducible archives + insta snapshot tests). Directory
/// entries are NOT included (the ZIP / TAR backends recreate parent
/// directories automatically as needed).
fn collect_dir_entries(dir: &Path) -> Result<Vec<(String, std::path::PathBuf)>, ArchiveError> {
    let mut out: Vec<(String, std::path::PathBuf)> = Vec::new();
    visit_dir(dir, dir, &mut out)?;
    out.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(out)
}

/// Recursive helper for [`collect_dir_entries`]. Walks `current`
/// relative to `root`, appending `(relative_name, abs_path)` to
/// `out` for every regular file encountered.
fn visit_dir(
    root: &Path,
    current: &Path,
    out: &mut Vec<(String, std::path::PathBuf)>,
) -> Result<(), ArchiveError> {
    let rd = std::fs::read_dir(current)?;
    for entry in rd {
        let entry = entry?;
        let path = entry.path();
        let rel = path.strip_prefix(root).unwrap_or(&path);
        let rel_name = rel.to_string_lossy().replace('\\', "/");
        if path.is_dir() {
            visit_dir(root, &path, out)?;
        } else if path.is_file() {
            out.push((rel_name, path));
        }
    }
    Ok(())
}

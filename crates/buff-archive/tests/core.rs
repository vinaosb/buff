//! Integration tests for the `buff-archive` crate.
//!
//! Covers all 7 public API entries per the T39 spec:
//! - `Format::{Zip, Tar, Gz, Zstd}` enum + 3 instance fns
//!   (`from_extension`, `extension`, `from_path`, `is_multifile`).
//! - `Archive::compress_dir` (4 formats × roundtrip).
//! - `Archive::extract` (4 formats × auto-detected roundtrip).
//! - `Archive::compress_bytes` (2 single-stream codecs × roundtrip).
//! - `Archive::decompress_bytes` (2 single-stream codecs × roundtrip).
//! - Error paths: empty input, missing dir, unknown format, byte-stream
//!   on multi-file format.
//!
//! All tests are hermetic: temp directories created under
//! `std::env::temp_dir()` with a unique prefix; cleaned up at end
//! (best-effort — `std::fs::remove_dir_all` errors ignored).
//!
//! Per the T39 acceptance criteria: 4+ examples + 12+ tests + 5 insta
//! snapshots. We ship 16 tests + 5 snapshots.

#![allow(clippy::needless_borrow)] // test-path ergonomic borrow

use buff_archive::{Archive, ArchiveError, Format};
use std::fs;
use std::path::PathBuf;

/// Build a unique temp dir for this test process. Returns the path;
/// the caller is responsible for cleanup (via `best_effort_cleanup`).
fn test_root(test_name: &str) -> PathBuf {
    let p = std::env::temp_dir().join(format!(
        "buff-archive-test-{}-{}-{test_name}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0),
    ));
    fs::create_dir_all(&p).expect("create test_root");
    p
}

fn best_effort_cleanup(path: &std::path::Path) {
    let _ = fs::remove_dir_all(path);
}

/// Build a known-shape source dir: 3 files in 2 subdirs.
/// `root/a.txt` ("alpha"), `root/sub/b.txt` ("beta"),
/// `root/sub/deep/c.txt` ("gamma").
fn build_source_dir(root: &std::path::Path) {
    fs::create_dir_all(root.join("sub/deep")).expect("mkdir sub/deep");
    fs::write(root.join("a.txt"), "alpha").expect("write a.txt");
    fs::write(root.join("sub/b.txt"), "beta").expect("write sub/b.txt");
    fs::write(root.join("sub/deep/c.txt"), "gamma").expect("write sub/deep/c.txt");
}

fn assert_roundtrip(_input_root: &std::path::Path, extracted_root: &std::path::Path) {
    let pairs = [
        ("a.txt", "alpha"),
        ("sub/b.txt", "beta"),
        ("sub/deep/c.txt", "gamma"),
    ];
    for (rel, expected) in pairs {
        let extracted = extracted_root.join(rel);
        assert!(
            extracted.exists(),
            "extracted file missing: {}",
            extracted.display()
        );
        let got =
            fs::read_to_string(&extracted).unwrap_or_else(|_| panic!("read extracted {}", rel));
        assert_eq!(got, expected, "content mismatch in extracted {rel}");
    }
}

// ---- Format enum -----------------------------------------------------------

#[test]
fn format_from_extension_canonical() {
    assert_eq!(Format::from_extension("zip"), Some(Format::Zip));
    assert_eq!(Format::from_extension("tar"), Some(Format::Tar));
    assert_eq!(Format::from_extension("gz"), Some(Format::Gz));
    assert_eq!(Format::from_extension("gzip"), Some(Format::Gz));
    assert_eq!(Format::from_extension("zst"), Some(Format::Zstd));
    assert_eq!(Format::from_extension("zstd"), Some(Format::Zstd));
}

#[test]
fn format_from_extension_case_insensitive() {
    assert_eq!(Format::from_extension("ZIP"), Some(Format::Zip));
    assert_eq!(Format::from_extension("GZ"), Some(Format::Gz));
    assert_eq!(Format::from_extension("ZST"), Some(Format::Zstd));
}

#[test]
fn format_from_extension_rejects_unknown() {
    assert_eq!(Format::from_extension("rar"), None);
    assert_eq!(Format::from_extension("7z"), None);
    assert_eq!(Format::from_extension("bz2"), None);
    assert_eq!(Format::from_extension(""), None);
}

#[test]
fn format_extension_round_trips() {
    for f in [Format::Zip, Format::Tar, Format::Gz, Format::Zstd] {
        let ext = f.extension();
        assert_eq!(Format::from_extension(ext), Some(f));
    }
}

#[test]
fn format_from_path_simple_extensions() {
    assert_eq!(
        Format::from_path(std::path::Path::new("a.zip")),
        Some(Format::Zip)
    );
    assert_eq!(
        Format::from_path(std::path::Path::new("a.tar")),
        Some(Format::Tar)
    );
    assert_eq!(
        Format::from_path(std::path::Path::new("a.gz")),
        Some(Format::Gz)
    );
    assert_eq!(
        Format::from_path(std::path::Path::new("a.zst")),
        Some(Format::Zstd)
    );
}

#[test]
fn format_from_path_compound_extensions() {
    assert_eq!(
        Format::from_path(std::path::Path::new("a.tar.gz")),
        Some(Format::Gz)
    );
    assert_eq!(
        Format::from_path(std::path::Path::new("a.tar.zst")),
        Some(Format::Zstd)
    );
}

#[test]
fn format_from_path_rejects_unknown() {
    assert_eq!(Format::from_path(std::path::Path::new("a.txt")), None);
    assert_eq!(Format::from_path(std::path::Path::new("noext")), None);
}

#[test]
fn format_is_multifile_classification() {
    assert!(Format::Zip.is_multifile());
    assert!(Format::Tar.is_multifile());
    assert!(!Format::Gz.is_multifile());
    assert!(!Format::Zstd.is_multifile());
}

#[test]
fn format_display_matches_extension() {
    assert_eq!(format!("{}", Format::Zip), "zip");
    assert_eq!(format!("{}", Format::Tar), "tar");
    assert_eq!(format!("{}", Format::Gz), "gz");
    assert_eq!(format!("{}", Format::Zstd), "zst");
}

// ---- compress_dir + extract roundtrips (4 formats) ------------------------

#[test]
fn roundtrip_zip_dir() {
    let root = test_root("roundtrip_zip_dir");
    let src = root.join("src");
    let archive_path = root.join("out.zip");
    let extracted = root.join("extracted");
    build_source_dir(&src);

    Archive::compress_dir(&src, &archive_path, Format::Zip).expect("compress zip");
    assert!(archive_path.exists(), "zip file was not written");
    Archive::extract(&archive_path, &extracted).expect("extract zip");
    assert_roundtrip(&src, &extracted);
    best_effort_cleanup(&root);
}

#[test]
fn roundtrip_tar_dir() {
    let root = test_root("roundtrip_tar_dir");
    let src = root.join("src");
    let archive_path = root.join("out.tar");
    let extracted = root.join("extracted");
    build_source_dir(&src);

    Archive::compress_dir(&src, &archive_path, Format::Tar).expect("compress tar");
    assert!(archive_path.exists(), "tar file was not written");
    Archive::extract(&archive_path, &extracted).expect("extract tar");
    assert_roundtrip(&src, &extracted);
    best_effort_cleanup(&root);
}

#[test]
fn roundtrip_gz_dir() {
    let root = test_root("roundtrip_gz_dir");
    let src = root.join("src");
    let archive_path = root.join("out.tar.gz");
    let extracted = root.join("extracted");
    build_source_dir(&src);

    Archive::compress_dir(&src, &archive_path, Format::Gz).expect("compress gz");
    assert!(archive_path.exists(), "tar.gz file was not written");
    Archive::extract(&archive_path, &extracted).expect("extract gz");
    assert_roundtrip(&src, &extracted);
    best_effort_cleanup(&root);
}

#[test]
fn roundtrip_zstd_dir() {
    let root = test_root("roundtrip_zstd_dir");
    let src = root.join("src");
    let archive_path = root.join("out.tar.zst");
    let extracted = root.join("extracted");
    build_source_dir(&src);

    Archive::compress_dir(&src, &archive_path, Format::Zstd).expect("compress zstd");
    assert!(archive_path.exists(), "tar.zst file was not written");
    Archive::extract(&archive_path, &extracted).expect("extract zstd");
    assert_roundtrip(&src, &extracted);
    best_effort_cleanup(&root);
}

// ---- compress_bytes + decompress_bytes (single-stream codecs) -------------

#[test]
fn roundtrip_gz_bytes() {
    let original = b"the quick brown fox jumps over the lazy dog".repeat(64);
    let compressed = Archive::compress_bytes(&original, Format::Gz).expect("compress bytes gz");
    assert!(
        compressed.len() < original.len(),
        "gzip should compress repetitive input: got {} from {}",
        compressed.len(),
        original.len()
    );
    let recovered =
        Archive::decompress_bytes(&compressed, Format::Gz).expect("decompress bytes gz");
    assert_eq!(recovered, original);
}

#[test]
fn roundtrip_zstd_bytes() {
    let original = b"the quick brown fox jumps over the lazy dog".repeat(64);
    let compressed = Archive::compress_bytes(&original, Format::Zstd).expect("compress bytes zstd");
    let recovered =
        Archive::decompress_bytes(&compressed, Format::Zstd).expect("decompress bytes zstd");
    assert_eq!(recovered, original);
}

#[test]
fn compress_bytes_rejects_multifile_formats() {
    let data = b"some bytes";
    let err = Archive::compress_bytes(data, Format::Zip).unwrap_err();
    assert!(matches!(
        err,
        ArchiveError::UnsupportedForByteStream {
            format: Format::Zip
        }
    ));
    let err = Archive::compress_bytes(data, Format::Tar).unwrap_err();
    assert!(matches!(
        err,
        ArchiveError::UnsupportedForByteStream {
            format: Format::Tar
        }
    ));
}

#[test]
fn decompress_bytes_rejects_multifile_formats() {
    let data = b"some bytes";
    let err = Archive::decompress_bytes(data, Format::Zip).unwrap_err();
    assert!(matches!(err, ArchiveError::UnsupportedForByteStream { .. }));
    let err = Archive::decompress_bytes(data, Format::Tar).unwrap_err();
    assert!(matches!(err, ArchiveError::UnsupportedForByteStream { .. }));
}

// ---- Error paths ----------------------------------------------------------

#[test]
fn compress_dir_rejects_missing_input() {
    let root = test_root("compress_dir_rejects_missing_input");
    let bogus = root.join("nonexistent_src");
    let out = root.join("out.zip");
    let err = Archive::compress_dir(&bogus, &out, Format::Zip).unwrap_err();
    assert!(matches!(err, ArchiveError::Io(_)));
    best_effort_cleanup(&root);
}

#[test]
fn extract_rejects_unknown_extension() {
    let root = test_root("extract_rejects_unknown_extension");
    let bogus_archive = root.join("data.bin");
    fs::write(&bogus_archive, b"not an archive").expect("write bogus");
    let dest = root.join("dest");
    let err = Archive::extract(&bogus_archive, &dest).unwrap_err();
    assert!(matches!(err, ArchiveError::UnknownFormat { .. }));
    best_effort_cleanup(&root);
}

#[test]
fn compress_bytes_rejects_empty() {
    let err = Archive::compress_bytes(&[], Format::Gz).unwrap_err();
    assert!(matches!(err, ArchiveError::EmptyInput));
}

#[test]
fn decompress_bytes_rejects_empty() {
    let err = Archive::decompress_bytes(&[], Format::Zstd).unwrap_err();
    assert!(matches!(err, ArchiveError::EmptyInput));
}

// ---- Insta snapshots (5) --------------------------------------------------

#[test]
fn snapshot_format_all_variants() {
    insta::assert_snapshot!(
        "format_all",
        format!(
            "{:?}|{:?}|{:?}|{:?}|{}|{}|{}|{}|{}|{}",
            Format::Zip,
            Format::Tar,
            Format::Gz,
            Format::Zstd,
            Format::Zip.extension(),
            Format::Tar.extension(),
            Format::Gz.extension(),
            Format::Zstd.extension(),
            Format::Zip.is_multifile(),
            Format::Gz.is_multifile(),
        )
    );
}

#[test]
fn snapshot_archive_error_debug() {
    let err1 = ArchiveError::EmptyInput;
    let err2 = ArchiveError::UnsupportedForByteStream {
        format: Format::Zip,
    };
    let err3 = ArchiveError::UnknownFormat {
        path: "/tmp/foo.bin".into(),
    };
    insta::assert_snapshot!("archive_error_debug", format!("{err1}\n{err2}\n{err3}"));
}

#[test]
fn snapshot_format_extension_lookup_table() {
    let table = ["zip", "tar", "gz", "gzip", "zst", "zstd", "rar", "7z", ""]
        .iter()
        .map(|e| format!("{e} -> {:?}", Format::from_extension(e)))
        .collect::<Vec<_>>()
        .join("\n");
    insta::assert_snapshot!("format_extension_lookup_table", table);
}

#[test]
fn snapshot_format_from_path_samples() {
    let samples = [
        "a.zip",
        "a.tar",
        "a.gz",
        "a.zst",
        "a.tar.gz",
        "a.tar.zst",
        "a.txt",
        "noext",
    ];
    let rendered = samples
        .iter()
        .map(|p| {
            let f = Format::from_path(std::path::Path::new(p));
            format!("{p} -> {f:?}")
        })
        .collect::<Vec<_>>()
        .join("\n");
    insta::assert_snapshot!("format_from_path_samples", rendered);
}

#[test]
fn snapshot_archive_compressed_size_profile() {
    // Sanity snapshot: a high-entropy (random-ish) input vs a
    // low-entropy (repetitive) input — both should roundtrip via
    // both single-stream codecs. Records the *ratio* (compressed /
    // original) so a future regression in compress_to_vec surfaces
    // clearly. ruzstd's compressor is acknowledged suboptimal, so
    // we only assert "<= original size" for Gz (the mature codec);
    // for Zstd we just record the ratio without an upper bound
    // (ruzstd's encoder occasionally produces output slightly larger
    // than the input for incompressible data).
    let repetitive = "buff".repeat(2048);
    let incompressible: Vec<u8> = (0..4096u32).map(|i| (i ^ 0x5a) as u8).collect();

    let gz_rep = Archive::compress_bytes(repetitive.as_bytes(), Format::Gz).unwrap();
    let zstd_rep = Archive::compress_bytes(repetitive.as_bytes(), Format::Zstd).unwrap();
    let zstd_incomp = Archive::compress_bytes(&incompressible, Format::Zstd).unwrap();

    assert!(
        gz_rep.len() <= repetitive.len(),
        "gz should never inflate output"
    );

    insta::assert_snapshot!(
        "archive_compressed_size_profile",
        format!(
            "gz/repetitive: {:.3}\nzstd/repetitive: {:.3}\nzstd/incompressible: {:.3}\n",
            gz_rep.len() as f64 / repetitive.len() as f64,
            zstd_rep.len() as f64 / repetitive.len() as f64,
            zstd_incomp.len() as f64 / incompressible.len() as f64,
        )
    );
}

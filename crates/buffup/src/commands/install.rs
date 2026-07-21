//! `buffup install <version>` — download and unpack a Buff release.
//!
//! Flow:
//!
//! 1. Parse `<version>` as a strict semver `MAJOR.MINOR.PATCH`.
//! 2. Compute the target install dir `<buff_home>/versions/<ver>/`.
//!    If it already exists, fail fast with
//!    [`BuffupError::VersionAlreadyInstalled`] (the user must
//!    manually delete the dir to reinstall — same stance as
//!    rustup).
//! 3. Query the GitHub Releases API for the matching release tag.
//! 4. Stream the gzip tarball to memory, then unpack into the
//!    target dir via `flate2::GzDecoder` + `tar::Archive::unpack`.
//!
//! # Graceful failure when Releases don't exist
//!
//! As of v1.0 the buff-lang/buff repo has NOT published a GitHub
//! Release. The CLI MUST surface a clear "no release published"
//! message rather than a cryptic `HttpStatus(404)`. The dispatch in
//! [`run`] special-cases [`BuffupError::HttpStatus`] with code `404`
//! to print actionable guidance (build-from-source instructions)
//! before re-returning the error.

use std::io::Cursor;

use crate::error::BuffupError;
use crate::{github, paths};

/// Entry point for `buffup install <version>`.
pub async fn run(version: String) -> Result<(), BuffupError> {
    let v = semver::Version::parse(&version)?;

    let target = paths::version_dir(&v)?;
    if target.exists() {
        return Err(BuffupError::VersionAlreadyInstalled(v));
    }

    eprintln!("buffup: v{} — querying GitHub Releases...", v);

    let client = reqwest::Client::builder()
        .user_agent(github::USER_AGENT)
        .build()?;

    let release = match github::fetch_release(&client, &v.to_string()).await {
        Ok(r) => r,
        Err(BuffupError::HttpStatus(404)) => {
            eprintln!(
                "buffup: Buff v{} has not been published to GitHub Releases yet.",
                v
            );
            eprintln!("       Pre-built binaries are part of the v1.12 milestone (T139).");
            eprintln!("       Until then, build from source:");
            eprintln!("         git clone https://github.com/buff-lang/buff");
            eprintln!("         cargo install --path crates/buff-lang-cli");
            return Err(BuffupError::HttpStatus(404));
        }
        Err(e) => return Err(e),
    };

    eprintln!("buffup: v{} — downloading tarball...", v);
    let tarball_bytes = client
        .get(&release.tarball_url)
        .header(reqwest::header::USER_AGENT, github::USER_AGENT)
        .send()
        .await?
        .bytes()
        .await?;

    eprintln!(
        "buffup: v{} — unpacking {} bytes into {}...",
        v,
        tarball_bytes.len(),
        target.display()
    );

    std::fs::create_dir_all(&target)?;
    extract_gzip_tarball(&tarball_bytes, &target).map_err(|e| {
        BuffupError::Extract(format!(
            "failed to unpack {} bytes into {}: {}",
            tarball_bytes.len(),
            target.display(),
            e
        ))
    })?;

    eprintln!("buffup: v{} installed at {}", v, target.display());
    eprintln!(
        "       run `buffup default {}` to make it the active version.",
        v
    );
    Ok(())
}

/// Decompress a gzip tarball and unpack into `dest`.
///
/// Pure-Rust via `flate2::GzDecoder` (miniz_oxide backend — no zlib
/// native dep) wrapping `tar::Archive::unpack`. The caller is
/// responsible for creating `dest` and cleaning up on error (the
/// caller in [`run`] does NOT clean up — partial extracts are
/// surfaced as [`BuffupError::Extract`] so the user can inspect the
/// version dir and decide whether to delete it).
fn extract_gzip_tarball(bytes: &[u8], dest: &std::path::Path) -> std::io::Result<()> {
    let decoder = flate2::read::GzDecoder::new(Cursor::new(bytes));
    let mut archive = tar::Archive::new(decoder);
    archive.unpack(dest)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn build_gzip_tarball(entries: &[(&str, &[u8])]) -> Vec<u8> {
        use std::io::Write;
        let mut builder = tar::Builder::new(Vec::new());
        for (name, body) in entries {
            let mut header = tar::Header::new_gnu();
            header.set_size(body.len() as u64);
            header.set_mode(0o644);
            header.set_cksum();
            builder
                .append_data(&mut header, name, std::io::Cursor::new(*body))
                .expect("append");
        }
        builder.finish().expect("finish");
        let raw = builder.into_inner().expect("inner");

        let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        encoder.write_all(&raw).expect("write");
        encoder.finish().expect("finish")
    }

    #[test]
    fn extract_gzip_tarball_roundtrip() {
        let dir = std::env::temp_dir().join(format!(
            "buffup-extract-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        std::fs::create_dir_all(&dir).expect("mkdir");

        let bytes = build_gzip_tarball(&[("buff", b"#!/bin/sh\necho hi\n")]);
        extract_gzip_tarball(&bytes, &dir).expect("extract");

        let buff = dir.join("buff");
        assert!(buff.exists(), "buff file should exist after extract");
        let body = std::fs::read(&buff).expect("read");
        assert_eq!(body, b"#!/bin/sh\necho hi\n");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn extract_gzip_tarball_rejects_plain_tar() {
        // A non-gzipped tar should fail when wrapped in GzDecoder.
        let mut builder = tar::Builder::new(Vec::new());
        let mut header = tar::Header::new_gnu();
        let body = b"x";
        header.set_size(body.len() as u64);
        header.set_mode(0o644);
        header.set_cksum();
        builder
            .append_data(&mut header, "buff", std::io::Cursor::new(body))
            .expect("append");
        builder.finish().expect("finish");
        let raw = builder.into_inner().expect("inner");

        let dir = std::env::temp_dir().join(format!(
            "buffup-badgz-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        std::fs::create_dir_all(&dir).expect("mkdir");
        let res = extract_gzip_tarball(&raw, &dir);
        assert!(res.is_err(), "plain tar must fail gzip decode");
        let _ = std::fs::remove_dir_all(&dir);
    }
}

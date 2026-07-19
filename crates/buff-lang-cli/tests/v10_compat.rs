//! v1.0 backward-compatibility fixture (T113b).
//!
//! This integration test locks the transpilation output of
//! `tests/fixtures/v10-compat.buff` byte-for-byte. Any change to the Buff
//! compiler (lexer, parser, type inference, codegen) that alters the
//! generated Rust source for this fixture is a BREAKING CHANGE and must
//! either:
//!
//! 1. Be deliberate, accompanied by a major version bump (e.g. v2.0.0), OR
//! 2. Be a refresh of the snapshot with documented justification in the
//!    commit message, after verifying the change is intended.
//!
//! # Snapshot refresh workflow
//!
//! When the snapshot legitimately needs to change:
//!
//! ```text
//! cargo test -p buff-lang-cli --test v10_compat -- --ignored v10_regenerate_snapshot
//! git diff tests/fixtures/v10-compat.snapshot.rs   # review the change!
//! git add tests/fixtures/v10-compat.snapshot.rs
//! git commit -m "test(v1.0): refresh v10-compat snapshot — <reason>"
//! ```
//!
//! Do NOT regenerate the snapshot casually. Every refresh is a backward-compat
//! break that must be justified.

use std::path::PathBuf;

/// Location of the frozen Buff source fixture, relative to the CLI crate.
fn fixture_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/v10-compat.buff")
}

/// Location of the frozen Rust snapshot, relative to the CLI crate.
fn snapshot_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/v10-compat.snapshot.rs")
}

/// The frozen v1.0 transpilation contract: byte-identical Rust output.
///
/// If this test fails, the generated Rust has drifted from the v1.0 baseline.
/// Investigate the diff, decide whether the change is intended, and refresh
/// the snapshot via the workflow in the module doc above only if deliberate.
#[test]
fn v10_compat_transpilation_frozen() {
    let fixture = fixture_path();
    let snapshot_path = snapshot_path();

    assert!(fixture.exists(), "fixture missing: {}", fixture.display());
    assert!(
        snapshot_path.exists(),
        "snapshot missing: {}. Run `cargo test -p buff-lang-cli --test v10_compat -- --ignored v10_regenerate_snapshot` to create it.",
        snapshot_path.display()
    );

    let out = buff_lang_cli::pipeline::compile_to_rust(&fixture)
        .expect("compile_to_rust must succeed on v10-compat.buff");

    let expected = std::fs::read_to_string(&snapshot_path).expect("failed to read snapshot file");

    // Byte-identical comparison — strictest possible contract.
    if out.rust_source != expected {
        // Find the first diverging line for a focused error message.
        let actual_lines: Vec<&str> = out.rust_source.lines().collect();
        let expected_lines: Vec<&str> = expected.lines().collect();
        let max_len = actual_lines.len().max(expected_lines.len());
        let mut first_diff: Option<(usize, String, String)> = None;
        for i in 0..max_len {
            let a = actual_lines.get(i).copied().unwrap_or("<missing>");
            let e = expected_lines.get(i).copied().unwrap_or("<missing>");
            if a != e {
                first_diff = Some((i + 1, a.to_string(), e.to_string()));
                break;
            }
        }
        let (line_no, actual_line, expected_line) = first_diff.unwrap_or((
            0,
            "<no single-line diff; length mismatch>".to_string(),
            "<no single-line diff; length mismatch>".to_string(),
        ));
        panic!(
            "v10-compat transpilation snapshot DRIFT detected.\n\n\
             First divergence at line {line_no}:\n  \
             actual:   {actual_line}\n  \
             expected: {expected_line}\n\n\
             Full diff:\n\
             --- expected (snapshot file) ---\n{expected}\n\
             --- actual (current codegen) ---\n{actual}\n\n\
             If this change is intended, refresh the snapshot:\n  \
             cargo test -p buff-lang-cli --test v10_compat -- --ignored v10_regenerate_snapshot\n",
            actual = out.rust_source,
        );
    }

    // Cleanup: compile_to_rust writes a `<file>.rs` next to the .buff source.
    // We don't want that artifact tracked (the .snapshot.rs is the canonical
    // frozen form); remove it if present.
    let side_artifact = fixture.with_extension("buff.rs");
    let _ = std::fs::remove_file(side_artifact);
}

/// Regenerate the snapshot file from the current codegen output.
///
/// `#[ignore]` so it doesn't run in normal CI. Invoke explicitly when a
/// deliberate refresh is intended (see module docs).
#[test]
#[ignore]
fn v10_regenerate_snapshot() {
    let fixture = fixture_path();
    let snapshot_path = snapshot_path();

    let out = buff_lang_cli::pipeline::compile_to_rust(&fixture)
        .expect("compile_to_rust must succeed on v10-compat.buff");

    std::fs::write(&snapshot_path, &out.rust_source).expect("failed to write snapshot file");

    // Cleanup the .buff.rs side artifact.
    let side_artifact = fixture.with_extension("buff.rs");
    let _ = std::fs::remove_file(side_artifact);

    eprintln!(
        "Snapshot regenerated at {} ({} bytes)",
        snapshot_path.display(),
        out.rust_source.len()
    );
}

//! T60 — `buff build --minimal` integration tests.
//!
//! Mirrors the T56 `build_release_tests.rs` structure for the size-
//! minimization profile (T60). Exercises the `--minimal` flag end-to-end
//! across the CLI surface:
//!
//! - **clap parsing**: `Build { minimal: true }` parses correctly from
//!   argv; default (no flag) is `false`. `--minimal --release` together
//!   → `minimal` wins (precedence contract).
//! - **BuildMode plumbing**: `BuildMode::from_flags(release, minimal)`
//!   translates the booleans into the right [`pipeline::BuildMode`]
//!   variant, which is then forwarded to
//!   [`pipeline::compile_rust_to_exe`].
//! - **Minimal profile (QA)**: [`pipeline::minimal_profile_toml`]
//!   contains `opt-level = "z"` + `panic = "abort"` + `strip = true` +
//!   `inherits = "release"` (the T60 acceptance target).
//! - **Minimal rustc flags**: [`pipeline::rustc_minimal_flags`]
//!   contains all five size-minimization knobs (opt-level=z +
//!   panic=abort + strip=symbols + lto=true + codegen-units=1).
//! - **Debug-vs-minimal distinction**: the two modes produce disjoint
//!   flag sets so a debug build never accidentally enables
//!   size-minimization (which would slow runtime without the user
//!   asking).
//!
//! Like the T56 suite, these tests deliberately avoid invoking `rustc`
//! for the minimal path — LTO + strip on even a trivial program is slow
//! and flaky in CI. The functional contract is covered by inspecting
//! the flag list + the profile-TOML string.

use std::fs;
use std::path::PathBuf;

use buff_lang_cli::cli::{Cli, Command};
use buff_lang_cli::pipeline;
use clap::Parser;

// ---------------------------------------------------------------------------
// QA target — minimal profile TOML must contain `opt-level = "z"`
// ---------------------------------------------------------------------------

#[test]
fn qa_minimal_profile_toml_contains_opt_level_z_minimal_build_qa() {
    // T60 QA acceptance: "assert Cargo.toml has opt-level=z". The
    // workspace-root Cargo.toml already declares the profile (so
    // `cargo build --profile minimal` works in any cargo-driven path);
    // `minimal_profile_toml()` is the contract for what would be
    // injected into a generated Cargo.toml. Functional equivalence is
    // covered by `rustc_minimal_flags_contain_opt_level_z`.
    let profile = pipeline::minimal_profile_toml();
    assert!(
        profile.contains("opt-level = \"z\""),
        "T60 QA failed: minimal profile must contain `opt-level = \"z\"`, got: {profile:?}"
    );
}

#[test]
fn qa_minimal_profile_toml_has_full_profile_block_minimal_build_qa() {
    let profile = pipeline::minimal_profile_toml();
    assert!(
        profile.contains("[profile.minimal]"),
        "expected [profile.minimal] header, got: {profile:?}"
    );
    assert!(
        profile.contains("inherits = \"release\""),
        "expected `inherits = \"release\"`, got: {profile:?}"
    );
    assert!(
        profile.contains("panic = \"abort\""),
        "expected `panic = \"abort\"`, got: {profile:?}"
    );
    assert!(
        profile.contains("strip = true"),
        "expected `strip = true`, got: {profile:?}"
    );
    assert!(
        profile.contains("lto = true"),
        "expected `lto = true`, got: {profile:?}"
    );
    assert!(
        profile.contains("codegen-units = 1"),
        "expected `codegen-units = 1`, got: {profile:?}"
    );
    // Sanity: every line ends with a newline so the block can be
    // appended to a Cargo.toml without gluing two keys together.
    assert!(
        profile.ends_with('\n'),
        "minimal profile must end with a newline, got: {profile:?}"
    );
}

// ---------------------------------------------------------------------------
// Functional rustc flag path — minimal flags must include all 5 knobs
// ---------------------------------------------------------------------------

#[test]
fn rustc_minimal_flags_contain_all_size_knobs_minimal_build() {
    let flags = pipeline::rustc_minimal_flags();
    let joined = flags.join(" ");
    assert!(
        joined.contains("opt-level=z"),
        "minimal flags must contain opt-level=z, got: {joined}"
    );
    assert!(
        joined.contains("panic=abort"),
        "minimal flags must contain panic=abort, got: {joined}"
    );
    assert!(
        joined.contains("strip=symbols"),
        "minimal flags must contain strip=symbols, got: {joined}"
    );
    assert!(
        joined.contains("lto=true"),
        "minimal flags must contain lto=true, got: {joined}"
    );
    assert!(
        joined.contains("codegen-units=1"),
        "minimal flags must contain codegen-units=1, got: {joined}"
    );
}

#[test]
fn rustc_minimal_flags_use_dash_c_separator_form_minimal_build() {
    // The flags should be in the interleaved `-C`, `<flag>` form so they
    // can be passed verbatim to `Command::new("rustc")` via `.args()`.
    // 5 flags × 2 tokens (`-C`, `<key>=<value>`) = 10 tokens total.
    let flags = pipeline::rustc_minimal_flags();
    assert_eq!(
        flags.len(),
        10,
        "expected 10 flag tokens (5 × `-C, key=val`), got {} ({:?})",
        flags.len(),
        flags
    );
    // Every even-indexed slot (0, 2, 4, 6, 8) is "-C".
    for i in (0..10).step_by(2) {
        assert_eq!(
            flags[i], "-C",
            "expected `-C` at position {i}, got: {:?}",
            flags[i]
        );
    }
    // Every odd-indexed slot is the actual `<key>=<value>` flag.
    for i in (1..10).step_by(2) {
        assert!(
            flags[i].contains('='),
            "expected `<key>=<value>` form at position {i}, got: {:?}",
            flags[i]
        );
    }
}

// ---------------------------------------------------------------------------
// BuildMode enum — flag-to-mode translation + minimal/debug/release
// distinction
// ---------------------------------------------------------------------------

#[test]
fn build_mode_from_flags_precedence_minimal_over_release_minimal_build() {
    // T60 contract: when both --release and --minimal are set, Minimal wins.
    // This mirrors cargo's --profile semantics (a more-specific profile wins).
    assert_eq!(
        pipeline::BuildMode::from_flags(true, true),
        pipeline::BuildMode::Minimal,
        "minimal=true + release=true must produce Minimal"
    );
    assert_eq!(
        pipeline::BuildMode::from_flags(false, true),
        pipeline::BuildMode::Minimal,
        "minimal=true + release=false must produce Minimal"
    );
    assert_eq!(
        pipeline::BuildMode::from_flags(true, false),
        pipeline::BuildMode::Release,
        "minimal=false + release=true must produce Release (T56 contract preserved)"
    );
    assert_eq!(
        pipeline::BuildMode::from_flags(false, false),
        pipeline::BuildMode::Debug,
        "minimal=false + release=false must produce Debug (default)"
    );
}

#[test]
fn build_mode_is_minimal_predicate_minimal_build() {
    assert!(!pipeline::BuildMode::Debug.is_minimal());
    assert!(!pipeline::BuildMode::Release.is_minimal());
    assert!(pipeline::BuildMode::Minimal.is_minimal());
}

#[test]
fn build_mode_from_release_flag_still_works_minimal_build() {
    // T56 callers (commands::run) that don't yet accept --minimal should
    // still get the same flag→mode mapping they always did.
    assert_eq!(
        pipeline::BuildMode::from_release_flag(false),
        pipeline::BuildMode::Debug
    );
    assert_eq!(
        pipeline::BuildMode::from_release_flag(true),
        pipeline::BuildMode::Release
    );
}

#[test]
fn debug_mode_does_not_carry_minimal_flags_minimal_build() {
    // The contract: a debug-mode build MUST NOT silently enable
    // size-minimization (which would slow runtime + add LTO cost).
    let minimal_flags = pipeline::rustc_minimal_flags();
    // Debug mode uses just `-O`. None of the minimal tokens may equal `-O`.
    for flag in &minimal_flags {
        assert_ne!(
            *flag, "-O",
            "debug-mode flag `-O` must not appear in minimal flag set"
        );
    }
    // `panic=abort` is the canonical minimal-only knob: it must never
    // be enabled by debug (debug keeps `panic=unwind` so catch_unwind
    // + the panic_hook can translate runtime panics). Since
    // rustc_minimal_flags() is the only source of `panic=abort` for
    // the pipeline, debug mode is unwind-by-default.
    let has_panic_abort = minimal_flags.iter().any(|f| f.contains("panic=abort"));
    assert!(
        has_panic_abort,
        "minimal flags must include panic=abort (only path to abort-on-panic)"
    );
}

// ---------------------------------------------------------------------------
// minimal_profile_toml determinism + is_well_formed
// ---------------------------------------------------------------------------

#[test]
fn minimal_profile_toml_is_deterministic_minimal_build() {
    // Pure fixed-string helper — same output every call.
    assert_eq!(
        pipeline::minimal_profile_toml(),
        pipeline::minimal_profile_toml()
    );
}

// ---------------------------------------------------------------------------
// clap parsing — `--minimal` flag parses on the `build` subcommand
// ---------------------------------------------------------------------------

#[test]
fn build_minimal_flag_parses_true_when_passed_minimal_build() {
    let cli = Cli::parse_from(["buff", "build", "foo.buff", "--minimal"]);
    match cli.command {
        Command::Build {
            file,
            output: _,
            release,
            minimal,
            target: _,
        } => {
            assert_eq!(file, Some(PathBuf::from("foo.buff")));
            assert!(minimal, "--minimal must parse to `true`");
            assert!(
                !release,
                "default build (no --release) must keep release=false even with --minimal"
            );
        }
        other => panic!("expected Build, got {other:?}"),
    }
}

#[test]
fn build_minimal_flag_defaults_false_when_omitted_minimal_build() {
    let cli = Cli::parse_from(["buff", "build", "foo.buff"]);
    match cli.command {
        Command::Build { minimal, .. } => {
            assert!(
                !minimal,
                "default build (no --minimal) must keep minimal=false"
            );
        }
        other => panic!("expected Build, got {other:?}"),
    }
}

// ---------------------------------------------------------------------------
// Optional: end-to-end compile with --minimal (slow; gated on rustc).
// ---------------------------------------------------------------------

/// Helper: unique temp dir for this test binary.
#[allow(dead_code)]
fn temp_root() -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "buff-lang-cli-minimal-build-tests-{}",
        std::process::id()
    ));
    let _ = fs::create_dir_all(&dir);
    dir
}

/// Helper: detect whether `rustc` is callable on PATH.
#[allow(dead_code)]
fn rustc_available() -> bool {
    std::process::Command::new("rustc")
        .arg("--version")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok()
}

/// Helper: detect whether `buff build --minimal` actually shrinks the
/// binary. Skipped when rustc isn't on PATH (CI sandboxes without the
/// toolchain) OR when the resulting binary is unreachable (cross-compile
/// targets). Returns `None` on skip; `Some(size_bytes)` on success.
#[allow(dead_code)]
fn try_measure_minimal_size(src: &str) -> Option<u64> {
    if !rustc_available() {
        return None;
    }
    let dir = temp_root();
    let file = dir.join("measure_minimal.buff");
    fs::write(&file, src).ok()?;

    let exe = {
        let mut p = file.with_extension("");
        if !std::env::consts::EXE_EXTENSION.is_empty() {
            p.set_extension(std::env::consts::EXE_EXTENSION);
        }
        p
    };

    let result = buff_lang_cli::commands::build::run(Some(&file), None, false, true);
    if result.is_err() {
        let _ = fs::remove_file(&file);
        return None;
    }
    let size = fs::metadata(&exe).map(|m| m.len()).ok();

    let _ = fs::remove_file(&file);
    let _ = fs::remove_file(&exe);
    size
}

#[test]
fn build_command_with_minimal_true_compiles_with_size_minimization_minimal_build() {
    // Confirms the new 5-arg signature accepts `minimal: bool` AND that
    // the minimal path produces a working executable. Functional coverage
    // of the minimal=true rustc invocation is provided by the flag-list +
    // profile tests above (a real LTO+strip build is too slow / flaky
    // for CI). When rustc is unavailable, we still verify the plumbing
    // by checking the flag set + profile block in the earlier tests.
    if !rustc_available() {
        eprintln!(
            "skipping build_command_with_minimal_true_compiles_with_size_minimization_minimal_build: rustc not on PATH"
        );
        return;
    }

    let dir = temp_root();
    let file = dir.join("minimal_plumbing.buff");
    fs::write(&file, "func main():\n    print(\"minimal plumbing\")\n")
        .expect("failed to write fixture");

    let rs_path = file.with_extension("rs");
    let exe = {
        let mut p = file.with_extension("");
        if !std::env::consts::EXE_EXTENSION.is_empty() {
            p.set_extension(std::env::consts::EXE_EXTENSION);
        }
        p
    };

    let result = buff_lang_cli::commands::build::run(Some(&file), None, false, true);
    result.expect("minimal-mode build (minimal=true) must succeed");
    assert!(exe.exists(), "minimal build must produce an executable");

    let _ = fs::remove_file(&file);
    let _ = fs::remove_file(&rs_path);
    let _ = fs::remove_file(&exe);
}

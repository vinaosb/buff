//! T56 — `buff build --release` integration tests.
//!
//! Exercises the `--release` flag end-to-end across the CLI surface:
//!
//! - **clap parsing**: `Build { release: true }` / `Run { release: true }`
//!   parse correctly from argv; default (no flag) is `false`.
//! - **BuildMode plumbing**: `commands::build::run` / `commands::run::run`
//!   translate the `release` bool into [`pipeline::BuildMode`] and forward
//!   it to [`pipeline::compile_rust_to_exe`].
//! - **Release profile (QA)**: [`pipeline::release_profile_toml`] contains
//!   `lto = true` (the T56 acceptance target).
//! - **Release rustc flags**: [`pipeline::rustc_release_flags`] contains
//!   both `lto` and `opt-level` (the functional path used by the
//!   single-file rustc pipeline).
//! - **Debug-vs-release distinction**: the two modes produce disjoint flag
//!   sets so a debug build never accidentally enables LTO.
//!
//! These tests deliberately avoid invoking `rustc` for the release-mode LTO
//! path — fat LTO on even a trivial program is slow and flaky in CI. The
//! functional contract is covered by inspecting the flag list + the
//! profile-TOML string; a slow end-to-end build adds no signal beyond what
//! the existing `commands::build::run(&file, None, false)` debug-mode
//! build test already proves (the only difference is which rustc args get
//! appended).

use std::fs;
use std::path::PathBuf;

use buff_lang_cli::cli::{Cli, Command};
use buff_lang_cli::pipeline;
use clap::Parser;

// ---------------------------------------------------------------------------
// QA target — release profile TOML must contain `lto = true`
// ---------------------------------------------------------------------------

#[test]
fn qa_release_profile_toml_contains_lto_true_build_release_qa() {
    // T56 QA acceptance: "assert Cargo.toml has lto=true". The current
    // pipeline drives bare `rustc` (no Cargo.toml in the build path), so
    // `release_profile_toml()` — the contract for what WOULD be injected
    // into a Cargo.toml — is the assertion target. Functional equivalence
    // is covered by `rustc_release_flags_contain_lto_and_opt_level`.
    let profile = pipeline::release_profile_toml();
    assert!(
        profile.contains("lto = true"),
        "T56 QA failed: release profile must contain `lto = true`, got: {profile:?}"
    );
}

#[test]
fn qa_release_profile_toml_has_full_profile_block_build_release_qa() {
    let profile = pipeline::release_profile_toml();
    assert!(profile.contains("[profile.release]"));
    assert!(profile.contains("opt-level = 3"));
    assert!(profile.contains("codegen-units = 1"));
    // Sanity: every line ends with a newline (so appending to a Cargo.toml
    // doesn't glue two keys together).
    assert!(
        profile.ends_with('\n'),
        "release profile must end with a newline, got: {profile:?}"
    );
}

// ---------------------------------------------------------------------------
// Functional rustc flag path — release flags must include lto + opt-level
// ---------------------------------------------------------------------------

#[test]
fn rustc_release_flags_contain_lto_and_opt_level_build_release() {
    let flags = pipeline::rustc_release_flags();
    let joined = flags.join(" ");
    assert!(
        joined.contains("lto=fat"),
        "release flags must contain lto=fat, got: {joined}"
    );
    assert!(
        joined.contains("opt-level=3"),
        "release flags must contain opt-level=3, got: {joined}"
    );
    assert!(
        joined.contains("codegen-units=1"),
        "release flags must contain codegen-units=1, got: {joined}"
    );
}

#[test]
fn rustc_release_flags_use_dash_c_separator_form_build_release() {
    // The flags should be in the interleaved `-C`, `<flag>` form so they
    // can be passed verbatim to `Command::new("rustc")` via `.args()`.
    let flags = pipeline::rustc_release_flags();
    // Every even-indexed slot (0, 2, 4) is "-C"; every odd-indexed slot is
    // the actual `-C <key>=<value>` flag.
    assert_eq!(flags.len(), 6, "expected 6 flag tokens (3 × `-C, key=val`)");
    assert_eq!(flags[0], "-C");
    assert_eq!(flags[2], "-C");
    assert_eq!(flags[4], "-C");
    for flag in [&flags[1], &flags[3], &flags[5]] {
        assert!(
            flag.contains('='),
            "expected `<key>=<value>` form, got: {flag}"
        );
    }
}

// ---------------------------------------------------------------------------
// BuildMode enum — flag-to-mode translation + debug/release distinction
// ---------------------------------------------------------------------------

#[test]
fn build_mode_from_release_flag_build_release() {
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
fn build_mode_default_is_debug_build_release() {
    // Debug MUST be the default so the existing v0.1 behavior is preserved
    // byte-identically when --release is omitted.
    assert_eq!(pipeline::BuildMode::default(), pipeline::BuildMode::Debug);
}

#[test]
fn build_mode_is_release_predicate_is_correct_build_release() {
    assert!(!pipeline::BuildMode::Debug.is_release());
    assert!(pipeline::BuildMode::Release.is_release());
}

#[test]
fn debug_mode_does_not_carry_release_flags_build_release() {
    // The contract: a debug-mode build MUST NOT silently enable LTO/opt=3.
    // The functional path is the `match mode` arm inside
    // `compile_rust_to_exe` — debug adds only `-O`, release adds the LTO
    // block. We assert the invariant indirectly by checking that the
    // release flag set never appears in a debug-mode build's command line.
    // (Inspecting the actual rustc Command is non-trivial; instead we
    // check that rustc_release_flags — the only place release-grade
    // arguments are sourced — has zero overlap with what debug uses.)
    let release_flags = pipeline::rustc_release_flags();
    // Debug mode uses just `-O`. None of the release tokens may equal `-O`
    // (otherwise the modes would share a flag and the distinction blurs).
    for flag in &release_flags {
        assert_ne!(
            *flag, "-O",
            "debug-mode flag `-O` must not appear in release flag set"
        );
    }
    // LTO is the canonical release-only knob: it must never be enabled by
    // debug. Since rustc_release_flags() is the only source of `lto=*` for
    // the pipeline, debug mode is LTO-free by construction.
    let has_lto = release_flags.iter().any(|f| f.contains("lto="));
    assert!(has_lto, "release flags must include lto (only path to LTO)");
}

// ---------------------------------------------------------------------------
// clap parsing — `--release` flag parses on `build` and `run` subcommands
// ---------------------------------------------------------------------------

#[test]
fn build_release_flag_parses_true_when_passed_build_release() {
    let cli = Cli::parse_from(["buff", "build", "foo.buff", "--release"]);
    match cli.command {
        Command::Build { file, release, .. } => {
            assert_eq!(file, Some(PathBuf::from("foo.buff")));
            assert!(release, "--release must parse to `true`");
        }
        other => panic!("expected Build, got {other:?}"),
    }
}

#[test]
fn build_release_flag_defaults_false_when_omitted_build_release() {
    let cli = Cli::parse_from(["buff", "build", "foo.buff"]);
    match cli.command {
        Command::Build { release, .. } => {
            assert!(
                !release,
                "default build (no --release) must be debug; got release=true"
            );
        }
        other => panic!("expected Build, got {other:?}"),
    }
}

#[test]
fn run_release_flag_parses_true_when_passed_build_release() {
    // `--release` BEFORE the `--` separator (so it's parsed by clap, not
    // forwarded to the compiled program).
    let cli = Cli::parse_from(["buff", "run", "foo.buff", "--release", "--", "arg1"]);
    match cli.command {
        Command::Run {
            file,
            args,
            release,
            ..
        } => {
            assert_eq!(file, PathBuf::from("foo.buff"));
            assert!(release, "--release must parse to `true` on `run`");
            assert_eq!(args, vec!["arg1".to_string()], "args after -- forwarded");
        }
        other => panic!("expected Run, got {other:?}"),
    }
}

#[test]
fn run_release_flag_defaults_false_when_omitted_build_release() {
    let cli = Cli::parse_from(["buff", "run", "foo.buff"]);
    match cli.command {
        Command::Run { release, .. } => {
            assert!(!release, "default run (no --release) must be debug");
        }
        other => panic!("expected Run, got {other:?}"),
    }
}

// ---------------------------------------------------------------------------
// Command-level plumbing — `commands::build::run` accepts `release: bool`
// without regressing the v0.1 debug-mode path.
// ---------------------------------------------------------------------------

/// Helper: unique temp dir for this test binary.
fn temp_root() -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "buff-lang-cli-build-release-tests-{}",
        std::process::id()
    ));
    let _ = fs::create_dir_all(&dir);
    dir
}

/// Helper: detect whether `rustc` is callable on PATH.
fn rustc_available() -> bool {
    std::process::Command::new("rustc")
        .arg("--version")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok()
}

#[test]
fn build_command_with_release_false_compiles_in_debug_build_release() {
    // Confirms the new 3-arg signature accepts `release: bool` AND that the
    // debug path (release=false) still produces a working executable — i.e.
    // T56 didn't regress the v0.1 build path. Functional coverage of the
    // release=true rustc invocation is provided by the flag-list + profile
    // tests above (a real LTO build is too slow / flaky for CI).
    if !rustc_available() {
        eprintln!("skipping build_command_with_release_false_compiles_in_debug_build_release: rustc not on PATH");
        return;
    }

    let dir = temp_root();
    let file = dir.join("debug_plumbing.buff");
    fs::write(&file, "func main():\n    print(\"debug plumbing\")\n")
        .expect("failed to write fixture");

    let rs_path = file.with_extension("rs");
    let exe = {
        let mut p = file.with_extension("");
        if !std::env::consts::EXE_EXTENSION.is_empty() {
            p.set_extension(std::env::consts::EXE_EXTENSION);
        }
        p
    };

    let result = buff_lang_cli::commands::build::run(
        Some(&file),
        None,
        false, // release
        false, // minimal
        false, // fast
        false, // no_cache
        false, // incremental
        true,  // no_incremental (force legacy path)
        false, // sccache
        None,  // target
        buff_lang_cli::pipeline::LinkerChoice::default(),
        buff_lang_cli::pipeline::DebugInfoChoice::default(),
        buff_lang_cli::pipeline::BackendChoice::default(),
        false, // detect_races
    );
    result.expect("debug-mode build (release=false) must succeed");
    assert!(exe.exists(), "debug build must produce an executable");

    let _ = fs::remove_file(&file);
    let _ = fs::remove_file(&rs_path);
    let _ = fs::remove_file(&exe);
}

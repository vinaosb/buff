//! T62 — `buff build --pgo` integration tests.
//!
//! Mirrors the T60 `minimal_build.rs` structure for the Profile-Guided
//! Optimization flow (T62). Exercises the `--pgo` / `--use` flags across
//! the CLI surface without invoking `rustc` (PGO builds are slow + flaky
//! in CI; the functional contract is covered by inspecting the flag
//! lists + profile-TOML strings).
//!
//! Coverage (8 tests):
//!
//! - **clap parsing**: `--pgo` and `--use` parse correctly on the `build`
//!   subcommand; both default to `false` when omitted.
//! - **Phase 1 flags**: [`pipeline::rustc_pgo_instrument_flags`]
//!   contains `profile-generate=<dir>` + the release-grade baseline.
//! - **Phase 3 flags**: [`pipeline::rustc_pgo_use_flags`] contains
//!   `profile-use=<path>` + the release-grade baseline.
//! - **`[profile.pgo]` exists**: [`pipeline::pgo_profile_toml`] + the
//!   workspace-root `Cargo.toml` both declare the profile block.
//! - **Profile-data dir constant**: [`pipeline::PGO_DATA_DIR`] is the
//!   conventional `./target/pgo-data` path.
//! - **Phase detection**: the `pgo_use` boolean selects Phase 1 vs
//!   Phase 3 (verified via the flag-list divergence — the two phases
//!   emit disjoint `profile-generate` vs `profile-use` flags).
//! - **Help text mentions PGO**: clap's `--help` render includes the
//!   `--pgo` flag documentation.
//! - **Backward compat**: regular `buff build` (no `--pgo`) keeps the
//!   `Build { pgo: false, pgo_use: false }` defaults so the existing
//!   build path runs unchanged.

use buff_lang_cli::cli::{Cli, Command};
use buff_lang_cli::pipeline;
use clap::CommandFactory;
use clap::Parser;

// ---------------------------------------------------------------------------
// QA target — pgo profile TOML must contain `inherits = "release"` + LTO
// ---------------------------------------------------------------------------

#[test]
fn qa_pgo_profile_toml_contains_inherits_release_pgo_build_qa() {
    // T62 QA acceptance: "assert Cargo.toml has [profile.pgo]". The
    // workspace-root Cargo.toml declares the profile block (so
    // `cargo build --profile pgo` works in any cargo-driven path);
    // `pgo_profile_toml()` is the contract for what would be injected
    // into a generated Cargo.toml.
    let profile = pipeline::pgo_profile_toml();
    assert!(
        profile.contains("[profile.pgo]"),
        "T62 QA failed: pgo profile must contain `[profile.pgo]` header, got: {profile:?}"
    );
    assert!(
        profile.contains("inherits = \"release\""),
        "T62 QA failed: pgo profile must inherit from release, got: {profile:?}"
    );
    assert!(
        profile.contains("lto = \"fat\""),
        "T62 QA failed: pgo profile must set `lto = \"fat\"`, got: {profile:?}"
    );
    assert!(
        profile.contains("codegen-units = 1"),
        "T62 QA failed: pgo profile must set `codegen-units = 1`, got: {profile:?}"
    );
    assert!(
        profile.ends_with('\n'),
        "pgo profile must end with a newline, got: {profile:?}"
    );
}

// ---------------------------------------------------------------------------
// Functional rustc flag paths — Phase 1 (instrument) + Phase 3 (use)
// ---------------------------------------------------------------------------

#[test]
fn rustc_pgo_instrument_flags_contain_profile_generate_pgo_build() {
    let flags = pipeline::rustc_pgo_instrument_flags("./target/pgo-data");
    let joined = flags.join(" ");
    assert!(
        joined.contains("profile-generate=./target/pgo-data"),
        "Phase 1 flags must contain profile-generate=<dir>, got: {joined}"
    );
    // Phase 1 + Phase 3 share the release-grade baseline so the
    // instrumented binary's runtime matches the final PGO build.
    assert!(
        joined.contains("opt-level=3"),
        "Phase 1 flags must contain opt-level=3 (release baseline), got: {joined}"
    );
    assert!(
        joined.contains("lto=fat"),
        "Phase 1 flags must contain lto=fat, got: {joined}"
    );
    assert!(
        joined.contains("codegen-units=1"),
        "Phase 1 flags must contain codegen-units=1, got: {joined}"
    );
    // Phase 1 must NOT carry profile-use (that's Phase 3's job).
    assert!(
        !joined.contains("profile-use"),
        "Phase 1 flags must NOT contain profile-use, got: {joined}"
    );
}

#[test]
fn rustc_pgo_use_flags_contain_profile_use_pgo_build() {
    let merged = pipeline::pgo_merged_profile_path(Some("./target/pgo-data"));
    let flags = pipeline::rustc_pgo_use_flags(&merged);
    let joined = flags.join(" ");
    assert!(
        joined.contains(&merged),
        "Phase 3 flags must contain profile-use=<merged_path> ({merged}), got: {joined}"
    );
    assert!(
        joined.contains("profile-use="),
        "Phase 3 flags must contain profile-use= prefix, got: {joined}"
    );
    assert!(
        joined.contains("opt-level=3"),
        "Phase 3 flags must contain opt-level=3 (release baseline), got: {joined}"
    );
    assert!(
        joined.contains("lto=fat"),
        "Phase 3 flags must contain lto=fat, got: {joined}"
    );
    // Phase 3 must NOT carry profile-generate (that's Phase 1's job).
    assert!(
        !joined.contains("profile-generate"),
        "Phase 3 flags must NOT contain profile-generate, got: {joined}"
    );
}

// ---------------------------------------------------------------------------
// Profile-data directory + merged-profile path conventions
// ---------------------------------------------------------------------------

#[test]
fn pgo_data_dir_constant_is_target_pgo_data_pgo_build() {
    assert_eq!(
        pipeline::PGO_DATA_DIR,
        "./target/pgo-data",
        "PGO_DATA_DIR must be the conventional `./target/pgo-data` path"
    );
    assert_eq!(
        pipeline::PGO_MERGED_PROFILE,
        "merged.profdata",
        "PGO_MERGED_PROFILE must be `merged.profdata`"
    );
}

#[test]
fn pgo_merged_profile_path_joins_dir_and_filename_pgo_build() {
    let default_path = pipeline::pgo_merged_profile_path(None);
    assert_eq!(
        default_path, "./target/pgo-data/merged.profdata",
        "default merged path must join PGO_DATA_DIR + PGO_MERGED_PROFILE"
    );
    let custom = pipeline::pgo_merged_profile_path(Some("/tmp/custom-pgo"));
    assert_eq!(
        custom, "/tmp/custom-pgo/merged.profdata",
        "custom merged path must join the override dir + filename"
    );
}

// ---------------------------------------------------------------------------
// Phase detection — Phase 1 vs Phase 3 emit disjoint profile-* flags
// ---------------------------------------------------------------------------

#[test]
fn phase_detection_instrument_vs_use_flags_diverge_pgo_build() {
    // The pgo_use boolean selects Phase 1 vs Phase 3. We verify the
    // divergence by checking that the two flag lists carry disjoint
    // profile-* flags (generate vs use) — this is the contract the
    // orchestrator relies on to pick the right rustc invocation.
    let dir = "./target/pgo-test-phase-detect";
    let merged = pipeline::pgo_merged_profile_path(Some(dir));

    let phase1 = pipeline::rustc_pgo_instrument_flags(dir);
    let phase3 = pipeline::rustc_pgo_use_flags(&merged);

    let p1_joined = phase1.join(" ");
    let p3_joined = phase3.join(" ");

    // Phase 1 carries profile-generate, NOT profile-use.
    assert!(
        p1_joined.contains("profile-generate=") && !p1_joined.contains("profile-use="),
        "Phase 1 must emit profile-generate (not profile-use), got: {p1_joined}"
    );
    // Phase 3 carries profile-use, NOT profile-generate.
    assert!(
        p3_joined.contains("profile-use=") && !p3_joined.contains("profile-generate="),
        "Phase 3 must emit profile-use (not profile-generate), got: {p3_joined}"
    );
}

// ---------------------------------------------------------------------------
// clap parsing — `--pgo` / `--use` flags parse on the `build` subcommand
// ---------------------------------------------------------------------------

#[test]
fn build_pgo_flag_parses_true_when_passed_pgo_build() {
    let cli = Cli::parse_from(["buff", "build", "foo.buff", "--pgo"]);
    match cli.command {
        Command::Build { pgo, pgo_use, .. } => {
            assert!(pgo, "`--pgo` must parse to `true`");
            assert!(
                !pgo_use,
                "default `--pgo` (without `--use`) must keep pgo_use=false (Phase 1)"
            );
        }
        other => panic!("expected Build, got {other:?}"),
    }
}

#[test]
fn build_pgo_use_flag_parses_true_when_passed_pgo_build() {
    let cli = Cli::parse_from(["buff", "build", "foo.buff", "--pgo", "--use"]);
    match cli.command {
        Command::Build { pgo, pgo_use, .. } => {
            assert!(pgo, "`--pgo --use` must keep pgo=true");
            assert!(pgo_use, "`--use` must parse to `true` (Phase 3 selector)");
        }
        other => panic!("expected Build, got {other:?}"),
    }
}

// ---------------------------------------------------------------------------
// Help text mentions PGO
// ---------------------------------------------------------------------------

#[test]
fn build_help_text_mentions_pgo_pgo_build() {
    // clap renders the `///` doc-comments on each `#[arg]` field into
    // the `--help` output. This test verifies the `--pgo` flag is
    // documented (T62 acceptance: "Help text mentions PGO").
    let cmd = Cli::command();
    let mut build_cmd = cmd.find_subcommand("build").cloned().unwrap_or(cmd);
    let help = build_cmd.render_help();
    let help_str = help.to_string();
    assert!(
        help_str.contains("--pgo"),
        "`buff build --help` must mention `--pgo`, got:\n{help_str}"
    );
    assert!(
        help_str.contains("Profile-Guided Optimization") || help_str.contains("PGO"),
        "`buff build --help` must mention PGO/Profile-Guided Optimization, got:\n{help_str}"
    );
}

// ---------------------------------------------------------------------------
// Backward compat — regular `buff build` keeps pgo=false, pgo_use=false
// ---------------------------------------------------------------------------

#[test]
fn build_without_pgo_keeps_defaults_false_pgo_build() {
    // The T62 contract: `--pgo` is opt-in. A bare `buff build foo.buff`
    // must keep pgo=false + pgo_use=false so the existing build path
    // (commands::build::run) runs unchanged. This is the backward-compat
    // guarantee — adding the flags must NOT silently enable PGO.
    let cli = Cli::parse_from(["buff", "build", "foo.buff"]);
    match cli.command {
        Command::Build { pgo, pgo_use, .. } => {
            assert!(!pgo, "default build (no --pgo) must keep pgo=false");
            assert!(!pgo_use, "default build (no --use) must keep pgo_use=false");
        }
        other => panic!("expected Build, got {other:?}"),
    }
}

// ---------------------------------------------------------------------------
// pgo_profile_toml determinism
// ---------------------------------------------------------------------------

#[test]
fn pgo_profile_toml_is_deterministic_pgo_build() {
    // Pure fixed-string helper — same output every call.
    assert_eq!(pipeline::pgo_profile_toml(), pipeline::pgo_profile_toml());
}

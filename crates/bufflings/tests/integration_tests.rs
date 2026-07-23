//! Integration tests for the bufflings CLI.
//!
//! Tests the full command flow: manifest loading, listing, verification
//! with mocked buff subprocess, and progress persistence.

use std::path::PathBuf;

use bufflings::{load_manifest, ProgressStore, VerifyConfig, VerifyOutcome};
use clap::Parser;

/// Resolve the workspace root (3 levels up from the test file).
fn workspace_root() -> PathBuf {
    let mut p = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    p.push("..");
    p.push("..");
    p.canonicalize().unwrap_or_else(|_| PathBuf::from("..\\.."))
}

/// The exercises directory relative to workspace root.
fn exercises_dir() -> PathBuf {
    workspace_root().join("exercises")
}

// ---------------------------------------------------------------------------
// Manifest loading
// ---------------------------------------------------------------------------

#[test]
fn load_seed_manifest_finds_all_exercises() {
    let ex_dir = exercises_dir();
    let manifest = load_manifest(&ex_dir).expect("manifest should load from exercises/");

    assert_eq!(
        manifest.total_count(),
        25,
        "expected 25 exercises (5 seed from T138a + 20 added by T138b)"
    );

    // Verify all expected exercises are present
    assert!(manifest.find_entry("variables1").is_some());
    assert!(manifest.find_entry("hello1").is_some());
    assert!(manifest.find_entry("functions1").is_some());
    assert!(manifest.find_entry("if1").is_some());
    assert!(manifest.find_entry("option1").is_some());

    // Verify topic grouping
    let topic_names: Vec<&str> = manifest.topics.iter().map(|t| t.name.as_str()).collect();
    assert!(topic_names.contains(&"basics"));
    assert!(topic_names.contains(&"functions"));
    assert!(topic_names.contains(&"control_flow"));
    assert!(topic_names.contains(&"types"));
}

// ---------------------------------------------------------------------------
// TODO detection on real seed exercises
// ---------------------------------------------------------------------------

#[test]
fn seed_exercises_contain_todo_markers() {
    let ex = exercises_dir();
    let exercises = [
        ex.join("basics/variables1.buff"),
        ex.join("basics/hello1.buff"),
        ex.join("functions/functions1.buff"),
        ex.join("control_flow/if1.buff"),
        ex.join("types/option1.buff"),
    ];

    for path in &exercises {
        let source = std::fs::read_to_string(path)
            .unwrap_or_else(|e| panic!("failed to read {}: {e}", path.display()));
        assert!(
            bufflings::contains_todo(&source),
            "{:?} should contain a TODO marker",
            path
        );
    }
}

#[test]
fn solution_files_contain_no_todo_markers() {
    let ex = exercises_dir();
    let solutions = [
        ex.join("basics/variables1.sol.buff"),
        ex.join("basics/hello1.sol.buff"),
        ex.join("functions/functions1.sol.buff"),
        ex.join("control_flow/if1.sol.buff"),
        ex.join("types/option1.sol.buff"),
    ];

    for path in &solutions {
        let source = std::fs::read_to_string(path)
            .unwrap_or_else(|e| panic!("failed to read {}: {e}", path.display()));
        assert!(
            !bufflings::contains_todo(&source),
            "{:?} solution should NOT contain a TODO marker",
            path
        );
    }
}

// ---------------------------------------------------------------------------
// Verify with mocked buff (BuffNotFound path)
// ---------------------------------------------------------------------------

#[test]
fn verify_solution_file_without_buff_returns_buff_not_found() {
    let config = VerifyConfig {
        buff_bin: "nonexistent_buff_for_test".to_string(),
    };
    let path = exercises_dir().join("basics/variables1.sol.buff");
    let source = std::fs::read_to_string(&path).unwrap();
    let outcome = bufflings::verify_exercise(&source, &path, &config);
    // Solution has no TODO, so it should attempt buff check and get BuffNotFound
    assert_eq!(outcome, VerifyOutcome::BuffNotFound);
}

#[test]
fn verify_seed_exercise_fast_fails_to_not_done_yet() {
    let config = VerifyConfig::default();
    let path = exercises_dir().join("basics/variables1.buff");
    let source = std::fs::read_to_string(&path).unwrap();
    let outcome = bufflings::verify_exercise(&source, &path, &config);
    assert_eq!(outcome, VerifyOutcome::NotDoneYet);
}

// ---------------------------------------------------------------------------
// Progress persistence round-trip
// ---------------------------------------------------------------------------

#[test]
fn progress_persist_to_temp_file() {
    let tmp = std::env::temp_dir().join("bufflings-test-progress.toml");
    let _ = std::fs::remove_file(&tmp);

    let mut store = ProgressStore::default();
    store.mark_done("variables1");
    store.mark_done("hello1");

    let content = toml::to_string_pretty(&store).unwrap();
    std::fs::write(&tmp, &content).unwrap();

    let read_back = std::fs::read_to_string(&tmp).unwrap();
    let loaded: ProgressStore = toml::from_str(&read_back).unwrap();
    assert!(loaded.is_done("variables1"));
    assert!(loaded.is_done("hello1"));
    assert!(!loaded.is_done("functions1"));

    let _ = std::fs::remove_file(&tmp);
}

// ---------------------------------------------------------------------------
// CLI parse smoke test
// ---------------------------------------------------------------------------

#[test]
fn cli_parse_list() {
    let cli = bufflings::Cli::try_parse_from(["bufflings", "list"]).unwrap();
    assert!(matches!(cli.command, bufflings::Command::List));
}

#[test]
fn cli_parse_start() {
    let cli = bufflings::Cli::try_parse_from(["bufflings", "start", "variables1"]).unwrap();
    if let bufflings::Command::Start { name } = cli.command {
        assert_eq!(name, "variables1");
    } else {
        panic!("expected Start command");
    }
}

#[test]
fn cli_parse_verify_single() {
    let cli = bufflings::Cli::try_parse_from(["bufflings", "verify", "hello1"]).unwrap();
    if let bufflings::Command::Verify { name, all } = cli.command {
        assert_eq!(name.unwrap(), "hello1");
        assert!(!all);
    } else {
        panic!("expected Verify command");
    }
}

#[test]
fn cli_parse_verify_all() {
    let cli = bufflings::Cli::try_parse_from(["bufflings", "verify", "--all"]).unwrap();
    if let bufflings::Command::Verify { name, all } = cli.command {
        assert!(name.is_none());
        assert!(all);
    } else {
        panic!("expected Verify command");
    }
}

#[test]
fn cli_parse_progress() {
    let cli = bufflings::Cli::try_parse_from(["bufflings", "progress"]).unwrap();
    assert!(matches!(cli.command, bufflings::Command::Progress));
}

#[test]
fn cli_parse_watch() {
    let cli = bufflings::Cli::try_parse_from(["bufflings", "watch"]).unwrap();
    assert!(matches!(cli.command, bufflings::Command::Watch));
}

#[test]
fn cli_parse_hint() {
    let cli = bufflings::Cli::try_parse_from(["bufflings", "hint", "variables1"]).unwrap();
    if let bufflings::Command::Hint { name } = cli.command {
        assert_eq!(name, "variables1");
    } else {
        panic!("expected Hint command");
    }
}

// ---------------------------------------------------------------------------
// Manifest entries have hints
// ---------------------------------------------------------------------------

#[test]
fn manifest_entries_have_hints() {
    let manifest = load_manifest(&exercises_dir()).unwrap();

    let entry = manifest.find_entry("variables1").unwrap();
    assert!(entry.hint.is_some());

    let entry = manifest.find_entry("hello1").unwrap();
    assert!(entry.hint.is_some());

    let entry = manifest.find_entry("functions1").unwrap();
    assert!(entry.hint.is_some());
}

//! `bufflings` — a Rustlings-style exercise runner for the Buff language.
//!
//! Provides a CLI that walks the user through `.buff` exercises, tracking
//! progress and verifying solutions via `buff check` (subprocess).
//!
//! # Architecture
//!
//! ```text
//!   bufflings list / start / verify / progress / watch / hint
//!       │
//!       ▼
//!   exercise::{load_manifest, list_exercises, ...}
//!       │
//!       ▼
//!   verify::{contains_todo, run_buff_check}
//!       │
//!       ▼
//!   progress::{load_progress, save_progress}
//! ```
//!
//! # No panics
//!
//! There are no `unwrap` / `expect` / `panic!` / `unimplemented!` /
//! `todo!` calls outside `#[cfg(test)]`.

mod cli;
mod exercise;
mod progress;
mod verify;
mod watch;

pub use cli::{Cli, Command};
pub use exercise::load_manifest;
pub use exercise::{ExerciseEntry, ExerciseManifest, TopicGroup};
pub use progress::ProgressStore;
pub use verify::{apply_solution, verify_all_with_solutions};
pub use verify::{contains_todo, run_buff_check, verify_exercise};
pub use verify::{SolutionVerificationReport, VerifyConfig, VerifyOutcome};

use std::io::Write;
use std::path::PathBuf;

/// Top-level dispatch. Matches the parsed [`Command`] and delegates to
/// the appropriate handler. Returns `Ok(())` on success (or on
/// expected non-zero-exit conditions like "not done yet").
pub fn run(cli: Cli) -> anyhow::Result<()> {
    let exercises_dir = resolve_exercises_dir();
    let manifest = exercise::load_manifest(&exercises_dir)?;
    let mut progress = progress::ProgressStore::load()?;

    match cli.command {
        Command::List => cmd_list(&manifest, &progress),
        Command::Start { name } => cmd_start(&manifest, &name),
        Command::Verify { name, all } => cmd_verify(&manifest, &mut progress, name.as_deref(), all),
        Command::Progress => cmd_progress(&manifest, &progress),
        Command::Watch => cmd_watch(&manifest, &mut progress, &exercises_dir),
        Command::Hint { name } => cmd_hint(&manifest, &name),
        Command::VerifyAllWithSolutions => cmd_verify_all_with_solutions(&manifest, &exercises_dir),
    }
}

/// Resolve the exercises directory. Checks, in order:
/// 1. `./exercises/` relative to the current working directory.
/// 2. `./exercises/` relative to the directory containing the
///    `bufflings.toml` manifest (by walking up).
///
/// Returns the first path that exists and contains a `bufflings.toml`.
fn resolve_exercises_dir() -> PathBuf {
    // Default: exercises/ next to CWD
    let candidate = std::env::current_dir()
        .unwrap_or_default()
        .join("exercises");
    if candidate.join("bufflings.toml").exists() {
        return candidate;
    }
    candidate
}

// ---------------------------------------------------------------------------
// Subcommand handlers
// ---------------------------------------------------------------------------

fn cmd_list(manifest: &ExerciseManifest, progress: &ProgressStore) -> anyhow::Result<()> {
    let mut out = std::io::stdout();
    let mut done = 0usize;
    let mut total = 0usize;
    for group in &manifest.topics {
        let _ = writeln!(out, "\n== {} ==", group.name);
        for entry in &group.exercises {
            total += 1;
            let is_done = progress.is_done(&entry.name);
            if is_done {
                done += 1;
            }
            let mark = if is_done { "x" } else { " " };
            let _ = writeln!(out, " [{mark}] {name}", name = entry.name);
        }
    }
    let _ = writeln!(out, "\nProgress: {done}/{total}");
    Ok(())
}

fn cmd_start(manifest: &ExerciseManifest, name: &str) -> anyhow::Result<()> {
    let entry = manifest
        .find_entry(name)
        .ok_or_else(|| anyhow::anyhow!("exercise `{name}` not found in manifest"))?;
    let path = &entry.path;
    println!("Exercise: {name}");
    println!("Path: {}", path.display());
    println!("Open the file above and fill in the missing code.");
    println!("Run `bufflings verify {name}` to check your solution.");

    // Try to open the user's editor
    if let Some(editor) = std::env::var_os("EDITOR") {
        let editor_str = editor.to_string_lossy().to_string();
        let _ = std::process::Command::new(&editor_str).arg(path).status();
    }
    Ok(())
}

fn cmd_verify(
    manifest: &ExerciseManifest,
    progress: &mut ProgressStore,
    name: Option<&str>,
    all: bool,
) -> anyhow::Result<()> {
    let config = VerifyConfig::default();
    let mut any_failed = false;

    let entries: Vec<&ExerciseEntry> = if all {
        manifest.all_entries()
    } else {
        let n = name.ok_or_else(|| anyhow::anyhow!("exercise name required (or use --all)"))?;
        let entry = manifest
            .find_entry(n)
            .ok_or_else(|| anyhow::anyhow!("exercise `{n}` not found in manifest"))?;
        vec![entry]
    };

    for entry in &entries {
        let source = match std::fs::read_to_string(&entry.path) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("Error reading {}: {e}", entry.path.display());
                any_failed = true;
                continue;
            }
        };

        let outcome = verify::verify_exercise(&source, &entry.path, &config);
        match outcome {
            VerifyOutcome::Solved => {
                println!("[SOLVED] {}", entry.name);
                progress.mark_done(&entry.name);
            }
            VerifyOutcome::NotDoneYet => {
                println!("[NOT DONE YET] {}", entry.name);
                println!("  Hint: remove // TODO: markers and complete the code.");
                any_failed = true;
            }
            VerifyOutcome::CompileError(msg) => {
                println!("[FAILED] {}", entry.name);
                if !msg.is_empty() {
                    for line in msg.lines().take(10) {
                        println!("  {line}");
                    }
                }
                any_failed = true;
            }
            VerifyOutcome::BuffNotFound => {
                println!("[SKIP] {}", entry.name);
                println!("  `buff` CLI not found. Install it to verify exercises.");
                any_failed = true;
            }
            VerifyOutcome::NotStarted => {
                println!("[NOT STARTED] {}", entry.name);
                any_failed = true;
            }
            VerifyOutcome::WrongOutput(msg) => {
                println!("[WRONG OUTPUT] {}", entry.name);
                println!("  {msg}");
                any_failed = true;
            }
        }
    }

    progress.save()?;

    if any_failed {
        std::process::exit(1);
    }
    Ok(())
}

fn cmd_progress(manifest: &ExerciseManifest, progress: &ProgressStore) -> anyhow::Result<()> {
    let total = manifest.total_count();
    let done = progress.count_done(manifest);
    let pct = if total > 0 { done * 100 } else { 0 } / if total > 0 { total } else { 1 };
    println!("Progress: {done}/{total} ({pct}%)");
    if done == total && total > 0 {
        println!("All exercises complete! You've mastered the basics of Buff.");
    } else if done > 0 {
        println!("Keep going! Run `bufflings watch` to work on the next exercise.");
    } else {
        println!("Start with `bufflings list` to see available exercises.");
    }
    Ok(())
}

fn cmd_watch(
    manifest: &ExerciseManifest,
    progress: &mut ProgressStore,
    exercises_dir: &PathBuf,
) -> anyhow::Result<()> {
    // Enter the tokio runtime for the async file watcher.
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    rt.block_on(async { watch::run_watch(manifest, progress, exercises_dir).await })
}

fn cmd_verify_all_with_solutions(
    _manifest: &ExerciseManifest,
    exercises_dir: &std::path::Path,
) -> anyhow::Result<()> {
    let config = VerifyConfig::default();
    let report = verify::verify_all_with_solutions(exercises_dir, &config);

    let solved = report.solved_count();
    let total = report.total_count();
    println!("Solvability gate: {solved}/{total} solutions pass buff check");

    let mut any_failed = false;
    for (name, outcome) in &report.results {
        match outcome {
            VerifyOutcome::Solved => {
                println!("  [OK] {name}");
            }
            VerifyOutcome::CompileError(msg) => {
                println!("  [FAIL] {name}");
                for line in msg.lines().take(10) {
                    println!("    {line}");
                }
                any_failed = true;
            }
            VerifyOutcome::BuffNotFound => {
                println!("  [SKIP] {name} (buff binary not found)");
                any_failed = true;
            }
            VerifyOutcome::NotDoneYet => {
                println!("  [FAIL] {name} (still contains TODO after solution apply)");
                any_failed = true;
            }
            VerifyOutcome::NotStarted => {
                println!("  [FAIL] {name} (not started)");
                any_failed = true;
            }
            VerifyOutcome::WrongOutput(msg) => {
                println!("  [FAIL] {name} (wrong output: {msg})");
                any_failed = true;
            }
        }
    }

    if any_failed {
        std::process::exit(1);
    }
    Ok(())
}

fn cmd_hint(manifest: &ExerciseManifest, name: &str) -> anyhow::Result<()> {
    let entry = manifest
        .find_entry(name)
        .ok_or_else(|| anyhow::anyhow!("exercise `{name}` not found in manifest"))?;
    let hint = entry
        .hint
        .as_deref()
        .unwrap_or("No hint available for this exercise.");
    println!("Hint for `{name}`: {hint}");
    Ok(())
}

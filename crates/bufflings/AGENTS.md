# bufflings

A Rustlings-style exercise runner for the Buff language. Shipped in
v1.11.0 (T138c). Walks the user through `.buff` exercises, tracks
progress, and verifies solutions via `buff check` (subprocess).

## STRUCTURE

```
src/
├── lib.rs        # Public API re-exports + architecture diagram.
├── cli.rs        # Cli enum + Command dispatch (list/start/verify/progress/watch/hint/VerifyAllWithSolutions).
├── exercise.rs   # load_manifest / list_exercises — ExerciseEntry / TopicGroup.
├── verify.rs     # contains_todo / run_buff_check / verify_exercise / verify_all_with_solutions.
├── progress.rs   # ProgressStore — load/save per-exercise progress JSON.
└── watch.rs      # file-watcher loop (re-verify on save).
main.rs           # thin binary dispatch.
exercises/        # 25 exercises across 11 topics (.buff + .README.md + .sol.buff).
tests/
├── integration_tests.rs
└── ...
```

## PUBLIC API

```text
Cli / Command                          // CLI surface
load_manifest() / list_exercises()     // exercise discovery
ExerciseEntry / ExerciseManifest / TopicGroup
contains_todo(source) -> bool          // detect // TODO: markers
run_buff_check(path, cfg) -> ...       // subprocess buff check
verify_exercise(...) -> VerifyOutcome
verify_all_with_solutions(...) -> SolutionVerificationReport
ProgressStore                          // load/save progress
```

## WHERE TO LOOK

| Task | File |
|---|---|
| Add/change a CLI subcommand | `src/cli.rs` |
| Change exercise manifest format / discovery | `src/exercise.rs` |
| Change TODO detection / buff-check invocation | `src/verify.rs` |
| Change progress persistence | `src/progress.rs` |
| Change the watch loop | `src/watch.rs` |
| Add a new exercise | `exercises/<topic>/<name>.buff` + `.README.md` + `.sol.buff` |

## CONVENTIONS (this crate only)

- **No `unwrap`/`expect`/`panic!`/`unimplemented!`/`todo!`** outside
  `#[cfg(test)]` (project hard rule). The crate explicitly documents this
  at the crate root.
- **Exercise contract:** each exercise = `.buff` (with `// TODO:` markers)
  + `.README.md` (concept) + `.sol.buff` (hidden solution). `contains_todo`
  detects the `// TODO:` marker (case-sensitive).
- **CI solvability gate:** `bufflings verify-all-with-solutions` runs
  `buff check` against every hidden solution before merge — catches
  unsolvable exercises.
- **BTreeMap/BTreeSet only** where collections are used.

## DEPS

All workspace-pinned: `clap`, `serde`/`serde_json`, `notify` (file
watcher). Dev: `insta`, `tempfile`.

## REFERENCES

- Plan: `.sisyphus/plans/buff-v1x-frameworks.md` task T138c.
- Pattern: Rustlings (https://github.com/rust-lang/rustlings).

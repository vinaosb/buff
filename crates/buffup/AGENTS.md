# buffup

Version manager for the Buff language. Shipped in v1.12.0 (T139).
Downloads pre-built Buff binaries from GitHub Releases and installs them
under `~/.buff/versions/<ver>/`, selecting the active version via a
symlink (Unix) or copy/junction (Windows fallback) at `~/.buff/bin/buff`.

## STRUCTURE

```
src/
├── lib.rs        # Public API re-exports + architecture + cross-platform behavior.
├── cli.rs        # Cli enum + Command dispatch (install/default/list/update).
├── error.rs      # BuffupError enum (thiserror).
├── github.rs     # GitHub Releases API client (reqwest rustls-tls).
├── paths.rs      # ~/.buff/ + ~/.buff/versions/ + ~/.buff/bin/ resolution.
├── main.rs       # thin binary dispatch (#[tokio::main]).
└── commands/
    ├── mod.rs
    ├── install.rs     # download gzip tarball (flate2) + unpack.
    ├── default_cmd.rs # symlink/copy the active-version pointer.
    ├── list.rs        # enumerate ~/.buff/versions/ + mark active.
    └── update.rs      # self-update (NOT YET IMPLEMENTED — prints build-from-source guidance).
tests/
├── install_mock.rs   # hermetic via httpmock + BUFFUP_HOME + BUFFUP_GITHUB_API.
├── list_installed.rs
└── cli.rs
```

## PUBLIC API

The crate is a binary + library (dual `bin`/`lib`). Entry point: the
`buffup` CLI with subcommands `install` / `default` / `list` / `update`.

## WHERE TO LOOK

| Task | File |
|---|---|
| Add/change a CLI subcommand | `src/cli.rs` + `src/commands/<name>.rs` |
| Change GitHub Releases API client | `src/github.rs` |
| Change install-dir / symlink resolution | `src/paths.rs` |
| Change download + unpack | `src/commands/install.rs` |
| Change active-version pointer | `src/commands/default_cmd.rs` |
| Change error variants | `src/error.rs` |

## CONVENTIONS (this crate only)

- **No `unwrap`/`expect`/`panic!`/`todo!`/`unimplemented!`** in non-test
  code (project hard rule).
- **Cross-platform active pointer:** Unix uses
  `std::os::unix::fs::symlink` (no privileges); Windows tries
  `std::os::windows::fs::symlink_file` (needs Developer Mode / admin) and
  falls back to a plain file copy. The copy fallback does NOT auto-track
  reinstalls — re-running `buffup install <ver>` over the same version
  does not refresh the active binary unless `buffup default <ver>` is also
  re-run.
- **Hermetic tests:** integration tests override `BUFFUP_HOME` (redirects
  `~/.buff/` to a `tempfile::TempDir`) and `BUFFUP_GITHUB_API` (redirects
  the API base URL to an `httpmock` server). No test touches the real
  network or the user's home directory.
- **rustls-tls** (NOT native-tls) per the project "Pure-Rust preference".
- **BTreeMap/BTreeSet only** where collections are used.

## OUT OF SCOPE (deferred)

Per the T139 task spec, these are explicitly NOT supported:
- Per-directory overrides (`.buff-version` files).
- Components (rustup-style splitting) — Buff ships as a single binary.
- Nightly channel — only release-tagged versions.
- Self-update — `buffup update` is reserved; implementation deferred.

## DEPS

All workspace-pinned: `clap`, `reqwest` (rustls-tls, blocking→async via
`#[tokio::main]`), `tokio`, `flate2`, `serde`/`serde_json`, `tar`,
`dirs`. Dev: `httpmock`, `tempfile`, `insta`.

## REFERENCES

- Plan: `.sisyphus/plans/buff-v1x-frameworks.md` task T139.
- Pattern: rustup (https://github.com/rust-lang/rustup).

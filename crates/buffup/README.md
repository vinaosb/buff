# buffup

> Version manager for the **Buff** language — install and switch between Buff releases.

`buffup` is the Rust implementation of the `rustup` workflow applied to Buff.
It downloads pre-built Buff binaries from
[github.com/buff-lang/buff/releases](https://github.com/buff-lang/buff/releases),
installs them under `~/.buff/versions/<ver>/`, and points `~/.buff/bin/buff`
at the active version via a symlink (Unix) or copy (Windows fallback).

## Status

**v1.0** — landed as part of the v1.12 *Distribution Scale* milestone (T139).

The GitHub Releases that `buffup install` consumes are published as part of
the same milestone. Until then, `buffup install <ver>` fails gracefully with
build-from-source instructions; `buffup default`, `buffup list`, and the CLI
plumbing all work against any manually-seeded `~/.buff/versions/` directory.

## Installation

```bash
cargo install --path crates/buffup --locked
```

A `cargo install buffup` flow (publishing to crates.io) is planned for a
later milestone.

## Quick start

```bash
# Install a version (requires GitHub Releases to be published)
buffup install 1.0.0
buffup install 1.1.0

# Set the active version
buffup default 1.1.0

# Add the active pointer to your PATH (shell-dependent — example for bash):
echo 'export PATH="$HOME/.buff/bin:$PATH"' >> ~/.bashrc

# Verify
buff --version

# List installed versions
buffup list
# v1.0.0
# v1.1.0 * (active)
```

## Subcommands

| Command                  | Description                                                                  |
| ----------------------- | ---------------------------------------------------------------------------- |
| `buffup install <ver>`  | Download and unpack `<ver>` from GitHub Releases.                            |
| `buffup default <ver>`  | Point `~/.buff/bin/buff` at the named installed version's binary.            |
| `buffup list`           | List installed versions; mark the active one with `* (active)`.              |
| `buffup update`         | Self-update buffup (NOT YET IMPLEMENTED — prints build-from-source guidance).|
| `buffup --version`      | Print the buffup version and exit.                                           |
| `buffup --help`         | Print usage and exit.                                                         |

## Directory layout

```
~/.buff/                        # BUFFUP_HOME (override via env var)
├── versions/
│   ├── 1.0.0/                  # per-version install dir
│   │   └── buff[.exe]
│   └── 1.1.0/
│       └── buff[.exe]
└── bin/
    └── buff[.exe]              # symlink (Unix) / copy (Windows) -> active version
```

Add `~/.buff/bin` to your `PATH` so the active `buff` is discoverable.

## Platform behavior

| Platform | Active-version pointer                                                                 |
| -------- | ------------------------------------------------------------------------------------- |
| Unix     | Real symlink via `std::os::unix::fs::symlink`. No privileges required.                |
| Windows  | Symlink first (`std::os::windows::fs::symlink_file`); copy fallback without dev mode. |

The Windows copy fallback does NOT auto-track reinstalls — re-running
`buffup install <ver>` over the same version does not refresh the active
binary unless you also re-run `buffup default <ver>`.

## Limitations & out-of-scope

Per the T139 task spec, the following are explicitly **NOT** supported:

- **Per-directory overrides** (`.buff-version` files) — a future task may add this.
- **Components** (rustup-style `rustc` / `cargo` / `clippy` splitting) — Buff
  ships as a single binary, so there's nothing to split.
- **Nightly channel** — only release-tagged versions (`v1.0.0`, `v1.1.0`, …).
- **Toolchain manifests** — every release is one tarball, no per-platform
  manifest to consult.
- **Self-update** — `buffup update` is reserved; the implementation is
  deferred to a follow-up of T139.

## Testing

```bash
cargo test -p buffup
cargo clippy -p buffup --all-targets -- -D warnings
cargo fmt -p buffup --check
```

Tests are fully hermetic: HTTP is mocked via `httpmock`, the filesystem
is isolated via `tempfile`, and the GitHub API base URL is redirected
through the `BUFFUP_GITHUB_API` env var. No test ever touches the real
network or the user's real `~/.buff/` directory.

## License

Dual-licensed under [MIT](../../LICENSE) or [Apache-2.0](../../LICENSE),
matching the rest of the Buff workspace.

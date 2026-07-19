# Contributing to Buff

Thanks for your interest in contributing. Buff is a source-to-source compiler
that transpiles `.buff` to Rust, and every contribution, from a bug fix to a new
language feature, moves the project forward.

This guide covers the development workflow, code conventions, and expectations
for pull requests. For the full project knowledge base, see
[AGENTS.md](./AGENTS.md).

## Code of Conduct

This project follows the [Contributor Covenant Code of Conduct](https://contributor-covenant.org/version/2/1/code_of_conduct/).
Be respectful, constructive, and inclusive in all interactions.

## Getting Started

**Prerequisites:** Rust 1.95.0 (pinned in `rust-toolchain.toml`). The
toolchain is resolved automatically when you use rustup inside the repo.

```bash
# Clone and build
cargo build --release -p buff-lang-cli

# Verify it works
cargo run -p buff-lang-cli -- run examples/ola.buff
# Expected output: Ola, Buff!
```

If that prints the greeting, your environment is set up correctly.

## Project Structure

Buff is a 9-crate Rust workspace. Each crate has a focused responsibility:

| Crate | Purpose |
|---|---|
| `buff-lang-error` | Spans, diagnostics, source maps (leaf crate, depended on by all) |
| `buff-lang-ast` | Pure AST data nodes (decl, expr, stmt, ty, op, ir) |
| `buff-lang-lexer` | Hand-rolled byte-scanner with offside-rule indent tracking |
| `buff-lang-parser` | Hand-rolled recursive-descent + Pratt parser |
| `buff-lang-types` | Type inference, prelude functions, range analysis |
| `buff-lang-codegen-rust` | AST to `syn::File` to Rust source via `prettyplease` |
| `buff-lang-codegen-wgsl` | AST to WGSL compute shaders |
| `buff-lang-runtime` | Rayon + wgpu + tokio host runtime |
| `buff-lang-cli` | Binary and library: pipeline orchestration |

The compilation pipeline flows:
`.buff source` -> lexer -> parser -> AST -> type inference (inside codegen)
-> Rust codegen -> `rustc` -> native binary.

For file-level guidance within each crate, see the per-crate `AGENTS.md` files
and the [WHERE TO LOOK table in the root AGENTS.md](./AGENTS.md).

## Development Workflow

Run these commands before every commit. CI enforces all of them:

```bash
cargo check --workspace
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

Note: the CI workflow runs `clippy` without `--all-targets`, so running it
locally with `--all-targets` catches additional lint in test code. Run the
stricter check locally.

### Running examples

```bash
cargo run -p buff-lang-cli -- run examples/ola.buff
cargo run -p buff-lang-cli -- run examples/fibonacci.buff
cargo run -p buff-lang-cli -- run examples/calculadora.buff
```

### Snapshot testing

Buff uses [insta](https://insta.rs/) for snapshot tests. When a snapshot test
creates or updates a `.snap.new` file:

```bash
cargo insta review        # interactively review pending snapshots
cargo insta accept        # accept all pending (use sparingly)
```

Never commit `.snap.new` or `.pending-snap` files. They are gitignored.

## Code Conventions

These rules are enforced by clippy and code review. Violations block merge.

### Hard rules

- **No `unwrap()`, `expect()`, `panic!()`, `unimplemented!()`, or `todo!()`**
  in non-test code. Use pattern matching or proper error propagation. This is
  a hard rule from the README.

- **No tabs.** The Buff lexer rejects tabs in source files. Use 4 spaces.

- **No raw-string Rust codegen.** Build output via `syn`/`quote`, format via
  `prettyplease`. The single string producer in the pipeline is `prettyplease`.

- **No trailing whitespace, no more than 2 consecutive blank lines,**
  trailing commas required in multi-line collections.

- **Workspace dependencies only.** Every crate uses `dep.workspace = true`.
  Never pin a version in a crate-level `Cargo.toml`. Add new dependencies to
  the root `[workspace.dependencies]` section first.

- **No `[features]`, `[lints]`, or `[profile.*]` sections** in any Cargo.toml.
  No crate-level `#![deny(...)]` or `#![forbid(unsafe_code)]`.

### Conventions

- **Derive defaults:** `Debug, Clone, PartialEq` on all types. Add `Eq, Hash`
  when the type is used in maps or sets.

- **Errors:** Use `thiserror::Error` derive. Map to `buff_lang_error::*Error`
  variants for the unified diagnostic system.

- **Tests:** Integration tests go in per-crate `tests/` directories (not `src`).
  Inline `#[cfg(test)]` modules are fine for unit smoke tests.

- **Rust crate naming:** folder `buff-lang-<thing>` (hyphens) becomes crate
  ident `buff_lang_<thing>` (underscores).

- **Edition and license:** Edition 2021, `MIT OR Apache-2.0` on every crate.

### Buff language rules

When writing `.buff` files (examples, tests, or fixtures), follow the 18
conventions documented in
[buff-conventions.md](.sisyphus/plans/buff-conventions.md). Key points:

- `snake_case` for functions and variables, `PascalCase` for types
- 4-space indentation, 100-character line limit
- No `_async` suffix on async functions (async is in the type, not the name)
- Constructors are `Type.new()` and `Type.from()` only
- Boolean parameters must always be named: `fetch(url, cache: true)`

## Commit Style

We use [Conventional Commits](https://www.conventionalcommits.org/):

```
feat(scope): add WGSL compute shader codegen
fix(parser): handle nested block expressions correctly
docs(readme): update quick start with new examples
test(runtime): add mock GPU backend and snapshot harness
refactor(ast): extract shared trait for AST visitors
chore(release): bump versions to 1.0.0
perf(runtime): add pipeline caching and buffer pooling
```

The scope is the crate or component affected. Keep the subject line under 72
characters. Write the body only when the "why" is not obvious from the diff.

## Testing Strategy

- **Snapshot tests (insta):** The primary testing mechanism. AST structures,
  generated Rust source, and error diagnostics are snapshotted. Run
  `cargo insta review` to accept changes.

- **Property-based tests (proptest):** Used for fuzzing edge cases in the lexer,
  parser, and type inferencer.

- **Golden `.buff` fixtures:** The `tests/` directory holds valid and invalid
  `.buff` files used as integration inputs.

- **TDD approach:** The project follows RED, GREEN, REFACTOR. Write a failing
  test for the behavior you want, make it pass, then clean up.

## Filing Issues

Bug reports should include:

1. Buff version (from `Cargo.toml` in the workspace root)
2. A minimal `.buff` file that reproduces the issue
3. Expected behavior vs. actual behavior
4. The generated Rust source (run with `--emit-rust` if available) or the full
   error output
5. Your platform and Rust version (`rustc --version`)

Feature requests should explain the problem they solve, not just the syntax
desired. Link to relevant plan files in `.sisyphus/plans/` if the feature maps
to an existing task.

## Pull Requests

- Keep PRs focused on a single concern. Split large changes into smaller,
  reviewable pieces.
- Link related issues in the PR description.
- All CI gates must pass: `cargo fmt --check`, `cargo clippy --workspace -- -D
  warnings`, `cargo test --workspace` on all three OSes.
- Do not force-push after someone has reviewed your PR.

## Roadmap and Planning

The project follows a phased roadmap tracked in `.sisyphus/plans/`:

| Phase | Plan file |
|---|---|
| v0.1 "Ola, Buff" | [`buff-v01-mvp.md`](.sisyphus/plans/buff-v01-mvp.md) |
| v0.5 "Real Language" | [`buff-v05-language.md`](.sisyphus/plans/buff-v05-language.md) |
| v1.0 "Production" | [`buff-v10-production.md`](.sisyphus/plans/buff-v10-production.md) |
| Master orchestrator | [`buff-master.md`](.sisyphus/plans/buff-master.md) |

If you are picking up a task from the plan files, note which task ID you are
addressing in your PR description so maintainers can track progress.

## License

Contributions are licensed under the same terms as the project:
[MIT OR Apache-2.0](./LICENSE).

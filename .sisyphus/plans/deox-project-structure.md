# Deox Project Structure Standard

> Inspired by: Rust (cargo), Go (standard layout), .NET (templates), Rails (convention over configuration)

## Philosophy
**One canonical structure that scales from 1 file to monorepo. No debate about where files go.**

---

## Single Project Layout

```
my_project/
├── deox.toml                    # Project config
├── src/                         # ALL source code
│   ├── main.deox                # Entry point (executable projects)
│   ├── lib.deox                 # Library root (library projects)
│   ├── module_name/             # Modules = directories
│   │   └── submodule.deox       # import from "module_name/submodule"
│   └── internal/                # Private modules (Go convention)
│       └── impl.deox            # Not exported, internal only
├── tests/                       # Integration tests
│   └── test_module.deox         # @test functions testing whole project
├── benches/                     # Performance benchmarks
│   └── bench_sort.deox          # @bench functions
├── examples/                    # Runnable example programs
│   └── demo.deox                # deox run examples/demo.deox
└── README.md
```

## Workspace Layout (Monorepo)

```
my_workspace/
├── deox.workspace.toml          # Workspace root
├── packages/                    # All packages
│   ├── core/
│   │   ├── deox.toml
│   │   └── src/
│   ├── cli/
│   │   ├── deox.toml
│   │   └── src/
│   └── web/
│       ├── deox.toml
│       └── src/
└── examples/                    # Shared examples
```

## deox.toml Schema

```toml
[package]
name = "my_project"              # Required
version = "0.1.0"                # SemVer
edition = "2024"                 # Language edition
authors = ["Name <email>"]
description = "Project description"
license = "MIT"

[dependencies]
http = "1.2"                     # Deox package
serde = { extern = "serde" }     # Rust crate via FFI

[dev-dependencies]               # Test-only dependencies
deox_test = "1.0"

[targets]
default = "native"               # native | wasm | both

[profile.release]
opt-level = 3
lto = true

[workspace]                      # Only in workspace root
members = ["packages/core", "packages/cli"]
```

## deox.workspace.toml Schema

```toml
[workspace]
members = ["packages/*"]
resolver = "2"                   # Dependency resolution version

[workspace.dependencies]         # Shared dependency versions
http = "1.2"
```

## CLI Commands

```bash
# Scaffolding
deox new my_app                  # New CLI app in new directory
deox new my_lib --lib            # New library
deox new my_server --server      # Async web server template
deox new my_gpu --gpu            # GPU compute template
deox new my_ws --workspace       # Multi-package workspace
deox init                        # Init in current directory
deox init --lib                  # Convert to library

# Building
deox build                       # Build debug
deox build --release             # Build optimized
deox run                         # Build + run
deox run examples/demo.deox      # Run specific file

# Testing
deox test                        # Run all tests
deox test --pattern "test_*"     # Filter tests
deox test --doc                  # Run doctests
deox bench                       # Run benchmarks

# Development
deox watch                       # Auto-rebuild on change
deox fmt                         # Format all .deox files
deox check                       # Type-check without codegen
deox doc                         # Generate HTML documentation

# Dependencies
deox add http                    # Add dependency to deox.toml
deox add serde --extern          # Add Rust crate via FFI
deox remove http                 # Remove dependency
deox update                      # Update all dependencies
```

## Convention Rules

| Rule | Convention |
|------|-----------|
| Entry point | `src/main.deox` |
| Library root | `src/lib.deox` |
| Module path | `src/math/matrix.deox` → `import from "math/matrix"` |
| Private modules | `src/internal/` (convention: not exported) |
| Unit tests | Inline `@test` in source files |
| Integration tests | `tests/test_*.deox` |
| Doc tests | `///` comments with code blocks |
| Benchmarks | `benches/bench_*.deox` |
| Examples | `examples/*.deox` |
| Config | `deox.toml` at project root |
| Workspace root | `deox.workspace.toml` |
| Lock file | `deox.lock` (auto-generated, committed) |
| Output | `target/` (gitignored) |

## Editions

```toml
[package]
edition = "2024"    # First stable Deox edition
```

Editions allow breaking syntax changes every 2-3 years without breaking existing code.
Old editions compile forever. New code opts into new edition explicitly.
(Same model as Rust editions — one of its most-loved features.)

## Templates

| Template | Command | Description |
|----------|---------|-------------|
| CLI App | `deox new name` | Basic command-line application |
| Library | `deox new name --lib` | Reusable library package |
| Web Server | `deox new name --server` | Async HTTP server with routing |
| GPU Compute | `deox new name --gpu` | wgpu setup with par_map example |
| Workspace | `deox new name --workspace` | Multi-package monorepo |

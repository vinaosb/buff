# Buff Project Structure Standard

> Inspired by: Rust (cargo), Go (standard layout), .NET (templates), Rails (convention over configuration)

## Philosophy
**One canonical structure that scales from 1 file to monorepo. No debate about where files go.**

---

## Single Project Layout

```
my_project/
├── buff.toml                    # Project config
├── src/                         # ALL source code
│   ├── main.buff                # Entry point (executable projects)
│   ├── lib.buff                 # Library root (library projects)
│   ├── module_name/             # Modules = directories
│   │   └── submodule.buff       # import from "module_name/submodule"
│   └── internal/                # Private modules (Go convention)
│       └── impl.buff            # Not exported, internal only
├── tests/                       # Integration tests
│   └── test_module.buff         # @test functions testing whole project
├── benches/                     # Performance benchmarks
│   └── bench_sort.buff          # @bench functions
├── examples/                    # Runnable example programs
│   └── demo.buff                # buff run examples/demo.buff
└── README.md
```

## Workspace Layout (Monorepo)

```
my_workspace/
├── buff.workspace.toml          # Workspace root
├── packages/                    # All packages
│   ├── core/
│   │   ├── buff.toml
│   │   └── src/
│   ├── cli/
│   │   ├── buff.toml
│   │   └── src/
│   └── web/
│       ├── buff.toml
│       └── src/
└── examples/                    # Shared examples
```

## buff.toml Schema

```toml
[package]
name = "my_project"              # Required
version = "0.1.0"                # SemVer
edition = "2024"                 # Language edition
authors = ["Name <email>"]
description = "Project description"
license = "MIT"

[dependencies]
http = "1.2"                     # Buff package
serde = { extern = "serde" }     # Rust crate via FFI

[dev-dependencies]               # Test-only dependencies
buff_test = "1.0"

[targets]
default = "native"               # native | wasm | both

[profile.release]
opt-level = 3
lto = true

[workspace]                      # Only in workspace root
members = ["packages/core", "packages/cli"]
```

## buff.workspace.toml Schema

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
buff new my_app                  # New CLI app in new directory
buff new my_lib --lib            # New library
buff new my_server --server      # Async web server template
buff new my_gpu --gpu            # GPU compute template
buff new my_ws --workspace       # Multi-package workspace
buff init                        # Init in current directory
buff init --lib                  # Convert to library

# Building
buff build                       # Build debug
buff build --release             # Build optimized
buff run                         # Build + run
buff run examples/demo.buff      # Run specific file

# Testing
buff test                        # Run all tests
buff test --pattern "test_*"     # Filter tests
buff test --doc                  # Run doctests
buff bench                       # Run benchmarks

# Development
buff watch                       # Auto-rebuild on change
buff fmt                         # Format all .buff files
buff check                       # Type-check without codegen
buff doc                         # Generate HTML documentation

# Dependencies
buff add http                    # Add dependency to buff.toml
buff add serde --extern          # Add Rust crate via FFI
buff remove http                 # Remove dependency
buff update                      # Update all dependencies
```

## Convention Rules

| Rule | Convention |
|------|-----------|
| Entry point | `src/main.buff` |
| Library root | `src/lib.buff` |
| Module path | `src/math/matrix.buff` → `import from "math/matrix"` |
| Private modules | `src/internal/` (convention: not exported) |
| Unit tests | Inline `@test` in source files |
| Integration tests | `tests/test_*.buff` |
| Doc tests | `///` comments with code blocks |
| Benchmarks | `benches/bench_*.buff` |
| Examples | `examples/*.buff` |
| Config | `buff.toml` at project root |
| Workspace root | `buff.workspace.toml` |
| Lock file | `buff.lock` (auto-generated, committed) |
| Output | `target/` (gitignored) |

## Editions

```toml
[package]
edition = "2024"    # First stable Buff edition
```

Editions allow breaking syntax changes every 2-3 years without breaking existing code.
Old editions compile forever. New code opts into new edition explicitly.
(Same model as Rust editions — one of its most-loved features.)

## Templates

| Template | Command | Description |
|----------|---------|-------------|
| CLI App | `buff new name` | Basic command-line application |
| Library | `buff new name --lib` | Reusable library package |
| Web Server | `buff new name --server` | Async HTTP server with routing |
| GPU Compute | `buff new name --gpu` | wgpu setup with par_map example |
| Workspace | `buff new name --workspace` | Multi-package monorepo |

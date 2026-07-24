# 10. Real-World Examples

> This chapter catalogs the **19 real-world use cases** shipped in v1.26.
> Each example exercises a distinct set of language features and framework
> crates, stress-testing the compiler in production-like scenarios. Every
> example lives in [`examples/use-cases/`](https://github.com/vinaosb/buff/tree/main/examples/use-cases)
> and has a `.expected` golden-output file for automated verification.

---

## Focused Examples (15)

Each focused example is 50–200 lines and targets a specific gap area.

### Batch 1 — HTTP / Web / Networking

| Example | Lines | Features Exercised |
|---|---|---|
| [`http_server.buff`](https://github.com/vinaosb/buff/tree/main/examples/use-cases/http_server.buff) | ~150 | `async func`, `buff-web` framework, JSON response, route registration, error handling |
| [`tcp_echo.buff`](https://github.com/vinaosb/buff/tree/main/examples/use-cases/tcp_echo.buff) | ~100 | FFI bindings, networking, async I/O, buffered reads/writes |
| [`http_client_retry.buff`](https://github.com/vinaosb/buff/tree/main/examples/use-cases/http_client_retry.buff) | ~150 | HTTP client, `Result` chains, pattern matching, loop control, exponential backoff |

### Batch 2 — File I/O / Data / CLI

| Example | Lines | Features Exercised |
|---|---|---|
| [`file_processor.buff`](https://github.com/vinaosb/buff/tree/main/examples/use-cases/file_processor.buff) | ~100 | File I/O (extern `std::fs`), string processing, `Result` handling |
| [`csv_analyzer.buff`](https://github.com/vinaosb/buff/tree/main/examples/use-cases/csv_analyzer.buff) | ~90 | `buff-dataframe` API, numeric ops, aggregation, formatting |
| [`cli_tool.buff`](https://github.com/vinaosb/buff/tree/main/examples/use-cases/cli_tool.buff) | ~110 | `buff-cli` framework, arg parsing, stdin/stdout, command dispatch |

### Batch 3 — Concurrency / Auth / Testing

| Example | Lines | Features Exercised |
|---|---|---|
| [`concurrent_workers.buff`](https://github.com/vinaosb/buff/tree/main/examples/use-cases/concurrent_workers.buff) | ~100 | `Channel<T>` MPSC, `async func`, `spawn`, parallelism, error aggregation |
| [`auth_flow.buff`](https://github.com/vinaosb/buff/tree/main/examples/use-cases/auth_flow.buff) | ~150 | `buff-auth` framework, crypto, time handling, `Result` chains |
| [`test_runner.buff`](https://github.com/vinaosb/buff/tree/main/examples/use-cases/test_runner.buff) | ~120 | Closures, higher-order functions, string formatting, `Vector<T>` |

### Batch 4 — Crypto / Logging / Error Recovery

| Example | Lines | Features Exercised |
|---|---|---|
| [`hash_verify.buff`](https://github.com/vinaosb/buff/tree/main/examples/use-cases/hash_verify.buff) | ~100 | `Hash.sha256()`, `Hex.encode()`, hex encoding, integrity verification |
| [`structured_logger.buff`](https://github.com/vinaosb/buff/tree/main/examples/use-cases/structured_logger.buff) | ~120 | `enum` variants, `match`, string formatting, I/O, log levels |
| [`error_recovery.buff`](https://github.com/vinaosb/buff/tree/main/examples/use-cases/error_recovery.buff) | ~130 | `Result`, `Option`, pattern matching, `buff-resilience` circuit breaker |

### Batch 5 — Generics / Patterns / Advanced

| Example | Lines | Features Exercised |
|---|---|---|
| [`generic_container.buff`](https://github.com/vinaosb/buff/tree/main/examples/use-cases/generic_container.buff) | ~170 | Generics, trait bounds, associated types, `struct<T>` |
| [`exhaustive_matching.buff`](https://github.com/vinaosb/buff/tree/main/examples/use-cases/exhaustive_matching.buff) | ~160 | `match` exhaustiveness, or-patterns, guards, 10+ variant enums |
| [`comptime_config.buff`](https://github.com/vinaosb/buff/tree/main/examples/use-cases/comptime_config.buff) | ~170 | `comptime` blocks, const evaluation, lookup tables |

---

## Full Applications (3)

Each full application is 500–1500 lines and exercises multiple framework crates.

### REST API Server

[`examples/use-cases/apps/rest_api_server.buff`](https://github.com/vinaosb/buff/tree/main/examples/use-cases/apps/rest_api_server.buff) — 918 lines

A complete CRUD REST API for a "tasks" resource built on `buff-web`:

| Method | Path | Description |
|---|---|---|
| `GET` | `/` | API documentation |
| `GET` | `/health` | Liveness probe |
| `GET` | `/stats` | Task statistics by status/priority |
| `GET` | `/search?q=` | Title substring search |
| `GET` | `/tasks` | List all tasks |
| `POST` | `/tasks` | Create task from JSON |
| `GET` | `/tasks/{id}` | Get task by ID |
| `PUT` | `/tasks/{id}` | Update task |
| `DELETE` | `/tasks/{id}` | Delete task |

**Features exercised:** `struct`, `enum`, `match`, `Result<T,E>` + `?`, `Map<K,V>`, `Vector<T>`, named args, lambda handlers, `async func` propagation, middleware chain.

### CLI File Manager

[`examples/use-cases/apps/cli_file_manager.buff`](https://github.com/vinaosb/buff/tree/main/examples/use-cases/apps/cli_file_manager.buff) — ~1000 lines

A CLI file management tool with subcommands:

- `list` — List files in a directory
- `search` — Search files by pattern
- `rename` — Rename/move files
- `convert` — Convert between JSON, CSV, and TOML formats

**Features exercised:** `buff-cli` framework, arg parsing, stdin/stdout piping, `buff-dataframe` for format conversion, string processing, error handling.

### ETL Data Pipeline

[`examples/use-cases/apps/data_pipeline.buff`](https://github.com/vinaosb/buff/tree/main/examples/use-cases/apps/data_pipeline.buff) — 1194 lines

A complete ETL data processing pipeline:

- **Extract:** CSV parsing, embedded sample data, multi-source extraction
- **Transform:** Filter by threshold, enrich, group-by + agg (sum/mean/min/max/count), join datasets, rolling window average, timeseries metrics
- **Load:** ASCII table, CSV, JSON-lines rendering
- **Error handling:** Per-row validation with `Warning` collection, bad rows skipped

**Features exercised:** `buff-pipeline`, `buff-dataframe`, `Pipeline.new()`, `.source()`, `.map()`, `.filter()`, `.batch()`, `.window()`, `.parallel()`, `.run()`, `Source.from_csv`, `Sink.to_csv`/`to_json`.

---

## Running the Examples

```bash
# Typecheck all use cases
for f in examples/use-cases/*.buff; do
  buff check "$f"
done

# Run with golden-output verification
./scripts/test-use-cases.ps1
```

> **Note:** On Windows, `buff check` and `buff run` require a complete MSVC
> toolchain. If you encounter `LNK1104: cannot open file 'msvcrt.lib'`, use
> WSL or a Linux/macOS environment. All examples are verified on the 3-OS CI
> matrix.

---

## Bug Discovery

The use cases uncovered **~21 bugs and limitations** across the compiler and
framework crates. See [`examples/use-cases/BUGS-FOUND.md`](https://github.com/vinaosb/buff/tree/main/examples/use-cases/BUGS-FOUND.md)
for the full catalog. Key findings:

| Area | Issues Found | Status |
|---|---|---|
| Framework Type variants | `Type::Tensor`, `Type::DataFrame`, etc. missing | ✅ Fixed in T8 |
| ML bias gradient | 3x error in batch>1 training | ✅ Fixed in T7 |
| `buff-fake` | `StreetAddress` removed in fake 2.10 | ✅ Fixed in T20 |
| `buff-game` | Borrow conflict in `step()` | ✅ Fixed in T20 |
| `buff-jobs` | `tokio::sync::atomic` doesn't exist | ✅ Fixed in T20 |
| Prelude method gaps | `DataFrame.to_json()`, `req.param()`, etc. | 📝 Documented |
| MSVC toolchain | Missing `vcruntime.h` / `msvcrt.lib` on Windows | 📝 Documented |

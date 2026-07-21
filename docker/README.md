# Buff Docker Images

Two official Docker images for the [Buff](https://github.com/buff-lang/buff) language.

## Tags

| Tag | Base | Size (approx.) | Description |
|---|---|---|---|
| `buff:builder` | `debian:bookworm-slim` | ~2.5 GB | Rust toolchain + `buff` CLI pre-installed. Use in CI/CD to compile `.buff` projects. |
| `buff:slim` | `debian:bookworm-slim` | ~90 MB | Minimal runtime. No compiler. Non-root `buff` user (UID 1000). Use as final stage for production. |
| `buff:<version>-builder` | — | — | Version-pinned builder (e.g., `buff:1.2.0-builder`). |
| `buff:<version>-slim` | — | — | Version-pinned slim runtime. |
| `buff:latest-builder` | — | — | Latest stable builder. |
| `buff:latest-slim` | — | — | Latest stable slim runtime. |

## When to use which

- **`buff:builder`** — CI pipelines, multi-stage build `FROM` stage, local development where you need the full compiler.
- **`buff:slim`** — Production runtime for pre-built Buff binaries. Add your own `ENTRYPOINT`.

## Quick start

```bash
# Pull the builder image
docker pull buff:builder

# Compile a Buff project
docker run --rm -v "$(pwd):/app" buff:builder buff build src/main.buff

# Run the compiled binary in slim
docker run --rm -v "$(pwd):/app" buff:slim ./target/release/myapp
```

See [docs/docker.md](../docs/docker.md) for a full usage guide.

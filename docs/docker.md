# Docker Usage Guide

Buff publishes two official Docker images: `buff:builder` (full compiler) and
`buff:slim` (minimal runtime). This guide explains when to use each and how
to integrate them into your workflow.

## Image comparison

| | `buff:builder` | `buff:slim` |
|---|---|---|
| **Contents** | Rust 1.95.0 toolchain, `buff` CLI, build-essential, git, ca-certificates | debian:bookworm-slim, ca-certificates, non-root `buff` user |
| **Size** | ~2.5 GB | ~90 MB |
| **Entrypoint** | `buff` | *(none — user-defined)* |
| **Use case** | CI/CD builds, multi-stage build stage | Production runtime for pre-built binaries |

## Multi-stage Dockerfile

The recommended pattern is a multi-stage build: compile with `buff:builder`,
then copy the binary into `buff:slim` for the final image.

```dockerfile
# syntax=docker/dockerfile:1

# ── Build stage ──
FROM buff:builder AS build
WORKDIR /app
COPY . .
RUN buff build --release src/main.buff

# ── Runtime stage ──
FROM buff:slim
COPY --from=build /app/target/release/myapp /usr/local/bin/myapp
ENTRYPOINT ["myapp"]
```

Save this as `Dockerfile` in your project root and build:

```bash
docker build -t my-buff-app .
```

## Commands

### Compile a project (ad-hoc)

```bash
docker run --rm -v "$(pwd):/app" buff:builder buff build src/main.buff
```

### Run a compiled binary

```bash
docker run --rm -v "$(pwd):/app" buff:slim ./target/release/myapp
```

### Interactive REPL

```bash
docker run --rm -it buff:builder repl
```

### Check a project (type-check only, no codegen)

```bash
docker run --rm -v "$(pwd):/app" buff:builder buff check src/main.buff
```

### Scaffold a new project

```bash
docker run --rm -v "$(pwd):/app" buff:builder buff new my_app
```

## Multi-architecture

Both images are built for `linux/amd64` and `linux/arm64`. Docker automatically
selects the correct variant for your host architecture:

```bash
# On an Apple Silicon Mac (arm64):
docker pull buff:builder   # pulls linux/arm64 variant

# On an x86_64 server:
docker pull buff:builder   # pulls linux/amd64 variant
```

## GPU support

Buff's GPU compute features (WGSL shader dispatch via wgpu) require the host
system to have GPU drivers installed. The Docker images do **not** include GPU
drivers. To use GPU acceleration:

1. Install NVIDIA Container Toolkit or equivalent for your GPU vendor.
2. Pass the GPU device to the container at runtime:
   ```bash
   docker run --rm --gpus all -v "$(pwd):/app" buff:builder buff run src/main.buff
   ```

## Building the images yourself

```bash
# Clone the repo
git clone https://github.com/buff-lang/buff.git
cd buff

# Build both images
docker buildx bake -f docker/docker-bake.hjson

# Or build individually
docker build -f docker/builder.Dockerfile -t buff:builder .
docker build -f docker/slim.Dockerfile -t buff:slim .
```

## Version tags

Images are tagged on every semver release:

- `buff:1.2.0-builder` / `buff:1.2.0-slim` — exact version
- `buff:latest-builder` / `buff:latest-slim` — latest stable
- `buff:builder` / `buff:slim` — latest stable (convenience aliases)

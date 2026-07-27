# syntax=docker/dockerfile:1
# buff:builder — Rust toolchain + Buff CLI pre-installed.
# Use this image in CI/CD pipelines to compile .buff projects.
#
# Expected size: ~2.5GB (includes Rust toolchain + build deps).
# See docker/slim.Dockerfile for the minimal runtime image.
#
# P0.10 — base images pinned by digest (sha256:) for supply-chain protection.
# To bump: pull the new tag, capture its `docker manifest inspect <tag>` digest,
# and replace BOTH the tag and the digest below in lock-step.

FROM rust:1.95.0-slim-bookworm@sha256:d7482085ff5b415f84dba5647ae71606650bdef00db7aeb69f4b3d170c3e4082 AS build

RUN apt-get update -y \
 && apt-get install -y --no-install-recommends \
    build-essential \
    git \
    ca-certificates \
 && rm -rf /var/lib/apt/lists/*

WORKDIR /buff
COPY . .

# Build the CLI in release mode (caches Rust dependencies in a layer).
RUN cargo build --release -p buff-lang-cli

# Install the `buff` binary to /usr/local/bin.
RUN cargo install --path crates/buff-lang-cli --locked --root /usr/local

# ── Final stage ──────────────────────────────────────────────────────────────
FROM debian:bookworm-slim@sha256:7b140f374b289a7c2befc338f42ebe6441b7ea838a042bbd5acbfca6ec875818

RUN apt-get update -y \
 && apt-get install -y --no-install-recommends \
    ca-certificates \
 && rm -rf /var/lib/apt/lists/*

COPY --from=build /usr/local/bin/buff /usr/local/bin/buff

RUN groupadd --gid 1000 buff \
 && useradd --uid 1000 --gid buff --create-home --shell /bin/bash buff

USER buff
WORKDIR /app

ENTRYPOINT ["buff"]

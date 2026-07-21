# syntax=docker/dockerfile:1
# buff:slim — Minimal runtime image for Buff-built binaries.
# No compiler, no build tools — just libc + ca-certificates + non-root user.
#
# Expected size: ~90MB (debian:bookworm-slim ~80MB + ca-certificates ~10MB).
# Use as the final stage in a multi-stage build (see Dockerfile.example).

FROM debian:bookworm-slim

RUN apt-get update -y \
 && apt-get install -y --no-install-recommends \
    ca-certificates \
 && rm -rf /var/lib/apt/lists/*

RUN groupadd --gid 1000 buff \
 && useradd --uid 1000 --gid buff --create-home --shell /bin/bash buff

USER buff
WORKDIR /app

# No ENTRYPOINT — this is a base image; the user adds their own binary + entrypoint.

# syntax=docker/dockerfile:1
# buff:slim — Minimal runtime image for Buff-built binaries.
# No compiler, no build tools — just libc + ca-certificates + non-root user.
#
# Expected size: ~90MB (debian:bookworm-slim ~80MB + ca-certificates ~10MB).
# Use as the final stage in a multi-stage build (see Dockerfile.example).
#
# P0.10 — base image pinned by digest (sha256:) for supply-chain protection.
# To bump: pull the new tag, capture its `docker manifest inspect <tag>` digest,
# and replace BOTH the tag and the digest below in lock-step.

FROM debian:bookworm-slim@sha256:7b140f374b289a7c2befc338f42ebe6441b7ea838a042bbd5acbfca6ec875818

RUN apt-get update -y \
 && apt-get install -y --no-install-recommends \
    ca-certificates \
 && rm -rf /var/lib/apt/lists/*

RUN groupadd --gid 1000 buff \
 && useradd --uid 1000 --gid buff --create-home --shell /bin/bash buff

USER buff
WORKDIR /app

# No ENTRYPOINT — this is a base image; the user adds their own binary + entrypoint.

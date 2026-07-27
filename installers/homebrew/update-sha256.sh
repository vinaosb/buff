#!/usr/bin/env bash
# update-sha256.sh — Compute SHA-256 hashes for Homebrew formula from release artifacts.
#
# Called by the release workflow AFTER all build artifacts are assembled but
# BEFORE the GitHub Release is published.  Reads the formula template, downloads
# each platform archive from the current release, computes its SHA-256, and
# writes the result back into the formula file.
#
# Usage:
#   ./installers/homebrew/update-sha256.sh <version>   # e.g. 1.38.0
#
# The script modifies installers/homebrew/buff.rb in-place.
# It is idempotent — re-running with the same version is a no-op.

set -euo pipefail

if [ $# -ne 1 ]; then
  echo "Usage: $0 <version>  (e.g. 1.38.0)"
  exit 1
fi

VERSION="$1"
FORMULA="$(dirname "$0")/buff.rb"
REPO="buff-lang/buff"
TAG="v${VERSION}"

# Platforms that the Homebrew formula covers (4 tarballs).
PLATFORMS=(
  "macos-arm64"
  "macos-x64"
  "linux-arm64"
  "linux-x64"
)

# ------------------------------------------------------------------
# 1.  Verify the formula exists and the version matches.
# ------------------------------------------------------------------
if [ ! -f "$FORMULA" ]; then
  echo "ERROR: Formula not found at $FORMULA"
  exit 1
fi

CURRENT_VERSION="$(sed -n 's/^  version "\(.*\)"$/\1/p' "$FORMULA")"
if [ "$CURRENT_VERSION" != "$VERSION" ]; then
  echo "INFO:  Bumping formula version $CURRENT_VERSION → $VERSION"
  sed -i.bak "s/^  version \".*\"/  version \"$VERSION\"/" "$FORMULA"
  rm -f "${FORMULA}.bak"
fi

# ------------------------------------------------------------------
# 2.  Download each archive and compute its SHA-256.
# ------------------------------------------------------------------
TMPDIR="$(mktemp -d)"
trap 'rm -rf "$TMPDIR"' EXIT

for PLATFORM in "${PLATFORMS[@]}"; do
  ARCHIVE="buff-${TAG}-${PLATFORM}.tar.gz"
  URL="https://github.com/${REPO}/releases/download/${TAG}/${ARCHIVE}"

  echo "INFO:  Fetching ${URL}"
  curl -sSfL -o "${TMPDIR}/${ARCHIVE}" "$URL"

  HASH="$(sha256sum "${TMPDIR}/${ARCHIVE}" | cut -d' ' -f1)"
  echo "INFO:  ${PLATFORM} → sha256 ${HASH}"

  # Replace the sha256 line for this platform in the formula.
  # The formula has blocks like:
  #   on_arm do
  #     url "...-macos-arm64.tar.gz"
  #     sha256 "..."
  #   end
  #
  # We match the URL line to find the right block, then replace the
  # sha256 line that follows it.
  ESCAPED_ARCHIVE="$(printf '%s\n' "$ARCHIVE" | sed 's/[.[\*^$]/\\&/g')"
  sed -i.bak -E "
    /${ESCAPED_ARCHIVE}/{
      n
      s/sha256 \".*\"/sha256 \"${HASH}\"/
    }
  " "$FORMULA"
  rm -f "${FORMULA}.bak"
done

echo "DONE:  ${FORMULA} updated for ${TAG}"

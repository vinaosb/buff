"use strict";
/**
 * Version string parsing helpers.
 *
 * Buff versions follow a relaxed semver format: "1.0.0", "0.5.0", "latest".
 * We parse them into components for cache key construction and comparison.
 */
Object.defineProperty(exports, "__esModule", { value: true });
exports.parseVersion = parseVersion;
exports.buildCacheKey = buildCacheKey;
exports.buildRestoreKeysFallback = buildRestoreKeysFallback;
/**
 * Parse a version string into SemVer components.
 * Returns null for "latest" or unparseable strings.
 *
 * @param version - Version string like "1.0.0", "1.2.3-alpha.1"
 * @returns SemVer object or null
 */
function parseVersion(version) {
    if (version === "latest" || version === "") {
        return null;
    }
    const cleaned = version.replace(/^v/, "");
    const re = /^(\d+)\.(\d+)\.(\d+)(?:-([a-zA-Z0-9._+-]+))?$/;
    const match = cleaned.match(re);
    if (!match) {
        return null;
    }
    return {
        major: parseInt(match[1], 10),
        minor: parseInt(match[2], 10),
        patch: parseInt(match[3], 10),
        prerelease: match[4] ?? null,
    };
}
/**
 * Build a cache key from OS, arch, and buff version.
 *
 * @param os - runner.os (e.g. "Linux", "Windows", "macOS")
 * @param arch - runner.arch (e.g. "X64", "ARM64")
 * @param buffVersion - Buff version string
 * @returns Cache key string
 */
function buildCacheKey(os, arch, buffVersion) {
    const versionPart = buffVersion === "latest" ? "latest" : buffVersion;
    return `${os}-${arch}-buff-${versionPart}`;
}
/**
 * Build a restore-keys fallback for cache lookup.
 *
 * @param os - runner.os
 * @param arch - runner.arch
 * @returns Restore-keys fallback string
 */
function buildRestoreKeysFallback(os, arch) {
    return [`${os}-${arch}-buff-`];
}

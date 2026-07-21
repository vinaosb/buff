import { describe, it, expect } from "@jest/globals";
import {
  parseVersion,
  buildCacheKey,
  buildRestoreKeysFallback,
} from "../src/helpers/version";

describe("parseVersion", () => {
  it("parses a full semver string", () => {
    const result = parseVersion("1.0.0");
    expect(result).toEqual({
      major: 1,
      minor: 0,
      patch: 0,
      prerelease: null,
    });
  });

  it("parses a version with prerelease", () => {
    const result = parseVersion("1.2.3-alpha.1");
    expect(result).toEqual({
      major: 1,
      minor: 2,
      patch: 3,
      prerelease: "alpha.1",
    });
  });

  it("parses a version with v prefix", () => {
    const result = parseVersion("v0.5.0");
    expect(result).toEqual({
      major: 0,
      minor: 5,
      patch: 0,
      prerelease: null,
    });
  });

  it("returns null for 'latest'", () => {
    expect(parseVersion("latest")).toBeNull();
  });

  it("returns null for empty string", () => {
    expect(parseVersion("")).toBeNull();
  });

  it("returns null for invalid version string", () => {
    expect(parseVersion("not-a-version")).toBeNull();
  });

  it("returns null for partial version", () => {
    expect(parseVersion("1.0")).toBeNull();
  });

  it("parses version with complex prerelease (including build metadata)", () => {
    const result = parseVersion("0.1.0-rc.2+build.123");
    expect(result).toEqual({
      major: 0,
      minor: 1,
      patch: 0,
      prerelease: "rc.2+build.123",
    });
  });
});

describe("buildCacheKey", () => {
  it("builds key from os, arch, and version", () => {
    const key = buildCacheKey("Linux", "X64", "1.0.0");
    expect(key).toBe("Linux-X64-buff-1.0.0");
  });

  it("uses 'latest' for latest version", () => {
    const key = buildCacheKey("Windows", "ARM64", "latest");
    expect(key).toBe("Windows-ARM64-buff-latest");
  });

  it("handles macOS", () => {
    const key = buildCacheKey("macOS", "X64", "0.5.0");
    expect(key).toBe("macOS-X64-buff-0.5.0");
  });
});

describe("buildRestoreKeysFallback", () => {
  it("returns array with prefix fallback", () => {
    const keys = buildRestoreKeysFallback("Linux", "X64");
    expect(keys).toEqual(["Linux-X64-buff-"]);
  });

  it("handles Windows", () => {
    const keys = buildRestoreKeysFallback("Windows", "ARM64");
    expect(keys).toEqual(["Windows-ARM64-buff-"]);
  });
});

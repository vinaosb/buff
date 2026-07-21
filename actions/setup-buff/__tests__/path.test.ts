import { describe, it, expect } from "@jest/globals";
import {
  getBuffBinDir,
  getBuffVersionsDir,
  getBuffHomeDir,
  getGithubPathLine,
} from "../src/helpers/path";

describe("getBuffBinDir", () => {
  it("returns path ending with .buff/bin", () => {
    const dir = getBuffBinDir();
    expect(dir).toMatch(/[\\/]\.buff[\\/]bin$/);
  });
});

describe("getBuffVersionsDir", () => {
  it("returns path ending with .buff/versions", () => {
    const dir = getBuffVersionsDir();
    expect(dir).toMatch(/[\\/]\.buff[\\/]versions$/);
  });
});

describe("getBuffHomeDir", () => {
  it("returns path ending with .buff", () => {
    const dir = getBuffHomeDir();
    expect(dir).toMatch(/[\\/]\.buff$/);
  });
});

describe("getGithubPathLine", () => {
  it("returns path with EOL suffix", () => {
    const line = getGithubPathLine();
    expect(line).toMatch(/[\\/]\.buff[\\/]bin\r?\n$/);
  });
});

import { describe, it, expect } from "@jest/globals";
import { getInstallScriptUrl, getBuffupDownloadUrl } from "../src/helpers/url";

describe("getInstallScriptUrl", () => {
  it("returns install.sh for linux with latest version", () => {
    const url = getInstallScriptUrl("linux", "latest");
    expect(url).toBe("https://buffup.buff-lang.dev/install.sh");
  });

  it("returns install.sh for darwin with latest version", () => {
    const url = getInstallScriptUrl("darwin", "latest");
    expect(url).toBe("https://buffup.buff-lang.dev/install.sh");
  });

  it("returns install.ps1 for win32 with latest version", () => {
    const url = getInstallScriptUrl("win32", "latest");
    expect(url).toBe("https://buffup.buff-lang.dev/install.ps1");
  });

  it("returns install.sh with version query param for specific version", () => {
    const url = getInstallScriptUrl("linux", "0.1.0");
    expect(url).toBe("https://buffup.buff-lang.dev/install.sh?version=0.1.0");
  });

  it("returns install.ps1 with version query param for specific version", () => {
    const url = getInstallScriptUrl("win32", "0.1.0");
    expect(url).toBe("https://buffup.buff-lang.dev/install.ps1?version=0.1.0");
  });

  it("handles empty string as latest", () => {
    const url = getInstallScriptUrl("linux", "");
    expect(url).toBe("https://buffup.buff-lang.dev/install.sh");
  });

  it("encodes version with special characters", () => {
    const url = getInstallScriptUrl("linux", "1.0.0-beta.1");
    expect(url).toBe(
      "https://buffup.buff-lang.dev/install.sh?version=1.0.0-beta.1",
    );
  });
});

describe("getBuffupDownloadUrl", () => {
  it("returns latest URL for 'latest'", () => {
    const url = getBuffupDownloadUrl("latest");
    expect(url).toBe("https://buffup.buff-lang.dev/latest/buffup");
  });

  it("returns versioned URL for specific version", () => {
    const url = getBuffupDownloadUrl("0.1.0");
    expect(url).toBe(
      "https://buffup.buff-lang.dev/releases/download/v0.1.0/buffup",
    );
  });

  it("handles empty string as latest", () => {
    const url = getBuffupDownloadUrl("");
    expect(url).toBe("https://buffup.buff-lang.dev/latest/buffup");
  });
});

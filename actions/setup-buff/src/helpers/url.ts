/**
 * URL building helpers for the buffup install script.
 *
 * The canonical install URL for buffup is:
 *   https://buffup.buff-lang.dev/install.sh    (Unix)
 *   https://buffup.buff-lang.dev/install.ps1   (Windows)
 *
 * Version-specific installers use a query parameter:
 *   https://buffup.buff-lang.dev/install.sh?version=0.1.0
 */

const BUFFUP_BASE = "https://buffup.buff-lang.dev";

/**
 * Return the install script URL for the given platform and optional buffup version.
 *
 * @param platform - Node.js process.platform value (e.g. "win32", "linux", "darwin")
 * @param buffupVersion - buffup version string or "latest"
 * @returns Fully qualified URL to the install script
 */
export function getInstallScriptUrl(
  platform: string,
  buffupVersion: string,
): string {
  const scriptName = platform === "win32" ? "install.ps1" : "install.sh";
  const baseUrl = `${BUFFUP_BASE}/${scriptName}`;

  if (buffupVersion === "latest" || buffupVersion === "") {
    return baseUrl;
  }

  return `${baseUrl}?version=${encodeURIComponent(buffupVersion)}`;
}

/**
 * Return the buffup binary download URL for a specific version.
 * Used as a fallback / cache key reference.
 *
 * @param version - buffup version string
 * @returns Download URL
 */
export function getBuffupDownloadUrl(version: string): string {
  if (version === "latest" || version === "") {
    return `${BUFFUP_BASE}/latest/buffup`;
  }
  return `${BUFFUP_BASE}/releases/download/v${version}/buffup`;
}

/**
 * PATH manipulation helpers for the setup-buff action.
 *
 * After installing buff, we need to add $HOME/.buff/bin to the PATH
 * so that subsequent steps can invoke `buff` and `buffup`.
 */

import * as os from "node:os";
import * as path from "node:path";

/**
 * Get the path to the Buff bin directory.
 *
 * @returns Absolute path to $HOME/.buff/bin
 */
export function getBuffBinDir(): string {
  return path.join(os.homedir(), ".buff", "bin");
}

/**
 * Get the path to the Buff versions cache directory.
 *
 * @returns Absolute path to $HOME/.buff/versions
 */
export function getBuffVersionsDir(): string {
  return path.join(os.homedir(), ".buff", "versions");
}

/**
 * Get the path to the BUFF_HOME directory.
 *
 * @returns Absolute path to $HOME/.buff
 */
export function getBuffHomeDir(): string {
  return path.join(os.homedir(), ".buff");
}

/**
 * Format a PATH entry line for GITHUB_PATH.
 * The GITHUB_PATH file expects one path per line, terminated by newline.
 *
 * @returns Formatted line to append to GITHUB_PATH
 */
export function getGithubPathLine(): string {
  return `${getBuffBinDir()}${os.EOL}`;
}

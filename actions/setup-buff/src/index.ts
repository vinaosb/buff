/**
 * setup-buff GitHub Action
 *
 * Installs the Buff programming language via buffup, caches the installation,
 * and adds it to PATH for subsequent workflow steps.
 *
 * Usage:
 *   - uses: buff-lang/setup-buff@v1
 *     with:
 *       buff-version: "1.0.0"
 *       buffup-version: "latest"
 */

import * as core from "@actions/core";
import * as exec from "@actions/exec";
import * as io from "@actions/io";
import * as cache from "@actions/cache";
import * as fs from "node:fs";
import * as os from "node:os";
import * as path from "node:path";

import { getInstallScriptUrl } from "./helpers/url";
import { buildCacheKey, buildRestoreKeysFallback } from "./helpers/version";
import {
  getBuffBinDir,
  getBuffVersionsDir,
  getBuffHomeDir,
  getGithubPathLine,
} from "./helpers/path";

/**
 * Download and run the buffup install script.
 */
async function installBuffup(
  platform: string,
  buffupVersion: string,
): Promise<void> {
  const scriptUrl = getInstallScriptUrl(platform, buffupVersion);

  core.info(`Downloading buffup install script from ${scriptUrl}`);

  if (platform === "win32") {
    // PowerShell: download and execute in one step
    const psScript = `[Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12; Invoke-WebRequest -Uri '${scriptUrl}' -OutFile "$env:TEMP\\install-buffup.ps1"; & "$env:TEMP\\install-buffup.ps1"`;
    await exec.exec("powershell", [
      "-NoProfile",
      "-ExecutionPolicy",
      "Bypass",
      "-Command",
      psScript,
    ]);
  } else {
    // Unix: curl | sh — pipe via shell command
    let pipeCmd = `curl -fsSL "${scriptUrl}" | sh -s --`;
    if (buffupVersion !== "latest") {
      pipeCmd += ` --version ${buffupVersion}`;
    }

    const result = await exec.exec("sh", ["-c", pipeCmd]);
    if (result !== 0) {
      throw new Error(`buffup install failed with exit code ${result}`);
    }
  }

  core.info("buffup installed successfully");
}

/**
 * Run `buffup install <version>` to install a specific Buff version.
 */
async function installBuffVersion(version: string): Promise<void> {
  const buffupPath = path.join(getBuffBinDir(), "buffup");
  const args = ["install", version];

  core.info(`Installing Buff version: ${version}`);
  await exec.exec(buffupPath, args);
  core.info(`Buff ${version} installed`);
}

/**
 * Run `buffup default <version>` to set the default Buff version.
 */
async function setDefaultBuffVersion(version: string): Promise<void> {
  const buffupPath = path.join(getBuffBinDir(), "buffup");
  const args = ["default", version];

  core.info(`Setting default Buff version to: ${version}`);
  await exec.exec(buffupPath, args);
  core.info(`Default Buff version set to ${version}`);
}

/**
 * The main action entry point.
 */
async function run(): Promise<void> {
  try {
    const buffVersion = core.getInput("buff-version") || "latest";
    const buffupVersion = core.getInput("buffup-version") || "latest";
    const platform = process.platform;

    core.info(`Platform: ${platform}`);
    core.info(`Buff version: ${buffVersion}`);
    core.info(`Buffup version: ${buffupVersion}`);

    // Validate platform
    if (platform !== "win32" && platform !== "linux" && platform !== "darwin") {
      core.setFailed(`Unsupported platform: ${platform}. Supported: linux, darwin, win32`);
      return;
    }

    // Set BUFF_HOME environment variable
    const buffHome = getBuffHomeDir();
    core.exportVariable("BUFF_HOME", buffHome);
    core.info(`BUFF_HOME set to ${buffHome}`);

    // Ensure .buff/bin directory exists
    const buffBin = getBuffBinDir();
    await io.mkdirP(buffBin);

    // Try to restore from cache first
    const cacheKey = buildCacheKey(
      process.env["RUNNER_OS"] ?? os.type(),
      process.env["RUNNER_ARCH"] ?? os.arch(),
      buffVersion,
    );
    const restoreKeys = buildRestoreKeysFallback(
      process.env["RUNNER_OS"] ?? os.type(),
      process.env["RUNNER_ARCH"] ?? os.arch(),
    );

    const versionsDir = getBuffVersionsDir();
    let cacheHit = false;

    try {
      const restoredKey = await cache.restoreCache(
        [versionsDir],
        cacheKey,
        restoreKeys,
      );
      if (restoredKey) {
        core.info(`Cache restored from key: ${restoredKey}`);
        cacheHit = true;
      } else {
        core.info("No cache found — proceeding with fresh install");
      }
    } catch (cacheErr) {
      core.warning(`Cache restore failed (non-fatal): ${cacheErr}`);
    }

    if (!cacheHit) {
      // Install buffup
      await installBuffup(platform, buffupVersion);

      // Install the requested Buff version
      await installBuffVersion(buffVersion);

      // Set it as default
      await setDefaultBuffVersion(buffVersion);

      // Save to cache
      try {
        await cache.saveCache([versionsDir], cacheKey);
        core.info(`Cache saved with key: ${cacheKey}`);
      } catch (saveErr) {
        core.warning(`Cache save failed (non-fatal): ${saveErr}`);
      }
    }

    // Add .buff/bin to GITHUB_PATH
    const githubPath = process.env["GITHUB_PATH"];
    if (githubPath) {
      fs.appendFileSync(githubPath, getGithubPathLine(), "utf-8");
      core.info(`Added ${buffBin} to GITHUB_PATH`);
    } else {
      core.warning("GITHUB_PATH not set — cannot add to PATH automatically");
    }

    core.info("Buff setup complete");
  } catch (error) {
    const message = error instanceof Error ? error.message : String(error);
    core.setFailed(`Buff setup failed: ${message}`);
    process.exitCode = core.ExitCode.Failure;
  }
}

// Run the action
void run();

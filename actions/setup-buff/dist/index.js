"use strict";
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
var __createBinding = (this && this.__createBinding) || (Object.create ? (function(o, m, k, k2) {
    if (k2 === undefined) k2 = k;
    var desc = Object.getOwnPropertyDescriptor(m, k);
    if (!desc || ("get" in desc ? !m.__esModule : desc.writable || desc.configurable)) {
      desc = { enumerable: true, get: function() { return m[k]; } };
    }
    Object.defineProperty(o, k2, desc);
}) : (function(o, m, k, k2) {
    if (k2 === undefined) k2 = k;
    o[k2] = m[k];
}));
var __setModuleDefault = (this && this.__setModuleDefault) || (Object.create ? (function(o, v) {
    Object.defineProperty(o, "default", { enumerable: true, value: v });
}) : function(o, v) {
    o["default"] = v;
});
var __importStar = (this && this.__importStar) || (function () {
    var ownKeys = function(o) {
        ownKeys = Object.getOwnPropertyNames || function (o) {
            var ar = [];
            for (var k in o) if (Object.prototype.hasOwnProperty.call(o, k)) ar[ar.length] = k;
            return ar;
        };
        return ownKeys(o);
    };
    return function (mod) {
        if (mod && mod.__esModule) return mod;
        var result = {};
        if (mod != null) for (var k = ownKeys(mod), i = 0; i < k.length; i++) if (k[i] !== "default") __createBinding(result, mod, k[i]);
        __setModuleDefault(result, mod);
        return result;
    };
})();
Object.defineProperty(exports, "__esModule", { value: true });
const core = __importStar(require("@actions/core"));
const exec = __importStar(require("@actions/exec"));
const io = __importStar(require("@actions/io"));
const cache = __importStar(require("@actions/cache"));
const fs = __importStar(require("node:fs"));
const os = __importStar(require("node:os"));
const path = __importStar(require("node:path"));
const url_1 = require("./helpers/url");
const version_1 = require("./helpers/version");
const path_1 = require("./helpers/path");
/**
 * Download and run the buffup install script.
 */
async function installBuffup(platform, buffupVersion) {
    const scriptUrl = (0, url_1.getInstallScriptUrl)(platform, buffupVersion);
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
    }
    else {
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
async function installBuffVersion(version) {
    const buffupPath = path.join((0, path_1.getBuffBinDir)(), "buffup");
    const args = ["install", version];
    core.info(`Installing Buff version: ${version}`);
    await exec.exec(buffupPath, args);
    core.info(`Buff ${version} installed`);
}
/**
 * Run `buffup default <version>` to set the default Buff version.
 */
async function setDefaultBuffVersion(version) {
    const buffupPath = path.join((0, path_1.getBuffBinDir)(), "buffup");
    const args = ["default", version];
    core.info(`Setting default Buff version to: ${version}`);
    await exec.exec(buffupPath, args);
    core.info(`Default Buff version set to ${version}`);
}
/**
 * The main action entry point.
 */
async function run() {
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
        const buffHome = (0, path_1.getBuffHomeDir)();
        core.exportVariable("BUFF_HOME", buffHome);
        core.info(`BUFF_HOME set to ${buffHome}`);
        // Ensure .buff/bin directory exists
        const buffBin = (0, path_1.getBuffBinDir)();
        await io.mkdirP(buffBin);
        // Try to restore from cache first
        const cacheKey = (0, version_1.buildCacheKey)(process.env["RUNNER_OS"] ?? os.type(), process.env["RUNNER_ARCH"] ?? os.arch(), buffVersion);
        const restoreKeys = (0, version_1.buildRestoreKeysFallback)(process.env["RUNNER_OS"] ?? os.type(), process.env["RUNNER_ARCH"] ?? os.arch());
        const versionsDir = (0, path_1.getBuffVersionsDir)();
        let cacheHit = false;
        try {
            const restoredKey = await cache.restoreCache([versionsDir], cacheKey, restoreKeys);
            if (restoredKey) {
                core.info(`Cache restored from key: ${restoredKey}`);
                cacheHit = true;
            }
            else {
                core.info("No cache found — proceeding with fresh install");
            }
        }
        catch (cacheErr) {
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
            }
            catch (saveErr) {
                core.warning(`Cache save failed (non-fatal): ${saveErr}`);
            }
        }
        // Add .buff/bin to GITHUB_PATH
        const githubPath = process.env["GITHUB_PATH"];
        if (githubPath) {
            fs.appendFileSync(githubPath, (0, path_1.getGithubPathLine)(), "utf-8");
            core.info(`Added ${buffBin} to GITHUB_PATH`);
        }
        else {
            core.warning("GITHUB_PATH not set — cannot add to PATH automatically");
        }
        core.info("Buff setup complete");
    }
    catch (error) {
        const message = error instanceof Error ? error.message : String(error);
        core.setFailed(`Buff setup failed: ${message}`);
        process.exitCode = core.ExitCode.Failure;
    }
}
// Run the action
void run();

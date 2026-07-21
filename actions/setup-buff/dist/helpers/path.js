"use strict";
/**
 * PATH manipulation helpers for the setup-buff action.
 *
 * After installing buff, we need to add $HOME/.buff/bin to the PATH
 * so that subsequent steps can invoke `buff` and `buffup`.
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
exports.getBuffBinDir = getBuffBinDir;
exports.getBuffVersionsDir = getBuffVersionsDir;
exports.getBuffHomeDir = getBuffHomeDir;
exports.getGithubPathLine = getGithubPathLine;
const os = __importStar(require("node:os"));
const path = __importStar(require("node:path"));
/**
 * Get the path to the Buff bin directory.
 *
 * @returns Absolute path to $HOME/.buff/bin
 */
function getBuffBinDir() {
    return path.join(os.homedir(), ".buff", "bin");
}
/**
 * Get the path to the Buff versions cache directory.
 *
 * @returns Absolute path to $HOME/.buff/versions
 */
function getBuffVersionsDir() {
    return path.join(os.homedir(), ".buff", "versions");
}
/**
 * Get the path to the BUFF_HOME directory.
 *
 * @returns Absolute path to $HOME/.buff
 */
function getBuffHomeDir() {
    return path.join(os.homedir(), ".buff");
}
/**
 * Format a PATH entry line for GITHUB_PATH.
 * The GITHUB_PATH file expects one path per line, terminated by newline.
 *
 * @returns Formatted line to append to GITHUB_PATH
 */
function getGithubPathLine() {
    return `${getBuffBinDir()}${os.EOL}`;
}

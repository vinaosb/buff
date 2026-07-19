import * as fs from 'fs';
import * as path from 'path';
import { ChildProcess, spawn } from 'child_process';
import * as vscode from 'vscode';
import {
    LanguageClient,
    LanguageClientOptions,
    ServerOptions,
    TransportKind,
    Trace,
} from 'vscode-languageclient/node';

/**
 * VSCode extension for the Buff language.
 *
 * Wires three already-shipped components:
 *   - tree-sitter-buff (T115)  -> consumed via the TextMate grammar in
 *     syntaxes/buff.tmLanguage.json (VSCode's native TextMate highlighting is
 *     the standard, lighter path; tree-sitter would require the proposed API
 *     and an extra dependency).
 *   - buff-lsp (T117)          -> launched over stdio via this module.
 *   - buff CLI (buff-lang-cli) -> driven by the buff.run / buff.build /
 *     buff.check commands.
 */

let client: LanguageClient | undefined;
let outputChannel: vscode.OutputChannel | undefined;

const SERVER_BINARY_NAMES = process.platform === 'win32'
    ? ['buff-lsp.exe', 'buff-lsp']
    : ['buff-lsp'];

const CLI_BINARY_NAMES = process.platform === 'win32'
    ? ['buff.exe', 'buff']
    : ['buff'];

// ---------------------------------------------------------------------------
// Activation
// ---------------------------------------------------------------------------

export async function activate(context: vscode.ExtensionContext): Promise<void> {
    outputChannel = vscode.window.createOutputChannel('Buff');
    context.subscriptions.push(outputChannel);

    const serverChannel = vscode.window.createOutputChannel('Buff Language Server');
    context.subscriptions.push(serverChannel);

    // Boot the language client (stdio transport). Tolerate failure so the
    // run/build/check commands still work even if buff-lsp is not yet built.
    try {
        client = await startLanguageServer(context, serverChannel);
    } catch (err) {
        serverChannel.appendLine(
            `[buff] language server startup failed: ${formatError(err)}`,
        );
        client = undefined;
    }

    // Register commands. Each command is gated on a Buff document being active.
    context.subscriptions.push(
        vscode.commands.registerCommand('buff.run', () => runCurrentFile(context)),
        vscode.commands.registerCommand('buff.build', () => buildCurrentFile(context)),
        vscode.commands.registerCommand('buff.check', () => checkCurrentFile(context)),
        vscode.commands.registerCommand('buff.restartServer', () => restartServer(context, serverChannel)),
    );

    // Wire buff.formatOnSave -> editor.formatOnSave for the [buff] language.
    await applyFormatOnSave();
    context.subscriptions.push(
        vscode.workspace.onDidChangeConfiguration(e => {
            if (e.affectsConfiguration('buff.formatOnSave')) {
                applyFormatOnSave().catch(err => {
                    outputChannel?.appendLine(`[buff] failed to apply formatOnSave: ${err}`);
                });
            }
        }),
    );

    // Sync LSP trace verbosity from configuration.
    applyServerTrace();
    context.subscriptions.push(
        vscode.workspace.onDidChangeConfiguration(e => {
            if (e.affectsConfiguration('buff.trace.server')) {
                applyServerTrace();
            }
        }),
    );
}

export async function deactivate(): Promise<void> {
    if (client) {
        await client.stop();
        client = undefined;
    }
}

// ---------------------------------------------------------------------------
// Language server
// ---------------------------------------------------------------------------

async function startLanguageServer(
    _context: vscode.ExtensionContext,
    serverChannel: vscode.OutputChannel,
): Promise<LanguageClient> {
    const serverPath = await resolveServerPath();
    if (!serverPath) {
        const msg = [
            'Buff language server not found.',
            '',
            'Build it with:',
            '  cargo build --release -p buff-lsp',
            '',
            'Then either:',
            '  - put `buff-lsp` on your PATH, or',
            '  - open the repo root as your workspace folder, or',
            '  - set "buff.serverPath" to the absolute path of the binary.',
        ].join('\n');
        void vscode.window.showWarningMessage(msg, 'Open Settings').then(action => {
            if (action === 'Open Settings') {
                void vscode.commands.executeCommand('workbench.action.openSettings', 'buff.serverPath');
            }
        });
        serverChannel.appendLine(msg);
        // We return a stub client below; this is unreachable in practice
        // because resolveServerPath throws on missing. Guard anyway.
    }

    const serverOptions: ServerOptions = {
        run: {
            command: serverPath ?? 'buff-lsp',
            transport: TransportKind.stdio,
        },
        debug: {
            command: serverPath ?? 'buff-lsp',
            transport: TransportKind.stdio,
        },
    };

    const clientOptions: LanguageClientOptions = {
        documentSelector: [
            { scheme: 'file', language: 'buff' },
        ],
        outputChannel: serverChannel,
        traceOutputChannel: serverChannel,
        synchronize: {
            // Notify the server about .buff file changes outside VSCode too.
            fileEvents: vscode.workspace.createFileSystemWatcher('**/*.buff'),
        },
        markdown: {
            isTrusted: false,
            supportHtml: false,
        },
    };

    const languageClient = new LanguageClient(
        'buff',
        'Buff Language Server',
        serverOptions,
        clientOptions,
    );

    // The 9.x API resolves client.start() on initialize.
    try {
        await languageClient.start();
    } catch (err) {
        serverChannel.appendLine(`[buff] language server failed to start: ${formatError(err)}`);
        throw err;
    }

    return languageClient;
}

async function restartServer(
    context: vscode.ExtensionContext,
    serverChannel: vscode.OutputChannel,
): Promise<void> {
    if (client) {
        await client.stop();
        client = undefined;
    }
    try {
        client = await startLanguageServer(context, serverChannel);
        void vscode.window.showInformationMessage('Buff language server restarted.');
    } catch (err) {
        void vscode.window.showErrorMessage(`Failed to restart Buff language server: ${formatError(err)}`);
    }
}

/**
 * Resolve the buff-lsp binary path.
 *
 * Search order:
 *   1. `buff.serverPath` configuration value (if set and exists).
 *   2. `target/release/buff-lsp[.exe]` in each workspace folder.
 *   3. `buff-lsp[.exe]` on PATH.
 */
async function resolveServerPath(): Promise<string | undefined> {
    const config = vscode.workspace.getConfiguration('buff');
    const configured = config.get<string>('serverPath')?.trim();
    if (configured) {
        if (fs.existsSync(configured)) {
            return configured;
        }
        outputChannel?.appendLine(
            `[buff] buff.serverPath is set but does not exist: ${configured}; falling back.`,
        );
    }

    for (const folder of workspaceFolderRoots()) {
        for (const name of SERVER_BINARY_NAMES) {
            const candidate = path.join(folder, 'target', 'release', name);
            if (fs.existsSync(candidate)) {
                return candidate;
            }
        }
    }

    // Fall back to bare name on PATH.
    if (await isOnPath(SERVER_BINARY_NAMES)) {
        return SERVER_BINARY_NAMES[0];
    }

    return undefined;
}

/**
 * Resolve the buff CLI binary path (for run/build/check commands).
 *
 * Search order mirrors resolveServerPath but for the CLI binary.
 */
async function resolveCliPath(): Promise<string | undefined> {
    const config = vscode.workspace.getConfiguration('buff');
    const configured = config.get<string>('binaryPath')?.trim();
    if (configured) {
        if (fs.existsSync(configured)) {
            return configured;
        }
        outputChannel?.appendLine(
            `[buff] buff.binaryPath is set but does not exist: ${configured}; falling back.`,
        );
    }

    for (const folder of workspaceFolderRoots()) {
        for (const name of CLI_BINARY_NAMES) {
            const candidate = path.join(folder, 'target', 'release', name);
            if (fs.existsSync(candidate)) {
                return candidate;
            }
        }
    }

    if (await isOnPath(CLI_BINARY_NAMES)) {
        return CLI_BINARY_NAMES[0];
    }

    return undefined;
}

function workspaceFolderRoots(): string[] {
    const folders = vscode.workspace.workspaceFolders ?? [];
    return folders.map(f => f.uri.fsPath);
}

async function isOnPath(names: string[]): Promise<boolean> {
    return new Promise(resolve => {
        // Use `where` on Windows, `which` on Unix. Both exit 0 if found.
        const cmd = process.platform === 'win32' ? 'where' : 'which';
        const proc = spawn(cmd, [names[0]], { shell: false });
        proc.on('error', () => resolve(false));
        proc.on('exit', code => resolve(code === 0));
    });
}

function applyServerTrace(): void {
    if (!client) {
        return;
    }
    const setting = vscode.workspace
        .getConfiguration('buff')
        .get<'off' | 'messages' | 'verbose'>('trace.server', 'off');
    const trace = setting === 'verbose' ? Trace.Verbose
        : setting === 'messages' ? Trace.Messages
        : Trace.Off;
    client.setTrace(trace).catch(err => {
        outputChannel?.appendLine(`[buff] failed to set trace: ${formatError(err)}`);
    });
}

// ---------------------------------------------------------------------------
// Commands: buff.run / buff.build / buff.check
// ---------------------------------------------------------------------------

function activeBuffDocument(): vscode.TextDocument | undefined {
    const editor = vscode.window.activeTextEditor;
    if (!editor) {
        void vscode.window.showErrorMessage('Buff: no active editor.');
        return undefined;
    }
    if (editor.document.languageId !== 'buff' && !editor.document.fileName.endsWith('.buff')) {
        void vscode.window.showErrorMessage('Buff: active file is not a Buff source.');
        return undefined;
    }
    return editor.document;
}

async function runCurrentFile(_context: vscode.ExtensionContext): Promise<void> {
    await runCliCommand('run');
}

async function buildCurrentFile(_context: vscode.ExtensionContext): Promise<void> {
    await runCliCommand('build');
}

async function checkCurrentFile(_context: vscode.ExtensionContext): Promise<void> {
    await runCliCommand('check');
}

/**
 * Drive the `buff` CLI: `buff <subcommand> <file>`.
 *
 * Streams stdout + stderr into the `Buff` OutputChannel. A terminal is used
 * for `run` so interactive programs work; `build` and `check` go to the
 * OutputChannel (compiler output, structured).
 */
async function runCliCommand(
    subcommand: 'run' | 'build' | 'check',
): Promise<void> {
    const doc = activeBuffDocument();
    if (!doc) {
        return;
    }
    await doc.save();

    const binary = await resolveCliPath();
    if (!binary) {
        const msg = [
            'Buff CLI binary not found.',
            '',
            'Build it with:',
            '  cargo build --release -p buff-lang-cli',
            '',
            'Then either:',
            '  - put `buff` on your PATH, or',
            '  - open the repo root as your workspace folder, or',
            '  - set "buff.binaryPath" to the absolute path of the binary.',
        ].join('\n');
        void vscode.window.showErrorMessage(msg, 'Open Settings').then(action => {
            if (action === 'Open Settings') {
                void vscode.commands.executeCommand('workbench.action.openSettings', 'buff.binaryPath');
            }
        });
        return;
    }

    if (subcommand === 'run') {
        runInTerminal(binary, [subcommand, doc.fileName]);
    } else {
        runInOutputChannel(binary, [subcommand, doc.fileName], subcommand);
    }
}

function runInTerminal(binary: string, args: string[]): void {
    const terminal = vscode.window.createTerminal({
        name: 'Buff',
        cwd: path.dirname(args[args.length - 1]),
    });
    terminal.show(true);
    const quoted = [quoteShell(binary), ...args.map(quoteShell)].join(' ');
    terminal.sendText(quoted, true);
}

function runInOutputChannel(
    binary: string,
    args: string[],
    label: string,
): void {
    if (!outputChannel) {
        outputChannel = vscode.window.createOutputChannel('Buff');
    }
    outputChannel.show(true);
    outputChannel.appendLine(`$ ${binary} ${args.join(' ')}`);

    const cwd = path.dirname(args[args.length - 1]);
    let proc: ChildProcess;
    try {
        proc = spawn(binary, args, { cwd });
    } catch (err) {
        outputChannel.appendLine(`[buff] failed to spawn: ${formatError(err)}`);
        return;
    }

    proc.stdout?.on('data', chunk => {
        outputChannel!.append(chunk.toString());
    });
    proc.stderr?.on('data', chunk => {
        outputChannel!.append(chunk.toString());
    });
    proc.on('error', err => {
        outputChannel!.appendLine(`[buff] ${label} failed: ${formatError(err)}`);
    });
    proc.on('exit', (code, signal) => {
        if (code === 0) {
            outputChannel!.appendLine(`[buff] ${label} succeeded.`);
        } else if (signal) {
            outputChannel!.appendLine(`[buff] ${label} killed by signal ${signal}.`);
        } else {
            outputChannel!.appendLine(`[buff] ${label} exited with code ${code}.`);
        }
    });
}

function quoteShell(arg: string): string {
    // Minimal quoting sufficient for our use (paths and known CLI args).
    if (process.platform === 'win32') {
        if (/[\s"']/.test(arg)) {
            return `"${arg.replace(/"/g, '\\"')}"`;
        }
        return arg;
    }
    if (/[\s"'\\$`]/.test(arg)) {
        return `'${arg.replace(/'/g, `'\\''`)}'`;
    }
    return arg;
}

// ---------------------------------------------------------------------------
// Format on Save
// ---------------------------------------------------------------------------

/**
 * When `buff.formatOnSave` is true, propagate the value into the
 * `[buff]` language-specific editor.formatOnSave override. This lets the user
 * opt-in once, in the Buff settings section, without having to know about
 * language overrides. The actual formatting is performed by buff-lsp
 * (which routes through `buff fmt`).
 */
async function applyFormatOnSave(): Promise<void> {
    const buffConfig = vscode.workspace.getConfiguration('buff');
    const enabled = buffConfig.get<boolean>('formatOnSave', false);

    // Mirror to editor.formatOnSave for [buff] language so the LSP formatter
    // (which VSCode auto-detects via documentFormattingProvider capability)
    // runs on save without further user setup.
    const editorCfg = vscode.workspace.getConfiguration('editor', { languageId: 'buff' });
    const current = editorCfg.inspect('formatOnSave')?.globalLanguageValue;
    if (current !== enabled) {
        await editorCfg.update(
            'formatOnSave',
            enabled,
            vscode.ConfigurationTarget.Global,
            true,
        );
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

function formatError(err: unknown): string {
    if (err instanceof Error) {
        return err.message;
    }
    return String(err);
}

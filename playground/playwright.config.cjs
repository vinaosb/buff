// @ts-check
// Playwright config for the Buff playground (T114).
//
// Runs against a LOCAL static server (python -m http.server 8092) — NOT a
// public URL. The deliverable per the task is local-only; deploy is a
// later user action documented in playground/README.md.
//
// Tests live alongside this config in `tests/` and use the fixtures in
// `tests/fixtures/`. Tests are DOM-asserting (per the plan: "DOM assertions,
// not visual") but include a screenshot capture for evidence.

const { defineConfig, devices } = require('@playwright/test');

const PORT = process.env.BUFF_PLAYGROUND_PORT || '8092';
const BASE_URL = `http://127.0.0.1:${PORT}/`;

module.exports = defineConfig({
    testDir: './tests',
    timeout: 30_000,
    expect: { timeout: 5_000 },
    fullyParallel: false,           // single-server, avoid port races
    retries: 0,
    workers: 1,
    reporter: [
        ['list'],
        ['junit', { outputFile: 'test-results/junit.xml' }],
        ['html',   { outputFolder: 'test-results/html', open: 'never' }],
    ],
    use: {
        baseURL: BASE_URL,
        headless: true,
        screenshot: 'only-on-failure',
        trace: 'retain-on-failure',
        // Generous action timeout — wasm load on first paint can be slow.
        actionTimeout: 10_000,
    },
    projects: [
        {
            name: 'chromium',
            use: { ...devices['Desktop Chrome'] },
        },
    ],
    // The webServer hook auto-starts the static file server if it isn't
    // already running, and tears it down at the end. Reuses port 8092 —
    // keep it in sync with BASE_URL above.
    webServer: {
        command: 'python -m http.server 8092 --bind 127.0.0.1',
        port: 8092,
        cwd: __dirname,
        reuseExistingServer: true,
        timeout: 30_000,
    },
});

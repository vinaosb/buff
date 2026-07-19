// @ts-check
// Playwright config for the Buff marketing website (T116).
//
// Runs against a LOCAL static server (python -m http.server 8093) serving
// from the website/ directory. Port 8093 avoids collision with playground
// (which uses 8092). Tests live in tests/.

const { defineConfig, devices } = require('@playwright/test');

const PORT = process.env.BUFF_WEBSITE_PORT || '8093';
const BASE_URL = `http://127.0.0.1:${PORT}/`;

module.exports = defineConfig({
    testDir: './tests',
    timeout: 30_000,
    expect: { timeout: 5_000 },
    fullyParallel: false,
    retries: 0,
    workers: 1,
    reporter: [
        ['list'],
    ],
    use: {
        baseURL: BASE_URL,
        headless: true,
        screenshot: 'only-on-failure',
        trace: 'retain-on-failure',
        actionTimeout: 10_000,
    },
    projects: [
        {
            name: 'chromium',
            use: { ...devices['Desktop Chrome'] },
        },
    ],
    webServer: {
        command: 'python -m http.server 8093 --bind 127.0.0.1',
        port: 8093,
        cwd: __dirname,
        reuseExistingServer: true,
        timeout: 30_000,
    },
});

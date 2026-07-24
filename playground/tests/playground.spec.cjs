// Playwright tests for the Buff playground (T114).
//
// QA scenarios from the plan:
//   1. Transpile fibonacci.buff  -> right pane contains "fn " and "fibonacci"
//   2. Error display              -> invalid input shows error message below editor
//   3. URL sharing                -> encode source into URL fragment, restore on reload
//
// DOM-asserting (per the plan: "DOM assertions, not visual"). Screenshots
// captured for evidence under .sisyphus/evidence/.

const { test, expect } = require('@playwright/test');
const fs = require('fs');
const path = require('path');

// ──────────────────────────────────────────────────────────────────────
//  Fixtures
// ──────────────────────────────────────────────────────────────────────

const FIBONACCI_BUFF = fs.readFileSync(
    path.resolve(__dirname, '../../examples/fibonacci.buff'),
    'utf8'
);

const EVIDENCE_DIR = path.resolve(__dirname, '../../.sisyphus/evidence');

function ensureEvidenceDir() {
    if (!fs.existsSync(EVIDENCE_DIR)) {
        fs.mkdirSync(EVIDENCE_DIR, { recursive: true });
    }
}

/** Playwright locator for the output pane's <code> element. */
function outputCode(page) {
    return page.locator('#output-code');
}

/** Playwright locator for the status pill in the footer. */
function statusPill(page) {
    return page.locator('#status-bar');
}

/** Wait until the playground has finished its initial Wasm load. */
async function waitForReady(page) {
    // The body[data-state] flips from "loading" → "ready" once boot() runs.
    // Status pill text "ok" or "error" indicates the first transpile landed.
    await page.waitForFunction(() => {
        const status = document.getElementById('status-bar');
        const text = document.getElementById('status-text');
        if (!status || !text) return false;
        const s = status.dataset.status;
        return s === 'ok' || s === 'error';
    }, { timeout: 15_000 });
}

/** Replace editor contents via CodeMirror's own API.
 *
 * CodeMirror 5 stores the instance on the `.CodeMirror` wrapper div
 * (NOT the textarea — that's CM6). We call `setValue(value)` directly
 * — that's atomic (no keypress simulation, no debounce races) and the
 * `change` event fires synchronously, so the app's debounce picks it up.
 */
async function setEditorValue(page, value) {
    await page.evaluate((text) => {
        const wrapper = document.querySelector('.CodeMirror');
        if (!wrapper || !wrapper.CodeMirror) {
            throw new Error('CodeMirror instance not attached to .CodeMirror wrapper');
        }
        wrapper.CodeMirror.setValue(text);
        wrapper.CodeMirror.setCursor({ line: 999999, ch: 0 });
    }, value);
}

// ──────────────────────────────────────────────────────────────────────
//  Tests
// ──────────────────────────────────────────────────────────────────────

test.describe('Buff playground — T114', () => {

    test.beforeAll(() => ensureEvidenceDir());

    test.beforeEach(async ({ page }) => {
        await page.goto('/');
        await waitForReady(page);
    });

    // ─── Scenario 1: Transpile fibonacci.buff ──────────────────────────
    test('transpiles fibonacci.buff: output contains `fn ` and `fib`', async ({ page }) => {
        await setEditorValue(page, FIBONACCI_BUFF);

        // Wait for the debounced transpile to land (300ms + jitter).
        await expect(statusPill(page)).toHaveAttribute('data-status', 'ok', { timeout: 5_000 });

        const rustText = await outputCode(page).textContent();
        expect(rustText).toContain('fn ');
        expect(rustText).toContain('fib');

        await page.screenshot({
            path: path.join(EVIDENCE_DIR, 'task-114-transpile-fib.png'),
            fullPage: true,
        });
    });

    // ─── Scenario 2: Error display ─────────────────────────────────────
    test('shows an error message for invalid Buff input', async ({ page }) => {
        // Type the exact invalid input from the plan scenario.
        await setEditorValue(page, 'func ( broken\n');

        // Status pill should flip to "error".
        await expect(statusPill(page)).toHaveAttribute('data-status', 'error', { timeout: 5_000 });

        // The status message in the footer should contain a non-empty error.
        const message = await page.locator('#status-message').textContent();
        expect(message).toBeTruthy();
        expect(message.length).toBeGreaterThan(0);
        // The error prefix from the wire contract is "parse error:".
        expect(message).toContain('parse error');

        // The output pane header should reflect the error state (crimson).
        await expect(page.locator('.pane-output')).toHaveAttribute('data-state', 'error');

        await page.screenshot({
            path: path.join(EVIDENCE_DIR, 'task-114-error-display.png'),
            fullPage: true,
        });
    });

    // ─── Scenario 3: URL sharing round-trip ───────────────────────────
    test('URL fragment round-trip restores the source', async ({ page, baseURL }) => {
        // Type a known unique snippet.
        const snippet = 'func main():\n    print("hello")\n';
        await setEditorValue(page, snippet);

        // Wait for the URL fragment to be written (debounced alongside transpile).
        await expect(statusPill(page)).toHaveAttribute('data-status', 'ok', { timeout: 5_000 });

        // Wait one extra tick for the URL fragment to settle.
        await page.waitForFunction(() => location.hash.startsWith('#s='), null, { timeout: 3_000 });
        const hash = await page.evaluate(() => location.hash);
        expect(hash.startsWith('#s=')).toBe(true);
        expect(hash.length).toBeGreaterThan('#s='.length);

        // Capture the current URL.
        const url = page.url();

        // Open a fresh page (no state) at the captured URL.
        const page2 = await page.context().newPage();
        await page2.goto(url);
        await waitForReady(page2);

        // The editor should contain the same code (modulo trailing newline).
        const restored = await page2.evaluate(() => {
            /* global window */
            // CodeMirror stores the value in its instance; we read via the
            // CM wrapper's textarea sync OR via the .cm-content textContent.
            const cmContent = document.querySelector('.CodeMirror-code');
            if (!cmContent) return null;
            // Concatenate line text from each line div.
            const lines = Array.from(cmContent.querySelectorAll('.CodeMirror-line'));
            return lines.map((l) => (l.textContent || '').replace(/\u200b/g, '')).join('\n');
        });
        expect(restored).toBeTruthy();
        // Check that the editor's restored text contains the print stmt
        // (whitespace may differ slightly due to CM rendering, so use a
        // contains check rather than exact-equality).
        expect(restored).toContain('func main');
        expect(restored).toContain('print');

        // Right pane should also show transpiled Rust (transpile fired on load).
        const rust2 = await outputCode(page2).textContent();
        expect(rust2).toContain('fn ');

        // Evidence: save the captured URL for the audit trail.
        fs.writeFileSync(
            path.join(EVIDENCE_DIR, 'task-114-url-share.txt'),
            `Captured URL: ${url}\nDecoded source:\n${snippet}\nRestored editor content:\n${restored}\n`,
            'utf8'
        );

        await page2.close();
    });

    // ─── Scenario 4: Share button copies URL to clipboard ─────────────
    test('share button writes a URL containing the source to the clipboard', async ({ page, context }) => {
        await context.grantPermissions(['clipboard-read', 'clipboard-write']);

        await setEditorValue(page, 'func main():\n    print("buff")\n');
        await expect(statusPill(page)).toHaveAttribute('data-status', 'ok', { timeout: 5_000 });

        await page.locator('#share-btn').click();

        // The clipboard should contain a URL whose fragment decodes to the source.
        const clip = await page.evaluate(() => navigator.clipboard.readText());
        expect(clip).toContain('#s=');
        expect(clip).toMatch(/https?:\/\//);

        // Decode the fragment and verify it round-trips.
        const decoded = await page.evaluate(async (clipUrl) => {
            const u = new URL(clipUrl);
            const hash = u.hash;
            if (!hash.startsWith('#s=')) return null;
            const b64 = hash.slice(3);
            // UTF-8-safe decode.
            const binary = atob(b64);
            const bytes = new Uint8Array(binary.length);
            for (let i = 0; i < binary.length; i++) bytes[i] = binary.charCodeAt(i);
            return new TextDecoder().decode(bytes);
        }, clip);
        expect(decoded).toContain('func main');
        expect(decoded).toContain('print("buff")');
    });

    // ─── Scenario 5: page load default ────────────────────────────────
    test('loads with the default fibonacci example pre-populated', async ({ page }) => {
        // On a fresh load (no #s= fragment), the page should auto-load
        // fibonacci. (Verified indirectly by the output pane showing fn .)
        const rust = await outputCode(page).textContent();
        expect(rust).toContain('fn ');
        expect(rust).toContain('fib');
    });

    // ─── Scenario 6 (T117): v1.25 example via dropdown ───────────────
    test('v1.25 generics example loads from the dropdown and transpiles', async ({ page }) => {
        // Open the examples dropdown.
        await page.locator('#example-picker').click();
        const menu = page.locator('#example-menu');
        await expect(menu).toBeVisible();

        // The v1.25 generics item must be present (T117 added 7 new items).
        const genericsItem = page.locator('.example-item[data-example="generics"]');
        await expect(genericsItem).toBeVisible();

        // Click it — the editor should swap to the generics snippet and
        // transpile immediately (transpileNow fires on selection).
        await genericsItem.click();
        await expect(menu).toBeHidden(); // menu closes on selection

        // The Rust output must contain the generic struct + generic fn.
        await expect(statusPill(page)).toHaveAttribute('data-status', 'ok', { timeout: 5_000 });
        const rust = await outputCode(page).textContent();
        expect(rust).toContain('struct Pair<T, U>');
        expect(rust).toContain('fn id<T>');

        // The selected item should be marked checked in the menu.
        await page.locator('#example-picker').click();
        await expect(genericsItem).toHaveAttribute('aria-checked', 'true');

        await page.screenshot({
            path: path.join(EVIDENCE_DIR, 'task-117-generics-dropdown.png'),
            fullPage: true,
        });
    });

    // ─── Scenario 7 (T117): dropdown groups are present ───────────────
    test('examples dropdown contains v1.25 group labels and all 11 items', async ({ page }) => {
        await page.locator('#example-picker').click();
        const menu = page.locator('#example-menu');
        await expect(menu).toBeVisible();

        // Group labels — basics + v1.25 language + v1.25 stdlib.
        const labels = await menu.locator('.example-group-label').allTextContents();
        expect(labels).toEqual(['basics', 'v1.25 — language', 'v1.25 — stdlib']);

        // All 11 example items (4 basics + 5 v1.25 language + 2 v1.25 stdlib).
        const items = await menu.locator('.example-item').count();
        expect(items).toBe(11);

        // Specific v1.25 items must be present.
        for (const key of [
            'generics', 'range', 'pattern_matching', 'raw_strings', 'defer',
            'http_client', 'json',
        ]) {
            await expect(menu.locator(`.example-item[data-example="${key}"]`)).toBeVisible();
        }
    });
});

// Playwright tests for the Buff marketing website (T116).
//
// QA scenarios:
//   1. Landing page loads: hero pitch visible, >=5 side-by-side examples
//   2. Side-by-side example renders 3 columns + "Try this" links to playground
//      with correctly encoded Buff source in the URL fragment

const { test, expect } = require('@playwright/test');

test.describe('Buff website — T116', () => {

    test.beforeEach(async ({ page }) => {
        await page.goto('/');
    });

    // ─── Scenario 1: Landing page loads with pitch and examples ───────
    test('landing page displays hero pitch and at least 5 examples', async ({ page }) => {
        // Hero section: the core pitch must be visible.
        const heroKicker = page.locator('.hero-kicker');
        await expect(heroKicker).toContainText('Rust performance');
        await expect(heroKicker).toContainText('Go productivity');

        // The hero body should mention the compiler or borrow checker.
        const heroBody = page.locator('.hero-body');
        await expect(heroBody).toBeVisible();

        // At least 5 example articles with 3-column grids.
        const examples = page.locator('.example');
        const count = await examples.count();
        expect(count).toBeGreaterThanOrEqual(5);

        // Each example should have a title and 3 columns.
        for (let i = 0; i < count; i++) {
            const example = examples.nth(i);
            await expect(example.locator('.example-title')).toBeVisible();

            const grid = example.locator('.example-grid');
            await expect(grid).toBeVisible();

            // 3 columns: Rust, Buff, Why
            const cols = grid.locator('.example-col');
            const colCount = await cols.count();
            expect(colCount).toBe(3);

            // Verify the Rust column has a "Rust" tag.
            await expect(grid.locator('.col-tag-rust')).toBeVisible();
            // Verify the Buff column has a "Buff" tag.
            await expect(grid.locator('.col-tag-buff')).toBeVisible();
            // Verify the Why column has a "Why easier" tag.
            await expect(grid.locator('.col-tag-why')).toBeVisible();
        }

        // Quick start section should be present.
        const quickstart = page.locator('.quickstart');
        await expect(quickstart).toBeVisible();
        await expect(quickstart.locator('.section-heading')).toContainText('Quick start');

        // Quick start should show install command.
        const stepCode = quickstart.locator('.step-code');
        await expect(stepCode.first()).toContainText('cargo install');

        // Playground link in the header nav should exist.
        const playgroundLink = page.locator('.site-nav .nav-btn-accent');
        await expect(playgroundLink).toBeVisible();
        await expect(playgroundLink).toHaveAttribute('href', '../playground/index.html');
    });

    // ─── Scenario 2: Try this links navigate to playground ────────────
    test('"Try this" links encode Buff source into playground URL fragment', async ({ page }) => {
        // Each example should have a "Try this" link.
        const tryLinks = page.locator('a.try-link');
        const linkCount = await tryLinks.count();
        expect(linkCount).toBeGreaterThanOrEqual(5);

        // Pick the first "Try this" link and verify it has an encoded fragment.
        const firstLink = tryLinks.first();
        const href = await firstLink.getAttribute('href');

        // The href should start with the playground path and contain #s=.
        expect(href).toContain('../playground/index.html#s=');

        // The base64 fragment should decode to valid Buff source containing "func".
        const decoded = await page.evaluate((hrefValue) => {
            const hashIndex = hrefValue.indexOf('#s=');
            if (hashIndex === -1) return null;
            const b64 = hrefValue.slice(hashIndex + 3);
            // UTF-8-safe decode (mirrors playground's decodeBase64).
            const binary = atob(b64);
            const bytes = new Uint8Array(binary.length);
            for (var i = 0; i < binary.length; i++) {
                bytes[i] = binary.charCodeAt(i);
            }
            return new TextDecoder().decode(bytes);
        }, href);

        // Decoded source should contain Buff keywords (func, print, etc.).
        expect(decoded).toBeTruthy();
        expect(decoded).toContain('func');

        // Verify all try-links have been wired (none should still have just "#s=").
        for (let i = 0; i < linkCount; i++) {
            const link = tryLinks.nth(i);
            const linkHref = await link.getAttribute('href');
            // Should be longer than just the base path + "#s=".
            expect(linkHref.length).toBeGreaterThan('../playground/index.html#s='.length);
        }
    });
});

import { test } from '@playwright/test';

test.describe('Trades realtime cutover', () => {
  test.fixme('verifies recovering-only replay and gap handling against a deterministic live fixture', async ({ page }) => {
    await page.goto('http://localhost:5000/trades');

    // This cutover contract still needs a deterministic Trades fixture plus
    // a supported realtime inspection surface that Playwright can observe
    // without reaching into implementation details.
  });
});

import { test } from '@playwright/test';

test.describe('Signal realtime cutover', () => {
  test.fixme('verifies invalidate-only recovery and idle websocket behavior against a deterministic live fixture', async ({ page }) => {
    await page.goto('http://localhost:5000/');

    // This cutover contract still needs a deterministic Signal fixture plus
    // a socket/debug surface that Playwright can inspect without reaching
    // into implementation details.
  });
});

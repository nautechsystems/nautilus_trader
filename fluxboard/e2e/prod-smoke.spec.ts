import { test, expect } from '@playwright/test';

// Capture console/page errors to fail on runtime exceptions
test.beforeEach(async ({ page }) => {
  page.on('pageerror', (err) => {
    // Surface any runtime error (e.g., Cannot access 'x' before initialization)
    throw err;
  });
  page.on('console', (msg) => {
    if (msg.type() === 'error') {
      throw new Error(`console.error: ${msg.text()}`);
    }
  });
});

const routes = [
  '/',
  '/pnl',
  '/params',
  '/trades',
  '/balances',
  '/fx',
  '/alerts',
];

test.describe('Production bundle smoke', () => {
  for (const route of routes) {
    test(`loads route ${route} without runtime errors`, async ({ page }) => {
      await page.goto(route);
      // Basic sanity: nav renders and URL matches
      const nav = page.locator('nav');
      await expect(nav).toBeVisible();
      await expect(page).toHaveURL(new RegExp(`${route.replace('/', '\\/')}$`));
    });
  }
});


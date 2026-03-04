// Sound toggle persistence E2E tests

import { test, expect } from '@playwright/test';

test.describe('Sound Toggle Persistence', () => {
  test.beforeEach(async ({ page }) => {
    await page.goto('http://localhost:5000/trades');
    await page.waitForLoadState('networkidle');
  });

  test('sound toggle button exists and works', async ({ page }) => {
    // Find sound toggle button in trades page header
    const soundButton = page.locator('button').filter({ hasText: '🔇' }).or(page.locator('button').filter({ hasText: '🔊' }));

    // Button should be visible (may be muted or enabled)
    await expect(soundButton).toBeVisible();

    // Get initial state
    const initialText = await soundButton.textContent();

    // Click to toggle
    await soundButton.click();

    // Text should change
    const newText = await soundButton.textContent();
    expect(newText).not.toBe(initialText);

    // Should be either 🔇 (muted) or 🔊 (enabled)
    expect(['🔇', '🔊']).toContain(newText);
  });

  test('sound setting persists across page reload', async ({ page }) => {
    // Find sound toggle button
    const soundButton = page.locator('button').filter({ hasText: '🔇' }).or(page.locator('button').filter({ hasText: '🔊' }));
    await expect(soundButton).toBeVisible();

    // Toggle the setting
    const initialText = await soundButton.textContent();
    await soundButton.click();

    // Wait for state to persist (localStorage)
    await page.waitForTimeout(500);

    // Reload the page
    await page.reload();
    await page.waitForLoadState('networkidle');

    // Sound button should maintain the toggled state
    const soundButtonAfterReload = page.locator('button').filter({ hasText: '🔇' }).or(page.locator('button').filter({ hasText: '🔊' }));
    await expect(soundButtonAfterReload).toBeVisible();

    const textAfterReload = await soundButtonAfterReload.textContent();
    expect(textAfterReload).not.toBe(initialText);
  });

  test('sound setting persists across different pages', async ({ page }) => {
    // Start on trades page
    const soundButtonTrades = page.locator('button').filter({ hasText: '🔇' }).or(page.locator('button').filter({ hasText: '🔊' }));
    await expect(soundButtonTrades).toBeVisible();

    // Toggle on trades page
    const initialText = await soundButtonTrades.textContent();
    await soundButtonTrades.click();

    // Navigate to signals page
    await page.goto('http://localhost:5000/signal');
    await page.waitForLoadState('networkidle');

    // Sound setting should persist (though button might not be visible on all pages)
    // Go back to trades to verify
    await page.goto('http://localhost:5000/trades');
    await page.waitForLoadState('networkidle');

    const soundButtonAfterNavigate = page.locator('button').filter({ hasText: '🔇' }).or(page.locator('button').filter({ hasText: '🔊' }));
    await expect(soundButtonAfterNavigate).toBeVisible();

    const textAfterNavigate = await soundButtonAfterNavigate.textContent();
    expect(textAfterNavigate).not.toBe(initialText);
  });

  test('sound button shows correct tooltip', async ({ page }) => {
    // Find sound toggle button
    const soundButton = page.locator('button').filter({ hasText: '🔇' }).or(page.locator('button').filter({ hasText: '🔊' }));
    await expect(soundButton).toBeVisible();

    // Get button title attribute
    const title = await soundButton.getAttribute('title');

    // Should have appropriate tooltip
    expect(title).toMatch(/Trade sounds/);
  });

  test('sound toggle works in both states', async ({ page }) => {
    const soundButton = page.locator('button').filter({ hasText: '🔇' }).or(page.locator('button').filter({ hasText: '🔊' }));
    await expect(soundButton).toBeVisible();

    // Test toggling multiple times
    const states: string[] = [];

    for (let i = 0; i < 4; i++) {
      const currentText = await soundButton.textContent();
      states.push(currentText || '');
      await soundButton.click();
      await page.waitForTimeout(100);
    }

    // Should have seen both states
    expect(states).toContain('🔇');
    expect(states).toContain('🔊');

    // Should alternate between states
    expect(states[0]).not.toBe(states[1]);
    expect(states[1]).not.toBe(states[2]);
    expect(states[2]).not.toBe(states[3]);
  });
});

// PnL report flow E2E tests

import { test, expect } from '@playwright/test';

test.describe('PnL Report Flow', () => {
  test.beforeEach(async ({ page }) => {
    await page.goto('http://localhost:5000/pnl');
    // Wait for page to load
    await page.waitForLoadState('networkidle');
  });

  test('PnL report auto-runs on mount and shows summary and groups table', async ({ page }) => {
    // Report auto-runs on mount, wait for it to complete
    await expect(page.getByText('Running...')).toBeVisible();
    await expect(page.getByText('Running...')).toBeHidden();

    // Should show summary section
    await expect(page.getByText('Overall Summary')).toBeVisible();

    // Should show groups table with PnL Groups header
    await expect(page.getByText('PnL Groups')).toBeVisible();

    // Should have some groups displayed
    const groupsTable = page.locator('table').last();
    await expect(groupsTable).toBeVisible();
  });

  test('expand/collapse groups table works', async ({ page }) => {
    // Report auto-runs on mount, wait for it to complete
    await expect(page.getByText('Running...')).toBeHidden();

    // Find expand button (▼ or ▶)
    const expandButton = page.locator('button:has-text("▼ Collapse")').or(page.locator('button:has-text("▶ Expand")'));
    await expect(expandButton).toBeVisible();

    // Click to collapse
    await expandButton.click();

    // Table should be hidden (only summary visible)
    const groupsTable = page.locator('table').last();
    await expect(groupsTable).not.toBeVisible();

    // Click to expand
    const expandButtonAgain = page.locator('button:has-text("▶ Expand")');
    await expandButtonAgain.click();

    // Table should be visible again
    await expect(groupsTable).toBeVisible();
  });

  test('CSV download button is enabled after report runs', async ({ page }) => {
    // Report auto-runs on mount, wait for it to complete
    await expect(page.getByText('Running...')).toBeHidden();

    // CSV button should be enabled after auto-run
    const csvButton = page.getByRole('button', { name: /CSV/i });
    await expect(csvButton).toBeEnabled();
  });

  test('symbol filtering works', async ({ page }) => {
    // Report auto-runs on mount, wait for it to complete
    await expect(page.getByText('Running...')).toBeHidden();

    // Wait for symbols to appear
    await page.waitForTimeout(500);

    // Find a symbol card and click it
    const symbolCard = page.locator('.rounded.p-4.border-2.cursor-pointer').first();
    if (await symbolCard.isVisible()) {
      const symbolText = await symbolCard.locator('h3').textContent();

      // Click the symbol card
      await symbolCard.click();

      // Should show filter indicator
      await expect(page.getByText(`${symbolText} ✕`)).toBeVisible();

      // Click the X to remove filter
      await page.getByText(`${symbolText} ✕`).click();

      // Filter should be removed
      await expect(page.getByText(`${symbolText} ✕`)).not.toBeVisible();
    }
  });

  test('PnL filters work', async ({ page }) => {
    // Report auto-runs on mount, wait for it to complete
    await expect(page.getByText('Running...')).toBeHidden();

    // Check that filter buttons exist
    await expect(page.getByRole('button', { name: 'All' })).toBeVisible();
    await expect(page.getByRole('button', { name: '+PnL' })).toBeVisible();
    await expect(page.getByRole('button', { name: '-PnL' })).toBeVisible();

    // Click "+PnL" filter
    await page.getByRole('button', { name: '+PnL' }).click();

    // Button should be highlighted (primary accent background)
    const profitableButton = page.getByRole('button', { name: '+PnL' });
    await expect(profitableButton).toHaveClass(/bg-\[#00ffae\]/);

    // Click "All" to reset
    await page.getByRole('button', { name: 'All' }).click();
    await expect(page.getByRole('button', { name: 'All' })).toHaveClass(/bg-\[#00ffae\]/);
  });
});

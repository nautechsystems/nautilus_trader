// Params editing and validation E2E tests

import { test, expect } from '@playwright/test';

test.describe('Params Editing and Validation', () => {
  test.beforeEach(async ({ page }) => {
    await page.goto('http://localhost:5000/params');
    await page.waitForLoadState('networkidle');
  });

  test('load params and display strategy grid', async ({ page }) => {
    // Should show loading initially
    await expect(page.getByText('Loading...')).toBeVisible();

    // Should load strategies
    await expect(page.getByText('Loading...')).toBeHidden();

    // Should show strategy count
    await expect(page.getByText(/\d+ strategies/)).toBeVisible();

    // Should show parameter columns
    await expect(page.getByText('Strategy')).toBeVisible();
    await expect(page.getByText('Run')).toBeVisible();
    await expect(page.getByText('bot_on')).toBeVisible();
    await expect(page.getByText('qty')).toBeVisible();

    // Should have at least one strategy row
    const strategyRows = page.locator('tbody tr');
    await expect(strategyRows.first()).toBeVisible();
  });

  test('edit parameter value and see dirty indicator', async ({ page }) => {
    // Wait for strategies to load
    await expect(page.getByText('Loading...')).toBeHidden();

    // Find first editable cell (skip strategy name and run columns)
    const firstStrategyRow = page.locator('tbody tr').first();
    const qtyCell = firstStrategyRow.locator('td').nth(4); // qty column
    const qtyInput = qtyCell.locator('input');

    await expect(qtyInput).toBeVisible();

    // Get current value
    const originalValue = await qtyInput.inputValue();

    // Edit the value
    await qtyInput.fill('200');

    // Should show dirty indicator (yellow background or similar)
    const cellElement = qtyCell.locator('input').locator('..');
    await expect(cellElement).toHaveClass(/bg-yellow-50/);

    // Save button should appear
    const saveButton = firstStrategyRow.locator('button:has-text("Save")');
    await expect(saveButton).toBeVisible();
  });

  test('save parameter changes and show success', async ({ page }) => {
    // Wait for strategies to load
    await expect(page.getByText('Loading...')).toBeHidden();

    // Find first strategy row
    const firstStrategyRow = page.locator('tbody tr').first();
    const qtyCell = firstStrategyRow.locator('td').nth(4); // qty column
    const qtyInput = qtyCell.locator('input');

    // Edit and save
    await qtyInput.fill('150');
    const saveButton = firstStrategyRow.locator('button:has-text("Save")');
    await saveButton.click();

    // Should show saving state
    await expect(saveButton).toHaveText('Saving...');

    // Should complete and show success (toast or visual feedback)
    await expect(saveButton).toHaveText('Save');
    // Dirty indicator should be gone
    const cellElement = qtyCell.locator('input').locator('..');
    await expect(cellElement).not.toHaveClass(/bg-yellow-50/);
  });

  test('validation error shows on invalid input', async ({ page }) => {
    // Wait for strategies to load
    await expect(page.getByText('Loading...')).toBeHidden();

    // Find qty input
    const firstStrategyRow = page.locator('tbody tr').first();
    const qtyCell = firstStrategyRow.locator('td').nth(4); // qty column
    const qtyInput = qtyCell.locator('input');

    // Enter invalid value (negative number)
    await qtyInput.fill('-100');
    await qtyInput.blur(); // Trigger validation

    // Should show validation error
    const errorElement = qtyCell.locator('.text-red-500');
    await expect(errorElement).toBeVisible();
    await expect(errorElement).toContainText('must be positive');
  });

  test('keyboard navigation works', async ({ page }) => {
    // Wait for strategies to load
    await expect(page.getByText('Loading...')).toBeHidden();

    // Focus first input
    const firstStrategyRow = page.locator('tbody tr').first();
    const firstInput = firstStrategyRow.locator('input').first();
    await firstInput.focus();

    // Press Tab to move to next input
    await page.keyboard.press('Tab');

    // Should focus next input
    const secondInput = firstStrategyRow.locator('input').nth(1);
    await expect(secondInput).toBeFocused();
  });

  test('auto-poll can be toggled', async ({ page }) => {
    // Wait for strategies to load
    await expect(page.getByText('Loading...')).toBeHidden();

    // Find auto checkbox
    const autoCheckbox = page.locator('input[type="checkbox"]').first();
    await expect(autoCheckbox).toBeVisible();

    // Toggle off
    if (await autoCheckbox.isChecked()) {
      await autoCheckbox.click();
    }

    // Should show "Paused" status
    await expect(page.getByText('Paused')).toBeVisible();

    // Toggle back on
    await autoCheckbox.click();

    // Paused status should disappear
    await expect(page.getByText('Paused')).not.toBeVisible();
  });

  test('refresh button reloads data', async ({ page }) => {
    // Wait for strategies to load
    await expect(page.getByText('Loading...')).toBeHidden();

    // Click refresh button
    const refreshButton = page.getByRole('button', { name: 'Refresh' });
    await refreshButton.click();

    // Should show loading again
    await expect(page.getByText('Loading...')).toBeVisible();

    // Should complete loading
    await expect(page.getByText('Loading...')).toBeHidden();
  });
});

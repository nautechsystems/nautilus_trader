import { test, expect } from '@playwright/test';

const PARAMS_URL = 'http://localhost:5000/params';

test.describe('Params auto-refresh pause indicators', () => {
  test.beforeEach(async ({ page }) => {
    await page.goto(PARAMS_URL);
    await page.waitForLoadState('networkidle');
    await expect(page.getByText('Loading parameters...')).toBeHidden();
  });

  test('shows paused labels while editing and when unsaved', async ({ page }) => {
    const firstRow = page.locator('tbody tr').first();
    const qtyInput = firstRow.locator('input').first();
    const originalValue = await qtyInput.inputValue();
    const numericValue = Number(originalValue);
    const newValue = Number.isFinite(numericValue) ? (numericValue + 5).toString() : '5';

    await qtyInput.focus();
    await expect(page.getByText('Paused (editing)')).toBeVisible();

    await qtyInput.fill(newValue);
    await qtyInput.blur();
    await expect(page.getByText('Paused (unsaved changes)')).toBeVisible();

    // Reload the page to discard the unsaved edit and keep the dataset clean
    await page.reload();
    await page.waitForLoadState('networkidle');
    await expect(page.getByText('Loading parameters...')).toBeHidden();
  });
});

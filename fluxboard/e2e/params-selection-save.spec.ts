import { test, expect } from '@playwright/test';

const PARAMS_URL = 'http://localhost:5000/params';

test.describe('Params selection + Save Selected', () => {
  test.beforeEach(async ({ page }) => {
    await page.goto(PARAMS_URL);
    await page.waitForLoadState('networkidle');
    // Initial loading placeholder should disappear
    await expect(page.getByText('Loading parameters...')).toBeHidden();
  });

  test('toolbar appears and Save Selected enables when a row is dirty', async ({ page }) => {
    const firstRow = page.locator('tbody tr').first();
    await firstRow.locator('td').first().click();
    await expect(page.getByText('1 selected')).toBeVisible();

    const qtyInput = firstRow.locator('input').first();
    const originalValue = await qtyInput.inputValue();
    const numericValue = Number(originalValue);
    const newValue = Number.isFinite(numericValue) ? (numericValue + 1).toString() : '1';

    await qtyInput.fill(newValue);
    await qtyInput.blur();

    const saveSelectedButton = page.getByRole('button', { name: 'Save Selected' });
    await expect(saveSelectedButton).toBeVisible();
    await expect(saveSelectedButton).toBeEnabled();

    await saveSelectedButton.click();
    await expect(page.getByText('1 selected')).toBeHidden();

    // Revert the change so downstream tests run against the original data
    await firstRow.locator('td').first().click();
    await expect(page.getByText('1 selected')).toBeVisible();
    const revertInput = firstRow.locator('input').first();
    await revertInput.fill(originalValue);
    await revertInput.blur();

    const revertSaveButton = page.getByRole('button', { name: 'Save Selected' });
    await expect(revertSaveButton).toBeEnabled();
    await revertSaveButton.click();
    await expect(page.getByText('1 selected')).toBeHidden();
  });
});

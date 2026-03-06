import { test, expect } from '@playwright/test';

const PARAMS_URL = 'http://localhost:5000/params';

test.describe('Params sort and filters', () => {
  test.beforeEach(async ({ page }) => {
    await page.goto(PARAMS_URL);
    await page.waitForLoadState('networkidle');
    await expect(page.getByText('Loading parameters...')).toBeHidden();
  });

  test('strategy header cycles aria-sort states and clear sort resets', async ({ page }) => {
    const strategyHeader = page.locator('thead th').first();
    const strategyButton = strategyHeader.getByRole('button', { name: 'Sort by strategy ID' });

    await expect(strategyHeader).toHaveAttribute('aria-sort', 'none');

    await strategyButton.click();
    await expect(strategyHeader).toHaveAttribute('aria-sort', 'ascending');

    await strategyButton.click();
    await expect(strategyHeader).toHaveAttribute('aria-sort', 'descending');

    await strategyButton.click();
    await expect(strategyHeader).toHaveAttribute('aria-sort', 'none');

    const clearSort = page.getByRole('button', { name: 'Clear Sort' });
    // Button enabled only when a sort is active, so toggle again to enable
    await strategyButton.click();
    await expect(clearSort).toBeEnabled();
    await clearSort.click();
    await expect(strategyHeader).toHaveAttribute('aria-sort', 'none');
    await expect(clearSort).toBeDisabled();
  });

  test('filters expand, show controls, and Clear All resets them', async ({ page }) => {
    const filtersToggle = page.getByRole('button', { name: 'Filters' });
    await filtersToggle.click();

    const strategyInput = page.getByPlaceholder('Search strategies, params...');
    const firstStrategy = await page.locator('tbody tr').first().locator('button').first().innerText();
    await strategyInput.fill(firstStrategy.slice(0, 3));

    const statusSelect = page.locator('label:has-text(\"Status\")').locator('..').locator('select');
    await expect(statusSelect).toHaveCount(1);
    await statusSelect.selectOption({ label: 'Running' });

    const clearAll = page.getByRole('button', { name: 'Clear All' });
    await expect(clearAll).toBeVisible();
    await clearAll.click();
    await expect(clearAll).toBeHidden();
  });
});

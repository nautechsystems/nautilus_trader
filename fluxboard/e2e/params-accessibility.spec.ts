import { test, expect } from '@playwright/test';

const PARAMS_URL = 'http://localhost:5000/params';

test.describe('Params accessibility semantics', () => {
  test.beforeEach(async ({ page }) => {
    await page.goto(PARAMS_URL);
    await page.waitForLoadState('networkidle');
    await expect(page.getByText('Loading parameters...')).toBeHidden();
  });

  test('sort headers expose aria-sort and controls have accessible labels', async ({ page }) => {
    const strategyHeader = page.locator('thead th').first();
    await expect(strategyHeader).toHaveAttribute('aria-sort', 'none');

    const sortButton = strategyHeader.getByRole('button', { name: 'Sort by strategy ID' });
    await expect(sortButton).toBeVisible();

    await sortButton.click();
    await expect(strategyHeader).toHaveAttribute('aria-sort', 'ascending');

    await sortButton.click();
    await expect(strategyHeader).toHaveAttribute('aria-sort', 'descending');

    const saveAllButton = page.getByRole('button', { name: /^Save all changes/ });
    await expect(saveAllButton).toBeVisible();
    await expect(saveAllButton).toHaveAttribute('aria-label', /Save all changes/);

    const autoToggle = page.getByLabel('Auto-refresh toggle');
    await expect(autoToggle).toBeVisible();
    await expect(autoToggle).toBeEnabled();
  });
});

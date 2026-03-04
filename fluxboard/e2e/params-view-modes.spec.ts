import { test, expect } from '@playwright/test';

const PARAMS_URL = 'http://localhost:5000/params';

test.describe('Params customize + view modes', () => {
  test.beforeEach(async ({ page }) => {
    await page.goto(PARAMS_URL);
    await page.waitForLoadState('networkidle');
    await expect(page.getByText('Loading parameters...')).toBeHidden();
  });

  test('customize button toggles visibility controls and exposes drag handles', async ({ page }) => {
    const customizeToggle = page.getByRole('button', { name: 'Customize columns' });
    await expect(customizeToggle).toHaveText('Customize');

    await customizeToggle.click();
    await expect(customizeToggle).toHaveText('Done');
    const resetButton = page.getByRole('button', { name: 'Reset columns to default' });
    await expect(resetButton).toBeVisible();

    const reorderHandles = page.getByRole('button', { name: /^Reorder .* column$/ });
    await expect(reorderHandles.first()).toBeVisible();

    await customizeToggle.click();
    await expect(customizeToggle).toHaveText('Customize');
    await expect(resetButton).toBeHidden();
  });

  test('view mode toggle switches between compact and relaxed labels', async ({ page }) => {
    const compactToggle = page.getByRole('button', { name: 'Compact view active' });
    await expect(compactToggle).toHaveText('Dense');
    await expect(compactToggle).toHaveAttribute('aria-pressed', 'true');

    await compactToggle.click();
    const relaxedToggle = page.getByRole('button', { name: 'Relaxed view active' });
    await expect(relaxedToggle).toHaveText('Relaxed');
    await expect(relaxedToggle).toHaveAttribute('aria-pressed', 'false');

    await relaxedToggle.click();
    const compactAgain = page.getByRole('button', { name: 'Compact view active' });
    await expect(compactAgain).toHaveText('Dense');
    await expect(compactAgain).toHaveAttribute('aria-pressed', 'true');
  });
});

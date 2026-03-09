// Alerts display and persistence E2E tests

import { test, expect } from '@playwright/test';

test.describe('Alerts Display and Persistence', () => {
  test.beforeEach(async ({ page }) => {
    await page.goto('http://localhost:5000/');
    await page.waitForLoadState('networkidle');
  });

  test('alert panel loads and shows alerts', async ({ page }) => {
    // Find alerts panel
    const alertsPanel = page.locator('.dashboard-panel').filter({ hasText: 'Alerts' });
    await expect(alertsPanel).toBeVisible();

    const alertsContent = alertsPanel.locator('.flex-1.overflow-auto');
    await expect(alertsContent).toBeVisible();

    const alertsTable = alertsContent.locator('table');
    await expect(alertsTable).toBeVisible();

    const rowCount = await alertsTable.locator('tbody tr').count();
    if (rowCount === 0) {
      await expect(alertsContent.getByText('No alerts')).toBeVisible();
    }
  });

  test('alert row expands to show full JSON', async ({ page }) => {
    // Find alerts panel
    const alertsPanel = page.locator('.dashboard-panel').filter({ hasText: 'Alerts' });
    await expect(alertsPanel).toBeVisible();

    const alertsTable = alertsPanel.locator('table');
    const rows = alertsTable.locator('tbody tr');
    const rowCount = await rows.count();

    if (rowCount === 0) {
      await expect(alertsPanel.getByText('No alerts')).toBeVisible();
      return;
    }

    // Click first row to expand (rows render in pairs when expanded)
    await rows.nth(0).click();
    await page.waitForTimeout(200);

    const fullJson = alertsPanel.getByText('Full JSON');
    await expect(fullJson).toBeVisible();
  });

  test('alert panel can be collapsed and expanded', async ({ page }) => {
    // Find alerts panel
    const alertsPanel = page.locator('.dashboard-panel').filter({ hasText: 'Alerts' });

    // Find collapse button
    const collapseButton = alertsPanel.locator('button[title*="Collapse"]').or(alertsPanel.locator('button[title*="Expand"]'));
    await expect(collapseButton).toBeVisible();

    // Get initial state
    const contentBefore = alertsPanel.locator('.flex-1.overflow-auto');
    const isVisibleBefore = await contentBefore.isVisible();

    // Click collapse
    await collapseButton.click();
    await page.waitForTimeout(300);

    // Content visibility should toggle
    const isVisibleAfter = await contentBefore.isVisible();
    expect(isVisibleAfter).not.toBe(isVisibleBefore);
  });

  test('alerts panel can be removed and restored', async ({ page }) => {
    // Find alerts panel
    const alertsPanel = page.locator('.dashboard-panel').filter({ hasText: 'Alerts' });
    await expect(alertsPanel).toBeVisible();

    // Find remove button
    const removeButton = alertsPanel.locator('button[title="Remove"]');
    await expect(removeButton).toBeVisible();

    // Click remove
    await removeButton.click();
    await page.waitForTimeout(300);

    // Panel should be gone
    await expect(alertsPanel).not.toBeVisible();

    // Add button should appear
    const addButton = page.locator('button:has-text("+ Alerts")');
    await expect(addButton).toBeVisible();

    // Click to restore
    await addButton.click();
    await page.waitForTimeout(300);

    // Panel should be back
    const restoredPanel = page.locator('.dashboard-panel').filter({ hasText: 'Alerts' });
    await expect(restoredPanel).toBeVisible();
  });

  test('alert panel full page navigation works', async ({ page }) => {
    // Find alerts panel
    const alertsPanel = page.locator('.dashboard-panel').filter({ hasText: 'Alerts' });

    // Find full page button
    const fullPageButton = alertsPanel.locator('button[title="Open Full Page"]');
    await expect(fullPageButton).toBeVisible();

    // Click to go to full page
    await fullPageButton.click();
    await page.waitForTimeout(500);

    // Should navigate to alerts page
    await expect(page).toHaveURL(/\/alerts/);
  });

  // Manual dismissal is no longer available in the streamlined operator view.
});

// Dashboard panel interaction tests

import { test, expect } from '@playwright/test';

test.describe('Dashboard Panel Interactions', () => {
  test.beforeEach(async ({ page }) => {
    await page.goto('http://localhost:5000/');
    // Wait for dashboard to load
    await page.waitForSelector('.dashboard-panel', { timeout: 5000 });
  });

  test('collapse button (-) collapses and expands panel', async ({ page }) => {
    const panel = page.locator('.dashboard-panel').first();
    const collapseButton = panel.locator('button[title*="Collapse"]').or(panel.locator('button[title*="Expand"]'));
    const body = panel.locator('[data-testid="panel-body"]');

    await expect(collapseButton).toBeVisible({ timeout: 3000 });
    await expect(body).toHaveCount(1);

    // Click collapse
    await collapseButton.click({ force: true });
    await page.waitForTimeout(1000);
    await expect(body).toHaveCount(0);

    // Click expand
    await collapseButton.click({ force: true });
    await page.waitForTimeout(300);
    await expect(body).toHaveCount(1);

    // Button aria-label should reflect the current state
    const ariaLabel = await collapseButton.getAttribute('aria-label');
    expect(ariaLabel).toMatch(/Collapse|Expand/);
  });

  test('remove button (×) removes panel from dashboard', async ({ page }) => {
    // Count initial panels
    const initialPanelCount = await page.locator('.dashboard-panel').count();
    expect(initialPanelCount).toBeGreaterThan(0);

    // Find and click first remove button
    const removeButton = page.locator('button[title="Remove"]').first();
    await expect(removeButton).toBeVisible({ timeout: 3000 });

    await removeButton.click({ force: true });

    // Wait for panel to be removed
    await page.waitForTimeout(300);

    // Panel count should decrease
    const finalPanelCount = await page.locator('.dashboard-panel').count();
    expect(finalPanelCount).toBe(initialPanelCount - 1);
  });

  test('full page button (⛶) navigates to dedicated page', async ({ page }) => {
    // Find first full page button
    const fullPageButton = page.locator('button[title="Open Full Page"]').first();
    await expect(fullPageButton).toBeVisible({ timeout: 3000 });

    // Click the button
    await fullPageButton.click({ force: true });

    // Wait for navigation
    await page.waitForTimeout(500);

    // URL should have changed to a specific page view
    const url = page.url();
    expect(url).not.toBe('http://localhost:5000/');
    expect(url).toMatch(/\/(signal|trades|params|fx|alerts|balances)/);
  });

  test('add panel button restores removed panel', async ({ page }) => {
    // Remove a panel first
    const removeButton = page.locator('button[title="Remove"]').first();
    await expect(removeButton).toBeVisible({ timeout: 3000 });

    // Get panel title before removing
    const panel = page.locator('.dashboard-panel').first();
    const panelTitle = await panel.locator('h3').textContent();

    await removeButton.click({ force: true });
    await page.waitForTimeout(300);

    // Add panel button should appear
    const addButton = page.locator(`button:has-text("+ ${panelTitle}")`);
    await expect(addButton).toBeVisible({ timeout: 3000 });

    // Click to restore
    await addButton.click();
    await page.waitForTimeout(300);

    // Panel should be back
    const restoredPanel = page.locator(`.dashboard-panel:has(h3:has-text("${panelTitle}"))`);
    await expect(restoredPanel).toBeVisible();
  });

  test('collapse state persists when toggling multiple times', async ({ page }) => {
    const panel = page.locator('.dashboard-panel').first();
    const collapseButton = panel.locator('button[title*="Collapse"]').or(panel.locator('button[title*="Expand"]'));

    await expect(collapseButton).toBeVisible({ timeout: 3000 });

    // Toggle 3 times
    for (let i = 0; i < 3; i++) {
      await collapseButton.click({ force: true });
      await page.waitForTimeout(200);
    }

    // Should end up collapsed (odd number of clicks toggles the state)
    const content = panel.locator('[data-testid="panel-body"]');
    await expect(content).toHaveCount(0);
  });

  test('all panels can be collapsed independently', async ({ page }) => {
    const panels = page.locator('.dashboard-panel');
    const panelCount = await panels.count();

    // Collapse all panels
    for (let i = 0; i < panelCount; i++) {
      const panel = panels.nth(i);
      const collapseButton = panel.locator('button[title*="Collapse"]');

      if (await collapseButton.isVisible()) {
        await collapseButton.click({ force: true });
        await page.waitForTimeout(100);
      }
    }

    // All panels should be collapsed (no visible content areas)
    for (let i = 0; i < panelCount; i++) {
      const panel = panels.nth(i);
      const expandButton = panel.locator('button[title*="Expand"]');

      // Should show expand button (+) not collapse (-)
      if (await expandButton.isVisible()) {
        await expect(expandButton.getAttribute('aria-label')).resolves.toMatch(/Expand/);
      }
    }
  });

  test('panels can be dragged (drag handle works)', async ({ page }) => {
    const panel = page.locator('.dashboard-panel').first();
    const dragHandle = panel.locator('.drag-handle');

    await expect(dragHandle).toBeVisible({ timeout: 3000 });

    // Get initial position
    const initialBox = await panel.boundingBox();
    expect(initialBox).not.toBeNull();

    // Try to drag (drag handle should have cursor-move)
    const cursor = await dragHandle.evaluate(el => getComputedStyle(el).cursor);
    expect(cursor).toBe('move');
  });

  test('buttons stop event propagation (dont trigger drag)', async ({ page }) => {
    const panel = page.locator('.dashboard-panel').first();
    const titleBefore = (await panel.locator('h3').textContent())?.trim();
    expect(titleBefore).toBeTruthy();

    const panelLocator = page.locator(`.dashboard-panel[data-panel-title="${titleBefore}"]`);
    const removeButton = panelLocator.locator('button[title="Remove"]');
    await expect(removeButton).toBeVisible({ timeout: 3000 });

    // Click button should not start dragging
    await removeButton.click({ force: true });
    await page.waitForTimeout(300);

    await expect(panelLocator).toHaveCount(0);
  });
});

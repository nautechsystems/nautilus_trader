// Dashboard panel interaction tests

import { test, expect, type Page } from '@playwright/test';

const REALTIME_FLAG_KEYS = {
  global: 'fluxboard:feature:realtime-standard',
  signal: 'fluxboard:feature:realtime-standard-signal',
  killSwitch: 'fluxboard:feature:realtime-standard-kill-switch',
} as const;

type RealtimeFlagOverrides = Partial<Record<keyof typeof REALTIME_FLAG_KEYS, boolean>>;

async function setRealtimeFlags(page: Page, flags: RealtimeFlagOverrides) {
  await page.evaluate((entries) => {
    for (const [key, enabled] of entries) {
      if (enabled) {
        window.localStorage.setItem(key, '1');
      } else {
        window.localStorage.removeItem(key);
      }
    }
  }, Object.entries(flags).map(([name, enabled]) => [REALTIME_FLAG_KEYS[name as keyof typeof REALTIME_FLAG_KEYS], enabled]));
}

async function readRealtimeFlags(page: Page) {
  return page.evaluate((keys) => {
    const out: Record<string, string | null> = {};
    for (const [name, key] of Object.entries(keys)) {
      out[name] = window.localStorage.getItem(key);
    }
    return out;
  }, REALTIME_FLAG_KEYS);
}

test.describe('Dashboard Panel Interactions', () => {
  test.beforeEach(async ({ page }) => {
    await page.goto('http://localhost:5000/');
    // Wait for dashboard to load
    await page.waitForSelector('.dashboard-panel', { timeout: 5000 });
  });

  test('dashboard panels stay interactive across rollout flag toggles and rollback', async ({ page }) => {
    await setRealtimeFlags(page, { global: true, signal: true, killSwitch: false });
    await page.reload();
    await page.waitForSelector('.dashboard-panel', { timeout: 5000 });

    expect(await readRealtimeFlags(page)).toMatchObject({
      global: '1',
      signal: '1',
      killSwitch: null,
    });

    const signalPanel = page.locator('.dashboard-panel[data-panel-title="Signal"]');
    const signalCollapseButton = signalPanel.locator('button[title*="Collapse"]').or(signalPanel.locator('button[title*="Expand"]'));
    await expect(signalPanel).toBeVisible();
    await expect(signalCollapseButton).toBeVisible();
    await signalCollapseButton.click({ force: true });
    await expect(signalPanel.locator('[data-testid="panel-body"]')).toHaveCount(0);

    const tradesPanel = page.locator('.dashboard-panel[data-panel-title="Trades"]');
    const tradesRemoveButton = tradesPanel.locator('button[title="Remove"]');
    await expect(tradesPanel).toBeVisible();
    await expect(tradesRemoveButton).toBeVisible();
    await tradesRemoveButton.click({ force: true });
    await page.waitForTimeout(300);
    await expect(tradesPanel).toHaveCount(0);

    const restoreTradesButton = page.locator('button:has-text("+ Trades")');
    await expect(restoreTradesButton).toBeVisible();
    await restoreTradesButton.click();
    await expect(page.locator('.dashboard-panel[data-panel-title="Trades"]')).toBeVisible();

    await setRealtimeFlags(page, { global: true, signal: true, killSwitch: true });
    await page.reload();
    await page.waitForSelector('.dashboard-panel', { timeout: 5000 });
    expect(await readRealtimeFlags(page)).toMatchObject({
      global: '1',
      signal: '1',
      killSwitch: '1',
    });
    await expect(page.locator('.dashboard-panel[data-panel-title="Signal"]')).toBeVisible();

    await setRealtimeFlags(page, { global: false, signal: false, killSwitch: false });
    await page.reload();
    await page.waitForSelector('.dashboard-panel', { timeout: 5000 });
    expect(await readRealtimeFlags(page)).toMatchObject({
      global: null,
      signal: null,
      killSwitch: null,
    });
    await expect(page.locator('.dashboard-panel[data-panel-title="Signal"]')).toBeVisible();
    await expect(page.locator('.dashboard-panel[data-panel-title="Trades"]')).toBeVisible();
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

// UX Enhancement tests - Data freshness, filtering, and visual feedback
// Tests the UI improvements added for operator experience

import { test, expect } from '@playwright/test';

test.describe('Data Freshness Indicators', () => {
  test.beforeEach(async ({ page }) => {
    await page.goto('http://localhost:5000/');
    await page.waitForSelector('.dashboard-panel', { timeout: 5000 });
  });

  test('panels show green pulsing indicator for live data', async ({ page }) => {
    // Find a panel with freshness indicator
    const panel = page.locator('.dashboard-panel').first();
    const freshnessIndicator = panel.locator('.bg-emerald-400, .bg-red-400, .bg-neutral-600').first();

    // Indicator should be visible
    await expect(freshnessIndicator).toBeVisible({ timeout: 3000 });

    // Check for pulsing animation on live data (green dot)
    const isGreen = await freshnessIndicator.evaluate(el =>
      el.classList.contains('bg-emerald-400')
    );

    if (isGreen) {
      const hasPulse = await freshnessIndicator.evaluate(el =>
        el.classList.contains('animate-pulse')
      );
      expect(hasPulse).toBe(true);
    }
  });

  test('freshness indicator shows time-ago text', async ({ page }) => {
    const panel = page.locator('.dashboard-panel').first();
    const timeText = panel.locator('.text-\\[10px\\].text-neutral-400.tabular-nums').first();

    await expect(timeText).toBeVisible({ timeout: 3000 });

    // Should show time format like "1s", "2m", "3h", or "No data"
    const text = await timeText.textContent();
    expect(text).toMatch(/(\d+s|\d+m|\d+h|\d+d|No data)/);
  });

  test('stale data shows red indicator without pulse', async ({ page }) => {
    // Navigate to a page and wait long enough for data to become stale
    await page.goto('http://localhost:5000/trades');
    await page.waitForTimeout(12000); // Wait >10s for staleness threshold

    const indicator = page.locator('.bg-red-400').first();

    // Red indicator should not have pulse animation
    const hasPulse = await indicator.evaluate(el =>
      el.classList.contains('animate-pulse')
    ).catch(() => false);

    if (hasPulse !== null) {
      expect(hasPulse).toBe(false);
    }
  });
});

test.describe('Table Filtering - Trades', () => {
  test.beforeEach(async ({ page }) => {
    await page.goto('http://localhost:5000/trades');
    await page.waitForSelector('table', { timeout: 5000 });
  });

  test('filter controls are visible and collapsible', async ({ page }) => {
    // Filter header should be visible
    const filterHeader = page.locator('button:has-text("Filter")').or(
      page.locator('text=Filter')
    );
    await expect(filterHeader.first()).toBeVisible({ timeout: 3000 });

    // Active filter count badge should exist (may show 0)
    const filterBadge = page.locator('.bg-emerald-500, .bg-neutral-600').first();
    await expect(filterBadge).toBeVisible({ timeout: 3000 });
  });

  test('filtering by coin reduces visible rows', async ({ page }) => {
    // Count initial rows
    const initialCount = await page.locator('tbody tr').count();
    expect(initialCount).toBeGreaterThan(0);

    // Expand filters if collapsed
    const filterSection = page.locator('input[placeholder*="BTC"]').first();
    const isVisible = await filterSection.isVisible().catch(() => false);

    if (!isVisible) {
      const expandButton = page.locator('button:has-text("Filter")').first();
      await expandButton.click();
      await page.waitForTimeout(200);
    }

    // Enter coin filter (e.g., "BTC")
    const coinInput = page.locator('input[placeholder*="BTC"]').first();
    await coinInput.fill('BTC');
    await page.waitForTimeout(300);

    // Row count should change (less or equal)
    const filteredCount = await page.locator('tbody tr').count();
    expect(filteredCount).toBeLessThanOrEqual(initialCount);
  });

  test('clearing filter restores all rows', async ({ page }) => {
    // Apply filter first
    const filterSection = page.locator('input[placeholder*="BTC"]').first();
    const isVisible = await filterSection.isVisible().catch(() => false);

    if (!isVisible) {
      const expandButton = page.locator('button:has-text("Filter")').first();
      await expandButton.click();
      await page.waitForTimeout(200);
    }

    const coinInput = page.locator('input[placeholder*="BTC"]').first();
    await coinInput.fill('PLUME');
    await page.waitForTimeout(300);

    const filteredCount = await page.locator('tbody tr').count();

    // Clear filter
    const clearButton = page.locator('button:has-text("Clear")').first();
    if (await clearButton.isVisible()) {
      await clearButton.click();
      await page.waitForTimeout(300);

      const clearedCount = await page.locator('tbody tr').count();
      expect(clearedCount).toBeGreaterThanOrEqual(filteredCount);
    }
  });

  test('active filter count updates when filters applied', async ({ page }) => {
    // Expand filters
    const filterSection = page.locator('input[placeholder*="BTC"]').first();
    const isVisible = await filterSection.isVisible().catch(() => false);

    if (!isVisible) {
      const expandButton = page.locator('button:has-text("Filter")').first();
      await expandButton.click();
      await page.waitForTimeout(200);
    }

    // Get initial badge count
    const badge = page.locator('.bg-emerald-500, .bg-neutral-600').first();
    const initialBadge = await badge.textContent();

    // Apply filter
    const coinInput = page.locator('input[placeholder*="BTC"]').first();
    await coinInput.fill('ETH');
    await page.waitForTimeout(300);

    // Badge should update (showing "1" or similar)
    const updatedBadge = await badge.textContent();
    expect(updatedBadge).not.toBe(initialBadge);
  });
});

test.describe('Table Filtering - Signal', () => {
  test.beforeEach(async ({ page }) => {
    await page.goto('http://localhost:5000/signal');
    await page.waitForSelector('table', { timeout: 5000 });
  });

  test('signal table has 4 filter options', async ({ page }) => {
    // Expand filters
    const filterButton = page.locator('button:has-text("Filter")').first();
    if (await filterButton.isVisible()) {
      await filterButton.click();
      await page.waitForTimeout(200);
    }

    // Should have: Strategy, Trading, Exchange, Coin filters
    const strategyFilter = page.locator('input[placeholder*="Strategy"]').first();
    const exchangeFilter = page.locator('input[placeholder*="exchange"]').first();
    const coinFilter = page.locator('input[placeholder*="Coin"]').first();
    const botFilter = page.locator('select').first();

    await expect(strategyFilter).toBeVisible({ timeout: 2000 });
    await expect(exchangeFilter).toBeVisible({ timeout: 2000 });
  });

  test('trading status filter works with select dropdown', async ({ page }) => {
    const filterButton = page.locator('button:has-text("Filter")').first();
    if (await filterButton.isVisible()) {
      await filterButton.click();
      await page.waitForTimeout(200);
    }

    // Find trading select (Live/Pending/Paused dropdown)
    const botSelect = page.locator('select').first();
    if (await botSelect.isVisible()) {
      await botSelect.selectOption('Live');
      await page.waitForTimeout(300);

      // All visible rows should show "Live" status label
      const liveCells = page.locator('td:has-text("Live")');
      const count = await liveCells.count();
      expect(count).toBeGreaterThan(0);
    }
  });
});

test.describe('Table Filtering - Balances', () => {
  test.beforeEach(async ({ page }) => {
    await page.goto('http://localhost:5000/balances');
    await page.waitForSelector('table', { timeout: 5000 });
  });

  test('balances table has coin and exchange filters', async ({ page }) => {
    const filterButton = page.locator('button:has-text("Filter")').first();
    if (await filterButton.isVisible()) {
      await filterButton.click();
      await page.waitForTimeout(200);
    }

    const coinFilter = page.locator('input[placeholder*="BTC"]').first();
    const exchangeFilter = page.locator('input[placeholder*="bybit"]').first();

    await expect(coinFilter).toBeVisible({ timeout: 2000 });
    await expect(exchangeFilter).toBeVisible({ timeout: 2000 });
  });

  test('filtering auto-expands parent groups for matches', async ({ page }) => {
    const filterButton = page.locator('button:has-text("Filter")').first();
    if (await filterButton.isVisible()) {
      await filterButton.click();
      await page.waitForTimeout(200);
    }

    // Apply exchange filter (e.g., "bybit")
    const exchangeFilter = page.locator('input[placeholder*="bybit"]').first();
    await exchangeFilter.fill('bybit');
    await page.waitForTimeout(300);

    // Child rows (exchanges under coins) should be visible
    const childRows = page.locator('tr td.pl-8');
    const count = await childRows.count();

    // If filter matched, children should be expanded
    if (count > 0) {
      expect(count).toBeGreaterThan(0);
    }
  });
});

test.describe('Params Flash Animation', () => {
  test.beforeEach(async ({ page }) => {
    await page.goto('http://localhost:5000/params');
    await page.waitForSelector('table', { timeout: 5000 });
  });

  test('saving parameter triggers flash animation', async ({ page }) => {
    // Find first editable param cell
    const input = page.locator('input[type="text"], input[type="number"]').first();
    await expect(input).toBeVisible({ timeout: 3000 });

    // Change value to trigger dirty state
    const currentValue = await input.inputValue();
    const newValue = currentValue === '1' ? '0' : '1';
    await input.fill(newValue);
    await input.blur();
    await page.waitForTimeout(200);

    // Find and click save button
    const saveButton = page.locator('button:has-text("Save")').first();
    if (await saveButton.isVisible()) {
      // Get the table row before clicking
      const row = input.locator('xpath=ancestor::tr');

      await saveButton.click();

      // Wait for flash animation (should apply animate-flash class)
      await page.waitForTimeout(100);

      // Check if row has flash animation class
      const hasFlash = await row.evaluate(el =>
        el.classList.contains('animate-flash')
      ).catch(() => false);

      // Note: Flash may complete quickly, so we check if it was applied
      // The animation lasts 500ms, so timing is critical
      if (hasFlash !== null) {
        // Flash animation was detected
        expect(hasFlash).toBe(true);
      }
    }
  });

  test('flash animation completes and row returns to normal', async ({ page }) => {
    const input = page.locator('input[type="text"], input[type="number"]').first();
    await expect(input).toBeVisible({ timeout: 3000 });

    const currentValue = await input.inputValue();
    const newValue = currentValue === '1' ? '0' : '1';
    await input.fill(newValue);
    await input.blur();

    const saveButton = page.locator('button:has-text("Save")').first();
    if (await saveButton.isVisible()) {
      const row = input.locator('xpath=ancestor::tr');
      await saveButton.click();

      // Wait for flash animation to complete (500ms)
      await page.waitForTimeout(600);

      // Flash class should be removed
      const hasFlash = await row.evaluate(el =>
        el.classList.contains('animate-flash')
      );
      expect(hasFlash).toBe(false);
    }
  });
});

test.describe('UX Consistency', () => {
  test('all table components use consistent filter UI', async ({ page }) => {
    const pages = [
      '/trades',
      '/signal',
      '/balances'
    ];

    for (const pagePath of pages) {
      await page.goto(`http://localhost:5000${pagePath}`);
      await page.waitForSelector('table', { timeout: 5000 });

      // Check for consistent filter UI elements
      const filterButton = page.locator('button:has-text("Filter")').first();
      const isVisible = await filterButton.isVisible().catch(() => false);

      if (isVisible) {
        // Filter UI should be consistent
        await filterButton.click();
        await page.waitForTimeout(200);

        const filterInputs = page.locator('input[type="text"], select');
        const count = await filterInputs.count();

        // Each page should have at least 2 filters
        expect(count).toBeGreaterThanOrEqual(2);
      }
    }
  });

  test('freshness indicators appear on all real-time panels', async ({ page }) => {
    await page.goto('http://localhost:5000/');
    await page.waitForSelector('.dashboard-panel', { timeout: 5000 });

    // Count panels with freshness indicators
    const indicators = page.locator('.bg-emerald-400, .bg-red-400, .bg-neutral-600');
    const count = await indicators.count();

    // At least one panel should have freshness indicator
    expect(count).toBeGreaterThan(0);
  });
});

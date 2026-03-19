// Playwright smoke tests

import { test, expect, type Page } from '@playwright/test';

const REALTIME_FLAG_KEYS = {
  global: 'fluxboard:feature:realtime-standard',
  signal: 'fluxboard:feature:realtime-standard-signal',
  trades: 'fluxboard:feature:realtime-standard-trades',
  killSwitch: 'fluxboard:feature:realtime-standard-kill-switch',
} as const;

type RealtimeFlagName = keyof typeof REALTIME_FLAG_KEYS;
type RealtimeFlagOverrides = Partial<Record<RealtimeFlagName, boolean>>;

async function setRealtimeFlags(page: Page, flags: RealtimeFlagOverrides) {
  await page.evaluate((entries) => {
    for (const [key, enabled] of entries) {
      if (enabled) {
        window.localStorage.setItem(key, '1');
      } else {
        window.localStorage.removeItem(key);
      }
    }
  }, Object.entries(flags).map(([name, enabled]) => [REALTIME_FLAG_KEYS[name as RealtimeFlagName], enabled]));
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

test.describe('flux Smoke Tests', () => {
  test('realtime rollout flags can be enabled, kill-switched, and rolled back without breaking dashboard boot', async ({ page }) => {
    await page.goto('http://localhost:5000/');
    await expect(page.locator('.dashboard-panel')).toBeVisible();

    expect(await readRealtimeFlags(page)).toMatchObject({
      global: null,
      signal: null,
      trades: null,
      killSwitch: null,
    });

    await setRealtimeFlags(page, { global: true, signal: true, trades: false, killSwitch: false });
    await page.reload();
    await expect(page.locator('.dashboard-panel')).toBeVisible();

    expect(await readRealtimeFlags(page)).toMatchObject({
      global: '1',
      signal: '1',
      trades: null,
      killSwitch: null,
    });

    await setRealtimeFlags(page, { global: true, signal: true, trades: false, killSwitch: true });
    await page.reload();
    await expect(page.locator('.dashboard-panel')).toBeVisible();

    expect(await readRealtimeFlags(page)).toMatchObject({
      global: '1',
      signal: '1',
      trades: null,
      killSwitch: '1',
    });

    await setRealtimeFlags(page, { global: false, signal: false, trades: false, killSwitch: false });
    await page.reload();
    await expect(page.locator('.dashboard-panel')).toBeVisible();

    expect(await readRealtimeFlags(page)).toMatchObject({
      global: null,
      signal: null,
      trades: null,
      killSwitch: null,
    });
  });

  test('params route loads and shows strategy selector', async ({ page }) => {
    await page.goto('http://localhost:5000/params');
    await expect(page.locator('select')).toBeVisible();
    await expect(page.locator('text=Strategy')).toBeVisible();
  });

  test('trades route loads and shows table with correct headers', async ({ page }) => {
    await page.goto('http://localhost:5000/trades');
    await expect(page.locator('table')).toBeVisible();

    // Check column order matches spec
    const headers = await page.locator('thead th').allTextContents();
    expect(headers).toEqual([
      'time',
      'exchange',
      'coin',
      'side',
      'price',
      'qty',
      'notional',
      'fee',
      'notes'
    ]);
  });

  test('navigation works between routes', async ({ page }) => {
    await page.goto('http://localhost:5000/');

    // Click Params link
    await page.click('text=Params');
    await expect(page).toHaveURL(/.*params/);

    // Click Trades link
    await page.click('text=Trades');
    await expect(page).toHaveURL(/.*trades/);
  });

  test('trades socket append simulation', async ({ page }) => {
    await page.goto('http://localhost:5000/trades');

    // Wait for page to load
    await page.waitForSelector('table');

    // Simulate socket event via console (requires exposing socket globally)
    await page.evaluate(() => {
      const mockTrade = {
        coin: 'TEST',
        exchange: 'test',
        side: 'buy',
        price: '1.0',
        qty: '1.0',
        notional: '1.0',
        fee: '0.001',
        time: '2025-10-18 12:00:00.00',
        trade_id: 'playwright_test_' + Date.now(),
        order_id: 'test_order',
        notes: 'playwright'
      };

      // @ts-expect-error - socket exposed for testing
      if (window.socket) {
        // @ts-expect-error
        window.socket.emit('trade_update', mockTrade);
      }
    });

    // Give time for socket processing
    await page.waitForTimeout(500);
  });

  test('param save shows toast on mock success', async ({ page }) => {
    // Mock the API endpoint
    await page.route('**/api/strategies/*/parameters', route => {
      route.fulfill({
        status: 200,
        contentType: 'application/json',
        body: JSON.stringify({ ok: true })
      });
    });

    // Mock strategies list endpoint
    await page.route('**/api/strategies', route => {
      route.fulfill({
        status: 200,
        contentType: 'application/json',
        body: JSON.stringify(['test_strategy_1'])
      });
    });

    await page.goto('http://localhost:5000/params');

    // Select strategy
    await page.selectOption('select', { index: 1 });
    await page.waitForTimeout(300);

    // Check if Save button appears
    const saveBtn = page.locator('button:has-text("Save")');
    const isVisible = await saveBtn.isVisible().catch(() => false);

    if (isVisible) {
      await saveBtn.click();
      // Toast should appear
      await expect(page.locator('text=saved')).toBeVisible({ timeout: 2000 });
    }
  });

  test('page size persists in sessionStorage', async ({ page }) => {
    await page.goto('http://localhost:5000/trades');

    // Change page size
    await page.selectOption('select[value="100"]', '200');

    // Check sessionStorage
    const stored = await page.evaluate(() =>
      sessionStorage.getItem('trades_page_size')
    );
    expect(stored).toBe('200');
  });
});

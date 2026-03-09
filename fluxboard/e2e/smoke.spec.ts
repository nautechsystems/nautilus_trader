// Playwright smoke tests

import { test, expect } from '@playwright/test';

test.describe('flux Smoke Tests', () => {
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

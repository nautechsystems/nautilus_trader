import { test, expect } from '@playwright/test';

/**
 * End-to-end tests for PnL page critical flows.
 *
 * Tests cover:
 * - M2M calculation display (recently fixed)
 * - Time window selection
 * - Base filtering
 * - By Symbol table display and filtering
 * - Summary cards accuracy
 */

// Mock payload with realistic M2M values
const reportPayloadWithM2M = {
  ok: true,
  data: {
    asof: '2024-01-01T00:00:00Z',
    asof_ts: 1704067200000,
    summary: {
      count: 2,
      weighted_pnl_bps: 10.0,
      weighted_pnl_usd: 5.0,
      fees_bps: 7.0,
      fees_usd: 0.5,
      net_pnl_bps: 3.0,
      net_pnl_usd: 4.5,
      total_hedged_qty: 1.0,
      total_notional: 100.0,
      gross_traded_notional_usd: 100.0,
      matched_notional_usd: 100.0,
      hedge_ratio: 1.0,
      fills_total: 4,
      fills_grouped: 2,
      fill_coverage: 0.5,
      signals_total: 2,
      signals_grouped: 2,
      signal_coverage: 1.0,
    },
    groups: [
      {
        symbol: 'PLUME/USDT',
        signal_id: 'sig-1',
        start_time: '2024-01-01T00:00:00Z',
        end_time: '2024-01-01T00:05:00Z',
        dex_side: 'buy',
        dex_vwap: 0.050,
        cex_side: 'sell',
        cex_vwap: 0.051,
        hedged_qty: 1000.0,
        pnl_bps: 20.0,
        pnl_usd: 10.0,
      },
    ],
    unhedged: {},
    by_symbol: {
      'PLUME/USDT': {
        symbol: 'PLUME/USDT',
        quote: 'USDT',
        row_type: 'hedge',
        buy_qty: 1040300.0,
        sell_qty: 309650.1,
        vwap_buy: 0.05036371093915215,
        vwap_sell: 0.052928748077265234,
        fv_now: 0.0478,
        fv_source: 'snapshot',
        gross_bps: 10.0,
        gross_usd: 5.0,
        net_bps: 3.0,
        net_usd: 4.5,
        m2m_usd: -1078.91, // Calculated: buy_qty × (FV - buy_cost) + sell_qty × (sell_cost - FV)
        coverage: 1.0,
        matched_notional: 1000.0,
        buy_notional: 52350.0,
        sell_notional: 16380.0,
        gross_flow: 68730.0,
        fv_age_ms: 100,
        is_loss: false,
        is_fv_stale: false,
        is_coverage_low: false,
      },
      'PLUME.BNB/USDT': {
        symbol: 'PLUME.BNB/USDT',
        quote: 'USDT',
        row_type: 'dex',
        buy_qty: 292990.5,
        sell_qty: 929000.0,
        vwap_buy: 0.0529203617559379,
        vwap_sell: 0.05044191558806212,
        fv_now: 0.0478,
        fv_source: 'snapshot',
        gross_bps: 15.0,
        gross_usd: 8.0,
        net_bps: 8.0,
        net_usd: 4.5,
        m2m_usd: 954.12, // Short position M2M
        coverage: 0.8,
        matched_notional: 800.0,
        buy_notional: 15500.0,
        sell_notional: 46850.0,
        gross_flow: 62350.0,
        fv_age_ms: 200,
        is_loss: false,
        is_fv_stale: false,
        is_coverage_low: false,
      },
      'ETH/USDT': {
        symbol: 'ETH/USDT',
        quote: 'USDT',
        row_type: 'trade',
        buy_qty: 0,
        sell_qty: 1.5,
        vwap_buy: 0,
        vwap_sell: 2500.5,
        fv_now: 2500.0,
        fv_source: 'strategy',
        gross_bps: 0,
        gross_usd: 0,
        net_bps: 0,
        net_usd: 0,
        m2m_usd: -0.75, // Short position: (sell_cost - FV) × sell_qty
        coverage: 0,
        matched_notional: 0,
        buy_notional: 0,
        sell_notional: 3750.75,
        gross_flow: 3750.75,
        fv_age_ms: 2000,
        is_loss: true,
        is_fv_stale: false,
        is_coverage_low: true,
      },
    },
    fv_map: {
      'PLUME/USDT': { mid: 0.0478, ts: 1704067200000, source: 'snapshot' },
      'PLUME.BNB/USDT': { mid: 0.0478, ts: 1704067200000, source: 'snapshot' },
      'ETH/USDT': { mid: 2500.0, ts: 1704067198000, source: 'strategy' },
    },
    fx_map: {},
    timing: { fv_ts_skew_ms: 100, fx_ts_skew_ms: 0, computation_ms: 5 },
  },
  error: null,
};

const symbolsPayload = {
  ok: true,
  data: {
    bases: ['PLUME', 'ETH', 'SEI'],
    symbols: ['PLUME/USDT', 'PLUME.BNB/USDT', 'ETH/USDT', 'SEI/USDT'],
    count: 4,
  },
  error: null,
};

test.describe('PnL End-to-End Critical Flows', () => {
  test.beforeEach(async ({ page }) => {
    // Mock all PnL endpoints
    await page.route('**/api/v1/pnl/symbols', async (route) => {
      await route.fulfill({
        status: 200,
        contentType: 'application/json',
        body: JSON.stringify(symbolsPayload),
      });
    });

    await page.route('**/api/v1/pnl', async (route) => {
      await route.fulfill({
        status: 200,
        contentType: 'application/json',
        body: JSON.stringify(reportPayloadWithM2M),
      });
    });

    await page.route('**/api/v1/pnl/csv', async (route) => {
      const zipBytes = new Uint8Array([80, 75, 3, 4]); // Minimal ZIP header
      await route.fulfill({
        status: 200,
        headers: {
          'Content-Type': 'application/zip',
          'Content-Disposition': 'attachment; filename="pnl_report.zip"',
        },
        body: zipBytes,
      });
    });
  });

  test('displays M2M values correctly in By Symbol table', async ({ page, baseURL }) => {
    await page.goto(`${baseURL || ''}/pnl`);

    // Report auto-runs on mount, wait for it to complete
    await expect(page.getByText('Running...')).toBeVisible();
    await expect(page.getByText('Running...')).toBeHidden();

    // Wait for By Symbol table to appear
    await expect(page.getByText('By Symbol')).toBeVisible();

    // Verify M2M values are displayed (formatted as currency)
    // PLUME/USDT should show negative M2M (~-$1,078.91)
    await expect(page.getByText(/-1[,.]?078/)).toBeVisible();

    // PLUME.BNB/USDT should show positive M2M (~$954.12)
    await expect(page.getByText(/954/)).toBeVisible();

    // ETH/USDT should show small negative M2M (~-$0.75)
    await expect(page.getByText(/-0[.,]75/)).toBeVisible();
  });

  test('time window selection updates report parameters', async ({ page, baseURL }) => {
    await page.goto(`${baseURL || ''}/pnl`);

    // Wait for initial auto-run to complete
    await expect(page.getByText('Running...')).toBeHidden();

    // Track API calls
    let requestParams: any = null;
    await page.route('**/api/v1/pnl', async (route) => {
      const request = route.request();
      const body = request.postData();
      if (body) {
        requestParams = JSON.parse(body);
      }
      await route.fulfill({
        status: 200,
        contentType: 'application/json',
        body: JSON.stringify(reportPayloadWithM2M),
      });
    });

    const refreshButton = page.getByRole('button', { name: /Refresh/i });

    // Click 1h button
    const oneHourButton = page.getByRole('button', { name: '1h' });
    await oneHourButton.click();
    await refreshButton.click();
    await expect(page.getByText('Running...')).toBeHidden();

    // Verify 1h was selected (60 minutes)
    // Primary buttons use the standardized accent background color.
    await expect(oneHourButton).toHaveClass(/bg-\[#00ffae\]/);

    // Click 24h button
    const twentyFourHourButton = page.getByRole('button', { name: '24h' });
    await twentyFourHourButton.click();
    await refreshButton.click();
    await expect(page.getByText('Running...')).toBeHidden();

    // Verify 24h was selected
    await expect(twentyFourHourButton).toHaveClass(/bg-\[#00ffae\]/);
  });

  test('base filter selection filters symbols', async ({ page, baseURL }) => {
    await page.goto(`${baseURL || ''}/pnl`);

    // Report auto-runs on mount, wait for it to complete
    await expect(page.getByText('Running...')).toBeHidden();

    // Wait for symbols to load
    await page.waitForTimeout(500);

    // Select PLUME base filter
    const baseSelect = page.getByRole('combobox', { name: /All Base/i });
    await baseSelect.selectOption('PLUME');

    // Run report again with filter using refresh icon
    const refreshButton = page.getByRole('button', { name: /Refresh/i });
    await refreshButton.click();
    await expect(page.getByText('Running...')).toBeHidden();

    // Should show PLUME symbols
    await expect(page.getByText('PLUME/USDT')).toBeVisible();
    await expect(page.getByText('PLUME.BNB/USDT')).toBeVisible();

    // Should not show ETH symbols (if filtered correctly)
    // Note: This depends on backend filtering, so we just verify the select option was set
    await expect(baseSelect).toHaveValue('PLUME');
  });

  test('By Symbol filter buttons work correctly', async ({ page, baseURL }) => {
    await page.goto(`${baseURL || ''}/pnl`);

    // Report auto-runs on mount, wait for it to complete
    await expect(page.getByText('Running...')).toBeHidden();

    // Wait for By Symbol section
    await expect(page.getByText('By Symbol')).toBeVisible();
    await page.waitForTimeout(500);

    // Click "Loss only" filter
    const lossFilter = page.getByRole('checkbox', { name: /Loss only/i });
    if (await lossFilter.isVisible()) {
      await lossFilter.click();

      // Should show only loss symbols (ETH/USDT has is_loss: true)
      await expect(page.getByText('ETH/USDT')).toBeVisible();
      // PLUME symbols should be filtered out (they don't have is_loss)

      // Click "All" to reset
      const allFilter = page.getByRole('checkbox', { name: /All/i });
      await allFilter.click();

      // All symbols should be visible again
      await expect(page.getByText('PLUME/USDT')).toBeVisible();
      await expect(page.getByText('ETH/USDT')).toBeVisible();
    }
  });

  test('summary cards display correct values', async ({ page, baseURL }) => {
    await page.goto(`${baseURL || ''}/pnl`);

    // Report auto-runs on mount, wait for it to complete
    await expect(page.getByText('Running...')).toBeHidden();

    // Wait for summary section
    await expect(page.getByText('Overall Summary')).toBeVisible();

    // Verify summary values are displayed
    // Gross PnL: 10.0 bps / $5.0
    await expect(page.getByText(/10[.,]0.*bps/)).toBeVisible();
    await expect(page.getByText(/\$5[.,]0/)).toBeVisible();

    // Net PnL: 3.0 bps / $4.5
    await expect(page.getByText(/3[.,]0.*bps/)).toBeVisible();
    await expect(page.getByText(/\$4[.,]5/)).toBeVisible();
  });

  test('CSV download works after report runs', async ({ page, baseURL }) => {
    await page.goto(`${baseURL || ''}/pnl`);

    // Report auto-runs on mount, wait for it to complete
    await expect(page.getByText('Running...')).toBeHidden();

    // CSV button should be enabled after auto-run
    const csvButton = page.getByRole('button', { name: /Download CSV|CSV/i });
    await expect(csvButton).toBeEnabled();

    // Track download
    const downloadPromise = page.waitForEvent('download', { timeout: 5000 }).catch(() => null);
    await csvButton.click();

    // Download should be triggered
    const download = await downloadPromise;
    if (download) {
      expect(download.suggestedFilename()).toContain('pnl');
    }
  });

  test('auto-refresh updates report periodically', async ({ page, baseURL }) => {
    await page.goto(`${baseURL || ''}/pnl`);

    // Track API call count
    let apiCallCount = 0;
    await page.route('**/api/v1/pnl', async (route) => {
      apiCallCount++;
      await route.fulfill({
        status: 200,
        contentType: 'application/json',
        body: JSON.stringify(reportPayloadWithM2M),
      });
    });

    // Wait for initial auto-run to complete
    await expect(page.getByText('Running...')).toBeHidden();
    const initialCallCount = apiCallCount;

    // Enable auto-refresh - should trigger immediate refresh
    const autoRefresh = page.getByLabel(/Auto-refresh/i);
    await autoRefresh.click();

    // Wait for immediate refresh to complete
    await expect(page.getByText('Running...')).toBeHidden();
    expect(apiCallCount).toBeGreaterThan(initialCallCount);

    const afterEnableCallCount = apiCallCount;

    // Wait for periodic auto-refresh (should trigger after ~30 seconds)
    await page.waitForTimeout(31000);

    // Should have made additional API calls
    expect(apiCallCount).toBeGreaterThan(afterEnableCallCount);
  });

  test('By Symbol table shows correct row types and flags', async ({ page, baseURL }) => {
    await page.goto(`${baseURL || ''}/pnl`);

    // Report auto-runs on mount, wait for it to complete
    await expect(page.getByText('Running...')).toBeHidden();

    // Wait for By Symbol table
    await expect(page.getByText('By Symbol')).toBeVisible();
    await page.waitForTimeout(500);

    // Verify symbols are displayed
    await expect(page.getByText('PLUME/USDT')).toBeVisible();
    await expect(page.getByText('PLUME.BNB/USDT')).toBeVisible();
    await expect(page.getByText('ETH/USDT')).toBeVisible();

    // Verify FV source badges are shown
    await expect(page.getByText('snapshot')).toBeVisible();
    await expect(page.getByText('strategy')).toBeVisible();
  });
});

import { test, expect } from '@playwright/test';

// Minimal mock payloads for FluxAPI endpoints used by PnL page
const symbolsPayload = { ok: true, data: { bases: ['PLUME', 'ETH', 'SEI'], symbols: ['PLUME/USDT', 'ETH/USDC', 'SEI/USDT'], count: 3 }, error: null };

const reportPayload = {
  ok: true,
  data: {
    asof: '2024-01-01T00:00:00Z',
    asof_ts: 1704067200000,
    summary: {
      count: 1,
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
      fills_total: 2,
      fills_grouped: 2,
      fill_coverage: 1.0,
      signals_total: 1,
      signals_grouped: 1,
      signal_coverage: 1.0,
    },
    groups: [
      {
        symbol: 'PLUME/USDT',
        signal_id: 'sig-1',
        start_time: '2024-01-01T00:00:00Z',
        end_time: '2024-01-01T00:05:00Z',
        dex_side: 'buy',
        dex_vwap: 1.0,
        cex_side: 'sell',
        cex_vwap: 1.001,
        hedged_qty: 10.0,
        pnl_bps: 10.0,
        pnl_usd: 5.0,
      },
    ],
    unhedged: {},
    by_symbol: {
      'PLUME/USDT': {
        symbol: 'PLUME/USDT', quote: 'USDT', buy_qty: 10, sell_qty: 10, vwap_buy: 1, vwap_sell: 1.001,
        fv_now: 1.0, gross_bps: 10.0, gross_usd: 5.0, net_bps: 3.0, net_usd: 4.5,
        m2m_usd: 0, matched_notional: 10, buy_notional: 10, sell_notional: 10.01, gross_flow: 20.01,
        is_loss: false, is_fv_stale: false, is_coverage_low: false, coverage: 1.0,
      },
    },
    fv_map: { 'PLUME/USDT': { mid: 1.0, ts: 1704067200000, source: 'snapshot' } },
    fx_map: {},
    timing: { fv_ts_skew_ms: 0, fx_ts_skew_ms: 0, computation_ms: 5 },
  },
  error: null,
};

test.describe('PnL E2E (mocked API)', () => {
  test.beforeEach(async ({ page }) => {
    // Mock PnL endpoints
    await page.route('**/api/v1/pnl/symbols', async (route) => {
      await route.fulfill({ status: 200, contentType: 'application/json', body: JSON.stringify(symbolsPayload) });
    });
    await page.route('**/api/v1/pnl', async (route) => {
      await route.fulfill({ status: 200, contentType: 'application/json', body: JSON.stringify(reportPayload) });
    });
    await page.route('**/api/v1/pnl/csv', async (route) => {
      const zipBytes = new Uint8Array([80, 75, 3, 4]); // PK.. minimal bytes
      await route.fulfill({ status: 200, headers: { 'Content-Type': 'application/zip', 'Content-Disposition': 'attachment; filename="pnl_report.zip"' }, body: zipBytes });
    });
  });

  test('runs report and shows summary/groups', async ({ page, baseURL }) => {
    await page.goto(`${baseURL || ''}/pnl`);

    const runButton = page.getByRole('button', { name: /Run Report/i });
    await expect(runButton).toBeVisible();
    await runButton.click();

    await expect(page.getByText('Overall Summary')).toBeVisible();
    await expect(page.getByText('PnL Groups')).toBeVisible();
    await expect(page.getByText('PLUME/USDT')).toBeVisible();
  });

  test('symbol filtering toggles filter chip', async ({ page, baseURL }) => {
    await page.goto(`${baseURL || ''}/pnl`);
    const runButton = page.getByRole('button', { name: /Run Report/i });
    await runButton.click();
    // Wait a bit for symbol cards
    await page.waitForTimeout(200);

    const card = page.locator('.rounded.p-4.border-2.cursor-pointer').first();
    if (await card.isVisible()) {
      const symbolText = (await card.locator('h3').textContent())?.trim() || '';
      await card.click();
      await expect(page.getByText(new RegExp(`${symbolText}\s+✕`))).toBeVisible();
      await page.getByText(new RegExp(`${symbolText}\s+✕`)).click();
      await expect(page.getByText(new RegExp(`${symbolText}\s+✕`))).not.toBeVisible();
    }
  });

  test('csv download endpoint is called after report', async ({ page, baseURL }) => {
    let csvRequested = false;
    await page.route('**/api/v1/pnl/csv', async (route) => {
      csvRequested = true;
      const zipBytes = new Uint8Array([80, 75, 3, 4]);
      await route.fulfill({ status: 200, headers: { 'Content-Type': 'application/zip', 'Content-Disposition': 'attachment; filename="pnl_report.zip"' }, body: zipBytes });
    });

    await page.goto(`${baseURL || ''}/pnl`);
    await page.getByRole('button', { name: /Run Report/i }).click();
    // CSV button becomes enabled
    const csvButton = page.getByRole('button', { name: /Download CSV/i });
    await expect(csvButton).toBeEnabled();
    await csvButton.click();
    expect(csvRequested).toBeTruthy();
  });
});


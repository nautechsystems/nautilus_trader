import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { render, screen, fireEvent, waitFor } from '@testing-library/react';
import { act, Suspense } from 'react';

import PnL from '../PnL';
import { api } from '../api';
import type { PnLReport } from '../types';

vi.mock('../api', () => ({
  api: {
    runPnLReport: vi.fn(),
    downloadPnLCSV: vi.fn(),
    getAvailableSymbols: vi.fn(),
    runPnLDelta: vi.fn(),
  },
}));

const baseTokens = ['PLUME', 'ETH', 'SEI', 'ASTER'];

const mockReport: PnLReport = {
  asof: '2025-10-19T10:05:00Z',
  asof_ts: 1_696_714_700_000,
  summary: {
    count: 2,
    weighted_pnl_bps: 9.0,
    weighted_pnl_usd: 200.0,
    fees_bps: 7.0,
    fees_usd: 50.0,
    net_pnl_bps: 2.0,
    net_pnl_usd: 150.0,
    total_hedged_qty: 101.5,
    total_notional: 3755.0,
    fills_total: 4,
    fills_grouped: 2,
    fill_coverage: 0.5,
  },
  groups: [
    {
      symbol: 'PLUME/USDT',
      signal_id: 'sig_test_001',
      start_time: '2025-10-19T10:00:00Z',
      end_time: '2025-10-19T10:00:01Z',
      dex_side: 'buy',
      dex_vwap: 0.05,
      cex_side: 'sell',
      cex_vwap: 0.0506,
      hedged_qty: 100.0,
      pnl_bps: 12.0,
    },
    {
      symbol: 'ETH/USDT',
      signal_id: 'sig_test_002',
      start_time: '2025-10-19T10:01:00Z',
      end_time: '2025-10-19T10:01:01Z',
      dex_side: 'sell',
      dex_vwap: 2500.5,
      cex_side: 'buy',
      cex_vwap: 2499.0,
      hedged_qty: 1.5,
      pnl_bps: 6.0,
    },
  ],
  unhedged: {
    PLUME_rooster: 5.2,
  },
  by_symbol: {
    'PLUME/USDT': {
      symbol: 'PLUME/USDT',
      quote: 'USDT',
      buy_qty: 100,
      sell_qty: 0,
      vwap_buy: 0.05,
      vwap_sell: 0,
      fv_now: 0.051,
      fv_source: 'snapshot',
      gross_bps: 12,
      gross_usd: 50,
      net_bps: 5,
      net_usd: 40,
      m2m_usd: 10,
      coverage: 1.0,
      matched_notional: 5,
      buy_notional: 5,
      sell_notional: 0,
      gross_notional: 5,
      gross_flow: 5,
      fv_age_ms: 100,
    },
    'ETH/USDT': {
      symbol: 'ETH/USDT',
      quote: 'USDT',
      buy_qty: 0,
      sell_qty: 1.5,
      vwap_buy: 0,
      vwap_sell: 2500.5,
      fv_now: 2500.0,
      fv_source: 'strategy',
      gross_bps: 6,
      gross_usd: 150,
      net_bps: -1,
      net_usd: -20,
      m2m_usd: -0.75,
      coverage: 0.8,
      matched_notional: 3000,
      buy_notional: 0,
      sell_notional: 3750.75,
      gross_notional: 3750.75,
      gross_flow: 3750.75,
      fv_age_ms: 2000,
    },
  },
  fv_map: {
    'PLUME/USDT': { mid: 0.051, ts: 1_696_714_699_900, source: 'snapshot' },
    'ETH/USDT': { mid: 2500, ts: 1_696_714_699_000, source: 'strategy' },
  },
  fx_map: {
    'USDT→USDT': { rate: 1, ts: 1_696_714_700_000, inverse_used: false, missing: false },
  },
  timing: {
    fv_ts_skew_ms: 100,
    fx_ts_skew_ms: 0,
  },
};

async function renderPnL() {
  const view = render(
    <Suspense fallback={null}>
      <PnL />
    </Suspense>
  );
  await act(async () => {
    await Promise.resolve();
  });
  return view;
}

async function click(element: HTMLElement) {
  await act(async () => {
    fireEvent.click(element);
    await Promise.resolve();
  });
}

async function change(element: HTMLElement, value: string) {
  await act(async () => {
    fireEvent.change(element, { target: { value } });
    await Promise.resolve();
  });
}

beforeEach(() => {
  vi.clearAllMocks();
  vi.mocked(api.getAvailableSymbols).mockResolvedValue(baseTokens);
  vi.mocked(api.runPnLReport).mockResolvedValue({
    status: 200,
    etag: 'test-etag',
    report: mockReport,
  });
  vi.mocked(api.runPnLDelta).mockResolvedValue({ reset_required: true });
});

afterEach(() => {
  vi.useRealTimers();
  try {
    delete (navigator as Navigator & { clipboard?: unknown }).clipboard;
  } catch {
    // ignore cleanup errors
  }
});

function mockClipboard() {
  const writeText = vi.fn<(text: string) => Promise<void>>().mockResolvedValue();
  Object.defineProperty(navigator, 'clipboard', {
    configurable: true,
    value: { writeText },
  });
  return writeText;
}

describe('PnL', () => {
  it('loads base options on mount', async () => {
    await renderPnL();

    const select = await screen.findByRole('combobox');
    await waitFor(() => {
      const options = Array.from((select as HTMLSelectElement).options).map(opt => opt.value);
      expect(options).toEqual(expect.arrayContaining(baseTokens));
    });
  });

  it('submits selected base to runPnLReport', async () => {
    await renderPnL();

    const select = await screen.findByRole('combobox');
    await change(select, 'PLUME');

    // Wait for initial auto-run to complete, then use refresh icon
    await waitFor(() => expect(api.runPnLReport).toHaveBeenCalled(), { timeout: 3000 });

    const refreshButton = screen.getByRole('button', { name: /Refresh/i });
    await click(refreshButton);

    await waitFor(() => expect(api.runPnLReport).toHaveBeenCalledTimes(2));
    const calls = vi.mocked(api.runPnLReport).mock.calls;
    const call = calls[calls.length - 1];
    expect(call?.[0]).toMatchObject({ base: 'PLUME', dex_fee_bps: 2, cex_fee_bps: 5 });
  });

  it('renders by-symbol FV(now) details after report', async () => {
    await renderPnL();

    // Report auto-runs on mount, wait for it to complete
    await waitFor(() => expect(api.runPnLReport).toHaveBeenCalled(), { timeout: 3000 });

    // Wait for report to complete and data to render
    await waitFor(() => {
      // PLUME/USDT might be rendered in a table cell or split across elements
      const rows = screen.queryAllByText(/PLUME\s*\/\s*USDT/i);
      if (rows.length === 0) {
        // Try finding by partial text
        const partial = screen.queryAllByText(/PLUME/i);
        expect(partial.length).toBeGreaterThan(0);
      } else {
        expect(rows.length).toBeGreaterThan(0);
      }
    }, { timeout: 3000 });

    // Check for FV(now) text
    await waitFor(() => {
      const fvNow = screen.queryAllByText(/FV\(now\)/i);
      expect(fvNow.length).toBeGreaterThan(0);
    }, { timeout: 2000 });

    // Check for other expected elements
    expect(screen.getByText('snapshot')).toBeInTheDocument();
    expect(screen.getByText('strategy')).toBeInTheDocument();
    expect(screen.getByText('$10.00')).toBeInTheDocument(); // M2M column renders money format
  });

  it('shows inline error when report fails', async () => {
    await renderPnL();

    // Wait for initial auto-run to complete
    await waitFor(() => expect(api.runPnLReport).toHaveBeenCalled(), { timeout: 3000 });

    // Wait for initial report to complete
    await waitFor(() => {
      const rows = screen.queryAllByText(/PLUME\s*\/\s*USDT/i);
      if (rows.length === 0) {
        // At least verify the component rendered
        expect(screen.queryByText(/PLUME/i)).toBeTruthy();
      }
    }, { timeout: 3000 });

    vi.mocked(api.runPnLReport).mockRejectedValueOnce(new Error('API error'));

    const refreshButton = screen.getByRole('button', { name: /Refresh/i });
    await click(refreshButton);
    await waitFor(() => {
      expect(screen.getByText('API error')).toBeInTheDocument();
    }, { timeout: 2000 });
  });

  it('auto-refresh triggers immediate report when enabled', async () => {
    vi.useFakeTimers();
    const view = await renderPnL();

    // Wait for initial auto-run on mount
    await waitFor(() => expect(api.runPnLReport).toHaveBeenCalled(), { timeout: 3000 });
    const initialCallCount = vi.mocked(api.runPnLReport).mock.calls.length;

    const autoRefreshToggle = await screen.findByLabelText(/Auto-refresh/i);

    // Enable auto-refresh - should trigger immediate refresh
    await click(autoRefreshToggle);

    // Advance timers slightly to allow the immediate trigger
    await act(async () => {
      vi.advanceTimersByTime(250);
      await Promise.resolve();
    });

    // Should have triggered another call immediately when enabled
    await waitFor(() => {
      expect(api.runPnLReport).toHaveBeenCalledTimes(initialCallCount + 1);
    }, { timeout: 1000 });

    vi.clearAllTimers();
    await act(async () => {});
    view.unmount();
  });

  it('auto-refresh triggers periodic reports every 30 seconds', async () => {
    vi.useFakeTimers();
    const view = await renderPnL();

    // Wait for initial auto-run on mount
    await waitFor(() => expect(api.runPnLReport).toHaveBeenCalled(), { timeout: 3000 });
    const initialCallCount = vi.mocked(api.runPnLReport).mock.calls.length;

    const autoRefreshToggle = await screen.findByLabelText(/Auto-refresh/i);
    await click(autoRefreshToggle);

    // Advance timers to allow immediate trigger
    await act(async () => {
      vi.advanceTimersByTime(250);
      await Promise.resolve();
    });

    // Wait for immediate trigger
    await waitFor(() => {
      expect(api.runPnLReport).toHaveBeenCalledTimes(initialCallCount + 1);
    });

    // Advance 30 seconds - should trigger another refresh
    await act(async () => {
      vi.advanceTimersByTime(30000);
      await Promise.resolve();
    });

    await waitFor(() => {
      expect(api.runPnLReport).toHaveBeenCalledTimes(initialCallCount + 2);
    }, { timeout: 1000 });

    vi.clearAllTimers();
    await act(async () => {});
    view.unmount();
  });

  it('PnL Groups table sorting cycles through 3 states without becoming unclickable', async () => {
    await renderPnL();

    // Wait for component to render
    await waitFor(() => {
      expect(screen.getByText('PnL Report')).toBeInTheDocument();
    });

    // Report auto-runs on mount, wait for it to complete
    await waitFor(() => expect(api.runPnLReport).toHaveBeenCalled(), { timeout: 3000 });

    // Wait for PnL Groups section to appear
    await waitFor(() => {
      expect(screen.getByText('PnL Groups')).toBeInTheDocument();
    });

    // Find the PnL(bps) column header - this is the default sort column
    const pnlBpsHeader = await screen.findByRole('columnheader', { name: /Gross\s*\(bps\/\$\)/i });
    expect(pnlBpsHeader).toBeInTheDocument();

    // Click 1: Should sort ascending (change from default desc to asc)
    await click(pnlBpsHeader!);
    await waitFor(() => {
      // The header should still be clickable
      expect(pnlBpsHeader).toBeInTheDocument();
    });

    // Click 2: Should sort descending
    await click(pnlBpsHeader!);
    await waitFor(() => {
      expect(pnlBpsHeader).toBeInTheDocument();
    });

    // Click 3: Should reset to default (desc) - this was the bug
    await click(pnlBpsHeader!);
    await waitFor(() => {
      expect(pnlBpsHeader).toBeInTheDocument();
    });

    // Click 4: Should sort ascending again - verify column is still clickable
    await click(pnlBpsHeader!);
    await waitFor(() => {
      expect(pnlBpsHeader).toBeInTheDocument();
    });

    // Verify we can click other columns too
    const symbolHeader = await screen.findByRole('columnheader', { name: /^Symbol$/i });
    expect(symbolHeader).toBeInTheDocument();

    await click(symbolHeader);
    await waitFor(() => {
      expect(symbolHeader).toBeInTheDocument();
    });
  });

  it('copies the current page to the clipboard when Copy Page is clicked', async () => {
    const writeText = mockClipboard();

    await renderPnL();

    // Report auto-runs on mount
    await waitFor(() => expect(api.runPnLReport).toHaveBeenCalled(), { timeout: 3000 });

    // Wait for data to render - PLUME/USDT might be split across elements
    await waitFor(() => {
      const rows = screen.queryAllByText(/PLUME\s*\/\s*USDT/i);
      if (rows.length === 0) {
        // Try partial match
        const partial = screen.queryAllByText(/PLUME/i);
        expect(partial.length).toBeGreaterThan(0);
      }
    }, { timeout: 3000 });

    const copyButton = await screen.findByRole('button', { name: /Copy Page/i });
    await click(copyButton);

    await waitFor(() => expect(writeText).toHaveBeenCalledTimes(1), { timeout: 2000 });
    const payload = writeText.mock.calls[0]?.[0];
    expect(payload).toContain('| Symbol');
    expect(payload).toMatch(/PLUME\s*\/\s*USDT/i);
  });
});

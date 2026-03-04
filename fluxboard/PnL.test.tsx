import { act, cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import PnL from './PnL';

const {
  featureFlagMock,
  runPnLReportMock,
  runPnLDeltaMock,
  downloadPnLCSVMOCK,
  getAvailableSymbolsMock,
  buildReport,
} = vi.hoisted(() => {
  const baseSummary = {
    count: 0,
    weighted_pnl_bps: 0,
    weighted_pnl_usd: 0,
    net_pnl_bps: 0,
    net_pnl_usd: 0,
    fills_grouped: 0,
    fills_total: 0,
    hedge_ratio: 1,
    total_hedged_qty: 0,
    gross_traded_notional_usd: 0,
    matched_notional_usd: 0,
    total_notional: 0,
    fees_bps: 0,
    fees_usd: 0,
  };

  const baseReport = {
    summary: baseSummary,
    groups: [],
    by_symbol: {},
    unhedged: {},
    fv_map: {},
    fx_map: {},
    timing: {
      fv_ts_skew_ms: 0,
      fx_ts_skew_ms: 0,
    },
    asof: '2025-01-01T00:00:00.000Z',
    asof_ts: 0,
    report_signature: 'test-signature',
    group_hashes: {},
    symbol_hashes: {},
    unhedged_hashes: {},
  };

  const cloneReport = () => JSON.parse(JSON.stringify(baseReport));
  const buildReport = (overrides: Partial<typeof baseReport> = {}) => {
    const summary = { ...baseSummary, ...(overrides.summary ?? {}) };
    return {
      ...cloneReport(),
      ...overrides,
      summary,
      report_signature: overrides.report_signature ?? `sig-${Math.random().toString(16).slice(2)}`,
    };
  };

  return {
    featureFlagMock: vi.fn(() => true),
    runPnLReportMock: vi.fn().mockImplementation(async () => ({
      status: 200,
      etag: 'etag',
      report: cloneReport(),
    })),
    runPnLDeltaMock: vi.fn().mockResolvedValue({ status: 304 }),
    downloadPnLCSVMOCK: vi.fn().mockResolvedValue(undefined),
    getAvailableSymbolsMock: vi.fn().mockResolvedValue(['BTC']),
    buildReport,
  };
});

vi.mock('./config/featureFlags', () => ({
  isTradesDecisionDetailsEnabled: featureFlagMock,
  isPnlDecisionDetailsEnabled: featureFlagMock,
}));

vi.mock('./api', () => ({
  api: {
    runPnLReport: runPnLReportMock,
    runPnLDelta: runPnLDeltaMock,
    downloadPnLCSV: downloadPnLCSVMOCK,
    getAvailableSymbols: getAvailableSymbolsMock,
  },
}));

vi.mock('./sockets', () => ({
  disconnectSocket: vi.fn(),
  connectSocket: vi.fn(),
}));

vi.mock('@/hooks/useMobileLayout', () => ({
  useMobileLayout: () => ({ isMobile: false }),
}));

vi.mock('./hooks/useCopyToClipboard', () => ({
  useCopyToClipboard: () => [null, vi.fn()],
}));

vi.mock('./components/ui/table/DataTable', () => ({
  DataTable: ({ children }: any) => <div data-testid="data-table">{children}</div>,
}));

vi.mock('./components/ui', () => {
  const MockComp = ({ children, ...props }: any) => <div {...props}>{children}</div>;
  const MockButton = ({ children, loading: _loading, ...props }: any) => <button {...props}>{children}</button>;
  const MockSwitch = ({ onCheckedChange, 'aria-label': ariaLabel, label, checked, ...props }: any) => (
    <input
      type="checkbox"
      role="switch"
      aria-label={ariaLabel || label}
      checked={checked}
      onChange={(e) => onCheckedChange?.(e.target.checked)}
      {...props}
    />
  );
  return {
    Button: MockButton,
    FilterChip: MockComp,
    SimpleTooltip: ({ children }: any) => <>{children}</>,
    Switch: MockSwitch,
  };
});

vi.mock('./components/shared/Pager', () => ({
  Pager: ({ children }: any) => <div>{children}</div>,
}));

vi.mock('./components/shared/PanelBody', () => ({
  PanelBody: ({ children }: any) => <div>{children}</div>,
}));

vi.mock('./components/shared/StatusPill', () => ({
  StatusPill: ({ label }: any) => <span>{label}</span>,
}));

vi.mock('./components/shared/status', () => ({
  statusFromMark: () => ({ label: 'ok', status: 'ok' }),
  StatusDescriptor: class {},
}));

vi.mock('./components/shared/PanelHeader', () => ({
  PanelHeader: ({ children, actions }: any) => (
    <div>
      {actions}
      {children}
    </div>
  ),
}));

vi.mock('./components/shared/LoadingState', () => ({
  LoadingState: () => <div>loading</div>,
}));

vi.mock('./components/shared/EmptyState', () => ({
  EmptyState: () => <div>empty</div>,
}));

vi.mock('./components/shared/TableFilter', () => ({
  TableFilter: () => <div>filters</div>,
}));

vi.mock('sonner', () => ({
  toast: {
    success: vi.fn(),
    error: vi.fn(),
    warning: vi.fn(),
    info: vi.fn(),
  },
}));

const flushEffects = async () => {
  await act(async () => {
    if (vi.isFakeTimers?.()) {
      if (vi.getTimerCount() > 0) {
        vi.runOnlyPendingTimers();
      }
    }
    await Promise.resolve();
  });
};

describe('PnL feature flags', () => {
  beforeEach(() => {
    featureFlagMock.mockClear();
    window.scrollTo = vi.fn();
    runPnLReportMock.mockClear();
    runPnLDeltaMock.mockClear();
    getAvailableSymbolsMock.mockClear();
  });

  afterEach(() => {
    cleanup();
  });

  it('renders without crashing when decision details flag hook runs', async () => {
    const { getByText } = render(<PnL />);
    await waitFor(() => expect(runPnLReportMock).toHaveBeenCalled(), { timeout: 1000 });
    expect(featureFlagMock).toHaveBeenCalled();
    await waitFor(() => expect(getByText(/Summary/i)).toBeInTheDocument());
  });

  it('marks the active time window control as selected for clarity', async () => {
    vi.useFakeTimers();
    runPnLReportMock.mockResolvedValueOnce({
      status: 200,
      etag: 'etag',
      report: buildReport(),
    });
    await act(async () => {
      render(<PnL />);
    });
    await flushEffects();

    const preset24h = screen.getByRole('button', { name: '24h' });
    expect(preset24h).toHaveAttribute('aria-pressed', 'true');

    const lastN = screen.getByRole('button', { name: 'Last N' });
    fireEvent.click(lastN);

    expect(lastN).toHaveAttribute('aria-pressed', 'true');
    expect(preset24h).toHaveAttribute('aria-pressed', 'false');
    vi.useRealTimers();
  });

  it('auto-refresh triggers an immediate and interval refresh when enabled', async () => {
    vi.useFakeTimers({ shouldAdvanceTime: true });
    runPnLReportMock.mockResolvedValueOnce({
      status: 200,
      etag: 'etag',
      report: buildReport(),
    });
    runPnLDeltaMock.mockResolvedValue({
      status: 200,
      etag: 'etag-2',
      report: buildReport({ report_signature: 'sig-2' }),
    });

    await act(async () => {
      render(<PnL />);
    });
    await flushEffects();
    act(() => {
      vi.advanceTimersByTime(250);
    });
    await waitFor(() => expect(runPnLReportMock).toHaveBeenCalledTimes(1));

    const autoToggle = screen.getByRole('switch', { name: /auto-refresh/i });
    await act(async () => {
      fireEvent.click(autoToggle);
    });

    const refreshCalls = () => runPnLDeltaMock.mock.calls.length + runPnLReportMock.mock.calls.length;

    await waitFor(() => expect(refreshCalls()).toBeGreaterThan(1));

    await act(async () => {
      vi.advanceTimersByTime(30500);
    });
    await flushEffects();
    await waitFor(() => expect(refreshCalls()).toBeGreaterThan(2));
    vi.useRealTimers();
  });

  it('allows sorting by symbol and normalized dollar columns in the by-symbol table', async () => {
    vi.useFakeTimers({ shouldAdvanceTime: true });
    const report = buildReport({
      summary: {
        net_pnl_bps: 0,
        net_pnl_usd: 0,
        gross_traded_notional_usd: 3000,
        matched_notional_usd: 2000,
      },
      by_symbol: {
        BTC: {
          buy_qty: 1,
          sell_qty: 0.5,
          vwap_buy: 100,
          vwap_sell: 105,
          fv_now: 102,
          gross_usd: 200,
          gross_bps: 25,
          net_usd: 150,
          net_bps: 18,
          m2m_usd: 50,
          gross_flow: 1200,
          coverage: 0.5,
          matched_notional: 800,
          buy_notional: 600,
          sell_notional: 600,
        },
        ETH: {
          buy_qty: 2,
          sell_qty: 1.5,
          vwap_buy: 50,
          vwap_sell: 52,
          fv_now: 51,
          gross_usd: 300,
          gross_bps: 12,
          net_usd: 280,
          net_bps: 30,
          m2m_usd: 80,
          gross_flow: 900,
          coverage: 0.9,
          matched_notional: 1200,
          buy_notional: 700,
          sell_notional: 500,
        },
      },
    });

    runPnLReportMock.mockResolvedValueOnce({
      status: 200,
      etag: 'etag',
      report,
    });

    const { container } = render(<PnL />);
    await flushEffects();
    act(() => {
      vi.advanceTimersByTime(250);
    });
    await waitFor(() => expect(Array.from(container.querySelectorAll('tbody tr')).length).toBeGreaterThan(0));

    const rowSymbols = () =>
      Array.from(container.querySelectorAll('tbody tr')).map(
        (row) => row.querySelector('td')?.textContent?.trim() ?? ''
      );

    // Default sorted by net bps (desc): ETH (30) then BTC (18)
    expect(rowSymbols()).toEqual(['ETH', 'BTC']);

    fireEvent.click(screen.getByText(/Net \(bps\/\$\)/i));
    await waitFor(() => expect(rowSymbols()).toEqual(['BTC', 'ETH']));

    fireEvent.click(screen.getByRole('button', { name: /show more/i }));
    fireEvent.click(screen.getByText(/Matched \(\$\)/i));
    await waitFor(() => expect(rowSymbols()).toEqual(['ETH', 'BTC']));

    fireEvent.click(screen.getByText(/^Symbol$/i));
    await waitFor(() => expect(rowSymbols()).toEqual(['BTC', 'ETH']));
    vi.useRealTimers();
  });

  it('shows FV source badge and caps long FV age in tooltip', async () => {
    vi.useFakeTimers({ shouldAdvanceTime: true });
    const report = buildReport({
      by_symbol: {
        'COIN/USDC': {
          buy_qty: 1,
          sell_qty: 0,
          vwap_buy: 100,
          vwap_sell: 0,
          fv_now: 123.45,
          fv_source: 'futu_md',
          fv_age_ms: 120000, // 2 minutes
          gross_flow: 100,
          gross_bps: 10,
          net_bps: 8,
          net_usd: 1,
          m2m_usd: 0,
        },
      },
    });

    runPnLReportMock.mockResolvedValueOnce({ status: 200, etag: 'etag', report });

    render(<PnL />);
    await flushEffects();

    const fvCell = screen.getByText('123.450000').closest('td');
    expect(screen.getAllByText('futu_md').length).toBeGreaterThanOrEqual(1);
    expect(fvCell?.getAttribute('title') || '').toMatch(/>60s/);
    vi.useRealTimers();
  });
});

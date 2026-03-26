import { render, screen, waitFor, act } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { beforeEach, afterEach, describe, expect, it, vi } from 'vitest';

import Balances from './Balances';
import { api } from './api';
import { PanelWrapper } from './components/layout/PanelWrapper';
import { useBalancesStore } from './stores';
import { INTERVALS } from './constants';

const mockIsRealtimeStandardEnabled = vi.hoisted(() => vi.fn(() => false));
const mockUseStandardWebSocketSubscription = vi.hoisted(() => vi.fn());

vi.mock('./api', async (importOriginal) => {
  const actual = await importOriginal<typeof import('./api')>();
  return {
    api: {
      ...actual.api,
      getBalances: vi.fn(),
    },
  };
});

vi.mock('sonner', () => ({
  toast: {
    success: vi.fn(),
    error: vi.fn(),
  },
}));

vi.mock('./config/featureFlags', async (importOriginal) => {
  const actual = await importOriginal<typeof import('./config/featureFlags')>();
  return {
    ...actual,
    isRealtimeStandardEnabled: (...args: unknown[]) => mockIsRealtimeStandardEnabled(...args),
  };
});

const mockedApi = vi.mocked(api, true);
const mockUsePolling = vi.fn();
let seenFetchFns: WeakSet<() => unknown>;

function setPathname(pathname: string) {
  (window.location as unknown as { pathname?: string }).pathname = pathname;
}

vi.mock('./hooks', async (importOriginal) => {
  const actual = await importOriginal<typeof import('./hooks')>();
  return {
    ...actual,
    usePolling: (fn: () => unknown | Promise<unknown>, interval: number, enabled?: boolean) =>
      mockUsePolling(fn, interval, enabled),
    useWebSocket: vi.fn(),
    useStandardWebSocketSubscription: (...args: unknown[]) =>
      mockUseStandardWebSocketSubscription(...args),
  };
});

const buildPayload = () => ({
  rows: [
    {
      id: 'PLUME_LOGICAL',
      coin: 'PLUME_LOGICAL',
      canonical: 'PLUME',
      is_parent: true,
      stable: false,
      qty_display: '1,500',
      qty_raw: 1500,
      mv_display: '$75.50',
      mv_raw: 75.5,
      mark_display: '0.0503',
      mark_raw: 0.0503,
      time_display: '1h ago',
      time_iso: '2024-01-01T01:00:00Z',
      last_ts: 1704070800000,
      raw: { qty: 1500, mv_usd: 75.5, mark: 0.0503 },
      children: [
        {
          id: 'PLUME_LOGICAL:PLUME:bybit_plume',
          parent_id: 'PLUME_LOGICAL',
          coin: 'PLUME',
          display_name_short: 'PLUME Perp',
          display_name_long: 'Bybit PLUME Perp',
          product_type: 'perp',
          inventory_asset: 'PLUME',
          venue: 'bybit',
          wallet: 'bybit-unified',
          address: null,
          label: null,
          contract: 'PLUMEUSDT-LINEAR.BYBIT',
          qty_display: '1,000',
          qty_raw: 1000,
          mv_display: '$50.00',
          mv_raw: 50,
          mark_display: '0.0500',
          mark_raw: 0.05,
          time_display: '1h ago',
          time_iso: '2024-01-01T00:00:00Z',
          last_ts: 1704067200000,
        },
        {
          id: 'PLUME_LOGICAL:WPLUME:wallet_wplume',
          parent_id: 'PLUME_LOGICAL',
          coin: 'WPLUME',
          venue: 'wallet',
          wallet: 'treasury',
          address: '0xabc1234567890000000000000000000000000000',
          label: 'Treasury',
          qty_display: '500',
          qty_raw: 500,
          mv_display: '$25.50',
          mv_raw: 25.5,
          mark_display: '0.0510',
          mark_raw: 0.051,
          time_display: '1h ago',
          time_iso: '2024-01-01T01:00:00Z',
          last_ts: 1704070800000,
        },
      ],
    },
  ],
  total: 1,
  totals: {
    mv_raw: 75.5,
    mv_display: '$75.50',
    net_mv_raw: 75.5,
    net_mv_display: '$75.50',
    long_mv_raw: 75.5,
    long_mv_display: '$75.50',
    short_mv_raw: 0,
    short_mv_display: '$0.00',
    gross_mv_raw: 75.5,
    gross_mv_display: '$75.50',
    stable_mv_raw: 0,
    stable_mv_display: '$0.00',
    non_stable_mv_raw: 75.5,
    non_stable_mv_display: '$75.50',
    account_equity_raw: 7478.386872,
    account_equity_display: '$7478.39',
    withdrawable_raw: 7478.386872,
    withdrawable_display: '$7478.39',
  },
  generated_at: '2024-01-01T01:05:00Z',
  view: 'parents_only',
  risk_groups: [],
});

const buildStandardPayload = () => ({
  ...buildPayload(),
  realtime: {
    contract_version: 2,
    surface: 'balances',
    profile: 'tokenmm',
    surface_query_key: 'balances|profile=tokenmm|strategy_ids=strategy_01',
    stream_id: 'balances:tokenmm:strategy_01',
    snapshot_revision: 'balances-snapshot-1',
    last_seq: 0,
    capabilities: {
      recovery_mode: 'invalidate_only',
      replay_supported: false,
      transport_mode: 'polling_only',
    },
  },
});

describe('Balances component', () => {
  beforeEach(() => {
    seenFetchFns = new WeakSet();
    setPathname('/balances');
    localStorage.clear();
    mockIsRealtimeStandardEnabled.mockReturnValue(false);
    useBalancesStore.setState({
      rows: [],
      totals: null,
      totalCount: 0,
      generatedAt: undefined,
      loading: false,
      lastUpdate: undefined,
      degraded: false,
      scopeStatus: [],
    });
    mockedApi.getBalances.mockResolvedValue(buildPayload());
    mockUsePolling.mockImplementation((fn, _interval, enabled = true) => {
      if (!enabled) return;
      if (seenFetchFns.has(fn)) {
        return;
      }
      seenFetchFns.add(fn);
      Promise.resolve().then(() => {
        void fn();
      });
    });
  });

  afterEach(() => {
    vi.clearAllMocks();
    mockUsePolling.mockReset();
  });

  it('requests contract_version=2 snapshots and standard socket lineage when balances realtime standard is enabled', async () => {
    mockIsRealtimeStandardEnabled.mockReturnValue(true);
    mockedApi.getBalances.mockResolvedValue(buildStandardPayload() as any);

    render(<Balances />);

    await waitFor(() => {
      expect(mockedApi.getBalances).toHaveBeenCalledWith({ contractVersion: 2 });
    });

    expect(mockUseStandardWebSocketSubscription).toHaveBeenCalledWith(
      expect.objectContaining({
        enabled: true,
        lineage: expect.objectContaining({
          contract_version: 2,
          surface: 'balances',
          stream_id: 'balances:tokenmm:strategy_01',
          snapshot_revision: 'balances-snapshot-1',
          last_seq: 0,
        }),
      }),
    );
  });

  it('polls balances at the configured interval', async () => {
    render(<Balances />);
    await waitFor(() => {
      expect(mockUsePolling).toHaveBeenCalledWith(expect.any(Function), INTERVALS.BALANCES_POLL, true);
      expect(mockedApi.getBalances).toHaveBeenCalledTimes(1);
    });

    const [pollFn] = mockUsePolling.mock.calls[0];
    await act(async () => {
      await pollFn();
    });

    await waitFor(() => {
      expect(mockedApi.getBalances).toHaveBeenCalledTimes(2);
    });
  });

  it('renders parent rows with totals', async () => {
    render(<Balances />);
    await waitFor(() => {
      expect(screen.getByText('PLUME')).toBeInTheDocument();
    });

    expect(screen.getByText('Net Equity (Σ MV): $75.50')).toBeInTheDocument();
    expect(screen.getByText('Account Equity')).toBeInTheDocument();
    expect(screen.getByText('Withdrawable')).toBeInTheDocument();
    expect(screen.getAllByText('$7478.39')).toHaveLength(2);
  });

  it('renders degraded shared-account scope status from balances payload', async () => {
    mockedApi.getBalances.mockResolvedValueOnce({
      ...buildPayload(),
      degraded: true,
      scope_status: [
        {
          account_scope_id: 'ibkr.reference.main',
          source_scope: 'shared_account',
          projection_status: {
            healthy: false,
            last_success_ts_ms: 1704067200000,
            last_attempt_ts_ms: 1704067216000,
            last_error_type: 'TimeoutError',
            last_error_message: '',
            stale_after_ms: 15000,
          },
        },
        {
          account_scope_id: 'binance.futures.main',
          source_scope: 'shared_account',
          projection_status: {
            healthy: true,
            last_success_ts_ms: 1704067216000,
            last_attempt_ts_ms: 1704067216000,
            last_error_type: null,
            last_error_message: null,
            stale_after_ms: 15000,
          },
        },
      ],
    } as any);

    render(<Balances />);

    await screen.findByText('Degraded reconciliation');
    expect(screen.getByText(/ibkr\.reference\.main stale · TimeoutError/i)).toBeInTheDocument();
    expect(screen.getByText(/binance\.futures\.main healthy/i)).toBeInTheDocument();
  });

  it('keeps non-zero quantity rows visible when market value is temporarily unavailable', async () => {
    mockedApi.getBalances.mockResolvedValueOnce({
      rows: [
        {
          id: 'AAPL_LOGICAL',
          coin: 'AAPL_LOGICAL',
          canonical: 'AAPL',
          is_parent: true,
          stable: false,
          qty_display: '5',
          qty_raw: 5,
          mv_display: '$0.00',
          mv_raw: 0,
          mark_display: null,
          mark_raw: null,
          time_display: '1h ago',
          time_iso: '2024-01-01T01:00:00Z',
          last_ts: 1704070800000,
          raw: { qty: 5, mv_usd: 0, mark: null },
          children: [
            {
              id: 'AAPL_LOGICAL:AAPL:ibkr',
              parent_id: 'AAPL_LOGICAL',
              coin: 'AAPL',
              display_name_short: 'AAPL Spot',
              venue: 'ibkr',
              wallet: 'shared',
              qty_display: '5',
              qty_raw: 5,
              mv_display: '$0.00',
              mv_raw: 0,
              mark_display: null,
              mark_raw: null,
              time_display: '1h ago',
              time_iso: '2024-01-01T01:00:00Z',
              last_ts: 1704070800000,
            },
          ],
        },
      ],
      total: 1,
      totals: { mv_raw: 0, mv_display: '$0.00' },
      generated_at: '2024-01-01T01:05:00Z',
      view: 'parents_only',
      risk_groups: [],
    } as any);

    render(<Balances />);

    await waitFor(() => {
      expect(screen.getByText('AAPL')).toBeInTheDocument();
    });

    expect(screen.queryByText('No balances found')).not.toBeInTheDocument();
  });

  it('formats balance MV cells with thousands separators from raw values', async () => {
    mockedApi.getBalances.mockResolvedValueOnce({
      ...buildPayload(),
      rows: [
        {
          ...buildPayload().rows[0],
          id: 'AAPL_LOGICAL',
          coin: 'AAPL_LOGICAL',
          canonical: 'AAPL',
          qty_display: '10',
          qty_raw: 10,
          mv_display: '$2550.00',
          mv_raw: 2550,
          mark_display: '255',
          mark_raw: 255,
          raw: { qty: 10, mv_usd: 2550, mark: 255 },
          children: [
            {
              ...buildPayload().rows[0].children[0],
              id: 'AAPL_LOGICAL:AAPL:ibkr',
              parent_id: 'AAPL_LOGICAL',
              coin: 'AAPL',
              display_name_short: 'AAPL Spot',
              display_name_long: 'IBKR AAPL Spot',
              venue: 'ibkr',
              wallet: 'shared',
              qty_display: '10',
              qty_raw: 10,
              mv_display: '$2550.00',
              mv_raw: 2550,
              mark_display: '255',
              mark_raw: 255,
            },
          ],
        },
      ],
      totals: { mv_raw: 2550, mv_display: '$2550.00' },
    } as any);

    render(<Balances />);

    await waitFor(() => {
      expect(screen.getByText('AAPL')).toBeInTheDocument();
    });

    expect(screen.getAllByText('$2,550.00')).toHaveLength(2);
  });

  it('prefers authoritative totals.mv_display when net_mv_display is missing', async () => {
    const payload = buildPayload();
    mockedApi.getBalances.mockResolvedValueOnce({
      ...payload,
      totals: {
        mv_raw: 999.12,
        mv_display: '$999.12',
      },
    } as any);

    render(<Balances />);
    await waitFor(() => {
      expect(screen.getByText('PLUME')).toBeInTheDocument();
    });

    expect(screen.getByText('Net Equity (Σ MV): $999.12')).toBeInTheDocument();
  });

  it('renders master account totals even when only balances totals are available', async () => {
    const payload = buildPayload();
    mockedApi.getBalances.mockResolvedValueOnce({
      ...payload,
      totals: {
        mv_raw: 1075.37415731,
        mv_display: '$1075.37',
        account_equity_raw: 7478.386872,
        account_equity_display: '$7478.39',
        withdrawable_raw: 7478.386872,
        withdrawable_display: '$7478.39',
      },
    } as any);

    render(<Balances />);
    await waitFor(() => {
      expect(screen.getByText('PLUME')).toBeInTheDocument();
    });

    expect(screen.getByText('Account Equity')).toBeInTheDocument();
    expect(screen.getByText('Withdrawable')).toBeInTheDocument();
    expect(screen.getAllByText('$7478.39')).toHaveLength(2);
  });

  it('rerenders when only master account totals change across polls', async () => {
    const initialPayload = buildPayload();
    const updatedPayload = buildPayload();
    initialPayload.totals = {
      mv_raw: 1075.37415731,
      mv_display: '$1075.37',
      account_equity_raw: 7478.386872,
      account_equity_display: '$7478.39',
      withdrawable_raw: 7478.386872,
      withdrawable_display: '$7478.39',
    } as any;
    updatedPayload.totals = {
      mv_raw: 1075.37415731,
      mv_display: '$1075.37',
      account_equity_raw: 8123.45,
      account_equity_display: '$8123.45',
      withdrawable_raw: 7999.01,
      withdrawable_display: '$7999.01',
    } as any;

    mockedApi.getBalances
      .mockResolvedValueOnce(initialPayload as any)
      .mockResolvedValueOnce(updatedPayload as any);

    render(<Balances />);

    await waitFor(() => {
      expect(screen.getByText('Account Equity')).toBeInTheDocument();
    });
    expect(screen.getAllByText('$7478.39')).toHaveLength(2);
    expect(screen.queryByText('$7999.01')).not.toBeInTheDocument();

    const [pollFn] = mockUsePolling.mock.calls[0];
    await act(async () => {
      await pollFn();
    });

    await waitFor(() => {
      expect(screen.getByText('$8123.45')).toBeInTheDocument();
      expect(screen.getByText('$7999.01')).toBeInTheDocument();
    });
  });

  it('defaults to expanded rows and allows collapsing from the header control', async () => {
    const user = userEvent.setup();
    render(<Balances />);

    await waitFor(() => {
      expect(screen.getByText('PLUME')).toBeInTheDocument();
    });

    const expandAllButton = await screen.findByRole('button', { name: /collapse all/i });

    // Parents with children should be expanded by default
    expect(await screen.findByText('0xabc1…0000')).toBeInTheDocument();

    await user.click(expandAllButton);

    // Button should switch to "Expand all" and rows collapse when clicked
    expect(expandAllButton).toHaveTextContent(/expand all/i);
    await waitFor(() => {
      expect(screen.queryByText('0xabc1…0000')).not.toBeInTheDocument();
    });

    await user.click(expandAllButton);
    expect(await screen.findByText('0xabc1…0000')).toBeInTheDocument();
  });

  it('renders full product contract inline for exchange balances', async () => {
    render(<Balances />);

    await waitFor(() => {
      expect(screen.getByText('PLUME')).toBeInTheDocument();
    });

    expect(
      screen.getAllByText((_, element) =>
        String(element?.textContent ?? '').includes('bybit') &&
        String(element?.textContent ?? '').includes('bybit-unified') &&
        String(element?.textContent ?? '').includes('PLUMEUSDT-LINEAR.BYBIT')
      ).length
    ).toBeGreaterThan(0);
    expect(screen.queryByText('PLUMEU...YBIT')).not.toBeInTheDocument();
  });

  it('prefers canonical child display names for instrument rows', async () => {
    render(<Balances />);

    await waitFor(() => {
      expect(screen.getByText('PLUME Perp')).toBeInTheDocument();
    });
  });

  it('surfaces header actions when embedded in dashboard wrapper', async () => {
    render(
      <PanelWrapper title="Balances">
        <Balances showHeader={false} />
      </PanelWrapper>
    );

    await waitFor(() => {
      expect(screen.getByText('PLUME')).toBeInTheDocument();
    });

    await screen.findByText('Balances');

    await screen.findByRole('button', { name: /collapse all/i });
    await screen.findByRole('button', { name: /filters/i });
  });

  it('switches to child view when logical parents are disabled', async () => {
    const user = userEvent.setup();
    render(<Balances />);

    await waitFor(() => {
      expect(screen.getByText('PLUME')).toBeInTheDocument();
    });

    await user.click(screen.getByRole('button', { name: /Filters/i }));
    const switches = screen.getAllByRole('switch');
    // Toggle "Show logical parents only" (second switch)
    await user.click(switches[1]);

    expect(await screen.findByText(/wallet \(treasury\)/i)).toBeInTheDocument();
  });

  it('renders wallet addresses with copy-to-clipboard support for child rows', async () => {
    const user = userEvent.setup();
    localStorage.setItem(
      'balances:filters:v1',
      JSON.stringify({
        hideZero: true,
        logicalOnly: false,
        stableOnly: false,
        sortBy: 'mv',
      }),
    );

    const clipboardMock = {
      writeText: vi.fn(() => Promise.resolve()),
    } as unknown as Clipboard;
    const originalClipboard = navigator.clipboard;
    Object.defineProperty(window.navigator, 'clipboard', {
      configurable: true,
      value: clipboardMock,
    });

    render(<Balances />);
    const addressButton = await screen.findByRole('button', { name: /copy wallet address/i });
    expect(addressButton).toHaveTextContent('0xabc1…0000');

    await user.click(addressButton);

    await waitFor(() => {
      expect(clipboardMock.writeText).toHaveBeenCalledWith('0xabc1234567890000000000000000000000000000');
    });

    if (originalClipboard) {
      Object.defineProperty(window.navigator, 'clipboard', {
        configurable: true,
        value: originalClipboard,
      });
    } else {
      // @ts-expect-error clean up mocked clipboard
      delete navigator.clipboard;
    }
  });

  it('sorts rows when clicking sortable headers', async () => {
    const user = userEvent.setup();
    const payload = {
      rows: [
        {
          id: 'BBB_LOGICAL',
          coin: 'BBB_LOGICAL',
          canonical: 'BBB',
          is_parent: true,
          stable: false,
          qty_display: '2,000',
          qty_raw: 2000,
          mv_display: '$20.00',
          mv_raw: 20,
          mark_display: '10.0',
          mark_raw: 10,
          time_display: 'just now',
          time_iso: '2024-01-01T00:00:00Z',
          last_ts: 1704067200000,
          raw: { qty: 2000, mv_usd: 20, mark: 10 },
          children: [],
        },
        {
          id: 'AAA_LOGICAL',
          coin: 'AAA_LOGICAL',
          canonical: 'AAA',
          is_parent: true,
          stable: false,
          qty_display: '1,000',
          qty_raw: 1000,
          mv_display: '$10.00',
          mv_raw: 10,
          mark_display: '9.0',
          mark_raw: 9,
          time_display: 'just now',
          time_iso: '2024-01-01T00:00:00Z',
          last_ts: 1704067200000,
          raw: { qty: 1000, mv_usd: 10, mark: 9 },
          children: [],
        },
      ],
      total: 2,
      totals: { mv_raw: 30, mv_display: '$30.00' },
      generated_at: '2024-01-01T00:00:00Z',
      view: 'parents_only',
    };
    mockedApi.getBalances.mockResolvedValue(payload);

    render(<Balances />);

    await waitFor(() => {
      expect(screen.getByText('BBB')).toBeInTheDocument();
    });

    const coinCells = screen.getAllByRole('cell', { name: /AAA|BBB/ });
    expect(coinCells[0]).toHaveTextContent('BBB');
    expect(coinCells[1]).toHaveTextContent('AAA');

    await user.click(screen.getByRole('button', { name: /sort by coin/i }));

    const resorted = screen.getAllByRole('cell', { name: /AAA|BBB/ });
    expect(resorted[0]).toHaveTextContent('AAA');
    expect(resorted[1]).toHaveTextContent('BBB');
  });

  it('renders Risk view using risk_groups data', async () => {
    const user = userEvent.setup();
    const payload = {
      ...buildPayload(),
      risk_groups: [
        {
          risk_key: 'PLUME',
          label: 'PLUME',
          net_qty: 1500,
          net_mv: 75.5,
          long_mv: 75.5,
          short_mv: 0,
          gross_mv: 75.5,
          abs_net_mv: 75.5,
          hedge_ratio: 0,
          sources: ['bybit', 'wallet'],
        },
      ],
    };
    mockedApi.getBalances.mockResolvedValue(payload);

    render(<Balances />);

    await waitFor(() => {
      expect(screen.getByText('PLUME')).toBeInTheDocument();
    });

    await user.click(screen.getByRole('button', { name: 'Risk' }));

    // Risk row label + sources badges should be visible
    const plumeLabels = await screen.findAllByText('PLUME');
    expect(plumeLabels.length).toBeGreaterThan(0);
    expect(await screen.findByText('bybit')).toBeInTheDocument();
    expect(await screen.findByText('wallet')).toBeInTheDocument();
  });

  it('rerenders child metadata when only rendered balance labels change across polls', async () => {
    const initialPayload = buildPayload();
    const updatedPayload = buildPayload();
    updatedPayload.rows[0].children[0].display_name_short = 'PLUME Treasury Position';
    updatedPayload.rows[0].children[0].display_name_long = 'Bybit PLUME Treasury Position';

    mockedApi.getBalances
      .mockResolvedValueOnce(initialPayload as any)
      .mockResolvedValueOnce(updatedPayload as any);

    render(<Balances />);

    await waitFor(() => {
      expect(screen.getByText('PLUME Perp')).toBeInTheDocument();
    });

    const [pollFn] = mockUsePolling.mock.calls[0];
    await act(async () => {
      await pollFn();
    });

    await waitFor(() => {
      expect(screen.getByText('PLUME Treasury Position')).toBeInTheDocument();
    });
    expect(screen.queryByText('PLUME Perp')).not.toBeInTheDocument();
  });

  it('does not apply balances filters persisted for another profile', async () => {
    setPathname('/tokenmm/balances');
    localStorage.setItem(
      'balances:filters:v2',
      JSON.stringify({
        hideZero: true,
        logicalOnly: true,
        stableOnly: false,
        sortBy: 'mv',
        sortDir: 'desc',
        columnFilters: { coin: 'PLUME' },
      }),
    );
    setPathname('/equities/balances');
    mockedApi.getBalances.mockResolvedValueOnce({
      ...buildPayload(),
      rows: [
        {
          ...buildPayload().rows[0],
          id: 'AAPL_LOGICAL',
          coin: 'AAPL_LOGICAL',
          canonical: 'AAPL',
          children: [
            {
              ...buildPayload().rows[0].children[0],
              id: 'AAPL_LOGICAL:AAPL:ibkr_aapl',
              parent_id: 'AAPL_LOGICAL',
              coin: 'AAPL',
              display_name_short: 'AAPL Spot',
              display_name_long: 'IBKR AAPL Spot',
              inventory_asset: 'AAPL',
              venue: 'ibkr',
              wallet: 'main',
            },
          ],
          raw: { qty: 10, mv_usd: 2550, mark: 255 },
          qty_display: '10',
          qty_raw: 10,
          mv_display: '$2550.00',
          mv_raw: 2550,
          mark_display: '255',
          mark_raw: 255,
        },
      ],
      totals: { mv_raw: 2550, mv_display: '$2550.00' },
    });

    render(<Balances />);

    await waitFor(() => {
      expect(mockedApi.getBalances).toHaveBeenCalledTimes(1);
      expect(screen.queryByText('No balances found')).not.toBeInTheDocument();
    });
  });

  it('keeps gross-but-net-flat risk rows visible by default', async () => {
    const user = userEvent.setup();
    mockedApi.getBalances.mockResolvedValueOnce({
      ...buildPayload(),
      risk_groups: [
        {
          risk_key: 'PAIR_BOOK',
          label: 'Pair Book',
          net_qty: 0,
          net_mv: 0,
          long_mv: 37.75,
          short_mv: -37.75,
          gross_mv: 75.5,
          abs_net_mv: 0,
          hedge_ratio: 1,
          sources: ['bybit', 'okx'],
          rows: [],
        },
      ],
    } as any);

    render(<Balances />);

    await waitFor(() => {
      expect(screen.getByText('PLUME')).toBeInTheDocument();
    });

    await user.click(screen.getByRole('button', { name: 'Risk' }));

    expect(await screen.findByText('Pair Book')).toBeInTheDocument();
  });

  it('uses backend risk breakdown rows when expanding a risk group', async () => {
    const user = userEvent.setup();
    mockedApi.getBalances.mockResolvedValueOnce({
      rows: [
        {
          id: 'STABLES_LOGICAL',
          coin: 'STABLES_LOGICAL',
          canonical: 'STABLES',
          is_parent: true,
          stable: true,
          qty_display: '100',
          qty_raw: 100,
          mv_display: '$100.00',
          mv_raw: 100,
          mark_display: '1.0',
          mark_raw: 1,
          time_display: '1h ago',
          time_iso: '2024-01-01T01:00:00Z',
          last_ts: 1704070800000,
          raw: { qty: 100, mv_usd: 100, mark: 1 },
          children: [
            {
              id: 'stable-parent:usdc',
              parent_id: 'STABLES_LOGICAL',
              coin: 'USDC',
              display_name_short: 'Treasury USDC',
              venue: 'wallet',
              wallet: 'treasury',
              qty_display: '50',
              qty_raw: 50,
              mv_display: '$50.00',
              mv_raw: 50,
              mark_display: '1.0',
              mark_raw: 1,
              time_display: '1h ago',
              time_iso: '2024-01-01T01:00:00Z',
              last_ts: 1704070800000,
              risk_key: 'STABLE_BUCKET',
              risk_label: 'Stable Bucket',
            },
            {
              id: 'stable-parent:usdt',
              parent_id: 'STABLES_LOGICAL',
              coin: 'USDT',
              display_name_short: 'Ops USDT',
              venue: 'wallet',
              wallet: 'ops',
              qty_display: '50',
              qty_raw: 50,
              mv_display: '$50.00',
              mv_raw: 50,
              mark_display: '1.0',
              mark_raw: 1,
              time_display: '1h ago',
              time_iso: '2024-01-01T01:00:00Z',
              last_ts: 1704070800000,
              risk_key: 'USD_CASH',
              risk_label: 'USD Cash',
            },
          ],
        },
      ],
      total: 1,
      totals: { mv_raw: 100, mv_display: '$100.00' },
      generated_at: '2024-01-01T01:05:00Z',
      view: 'parents_only',
      risk_groups: [
        {
          risk_key: 'STABLE_BUCKET',
          label: 'Stable Bucket',
          net_qty: 0,
          net_mv: 0,
          long_mv: 50,
          short_mv: -50,
          gross_mv: 100,
          abs_net_mv: 0,
          hedge_ratio: 1,
          sources: ['wallet'],
          rows: [
            { venue: 'wallet', coin: 'USDC', qty_raw: 50, mv_raw: 50, mark_raw: 1, time_display: '1h ago' },
            { venue: 'wallet', coin: 'USDT', qty_raw: -50, mv_raw: -50, mark_raw: 1, time_display: '1h ago' },
          ],
        },
      ],
    } as any);

    render(<Balances />);

    await waitFor(() => {
      expect(screen.getByText('STABLES')).toBeInTheDocument();
    });

    await user.click(screen.getByRole('button', { name: 'Risk' }));
    await user.click(screen.getByRole('button', { name: /expand risk sources/i }));

    expect(await screen.findByText('USDC')).toBeInTheDocument();
    expect(await screen.findByText('USDT')).toBeInTheDocument();
  });

  it('filters holdings using backend risk keys when a risk row is selected', async () => {
    const user = userEvent.setup();
    mockedApi.getBalances.mockResolvedValueOnce({
      rows: [
        {
          id: 'STABLES_LOGICAL',
          coin: 'STABLES_LOGICAL',
          canonical: 'STABLES',
          is_parent: true,
          stable: true,
          qty_display: '100',
          qty_raw: 100,
          mv_display: '$100.00',
          mv_raw: 100,
          mark_display: '1.0',
          mark_raw: 1,
          time_display: '1h ago',
          time_iso: '2024-01-01T01:00:00Z',
          last_ts: 1704070800000,
          raw: { qty: 100, mv_usd: 100, mark: 1 },
          children: [
            {
              id: 'stable-parent:usdc',
              parent_id: 'STABLES_LOGICAL',
              coin: 'USDC',
              display_name_short: 'Treasury USDC',
              venue: 'wallet',
              wallet: 'treasury',
              qty_display: '50',
              qty_raw: 50,
              mv_display: '$50.00',
              mv_raw: 50,
              mark_display: '1.0',
              mark_raw: 1,
              time_display: '1h ago',
              time_iso: '2024-01-01T01:00:00Z',
              last_ts: 1704070800000,
              risk_key: 'STABLE_BUCKET',
              risk_label: 'Stable Bucket',
            },
            {
              id: 'stable-parent:usdt',
              parent_id: 'STABLES_LOGICAL',
              coin: 'USDT',
              display_name_short: 'Ops USDT',
              venue: 'wallet',
              wallet: 'ops',
              qty_display: '50',
              qty_raw: 50,
              mv_display: '$50.00',
              mv_raw: 50,
              mark_display: '1.0',
              mark_raw: 1,
              time_display: '1h ago',
              time_iso: '2024-01-01T01:00:00Z',
              last_ts: 1704070800000,
              risk_key: 'USD_CASH',
              risk_label: 'USD Cash',
            },
          ],
        },
      ],
      total: 1,
      totals: { mv_raw: 100, mv_display: '$100.00' },
      generated_at: '2024-01-01T01:05:00Z',
      view: 'parents_only',
      risk_groups: [
        {
          risk_key: 'STABLE_BUCKET',
          label: 'Stable Bucket',
          net_qty: 50,
          net_mv: 50,
          long_mv: 50,
          short_mv: 0,
          gross_mv: 50,
          abs_net_mv: 50,
          hedge_ratio: 0,
          sources: ['wallet'],
          rows: [
            { venue: 'wallet', coin: 'USDC', qty_raw: 50, mv_raw: 50, mark_raw: 1, time_display: '1h ago' },
          ],
        },
      ],
    } as any);

    render(<Balances />);

    await waitFor(() => {
      expect(screen.getByText('STABLES')).toBeInTheDocument();
    });

    await user.click(screen.getByRole('button', { name: 'Risk' }));
    await user.click(screen.getByRole('button', { name: /filter holdings by underlying/i }));

    expect(await screen.findByText('Treasury USDC')).toBeInTheDocument();
    expect(screen.queryByText('Ops USDT')).not.toBeInTheDocument();
  });
});

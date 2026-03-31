import { act, render, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import Balances from './Balances';
import { api } from './api';
import { INTERVALS } from './constants';
import { useBalancesStore } from './stores';

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

function buildPayload() {
  return {
    rows: [
      {
        id: 'USDC_LOGICAL',
        coin: 'USDC_LOGICAL',
        canonical: 'USDC',
        is_parent: true,
        stable: true,
        qty_display: '1000',
        qty_raw: 1000,
        mv_display: '$1000.00',
        mv_raw: 1000,
        mark_display: '1.0000',
        mark_raw: 1,
        time_display: '2024-01-01T01:04:55.000Z',
        time_iso: '2024-01-01T01:04:55.000Z',
        last_ts: 1704071095000,
        raw: { qty: 1000, mv_usd: 1000, mark: 1 },
        children: [
          {
            id: 'USDC_LOGICAL:USDC:bybit',
            parent_id: 'USDC_LOGICAL',
            coin: 'USDC',
            display_name_short: 'Bybit USDC Spot',
            display_name_long: 'Bybit USDC Unified Spot',
            product_type: 'spot',
            contract_type: 'cash',
            raw_symbol: 'USDC',
            venue: 'bybit',
            wallet: 'bybit-unified',
            address: null,
            label: null,
            contract: 'USDC',
            qty_display: '1000',
            qty_raw: 1000,
            mv_display: '$1000.00',
            mv_raw: 1000,
            mark_display: '1.0000',
            mark_raw: 1,
            time_display: '2024-01-01T01:04:55.000Z',
            time_iso: '2024-01-01T01:04:55.000Z',
            last_ts: 1704071095000,
          },
        ],
      },
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
        time_display: '2024-01-01T01:04:55.000Z',
        time_iso: '2024-01-01T01:04:55.000Z',
        last_ts: 1704071095000,
        raw: { qty: 1500, mv_usd: 75.5, mark: 0.0503 },
        children: [
          {
            id: 'PLUME_LOGICAL:PLUME:bybit_plume',
            parent_id: 'PLUME_LOGICAL',
            coin: 'PLUME',
            display_name_short: 'Bybit PLUME Perp',
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
            time_display: '2024-01-01T01:03:00.000Z',
            time_iso: '2024-01-01T01:03:00.000Z',
            last_ts: 1704070980000,
          },
          {
            id: 'PLUME_LOGICAL:WPLUME:wallet_wplume',
            parent_id: 'PLUME_LOGICAL',
            coin: 'WPLUME',
            venue: 'wallet',
            wallet: 'treasury',
            address: '0xabc1234567890000000000000000000000000000',
            label: 'Treasury',
            contract_type: 'cash',
            raw_symbol: 'WPLUME',
            qty_display: '500',
            qty_raw: 500,
            mv_display: '$25.50',
            mv_raw: 25.5,
            mark_display: '0.0510',
            mark_raw: 0.051,
            time_display: '2024-01-01T01:04:55.000Z',
            time_iso: '2024-01-01T01:04:55.000Z',
            last_ts: 1704071095000,
          },
        ],
      },
      {
        id: 'ZERO_LOGICAL',
        coin: 'ZERO_LOGICAL',
        canonical: 'ZERO',
        is_parent: true,
        stable: false,
        qty_display: '0',
        qty_raw: 0,
        mv_display: '$0.00',
        mv_raw: 0,
        mark_display: '0.0000',
        mark_raw: 0,
        time_display: '2024-01-01T01:04:55.000Z',
        time_iso: '2024-01-01T01:04:55.000Z',
        last_ts: 1704071095000,
        raw: { qty: 0, mv_usd: 0, mark: 0 },
        children: [
          {
            id: 'ZERO_LOGICAL:ZERO:wallet',
            parent_id: 'ZERO_LOGICAL',
            coin: 'ZERO',
            venue: 'wallet',
            wallet: 'dust-wallet',
            contract_type: 'cash',
            raw_symbol: 'ZERO',
            qty_display: '0',
            qty_raw: 0,
            mv_display: '$0.00',
            mv_raw: 0,
            mark_display: '0.0000',
            mark_raw: 0,
            time_display: '2024-01-01T01:04:55.000Z',
            time_iso: '2024-01-01T01:04:55.000Z',
            last_ts: 1704071095000,
          },
        ],
      },
    ],
    total: 3,
    totals: {
      mv_raw: 1075.5,
      mv_display: '$1075.50',
      stable_mv_raw: 1000,
      stable_mv_display: '$1000.00',
      non_stable_mv_raw: 75.5,
      non_stable_mv_display: '$75.50',
    },
    generated_at: '2024-01-01T01:05:00.000Z',
    view: 'parents_only',
    source: 'portfolio_snapshot',
    stale_after_ms: 30000,
    aggregation_mode: 'partial',
    components: [],
    missing_required: [],
    stale_required: [],
    null_qty_required: [],
    degraded: false,
    scope_status: [],
    risk_groups: [],
  };
}

function buildStandardPayload() {
  return {
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
  };
}

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
    mockedApi.getBalances.mockResolvedValue(buildPayload() as any);
    mockUsePolling.mockImplementation((fn, _interval, enabled = true) => {
      if (!enabled) return;
      if (seenFetchFns.has(fn)) return;
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

  it('renders standard-mode balance rows without falling into the loading empty state', async () => {
    mockIsRealtimeStandardEnabled.mockReturnValue(true);
    mockedApi.getBalances.mockResolvedValue(buildStandardPayload() as any);

    render(<Balances />);

    await waitFor(() => {
      expect(screen.getByText('PLUME')).toBeInTheDocument();
    });

    expect(screen.queryByText('Loading balances...')).not.toBeInTheDocument();
    expect(screen.queryByText('No balances found')).not.toBeInTheDocument();
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

  it('renders the tokenmm holdings layout and expands grouped rows', async () => {
    const user = userEvent.setup();
    render(<Balances />);

    await screen.findByText('Shared snapshot live');
    expect(screen.queryByRole('button', { name: 'Risk' })).not.toBeInTheDocument();

    expect(screen.getByText('Total Inventory')).toBeInTheDocument();
    expect(screen.getByText('Stable Inventory')).toBeInTheDocument();
    expect(screen.getByText('Trading Inventory')).toBeInTheDocument();
    expect(screen.getByText('Non-zero Coins')).toBeInTheDocument();
    expect(screen.getByText('Stale Rows')).toBeInTheDocument();

    expect(screen.getByLabelText('Search holdings')).toBeInTheDocument();
    expect(screen.getByLabelText('Venue filter')).toBeInTheDocument();
    expect(screen.getByLabelText('Type filter')).toBeInTheDocument();
    expect(screen.getByLabelText('Hide zero balances')).toBeChecked();
    expect(screen.getByRole('button', { name: 'Expand all' })).toBeInTheDocument();

    expect(screen.getByText('Stables')).toBeInTheDocument();
    expect(screen.getByText('Trading Assets')).toBeInTheDocument();
    expect(screen.getAllByText('Coin').length).toBeGreaterThan(0);
    expect(screen.getAllByText('Net Qty').length).toBeGreaterThan(0);
    expect(screen.getAllByText('Net MV').length).toBeGreaterThan(0);
    expect(screen.getAllByText('Spot Qty').length).toBeGreaterThan(0);
    expect(screen.getAllByText('Perp Qty').length).toBeGreaterThan(0);
    expect(screen.getAllByText('Venues').length).toBeGreaterThan(0);
    expect(screen.getAllByText('Updated').length).toBeGreaterThan(0);
    expect(screen.getAllByText('Status').length).toBeGreaterThan(0);

    expect(screen.getByText('USDC')).toBeInTheDocument();
    expect(screen.getByText('PLUME')).toBeInTheDocument();
    expect(screen.queryByText('ZERO')).not.toBeInTheDocument();

    await user.click(screen.getByRole('button', { name: 'Expand all' }));

    expect(screen.getAllByText('Venue').length).toBeGreaterThan(0);
    expect(screen.getAllByText('Account').length).toBeGreaterThan(0);
    expect(screen.getAllByText('Type').length).toBeGreaterThan(0);
    expect(screen.getAllByText('Symbol / Instrument').length).toBeGreaterThan(0);
    expect(screen.getByText('Bybit PLUME Perp')).toBeInTheDocument();
    expect(screen.getAllByText('bybit-unified').length).toBeGreaterThan(0);
    expect(screen.getAllByText('Perp').length).toBeGreaterThan(0);
    expect(screen.getByText('PLUMEUSDT-LINEAR.BYBIT')).toBeInTheDocument();
  });

  it('wires the tokenmm toolbar filters into the holdings table', async () => {
    const user = userEvent.setup();
    render(<Balances />);

    await screen.findByText('PLUME');

    await user.type(screen.getByLabelText('Search holdings'), 'usdc');
    expect(screen.getByText('USDC')).toBeInTheDocument();
    expect(screen.queryByText('PLUME')).not.toBeInTheDocument();

    await user.clear(screen.getByLabelText('Search holdings'));
    await user.selectOptions(screen.getByLabelText('Venue filter'), 'wallet');
    expect(screen.queryByText('USDC')).not.toBeInTheDocument();
    expect(screen.getByText('PLUME')).toBeInTheDocument();
  });

  it('renders degraded shared snapshot status when reconciliation metadata is degraded', async () => {
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
      ],
    } as any);

    render(<Balances />);

    await screen.findByText('Degraded shared snapshot');
    expect(screen.getByText('1 degraded scope')).toBeInTheDocument();
  });
});

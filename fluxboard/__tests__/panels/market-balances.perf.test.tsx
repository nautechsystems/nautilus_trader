import { act, fireEvent, render, screen, waitFor } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { useEffect } from 'react';

import MarketData from '../../MarketData';
import Balances from '../../Balances';
import {
  __resetViewportClockRegistryForTests,
  getViewportClockDebugState,
} from '../../hooks/useViewportClock';
import { useMarketDataStore } from '../../stores/marketDataStore';
import { useBalancesStore } from '../../stores';

const marketSnapshotMock = vi.fn();
const balancesSnapshotMock = vi.fn();

const hookState = vi.hoisted(() => ({
  enabledSurfaces: new Set<string>(),
  websocketCalls: [] as Array<{
    event: string;
    surface?: string;
    handler: (payload: unknown) => void;
  }>,
  pollingCalls: [] as Array<{
    interval: number;
    enabled: boolean | undefined;
  }>,
}));

const flushTasks = async () => {
  await Promise.resolve();
  await Promise.resolve();
};

const buildMarketRows = (count: number) => Array.from({ length: count }, (_, index) => ({
  coin: `COIN-${String(index).padStart(3, '0')}`,
  exchange: index % 2 === 0 ? 'bybit' : 'binance',
  bid: String(100 + index),
  ask: String(101 + index),
  mid_px: String(100.5 + index),
  bid_qty: '1',
  ask_qty: '1',
  timestamp_ms: 1_700_000_000_000 + index,
}));

vi.mock('../../api', async (importOriginal) => {
  const actual = await importOriginal<typeof import('../../api')>();
  return {
    api: {
      ...actual.api,
      getMarketDataSnapshot: vi.fn(() => marketSnapshotMock()),
      getBalances: vi.fn(() => balancesSnapshotMock()),
    },
  };
});

vi.mock('../../config/featureFlags', async (importOriginal) => {
  const actual = await importOriginal<typeof import('../../config/featureFlags')>();
  return {
    ...actual,
    isRealtimeStandardEnabled: (surface: string) => hookState.enabledSurfaces.has(surface),
  };
});

vi.mock('../../hooks', async () => {
  const actual = await vi.importActual<any>('../../hooks');
  return {
    ...actual,
    usePolling: (
      fn: () => void | Promise<void>,
      interval: number,
      enabled?: boolean,
    ) => {
      hookState.pollingCalls.push({ interval, enabled });
      useEffect(() => {
        if (enabled === false) {
          return;
        }
        void fn();
      }, [fn, enabled]);
    },
    useWebSocket: (
      event: string,
      handler: (payload: unknown) => void,
      options?: { surface?: string },
    ) => {
      useEffect(() => {
        hookState.websocketCalls.push({ event, surface: options?.surface, handler });
      }, [event, handler, options?.surface]);
    },
  };
});

describe('market/balances realtime polling fallback', () => {
  beforeEach(() => {
    __resetViewportClockRegistryForTests();
    hookState.enabledSurfaces.clear();
    hookState.websocketCalls = [];
    hookState.pollingCalls = [];

    marketSnapshotMock.mockResolvedValue({
      rows: [
        {
          coin: 'BTC/USDT',
          exchange: 'bybit',
          bid: '100',
          ask: '101',
          mid_px: '100.5',
          bid_qty: '1',
          ask_qty: '1',
          timestamp_ms: 1700000000000,
        },
      ],
      count: 1,
      freshnessKey: 'market-freshness-1',
    });

    balancesSnapshotMock.mockResolvedValue({
      rows: [
        {
          id: 'BTC_LOGICAL',
          coin: 'BTC_LOGICAL',
          canonical: 'BTC',
          is_parent: true,
          stable: false,
          qty_display: '1',
          qty_raw: 1,
          mv_display: '$100.00',
          mv_raw: 100,
          mark_display: '100.00',
          mark_raw: 100,
          time_display: '2024-01-01T00:00:00Z',
          time_iso: '2024-01-01T00:00:00Z',
          last_ts: 1704067200000,
          raw: { qty: 1, mv_usd: 100, mark: 100 },
          children: [],
        },
      ],
      total: 1,
      totals: {
        mv_raw: 100,
        mv_display: '$100.00',
      },
      generated_at: '2024-01-01T00:00:00Z',
      view: 'parents_only',
      risk_groups: [],
    });
  });

  afterEach(() => {
    __resetViewportClockRegistryForTests();
    vi.useRealTimers();
    useMarketDataStore.setState({ rows: [], loading: false, lastUpdate: null });
    useBalancesStore.setState({
      rows: [],
      totals: null,
      totalCount: 0,
      generatedAt: undefined,
      loading: false,
      lastUpdate: undefined,
      riskGroups: [],
      riskSort: { key: 'risk_delta_pct', direction: 'desc' },
    });
    vi.clearAllMocks();
  });

  it('disables MarketData polling after the initial standard snapshot is loaded', async () => {
    hookState.enabledSurfaces.add('marketData');

    render(<MarketData />);

    await waitFor(() => {
      expect(marketSnapshotMock).toHaveBeenCalledTimes(1);
    });

    await waitFor(() => {
      expect(hookState.pollingCalls.some((call) => call.enabled === false)).toBe(true);
    });
  });

  it('disables Balances polling after the initial standard snapshot is loaded', async () => {
    hookState.enabledSurfaces.add('balances');

    render(<Balances />);

    await waitFor(() => {
      expect(balancesSnapshotMock).toHaveBeenCalledTimes(1);
    });

    await waitFor(() => {
      expect(hookState.pollingCalls.some((call) => call.enabled === false)).toBe(true);
    });
  });

  it('keeps MarketData page anchors stable and freshness fanout bounded for large snapshots', async () => {
    vi.useFakeTimers();
    vi.setSystemTime(new Date('2026-03-23T00:00:00.000Z'));
    hookState.enabledSurfaces.add('marketData');

    const largeRows = buildMarketRows(200);
    marketSnapshotMock.mockResolvedValue({
      rows: largeRows,
      count: largeRows.length,
      freshnessKey: 'market-freshness-large-1',
    });

    render(<MarketData />);
    await act(async () => {
      await flushTasks();
    });

    expect(marketSnapshotMock).toHaveBeenCalledTimes(1);
    expect(screen.getByText('Page 1 / 4')).toBeInTheDocument();

    await act(async () => {
      fireEvent.click(screen.getByRole('button', { name: /Next page/i }));
    });

    expect(screen.getByText('Page 2 / 4')).toBeInTheDocument();
    expect(getViewportClockDebugState('surface:marketData')).toMatchObject({
      timerCount: 1,
    });
    expect(
      getViewportClockDebugState('surface:marketData')?.activeSubscriberCount ?? 0,
    ).toBeLessThanOrEqual(52);

    act(() => {
      vi.advanceTimersByTime(10_000);
    });

    expect(screen.getByText('Page 2 / 4')).toBeInTheDocument();
    expect(
      getViewportClockDebugState('surface:marketData')?.activeSubscriberCount ?? 0,
    ).toBeLessThanOrEqual(52);

    marketSnapshotMock.mockResolvedValueOnce({
      rows: largeRows.map((row, index) => ({
        ...row,
        bid: String(500 + index),
      })),
      count: largeRows.length,
      freshnessKey: 'market-freshness-large-2',
    });

    const marketHandler = hookState.websocketCalls.find((call) => call.surface === 'marketData')?.handler;
    expect(marketHandler).toBeTypeOf('function');

    await act(async () => {
      marketHandler?.({ market_data: { count: largeRows.length } });
      await flushTasks();
    });

    expect(marketSnapshotMock).toHaveBeenCalledTimes(2);
    expect(screen.getByText('Page 2 / 4')).toBeInTheDocument();
    expect(
      getViewportClockDebugState('surface:marketData')?.activeSubscriberCount ?? 0,
    ).toBeLessThanOrEqual(52);
  });
});

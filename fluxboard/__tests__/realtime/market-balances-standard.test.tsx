import { act, render, waitFor } from '@testing-library/react';
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
  standardSubscriptionCalls: [] as Array<Record<string, any>>,
  pollingCalls: [] as Array<{
    interval: number;
    enabled: boolean | undefined;
  }>,
}));

const flushPromises = () => new Promise((resolve) => setTimeout(resolve, 0));
const createDeferred = <T,>() => {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((res) => {
    resolve = res;
  });
  return { promise, resolve };
};

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
    useStandardWebSocketSubscription: (options: Record<string, any>) => {
      useEffect(() => {
        hookState.standardSubscriptionCalls.push(options);
      }, [options]);
    },
  };
});

describe('market/balances realtime standard surface wiring', () => {
  beforeEach(() => {
    __resetViewportClockRegistryForTests();
    hookState.enabledSurfaces.clear();
    hookState.websocketCalls = [];
    hookState.standardSubscriptionCalls = [];
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
  });

  afterEach(() => {
    __resetViewportClockRegistryForTests();
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

  it('uses one shared freshness clock per standard surface instead of per-widget timers', async () => {
    hookState.enabledSurfaces.add('marketData');
    hookState.enabledSurfaces.add('balances');

    render(
      <>
        <MarketData />
        <Balances />
      </>,
    );

    await waitFor(() => {
      expect(marketSnapshotMock).toHaveBeenCalledTimes(1);
      expect(balancesSnapshotMock).toHaveBeenCalledTimes(1);
    });

    expect(getViewportClockDebugState('surface:marketData')).toMatchObject({
      activeSubscriberCount: expect.any(Number),
      timerCount: 1,
    });
    expect(getViewportClockDebugState('surface:balances')).toMatchObject({
      activeSubscriberCount: expect.any(Number),
      timerCount: 1,
    });
  });

  it('keeps MarketData on the bridge while Balances uses the standard subscription when standard flags are on', async () => {
    hookState.enabledSurfaces.add('marketData');
    hookState.enabledSurfaces.add('balances');

    render(
      <>
        <MarketData />
        <Balances />
      </>,
    );

    await waitFor(() => {
      expect(marketSnapshotMock).toHaveBeenCalledTimes(1);
      expect(balancesSnapshotMock).toHaveBeenCalledTimes(1);
    });

    expect(hookState.websocketCalls).toEqual(
      expect.arrayContaining([
        expect.objectContaining({ event: 'market_update', surface: 'marketData' }),
      ]),
    );
    expect(hookState.standardSubscriptionCalls).toEqual(
      expect.arrayContaining([
        expect.objectContaining({
          enabled: true,
          lineage: expect.objectContaining({
            contract_version: 2,
            surface: 'balances',
            stream_id: 'balances:tokenmm:strategy_01',
          }),
        }),
      ]),
    );

    await act(async () => {
      hookState.websocketCalls.forEach(({ handler, surface }) => {
        if (surface === 'marketData') {
          handler({ market_data: { count: 1 } });
        }
      });
      hookState.standardSubscriptionCalls.at(-1)?.onEvent?.({
        kind: 'invalidate',
        seq: 1,
        contract_version: 2,
        stream_id: 'balances:tokenmm:strategy_01',
        snapshot_revision: 'balances-snapshot-1',
        payload: {},
      });
      await flushPromises();
    });

    await waitFor(() => {
      expect(marketSnapshotMock).toHaveBeenCalledTimes(2);
      expect(balancesSnapshotMock).toHaveBeenCalledTimes(2);
    });
  });

  it('can enable only MarketData while Balances stays on the legacy polling path', async () => {
    hookState.enabledSurfaces.add('marketData');

    render(
      <>
        <MarketData />
        <Balances />
      </>,
    );

    await waitFor(() => {
      expect(marketSnapshotMock).toHaveBeenCalledTimes(1);
      expect(balancesSnapshotMock).toHaveBeenCalledTimes(1);
    });

    expect(
      hookState.websocketCalls.filter((call) => call.surface === 'marketData'),
    ).toHaveLength(1);
    expect(
      hookState.websocketCalls.filter((call) => call.surface === 'balances'),
    ).toHaveLength(1);
    expect(hookState.standardSubscriptionCalls.some((call) => call.enabled === true)).toBe(false);
    expect(hookState.pollingCalls.some((call) => call.enabled === false)).toBe(true);
    expect(hookState.pollingCalls.some((call) => call.enabled !== false)).toBe(true);
  });

  it('can enable only Balances while MarketData stays on the legacy polling path', async () => {
    hookState.enabledSurfaces.add('balances');

    render(
      <>
        <MarketData />
        <Balances />
      </>,
    );

    await waitFor(() => {
      expect(marketSnapshotMock).toHaveBeenCalledTimes(1);
      expect(balancesSnapshotMock).toHaveBeenCalledTimes(1);
    });

    expect(
      hookState.websocketCalls.filter((call) => call.surface === 'marketData'),
    ).toHaveLength(0);
    expect(
      hookState.websocketCalls.filter((call) => call.surface === 'balances'),
    ).toHaveLength(0);
    expect(hookState.standardSubscriptionCalls).toEqual(
      expect.arrayContaining([
        expect.objectContaining({
          enabled: true,
          lineage: expect.objectContaining({ surface: 'balances' }),
        }),
      ]),
    );
    expect(hookState.pollingCalls.some((call) => call.enabled === false)).toBe(true);
    expect(hookState.pollingCalls.some((call) => call.enabled !== false)).toBe(true);
  });

  it('keeps MarketData and Balances on the legacy polling path when standard flags are off', async () => {
    render(
      <>
        <MarketData />
        <Balances />
      </>,
    );

    await waitFor(() => {
      expect(marketSnapshotMock).toHaveBeenCalledTimes(1);
      expect(balancesSnapshotMock).toHaveBeenCalledTimes(1);
    });

    expect(hookState.websocketCalls.filter((call) => call.surface)).toEqual(
      expect.arrayContaining([
        expect.objectContaining({ surface: 'balances', event: 'market_update' }),
      ]),
    );
    expect(hookState.standardSubscriptionCalls.some((call) => call.enabled === true)).toBe(false);
    expect(hookState.pollingCalls.some((call) => call.enabled !== false)).toBe(true);
  });

  it('falls back to the legacy polling path after a flag-off remount', async () => {
    hookState.enabledSurfaces.add('marketData');
    hookState.enabledSurfaces.add('balances');

    const firstRender = render(
      <>
        <MarketData />
        <Balances />
      </>,
    );

    await waitFor(() => {
      expect(marketSnapshotMock).toHaveBeenCalledTimes(1);
      expect(balancesSnapshotMock).toHaveBeenCalledTimes(1);
    });

    expect(hookState.websocketCalls.filter((call) => call.surface)).toEqual(
      expect.arrayContaining([
        expect.objectContaining({ surface: 'marketData' }),
      ]),
    );
    expect(hookState.standardSubscriptionCalls).toEqual(
      expect.arrayContaining([
        expect.objectContaining({
          enabled: true,
          lineage: expect.objectContaining({ surface: 'balances' }),
        }),
      ]),
    );

    firstRender.unmount();
    hookState.enabledSurfaces.clear();
    hookState.websocketCalls = [];
    hookState.standardSubscriptionCalls = [];
    hookState.pollingCalls = [];
    vi.clearAllMocks();

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
      freshnessKey: 'market-freshness-remount',
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
      realtime: {
        contract_version: 2,
        surface: 'balances',
        profile: 'tokenmm',
        surface_query_key: 'balances|profile=tokenmm|strategy_ids=strategy_01',
        stream_id: 'balances:tokenmm:strategy_01',
        snapshot_revision: 'balances-snapshot-1',
        last_seq: 0,
      },
    });

    render(
      <>
        <MarketData />
        <Balances />
      </>,
    );

    await waitFor(() => {
      expect(marketSnapshotMock).toHaveBeenCalledTimes(1);
      expect(balancesSnapshotMock).toHaveBeenCalledTimes(1);
    });

    expect(hookState.websocketCalls.filter((call) => call.surface)).toEqual(
      expect.arrayContaining([
        expect.objectContaining({ surface: 'balances', event: 'market_update' }),
      ]),
    );
    expect(hookState.standardSubscriptionCalls.some((call) => call.enabled === true)).toBe(false);
    expect(hookState.pollingCalls.some((call) => call.enabled !== false)).toBe(true);
  });

  it('serializes invalidate-only refreshes and preserves one queued follow-up while a snapshot is in flight', async () => {
    hookState.enabledSurfaces.add('marketData');
    hookState.enabledSurfaces.add('balances');

    render(
      <>
        <MarketData />
        <Balances />
      </>,
    );

    await waitFor(() => {
      expect(marketSnapshotMock).toHaveBeenCalledTimes(1);
      expect(balancesSnapshotMock).toHaveBeenCalledTimes(1);
    });

    const pendingMarketSnapshot = createDeferred<Awaited<ReturnType<typeof marketSnapshotMock>>>();
    const pendingBalancesSnapshot = createDeferred<Awaited<ReturnType<typeof balancesSnapshotMock>>>();

    marketSnapshotMock
      .mockImplementationOnce(() => pendingMarketSnapshot.promise)
      .mockResolvedValue({
        rows: [
          {
            coin: 'ETH/USDT',
            exchange: 'binance',
            bid: '200',
            ask: '201',
            mid_px: '200.5',
            bid_qty: '1',
            ask_qty: '1',
            timestamp_ms: 1700000005000,
          },
        ],
        count: 1,
        freshnessKey: 'market-freshness-2',
      });

    balancesSnapshotMock
      .mockImplementationOnce(() => pendingBalancesSnapshot.promise)
      .mockResolvedValue({
        rows: [
          {
            id: 'USDC_LOGICAL',
            coin: 'USDC_LOGICAL',
            canonical: 'USDC',
            is_parent: true,
            stable: true,
            qty_display: '100',
            qty_raw: 100,
            mv_display: '$100.00',
            mv_raw: 100,
            mark_display: '1.00',
            mark_raw: 1,
            time_display: '2024-01-01T00:00:05Z',
            time_iso: '2024-01-01T00:00:05Z',
            last_ts: 1704067205000,
            raw: { qty: 100, mv_usd: 100, mark: 1 },
            children: [],
          },
        ],
        total: 1,
        totals: {
          mv_raw: 100,
          mv_display: '$100.00',
        },
      generated_at: '2024-01-01T00:00:05Z',
      view: 'parents_only',
      risk_groups: [],
      realtime: {
        contract_version: 2,
        surface: 'balances',
        profile: 'tokenmm',
        surface_query_key: 'balances|profile=tokenmm|strategy_ids=strategy_01',
        stream_id: 'balances:tokenmm:strategy_01',
        snapshot_revision: 'balances-snapshot-1',
        last_seq: 1,
      },
    });

    const marketHandler = hookState.websocketCalls.find((call) => call.surface === 'marketData')?.handler;
    const balancesSubscription = hookState.standardSubscriptionCalls.at(-1);
    const initialBalancesLineage = balancesSubscription?.lineage;

    expect(marketHandler).toBeTypeOf('function');
    expect(balancesSubscription?.onEvent).toBeTypeOf('function');

    await act(async () => {
      marketHandler?.({ strategies: { changed: ['signal-a'] } });
      marketHandler?.({ strategies: { changed: ['signal-b'] } });
      balancesSubscription?.onEvent?.({
        kind: 'invalidate',
        seq: 1,
        contract_version: 2,
        stream_id: 'balances:tokenmm:strategy_01',
        snapshot_revision: 'balances-snapshot-1',
        payload: {},
      });
      balancesSubscription?.onEvent?.({
        kind: 'invalidate',
        seq: 2,
        contract_version: 2,
        stream_id: 'balances:tokenmm:strategy_01',
        snapshot_revision: 'balances-snapshot-1',
        payload: {},
      });
      await flushPromises();
    });

    expect(marketSnapshotMock).toHaveBeenCalledTimes(2);
    expect(balancesSnapshotMock).toHaveBeenCalledTimes(2);

    await act(async () => {
      pendingMarketSnapshot.resolve({
        rows: [
          {
            coin: 'BTC/USDT',
            exchange: 'bybit',
            bid: '150',
            ask: '151',
            mid_px: '150.5',
            bid_qty: '1',
            ask_qty: '1',
            timestamp_ms: 1700000004000,
          },
        ],
        count: 1,
        freshnessKey: 'market-freshness-stale',
      });
      pendingBalancesSnapshot.resolve({
        rows: [
          {
            id: 'BTC_LOGICAL',
            coin: 'BTC_LOGICAL',
            canonical: 'BTC',
            is_parent: true,
            stable: false,
            qty_display: '1',
            qty_raw: 1,
            mv_display: '$150.00',
            mv_raw: 150,
            mark_display: '150.00',
            mark_raw: 150,
            time_display: '2024-01-01T00:00:04Z',
            time_iso: '2024-01-01T00:00:04Z',
            last_ts: 1704067204000,
            raw: { qty: 1, mv_usd: 150, mark: 150 },
            children: [],
          },
        ],
        total: 1,
        totals: {
          mv_raw: 150,
          mv_display: '$150.00',
        },
        generated_at: '2024-01-01T00:00:04Z',
        view: 'parents_only',
        risk_groups: [],
        realtime: {
          contract_version: 2,
          surface: 'balances',
          profile: 'tokenmm',
          surface_query_key: 'balances|profile=tokenmm|strategy_ids=strategy_01',
          stream_id: 'balances:tokenmm:strategy_01',
          snapshot_revision: 'balances-snapshot-1',
          last_seq: 2,
        },
      });
      await flushPromises();
      await flushPromises();
    });

    await waitFor(() => {
      expect(marketSnapshotMock).toHaveBeenCalledTimes(3);
      expect(balancesSnapshotMock).toHaveBeenCalledTimes(3);
    });

    await waitFor(() => {
      expect(useMarketDataStore.getState().rows[0]?.coin).toBe('ETH/USDT');
      expect(useBalancesStore.getState().rows[0]?.canonical).toBe('USDC');
    });

    const latestBalancesSubscription = [...hookState.standardSubscriptionCalls]
      .reverse()
      .find((call) => call.lineage?.surface === 'balances');
    expect(latestBalancesSubscription?.lineage).toBe(initialBalancesLineage);
  });
});

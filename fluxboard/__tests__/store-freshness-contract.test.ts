import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import {
  useAlertsStore,
  useBalancesStore,
  useTradesStore,
  selectAlertsFreshnessTs,
  selectBalancesFreshnessTs,
  selectTradesFreshnessTs,
} from '../stores';

describe('store freshness contract', () => {
  beforeEach(() => {
    vi.useFakeTimers();
    vi.setSystemTime(new Date('2026-02-11T00:00:00.000Z'));
    localStorage.clear();

    useTradesStore.getState().clear();
    useBalancesStore.setState({
      rows: [],
      totals: null,
      totalCount: 0,
      generatedAt: undefined,
      loading: false,
      riskGroups: [],
      riskSort: { column: 'gross_mv', direction: 'desc' },
      lastUpdate: undefined,
      lastDataTs: undefined,
      lastReceiveTs: undefined,
    } as any);
    useAlertsStore.setState({
      rows: [],
      loading: false,
      auto: true,
      dismissedIds: new Set(),
      lastUpdate: undefined,
      lastDataTs: undefined,
      lastReceiveTs: undefined,
    } as any);
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  it('trades: applyDelta updates receive timestamp even when payload does not change rows', () => {
    useTradesStore.getState().applyDelta([
      {
        op: 'upsert',
        row_id: 'trade-1',
        seq: 1,
        version: 1,
        coin: 'PLUME/USDT',
        exchange: 'bybit',
      } as any,
    ]);

    const first = useTradesStore.getState();
    const firstDataTs = first.lastDataTs;
    const firstReceiveTs = first.lastReceiveTs;

    expect(firstDataTs).toBe(Date.parse('2026-02-11T00:00:00.000Z'));
    expect(firstReceiveTs).toBe(Date.parse('2026-02-11T00:00:00.000Z'));

    vi.setSystemTime(new Date('2026-02-11T00:00:02.000Z'));

    useTradesStore.getState().applyDelta([
      {
        op: 'upsert',
        row_id: 'trade-1',
        seq: 2,
        version: 1,
        coin: 'PLUME/USDT',
        exchange: 'bybit',
      } as any,
    ]);

    const second = useTradesStore.getState();
    expect(second.lastDataTs).toBe(firstDataTs);
    expect(second.lastReceiveTs).toBe(Date.parse('2026-02-11T00:00:02.000Z'));
  });

  it('trades: setSnapshot on already-empty payload does not advance lastDataTs', () => {
    useTradesStore.getState().setSnapshot([
      {
        op: 'upsert',
        row_id: 'trade-1',
        seq: 1,
        version: 1,
        coin: 'PLUME/USDT',
        exchange: 'bybit',
      } as any,
    ]);

    vi.setSystemTime(new Date('2026-02-11T00:00:02.000Z'));
    useTradesStore.getState().setSnapshot([]);
    const afterClear = useTradesStore.getState();

    expect(afterClear.rows).toHaveLength(0);
    expect(afterClear.lastDataTs).toBe(Date.parse('2026-02-11T00:00:02.000Z'));
    expect(afterClear.lastReceiveTs).toBe(Date.parse('2026-02-11T00:00:02.000Z'));
    expect(afterClear.lastUpdate).toBe(Date.parse('2026-02-11T00:00:02.000Z'));

    vi.setSystemTime(new Date('2026-02-11T00:00:04.000Z'));
    useTradesStore.getState().setSnapshot([]);
    const afterSecondEmpty = useTradesStore.getState();

    expect(afterSecondEmpty.rows).toHaveLength(0);
    expect(afterSecondEmpty.lastDataTs).toBe(Date.parse('2026-02-11T00:00:02.000Z'));
    expect(afterSecondEmpty.lastReceiveTs).toBe(Date.parse('2026-02-11T00:00:04.000Z'));
    expect(afterSecondEmpty.lastUpdate).toBe(Date.parse('2026-02-11T00:00:04.000Z'));
  });

  it('balances: setData updates lastReceiveTs on no-op payload without advancing lastDataTs', () => {
    const payload = {
      rows: [
        {
          id: 'plume-parent',
          coin: 'PLUME',
          canonical: 'PLUME',
          qty_raw: 100,
          mv_raw: 50,
          mark_raw: 0.5,
          last_ts: 1000,
          children: [],
        },
      ],
      total: 1,
      totals: null,
      generated_at: '2026-02-11T00:00:00.000Z',
      risk_groups: [],
    } as any;

    useBalancesStore.getState().setData(payload);
    const first = useBalancesStore.getState();
    const firstDataTs = first.lastDataTs;

    expect(firstDataTs).toBe(Date.parse('2026-02-11T00:00:00.000Z'));
    expect(first.lastReceiveTs).toBe(Date.parse('2026-02-11T00:00:00.000Z'));

    vi.setSystemTime(new Date('2026-02-11T00:00:03.000Z'));
    useBalancesStore.getState().setData(payload);

    const second = useBalancesStore.getState();
    expect(second.lastDataTs).toBe(firstDataTs);
    expect(second.lastReceiveTs).toBe(Date.parse('2026-02-11T00:00:03.000Z'));
    expect(second.lastUpdate).toBe(Date.parse('2026-02-11T00:00:03.000Z'));
  });

  it('balances: setData advances lastDataTs when totals/total/risk_groups metadata changes', () => {
    useBalancesStore.getState().setData({
      rows: [
        {
          id: 'plume-parent',
          coin: 'PLUME',
          canonical: 'PLUME',
          qty_raw: 100,
          mv_raw: 50,
          mark_raw: 0.5,
          last_ts: 1000,
          children: [],
        },
      ],
      total: 1,
      totals: { mv_raw: 50, mv_display: '$50.00' },
      generated_at: '2026-02-11T00:00:00.000Z',
      risk_groups: [],
    } as any);

    expect(useBalancesStore.getState().lastDataTs).toBe(Date.parse('2026-02-11T00:00:00.000Z'));

    vi.setSystemTime(new Date('2026-02-11T00:00:02.000Z'));
    useBalancesStore.getState().setData({
      rows: [
        {
          id: 'plume-parent',
          coin: 'PLUME',
          canonical: 'PLUME',
          qty_raw: 100,
          mv_raw: 50,
          mark_raw: 0.5,
          last_ts: 1000,
          children: [],
        },
      ],
      total: 1,
      totals: { mv_raw: 55, mv_display: '$55.00' },
      generated_at: '2026-02-11T00:00:00.000Z',
      risk_groups: [],
    } as any);
    expect(useBalancesStore.getState().lastDataTs).toBe(Date.parse('2026-02-11T00:00:02.000Z'));

    vi.setSystemTime(new Date('2026-02-11T00:00:04.000Z'));
    useBalancesStore.getState().setData({
      rows: [
        {
          id: 'plume-parent',
          coin: 'PLUME',
          canonical: 'PLUME',
          qty_raw: 100,
          mv_raw: 50,
          mark_raw: 0.5,
          last_ts: 1000,
          children: [],
        },
      ],
      total: 2,
      totals: { mv_raw: 55, mv_display: '$55.00' },
      generated_at: '2026-02-11T00:00:00.000Z',
      risk_groups: [],
    } as any);
    expect(useBalancesStore.getState().lastDataTs).toBe(Date.parse('2026-02-11T00:00:04.000Z'));

    vi.setSystemTime(new Date('2026-02-11T00:00:06.000Z'));
    useBalancesStore.getState().setData({
      rows: [
        {
          id: 'plume-parent',
          coin: 'PLUME',
          canonical: 'PLUME',
          qty_raw: 100,
          mv_raw: 50,
          mark_raw: 0.5,
          last_ts: 1000,
          children: [],
        },
      ],
      total: 2,
      totals: { mv_raw: 55, mv_display: '$55.00' },
      generated_at: '2026-02-11T00:00:00.000Z',
      risk_groups: [{ risk_key: 'PLUME', label: 'PLUME', net_mv: 55 }],
    } as any);
    expect(useBalancesStore.getState().lastDataTs).toBe(Date.parse('2026-02-11T00:00:06.000Z'));
  });

  it('alerts: dismiss/clear do not advance lastDataTs', () => {
    useAlertsStore.getState().setRows([
      {
        id: 'alert-1',
        level: 'WARNING',
        severity: 'WARNING',
        title: 'Alert',
        message: 'Test',
        timestamp: 1,
        ts: 1,
      } as any,
    ]);

    const afterData = useAlertsStore.getState();
    const dataTs = afterData.lastDataTs;

    expect(dataTs).toBe(Date.parse('2026-02-11T00:00:00.000Z'));
    expect(afterData.lastReceiveTs).toBe(Date.parse('2026-02-11T00:00:00.000Z'));

    vi.setSystemTime(new Date('2026-02-11T00:00:05.000Z'));
    useAlertsStore.getState().dismissAlert('alert-1');
    const afterDismiss = useAlertsStore.getState();

    expect(afterDismiss.lastDataTs).toBe(dataTs);
    expect(afterDismiss.lastReceiveTs).toBe(Date.parse('2026-02-11T00:00:00.000Z'));

    vi.setSystemTime(new Date('2026-02-11T00:00:07.000Z'));
    useAlertsStore.getState().clearAlerts();
    const afterClear = useAlertsStore.getState();

    expect(afterClear.lastDataTs).toBe(dataTs);
    expect(afterClear.lastReceiveTs).toBe(Date.parse('2026-02-11T00:00:00.000Z'));
    expect(afterClear.lastUpdate).toBe(Date.parse('2026-02-11T00:00:07.000Z'));
  });

  it('freshness selectors prefer lastDataTs and handle lastUpdate fallback safely', () => {
    const withData = { lastUpdate: 10, lastDataTs: 20 } as any;
    const legacyOnly = { lastUpdate: 30, lastDataTs: undefined } as any;
    const tradesLegacyWithRows = { lastUpdate: 30, lastDataTs: undefined, rows: [{ row_id: 't-1' }] } as any;
    const tradesEmptyNoDataTs = { lastUpdate: 40, lastDataTs: undefined, rows: [] } as any;

    expect(selectTradesFreshnessTs(withData)).toBe(20);
    expect(selectTradesFreshnessTs(tradesLegacyWithRows)).toBe(30);
    expect(selectTradesFreshnessTs(tradesEmptyNoDataTs)).toBeUndefined();

    expect(selectBalancesFreshnessTs(withData)).toBe(20);
    expect(selectBalancesFreshnessTs(legacyOnly)).toBe(30);

    expect(selectAlertsFreshnessTs(withData)).toBe(20);
    expect(selectAlertsFreshnessTs(legacyOnly)).toBe(30);
  });
});

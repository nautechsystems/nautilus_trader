import { beforeEach, describe, expect, it } from 'vitest';
import {
  ORDER_VIEW_BUFFER_CAP,
  ORDER_VIEW_CANDLES_CAP,
  ORDER_VIEW_L2_DEPTH_CAP,
  ORDER_VIEW_MARKET_TRADES_CAP,
  useOrderViewStore,
} from './orderViewStore';

const makeSnapshot = (overrides: Partial<any> = {}) => ({
  room_id: 'order_view:strat-1:maker:book:0:depth:20',
  server_ts_ms: 1_700_000_000_000,
  snapshot_id: 'snap-1',
  seq: 0,
  selection: { strategy_id: 'strat-1', leg: 'maker' },
  context: {
    maker: { exchange: 'bybit_linear', symbol: 'BTC_USDT' },
    hedge: { exchange: 'binance_spot', symbol: 'BTC_USDT' },
  },
  state_rev: 'rev-1',
  maker_state_ts_ms: 1_700_000_000_000,
  bbo: {
    maker: { bid: 30000, ask: 30010, mid: 30005, ts_ms: 1_700_000_000_000 },
  },
  open_orders: {
    rows: [
      {
        order_row_id: 'maker:bid:1:cl-1',
        leg: 'maker',
        side: 'bid',
        level: 1,
        px: '30000',
        rem_qty: '1',
        client_order_id: 'cl-1',
      },
    ],
  },
  events: { rows: [] },
  status: {
    md_ok: true,
    maker_state_ok: true,
    events_ok: true,
    last_md_ts_ms: 1_700_000_000_000,
    last_state_ts_ms: 1_700_000_000_000,
    notes: [],
  },
  ...overrides,
});

const makeDelta = (seq: number, overrides: Partial<any> = {}) => ({
  room_id: 'order_view:strat-1:maker:book:0:depth:20',
  seq,
  snapshot_id: 'snap-1',
  state_rev: 'rev-1',
  server_ts_ms: 1_700_000_000_000 + seq,
  selection: { strategy_id: 'strat-1', leg: 'maker' },
  context: {
    maker: { exchange: 'bybit_linear', symbol: 'BTC_USDT' },
    hedge: { exchange: 'binance_spot', symbol: 'BTC_USDT' },
  },
  bbo: {
    maker: { bid: 30000 + seq, ask: 30010 + seq, mid: 30005 + seq, ts_ms: 1_700_000_000_000 + seq },
  },
  events: { rows: [] },
  status: {
    md_ok: true,
    maker_state_ok: true,
    events_ok: true,
    last_md_ts_ms: 1_700_000_000_000 + seq,
    last_state_ts_ms: 1_700_000_000_000,
    notes: [],
  },
  ...overrides,
});

describe('orderViewStore', () => {
  beforeEach(() => {
    useOrderViewStore.getState().clear();
  });

  it('applies snapshot then full-refresh order lifecycle deltas', () => {
    const store = useOrderViewStore.getState();
    store.applySnapshot(makeSnapshot());

    let state = useOrderViewStore.getState();
    expect(state.openOrderIds).toEqual(['maker:bid:1:cl-1']);

    store.applyDelta(
      makeDelta(1, {
        state_rev: 'rev-2',
        open_orders: {
          full_refresh: 1,
          rows: [
            {
              order_row_id: 'maker:bid:1:cl-1',
              leg: 'maker',
              side: 'bid',
              level: 1,
              px: '30000',
              rem_qty: '0.5',
              client_order_id: 'cl-1',
            },
            {
              order_row_id: 'maker:ask:1:cl-2',
              leg: 'maker',
              side: 'ask',
              level: 1,
              px: '30010',
              rem_qty: '0.2',
              client_order_id: 'cl-2',
            },
          ],
        },
      })
    );

    state = useOrderViewStore.getState();
    expect(state.stateRev).toBe('rev-2');
    expect(state.openOrderIds).toEqual(['maker:bid:1:cl-1', 'maker:ask:1:cl-2']);
    expect(state.openOrdersById['maker:bid:1:cl-1']?.rem_qty).toBe('0.5');

    store.applyDelta(
      makeDelta(2, {
        state_rev: 'rev-3',
        open_orders: { full_refresh: 1, rows: [] },
      })
    );

    state = useOrderViewStore.getState();
    expect(state.stateRev).toBe('rev-3');
    expect(state.openOrderIds).toEqual([]);
    expect(Object.keys(state.openOrdersById)).toHaveLength(0);
  });

  it('drops non-actionable placeholder ladder rows from open orders', () => {
    const store = useOrderViewStore.getState();
    store.applySnapshot(
      makeSnapshot({
        open_orders: {
          rows: [
            {
              order_row_id: 'maker:bid:1:na',
              leg: 'maker',
              side: 'bid',
              level: 1,
              px: null,
              rem_qty: null,
              client_order_id: null,
              order_id: null,
            },
            {
              order_row_id: 'maker:ask:1:cl-2',
              leg: 'maker',
              side: 'ask',
              level: 1,
              px: '30010',
              rem_qty: '0.2',
              client_order_id: 'cl-2',
            },
          ],
        },
      })
    );

    const state = useOrderViewStore.getState();
    expect(state.openOrderIds).toEqual(['maker:ask:1:cl-2']);
    expect(state.openOrdersById['maker:bid:1:na']).toBeUndefined();
    expect(state.openOrdersById['maker:ask:1:cl-2']).toBeDefined();
  });

  it('flags seq gaps for resync and rejects stale resync epochs', () => {
    const store = useOrderViewStore.getState();

    store.applySnapshot(makeSnapshot(), 2);
    const first = store.applyDelta(makeDelta(1), 2);
    expect(first.applied).toBe(true);

    const gap = store.applyDelta(makeDelta(4), 2);
    expect(gap.seqGap).toBe(true);
    expect(gap.needsResync).toBe(true);

    const stale = store.applyDelta(makeDelta(2), 1);
    expect(stale.staleRejected).toBe(true);
    expect(useOrderViewStore.getState().lastSeq).toBe(1);
  });

  it('sets and clears normalized cross-panel focus state', () => {
    const store = useOrderViewStore.getState();

    store.setFocus({
      orderKey: 'oid-focus',
      eventKey: 'evt-focus',
      side: 'buy',
      price: '30000.5',
    });
    let state = useOrderViewStore.getState();
    expect(state.focus).toEqual({
      orderKey: 'oid-focus',
      eventKey: 'evt-focus',
      side: 'bid',
      price: 30000.5,
    });

    store.clearFocus();
    state = useOrderViewStore.getState();
    expect(state.focus).toEqual({
      orderKey: null,
      eventKey: null,
      side: null,
      price: null,
    });
  });

  it('clears focus when selection changes to a different strategy/leg', () => {
    const store = useOrderViewStore.getState();
    store.setSelection({ strategyId: 'strat-1', leg: 'maker' });
    store.setFocus({
      orderKey: 'oid-focus',
      eventKey: 'evt-focus',
      side: 'ask',
      price: '30010',
    });

    store.setSelection({ strategyId: 'strat-2' });
    const state = useOrderViewStore.getState();
    expect(state.selection.strategyId).toBe('strat-2');
    expect(state.focus).toEqual({
      orderKey: null,
      eventKey: null,
      side: null,
      price: null,
    });
  });

  it('accepts seq reset when snapshot epoch changes and snapshot_id advances', () => {
    const store = useOrderViewStore.getState();

    store.applySnapshot(makeSnapshot({ snapshot_id: 'snap-1', seq: 0 }));
    const first = store.applyDelta(makeDelta(1, { snapshot_id: 'snap-1' }));
    expect(first.applied).toBe(true);

    const epochReset = store.applyDelta(
      makeDelta(1, {
        snapshot_id: 'snap-2',
        state_rev: 'rev-2',
        open_orders: {
          full_refresh: 1,
          rows: [
            {
              order_row_id: 'maker:bid:1:cl-1',
              leg: 'maker',
              side: 'bid',
              level: 1,
              px: '30000',
              rem_qty: '0.8',
              client_order_id: 'cl-1',
            },
          ],
        },
      })
    );
    expect(epochReset.applied).toBe(true);
    const state = useOrderViewStore.getState();
    expect(state.lastSeq).toBe(1);
    expect(state.stateRev).toBe('rev-2');
  });

  it('prefers snapshot last_seq over seq when provided', () => {
    const store = useOrderViewStore.getState();
    store.applySnapshot(makeSnapshot({ snapshot_id: 'snap-1', seq: 1, last_seq: 9 }));

    const state = useOrderViewStore.getState();
    expect(state.lastSeq).toBe(9);
  });

  it('requires resync when a new snapshot epoch starts with seq gap', () => {
    const store = useOrderViewStore.getState();
    store.applySnapshot(makeSnapshot({ snapshot_id: 'snap-1', seq: 0 }));
    store.applyDelta(makeDelta(1, { snapshot_id: 'snap-1' }));

    const gap = store.applyDelta(
      makeDelta(3, {
        snapshot_id: 'snap-2',
        state_rev: 'rev-2',
      })
    );
    expect(gap.applied).toBe(false);
    expect(gap.seqGap).toBe(true);
    expect(gap.needsResync).toBe(true);
    expect(useOrderViewStore.getState().needsResync).toBe(true);
  });

  it('requires resync when seq regresses within the same snapshot epoch', () => {
    const store = useOrderViewStore.getState();
    store.applySnapshot(makeSnapshot({ snapshot_id: 'snap-1', seq: 0 }));

    // Existing rooms can legitimately start at seq>1 after snapshot fetch.
    const first = store.applyDelta(makeDelta(9, { snapshot_id: 'snap-1' }));
    expect(first.applied).toBe(true);
    expect(useOrderViewStore.getState().lastSeq).toBe(9);

    const regressed = store.applyDelta(makeDelta(1, { snapshot_id: 'snap-1' }));
    expect(regressed.applied).toBe(false);
    expect(regressed.needsResync).toBe(true);
    expect(useOrderViewStore.getState().needsResync).toBe(true);
  });

  it('ignores mismatched-room regressed seq deltas without forcing resync', () => {
    const store = useOrderViewStore.getState();
    store.applySnapshot(makeSnapshot({ snapshot_id: 'snap-1', seq: 0 }));
    store.applyDelta(makeDelta(9, { snapshot_id: 'snap-1' }));

    const staleRoomRegression = store.applyDelta(
      makeDelta(1, {
        snapshot_id: 'snap-1',
        room_id: 'order_view:strat-1:maker:book:0:depth:20:stale',
      })
    );

    expect(staleRoomRegression.applied).toBe(false);
    expect(staleRoomRegression.needsResync).toBe(false);
    const state = useOrderViewStore.getState();
    expect(state.needsResync).toBe(false);
    expect(state.lastSeq).toBe(9);
    expect(state.roomId).toBe('order_view:strat-1:maker:book:0:depth:20');
  });

  it('rejects stale-room deltas after selection switch while room is unset', () => {
    const store = useOrderViewStore.getState();
    store.applySnapshot(makeSnapshot());

    store.setSelection({ strategyId: 'strat-2', leg: 'maker' });
    const staleRoomDelta = store.applyDelta(
      makeDelta(1, {
        room_id: 'order_view:strat-1:maker:book:0:depth:20',
        selection: { strategy_id: 'strat-1', leg: 'maker' },
      })
    );

    expect(staleRoomDelta.applied).toBe(false);
    expect(staleRoomDelta.accepted).toBe(false);

    const state = useOrderViewStore.getState();
    expect(state.selection.strategyId).toBe('strat-2');
    expect(state.roomId).toBeNull();
    expect(state.lastSeq).toBe(0);
    expect(state.priceSeries).toEqual([]);
    expect(state.events).toEqual([]);
  });

  it('keeps events and fills bounded newest-first', () => {
    const store = useOrderViewStore.getState();
    const totalEvents = ORDER_VIEW_BUFFER_CAP + 25;
    const rows = Array.from({ length: totalEvents }, (_, idx) => {
      const index = idx + 1;
      return [
        {
          event_key: `quote-${index}`,
          ts_ms: 1_700_000_000_000 + index,
          type: 'quote.placed',
        },
        {
          event_key: `fill-${index}`,
          ts_ms: 1_700_000_000_000 + index,
          type: 'fill',
        },
      ];
    }).flat();
    store.applySnapshot(makeSnapshot({ events: { rows } }));

    const state = useOrderViewStore.getState();
    expect(state.events).toHaveLength(ORDER_VIEW_BUFFER_CAP);
    expect(state.fills.length).toBeLessThanOrEqual(ORDER_VIEW_BUFFER_CAP);
    expect(state.events[0].event_key).toBe('quote-1');
    expect(state.events[1].event_key).toBe('fill-1');
    expect(state.fills[0].event_key).toBe('fill-1');
    expect(state.fills.every((row) => row.type === 'fill')).toBe(true);
  });

  it('captures v0.2 l2, candles, and market trades from snapshot', () => {
    const store = useOrderViewStore.getState();
    store.applySnapshot(
      makeSnapshot({
        l2: {
          bids: Array.from({ length: 75 }, (_, idx) => ({ px: String(30000 - idx), qty: String(1 + idx) })),
          asks: Array.from({ length: 75 }, (_, idx) => ({ px: String(30001 + idx), qty: String(2 + idx) })),
          top_n: 99,
          spread_abs: 1,
          spread_bps: 0.33,
        },
        candles: {
          source: 'trades',
          rows: [
            { ts_ms: 1_700_000_000_000, open: 30000, high: 30005, low: 29995, close: 30003, volume: 1.2 },
            { ts_ms: 1_700_000_001_000, open: 30003, high: 30008, low: 30000, close: 30007, volume: 0.8 },
          ],
          candle_current: { ts_ms: 1_700_000_001_000, open: 30003, high: 30009, low: 30000, close: 30008, volume: 0.9 },
        },
        market_trades: {
          rows: [
            { trade_id: 'mkt-1', ts_ms: 1_700_000_001_100, side: 'buy', price: '30008', qty: '0.10' },
            { trade_id: 'mkt-2', ts_ms: 1_700_000_001_050, side: 'sell', price: '30007', qty: '0.20' },
          ],
        },
      })
    );

    const state = useOrderViewStore.getState();
    expect(state.l2?.top_n).toBe(ORDER_VIEW_L2_DEPTH_CAP);
    expect(state.l2?.bids).toHaveLength(ORDER_VIEW_L2_DEPTH_CAP);
    expect(state.l2?.asks).toHaveLength(ORDER_VIEW_L2_DEPTH_CAP);
    expect(state.candleSource).toBe('trades');
    expect(state.candles).toHaveLength(2);
    expect(state.candles[1]?.close).toBe(30008);
    expect(state.marketTrades).toHaveLength(2);
    expect(state.marketTrades[0]?.trade_id).toBe('mkt-1');
  });

  it('defaults candle volume to 0 when missing from payload', () => {
    const store = useOrderViewStore.getState();
    store.applySnapshot(
      makeSnapshot({
        candles: {
          source: 'trades',
          rows: [{ ts_ms: 1_700_000_000_000, open: 30000, high: 30005, low: 29995, close: 30003 }] as any,
        },
      })
    );

    const state = useOrderViewStore.getState();
    expect(state.candles).toHaveLength(1);
    expect(state.candles[0]?.volume).toBe(0);
  });

  it('appends and caps v0.2 market trades and candles on delta', () => {
    const store = useOrderViewStore.getState();
    store.applySnapshot(
      makeSnapshot({
        candles: {
          source: 'bbo_fallback',
          rows: [{ ts_ms: 1_700_000_000_000, open: 30000, high: 30000, low: 30000, close: 30000, volume: 0 }],
        },
        market_trades: {
          rows: Array.from({ length: ORDER_VIEW_MARKET_TRADES_CAP }, (_, idx) => ({
            trade_id: `seed-${idx}`,
            ts_ms: 1_700_000_000_000 - idx,
            side: 'buy',
            price: '30000',
            qty: '0.01',
          })),
        },
      })
    );

    const longRows = Array.from({ length: ORDER_VIEW_CANDLES_CAP + 20 }, (_, idx) => ({
      ts_ms: 1_700_000_001_000 + idx * 1_000,
      open: 30001 + idx,
      high: 30002 + idx,
      low: 30000 + idx,
      close: 30001 + idx,
      volume: 0.1,
    }));
    store.applyDelta(
      makeDelta(1, {
        candles: {
          source: 'trades',
          rows: longRows,
        },
        market_trades: {
          rows: [
            { trade_id: 'new-1', ts_ms: 1_700_000_009_999, side: 'sell', price: '30010', qty: '0.2' },
            { ts_ms: 1_700_000_009_998, side: 'buy', price: '30009', qty: '0.1' },
          ],
        },
      })
    );

    const state = useOrderViewStore.getState();
    expect(state.candleSource).toBe('trades');
    expect(state.candles).toHaveLength(ORDER_VIEW_CANDLES_CAP);
    expect(state.marketTrades).toHaveLength(ORDER_VIEW_MARKET_TRADES_CAP);
    expect(state.marketTrades[0]?.trade_id).toBe('new-1');
  });

  it('dedupes market trades by fallback fingerprint when trade_id is missing', () => {
    const store = useOrderViewStore.getState();
    store.applySnapshot(makeSnapshot({ market_trades: { rows: [] } }));

    store.applyDelta(
      makeDelta(1, {
        market_trades: {
          rows: [
            { ts_ms: 1_700_000_010_000, side: 'buy', price: '30011', qty: '0.3' },
            { ts_ms: 1_700_000_010_000, side: 'buy', price: '30011', qty: '0.3' },
          ],
        },
      })
    );

    const state = useOrderViewStore.getState();
    expect(state.marketTrades).toHaveLength(1);
    expect(state.marketTrades[0]?.trade_id).toBeNull();
  });

  it('preserves events/fills/trades/candles across snapshot resync', () => {
    const store = useOrderViewStore.getState();
    store.applySnapshot(
      makeSnapshot({
        snapshot_id: 'snap-1',
        seq: 0,
        candles: {
          source: 'trades',
          rows: [{ ts_ms: 1_700_000_000_000, open: 30000, high: 30001, low: 29999, close: 30000, volume: 1 }],
        },
        market_trades: {
          rows: [{ trade_id: 'seed-1', ts_ms: 1_700_000_000_100, side: 'buy', price: '30000', qty: '0.10' }],
        },
        events: {
          rows: [{ event_key: 'seed-evt-1', ts_ms: 1_700_000_000_100, type: 'quote.placed', order_id: 'oid-seed' }],
        },
      })
    );

    store.applyDelta(
      makeDelta(1, {
        snapshot_id: 'snap-1',
        market_trades: {
          rows: [{ trade_id: 'delta-1', ts_ms: 1_700_000_000_200, side: 'sell', price: '30001', qty: '0.20' }],
        },
        candles: {
          source: 'trades',
          candle_current: {
            ts_ms: 1_700_000_001_000,
            open: 30001,
            high: 30002,
            low: 30000,
            close: 30001,
            volume: 0.5,
          },
        },
        events: {
          rows: [
            { event_key: 'delta-fill-1', ts_ms: 1_700_000_000_200, type: 'fill', order_id: 'oid-seed', qty: '0.10', px: '30001' },
          ],
        },
      })
    );

    store.applySnapshot(
      makeSnapshot({
        snapshot_id: 'snap-2',
        seq: 2,
        state_rev: 'rev-2',
        events: { rows: [] },
        market_trades: { rows: [] },
        candles: { source: 'trades', rows: [] },
      })
    );

    const state = useOrderViewStore.getState();
    expect(state.events.map((row) => row.event_key)).toContain('delta-fill-1');
    expect(state.fills.map((row) => row.event_key)).toContain('delta-fill-1');
    expect(state.marketTrades.some((row) => row.trade_id === 'delta-1')).toBe(true);
    expect(state.candles.some((row) => row.ts_ms === 1_700_000_001_000)).toBe(true);
  });

  it('upserts candles by ts_ms and overwrites existing bucket values', () => {
    const store = useOrderViewStore.getState();
    store.applySnapshot(
      makeSnapshot({
        snapshot_id: 'snap-1',
        seq: 0,
        candles: {
          source: 'trades',
          rows: [
            { ts_ms: 1_700_000_000_000, open: 30000, high: 30002, low: 29999, close: 30001, volume: 1 },
            { ts_ms: 1_700_000_001_000, open: 30001, high: 30003, low: 30000, close: 30002, volume: 2 },
          ],
        },
      })
    );

    store.applySnapshot(
      makeSnapshot({
        snapshot_id: 'snap-2',
        seq: 1,
        state_rev: 'rev-2',
        candles: {
          source: 'trades',
          rows: [{ ts_ms: 1_700_000_001_000, open: 30100, high: 30110, low: 30090, close: 30105, volume: 9 }],
        },
      })
    );

    const state = useOrderViewStore.getState();
    expect(state.candles).toHaveLength(2);
    const overwritten = state.candles.find((row) => row.ts_ms === 1_700_000_001_000);
    expect(overwritten?.open).toBe(30100);
    expect(overwritten?.close).toBe(30105);
  });

  it('uses snapshot server_time_ms as lifetime segment start when created_ts_ms is missing', () => {
    const store = useOrderViewStore.getState();
    store.applySnapshot(
      makeSnapshot({
        server_time_ms: 1_700_000_010_000,
        open_orders: {
          rows: [
            {
              order_row_id: 'maker:bid:1:cl-fallback',
              leg: 'maker',
              side: 'bid',
              level: 1,
              px: '30000',
              rem_qty: '0.8',
              client_order_id: 'cl-fallback',
              order_id: 'oid-fallback',
              created_ts_ms: null,
            },
          ],
        },
        events: { rows: [] },
      })
    );

    const state = useOrderViewStore.getState();
    expect(state.lifetimeSegments).toHaveLength(1);
    expect(state.lifetimeSegments[0]?.order_key).toBe('oid-fallback');
    expect(state.lifetimeSegments[0]?.start_ts_ms).toBe(1_700_000_010_000);
    expect(state.lifetimeSegments[0]?.lifetime_start_unknown).toBe(true);
  });

  it('builds lifetime segments from active orders using created_ts_ms and unknown-start flag', () => {
    const store = useOrderViewStore.getState();
    store.applySnapshot(
      makeSnapshot({
        open_orders: {
          rows: [
            {
              order_row_id: 'maker:bid:1:cl-live',
              leg: 'maker',
              side: 'bid',
              level: 1,
              px: '30000',
              rem_qty: '1.0',
              client_order_id: 'cl-live',
              order_id: 'oid-live',
              created_ts_ms: 1_700_000_000_123,
              lifetime_start_unknown: true,
            },
          ],
        },
        events: { rows: [] },
      })
    );

    const state = useOrderViewStore.getState();
    expect(state.lifetimeSegments).toHaveLength(1);
    expect(state.lifetimeSegments[0]?.order_key).toBe('oid-live');
    expect(state.lifetimeSegments[0]?.start_ts_ms).toBe(1_700_000_000_123);
    expect(state.lifetimeSegments[0]?.end_ts_ms).toBeNull();
    expect(state.lifetimeSegments[0]?.lifetime_start_unknown).toBe(true);
  });

  it('closes lifetime segments on fill events using order identity', () => {
    const store = useOrderViewStore.getState();
    store.applySnapshot(
      makeSnapshot({
        open_orders: {
          rows: [
            {
              order_row_id: 'maker:ask:1:cl-close',
              leg: 'maker',
              side: 'ask',
              level: 1,
              px: '30010',
              rem_qty: '0.4',
              client_order_id: 'cl-close',
              order_id: 'oid-close',
              created_ts_ms: 1_700_000_001_000,
              lifetime_start_unknown: false,
            },
          ],
        },
        events: {
          rows: [
            {
              event_key: 'evt-place-close',
              ts_ms: 1_700_000_001_000,
              type: 'quote.placed',
              order_id: 'oid-close',
              client_order_id: 'cl-close',
              side: 'ask',
              px: '30010',
            },
          ],
        },
      })
    );

    store.applyDelta(
      makeDelta(1, {
        open_orders: {
          full_refresh: 1,
          rows: [],
        },
        events: {
          rows: [
            {
              event_key: 'evt-fill-close',
              ts_ms: 1_700_000_001_222,
              type: 'fill',
              order_id: 'oid-close',
              client_order_id: 'cl-close',
              side: 'sell',
              px: '30010',
              qty: '0.4',
            },
          ],
        },
      })
    );

    const state = useOrderViewStore.getState();
    expect(state.lifetimeSegments).toHaveLength(1);
    expect(state.lifetimeSegments[0]?.order_key).toBe('oid-close');
    expect(state.lifetimeSegments[0]?.close_reason).toBe('fill');
    expect(state.lifetimeSegments[0]?.end_ts_ms).toBe(1_700_000_001_222);
  });

  it('trims price series by selected time range', () => {
    const store = useOrderViewStore.getState();
    store.setSelection({ timeRange: '5m' });
    store.applySnapshot(makeSnapshot({ server_ts_ms: 1_700_000_000_000 }));
    store.applyDelta(makeDelta(1, { server_ts_ms: 1_700_000_100_000 }));
    store.applyDelta(makeDelta(2, { server_ts_ms: 1_700_000_400_000 }));

    const state = useOrderViewStore.getState();
    expect(state.priceSeries.length).toBeGreaterThan(0);
    const latestTs = state.priceSeries[state.priceSeries.length - 1]?.ts_ms ?? 0;
    const earliestTs = state.priceSeries[0]?.ts_ms ?? latestTs;
    expect(latestTs - earliestTs).toBeLessThanOrEqual(300_000);
  });

  it('preserves accumulated price history across same-selection snapshots', () => {
    const store = useOrderViewStore.getState();
    store.applySnapshot(makeSnapshot({ server_ts_ms: 1_700_000_000_000 }));
    store.applyDelta(
      makeDelta(1, {
        server_ts_ms: 1_700_000_001_000,
        bbo: {
          maker: { bid: 30001, ask: 30011, mid: 30006, ts_ms: 1_700_000_001_000 },
        },
      })
    );

    const beforeResnapshot = useOrderViewStore.getState().priceSeries.slice();
    expect(beforeResnapshot).toHaveLength(2);

    store.applySnapshot(
      makeSnapshot({
        state_rev: 'rev-2',
        server_ts_ms: 1_700_000_002_000,
        bbo: {
          maker: { bid: 30002, ask: 30012, mid: 30007, ts_ms: 1_700_000_002_000 },
        },
      })
    );

    const afterResnapshot = useOrderViewStore.getState().priceSeries;
    expect(afterResnapshot).toHaveLength(3);
    expect(afterResnapshot[0]?.ts_ms).toBe(beforeResnapshot[0]?.ts_ms);
    expect(afterResnapshot[1]?.ts_ms).toBe(beforeResnapshot[1]?.ts_ms);
    expect(afterResnapshot[2]?.ts_ms).toBe(1_700_000_002_000);
  });

  it('coalesces sub-second points into per-second buckets', () => {
    const store = useOrderViewStore.getState();
    store.applySnapshot(makeSnapshot({ server_ts_ms: 1_700_000_000_000 }));
    store.applyDelta(
      makeDelta(1, {
        server_ts_ms: 1_700_000_000_100,
        bbo: {
          maker: { bid: 30001, ask: 30011, mid: 30006, ts_ms: 1_700_000_000_100 },
        },
      })
    );
    store.applyDelta(
      makeDelta(2, {
        server_ts_ms: 1_700_000_000_900,
        bbo: {
          maker: { bid: 30002, ask: 30012, mid: 30007, ts_ms: 1_700_000_000_900 },
        },
      })
    );

    let state = useOrderViewStore.getState();
    expect(state.priceSeries).toHaveLength(1);
    expect(state.priceSeries[0]?.ts_ms).toBe(1_700_000_000_000);
    expect(state.priceSeries[0]?.maker_mid).toBe(30007);

    store.applyDelta(
      makeDelta(3, {
        server_ts_ms: 1_700_000_001_001,
        bbo: {
          maker: { bid: 31001, ask: 31011, mid: 31006, ts_ms: 1_700_000_001_001 },
        },
      })
    );

    state = useOrderViewStore.getState();
    expect(state.priceSeries).toHaveLength(2);
    expect(state.priceSeries[1]?.ts_ms).toBe(1_700_000_001_000);
  });

  it('ignores out-of-order price points to keep series monotonic', () => {
    const store = useOrderViewStore.getState();
    store.applySnapshot(makeSnapshot({ server_ts_ms: 1_700_000_000_000 }));
    store.applyDelta(
      makeDelta(1, {
        server_ts_ms: 1_700_000_001_000,
        bbo: {
          maker: { bid: 30100, ask: 30110, mid: 30105, ts_ms: 1_700_000_001_000 },
        },
      })
    );
    const before = useOrderViewStore.getState().priceSeries.map((point) => point.ts_ms);

    store.applyDelta(
      makeDelta(2, {
        server_ts_ms: 1_700_000_000_500,
        bbo: {
          maker: { bid: 30050, ask: 30060, mid: 30055, ts_ms: 1_700_000_000_500 },
        },
      })
    );

    const after = useOrderViewStore.getState().priceSeries.map((point) => point.ts_ms);
    expect(after).toEqual(before);
  });
});

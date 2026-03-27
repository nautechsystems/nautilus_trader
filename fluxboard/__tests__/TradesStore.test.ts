import { beforeEach, describe, expect, it } from 'vitest';
import {
  registerGlobalResyncConsumer,
  unregisterGlobalResyncConsumer,
  useResyncStore,
  useTradesStore,
} from '../stores';

describe('useTradesStore', () => {
  beforeEach(() => {
    useTradesStore.getState().clear();
    useResyncStore.getState().resetResyncState();
  });

  it('defaults missing version to 1 when applying delta', () => {
    useTradesStore.getState().applyDelta([
      {
        op: 'upsert',
        row_id: 'row-1',
        seq: 42,
        coin: 'PLUME/USDT',
        exchange: 'bybit',
      } as any,
    ]);

    const state = useTradesStore.getState();
    const row = state.byId.get('row-1');

    expect(row?.version).toBe(1);
    expect(state.lastSeq).toBe(42);
  });

  it('coerces numeric string seq/version values', () => {
    useTradesStore.getState().applyDelta([
      {
        op: 'upsert',
        row_id: 'row-2',
        seq: '100',
        version: '3',
      } as any,
    ]);

    const row = useTradesStore.getState().byId.get('row-2');
    expect(row?.seq).toBe(100);
    expect(row?.version).toBe(3);
  });

  it('maintains newest-first ordering with incremental inserts', () => {
    const store = useTradesStore.getState();
    store.applyDelta([
      { op: 'upsert', row_id: 't1', seq: 100, version: 1 } as any,
    ]);
    store.applyDelta([
      { op: 'upsert', row_id: 't2', seq: 200, version: 1 } as any,
    ]);

    const rows = useTradesStore.getState().rows;
    expect(rows.map((r) => r.row_id)).toEqual(['t2', 't1']);
  });

  it('reorders existing row on higher version', () => {
    const store = useTradesStore.getState();
    store.applyDelta([
      { op: 'upsert', row_id: 't1', seq: 100, version: 1 } as any,
      { op: 'upsert', row_id: 't2', seq: 200, version: 1 } as any,
    ]);
    store.applyDelta([
      { op: 'upsert', row_id: 't1', seq: 150, version: 2 } as any,
    ]);

    const rows = useTradesStore.getState().rows;
    expect(rows[0].row_id).toBe('t2');
    expect(rows[1].row_id).toBe('t1');
    expect(rows[1].version).toBe(2);
  });

  it('removes rows on delete events', () => {
    const store = useTradesStore.getState();
    store.applyDelta([
      { op: 'upsert', row_id: 't1', seq: 100, version: 1 } as any,
      { op: 'upsert', row_id: 't2', seq: 200, version: 1 } as any,
    ]);

    store.applyDelta([
      { op: 'delete', row_id: 't1', seq: 300 } as any,
    ]);

    const rows = useTradesStore.getState().rows;
    expect(rows).toHaveLength(1);
    expect(rows[0].row_id).toBe('t2');
  });

  it('enforces caps when applying deltas', () => {
    const store = useTradesStore.getState();
    store.applyDelta([
      { op: 'upsert', row_id: 't1', seq: 100, version: 1 } as any,
      { op: 'upsert', row_id: 't2', seq: 200, version: 1 } as any,
      { op: 'upsert', row_id: 't3', seq: 300, version: 1 } as any,
    ], 2);

    const rows = useTradesStore.getState().rows;
    expect(rows.map((r) => r.row_id)).toEqual(['t3', 't2']);
    expect(useTradesStore.getState().byId.has('t1')).toBe(false);
  });

  it('setSnapshot sorts newest-first by timestamp/seq', () => {
    const store = useTradesStore.getState();
    store.setSnapshot([
      { op: 'upsert', row_id: 'old', seq: 100, version: 1 } as any,
      { op: 'upsert', row_id: 'new', seq: 200, version: 1 } as any,
    ]);

    const rows = useTradesStore.getState().rows;
    expect(rows.map((r) => r.row_id)).toEqual(['new', 'old']);
  });

  it('setSnapshot keeps highest version per row_id', () => {
    const store = useTradesStore.getState();
    store.setSnapshot([
      { op: 'upsert', row_id: 'dup', seq: 100, version: 1 } as any,
      { op: 'upsert', row_id: 'dup', seq: 150, version: 2 } as any,
    ]);

    const rows = useTradesStore.getState().rows;
    expect(rows).toHaveLength(1);
    expect(rows[0].row_id).toBe('dup');
    expect(rows[0].version).toBe(2);
  });

  it('preserves canonical naming fields when normalizing trades into store rows', () => {
    const store = useTradesStore.getState();
    store.setSnapshot([
      {
        op: 'upsert',
        row_id: 'trade-canonical',
        seq: 100,
        version: 1,
        time: '2026-03-07T03:00:00.000Z',
        coin: 'PLUME',
        symbol: 'PLUME/USDT',
        exchange: 'bybit',
        side: 'buy',
        price: '0.0106',
        qty: '1000',
        mv: '10.6',
        fee: 0,
        instrument_id: 'PLUMEUSDT-SPOT.BYBIT',
        venue: 'BYBIT',
        venue_root: 'bybit',
        product_type: 'spot',
        market_type: 'spot',
        contract_type: 'spot',
        raw_symbol: 'PLUMEUSDT',
        base_asset: 'PLUME',
        quote_asset: 'USDT',
        pair: 'PLUME/USDT',
        inventory_asset: 'PLUME',
        display_name_short: 'PLUME Spot',
        display_name_long: 'Bybit PLUME Spot',
      } as any,
    ]);

    expect(useTradesStore.getState().rows[0]).toMatchObject({
      symbol: 'PLUME/USDT',
      instrument_id: 'PLUMEUSDT-SPOT.BYBIT',
      venue: 'BYBIT',
      venue_root: 'bybit',
      product_type: 'spot',
      market_type: 'spot',
      contract_type: 'spot',
      raw_symbol: 'PLUMEUSDT',
      base_asset: 'PLUME',
      quote_asset: 'USDT',
      pair: 'PLUME/USDT',
      inventory_asset: 'PLUME',
      display_name_short: 'PLUME Spot',
      display_name_long: 'Bybit PLUME Spot',
    });
  });

  it('setSnapshot resets lastSeq to snapshot max seq', () => {
    const store = useTradesStore.getState();
    store.applyDelta([
      { op: 'upsert', row_id: 'old-high', seq: 14536, version: 1 } as any,
    ]);
    expect(useTradesStore.getState().lastSeq).toBe(14536);

    store.setSnapshot([
      { op: 'upsert', row_id: 'new-low-a', seq: 666, version: 1 } as any,
      { op: 'upsert', row_id: 'new-low-b', seq: 667, version: 1 } as any,
    ]);

    const state = useTradesStore.getState();
    expect(state.rows.map((row) => row.row_id)).toEqual(['new-low-b', 'new-low-a']);
    expect(state.lastSeq).toBe(667);
  });

  it('rejects stale deltas from older resync epochs', () => {
    const store = useTradesStore.getState() as any;
    store.setSnapshot(
      [{ op: 'upsert', row_id: 'fresh', seq: 200, version: 1 } as any],
      100,
      2,
    );

    const staleStats = store.applyDelta(
      [{ op: 'upsert', row_id: 'stale', seq: 201, version: 1 } as any],
      100,
      1,
    );

    const state = useTradesStore.getState() as any;
    expect(state.rows.map((row: any) => row.row_id)).toEqual(['fresh']);
    expect(state.appliedResyncId).toBe(2);
    expect(staleStats.staleRejected).toBe(1);
  });

  it('rejects old-epoch payloads while newer global resync is active', () => {
    const store = useTradesStore.getState() as any;
    store.setSnapshot(
      [{ op: 'upsert', row_id: 'seed', seq: 100, version: 1 } as any],
      100,
      1,
    );

    const currentEpoch = useResyncStore.getState().bumpResync('test-active-resync');
    expect(currentEpoch).toBe(1);
    const nextEpoch = useResyncStore.getState().bumpResync('test-active-resync-2');
    expect(nextEpoch).toBe(2);
    expect(useResyncStore.getState().isResyncing).toBe(true);

    const staleStats = store.applyDelta(
      [{ op: 'upsert', row_id: 'stale-active', seq: 101, version: 1 } as any],
      100,
      1,
    );

    const state = useTradesStore.getState() as any;
    expect(state.rows.map((row: any) => row.row_id)).toEqual(['seed']);
    expect(staleStats.staleRejected).toBe(1);
    expect(staleStats.accepted).toBe(false);
    expect(staleStats.applied).toBe(false);
  });

  it('does not clear global resync after only trades acknowledges the current epoch', () => {
    registerGlobalResyncConsumer('trades');
    registerGlobalResyncConsumer('order-view');

    const currentResyncId = useResyncStore.getState().bumpResync('two-consumer-contract');
    expect(currentResyncId).toBe(1);

    useResyncStore.getState().markResyncApplied('trades', currentResyncId);

    const state = useResyncStore.getState();
    expect(state.appliedBy).toMatchObject({ trades: currentResyncId });
    expect(state.isResyncing).toBe(true);

    useResyncStore.getState().markResyncApplied('order-view', currentResyncId);
    expect(useResyncStore.getState().isResyncing).toBe(false);
  });

  it('clears trades-only resync for tokenmm trades after trades acknowledges the current epoch', () => {
    window.history.pushState({}, '', '/tokenmm/trades');
    registerGlobalResyncConsumer('trades');

    const currentResyncId = useResyncStore.getState().bumpResync('tokenmm-trades-refresh');
    expect(currentResyncId).toBe(1);

    useResyncStore.getState().markResyncApplied('trades', currentResyncId);
    expect(useResyncStore.getState().isResyncing).toBe(false);
  });

  it('clears trades-only resync for equities trades after trades acknowledges the current epoch', () => {
    window.history.pushState({}, '', '/equities/trades');
    registerGlobalResyncConsumer('trades');

    const currentResyncId = useResyncStore.getState().bumpResync('equities-trades-refresh');
    expect(currentResyncId).toBe(1);

    useResyncStore.getState().markResyncApplied('trades', currentResyncId);
    expect(useResyncStore.getState().isResyncing).toBe(false);
  });

  it('requires both acknowledgements when trades and order-view are both mounted', () => {
    registerGlobalResyncConsumer('trades');
    registerGlobalResyncConsumer('order-view');

    const currentResyncId = useResyncStore.getState().bumpResync('dual-consumer-contract');
    expect(currentResyncId).toBe(1);

    useResyncStore.getState().markResyncApplied('trades', currentResyncId);
    expect(useResyncStore.getState().isResyncing).toBe(true);

    useResyncStore.getState().markResyncApplied('order-view', currentResyncId);
    expect(useResyncStore.getState().isResyncing).toBe(false);
  });

  it('requires consumers that mount while the current epoch is still active', () => {
    registerGlobalResyncConsumer('trades');
    registerGlobalResyncConsumer('order-view');
    unregisterGlobalResyncConsumer('order-view');

    const currentResyncId = useResyncStore.getState().bumpResync('dynamic-consumer-join');
    expect(currentResyncId).toBe(1);

    registerGlobalResyncConsumer('order-view');
    useResyncStore.getState().markResyncApplied('trades', currentResyncId);
    expect(useResyncStore.getState().isResyncing).toBe(true);

    useResyncStore.getState().markResyncApplied('order-view', currentResyncId);
    expect(useResyncStore.getState().isResyncing).toBe(false);
  });

  it('drops order-view from the active epoch once it unmounts and the surface becomes trades-only', () => {
    registerGlobalResyncConsumer('trades');
    registerGlobalResyncConsumer('order-view');

    const currentResyncId = useResyncStore.getState().bumpResync('dual-consumer-to-trades-only');
    expect(currentResyncId).toBe(1);

    unregisterGlobalResyncConsumer('order-view');
    useResyncStore.getState().markResyncApplied('trades', currentResyncId);

    const state = useResyncStore.getState();
    expect(state.requiredConsumers).toEqual(['trades']);
    expect(state.isResyncing).toBe(false);
  });

  it('keeps resync active for rejected/no-op events and after accepted trades apply until order view acknowledges', () => {
    registerGlobalResyncConsumer('trades');
    registerGlobalResyncConsumer('order-view');

    const store = useTradesStore.getState() as any;
    store.setSnapshot(
      [{ op: 'upsert', row_id: 'base', seq: 10, version: 2 } as any],
      100,
      1,
    );

    const currentResyncId = useResyncStore.getState().bumpResync('trade-refresh');
    expect(currentResyncId).toBe(1);
    const nextResyncId = useResyncStore.getState().bumpResync('trade-refresh-2');
    expect(nextResyncId).toBe(2);
    expect(useResyncStore.getState().isResyncing).toBe(true);

    const rejected = store.applyDelta(
      [{ op: 'upsert', row_id: 'rejected', seq: 11, version: 1 } as any],
      100,
      1,
    );
    if (rejected.applied) {
      useResyncStore.getState().markResyncApplied('trades', nextResyncId);
    }
    expect(useResyncStore.getState().isResyncing).toBe(true);

    const noOp = store.applyDelta(
      [{ op: 'upsert', row_id: 'base', seq: 12, version: 2 } as any],
      100,
      nextResyncId,
    );
    if (noOp.applied) {
      useResyncStore.getState().markResyncApplied('trades', nextResyncId);
    }
    expect(noOp.accepted).toBe(true);
    expect(noOp.applied).toBe(false);
    expect(useResyncStore.getState().isResyncing).toBe(true);

    const accepted = store.applyDelta(
      [{ op: 'upsert', row_id: 'base', seq: 13, version: 3 } as any],
      100,
      nextResyncId,
    );
    if (accepted.applied) {
      useResyncStore.getState().markResyncApplied('trades', nextResyncId);
    }
    expect(accepted.accepted).toBe(true);
    expect(accepted.applied).toBe(true);
    expect(useResyncStore.getState().isResyncing).toBe(true);

    useResyncStore.getState().markResyncApplied('order-view', nextResyncId);
    expect(useResyncStore.getState().isResyncing).toBe(false);
  });

  it('does not clear the current resync when trades replays an older epoch acknowledgement', () => {
    registerGlobalResyncConsumer('trades');
    registerGlobalResyncConsumer('order-view');

    const firstEpoch = useResyncStore.getState().bumpResync('epoch-1');
    expect(firstEpoch).toBe(1);
    useResyncStore.getState().markResyncApplied('trades', firstEpoch);
    useResyncStore.getState().markResyncApplied('order-view', firstEpoch);
    expect(useResyncStore.getState().isResyncing).toBe(false);

    const secondEpoch = useResyncStore.getState().bumpResync('epoch-2');
    expect(secondEpoch).toBe(2);

    useResyncStore.getState().markResyncApplied('order-view', secondEpoch);
    useResyncStore.getState().markResyncApplied('trades', firstEpoch);

    const state = useResyncStore.getState();
    expect(state.appliedBy.trades).toBe(firstEpoch);
    expect(state.appliedBy['order-view']).toBe(secondEpoch);
    expect(state.isResyncing).toBe(true);

    useResyncStore.getState().markResyncApplied('trades', secondEpoch);
    expect(useResyncStore.getState().isResyncing).toBe(false);
  });
});

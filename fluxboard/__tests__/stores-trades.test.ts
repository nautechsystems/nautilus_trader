import { beforeEach, describe, expect, it } from 'vitest';
import { useTradesStore } from '../stores';

describe('trades store applyDelta stats', () => {
  beforeEach(() => {
    useTradesStore.getState().clear();
  });

  const makeEvent = (overrides: Record<string, unknown> = {}) => ({
    op: 'upsert',
    row_id: 'row-1',
    version: 1,
    seq: 1,
    ts: 1,
    coin: 'PLUME/USDT',
    exchange: 'bybit',
    side: 'buy',
    price: 0.5,
    qty: 10,
    mv: 5,
    ...overrides,
  });

  it('counts only brand-new rows as newRows', () => {
    const stats1 = useTradesStore.getState().applyDelta([makeEvent()]);
    expect(stats1.newRows).toBe(1);

    const stats2 = useTradesStore.getState().applyDelta([makeEvent({ version: 2, seq: 2 })]);
    expect(stats2.newRows).toBe(0);

    const stats3 = useTradesStore.getState().applyDelta([
      makeEvent({ row_id: 'row-2', seq: 3, version: 1 }),
    ]);
    expect(stats3.newRows).toBe(1);
  });

  it('normalizes side/coin/exchange/time/notional for raw trade events', () => {
    useTradesStore.getState().applyDelta([
      {
        op: 'upsert',
        row_id: 'raw-1',
        version: 1,
        seq: 100,
        ts_ms: 1700000000000,
        instrument_id: 'PLUMEUSDT.BYBIT',
        side: '1',
        price: 0.009974,
        qty: 1000,
      } as any,
    ]);

    const [row] = useTradesStore.getState().rows;
    expect(row).toBeTruthy();
    expect(row.coin).toBe('PLUME');
    expect(row.exchange).toBe('bybit');
    expect(row.side).toBe('buy');
    expect(row.time).toBe(new Date(1700000000000).toISOString());
    expect(row.mv).toBeCloseTo(9.974);
  });
});

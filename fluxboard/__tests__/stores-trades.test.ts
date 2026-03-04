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
});

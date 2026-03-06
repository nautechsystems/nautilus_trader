import { beforeEach, describe, expect, it } from 'vitest';

import { selectActiveStrategies, useSignalStore } from '@/stores';
import type { SignalStrategy } from '@/types';

describe('signal store params merge semantics', () => {
  beforeEach(() => {
    useSignalStore.getState().setRows([]);
  });

  it('merges params patches without dropping existing keys', () => {
    const seeded: SignalStrategy = {
      id: 'strat-1',
      params: { bot_on: '0', qty: '10' },
      legs: {},
      balances_ok: false,
    } as any;

    useSignalStore.getState().setRows([seeded]);
    useSignalStore.getState().mergeStrategy({ id: 'strat-1', params: { bot_on: '1' } } as any);

    const row = useSignalStore.getState().rows.find((r) => r.id === 'strat-1');
    expect(row?.params).toEqual({ bot_on: '1', qty: '10' });
  });

  it('does not throw when a new strategy arrives without params', () => {
    useSignalStore.getState().setRows([]);
    useSignalStore.getState().mergeStrategy({ id: 'strat-2', legs: {}, balances_ok: false } as any);

    expect(() => selectActiveStrategies(useSignalStore.getState())).not.toThrow();
    expect(selectActiveStrategies(useSignalStore.getState())).toEqual([]);
  });
});


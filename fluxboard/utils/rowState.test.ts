import { describe, it, expect } from 'vitest';
import { diffRemoteChanges, type DirtyParamMap, type StrategyParamsMap } from './rowState';

const map = <T extends Record<string, any>>(obj: Record<string, T>): Map<string, T> =>
  new Map(Object.entries(obj));

describe('rowState.diffRemoteChanges', () => {
  it('flags conflicts only when remote changes overlap dirty keys', () => {
    const prev: StrategyParamsMap = map({
      alpha: { qty: '10', bot_on: '0' }
    });
    const next: StrategyParamsMap = map({
      alpha: { qty: '11', bot_on: '0' }
    });
    const dirty: DirtyParamMap = map({
      alpha: new Set(['qty'])
    });

    const { remoteUpdated, conflictingDirty } = diffRemoteChanges(prev, next, dirty);

    expect(remoteUpdated.size).toBe(0);
    expect(conflictingDirty.size).toBe(1);
    expect(conflictingDirty.get('alpha')).toEqual(new Set(['qty']));
  });

  it('treats remote changes as non-conflicting when they do not touch dirty keys', () => {
    const prev: StrategyParamsMap = map({
      alpha: { qty: '10', bot_on: '0' }
    });
    const next: StrategyParamsMap = map({
      alpha: { qty: '12', bot_on: '0' }
    });
    const dirty: DirtyParamMap = map({
      alpha: new Set(['bot_on'])
    });

    const { remoteUpdated, conflictingDirty } = diffRemoteChanges(prev, next, dirty);

    expect(conflictingDirty.size).toBe(0);
    expect(remoteUpdated.has('alpha')).toBe(true);
  });
});

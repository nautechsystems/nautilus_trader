import { describe, expect, it } from 'vitest';

import type { FvSnapshot } from '../types';
import {
  DEFAULT_FV_PROFILE,
  mergeSnapshotWithStickyWhatMoved,
  normalizeProfile,
} from '../lib/fvSnapshot';

const baseSnapshot = (overrides: Partial<FvSnapshot> = {}): FvSnapshot => ({
  symbol: 'ETH_USDT',
  fv_profile: 'fv1',
  ts_ms: 1,
  final: 100,
  base: 100,
  signed_volume: 0,
  overlay_pct: 0,
  terms: [],
  ...overrides,
});

describe('mergeSnapshotWithStickyWhatMoved', () => {
  it('normalizes FV profile with default fallback', () => {
    expect(DEFAULT_FV_PROFILE).toBe('fv1');
    expect(normalizeProfile()).toBe('fv1');
    expect(normalizeProfile('  FV2  ')).toBe('fv2');
    expect(normalizeProfile('   ')).toBe('fv1');
  });

  it('keeps prior what_moved when next snapshot omits it on the same stream', () => {
    const previous = baseSnapshot({
      what_moved: { kind: 'none', trigger: 'timer', delta_final: 0 },
    });
    const next = baseSnapshot({ ts_ms: 2, final: 100.0001 });

    const merged = mergeSnapshotWithStickyWhatMoved(previous, next);

    expect(merged.what_moved).toEqual(previous.what_moved);
  });

  it('does not carry what_moved across different profile streams', () => {
    const previous = baseSnapshot({
      fv_profile: 'fv1',
      what_moved: { kind: 'none', trigger: 'timer' },
    });
    const next = baseSnapshot({ fv_profile: 'fv2', ts_ms: 2 });

    const merged = mergeSnapshotWithStickyWhatMoved(previous, next);

    expect(merged.what_moved).toBeUndefined();
  });

  it('does not carry what_moved across different symbols', () => {
    const previous = baseSnapshot({
      symbol: 'ETH_USDT',
      what_moved: { kind: 'none', trigger: 'timer' },
    });
    const next = baseSnapshot({
      symbol: 'BTC_USDT',
      ts_ms: 2,
    });

    const merged = mergeSnapshotWithStickyWhatMoved(previous, next);

    expect(merged.what_moved).toBeUndefined();
  });

  it('uses next what_moved when provided', () => {
    const previous = baseSnapshot({
      what_moved: { kind: 'none', trigger: 'timer' },
    });
    const next = baseSnapshot({
      ts_ms: 2,
      what_moved: { kind: 'term', term_id: 3, trigger: 'trade' },
    });

    const merged = mergeSnapshotWithStickyWhatMoved(previous, next);

    expect(merged.what_moved).toEqual(next.what_moved);
  });
});

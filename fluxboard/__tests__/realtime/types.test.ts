import { describe, expect, expectTypeOf, it } from 'vitest';
import { REALTIME_STANDARD_SURFACES } from '@/lib/realtime/constants';
import {
  RealtimeSurfaceState,
  STANDARD_CONTRACT_VERSION,
  type RealtimeContractFields,
  type RealtimeSequence,
  type RealtimeSnapshotRevision,
  type RealtimeStreamId,
} from '@/lib/realtime/types';

describe('realtime surface types', () => {
  it('enumerates canonical lifecycle states and contract version', () => {
    expect(STANDARD_CONTRACT_VERSION).toBe(2);
    expect(RealtimeSurfaceState.SYNCING).toBe('syncing');
    expect(RealtimeSurfaceState.LIVE).toBe('live');
    expect(RealtimeSurfaceState.LAGGING).toBe('lagging');
    expect(RealtimeSurfaceState.STALE).toBe('stale');
    expect(RealtimeSurfaceState.RECOVERING).toBe('recovering');
    expect(RealtimeSurfaceState.MANUAL_REFRESH_REQUIRED).toBe('manual_refresh_required');
  });

  it('defines the canonical realtime rollout surfaces', () => {
    expect(REALTIME_STANDARD_SURFACES).toEqual([
      'signal',
      'trades',
      'alerts',
      'marketData',
      'balances',
      'scanners',
    ]);
  });

  it('models standard contract fields as first-class concepts', () => {
    expectTypeOf<RealtimeContractFields>().toMatchTypeOf<{
      contract_version: typeof STANDARD_CONTRACT_VERSION;
      stream_id: RealtimeStreamId;
      seq: RealtimeSequence;
      snapshot_revision: RealtimeSnapshotRevision;
    }>();
  });
});

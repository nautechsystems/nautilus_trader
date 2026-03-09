import { describe, expect, it } from 'vitest';

import { EMPTY_SNAPSHOT_HOLD_MS, evaluateEmptySnapshotPolicy } from '@/components/domain/signal/emptySnapshotPolicy';

describe('evaluateEmptySnapshotPolicy', () => {
  it('retains existing rows on first empty snapshot while disconnected', () => {
    const result = evaluateEmptySnapshotPolicy({
      hasExistingRows: true,
      wsConnected: false,
      nowMs: 10_000,
      emptySinceMs: null,
      holdWindowMs: EMPTY_SNAPSHOT_HOLD_MS,
    });

    expect(result.clearRows).toBe(false);
    expect(result.nextEmptySinceMs).toBe(10_000);
  });

  it('clears rows only after sustained emptiness while disconnected', () => {
    const first = evaluateEmptySnapshotPolicy({
      hasExistingRows: true,
      wsConnected: false,
      nowMs: 10_000,
      emptySinceMs: null,
      holdWindowMs: EMPTY_SNAPSHOT_HOLD_MS,
    });
    const second = evaluateEmptySnapshotPolicy({
      hasExistingRows: true,
      wsConnected: false,
      nowMs: 39_999,
      emptySinceMs: first.nextEmptySinceMs,
      holdWindowMs: EMPTY_SNAPSHOT_HOLD_MS,
    });
    const third = evaluateEmptySnapshotPolicy({
      hasExistingRows: true,
      wsConnected: false,
      nowMs: 40_000,
      emptySinceMs: first.nextEmptySinceMs,
      holdWindowMs: EMPTY_SNAPSHOT_HOLD_MS,
    });

    expect(second.clearRows).toBe(false);
    expect(third.clearRows).toBe(true);
    expect(third.nextEmptySinceMs).toBeNull();
  });

  it('clears after sustained emptiness while websocket is connected', () => {
    const result = evaluateEmptySnapshotPolicy({
      hasExistingRows: true,
      wsConnected: true,
      nowMs: 60_000,
      emptySinceMs: 10_000,
      holdWindowMs: EMPTY_SNAPSHOT_HOLD_MS,
    });

    expect(result.clearRows).toBe(true);
    expect(result.nextEmptySinceMs).toBeNull();
  });

  it('resets empty timer after a non-empty interval', () => {
    // First empty period starts at 10s.
    const first = evaluateEmptySnapshotPolicy({
      hasExistingRows: true,
      wsConnected: false,
      nowMs: 10_000,
      emptySinceMs: null,
      holdWindowMs: EMPTY_SNAPSHOT_HOLD_MS,
    });

    // Non-empty snapshot occurs in between and clears timer in caller state.
    // New empty period should start fresh and not clear at 45s.
    const afterReset = evaluateEmptySnapshotPolicy({
      hasExistingRows: true,
      wsConnected: false,
      nowMs: 45_000,
      emptySinceMs: null,
      holdWindowMs: EMPTY_SNAPSHOT_HOLD_MS,
    });

    expect(first.nextEmptySinceMs).toBe(10_000);
    expect(afterReset.clearRows).toBe(false);
    expect(afterReset.nextEmptySinceMs).toBe(45_000);
  });

  it('ignores stale empty responses that started before a newer non-empty update', () => {
    const result = evaluateEmptySnapshotPolicy({
      hasExistingRows: true,
      wsConnected: true,
      nowMs: 50_000,
      emptySinceMs: null,
      holdWindowMs: EMPTY_SNAPSHOT_HOLD_MS,
      requestStartedAtMs: 10_000,
      lastNonEmptyAtMs: 20_000,
    });

    expect(result.clearRows).toBe(false);
    expect(result.nextEmptySinceMs).toBeNull();
  });

  it('clears immediately on cold start when no rows exist', () => {
    const result = evaluateEmptySnapshotPolicy({
      hasExistingRows: false,
      wsConnected: false,
      nowMs: 10_000,
      emptySinceMs: null,
      holdWindowMs: EMPTY_SNAPSHOT_HOLD_MS,
    });

    expect(result.clearRows).toBe(true);
    expect(result.nextEmptySinceMs).toBeNull();
  });
});

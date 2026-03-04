export type EmptySnapshotPolicyInput = {
  hasExistingRows: boolean;
  wsConnected: boolean;
  nowMs: number;
  emptySinceMs: number | null;
  holdWindowMs: number;
  requestStartedAtMs?: number | null;
  lastNonEmptyAtMs?: number | null;
};

export type EmptySnapshotPolicyResult = {
  clearRows: boolean;
  nextEmptySinceMs: number | null;
};

export const EMPTY_SNAPSHOT_HOLD_MS = 30_000;

/**
 * Decide when an empty snapshot should clear the table.
 *
 * Policy:
 * - Cold start with no rows: clear immediately.
 * - Existing rows: clear only after sustained emptiness.
 * - If WS is connected and this empty response started before a newer non-empty update,
 *   ignore the empty response as stale (out-of-order protection).
 */
export function evaluateEmptySnapshotPolicy({
  hasExistingRows,
  wsConnected,
  nowMs,
  emptySinceMs,
  holdWindowMs,
  requestStartedAtMs,
  lastNonEmptyAtMs,
}: EmptySnapshotPolicyInput): EmptySnapshotPolicyResult {
  if (!hasExistingRows) {
    return { clearRows: true, nextEmptySinceMs: null };
  }

  if (
    wsConnected
    && typeof requestStartedAtMs === 'number'
    && typeof lastNonEmptyAtMs === 'number'
    && lastNonEmptyAtMs > requestStartedAtMs
  ) {
    return { clearRows: false, nextEmptySinceMs: emptySinceMs };
  }

  const holdMs = Math.max(0, holdWindowMs);
  const nextEmptySinceMs = emptySinceMs ?? nowMs;

  const emptyDurationMs = nowMs - nextEmptySinceMs;
  if (emptyDurationMs >= holdMs) {
    return { clearRows: true, nextEmptySinceMs: null };
  }

  return { clearRows: false, nextEmptySinceMs };
}

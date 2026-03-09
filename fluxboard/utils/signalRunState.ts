import type { SignalStrategy, StrategyRunState } from '@/types';

const SIGNAL_RUN_STATE_STALE_MS = 5_000;

function toFiniteTimestampMs(value: unknown): number | null {
  if (typeof value === 'number') {
    return Number.isFinite(value) ? value : null;
  }
  if (typeof value === 'string') {
    const parsed = Number(value);
    return Number.isFinite(parsed) ? parsed : null;
  }
  return null;
}

export function resolveSignalRunning(
  strategy: Pick<SignalStrategy, 'running' | 'state'>,
  nowMs: number,
): boolean | null {
  if (strategy.running === true || strategy.running === false) {
    return strategy.running;
  }

  const state = strategy.state;
  if (!state || typeof state !== 'object') {
    return null;
  }

  const stateName = String((state as Record<string, unknown>).state ?? '').trim().toLowerCase();
  if (!stateName) {
    return null;
  }
  if (stateName === 'on_stop') {
    return false;
  }

  const tsMs = toFiniteTimestampMs(
    (state as Record<string, unknown>).ts_ms ?? (state as Record<string, unknown>).tsMs,
  );
  if (tsMs != null && nowMs - tsMs > SIGNAL_RUN_STATE_STALE_MS) {
    return false;
  }

  return true;
}

export function deriveSignalRunState(
  strategy: Pick<SignalStrategy, 'running' | 'state'>,
  nowMs: number,
): StrategyRunState {
  const running = resolveSignalRunning(strategy, nowMs);
  if (running === true) return 'running';
  if (running === false) return 'stopped';
  return 'unknown';
}

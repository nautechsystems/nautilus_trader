import type { SignalStrategy, StrategyRunState } from '@/types';

const SIGNAL_RUN_STALE_AFTER_MS = 3_000;

function coerceTimestampMs(value: unknown): number | undefined {
  if (typeof value === 'number' && Number.isFinite(value)) {
    return value > 1_000_000_000_000_000 ? Math.trunc(value / 1_000_000) : value;
  }
  if (typeof value === 'string') {
    const parsed = Number(value.trim());
    if (Number.isFinite(parsed)) {
      return parsed > 1_000_000_000_000_000 ? Math.trunc(parsed / 1_000_000) : parsed;
    }
  }
  return undefined;
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

  const tsMs = coerceTimestampMs(
    (state as Record<string, unknown>).ts_ms ?? (state as Record<string, unknown>).ts_event,
  );
  if (tsMs !== undefined && nowMs - tsMs > SIGNAL_RUN_STALE_AFTER_MS) {
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


import type { SignalStrategy, StrategyRunState } from '@/types';

export function resolveSignalRunning(
  strategy: Pick<SignalStrategy, 'running' | 'state'>,
  nowMs: number,
): boolean | null {
  void nowMs;

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

import { describe, expect, it } from 'vitest';

import { deriveSignalRunState, resolveSignalRunning } from './signalRunState';
import type { SignalStrategy } from '@/types';

describe('signalRunState', () => {
  it('treats fresh bot_off state as running even when trading is disabled', () => {
    const nowMs = 1_700_000_000_000;
    const strategy = {
      state: {
        state: 'bot_off',
        ts_ms: nowMs - 500,
        bot_on: false,
      },
    } as Pick<SignalStrategy, 'running' | 'state'>;

    expect(resolveSignalRunning(strategy, nowMs)).toBe(true);
    expect(deriveSignalRunState(strategy, nowMs)).toBe('running');
  });

  it('treats stale running heartbeat as stopped', () => {
    const nowMs = 1_700_000_000_000;
    const strategy = {
      state: {
        state: 'running',
        ts_ms: nowMs - 10_000,
      },
    } as Pick<SignalStrategy, 'running' | 'state'>;

    expect(resolveSignalRunning(strategy, nowMs)).toBe(false);
    expect(deriveSignalRunState(strategy, nowMs)).toBe('stopped');
  });

  it('honors explicit running flag when present', () => {
    const nowMs = 1_700_000_000_000;
    const strategy = {
      running: false,
      state: {
        state: 'running',
        ts_ms: nowMs,
      },
    } as Pick<SignalStrategy, 'running' | 'state'>;

    expect(resolveSignalRunning(strategy, nowMs)).toBe(false);
    expect(deriveSignalRunState(strategy, nowMs)).toBe('stopped');
  });
});


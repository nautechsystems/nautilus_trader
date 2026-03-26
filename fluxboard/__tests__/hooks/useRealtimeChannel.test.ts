import { act, renderHook } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import {
  createRealtimeSurfaceController,
  type RealtimeChannelAdapter,
  useRealtimeChannel,
} from '@/hooks/useRealtimeChannel';

type Row = {
  id: string;
  rank: number;
  value: string;
};

function createController() {
  return createRealtimeSurfaceController<Row>({
    getRowId: (row) => row.id,
    compareRows: (left, right) => right.rank - left.rank,
    batchSchedule: (flush) => {
      const id = window.setTimeout(flush, 0);
      return () => window.clearTimeout(id);
    },
  });
}

describe('useRealtimeChannel', () => {
  beforeEach(() => {
    vi.useFakeTimers();
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  it('applies snapshots and batches live deltas through the shared controller', () => {
    const controller = createController();
    const connections: Array<Parameters<RealtimeChannelAdapter<Row[], any>['connect']>[0]> = [];
    const adapter: RealtimeChannelAdapter<Row[], any> = {
      connect: vi.fn((handlers) => {
        connections.push(handlers);
        return vi.fn();
      }),
    };

    const { result } = renderHook(() =>
      useRealtimeChannel({
        channelKey: 'trades',
        adapter,
        controller,
        recoveryBaseDelayMs: 1_000,
        recoveryMaxDelayMs: 4_000,
      }),
    );

    expect(adapter.connect).toHaveBeenCalledTimes(1);

    act(() => {
      connections[0]?.onOpen?.();
      connections[0]?.onSnapshot?.([
        { id: 'alpha', rank: 3, value: 'A' },
        { id: 'beta', rank: 2, value: 'B' },
      ]);
    });

    expect(result.current.status).toBe('live');
    expect(controller.getSnapshot().rows.map((row) => row.value)).toEqual(['A', 'B']);

    act(() => {
      connections[0]?.onDelta?.({ kind: 'upsert', row: { id: 'alpha', rank: 3, value: 'A+' } });
      connections[0]?.onDelta?.({ kind: 'upsert', row: { id: 'beta', rank: 2, value: 'B+' } });
    });

    expect(controller.getSnapshot().rows.map((row) => row.value)).toEqual(['A', 'B']);

    act(() => {
      vi.advanceTimersByTime(0);
    });

    expect(controller.getSnapshot().rows.map((row) => row.value)).toEqual(['A+', 'B+']);
  });

  it('schedules recovery and reconnects after the backoff delay', () => {
    const controller = createController();
    const connections: Array<Parameters<RealtimeChannelAdapter<Row[], any>['connect']>[0]> = [];
    const adapter: RealtimeChannelAdapter<Row[], any> = {
      connect: vi.fn((handlers) => {
        connections.push(handlers);
        return vi.fn();
      }),
    };

    const { result } = renderHook(() =>
      useRealtimeChannel({
        channelKey: 'trades',
        adapter,
        controller,
        recoveryBaseDelayMs: 1_000,
        recoveryMaxDelayMs: 4_000,
      }),
    );

    act(() => {
      connections[0]?.onOpen?.();
      connections[0]?.onClose?.('socket-lost');
    });

    expect(result.current.status).toBe('recovering');

    act(() => {
      vi.advanceTimersByTime(1_000);
    });

    expect(adapter.connect).toHaveBeenCalledTimes(2);

    act(() => {
      connections[1]?.onOpen?.();
    });

    expect(result.current.status).toBe('live');
    expect(result.current.reconnectAttempt).toBe(0);
  });

  it('discards queued deltas when a full snapshot arrives before the batch flush', () => {
    const controller = createController();
    const connections: Array<Parameters<RealtimeChannelAdapter<Row[], any>['connect']>[0]> = [];
    const adapter: RealtimeChannelAdapter<Row[], any> = {
      connect: vi.fn((handlers) => {
        connections.push(handlers);
        return vi.fn();
      }),
    };

    renderHook(() =>
      useRealtimeChannel({
        channelKey: 'trades',
        adapter,
        controller,
        recoveryBaseDelayMs: 1_000,
        recoveryMaxDelayMs: 4_000,
      }),
    );

    act(() => {
      connections[0]?.onOpen?.();
      connections[0]?.onSnapshot?.([
        { id: 'alpha', rank: 3, value: 'A' },
        { id: 'beta', rank: 2, value: 'B' },
      ]);
      connections[0]?.onDelta?.({ kind: 'upsert', row: { id: 'alpha', rank: 3, value: 'stale-delta' } });
      connections[0]?.onSnapshot?.([
        { id: 'alpha', rank: 3, value: 'fresh-snapshot' },
        { id: 'beta', rank: 2, value: 'B' },
      ]);
    });

    act(() => {
      vi.runOnlyPendingTimers();
    });

    expect(controller.getSnapshot().rows.map((row) => row.value)).toEqual(['fresh-snapshot', 'B']);
  });

  it('invalidates queued and late deltas when recovery starts', () => {
    const controller = createController();
    const connections: Array<Parameters<RealtimeChannelAdapter<Row[], any>['connect']>[0]> = [];
    const adapter: RealtimeChannelAdapter<Row[], any> = {
      connect: vi.fn((handlers) => {
        connections.push(handlers);
        return vi.fn();
      }),
    };

    renderHook(() =>
      useRealtimeChannel({
        channelKey: 'trades',
        adapter,
        controller,
        recoveryBaseDelayMs: 1_000,
        recoveryMaxDelayMs: 4_000,
      }),
    );

    act(() => {
      connections[0]?.onOpen?.();
      connections[0]?.onSnapshot?.([
        { id: 'alpha', rank: 3, value: 'A' },
        { id: 'beta', rank: 2, value: 'B' },
      ]);
      connections[0]?.onDelta?.({ kind: 'upsert', row: { id: 'alpha', rank: 3, value: 'queued-before-close' } });
      connections[0]?.onClose?.('socket-lost');
      connections[0]?.onDelta?.({ kind: 'upsert', row: { id: 'alpha', rank: 3, value: 'late-after-close' } });
    });

    act(() => {
      vi.advanceTimersByTime(0);
    });

    expect(controller.getSnapshot().rows.map((row) => row.value)).toEqual(['A', 'B']);

    act(() => {
      vi.advanceTimersByTime(1_000);
    });

    act(() => {
      connections[1]?.onOpen?.();
      connections[1]?.onSnapshot?.([
        { id: 'alpha', rank: 3, value: 'fresh-after-recovery' },
        { id: 'beta', rank: 2, value: 'B' },
      ]);
      vi.runOnlyPendingTimers();
    });

    expect(controller.getSnapshot().rows.map((row) => row.value)).toEqual(['fresh-after-recovery', 'B']);
  });
});

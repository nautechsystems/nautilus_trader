import { act, renderHook } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { useRecoveryScheduler } from '@/hooks/useRecoveryScheduler';

describe('useRecoveryScheduler', () => {
  beforeEach(() => {
    vi.useFakeTimers();
    vi.setSystemTime(new Date('2026-03-19T00:00:00.000Z'));
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  it('backs off exponentially and de-duplicates concurrent schedules', () => {
    const onRecover = vi.fn();
    const { result } = renderHook(() =>
      useRecoveryScheduler({
        baseDelayMs: 1_000,
        maxDelayMs: 4_000,
        onRecover,
      }),
    );

    act(() => {
      expect(result.current.schedule('disconnect')).toBe(1_000);
      expect(result.current.schedule('duplicate')).toBe(1_000);
    });

    expect(result.current.pending).toBe(true);
    expect(result.current.attempt).toBe(0);

    act(() => {
      vi.advanceTimersByTime(1_000);
    });

    expect(onRecover).toHaveBeenCalledTimes(1);
    expect(onRecover).toHaveBeenLastCalledWith(
      expect.objectContaining({ attempt: 1, delayMs: 1_000, reason: 'disconnect' }),
    );
    expect(result.current.attempt).toBe(1);

    act(() => {
      expect(result.current.schedule('disconnect')).toBe(2_000);
      vi.advanceTimersByTime(2_000);
    });

    expect(onRecover).toHaveBeenCalledTimes(2);
    expect(onRecover).toHaveBeenLastCalledWith(
      expect.objectContaining({ attempt: 2, delayMs: 2_000, reason: 'disconnect' }),
    );

    act(() => {
      expect(result.current.schedule('disconnect')).toBe(4_000);
      vi.advanceTimersByTime(4_000);
    });

    expect(onRecover).toHaveBeenCalledTimes(3);
    expect(onRecover).toHaveBeenLastCalledWith(
      expect.objectContaining({ attempt: 3, delayMs: 4_000, reason: 'disconnect' }),
    );
  });

  it('can cancel and reset pending recovery state', () => {
    const onRecover = vi.fn();
    const { result } = renderHook(() =>
      useRecoveryScheduler({
        baseDelayMs: 1_000,
        maxDelayMs: 4_000,
        onRecover,
      }),
    );

    act(() => {
      result.current.schedule('gap');
      result.current.cancel();
      vi.advanceTimersByTime(1_000);
    });

    expect(onRecover).not.toHaveBeenCalled();
    expect(result.current.pending).toBe(false);

    act(() => {
      result.current.schedule('gap');
      vi.advanceTimersByTime(1_000);
    });

    expect(onRecover).toHaveBeenCalledTimes(1);
    expect(result.current.attempt).toBe(1);

    act(() => {
      result.current.reset();
    });

    expect(result.current.attempt).toBe(0);
    expect(result.current.pending).toBe(false);

    act(() => {
      expect(result.current.schedule('fresh-start')).toBe(1_000);
    });
  });
});

/**
 * Tests for useBoundedSave hook
 *
 * Validates bounded concurrent save operations.
 */

import { describe, it, expect, vi } from 'vitest';
import { renderHook, act, waitFor } from '@testing-library/react';
import { useBoundedSave } from '@/hooks/useBoundedSave';

describe('useBoundedSave', () => {
  it('should initialize with empty state', () => {
    const { result } = renderHook(() => useBoundedSave());

    expect(result.current.saving.size).toBe(0);
    expect(result.current.progress).toBe(null);
    expect(result.current.isSaving('strategy1')).toBe(false);
  });

  it('should mark strategy as saving', () => {
    const { result } = renderHook(() => useBoundedSave());

    act(() => {
      result.current.markSaving('strategy1');
    });

    expect(result.current.isSaving('strategy1')).toBe(true);
    expect(result.current.saving.size).toBe(1);
  });

  it('should mark strategy as done', () => {
    const { result } = renderHook(() => useBoundedSave());

    act(() => {
      result.current.markSaving('strategy1');
      result.current.markDone('strategy1');
    });

    expect(result.current.isSaving('strategy1')).toBe(false);
    expect(result.current.saving.size).toBe(0);
  });

  it('should execute saves successfully', async () => {
    const { result } = renderHook(() => useBoundedSave());
    const items = ['item1', 'item2', 'item3'];
    const saveFn = vi.fn().mockResolvedValue(undefined);

    let saveResult: Awaited<ReturnType<typeof result.current.executeSaves>> | undefined;

    await act(async () => {
      saveResult = await result.current.executeSaves(items, saveFn);
    });

    expect(saveFn).toHaveBeenCalledTimes(3);
    expect(saveResult!.successful).toEqual(items);
    expect(saveResult!.failed).toEqual([]);
    expect(result.current.progress).toBe(null); // Cleared after completion
  });

  it('should handle save failures', async () => {
    const { result } = renderHook(() => useBoundedSave());
    const items = ['item1', 'item2', 'item3'];
    const saveFn = vi.fn()
      .mockResolvedValueOnce(undefined) // item1 succeeds
      .mockRejectedValueOnce(new Error('Network error')) // item2 fails
      .mockResolvedValueOnce(undefined); // item3 succeeds

    let saveResult: Awaited<ReturnType<typeof result.current.executeSaves>> | undefined;

    await act(async () => {
      saveResult = await result.current.executeSaves(items, saveFn);
    });

    expect(saveResult!.successful).toEqual(['item1', 'item3']);
    expect(saveResult!.failed).toHaveLength(1);
    expect(saveResult!.failed[0]).toEqual({
      item: 'item2',
      error: 'Network error',
    });
  });

  it('should respect max concurrency', async () => {
    const { result } = renderHook(() => useBoundedSave());
    const items = Array.from({ length: 10 }, (_, i) => `item${i + 1}`);
    let concurrentCount = 0;
    let maxConcurrent = 0;

    const saveFn = vi.fn(async () => {
      concurrentCount++;
      maxConcurrent = Math.max(maxConcurrent, concurrentCount);
      await new Promise((resolve) => setTimeout(resolve, 10));
      concurrentCount--;
    });

    await act(async () => {
      await result.current.executeSaves(items, saveFn, { maxConcurrency: 3 });
    });

    expect(saveFn).toHaveBeenCalledTimes(10);
    expect(maxConcurrent).toBeLessThanOrEqual(3);
  });

  it('should set progress to null after completion', async () => {
    const { result } = renderHook(() => useBoundedSave());
    const items = ['item1', 'item2'];
    const saveFn = vi.fn().mockResolvedValue(undefined);

    await act(async () => {
      await result.current.executeSaves(items, saveFn);
    });

    // Progress should be cleared after save completes
    expect(result.current.progress).toBe(null);
  });

  it('should handle empty items array', async () => {
    const { result } = renderHook(() => useBoundedSave());
    const saveFn = vi.fn();

    let saveResult: Awaited<ReturnType<typeof result.current.executeSaves>> | undefined;

    await act(async () => {
      saveResult = await result.current.executeSaves([], saveFn);
    });

    expect(saveFn).not.toHaveBeenCalled();
    expect(saveResult!.successful).toEqual([]);
    expect(saveResult!.failed).toEqual([]);
  });

  it('should reset all state', () => {
    const { result } = renderHook(() => useBoundedSave());

    act(() => {
      result.current.markSaving('strategy1');
      result.current.markSaving('strategy2');
      result.current.reset();
    });

    expect(result.current.saving.size).toBe(0);
    expect(result.current.progress).toBe(null);
  });

  it('should handle 50 concurrent saves correctly', async () => {
    const { result } = renderHook(() => useBoundedSave());
    const items = Array.from({ length: 50 }, (_, i) => ({ id: `strategy${i + 1}`, value: i }));
    const saveFn = vi.fn().mockResolvedValue(undefined);

    let saveResult: Awaited<ReturnType<typeof result.current.executeSaves>> | undefined;

    await act(async () => {
      saveResult = await result.current.executeSaves(items, saveFn, { maxConcurrency: 5 });
    });

    expect(saveFn).toHaveBeenCalledTimes(50);
    expect(saveResult!.successful.length).toBe(50);
    expect(saveResult!.failed.length).toBe(0);
  });

  it('should handle partial failures in large batch', async () => {
    const { result } = renderHook(() => useBoundedSave());
    const items = Array.from({ length: 20 }, (_, i) => i + 1);

    // Fail items 5, 10, 15
    const saveFn = vi.fn((item: number) => {
      if (item % 5 === 0) {
        return Promise.reject(new Error(`Failed item ${item}`));
      }
      return Promise.resolve();
    });

    let saveResult: Awaited<ReturnType<typeof result.current.executeSaves>> | undefined;

    await act(async () => {
      saveResult = await result.current.executeSaves(items, saveFn, { maxConcurrency: 5 });
    });

    expect(saveResult!.successful.length).toBe(16); // 20 - 4 failures
    expect(saveResult!.failed.length).toBe(4); // Items 5, 10, 15, 20
    expect(saveResult!.failed.map(f => f.item)).toEqual([5, 10, 15, 20]);
  });
});

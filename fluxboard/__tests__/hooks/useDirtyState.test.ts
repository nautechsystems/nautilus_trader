/**
 * Tests for useDirtyState hook
 *
 * Validates dirty tracking logic for parameter editing.
 */

import { describe, it, expect } from 'vitest';
import { renderHook, act } from '@testing-library/react';
import { useDirtyState } from '@/hooks/useDirtyState';

describe('useDirtyState', () => {
  it('should initialize with empty state', () => {
    const { result } = renderHook(() => useDirtyState());

    expect(result.current.dirtyParams.size).toBe(0);
    expect(result.current.dirtyCount).toBe(0);
    expect(result.current.isDirty('strategy1', 'qty')).toBe(false);
  });

  it('should mark param as dirty when value differs from original', () => {
    const { result } = renderHook(() => useDirtyState());

    act(() => {
      result.current.markDirty('strategy1', 'qty', '100', '50');
    });

    expect(result.current.isDirty('strategy1', 'qty')).toBe(true);
    expect(result.current.dirtyCount).toBe(1);
    expect(result.current.getDirtyKeys('strategy1')).toEqual(new Set(['qty']));
  });

  it('should mark param as clean when value matches original', () => {
    const { result } = renderHook(() => useDirtyState());

    act(() => {
      result.current.markDirty('strategy1', 'qty', '100', '50');
      result.current.markDirty('strategy1', 'qty', '50', '50'); // Back to original
    });

    expect(result.current.isDirty('strategy1', 'qty')).toBe(false);
    expect(result.current.dirtyCount).toBe(0);
  });

  it('should track multiple dirty params for same strategy', () => {
    const { result } = renderHook(() => useDirtyState());

    act(() => {
      result.current.markDirty('strategy1', 'qty', '100', '50');
      result.current.markDirty('strategy1', 'edge', '10', '5');
    });

    expect(result.current.isDirty('strategy1', 'qty')).toBe(true);
    expect(result.current.isDirty('strategy1', 'edge')).toBe(true);
    expect(result.current.dirtyCount).toBe(1); // Still 1 strategy
    expect(result.current.getDirtyKeys('strategy1').size).toBe(2);
  });

  it('should track dirty params across multiple strategies', () => {
    const { result } = renderHook(() => useDirtyState());

    act(() => {
      result.current.markDirty('strategy1', 'qty', '100', '50');
      result.current.markDirty('strategy2', 'edge', '10', '5');
    });

    expect(result.current.isDirty('strategy1', 'qty')).toBe(true);
    expect(result.current.isDirty('strategy2', 'edge')).toBe(true);
    expect(result.current.dirtyCount).toBe(2);
  });

  it('should clear all dirty params for a strategy', () => {
    const { result } = renderHook(() => useDirtyState());

    act(() => {
      result.current.markDirty('strategy1', 'qty', '100', '50');
      result.current.markDirty('strategy1', 'edge', '10', '5');
      result.current.clearDirty('strategy1');
    });

    expect(result.current.isDirty('strategy1', 'qty')).toBe(false);
    expect(result.current.isDirty('strategy1', 'edge')).toBe(false);
    expect(result.current.dirtyCount).toBe(0);
  });

  it('should clear specific dirty param', () => {
    const { result } = renderHook(() => useDirtyState());

    act(() => {
      result.current.markDirty('strategy1', 'qty', '100', '50');
      result.current.markDirty('strategy1', 'edge', '10', '5');
      result.current.clearParam('strategy1', 'qty');
    });

    expect(result.current.isDirty('strategy1', 'qty')).toBe(false);
    expect(result.current.isDirty('strategy1', 'edge')).toBe(true);
    expect(result.current.dirtyCount).toBe(1);
  });

  it('should remove strategy from map when last param is cleared', () => {
    const { result } = renderHook(() => useDirtyState());

    act(() => {
      result.current.markDirty('strategy1', 'qty', '100', '50');
      result.current.clearParam('strategy1', 'qty');
    });

    expect(result.current.dirtyCount).toBe(0);
    expect(result.current.dirtyParams.has('strategy1')).toBe(false);
  });

  it('should reset all dirty state', () => {
    const { result } = renderHook(() => useDirtyState());

    act(() => {
      result.current.markDirty('strategy1', 'qty', '100', '50');
      result.current.markDirty('strategy2', 'edge', '10', '5');
      result.current.resetAll();
    });

    expect(result.current.dirtyCount).toBe(0);
    expect(result.current.dirtyParams.size).toBe(0);
  });

  it('should handle empty string vs whitespace correctly', () => {
    const { result } = renderHook(() => useDirtyState());

    act(() => {
      result.current.markDirty('strategy1', 'qty', '  ', ''); // Whitespace vs empty
    });

    // Both are different values, so should be dirty
    expect(result.current.isDirty('strategy1', 'qty')).toBe(true);
  });

  it('should handle case-sensitive comparisons', () => {
    const { result } = renderHook(() => useDirtyState());

    act(() => {
      result.current.markDirty('strategy1', 'mode', 'FAST', 'fast');
    });

    expect(result.current.isDirty('strategy1', 'mode')).toBe(true);
  });
});

import { act, renderHook } from '@testing-library/react';
import { describe, expect, it, beforeEach } from 'vitest';
import { useResyncStatus } from '@/hooks/useResyncStatus';
import { useResyncStore } from '@/stores';

describe('useResyncStatus', () => {
  beforeEach(() => {
    useResyncStore.getState().resetResyncState();
  });

  it('tracks bump -> applied transitions', () => {
    const { result } = renderHook(() => useResyncStatus());

    expect(result.current.resyncId).toBe(0);
    expect(result.current.isResyncing).toBe(false);

    act(() => {
      useResyncStore.getState().bumpResync('params-save');
    });
    expect(result.current.resyncId).toBe(1);
    expect(result.current.isResyncing).toBe(true);
    expect(result.current.lastReason).toBe('params-save');

    act(() => {
      useResyncStore.getState().markResyncApplied('trades', 0);
    });
    expect(result.current.isResyncing).toBe(true);

    act(() => {
      useResyncStore.getState().markResyncApplied('trades', 1);
    });
    expect(result.current.isResyncing).toBe(false);
  });
});

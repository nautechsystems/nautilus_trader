import { act, renderHook } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { usePolling } from './usePolling';

const setDocumentHidden = (hidden: boolean): void => {
  Object.defineProperty(document, 'hidden', {
    configurable: true,
    get: () => hidden,
  });
};

describe('usePolling', () => {
  beforeEach(() => {
    vi.useFakeTimers();
    setDocumentHidden(false);
  });

  afterEach(() => {
    vi.useRealTimers();
    vi.restoreAllMocks();
  });

  it('runs immediately and then on visible interval when enabled', () => {
    const fetchFn = vi.fn();

    renderHook(() => usePolling(fetchFn, 1000, true));

    expect(fetchFn).toHaveBeenCalledTimes(1);

    act(() => {
      vi.advanceTimersByTime(1000);
    });
    expect(fetchFn).toHaveBeenCalledTimes(2);

    act(() => {
      vi.advanceTimersByTime(2000);
    });
    expect(fetchFn).toHaveBeenCalledTimes(4);
  });

  it('uses hidden interval when document is hidden', () => {
    const fetchFn = vi.fn();
    setDocumentHidden(true);

    renderHook(() => usePolling(fetchFn, 1000, true, { hiddenIntervalMs: 5000 }));

    expect(fetchFn).toHaveBeenCalledTimes(1);

    act(() => {
      vi.advanceTimersByTime(4000);
    });
    expect(fetchFn).toHaveBeenCalledTimes(1);

    act(() => {
      vi.advanceTimersByTime(1000);
    });
    expect(fetchFn).toHaveBeenCalledTimes(2);
  });

  it('switches polling cadence on visibility change and refreshes immediately on visible', () => {
    const fetchFn = vi.fn();

    renderHook(() => usePolling(fetchFn, 1000, true, { hiddenIntervalMs: 5000, refreshOnVisible: true }));

    expect(fetchFn).toHaveBeenCalledTimes(1);

    act(() => {
      vi.advanceTimersByTime(1000);
    });
    expect(fetchFn).toHaveBeenCalledTimes(2);

    setDocumentHidden(true);
    act(() => {
      document.dispatchEvent(new Event('visibilitychange'));
    });

    act(() => {
      vi.advanceTimersByTime(4000);
    });
    expect(fetchFn).toHaveBeenCalledTimes(2);

    act(() => {
      vi.advanceTimersByTime(1000);
    });
    expect(fetchFn).toHaveBeenCalledTimes(3);

    setDocumentHidden(false);
    act(() => {
      document.dispatchEvent(new Event('visibilitychange'));
    });

    expect(fetchFn).toHaveBeenCalledTimes(4);

    act(() => {
      vi.advanceTimersByTime(1000);
    });
    expect(fetchFn).toHaveBeenCalledTimes(5);
  });
});

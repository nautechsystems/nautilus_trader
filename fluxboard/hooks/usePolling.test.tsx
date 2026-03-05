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

  it('attaches a rejection handler when polling callback returns a promise', () => {
    const catchSpy = vi.fn();
    const thenable = { catch: catchSpy };
    const fetchFn = vi.fn(() => thenable as any);

    renderHook(() => usePolling(fetchFn, 1000, true));

    expect(fetchFn).toHaveBeenCalledTimes(1);
    expect(catchSpy).toHaveBeenCalledTimes(1);
  });

  it('does not leak unhandled rejections when the polling function rejects', async () => {
    // Use real timers to allow Node to surface unhandledRejection events reliably.
    vi.useRealTimers();

    const fetchFn = vi.fn(async () => {
      throw new Error('boom');
    });

    const unhandled = vi.fn();
    process.on('unhandledRejection', unhandled);
    try {
      const view = renderHook(() => usePolling(fetchFn, 1000, true));
      // Give the event loop a turn; if the hook doesn't catch the rejection,
      // Node will emit `unhandledRejection`.
      await new Promise((resolve) => setTimeout(resolve, 0));
      view.unmount();
    } finally {
      process.off('unhandledRejection', unhandled);
    }

    expect(unhandled).not.toHaveBeenCalled();
  });
});

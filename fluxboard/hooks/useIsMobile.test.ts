import { renderHook, act } from '@testing-library/react';
import { describe, it, expect, beforeEach, afterEach, vi } from 'vitest';
import { useIsMobile } from './useIsMobile';

const getInnerWidth = () => (global as any).innerWidth as number;

describe('useIsMobile', () => {
  const originalInnerWidth = getInnerWidth();
  let resizeListeners: Array<() => void> = [];

  beforeEach(() => {
    resizeListeners = [];
    // Spy on add/removeEventListener to ensure hook uses resize events
    vi.spyOn(window, 'addEventListener').mockImplementation((event, handler: any) => {
      if (event === 'resize' && typeof handler === 'function') {
        resizeListeners.push(() => handler(new Event('resize')));
      }
    });
    vi.spyOn(window, 'removeEventListener').mockImplementation(() => {});
  });

  afterEach(() => {
    (global as any).innerWidth = originalInnerWidth;
    vi.restoreAllMocks();
  });

  it('returns true when width is below the default breakpoint and updates on resize', () => {
    (global as any).innerWidth = 640;
    const { result } = renderHook(() => useIsMobile());
    expect(result.current).toBe(true);

    act(() => {
      (global as any).innerWidth = 900;
      resizeListeners.forEach(fn => fn());
    });

    expect(result.current).toBe(false);
  });

  it('respects custom breakpoint parameter', () => {
    (global as any).innerWidth = 720;
    const { result } = renderHook(() => useIsMobile(700));

    expect(result.current).toBe(false);

    act(() => {
      (global as any).innerWidth = 680;
      resizeListeners.forEach(fn => fn());
    });

    expect(result.current).toBe(true);
  });
});

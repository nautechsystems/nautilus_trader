// usePolling hook - Standardize polling patterns across components

import { useEffect, useRef } from 'react';

export type UsePollingOptions = {
  hiddenIntervalMs?: number;
  refreshOnVisible?: boolean;
};

/**
 * Hook to poll a function at a given interval
 *
 * @param fetchFn - Function to call on each poll
 * @param interval - Polling interval in milliseconds
 * @param enabled - Whether polling is enabled (default: true)
 *
 * @example
 * ```tsx
 * import { INTERVALS } from '../constants';
 *
 * const loadAlerts = useCallback(async () => {
 *   const data = await api.getAlerts();
 *   setAlerts(data);
 * }, []);
 *
 * usePolling(loadAlerts, INTERVALS.ALERTS_POLL, autoRefreshEnabled);
 * ```
 */
export function usePolling(
  fetchFn: () => void | Promise<void>,
  interval: number,
  enabled: boolean = true,
  options?: UsePollingOptions
): void {
  // Use ref to always have the latest fetchFn without recreating the effect
  const fetchFnRef = useRef(fetchFn);

  useEffect(() => {
    fetchFnRef.current = fetchFn;
  }, [fetchFn]);

  useEffect(() => {
    if (!enabled) return;

    const canUseDocument = typeof document !== 'undefined';
    const hasHiddenInterval =
      typeof options?.hiddenIntervalMs === 'number' && options.hiddenIntervalMs > 0;
    const shouldTrackVisibility = hasHiddenInterval || Boolean(options?.refreshOnVisible);

    const currentInterval = (): number => {
      if (!hasHiddenInterval || !canUseDocument) {
        return interval;
      }
      return document.hidden ? (options?.hiddenIntervalMs as number) : interval;
    };

    let timer: ReturnType<typeof setInterval> | null = null;
    const startTimer = (): void => {
      if (timer !== null) {
        clearInterval(timer);
      }
      timer = setInterval(() => {
        fetchFnRef.current();
      }, currentInterval());
    };

    // Initial fetch
    fetchFnRef.current();
    startTimer();

    const handleVisibilityChange = (): void => {
      if (options?.refreshOnVisible && canUseDocument && !document.hidden) {
        fetchFnRef.current();
      }
      if (hasHiddenInterval) {
        startTimer();
      }
    };

    if (shouldTrackVisibility && canUseDocument) {
      document.addEventListener('visibilitychange', handleVisibilityChange);
    }

    // Cleanup
    return () => {
      if (timer !== null) {
        clearInterval(timer);
      }
      if (shouldTrackVisibility && canUseDocument) {
        document.removeEventListener('visibilitychange', handleVisibilityChange);
      }
    };
  }, [interval, enabled, options?.hiddenIntervalMs, options?.refreshOnVisible]);
}

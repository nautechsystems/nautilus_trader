import { useCallback, useMemo } from 'react';
import { useViewportClock } from '@/hooks/useViewportClock';

type UseVisibleNowMsOptions = {
  intervalMs?: number;
  nowProvider?: () => number;
  refreshKey?: unknown;
  disabled?: boolean;
  root?: Element | Document | null;
  detectScrollParent?: boolean;
};

type UseVisibleNowMsResult<T extends HTMLElement> = {
  nowMs: number;
  isVisible: boolean;
  targetRef: (node: T | null) => void;
};

/**
 * Shared viewport-clock ticker for table cells.
 * The large-table path must not allocate per-cell timers or observers.
 */
export function useVisibleNowMs<T extends HTMLElement = HTMLElement>({
  intervalMs = 1000,
  nowProvider = Date.now,
  refreshKey,
  disabled = false,
  root: _root,
  detectScrollParent: _detectScrollParent = true,
}: UseVisibleNowMsOptions = {}): UseVisibleNowMsResult<T> {
  const tick = useViewportClock({
    clockKey: 'signal:visible-now-ms',
    intervalMs,
    active: !disabled,
  });
  const nowMs = useMemo(() => nowProvider(), [nowProvider, refreshKey, tick]);
  const isVisible = !disabled;

  const targetRef = useCallback((_node: T | null) => {}, []);

  return { nowMs, isVisible, targetRef };
}

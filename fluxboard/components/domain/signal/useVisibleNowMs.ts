import { useCallback, useEffect, useState } from 'react';

type UseVisibleNowMsOptions = {
  intervalMs?: number;
  nowProvider?: () => number;
  disabled?: boolean;
  root?: Element | Document | null;
  detectScrollParent?: boolean;
};

type UseVisibleNowMsResult<T extends HTMLElement> = {
  nowMs: number;
  isVisible: boolean;
  targetRef: (node: T | null) => void;
};

const SCROLLABLE_OVERFLOW_RE = /(auto|scroll|overlay)/i;

function isScrollableElement(element: HTMLElement): boolean {
  const style = window.getComputedStyle(element);
  return (
    SCROLLABLE_OVERFLOW_RE.test(style.overflow)
    || SCROLLABLE_OVERFLOW_RE.test(style.overflowX)
    || SCROLLABLE_OVERFLOW_RE.test(style.overflowY)
  );
}

function findNearestScrollParent(element: HTMLElement | null): HTMLElement | null {
  let current = element?.parentElement ?? null;
  while (current) {
    if (isScrollableElement(current)) return current;
    current = current.parentElement;
  }
  return null;
}

/**
 * Visibility-aware ticker for table cells:
 * updates every interval while intersecting, otherwise pauses.
 */
export function useVisibleNowMs<T extends HTMLElement = HTMLElement>({
  intervalMs = 1000,
  nowProvider = Date.now,
  disabled = false,
  root,
  detectScrollParent = true,
}: UseVisibleNowMsOptions = {}): UseVisibleNowMsResult<T> {
  const [element, setElement] = useState<T | null>(null);
  const [nowMs, setNowMs] = useState<number>(() => nowProvider());
  const [isVisible, setIsVisible] = useState<boolean>(
    () => typeof IntersectionObserver === 'undefined'
  );

  const targetRef = useCallback((node: T | null) => {
    setElement(node);
  }, []);

  useEffect(() => {
    if (disabled) {
      setIsVisible(false);
      return;
    }

    if (typeof IntersectionObserver === 'undefined') {
      setIsVisible(true);
      return;
    }

    if (!element) {
      setIsVisible(false);
      return;
    }

    const resolvedRoot = root ?? (
      detectScrollParent && element instanceof HTMLElement
        ? findNearestScrollParent(element)
        : null
    );

    const observer = new IntersectionObserver((entries) => {
      const entry = entries[0];
      setIsVisible(entry?.isIntersecting ?? true);
    }, {
      root: resolvedRoot ?? null,
    });

    observer.observe(element);
    return () => observer.disconnect();
  }, [detectScrollParent, disabled, element, root]);

  useEffect(() => {
    if (!disabled && isVisible) {
      setNowMs(nowProvider());
    }
  }, [disabled, isVisible, nowProvider]);

  useEffect(() => {
    if (disabled || !isVisible) return;

    const intervalId = window.setInterval(() => {
      setNowMs(nowProvider());
    }, intervalMs);

    return () => window.clearInterval(intervalId);
  }, [disabled, intervalMs, isVisible, nowProvider]);

  return { nowMs, isVisible, targetRef };
}

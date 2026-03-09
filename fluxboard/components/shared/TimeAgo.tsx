import React, { useEffect, useMemo, useState } from 'react';

/**
 * TimeAgo
 *
 * Displays a compact relative time label (e.g., "just now", "4m", "2h")
 * with a title tooltip containing the full local datetime. Colors are
 * muted and can be customized via className; default is neutral.
 */
export type TimeAgoProps = {
  /** Unix timestamp in seconds or milliseconds */
  timestamp: number | null | undefined;
  /** Polling interval in ms for live updates (default: 1000ms) */
  intervalMs?: number;
  /** Additional className for styling */
  className?: string;
  /** Optional override for "now" to keep all cells in sync */
  now?: number;
};

function isMillis(ts: number) {
  // Heuristic: any timestamp larger than year 3000 in seconds is likely ms
  return ts > 1e12;
}

function toMillis(ts: number): number {
  return isMillis(ts) ? ts : ts * 1000;
}

function formatRelative(msAgo: number): string {
  if (msAgo < 1_000) return 'just now';
  const s = Math.floor(msAgo / 1_000);
  if (s < 60) return `${s}s`;
  const m = Math.floor(s / 60);
  if (m < 60) return `${m}m`;
  const h = Math.floor(m / 60);
  if (h < 24) return `${h}h`;
  const d = Math.floor(h / 24);
  return `${d}d`;
}

function formatLocal(tsMs: number): string {
  const d = new Date(tsMs);
  return d.toLocaleString(undefined, {
    year: 'numeric',
    month: '2-digit',
    day: '2-digit',
    hour: '2-digit',
    minute: '2-digit',
    second: '2-digit',
    hour12: false,
  });
}

export function TimeAgo({ timestamp, intervalMs = 1000, className, now }: TimeAgoProps) {
  const [tick, setTick] = useState(0);
  const lastNowRef = React.useRef<number | undefined>(now);

  // Track `now` prop changes to force re-renders when it updates
  useEffect(() => {
    if (typeof now === 'number' && now !== lastNowRef.current) {
      lastNowRef.current = now;
      // Force a tick update when `now` prop changes to trigger recalculation
      setTick((t) => t + 1);
    }
  }, [now]);

  useEffect(() => {
    if (!timestamp) return;

    // When `now` prop is provided, we still need a ticker to trigger re-renders
    // so that useMemo recalculates with the latest `now` value from parent.
    // However, we can optimize the interval when `now` is provided since the
    // parent is already updating `now` periodically.
    const m = window.matchMedia?.('(prefers-reduced-motion: reduce)');
    const effectiveInterval = m?.matches ? Math.max(intervalMs, 5000) : intervalMs;

    // If `now` is provided, use a shorter interval to ensure we react quickly
    // to parent updates. Otherwise, use the standard interval.
    // When `now` is provided and updating, we can rely on the parent's ticker
    // and use a longer interval as a fallback (in case parent stops updating)
    const tickerInterval = typeof now === 'number' ? Math.min(effectiveInterval, 1000) : effectiveInterval;

    const id = window.setInterval(() => {
      // Always increment tick to force recalculation
      setTick((t) => t + 1);
    }, tickerInterval);
    return () => window.clearInterval(id);
  }, [timestamp, intervalMs, now]);

  const { label, title } = useMemo(() => {
    if (!timestamp || timestamp <= 0) {
      return { label: '—', title: '' };
    }
    const tsMs = toMillis(timestamp);
    // When `now` prop is provided, use it; otherwise fall back to Date.now()
    // The `tick` dependency ensures we recalculate even when `now` doesn't change
    // (for cases where parent doesn't provide `now` prop)
    const referenceNow = typeof now === 'number' ? now : Date.now();
    const ageMs = referenceNow - tsMs;
    return {
      label: formatRelative(ageMs),
      title: formatLocal(tsMs),
    };
  }, [timestamp, tick, now]);

  return (
    <span className={className} title={title} aria-label={title}>
      {label}
    </span>
  );
}

export default TimeAgo;

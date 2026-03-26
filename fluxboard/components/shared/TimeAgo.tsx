import React, { useId, useMemo } from 'react';
import { useViewportClock } from '@/hooks/useViewportClock';

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
  /** Shared polling interval in ms for live updates (default: 1000ms) */
  intervalMs?: number;
  /** Additional className for styling */
  className?: string;
  /** Optional inline styles */
  style?: React.CSSProperties;
  /** Optional override for "now" to keep all cells in sync */
  now?: number;
  /** Shared clock key for grouped live cells */
  clockKey?: string;
  /** Stable cell id within the shared clock */
  clockId?: string;
  /** Whether this cell is currently visible and should receive ticks */
  isVisible?: boolean;
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

export function TimeAgo({
  timestamp,
  intervalMs = 1000,
  className,
  style,
  now,
  clockKey,
  clockId,
  isVisible = true,
}: TimeAgoProps) {
  const autoClockId = useId();
  const reducedMotion =
    typeof window !== 'undefined'
      ? window.matchMedia?.('(prefers-reduced-motion: reduce)').matches ?? false
      : false;
  const effectiveInterval = reducedMotion ? Math.max(intervalMs, 5000) : intervalMs;
  const clockNow = useViewportClock({
    clockKey,
    subscriberId: clockId ?? autoClockId,
    intervalMs: effectiveInterval,
    active: Boolean(timestamp) && typeof now !== 'number' && isVisible,
  });

  const { label, title } = useMemo(() => {
    if (!timestamp || timestamp <= 0) {
      return { label: '—', title: '' };
    }
    const tsMs = toMillis(timestamp);
    const referenceNow = typeof now === 'number' ? now : clockNow;
    const ageMs = referenceNow - tsMs;
    return {
      label: formatRelative(ageMs),
      title: formatLocal(tsMs),
    };
  }, [clockNow, now, timestamp]);

  return (
    <span className={className} style={style} title={title} aria-label={title}>
      {label}
    </span>
  );
}

export default TimeAgo;

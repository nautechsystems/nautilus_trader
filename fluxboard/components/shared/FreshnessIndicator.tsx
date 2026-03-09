// Freshness indicator showing data staleness with visual heartbeat

import { useEffect, useMemo, useState } from 'react';
import { StatusDot } from '../ui';
import { colors, typography } from '@/lib/tokens';
import type { StatusDotState } from '../ui';

export interface FreshnessIndicatorProps {
  lastUpdate?: number;  // Unix timestamp in milliseconds
  staleThresholdMs?: number;  // Threshold for stale data (default: 10s)
}

export function FreshnessIndicator({
  lastUpdate,
  staleThresholdMs = 10000  // 10 seconds default
}: FreshnessIndicatorProps) {
  // Tick every second so the displayed "time ago" stays fresh
  // Keep the tick minimal to avoid performance impact
  const [, setNowTick] = useState(0);
  useEffect(() => {
    const id = setInterval(() => setNowTick((n) => (n + 1) % 1_000_000), 1000);
    return () => clearInterval(id);
  }, []);

  const status = useMemo(() => {
    if (!lastUpdate) {
      return {
        dotStatus: 'loading' as StatusDotState,
        text: 'No data'
      };
    }

    const ageMs = Date.now() - lastUpdate;
    const isLive = ageMs < staleThresholdMs;

    return {
      dotStatus: (isLive ? 'live' : 'stale') as StatusDotState,
      text: formatTimeAgo(ageMs)
    };
  }, [lastUpdate, staleThresholdMs]);

  return (
    <div
      className="flex items-center gap-1.5"
      title={`Last updated: ${status.text}`}
    >
      <StatusDot status={status.dotStatus} size="xs" />
      <span
        className="tabular-nums"
        style={{
          fontSize: typography.fontSize.xs,
          color: colors.text.muted,
        }}
      >
        {status.text}
      </span>
    </div>
  );
}

/**
 * Format milliseconds as human-readable time ago string
 */
function formatTimeAgo(ms: number): string {
  if (ms < 1000) {
    return 'just now';
  }

  const seconds = Math.floor(ms / 1000);
  if (seconds < 60) {
    return `${seconds}s`;
  }

  const minutes = Math.floor(seconds / 60);
  if (minutes < 60) {
    return `${minutes}m`;
  }

  const hours = Math.floor(minutes / 60);
  if (hours < 24) {
    return `${hours}h`;
  }

  const days = Math.floor(hours / 24);
  return `${days}d`;
}

/**
 * DataAgeCell - Timestamp display with absolute local time
 *
 * Displays:
 * - Cell: Absolute local time (HH:mm:ss if same day, else MM/DD HH:mm:ss)
 * - Color: Red >30m, amber >15m, neutral ≤15m (matches spec age thresholds)
 *
 * Usage:
 *   <DataAgeCell timestamp={1699564800000} />
 */

import React, { useMemo } from 'react';
import { formatAbsoluteTime } from '@/utils/time';
import { colors, severity, typography } from '@/lib/tokens';

interface DataAgeCellProps {
  /** Unix timestamp in milliseconds */
  timestamp: number | null | undefined;
  /** Optional additional className */
  className?: string;
}

/**
 * Map age thresholds to semantic colors.
 * >30m → critical, >15m → warning, else neutral.
 */
function getAgeColor(timestampMs: number | null | undefined): string {
  if (!timestampMs || timestampMs <= 0) {
    return colors.text.tertiary;
  }

  const now = Date.now();
  const ageMs = now - timestampMs;

  if (ageMs > 30 * 60 * 1000) {
    return severity.critical.color;
  }

  if (ageMs > 15 * 60 * 1000) {
    return severity.warning.color;
  }

  return colors.text.secondary;
}

export function DataAgeCell({ timestamp, className = '' }: DataAgeCellProps) {
  // Memoize calculations to avoid recomputation on every render
  const absoluteTime = useMemo(() => formatAbsoluteTime(timestamp), [timestamp]);
  const color = useMemo(() => getAgeColor(timestamp), [timestamp]);

  return (
    <span
      className={`tabular-nums ${className}`}
      style={{
        color,
        fontSize: typography.fontSize.sm,
      }}
    >
      {absoluteTime}
    </span>
  );
}

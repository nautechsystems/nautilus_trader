import React, { useMemo } from 'react';
import StatusDot from '../ui/badge/StatusDot';
import { colors, spacing, typography } from '@/lib/tokens';

type FreshnessProps = {
  tsEvent?: number | null;
  tsOrderbook?: number | null;
};

const FORMAT_AGE = (ageMs: number | null) => {
  if (ageMs === null) return '-';
  if (ageMs < 1000) return `${ageMs}ms`;
  return `${(ageMs / 1000).toFixed(1)}s`;
};

export function FreshnessCell({ tsEvent, tsOrderbook }: FreshnessProps) {
  const now = Date.now();
  const ageEvent = tsEvent && tsEvent > 0 ? Math.max(0, now - tsEvent) : null;
  const ageOb = tsOrderbook && tsOrderbook > 0 ? Math.max(0, now - tsOrderbook) : null;
  const worstAge = useMemo(() => {
    if (ageEvent === null && ageOb === null) return null;
    if (ageEvent === null) return ageOb;
    if (ageOb === null) return ageEvent;
    return Math.max(ageEvent, ageOb);
  }, [ageEvent, ageOb]);

  const status =
    worstAge !== null && worstAge > 3000 ? 'stale' : 'live'; // >3s => stale indicator
  const textColor = status === 'stale' ? colors.semantic.danger.light : colors.text.secondary;

  return (
    <span
      style={{
        display: 'inline-flex',
        alignItems: 'center',
        gap: spacing.gap.xs,
        fontSize: typography.fontSize.sm,
        color: textColor,
      }}
      title={`ts_event: ${tsEvent ?? '-'} | ts_orderbook: ${tsOrderbook ?? '-'}`}
    >
      <StatusDot status={status === 'stale' ? 'stale' : 'live'} size="xs" />
      <span style={{ fontVariantNumeric: 'tabular-nums' }}>
        evt {FORMAT_AGE(ageEvent)} / ob {FORMAT_AGE(ageOb)}
      </span>
    </span>
  );
}

export default FreshnessCell;

/**
 * StatusDot Component
 *
 * Animated status indicator dot with live/stale/loading states.
 * Used for data freshness indicators and connection status.
 */

import React from 'react';
import { cn } from '@/lib/utils';
import { colors } from '@/lib/tokens';

export type StatusDotState = 'live' | 'stale' | 'loading';
export type StatusDotSize = 'xs' | 'sm' | 'md';

export interface StatusDotProps {
  /**
   * Status state
   * - live: green, pulsing (data actively updating)
   * - stale: red, static (data outdated)
   * - loading: gray, pulsing (waiting for data)
   */
  status: StatusDotState;

  /**
   * Size variant
   * - xs: 6px
   * - sm: 8px
   * - md: 10px
   */
  size?: StatusDotSize;

  /**
   * Override pulse animation (defaults to auto based on status)
   */
  pulse?: boolean;

  /**
   * Additional CSS classes
   */
  className?: string;

  /**
   * Optional aria-label for accessibility
   */
  'aria-label'?: string;
}

/**
 * Get status-specific classes
 */
const STATUS_COLORS: Record<StatusDotState, string> = {
  live: colors.semantic.success.DEFAULT,
  stale: colors.semantic.danger.DEFAULT,
  loading: colors.text.muted,
};

/**
 * Get size-specific classes
 */
function getSizeClasses(size: StatusDotSize): string {
  const sizeMap: Record<StatusDotSize, string> = {
    xs: 'w-1.5 h-1.5',  // 6px
    sm: 'w-2 h-2',      // 8px
    md: 'w-2.5 h-2.5',  // 10px
  };

  return sizeMap[size];
}

/**
 * Determine if dot should pulse
 */
function shouldPulse(status: StatusDotState, pulseProp?: boolean): boolean {
  // Explicit override
  if (pulseProp !== undefined) {
    return pulseProp;
  }

  // Auto-pulse for live and loading states
  return status === 'live' || status === 'loading';
}

/**
 * StatusDot component
 *
 * @example
 * <StatusDot status="live" size="sm" />
 * <StatusDot status="stale" size="md" />
 * <StatusDot status="loading" size="xs" />
 */
export default function StatusDot({
  status,
  size = 'sm',
  pulse,
  className,
  'aria-label': ariaLabel,
}: StatusDotProps) {
  const doPulse = shouldPulse(status, pulse);

  return (
    <span
      className={cn(
        // Base styles
        'inline-block rounded-full',

        // Size
        getSizeClasses(size),

        // Animation
        doPulse && 'animate-pulse',

        // Custom classes
        className
      )}
      aria-label={ariaLabel || `Status: ${status}`}
      role="status"
      aria-live={status === 'live' ? 'polite' : 'off'}
      style={{ backgroundColor: STATUS_COLORS[status] }}
    />
  );
}

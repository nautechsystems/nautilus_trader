/**
 * Badge Component
 *
 * Generalized status badge with variant and size support.
 * Designed for professional trading terminals - compact, legible, consistent.
 */

import React from 'react';
import { cn } from '@/lib/utils';
import { colors } from '@/lib/tokens';

export type BadgeVariant = 'success' | 'danger' | 'warning' | 'info' | 'neutral' | 'outline';
export type BadgeSize = 'xs' | 'sm' | 'md';

export interface BadgeProps extends React.HTMLAttributes<HTMLSpanElement> {
  variant?: BadgeVariant;
  size?: BadgeSize;
  children: React.ReactNode;
  className?: string;
  'aria-label'?: string;
  dot?: boolean; // Optional status dot
}

const VARIANT_STYLES: Record<BadgeVariant, React.CSSProperties> = {
  success: {
    backgroundColor: colors.semantic.success.bg,
    color: colors.semantic.success.light,
    borderColor: colors.semantic.success.border,
  },
  danger: {
    backgroundColor: colors.semantic.danger.bg,
    color: colors.semantic.danger.light,
    borderColor: colors.semantic.danger.border,
  },
  warning: {
    backgroundColor: colors.semantic.warning.bg,
    color: colors.semantic.warning.light,
    borderColor: colors.semantic.warning.border,
  },
  info: {
    backgroundColor: colors.semantic.info.bg,
    color: colors.semantic.info.light,
    borderColor: colors.semantic.info.border,
  },
  neutral: {
    backgroundColor: colors.bg.hover,
    color: colors.text.secondary,
    borderColor: colors.border.DEFAULT,
  },
  outline: {
    backgroundColor: colors.bg.surface,
    color: colors.text.secondary,
    borderColor: colors.border.DEFAULT,
  },
};

const SIZES: Record<BadgeSize, string> = {
  xs: 'text-[10px] px-[6px] py-[3px] h-[18px]',
  sm: 'text-[11px] px-[8px] py-[4px] h-[22px]',
  md: 'text-[12px] px-[10px] py-[5px] h-[26px]',
  };

const DOT_COLORS: Record<BadgeVariant, string> = {
  success: colors.semantic.success.light,
  danger: colors.semantic.danger.light,
  warning: colors.semantic.warning.light,
  info: colors.semantic.info.light,
  neutral: colors.text.muted,
  outline: colors.text.muted,
};

export function Badge({
  variant = 'neutral',
  size = 'sm',
  children,
  className,
  dot,
  'aria-label': ariaLabel,
  role,
  ...rest
}: BadgeProps) {
  return (
    <span
      className={cn(
        'inline-flex items-center justify-center gap-1.5',
        'rounded-[3px] font-semibold tracking-tight whitespace-nowrap select-none border',
        SIZES[size],
        className
      )}
      aria-label={ariaLabel}
      role={role}
      style={VARIANT_STYLES[variant]}
      {...rest}
    >
      {dot && (
        <span
          className="w-1.5 h-1.5 rounded-full"
          style={{ backgroundColor: DOT_COLORS[variant] }}
        />
      )}
      {children}
    </span>
  );
}

export default Badge;

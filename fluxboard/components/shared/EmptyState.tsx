// Standardized empty state component

import { cn } from '@/lib/utils';
import { colors, spacing, typography } from '@/lib/tokens';

export interface EmptyStateProps {
  message?: string;
  className?: string;
  icon?: string;
}

export function EmptyState({
  message = 'No data found',
  className = '',
  icon
}: EmptyStateProps) {
  return (
    <div
      className={cn('flex flex-col items-center justify-center h-full', className)}
      style={{
        gap: spacing.gap.sm,
      }}
    >
      {icon && (
        <div
          className="text-4xl"
          style={{
            color: colors.neutral[600],
            marginBottom: spacing.gap.xs,
          }}
        >
          {icon}
        </div>
      )}
      <div
        className="text-sm"
        style={{
          color: colors.text.muted,
          fontSize: typography.fontSize.sm,
          fontFamily: typography.fontFamily.sans,
        }}
      >
        {message}
      </div>
    </div>
  );
}

export default EmptyState;

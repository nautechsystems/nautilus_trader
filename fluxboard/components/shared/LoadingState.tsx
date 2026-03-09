// Standardized loading state component

import { Loader2 } from 'lucide-react';
import { cn } from '@/lib/utils';
import { colors, spacing, typography } from '@/lib/tokens';

export interface LoadingStateProps {
  message?: string;
  className?: string;
  size?: 'sm' | 'md' | 'lg';
}

export function LoadingState({
  message = 'Loading...',
  className = '',
  size = 'md'
}: LoadingStateProps) {
  const sizeConfig = {
    sm: {
      iconClass: 'w-4 h-4',
      fontSize: typography.fontSize.xs,
    },
    md: {
      iconClass: 'w-5 h-5',
      fontSize: typography.fontSize.sm,
    },
    lg: {
      iconClass: 'w-6 h-6',
      fontSize: typography.fontSize.base,
    },
  };

  const config = sizeConfig[size];

  return (
    <div
      className={cn('flex flex-col items-center justify-center h-full', className)}
      style={{
        gap: spacing.gap.sm,
      }}
    >
      <Loader2
        className={cn('animate-spin', config.iconClass)}
        style={{
          color: colors.text.muted,
        }}
      />
      <div
        style={{
          color: colors.text.muted,
          fontSize: config.fontSize,
          fontFamily: typography.fontFamily.sans,
        }}
      >
        {message}
      </div>
    </div>
  );
}

export default LoadingState;

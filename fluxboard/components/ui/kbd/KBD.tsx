/**
 * KBD Component
 *
 * Keyboard shortcut hint display for documentation and tooltips.
 * Renders keys with proper styling to indicate keyboard input.
 *
 * @example
 * ```tsx
 * <KBD>⌘K</KBD>
 * <KBD>Enter</KBD>
 * <KBD>Ctrl+S</KBD>
 * <span>Press <KBD>Esc</KBD> to cancel</span>
 * ```
 */

import * as React from 'react';
import { cn } from '@/lib/utils';
import { colors, typography, spacing, borderRadius } from '@/lib/tokens';

// =============================================================================
// TYPES
// =============================================================================

export interface KBDProps extends React.HTMLAttributes<HTMLElement> {
  /**
   * Keyboard shortcut text (e.g., "⌘K", "Enter", "Ctrl+S")
   */
  children: React.ReactNode;

  /**
   * Additional CSS classes
   */
  className?: string;
}

// =============================================================================
// KBD COMPONENT
// =============================================================================

const KBD = React.forwardRef<HTMLElement, KBDProps>(
  ({ children, className, ...props }, ref) => {
    return (
      <kbd
        ref={ref}
        className={cn(
          // Base styles
          'inline-block',
          'font-mono',
          'text-xs',
          'font-medium',
          'whitespace-nowrap',

          // Layout
          'px-1.5 py-0.5',
          'rounded',

          // Colors
          'bg-neutral-800',
          'text-neutral-200',
          'border border-neutral-700',

          // Subtle shadow for depth
          'shadow-sm',

          // Custom classes
          className
        )}
        style={{
          fontFamily: typography.fontFamily.mono,
          fontSize: typography.fontSize.xs,
          borderRadius: borderRadius.sm,
          lineHeight: typography.lineHeight.tight,
        }}
        {...props}
      >
        {children}
      </kbd>
    );
  }
);

KBD.displayName = 'KBD';

export { KBD };

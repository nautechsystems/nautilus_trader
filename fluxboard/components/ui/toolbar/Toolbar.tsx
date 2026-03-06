/**
 * Toolbar Component
 *
 * Button group container with configurable spacing and orientation.
 * Used to organize related actions (save/cancel, filters, etc.).
 *
 * @example
 * ```tsx
 * <Toolbar spacing="normal">
 *   <Button>Save</Button>
 *   <Button>Cancel</Button>
 * </Toolbar>
 *
 * <Toolbar orientation="vertical" spacing="loose">
 *   <IconButton icon={<Plus />} />
 *   <IconButton icon={<Edit />} />
 * </Toolbar>
 * ```
 */

import * as React from 'react';
import { cn } from '@/lib/utils';
import { spacing } from '@/lib/tokens';

// =============================================================================
// TYPES
// =============================================================================

export interface ToolbarProps {
  /**
   * Child elements (typically buttons)
   */
  children: React.ReactNode;

  /**
   * Layout orientation
   * @default 'horizontal'
   */
  orientation?: 'horizontal' | 'vertical';

  /**
   * Spacing between children
   * @default 'normal'
   */
  spacing?: 'tight' | 'normal' | 'loose';

  /**
   * Additional CSS classes
   */
  className?: string;

  /**
   * Role for accessibility
   * @default 'toolbar'
   */
  role?: string;

  /**
   * Aria label for accessibility
   */
  'aria-label'?: string;
}

// =============================================================================
// SPACING MAP
// =============================================================================

const SPACING_MAP = {
  tight: spacing.gap.xs,     // 4px
  normal: spacing.gap.sm,    // 8px
  loose: spacing.gap.md,     // 12px
} as const;

// =============================================================================
// TOOLBAR COMPONENT
// =============================================================================

const Toolbar = React.forwardRef<HTMLDivElement, ToolbarProps>(
  (
    {
      children,
      orientation = 'horizontal',
      spacing: spacingProp = 'normal',
      className,
      role = 'toolbar',
      'aria-label': ariaLabel,
      ...props
    },
    ref
  ) => {
    const gapValue = SPACING_MAP[spacingProp];

    return (
      <div
        ref={ref}
        role={role}
        aria-label={ariaLabel}
        className={cn(
          // Base flex layout
          'flex',

          // Orientation
          orientation === 'horizontal' ? 'flex-row items-center' : 'flex-col items-start',

          // Custom classes
          className
        )}
        style={{
          gap: gapValue,
        }}
        {...props}
      >
        {children}
      </div>
    );
  }
);

Toolbar.displayName = 'Toolbar';

export { Toolbar };

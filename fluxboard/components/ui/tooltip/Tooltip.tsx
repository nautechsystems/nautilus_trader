/**
 * Tooltip Component
 *
 * Lightweight tooltip component built on Radix UI with Fluxboard density styling.
 * Provides accessible tooltips with configurable delay and positioning.
 *
 * @example
 * ```tsx
 * <Tooltip content="This is a helpful tooltip" side="top">
 *   <button>Hover me</button>
 * </Tooltip>
 *
 * <Tooltip content="Instant tooltip" delay={0}>
 *   <span>No delay</span>
 * </Tooltip>
 * ```
 */

import * as React from 'react';
import * as TooltipPrimitive from '@radix-ui/react-tooltip';
import { cn } from '@/lib/utils';
import { colors, borderRadius, elevation } from '@/lib/tokens';

// =============================================================================
// TOOLTIP PROVIDER
// =============================================================================

/**
 * Tooltip provider that wraps the application root
 * Must be present in component tree for tooltips to work
 */
export const TooltipProvider = TooltipPrimitive.Provider;

// =============================================================================
// TOOLTIP COMPONENT
// =============================================================================

export interface TooltipProps {
  /**
   * Trigger element that shows tooltip on hover
   */
  children: React.ReactElement;

  /**
   * Tooltip content (string or ReactNode)
   */
  content: React.ReactNode;

  /**
   * Side on which to position the tooltip relative to trigger
   * @default "top"
   */
  side?: 'top' | 'right' | 'bottom' | 'left';

  /**
   * Alignment relative to trigger
   * @default "center"
   */
  align?: 'start' | 'center' | 'end';

  /**
   * Delay before showing tooltip (in milliseconds)
   * @default 200
   */
  delay?: number;

  /**
   * Distance from trigger in pixels
   * @default 4
   */
  sideOffset?: number;

  /**
   * Additional className for tooltip content
   */
  className?: string;

  /**
   * Whether to disable the tooltip
   * @default false
   */
  disabled?: boolean;
}

export const Tooltip = React.forwardRef<HTMLDivElement, TooltipProps>(
  (
    {
      children,
      content,
      side = 'top',
      align = 'center',
      delay = 200,
      sideOffset = 4,
      className,
      disabled = false,
    },
    ref
  ) => {
    // If disabled, just render children without tooltip
    if (disabled || !content) {
      return children;
    }

    return (
      <TooltipPrimitive.Root delayDuration={delay}>
        {/* Trigger */}
        <TooltipPrimitive.Trigger asChild>
          {children}
        </TooltipPrimitive.Trigger>

        {/* Tooltip Portal */}
        <TooltipPrimitive.Portal>
          <TooltipPrimitive.Content
            ref={ref}
            side={side}
            align={align}
            sideOffset={sideOffset}
            className={cn(
              'px-2 py-1 rounded text-xs max-w-xs',
              'shadow-md border',
              'data-[state=delayed-open]:data-[side=top]:animate-slideDownAndFade',
              'data-[state=delayed-open]:data-[side=right]:animate-slideLeftAndFade',
              'data-[state=delayed-open]:data-[side=left]:animate-slideRightAndFade',
              'data-[state=delayed-open]:data-[side=bottom]:animate-slideUpAndFade',
              'select-none',
              className
            )}
            style={{
              backgroundColor: colors.bg.surface,
              borderColor: colors.border.DEFAULT,
              color: colors.text.secondary,
              zIndex: elevation.tooltip,
            }}
            collisionPadding={8}
          >
            {content}

            {/* Arrow */}
            <TooltipPrimitive.Arrow
              className="fill-current"
              width={8}
              height={4}
              style={{ color: colors.bg.surface }}
            />
          </TooltipPrimitive.Content>
        </TooltipPrimitive.Portal>
      </TooltipPrimitive.Root>
    );
  }
);

Tooltip.displayName = 'Tooltip';

// =============================================================================
// SIMPLE TOOLTIP (WITH PROVIDER)
// =============================================================================

/**
 * Simple tooltip that includes its own provider
 * Use this for standalone tooltips outside of a TooltipProvider
 */
export interface SimpleTooltipProps extends TooltipProps {
  /**
   * Skip delay duration for provider (affects all tooltips in group)
   * @default 300
   */
  skipDelayDuration?: number;
}

export const SimpleTooltip: React.FC<SimpleTooltipProps> = ({
  skipDelayDuration = 300,
  ...props
}) => {
  return (
    <TooltipProvider skipDelayDuration={skipDelayDuration}>
      <Tooltip {...props} />
    </TooltipProvider>
  );
};

SimpleTooltip.displayName = 'SimpleTooltip';

// =============================================================================
// ICON TOOLTIP HELPER
// =============================================================================

/**
 * Specialized tooltip for icons with consistent styling
 * Wraps icon in a non-interactive wrapper to prevent focus issues
 */
export interface IconTooltipProps extends Omit<TooltipProps, 'children'> {
  icon: React.ReactElement;
}

export const IconTooltip: React.FC<IconTooltipProps> = ({ icon, ...props }) => {
  return (
    <Tooltip {...props}>
      <span className="inline-flex items-center justify-center cursor-help">
        {icon}
      </span>
    </Tooltip>
  );
};

IconTooltip.displayName = 'IconTooltip';

/**
 * Popover Component
 *
 * Floating popover component built on Radix UI with Fluxboard density styling.
 * Provides accessible popovers with automatic positioning and arrow pointer.
 *
 * @example
 * ```tsx
 * <Popover
 *   trigger={<Button>Open Popover</Button>}
 *   side="bottom"
 *   align="start"
 * >
 *   <div className="p-3">
 *     <h3 className="font-semibold mb-2">Popover Title</h3>
 *     <p>Popover content goes here</p>
 *   </div>
 * </Popover>
 * ```
 */

import * as React from 'react';
import * as PopoverPrimitive from '@radix-ui/react-popover';
import { cn } from '@/lib/utils';
import { colors, borderRadius, elevation } from '@/lib/tokens';

// =============================================================================
// POPOVER COMPONENT
// =============================================================================

export interface PopoverProps {
  /**
   * Trigger element that opens the popover
   */
  trigger: React.ReactNode;

  /**
   * Popover content
   */
  children: React.ReactNode;

  /**
   * Side on which to position the popover relative to trigger
   * @default "bottom"
   */
  side?: 'top' | 'right' | 'bottom' | 'left';

  /**
   * Alignment relative to trigger
   * @default "center"
   */
  align?: 'start' | 'center' | 'end';

  /**
   * Distance from trigger in pixels
   * @default 8
   */
  sideOffset?: number;

  /**
   * Alignment offset in pixels
   * @default 0
   */
  alignOffset?: number;

  /**
   * Whether popover is controlled (open state managed externally)
   */
  open?: boolean;

  /**
   * Callback when open state changes (for controlled mode)
   */
  onOpenChange?: (open: boolean) => void;

  /**
   * Additional className for popover content
   */
  className?: string;

  /**
   * Whether to show arrow pointer
   * @default true
   */
  showArrow?: boolean;

  /**
   * Width mode
   * @default "auto"
   */
  widthMode?: 'auto' | 'trigger';
}

export const Popover = React.forwardRef<HTMLDivElement, PopoverProps>(
  (
    {
      trigger,
      children,
      side = 'bottom',
      align = 'center',
      sideOffset = 8,
      alignOffset = 0,
      open,
      onOpenChange,
      className,
      showArrow = true,
      widthMode = 'auto',
    },
    ref
  ) => {
    return (
      <PopoverPrimitive.Root open={open} onOpenChange={onOpenChange}>
        {/* Trigger */}
        <PopoverPrimitive.Trigger asChild>
          {trigger}
        </PopoverPrimitive.Trigger>

        {/* Popover Portal */}
        <PopoverPrimitive.Portal>
          <PopoverPrimitive.Content
            ref={ref}
            side={side}
            align={align}
            sideOffset={sideOffset}
            alignOffset={alignOffset}
            className={cn(
              'rounded shadow-lg border',
              'data-[state=open]:animate-in data-[state=closed]:animate-out',
              'data-[state=closed]:fade-out-0 data-[state=open]:fade-in-0',
              'data-[state=closed]:zoom-out-95 data-[state=open]:zoom-in-95',
              'data-[side=bottom]:slide-in-from-top-2',
              'data-[side=left]:slide-in-from-right-2',
              'data-[side=right]:slide-in-from-left-2',
              'data-[side=top]:slide-in-from-bottom-2',
              'duration-200',
              widthMode === 'trigger' && 'w-[var(--radix-popover-trigger-width)]',
              className
            )}
            style={{
              backgroundColor: colors.bg.surface,
              borderColor: colors.border.DEFAULT,
              zIndex: elevation.dropdown,
            }}
            collisionPadding={8}
          >
            {children}

            {/* Arrow */}
            {showArrow && (
              <PopoverPrimitive.Arrow
                className="fill-current"
                width={12}
                height={6}
                style={{ color: colors.bg.surface }}
              />
            )}
          </PopoverPrimitive.Content>
        </PopoverPrimitive.Portal>
      </PopoverPrimitive.Root>
    );
  }
);

Popover.displayName = 'Popover';

// =============================================================================
// POPOVER CLOSE TRIGGER
// =============================================================================

/**
 * Close button for use inside popover content
 */
export const PopoverClose = PopoverPrimitive.Close;

// =============================================================================
// POPOVER CONTENT WRAPPER
// =============================================================================

/**
 * Convenient content wrapper with standard padding
 */
export interface PopoverContentWrapperProps {
  children: React.ReactNode;
  className?: string;
  /**
   * Padding size
   * @default "md"
   */
  padding?: 'sm' | 'md' | 'lg';
}

export const PopoverContentWrapper: React.FC<PopoverContentWrapperProps> = ({
  children,
  className,
  padding = 'md',
}) => {
  const paddingClasses = {
    sm: 'p-2',
    md: 'p-3',
    lg: 'p-4',
  };

  return (
    <div className={cn(paddingClasses[padding], className)} style={{ color: colors.text.secondary }}>
      {children}
    </div>
  );
};

PopoverContentWrapper.displayName = 'PopoverContentWrapper';

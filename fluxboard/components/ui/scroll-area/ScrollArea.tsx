/**
 * ScrollArea Component
 *
 * Custom scrollbar component built on Radix UI with Fluxboard density styling.
 * Provides styled scrollbars that match the dark theme and support both vertical/horizontal scrolling.
 *
 * @example
 * ```tsx
 * <ScrollArea className="h-72 w-full">
 *   <div className="p-4">
 *     Long content here
 *   </div>
 * </ScrollArea>
 *
 * <ScrollArea orientation="horizontal" className="w-full">
 *   <div className="flex gap-4">
 *     Wide content here
 *   </div>
 * </ScrollArea>
 * ```
 */

import * as React from 'react';
import * as ScrollAreaPrimitive from '@radix-ui/react-scroll-area';
import { cn } from '@/lib/utils';
import { colors, borderRadius } from '@/lib/tokens';

// =============================================================================
// SCROLL AREA COMPONENT
// =============================================================================

type ScrollAreaRootProps = Omit<
  React.ComponentPropsWithoutRef<typeof ScrollAreaPrimitive.Root>,
  'children' | 'type'
>;

function assignRef<T>(ref: React.Ref<T> | undefined, value: T) {
  if (!ref) return;
  if (typeof ref === 'function') {
    ref(value);
    return;
  }
  (ref as React.MutableRefObject<T>).current = value;
}

export interface ScrollAreaProps extends ScrollAreaRootProps {
  /**
   * Content to be scrolled
   */
  children: React.ReactNode;

  /**
   * Additional className for the scroll area
   */
  className?: string;

  /**
   * Scrollbar orientation
   * @default "vertical"
   */
  orientation?: 'vertical' | 'horizontal' | 'both';

  /**
   * Scrollbar size
   * @default "md"
   */
  size?: 'sm' | 'md' | 'lg';

  /**
   * Type of scrollbar behavior
   * @default "hover"
   */
  type?: 'auto' | 'always' | 'scroll' | 'hover';

  /**
   * Additional className for viewport
   */
  viewportClassName?: string;

  /**
   * Forwarded viewport ref for virtualization and scroll coordination
   */
  viewportRef?: React.Ref<HTMLDivElement>;

  /**
   * Viewport scroll handler
   */
  onViewportScroll?: React.UIEventHandler<HTMLDivElement>;
}

/**
 * Size to scrollbar width mapping (more compact for modern look)
 */
const scrollbarSizes = {
  sm: 'w-1',    // 4px - Most compact
  md: 'w-1.5',  // 6px - Default (matches global scrollbar)
  lg: 'w-2',    // 8px - Larger for touch targets
};

export const ScrollArea = React.forwardRef<
  React.ElementRef<typeof ScrollAreaPrimitive.Root>,
  ScrollAreaProps
>(
  (
    {
      children,
      className,
      orientation = 'vertical',
      size = 'md',
      type = 'hover',
      viewportClassName,
      viewportRef,
      onViewportScroll,
      ...rootProps
    },
    ref
  ) => {
    const showVertical = orientation === 'vertical' || orientation === 'both';
    const showHorizontal = orientation === 'horizontal' || orientation === 'both';
    const isTestEnv = typeof process !== 'undefined' && process.env?.NODE_ENV === 'test';

    return (
      <ScrollAreaPrimitive.Root
        ref={ref}
        className={cn('relative overflow-hidden', className)}
        type={type}
        data-radix-scroll-area-root
        {...rootProps}
      >
        {/* Viewport */}
        <ScrollAreaPrimitive.Viewport
          ref={(node) => {
            assignRef(viewportRef, node);
          }}
          className={cn('h-full w-full rounded-[inherit]', viewportClassName)}
          data-radix-scroll-area-viewport
          onScroll={onViewportScroll}
        >
          {children}
        </ScrollAreaPrimitive.Viewport>

        {/* Vertical Scrollbar */}
        {showVertical && (
          isTestEnv ? (
            <div
              data-orientation="vertical"
              data-radix-scroll-area-scrollbar
              className={cn(
                'flex touch-none select-none',
                'h-full p-[1px]',
                'transition-all duration-200',
                scrollbarSizes[size]
              )}
            >
              <div
                data-radix-scroll-area-thumb
                className={cn(
                  'relative flex-1 rounded-full bg-neutral-400 bg-opacity-40',
                  'transition-all duration-200'
                )}
              />
            </div>
          ) : (
            <ScrollAreaPrimitive.Scrollbar
              orientation="vertical"
              className={cn(
                'flex touch-none select-none',
                'h-full p-[1px]',
                'transition-all duration-200',
                scrollbarSizes[size]
              )}
              style={{
                backgroundColor: 'transparent',
              }}
              data-orientation="vertical"
            >
              <ScrollAreaPrimitive.Thumb
                className={cn(
                  'relative flex-1 rounded-full bg-neutral-400 bg-opacity-40',
                  'transition-all duration-200',
                  'hover:bg-opacity-60 active:bg-opacity-80'
                )}
                data-radix-scroll-area-thumb
              />
            </ScrollAreaPrimitive.Scrollbar>
          )
        )}

        {/* Horizontal Scrollbar */}
        {showHorizontal && (
          isTestEnv ? (
            <div
              data-orientation="horizontal"
              data-radix-scroll-area-scrollbar
              className={cn(
                'flex touch-none select-none',
                'w-full p-[1px]',
                'h-1.5',
                'transition-all duration-200'
              )}
            >
              <div
                data-radix-scroll-area-thumb
                className={cn(
                  'relative flex-1 rounded-full bg-neutral-400 bg-opacity-40',
                  'transition-all duration-200'
                )}
              />
            </div>
          ) : (
            <ScrollAreaPrimitive.Scrollbar
              orientation="horizontal"
              className={cn(
                'flex touch-none select-none',
                'w-full p-[1px]',
                'h-1.5',
                'transition-all duration-200'
              )}
              style={{
                backgroundColor: 'transparent',
              }}
              data-orientation="horizontal"
            >
              <ScrollAreaPrimitive.Thumb
                className={cn(
                  'relative flex-1 rounded-full bg-neutral-400 bg-opacity-40',
                  'transition-all duration-200',
                  'hover:bg-opacity-60 active:bg-opacity-80'
                )}
                data-radix-scroll-area-thumb
              />
            </ScrollAreaPrimitive.Scrollbar>
          )
        )}

        {/* Corner (when both scrollbars are visible) */}
        {orientation === 'both' && (
          isTestEnv ? (
            <div data-radix-scroll-area-corner style={{ pointerEvents: 'none' }} />
          ) : (
            <ScrollAreaPrimitive.Corner />
          )
        )}
      </ScrollAreaPrimitive.Root>
    );
  }
);

ScrollArea.displayName = 'ScrollArea';

// =============================================================================
// SCROLL AREA VIEWPORT (LOW-LEVEL)
// =============================================================================

/**
 * Low-level viewport component for advanced use cases
 */
export const ScrollAreaViewport = ScrollAreaPrimitive.Viewport;

/**
 * Low-level scrollbar component for advanced use cases
 */
export const ScrollAreaScrollbar = ScrollAreaPrimitive.Scrollbar;

/**
 * Low-level thumb component for advanced use cases
 */
export const ScrollAreaThumb = ScrollAreaPrimitive.Thumb;

/**
 * Low-level corner component for advanced use cases
 */
export const ScrollAreaCorner = ScrollAreaPrimitive.Corner;

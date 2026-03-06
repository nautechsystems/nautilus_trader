/**
 * Tabs Component
 *
 * Tab navigation component built on Radix UI with Fluxboard density styling.
 * Provides accessible tabs with keyboard navigation and animated indicator.
 *
 * @example
 * ```tsx
 * <Tabs
 *   defaultValue="tab1"
 *   tabs={[
 *     { value: 'tab1', label: 'First Tab', content: <div>Content 1</div> },
 *     { value: 'tab2', label: 'Second Tab', content: <div>Content 2</div> },
 *   ]}
 * />
 * ```
 */

import * as React from 'react';
import * as TabsPrimitive from '@radix-ui/react-tabs';
import { cn } from '@/lib/utils';
import { colors, spacing, borderRadius, animation } from '@/lib/tokens';
import { useDensityMode } from '@/hooks/useMobileLayout';

// =============================================================================
// TABS COMPONENT
// =============================================================================

export interface Tab {
  /**
   * Unique value for the tab
   */
  value: string;

  /**
   * Display label for tab trigger
   */
  label: string;

  /**
   * Tab content
   */
  content: React.ReactNode;

  /**
   * Whether the tab is disabled
   */
  disabled?: boolean;
}

export interface TabsProps {
  /**
   * List of tabs to display
   */
  tabs: Tab[];

  /**
   * Initial active tab value (for uncontrolled mode)
   */
  defaultValue?: string;

  /**
   * Active tab value (for controlled mode)
   */
  value?: string;

  /**
   * Callback when active tab changes
   */
  onValueChange?: (value: string) => void;

  /**
   * Additional className for tabs container
   */
  className?: string;

  /**
   * Orientation of tabs
   * @default "horizontal"
   */
  orientation?: 'horizontal' | 'vertical';

  /**
   * Size variant
   * @default "md"
   */
  size?: 'sm' | 'md' | 'lg';

  /**
   * Additional className for tab list
   */
  tabListClassName?: string;

  /**
   * Additional className for tab content
   */
  contentClassName?: string;

  /**
   * Density override; defaults to layout density
   */
  density?: 'mobile' | 'desktop';
}

/**
 * Size to spacing mapping
 */
const sizeClasses = {
  sm: {
    trigger: {
      desktop: 'px-2 py-1 text-xs',
      mobile: 'px-3 py-2 text-sm',
    },
    gap: {
      desktop: 'gap-1',
      mobile: 'gap-2',
    },
  },
  md: {
    trigger: {
      desktop: 'px-3 py-1.5 text-sm',
      mobile: 'px-4 py-2 text-base',
    },
    gap: {
      desktop: 'gap-2',
      mobile: 'gap-3',
    },
  },
  lg: {
    trigger: {
      desktop: 'px-4 py-2 text-base',
      mobile: 'px-5 py-2.5 text-lg',
    },
    gap: {
      desktop: 'gap-3',
      mobile: 'gap-4',
    },
  },
} as const;

export const Tabs = React.forwardRef<HTMLDivElement, TabsProps>(
  (
    {
      tabs,
      defaultValue,
      value,
      onValueChange,
      className,
      orientation = 'horizontal',
      size = 'md',
      tabListClassName,
      contentClassName,
      density,
    },
    ref
  ) => {
    // Use first tab as default if not specified
    const initialValue = defaultValue || value || tabs[0]?.value;
    const resolvedDensity = useDensityMode(density);
    const densityKey = resolvedDensity === 'mobile' ? 'mobile' : 'desktop';

    return (
      <TabsPrimitive.Root
        ref={ref}
        defaultValue={initialValue}
        value={value}
        onValueChange={onValueChange}
        orientation={orientation}
        className={cn('flex', orientation === 'vertical' ? 'flex-row' : 'flex-col', className)}
      >
        {/* Tab List */}
        <TabsPrimitive.List
          className={cn(
            'flex border-b',
            orientation === 'vertical' ? 'flex-col border-r border-b-0' : 'flex-row',
            sizeClasses[size].gap[densityKey],
            tabListClassName
          )}
          style={{
            borderColor: colors.border.DEFAULT,
          }}
        >
          {tabs.map((tab) => (
            <TabsPrimitive.Trigger
              key={tab.value}
              value={tab.value}
              disabled={tab.disabled}
              className={cn(
                'relative inline-flex items-center justify-center',
                'font-medium whitespace-nowrap',
                'transition-all duration-200',
                'border-b-2 border-transparent',
                'disabled:pointer-events-none disabled:opacity-50',
                'focus:outline-none focus:ring-2 focus:ring-offset-2',
                'data-[state=active]:border-emerald-500',
                sizeClasses[size].trigger[densityKey]
              )}
              style={{
                color: colors.text.secondary,
                '--tw-ring-color': colors.border.focus,
                '--tw-ring-offset-color': colors.bg.base,
              } as React.CSSProperties}
            >
              {tab.label}
            </TabsPrimitive.Trigger>
          ))}
        </TabsPrimitive.List>

        {/* Tab Content */}
        {tabs.map((tab) => (
          <TabsPrimitive.Content
            key={tab.value}
            value={tab.value}
            className={cn(
              'flex-1 pt-3',
              'focus:outline-none',
              'data-[state=inactive]:hidden',
              contentClassName
            )}
            style={{ color: colors.text.secondary }}
          >
            {tab.content}
          </TabsPrimitive.Content>
        ))}
      </TabsPrimitive.Root>
    );
  }
);

Tabs.displayName = 'Tabs';

// =============================================================================
// LOW-LEVEL TABS COMPONENTS
// =============================================================================

/**
 * Low-level tabs components for advanced use cases
 * Use these when you need more control over layout
 */
export const TabsRoot = TabsPrimitive.Root;
export const TabsList = TabsPrimitive.List;
export const TabsTrigger = TabsPrimitive.Trigger;
export const TabsContent = TabsPrimitive.Content;

/**
 * Switch Component
 *
 * Toggle switch component built on Radix UI with Fluxboard density styling.
 * Provides accessible on/off switch with keyboard support and smooth animation.
 *
 * @example
 * ```tsx
 * <Switch
 *   checked={isEnabled}
 *   onCheckedChange={setIsEnabled}
 *   label="Enable notifications"
 * />
 *
 * <Switch
 *   checked={isDense}
 *   onCheckedChange={setIsDense}
 *   size="sm"
 * />
 * ```
 */

import * as React from 'react';
import type { ComponentPropsWithoutRef } from 'react';
import * as SwitchPrimitive from '@radix-ui/react-switch';
import { cn } from '@/lib/utils';
import { colors, borderRadius, animation } from '@/lib/tokens';
import { useDensityMode } from '@/hooks/useMobileLayout';

// =============================================================================
// SWITCH COMPONENT
// =============================================================================

export interface SwitchProps extends Omit<ComponentPropsWithoutRef<typeof SwitchPrimitive.Root>, 'checked' | 'defaultChecked' | 'onCheckedChange'> {
  /**
   * Whether the switch is on
   */
  checked?: boolean;

  /**
   * Default checked state (uncontrolled mode)
   */
  defaultChecked?: boolean;

  /**
   * Callback when checked state changes
   */
  onCheckedChange?: (checked: boolean) => void;

  /**
   * Optional label text
   */
  label?: string;

  /**
   * Whether the switch is disabled
   * @default false
   */
  disabled?: boolean;

  /**
   * Additional className for switch
   */
  className?: string;

  /**
   * Size variant
   * @default "md"
   */
  size?: 'sm' | 'md';

  /**
   * Label position
   * @default "right"
   */
  labelPosition?: 'left' | 'right';

  /**
   * Name attribute (for form integration)
   */
  name?: string;

  /**
   * ID attribute
   */
  id?: string;

  /**
   * Whether to show label and switch inline (same line)
   * @default true
   */
  inline?: boolean;

  /**
   * Density override; defaults to layout density
   */
  density?: 'mobile' | 'desktop';
}

/**
 * Size to dimension mapping
 */
const sizeConfig = {
  sm: {
    switch: {
      desktop: 'h-4 w-8',
      mobile: '!h-6 !w-12',
    },
    thumb: {
      desktop: 'h-3 w-3 data-[state=checked]:translate-x-4',
      mobile: 'h-4 w-4 data-[state=checked]:translate-x-5',
    },
    label: {
      desktop: 'text-xs',
      mobile: 'text-sm',
    },
  },
  md: {
    switch: {
      desktop: 'h-5 w-10',
      mobile: '!h-7 !w-13',
    },
    thumb: {
      desktop: 'h-4 w-4 data-[state=checked]:translate-x-5',
      mobile: 'h-5 w-5 data-[state=checked]:translate-x-6',
    },
    label: {
      desktop: 'text-sm',
      mobile: 'text-base',
    },
  },
} as const;

export const Switch = React.forwardRef<
  React.ElementRef<typeof SwitchPrimitive.Root>,
  SwitchProps
>(
  (
    {
      checked,
      defaultChecked,
      onCheckedChange,
      label,
      disabled = false,
      className,
      size = 'md',
      labelPosition = 'right',
      name,
      id,
      inline = true,
      density,
      ...rest
    },
    ref
  ) => {
    const resolvedDensity = useDensityMode(density);
    const densityKey = resolvedDensity === 'mobile' ? 'mobile' : 'desktop';
    const preset = sizeConfig[size];
    const switchElement = (
      <SwitchPrimitive.Root
        ref={ref}
        checked={checked}
        defaultChecked={defaultChecked}
        onCheckedChange={onCheckedChange}
        disabled={disabled}
        name={name}
        id={id}
        className={cn(
          'group relative inline-flex shrink-0 cursor-pointer items-center',
          'rounded-[6px] border border-border bg-bg-hover',
          'transition-colors',
          'focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-border-focus focus-visible:ring-offset-0',
          'disabled:cursor-not-allowed disabled:opacity-50',
          'data-[state=checked]:bg-[#2f9b74] data-[state=checked]:border-[#2f9b74]',
          'data-[state=unchecked]:bg-bg-hover data-[state=unchecked]:border-border',
          preset.switch[densityKey],
          className
        )}
        style={{
          '--tw-ring-color': colors.border.focus,
          '--tw-ring-offset-color': colors.bg.surface,
        } as React.CSSProperties}
        {...rest}
      >
        <SwitchPrimitive.Thumb
          className={cn(
            'pointer-events-none block rounded-[3px] shadow-none border border-border bg-neutral-100',
            'transition-transform',
            preset.thumb[densityKey]
          )}
          style={{
            backgroundColor: colors.neutral[50],
          }}
        />
      </SwitchPrimitive.Root>
    );

    // If no label, return just the switch
    if (!label) {
      return switchElement;
    }

    // Render with label
    return (
      <label
        htmlFor={id}
        className={cn(
          'flex cursor-pointer select-none',
          inline ? 'items-center' : 'flex-col',
          inline && 'gap-2',
          !inline && 'gap-1',
          disabled && 'cursor-not-allowed opacity-50',
          labelPosition === 'left' && 'flex-row-reverse'
        )}
      >
        {switchElement}
        <span
          className={cn('font-medium', preset.label[densityKey])}
          style={{ color: colors.text.secondary }}
        >
          {label}
        </span>
      </label>
    );
  }
);

Switch.displayName = 'Switch';

// =============================================================================
// SWITCH GROUP HELPER
// =============================================================================

/**
 * Group multiple switches with consistent spacing
 */
export interface SwitchGroupProps {
  children: React.ReactNode;
  className?: string;
  /**
   * Spacing between switches
   * @default "md"
   */
  spacing?: 'sm' | 'md' | 'lg';
  /**
   * Layout direction
   * @default "vertical"
   */
  direction?: 'horizontal' | 'vertical';
}

export const SwitchGroup: React.FC<SwitchGroupProps> = ({
  children,
  className,
  spacing = 'md',
  direction = 'vertical',
}) => {
  const spacingClasses = {
    sm: direction === 'vertical' ? 'gap-1' : 'gap-2',
    md: direction === 'vertical' ? 'gap-2' : 'gap-3',
    lg: direction === 'vertical' ? 'gap-3' : 'gap-4',
  };

  return (
    <div
      className={cn(
        'flex',
        direction === 'vertical' ? 'flex-col' : 'flex-row flex-wrap',
        spacingClasses[spacing],
        className
      )}
    >
      {children}
    </div>
  );
};

SwitchGroup.displayName = 'SwitchGroup';

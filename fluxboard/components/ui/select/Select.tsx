/**
 * Select Component
 *
 * Dropdown select component built on Radix UI with Fluxboard density styling.
 * Provides accessible select with keyboard navigation and search.
 *
 * @example
 * ```tsx
 * <Select
 *   value={selectedValue}
 *   onChange={handleChange}
 *   options={[
 *     { label: 'Option 1', value: '1' },
 *     { label: 'Option 2', value: '2' },
 *   ]}
 *   placeholder="Select an option"
 * />
 * ```
 */

import * as React from 'react';
import * as SelectPrimitive from '@radix-ui/react-select';
import { Check, ChevronDown, ChevronUp } from 'lucide-react';
import { cn } from '@/lib/utils';
import { colors, spacing, borderRadius, elevation } from '@/lib/tokens';
import { useDensityMode } from '@/hooks/useMobileLayout';

// =============================================================================
// SELECT COMPONENT
// =============================================================================

export interface SelectOption {
  label: string;
  value: string;
  disabled?: boolean;
}

export interface SelectProps {
  /**
   * Currently selected value
   */
  value?: string;

  /**
   * Callback when selection changes
   */
  onChange: (value: string) => void;

  /**
   * List of options to display
   */
  options: SelectOption[];

  /**
   * Placeholder text when no option is selected
   * @default "Select..."
   */
  placeholder?: string;

  /**
   * Whether the select is disabled
   * @default false
   */
  disabled?: boolean;

  /**
   * Additional className for trigger button
   */
  className?: string;

  /**
   * Size variant
   * @default "md"
   */
  size?: 'xs' | 'sm' | 'md' | 'lg';

  /**
   * Name attribute (for form integration)
   */
  name?: string;

  /**
   * Whether to show as full width
   * @default false
   */
  fullWidth?: boolean;

  /**
   * Density override; defaults to layout density
   */
  density?: 'mobile' | 'desktop';
}

type SelectSize = 'xs' | 'sm' | 'md' | 'lg';

/**
 * Size to height mapping
 */
const compactTrigger = {
  desktop: 'h-6 text-xs px-2 gap-1',
  mobile: '!h-9 text-sm px-3',
} as const;

const compactItem = {
  desktop: 'text-xs py-1 px-2',
  mobile: 'text-sm py-2 px-3',
} as const;

const sizeClasses: Record<
  SelectSize,
  {
    trigger: { desktop: string; mobile: string };
    item: { desktop: string; mobile: string };
  }
> = {
  xs: {
    trigger: compactTrigger,
    item: compactItem,
  },
  sm: {
    trigger: compactTrigger,
    item: compactItem,
  },
  md: {
    trigger: {
      desktop: 'h-7 text-sm px-3 gap-2',
      mobile: '!h-10 text-base px-4',
    },
    item: {
      desktop: 'text-sm py-1.5 px-3',
      mobile: 'text-base py-2 px-4',
    },
  },
  lg: {
    trigger: {
      desktop: 'h-8 text-base px-4 gap-2',
      mobile: '!h-11 text-lg px-5',
    },
    item: {
      desktop: 'text-base py-2 px-4',
      mobile: 'text-lg py-2.5 px-4',
    },
  },
} as const;

const resolveSize = (value?: string): SelectSize => {
  if (value && value in sizeClasses) {
    return value as SelectSize;
  }
  return 'md';
};

export const Select = React.forwardRef<HTMLButtonElement, SelectProps>(
  (
    {
      value,
      onChange,
      options,
      placeholder = 'Select...',
      disabled = false,
      className,
      size = 'md',
      name,
      fullWidth = false,
      density,
    },
    ref
  ) => {
    const resolvedDensity = useDensityMode(density);
    const resolvedSize = resolveSize(size);
    const densityKey = resolvedDensity === 'mobile' ? 'mobile' : 'desktop';
    // Find selected option label
    const selectedOption = options.find((opt) => opt.value === value);
    const displayLabel = selectedOption?.label || placeholder;

    return (
      <SelectPrimitive.Root value={value} onValueChange={onChange} disabled={disabled} name={name}>
        {/* Trigger */}
        <SelectPrimitive.Trigger
          ref={ref}
          className={cn(
            'inline-flex items-center justify-between',
            'rounded border',
            'font-medium',
            'transition-colors',
            'focus:outline-none focus:ring-2 focus:ring-offset-2',
            'disabled:pointer-events-none disabled:opacity-50',
            'data-[placeholder]:text-neutral-400',
            sizeClasses[resolvedSize].trigger[densityKey],
            fullWidth && 'w-full',
            className
          )}
          style={{
            backgroundColor: colors.bg.surface,
            borderColor: colors.border.DEFAULT,
            color: selectedOption ? colors.text.secondary : colors.text.muted,
            '--tw-ring-color': colors.border.focus,
            '--tw-ring-offset-color': colors.bg.base,
          } as React.CSSProperties}
        >
          <SelectPrimitive.Value placeholder={placeholder}>
            {displayLabel}
          </SelectPrimitive.Value>

          <SelectPrimitive.Icon asChild>
            <ChevronDown className="h-3.5 w-3.5 opacity-50" />
          </SelectPrimitive.Icon>
        </SelectPrimitive.Trigger>

        {/* Dropdown Content */}
        <SelectPrimitive.Portal>
          <SelectPrimitive.Content
            className={cn(
              'relative overflow-hidden rounded-md border shadow-lg',
              'data-[state=open]:animate-in data-[state=closed]:animate-out',
              'data-[state=closed]:fade-out-0 data-[state=open]:fade-in-0',
              'data-[state=closed]:zoom-out-95 data-[state=open]:zoom-in-95',
              'data-[side=bottom]:slide-in-from-top-2',
              'data-[side=left]:slide-in-from-right-2',
              'data-[side=right]:slide-in-from-left-2',
              'data-[side=top]:slide-in-from-bottom-2',
              'duration-200'
            )}
            style={{
              backgroundColor: colors.bg.surface,
              borderColor: colors.border.DEFAULT,
              zIndex: elevation.dropdown,
            }}
            position="popper"
            sideOffset={4}
          >
            {/* Scroll Up Button */}
            <SelectPrimitive.ScrollUpButton
              className="flex items-center justify-center h-6 cursor-default"
              style={{ backgroundColor: colors.bg.surface }}
            >
              <ChevronUp className="h-4 w-4" style={{ color: colors.text.muted }} />
            </SelectPrimitive.ScrollUpButton>

            {/* Viewport */}
            <SelectPrimitive.Viewport className="p-1">
              {options.map((option) => (
                <SelectItem
                  key={option.value}
                  value={option.value}
                  disabled={option.disabled}
                  size={resolvedSize}
                  density={densityKey}
                >
                  {option.label}
                </SelectItem>
              ))}
            </SelectPrimitive.Viewport>

            {/* Scroll Down Button */}
            <SelectPrimitive.ScrollDownButton
              className="flex items-center justify-center h-6 cursor-default"
              style={{ backgroundColor: colors.bg.surface }}
            >
              <ChevronDown className="h-4 w-4" style={{ color: colors.text.muted }} />
            </SelectPrimitive.ScrollDownButton>
          </SelectPrimitive.Content>
        </SelectPrimitive.Portal>
      </SelectPrimitive.Root>
    );
  }
);

Select.displayName = 'Select';

// =============================================================================
// SELECT ITEM (INTERNAL)
// =============================================================================

interface SelectItemProps {
  value: string;
  disabled?: boolean;
  children: React.ReactNode;
  size: SelectSize;
  density: 'desktop' | 'mobile';
}

const SelectItem = React.forwardRef<HTMLDivElement, SelectItemProps>(
  ({ value, disabled, children, size, density }, ref) => {
    const itemSizeClasses = sizeClasses[size]?.item ?? sizeClasses.md.item;

    return (
      <SelectPrimitive.Item
        ref={ref}
        value={value}
        disabled={disabled}
        className={cn(
          'relative flex items-center rounded-sm cursor-pointer select-none',
          'outline-none transition-colors',
          'focus:bg-neutral-800',
          'data-[disabled]:pointer-events-none data-[disabled]:opacity-50',
          itemSizeClasses[density]
        )}
        style={{
          color: colors.text.secondary,
        }}
      >
        {/* Check icon for selected item */}
        <span className="absolute left-1 inline-flex h-3.5 w-3.5 items-center justify-center">
          <SelectPrimitive.ItemIndicator>
            <Check className="h-3 w-3" style={{ color: colors.accent.DEFAULT }} />
          </SelectPrimitive.ItemIndicator>
        </span>

        {/* Item text */}
        <SelectPrimitive.ItemText className="pl-5">
          {children}
        </SelectPrimitive.ItemText>
      </SelectPrimitive.Item>
    );
  }
);

SelectItem.displayName = 'SelectItem';

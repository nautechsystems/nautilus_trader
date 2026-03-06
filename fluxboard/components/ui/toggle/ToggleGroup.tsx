/**
 * ToggleGroup Component
 *
 * Group of toggle buttons for single or multiple selection.
 * Supports icons, disabled state, and flexible sizing.
 *
 * @example
 * ```tsx
 * // Single selection
 * <ToggleGroup
 *   type="single"
 *   value="grid"
 *   onValueChange={setValue}
 *   options={[
 *     { value: 'list', label: 'List' },
 *     { value: 'grid', label: 'Grid' }
 *   ]}
 * />
 *
 * // Multiple selection
 * <ToggleGroup
 *   type="multiple"
 *   value={['bold', 'italic']}
 *   onValueChange={setValues}
 *   options={[
 *     { value: 'bold', label: 'Bold' },
 *     { value: 'italic', label: 'Italic' },
 *     { value: 'underline', label: 'Underline' }
 *   ]}
 * />
 * ```
 */

import * as React from 'react';
import { cn } from '@/lib/utils';
import { colors, spacing, borderRadius, animation, typography } from '@/lib/tokens';

// =============================================================================
// TYPES
// =============================================================================

export interface ToggleGroupOption {
  /**
   * Unique value for this option
   */
  value: string;

  /**
   * Display label
   */
  label: string;

  /**
   * Optional icon (ReactNode for flexibility)
   */
  icon?: React.ReactNode;

  /**
   * Disable this specific option
   */
  disabled?: boolean;
}

export interface ToggleGroupProps {
  /**
   * Selection mode
   */
  type: 'single' | 'multiple';

  /**
   * Current value(s)
   * - single: string
   * - multiple: string[]
   */
  value: string | string[];

  /**
   * Change handler
   * - single: (value: string) => void
   * - multiple: (values: string[]) => void
   */
  onValueChange: (value: string | string[]) => void;

  /**
   * Available options
   */
  options: ToggleGroupOption[];

  /**
   * Size variant
   * @default 'md'
   */
  size?: 'sm' | 'md' | 'lg';

  /**
   * Disable entire group
   */
  disabled?: boolean;

  /**
   * Additional CSS classes
   */
  className?: string;

  /**
   * Aria label for accessibility
   */
  'aria-label'?: string;
}

// =============================================================================
// TOGGLE GROUP COMPONENT
// =============================================================================

const ToggleGroup = React.forwardRef<HTMLDivElement, ToggleGroupProps>(
  (
    {
      type,
      value,
      onValueChange,
      options,
      size = 'md',
      disabled = false,
      className,
      'aria-label': ariaLabel,
      ...props
    },
    ref
  ) => {
    // Helper to check if option is selected
    const isSelected = (optionValue: string): boolean => {
      if (type === 'single') {
        return value === optionValue;
      } else {
        return Array.isArray(value) && value.includes(optionValue);
      }
    };

    // Handle option click
    const handleToggle = (optionValue: string) => {
      if (disabled) return;

      if (type === 'single') {
        // Single selection: set new value
        onValueChange(optionValue);
      } else {
        // Multiple selection: toggle in array
        const currentValues = Array.isArray(value) ? value : [];
        if (currentValues.includes(optionValue)) {
          onValueChange(currentValues.filter((v) => v !== optionValue));
        } else {
          onValueChange([...currentValues, optionValue]);
        }
      }
    };

    // Size classes
    const sizeClasses = {
      sm: 'h-7 px-2.5 text-xs gap-1',
      md: 'h-9 px-3 text-sm gap-1.5',
      lg: 'h-11 px-4 text-base gap-2',
    }[size];

    return (
      <div
        ref={ref}
        role="group"
        aria-label={ariaLabel}
        className={cn(
          // Base layout
          'inline-flex',
          'rounded',

          // Border around entire group
          'border border-neutral-700',
          'overflow-hidden',

          // Background
          'bg-neutral-900',

          // Custom classes
          className
        )}
        style={{
          borderRadius: borderRadius.DEFAULT,
        }}
        {...props}
      >
        {options.map((option, index) => {
          const selected = isSelected(option.value);
          const isDisabled = disabled || option.disabled;

          return (
            <button
              key={option.value}
              type="button"
              role={type === 'single' ? 'radio' : 'checkbox'}
              aria-checked={selected}
              disabled={isDisabled}
              onClick={() => handleToggle(option.value)}
              className={cn(
                // Base styles
                'inline-flex items-center justify-center',
                'font-medium',
                'transition-all',
                'relative',

                // Size
                sizeClasses,

                // Border between buttons (not on first)
                index > 0 && 'border-l border-neutral-700',

                // States - Not selected
                !selected && !isDisabled && 'bg-neutral-900 text-neutral-300',
                !selected &&
                  !isDisabled &&
                  'hover:bg-neutral-800 hover:text-neutral-100',

                // States - Selected
                selected && !isDisabled && 'bg-emerald-600 text-white',
                selected &&
                  !isDisabled &&
                  'hover:bg-emerald-500',

                // States - Disabled
                isDisabled && 'opacity-50 cursor-not-allowed',

                // Focus ring
                !isDisabled &&
                  'focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-inset focus-visible:ring-emerald-500'
              )}
              style={{
                fontSize:
                  size === 'sm'
                    ? typography.fontSize.xs
                    : size === 'md'
                    ? typography.fontSize.sm
                    : typography.fontSize.base,
                transition: `all ${animation.duration.fast} ${animation.easing.easeOut}`,
              }}
            >
              {/* Icon */}
              {option.icon && (
                <span className="inline-flex shrink-0">{option.icon}</span>
              )}

              {/* Label */}
              <span>{option.label}</span>
            </button>
          );
        })}
      </div>
    );
  }
);

ToggleGroup.displayName = 'ToggleGroup';

export { ToggleGroup };

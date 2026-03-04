/**
 * Checkbox Component
 *
 * Standard checkbox input with label, indeterminate state, and dense mode.
 * Built with accessible focus handling and keyboard support.
 *
 * @example
 * ```tsx
 * <Checkbox checked={true} onChange={handleChange} label="Enable feature" />
 * <Checkbox checked={false} indeterminate={true} label="Select all" />
 * <Checkbox checked={true} disabled label="Read-only option" />
 * <Checkbox dense label="Compact mode" />
 * ```
 */

import * as React from 'react';
import { Check, Minus } from 'lucide-react';
import { cn } from '@/lib/utils';
import { colors, spacing, borderRadius, animation, typography } from '@/lib/tokens';

// =============================================================================
// TYPES
// =============================================================================

export interface CheckboxProps {
  /**
   * Checked state
   */
  checked: boolean;

  /**
   * Change handler
   */
  onChange: (checked: boolean) => void;

  /**
   * Label text (optional)
   */
  label?: string;

  /**
   * Indeterminate state (overrides checked visual)
   */
  indeterminate?: boolean;

  /**
   * Disabled state
   */
  disabled?: boolean;

  /**
   * Dense mode (smaller padding, text)
   */
  dense?: boolean;

  /**
   * Additional CSS classes
   */
  className?: string;

  /**
   * ID for input element
   */
  id?: string;

  /**
   * Name attribute for form handling
   */
  name?: string;

  /**
   * Aria label (if no label prop)
   */
  'aria-label'?: string;
}

// =============================================================================
// CHECKBOX COMPONENT
// =============================================================================

const Checkbox = React.forwardRef<HTMLInputElement, CheckboxProps>(
  (
    {
      checked,
      onChange,
      label,
      indeterminate = false,
      disabled = false,
      dense = false,
      className,
      id,
      name,
      'aria-label': ariaLabel,
      ...props
    },
    ref
  ) => {
    const inputRef = React.useRef<HTMLInputElement>(null);

    // Merge refs
    React.useImperativeHandle(ref, () => inputRef.current!);

    // Set indeterminate property (can only be set via JS)
    React.useEffect(() => {
      if (inputRef.current) {
        inputRef.current.indeterminate = indeterminate;
      }
    }, [indeterminate]);

    // Handle change
    const handleChange = (e: React.ChangeEvent<HTMLInputElement>) => {
      if (disabled) return;
      onChange(e.target.checked);
    };

    // Handle keyboard
    const handleKeyDown = (e: React.KeyboardEvent<HTMLLabelElement>) => {
      if (disabled) return;

      // Space activates checkbox
      if (e.key === ' ') {
        e.preventDefault();
        onChange(!checked);
      }
    };

    // Generate ID if not provided
    const inputId = id || `checkbox-${React.useId()}`;

    return (
      <label
        htmlFor={inputId}
        className={cn(
          // Base layout
          'inline-flex items-center gap-2',

          // Cursor
          disabled ? 'cursor-not-allowed' : 'cursor-pointer',

          // Padding
          dense ? 'py-0.5' : 'py-1',

          // Focus ring on label
          'focus-within:outline-none',

          // Custom classes
          className
        )}
        onKeyDown={handleKeyDown}
        tabIndex={disabled ? -1 : 0}
      >
        {/* Hidden native input */}
        <input
          ref={inputRef}
          type="checkbox"
          id={inputId}
          name={name}
          checked={checked}
          onChange={handleChange}
          disabled={disabled}
          className="sr-only"
          aria-label={ariaLabel || label}
          tabIndex={-1}
          {...props}
        />

        {/* Custom checkbox visual */}
        <span
          className={cn(
            // Base styles
            'inline-flex items-center justify-center',
            'flex-shrink-0',
            'rounded',
            'border-2',
            'transition-all',

            // Size
            dense ? 'w-4 h-4' : 'w-5 h-5',

            // States - Unchecked
            !checked &&
              !indeterminate &&
              'border-neutral-600 bg-neutral-900',

            // States - Checked or Indeterminate
            (checked || indeterminate) &&
              !disabled &&
              'border-emerald-500 bg-emerald-500',

            // States - Disabled
            disabled && 'opacity-50',

            // Hover
            !disabled &&
              !checked &&
              !indeterminate &&
              'hover:border-neutral-500',

            // Focus ring
            'focus-visible:ring-2 focus-visible:ring-emerald-500 focus-visible:ring-offset-2 focus-visible:ring-offset-neutral-900'
          )}
          style={{
            borderRadius: borderRadius.sm,
            transition: `all ${animation.duration.fast} ${animation.easing.easeOut}`,
          }}
          aria-hidden="true"
        >
          {/* Check icon */}
          {checked && !indeterminate && (
            <Check
              className={cn('text-neutral-900', dense ? 'w-3 h-3' : 'w-4 h-4')}
              strokeWidth={3}
            />
          )}

          {/* Indeterminate dash icon */}
          {indeterminate && (
            <Minus
              className={cn('text-neutral-900', dense ? 'w-3 h-3' : 'w-4 h-4')}
              strokeWidth={3}
            />
          )}
        </span>

        {/* Label text */}
        {label && (
          <span
            className={cn(
              'select-none',
              disabled ? 'text-neutral-500' : 'text-neutral-200',
              dense ? 'text-xs' : 'text-sm'
            )}
            style={{
              fontSize: dense ? typography.fontSize.xs : typography.fontSize.sm,
            }}
          >
            {label}
          </span>
        )}
      </label>
    );
  }
);

Checkbox.displayName = 'Checkbox';

export { Checkbox };

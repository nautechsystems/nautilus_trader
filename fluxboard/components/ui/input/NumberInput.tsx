/**
 * NumberInput Component
 *
 * Numeric input with step controls, validation, and precision formatting.
 * Extends TextInput with number-specific features.
 */

import { forwardRef, useCallback, type InputHTMLAttributes } from 'react';
import { ChevronUp, ChevronDown } from 'lucide-react';
import { colors, spacing, typography, borderRadius, animation } from '@/lib/tokens';
import { cn } from '@/lib/utils';

export interface NumberInputProps extends Omit<InputHTMLAttributes<HTMLInputElement>, 'onChange' | 'value' | 'type'> {
  /** Current numeric value */
  value: number | '';
  /** Change handler - receives number or empty string */
  onChange: (value: number | '') => void;
  /** Blur handler */
  onBlur?: () => void;
  /** Focus handler */
  onFocus?: () => void;
  /** Minimum value */
  min?: number;
  /** Maximum value */
  max?: number;
  /** Step increment/decrement value */
  step?: number;
  /** Decimal precision (number of decimal places) */
  precision?: number;
  /** Disabled state */
  disabled?: boolean;
  /** Error state - boolean or error message */
  error?: string | boolean;
  /** Optional label */
  label?: string;
  /** Optional hint text */
  hint?: string;
  /** Show increment/decrement buttons */
  showSteppers?: boolean;
  /** Additional class names */
  className?: string;
}

/**
 * Validate number against min/max bounds
 */
function validateNumber(value: string, min?: number, max?: number): boolean {
  if (value === '' || value === '-') return true; // Allow empty and negative sign
  const num = parseFloat(value);
  if (isNaN(num)) return false;
  if (min !== undefined && num < min) return false;
  if (max !== undefined && num > max) return false;
  return true;
}

/**
 * Format number with specified precision
 */
function formatWithPrecision(value: number | '', precision?: number): string {
  if (value === '') return '';
  if (precision === undefined) return value.toString();
  return value.toFixed(precision);
}

/**
 * NumberInput - Numeric input with validation and step controls
 *
 * Features:
 * - Min/max validation
 * - Precision formatting on blur
 * - Optional increment/decrement buttons
 * - Keyboard support for arrows
 * - Design token-based styling
 */
export const NumberInput = forwardRef<HTMLInputElement, NumberInputProps>(
  (
    {
      value,
      onChange,
      onBlur,
      onFocus,
      min,
      max,
      step = 1,
      precision,
      disabled = false,
      error,
      label,
      hint,
      showSteppers = true,
      className,
      placeholder,
      ...rest
    },
    ref
  ) => {
    const errorMessage = typeof error === 'string' ? error : undefined;
    const hasError = Boolean(error);

    const inputId = rest.id || `number-input-${Math.random().toString(36).substr(2, 9)}`;

    // Handle input change with validation
    const handleChange = useCallback(
      (rawValue: string) => {
        // Allow empty string
        if (rawValue === '') {
          onChange('');
          return;
        }

        // Allow negative sign
        if (rawValue === '-') {
          onChange('');
          return;
        }

        // Validate and parse
        if (!validateNumber(rawValue, min, max)) {
          return; // Reject invalid input
        }

        const num = parseFloat(rawValue);
        if (!isNaN(num)) {
          onChange(num);
        } else {
          onChange('');
        }
      },
      [onChange, min, max]
    );

    // Handle blur - format with precision
    const handleBlur = useCallback(() => {
      if (value !== '') {
        const formatted = formatWithPrecision(value, precision);
        const num = parseFloat(formatted);
        if (!isNaN(num)) {
          onChange(num);
        }
      }
      onBlur?.();
    }, [value, precision, onChange, onBlur]);

    // Increment value
    const increment = useCallback(() => {
      const current = value === '' ? 0 : value;
      const newValue = current + step;
      if (max === undefined || newValue <= max) {
        onChange(newValue);
      }
    }, [value, step, max, onChange]);

    // Decrement value
    const decrement = useCallback(() => {
      const current = value === '' ? 0 : value;
      const newValue = current - step;
      if (min === undefined || newValue >= min) {
        onChange(newValue);
      }
    }, [value, step, min, onChange]);

    // Handle keyboard navigation
    const handleKeyDown = useCallback(
      (e: React.KeyboardEvent<HTMLInputElement>) => {
        if (e.key === 'ArrowUp') {
          e.preventDefault();
          increment();
        } else if (e.key === 'ArrowDown') {
          e.preventDefault();
          decrement();
        }
      },
      [increment, decrement]
    );

    const displayValue = value === '' ? '' : value.toString();

    return (
      <div className={cn('flex flex-col', className)}>
        {/* Label */}
        {label && (
          <label
            htmlFor={inputId}
            className="mb-1 text-xs font-medium text-neutral-200"
            style={{
              fontSize: typography.fontSize.xs,
              fontFamily: typography.fontFamily.sans,
            }}
          >
            {label}
          </label>
        )}

        {/* Input Container */}
        <div className="relative">
          <input
            ref={ref}
            id={inputId}
            type="text"
            inputMode="decimal"
            value={displayValue}
            onChange={(e) => handleChange(e.target.value)}
            onBlur={handleBlur}
            onFocus={onFocus}
            onKeyDown={handleKeyDown}
            placeholder={placeholder}
            disabled={disabled}
            aria-invalid={hasError}
            aria-describedby={
              hasError && errorMessage
                ? `${inputId}-error`
                : hint
                ? `${inputId}-hint`
                : undefined
            }
            className={cn(
              // Base styles
              'w-full py-2 rounded transition-all text-right',
              'text-sm font-mono',
              'outline-none',

              // Padding - adjust for steppers
              showSteppers ? 'pl-3 pr-8' : 'px-3',

              // Background and border
              'border',

              // States - Default
              !hasError &&
                !disabled &&
                'bg-neutral-800 border-neutral-700 text-neutral-100',

              // States - Focus (emerald ring)
              !hasError &&
                !disabled &&
                'focus:border-emerald-500 focus:ring-2 focus:ring-emerald-500/50',

              // States - Error
              hasError &&
                !disabled &&
                'bg-neutral-800 border-red-500 text-neutral-100',
              hasError && !disabled && 'focus:ring-2 focus:ring-red-500/50',

              // States - Disabled
              disabled && 'bg-neutral-900 border-neutral-700 text-neutral-500 cursor-not-allowed',

              // Placeholder
              'placeholder:text-neutral-600'
            )}
            style={{
              fontSize: typography.fontSize.sm,
              fontFamily: typography.fontFamily.mono,
              borderRadius: borderRadius.DEFAULT,
              transition: `all ${animation.duration.fast} ${animation.easing.easeOut}`,
            }}
            {...rest}
          />

          {/* Step buttons */}
          {showSteppers && !disabled && (
            <div className="absolute right-1 top-1/2 -translate-y-1/2 flex flex-col gap-0.5">
              <button
                type="button"
                onClick={increment}
                disabled={max !== undefined && value !== '' && value >= max}
                className={cn(
                  'p-0.5 rounded transition-colors',
                  'hover:bg-neutral-700',
                  'disabled:opacity-30 disabled:cursor-not-allowed disabled:hover:bg-transparent',
                  'focus:outline-none focus:ring-1 focus:ring-emerald-500'
                )}
                aria-label="Increment"
                tabIndex={-1}
              >
                <ChevronUp size={12} className="text-neutral-400" />
              </button>
              <button
                type="button"
                onClick={decrement}
                disabled={min !== undefined && value !== '' && value <= min}
                className={cn(
                  'p-0.5 rounded transition-colors',
                  'hover:bg-neutral-700',
                  'disabled:opacity-30 disabled:cursor-not-allowed disabled:hover:bg-transparent',
                  'focus:outline-none focus:ring-1 focus:ring-emerald-500'
                )}
                aria-label="Decrement"
                tabIndex={-1}
              >
                <ChevronDown size={12} className="text-neutral-400" />
              </button>
            </div>
          )}
        </div>

        {/* Hint text */}
        {hint && !hasError && (
          <p
            id={`${inputId}-hint`}
            className="mt-1 text-xs text-neutral-400"
            style={{
              fontSize: typography.fontSize['2xs'],
              fontFamily: typography.fontFamily.sans,
            }}
          >
            {hint}
          </p>
        )}

        {/* Error message */}
        {hasError && errorMessage && (
          <p
            id={`${inputId}-error`}
            className="mt-1 text-xs text-red-400"
            role="alert"
            style={{
              fontSize: typography.fontSize['2xs'],
              fontFamily: typography.fontFamily.sans,
              color: colors.semantic.danger.light,
            }}
          >
            {errorMessage}
          </p>
        )}
      </div>
    );
  }
);

NumberInput.displayName = 'NumberInput';

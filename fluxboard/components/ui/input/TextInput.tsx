/**
 * TextInput Component
 *
 * Standard text input with states: default, focus, error, disabled.
 * Uses design tokens for consistent styling across Fluxboard.
 */

import { forwardRef, type InputHTMLAttributes } from 'react';
import { colors, spacing, typography, borderRadius, animation } from '@/lib/tokens';
import { cn } from '@/lib/utils';

export interface TextInputProps extends Omit<InputHTMLAttributes<HTMLInputElement>, 'onChange'> {
  /** Current input value */
  value: string;
  /** Change handler */
  onChange: (value: string) => void;
  /** Blur handler */
  onBlur?: () => void;
  /** Focus handler */
  onFocus?: () => void;
  /** Placeholder text */
  placeholder?: string;
  /** Disabled state */
  disabled?: boolean;
  /** Error state - boolean or error message */
  error?: string | boolean;
  /** Optional label */
  label?: string;
  /** Optional hint text */
  hint?: string;
  /** Additional class names */
  className?: string;
  /** Input type */
  type?: 'text' | 'email' | 'password' | 'url' | 'tel' | 'search';
}

/**
 * TextInput - Standard text input with error states and focus rings
 *
 * Features:
 * - Emerald focus ring
 * - Error state with red border and message
 * - Label and hint text support
 * - Disabled state
 * - Design token-based styling
 */
export const TextInput = forwardRef<HTMLInputElement, TextInputProps>(
  (
    {
      value,
      onChange,
      onBlur,
      onFocus,
      placeholder,
      disabled = false,
      error,
      label,
      hint,
      className,
      type = 'text',
      ...rest
    },
    ref
  ) => {
    const errorMessage = typeof error === 'string' ? error : undefined;
    const hasError = Boolean(error);

    const inputId = rest.id || `text-input-${Math.random().toString(36).substr(2, 9)}`;

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
            type={type}
            value={value}
            onChange={(e) => onChange(e.target.value)}
            onBlur={onBlur}
            onFocus={onFocus}
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
              'w-full px-3 py-2 rounded transition-all',
              'text-sm font-mono',
              'outline-none',

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

TextInput.displayName = 'TextInput';

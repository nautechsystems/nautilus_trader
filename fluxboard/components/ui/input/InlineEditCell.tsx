/**
 * InlineEditCell Component
 *
 * Table cell with inline editing - critical for Params panel migration.
 * Supports text and number types with validation.
 */

import { useState, useRef, useEffect, useCallback, type KeyboardEvent } from 'react';
import { colors, spacing, typography, borderRadius, animation } from '@/lib/tokens';
import { cn } from '@/lib/utils';

export interface InlineEditCellProps {
  /** Current cell value */
  value: string | number;
  /** Change handler - called on edit */
  onChange: (value: string | number) => void;
  /** Save handler - called on Enter or blur */
  onSave: (value: string | number) => void;
  /** Cancel handler - called on Esc */
  onCancel?: () => void;
  /** Input type */
  type?: 'text' | 'number';
  /** Validation function - return true if valid */
  validation?: (value: string | number) => boolean;
  /** Min value (for number type) */
  min?: number;
  /** Max value (for number type) */
  max?: number;
  /** Precision (for number type) */
  precision?: number;
  /** Additional class names */
  className?: string;
  /** Disabled state */
  disabled?: boolean;
}

/**
 * InlineEditCell - Click-to-edit table cell
 *
 * Features:
 * - Click to enter edit mode
 * - Enter to save
 * - Esc to cancel
 * - Blur to save (if valid)
 * - Validation with error styling
 * - Number and text modes
 *
 * Interaction:
 * - View mode: Displays value, click to edit
 * - Edit mode: Shows input, border highlight
 * - Enter: Save and exit edit mode
 * - Esc: Cancel and revert changes
 * - Blur: Save if valid, otherwise stay in edit mode
 */
export function InlineEditCell({
  value,
  onChange,
  onSave,
  onCancel,
  type = 'text',
  validation,
  min,
  max,
  precision,
  className,
  disabled = false,
}: InlineEditCellProps) {
  const [isEditing, setIsEditing] = useState(false);
  const [editValue, setEditValue] = useState(value.toString());
  const [error, setError] = useState(false);
  const inputRef = useRef<HTMLInputElement>(null);

  // Sync editValue when value changes externally
  useEffect(() => {
    if (!isEditing) {
      setEditValue(value.toString());
    }
  }, [value, isEditing]);

  // Focus input when entering edit mode
  useEffect(() => {
    if (isEditing && inputRef.current) {
      inputRef.current.focus();
      inputRef.current.select();
    }
  }, [isEditing]);

  // Validate current edit value
  const isValid = useCallback(
    (val: string): boolean => {
      // Empty check
      if (val.trim() === '') return false;

      // Type-specific validation
      if (type === 'number') {
        const num = parseFloat(val);
        if (isNaN(num)) return false;
        if (min !== undefined && num < min) return false;
        if (max !== undefined && num > max) return false;
      }

      // Custom validation
      if (validation) {
        const testValue = type === 'number' ? parseFloat(val) : val;
        if (isNaN(testValue as number) && type === 'number') return false;
        return validation(testValue);
      }

      return true;
    },
    [type, min, max, validation]
  );

  // Enter edit mode
  const enterEditMode = useCallback(() => {
    if (disabled) return;
    setIsEditing(true);
    setError(false);
  }, [disabled]);

  // Exit edit mode and save
  const exitEditMode = useCallback(
    (save: boolean) => {
      if (!isEditing) return;

      if (save) {
        if (isValid(editValue)) {
          const finalValue = type === 'number' ? parseFloat(editValue) : editValue;

          // Format number with precision
          if (type === 'number' && precision !== undefined) {
            const formatted = parseFloat(editValue).toFixed(precision);
            setEditValue(formatted);
            onChange(parseFloat(formatted));
            onSave(parseFloat(formatted));
          } else {
            onChange(finalValue);
            onSave(finalValue);
          }

          setIsEditing(false);
          setError(false);
        } else {
          setError(true);
          // Stay in edit mode on validation error
        }
      } else {
        // Cancel - revert to original value
        setEditValue(value.toString());
        setIsEditing(false);
        setError(false);
        onCancel?.();
      }
    },
    [isEditing, editValue, value, type, precision, isValid, onChange, onSave, onCancel]
  );

  // Handle keyboard events
  const handleKeyDown = useCallback(
    (e: KeyboardEvent<HTMLInputElement>) => {
      if (e.key === 'Enter') {
        e.preventDefault();
        exitEditMode(true);
      } else if (e.key === 'Escape') {
        e.preventDefault();
        exitEditMode(false);
      }
    },
    [exitEditMode]
  );

  // Handle input change
  const handleChange = useCallback(
    (newValue: string) => {
      setEditValue(newValue);
      setError(!isValid(newValue));
    },
    [isValid]
  );

  // Handle blur - save if valid
  const handleBlur = useCallback(() => {
    exitEditMode(true);
  }, [exitEditMode]);

  // View mode
  if (!isEditing) {
    return (
      <div
        onClick={enterEditMode}
        className={cn(
          'min-h-[28px] px-2 py-1 rounded cursor-pointer transition-colors',
          'hover:bg-neutral-800/50',
          'font-mono text-sm text-neutral-100',
          disabled && 'cursor-not-allowed opacity-50',
          className
        )}
        style={{
          fontSize: typography.fontSize.sm,
          fontFamily: typography.fontFamily.mono,
          transition: `background-color ${animation.duration.fast} ${animation.easing.easeOut}`,
        }}
        role="button"
        tabIndex={disabled ? -1 : 0}
        onKeyDown={(e) => {
          if (e.key === 'Enter' || e.key === ' ') {
            e.preventDefault();
            enterEditMode();
          }
        }}
      >
        {value}
      </div>
    );
  }

  // Edit mode
  return (
    <div className={cn('relative', className)}>
      <input
        ref={inputRef}
        type={type === 'number' ? 'text' : 'text'}
        inputMode={type === 'number' ? 'decimal' : 'text'}
        value={editValue}
        onChange={(e) => handleChange(e.target.value)}
        onBlur={handleBlur}
        onKeyDown={handleKeyDown}
        className={cn(
          // Base styles
          'w-full px-2 py-1 rounded transition-all',
          'font-mono text-sm',
          'outline-none',
          'border-2',

          // Text alignment
          type === 'number' ? 'text-right' : 'text-left',

          // States - Valid
          !error && 'bg-neutral-800 border-emerald-500 text-neutral-100',
          !error && 'focus:ring-2 focus:ring-emerald-500/50',

          // States - Error
          error && 'bg-neutral-800 border-red-500 text-neutral-100',
          error && 'focus:ring-2 focus:ring-red-500/50'
        )}
        style={{
          fontSize: typography.fontSize.sm,
          fontFamily: typography.fontFamily.mono,
          borderRadius: borderRadius.DEFAULT,
          transition: `all ${animation.duration.fast} ${animation.easing.easeOut}`,
          // Ensure input takes full cell width
          minHeight: spacing.row.normal,
        }}
        aria-invalid={error}
      />

      {/* Error indicator */}
      {error && (
        <div
          className="absolute -bottom-1 -right-1 w-2 h-2 rounded-full bg-red-500 ring-2 ring-neutral-900"
          title="Invalid value"
        />
      )}
    </div>
  );
}

InlineEditCell.displayName = 'InlineEditCell';

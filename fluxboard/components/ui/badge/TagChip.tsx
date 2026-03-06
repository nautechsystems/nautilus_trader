/**
 * TagChip Component
 *
 * Removable filter chip with optional close button.
 * Similar to Badge but with interactive remove functionality.
 */

import React from 'react';
import { X } from 'lucide-react';
import { cn } from '@/lib/utils';
import { colors } from '@/lib/tokens';
import type { BadgeVariant, BadgeSize } from './Badge';

export interface TagChipProps {
  /**
   * Label text to display
   */
  label: string;

  /**
   * Optional callback when remove button clicked
   */
  onRemove?: () => void;

  /**
   * Visual variant determining color scheme
   */
  variant?: BadgeVariant;

  /**
   * Size variant affecting padding and text size
   */
  size?: BadgeSize;

  /**
   * Additional CSS classes
   */
  className?: string;

  /**
   * Optional aria-label for accessibility
   */
  'aria-label'?: string;
}

/**
 * Get variant-specific classes
 */
function getVariantClasses(variant: BadgeVariant): string {
  const variantMap: Record<BadgeVariant, string> = {
    success: cn(
      'bg-[rgba(15,143,107,0.14)] text-[rgb(47,180,138)]',
      'border border-[rgba(15,143,107,0.28)]',
      'ring-1 ring-[rgba(15,143,107,0.18)]'
    ),
    danger: cn(
      'bg-[rgba(224,75,73,0.14)] text-[rgb(240,112,112)]',
      'border border-[rgba(224,75,73,0.28)]',
      'ring-1 ring-[rgba(224,75,73,0.20)]'
    ),
    warning: cn(
      'bg-[rgba(201,154,46,0.16)] text-[rgb(221,181,80)]',
      'border border-[rgba(201,154,46,0.30)]',
      'ring-1 ring-[rgba(201,154,46,0.22)]'
    ),
    info: cn(
      'bg-[rgba(76,122,214,0.14)] text-[rgb(111,146,222)]',
      'border border-[rgba(76,122,214,0.26)]',
      'ring-1 ring-[rgba(76,122,214,0.18)]'
    ),
    neutral: cn(
      'bg-[rgba(44,47,55,0.9)] text-[rgb(156,161,171)]',
      'border border-[rgba(43,47,55,0.9)]',
      'ring-1 ring-[rgba(54,59,68,0.35)]'
    ),
  };

  return variantMap[variant];
}

/**
 * Get size-specific classes
 */
function getSizeClasses(size: BadgeSize): string {
  const sizeMap: Record<BadgeSize, string> = {
    xs: 'text-2xs px-1.5 py-0.5 gap-1',
    sm: 'text-xs px-2 py-1 gap-1.5',
    md: 'text-sm px-2.5 py-1 gap-1.5',
  };

  return sizeMap[size];
}

/**
 * Get remove button size classes
 */
function getRemoveButtonSize(size: BadgeSize): string {
  const sizeMap: Record<BadgeSize, string> = {
    xs: 'w-2.5 h-2.5',  // ~10px
    sm: 'w-3 h-3',      // ~12px
    md: 'w-3.5 h-3.5',  // ~14px
  };

  return sizeMap[size];
}

/**
 * TagChip component
 *
 * @example
 * <TagChip label="Active" variant="success" />
 * <TagChip label="Error" variant="danger" onRemove={() => console.log('removed')} />
 * <TagChip label="Filter: USD" variant="neutral" size="md" onRemove={handleRemove} />
 */
export default function TagChip({
  label,
  onRemove,
  variant = 'neutral',
  size = 'sm',
  className,
  'aria-label': ariaLabel,
}: TagChipProps) {
  return (
    <span
      className={cn(
        // Base styles
        'inline-flex items-center justify-center',
        'rounded-md',
        'font-medium tracking-tight',
        'uppercase',
        'whitespace-nowrap',

        // Variant colors
        getVariantClasses(variant),

        // Size
        getSizeClasses(size),

        // Custom classes
        className
      )}
      aria-label={ariaLabel || label}
    >
      {/* Label */}
      <span>{label}</span>

      {/* Remove button (if onRemove provided) */}
      {onRemove && (
        <button
          type="button"
          onClick={onRemove}
          className={cn(
            // Base styles
            'inline-flex items-center justify-center',
            'rounded-sm',
            'transition-opacity',

            // Hover state
            'hover:opacity-70',

            // Focus state
            'focus:outline-none focus:ring-1 focus:ring-current',

            // Size
            getRemoveButtonSize(size)
          )}
          aria-label={`Remove ${label}`}
        >
          <X className="w-full h-full" />
        </button>
      )}
    </span>
  );
}

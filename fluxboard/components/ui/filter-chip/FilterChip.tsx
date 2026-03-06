/**
 * FilterChip Component
 *
 * Compact, toggleable filter chip component (~24px height).
 * Designed for multi-select filtering with optional count badges.
 * Supports keyboard navigation and accessibility.
 *
 * @example
 * ```tsx
 * <FilterChip
 *   label="Loss Only"
 *   selected={filter === 'loss'}
 *   count={12}
 *   variant="danger"
 *   onClick={() => setFilter('loss')}
 * />
 * ```
 */

import * as React from 'react';
import { cva, type VariantProps } from 'class-variance-authority';
import { cn } from '@/lib/utils';

// =============================================================================
// VARIANTS
// =============================================================================

const filterChipVariants = cva(
  // Base styles (applied to all variants)
  cn(
    'inline-flex items-center justify-center',
    'rounded-md',
    'font-semibold tracking-tight',
    'transition-all duration-150',
    'border border-border text-text-muted bg-bg-surface/70',
    'cursor-pointer',
    'focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-accent focus-visible:ring-offset-0',
    'disabled:pointer-events-none disabled:opacity-50',
    // Compact sizing: ~24-26px height
    'h-7 px-2 py-[2px] text-[11px] gap-1.5'
  ),
  {
    variants: {
      variant: {
        default: cn(
          'hover:border-border-hover hover:text-text-primary hover:bg-bg-hover',
          'data-[selected=true]:text-text-primary data-[selected=true]:border-border-hover data-[selected=true]:bg-bg-surface-alt data-[selected=true]:shadow-[0_0_0_1px_rgba(255,255,255,0.06)]'
        ),
        danger: cn(
          'hover:border-border-hover hover:text-text-primary hover:bg-bg-hover',
          'data-[selected=true]:text-danger-light data-[selected=true]:border-border-hover data-[selected=true]:bg-bg-surface-alt'
        ),
        warning: cn(
          'hover:border-border-hover hover:text-text-primary hover:bg-bg-hover',
          'data-[selected=true]:text-warning-light data-[selected=true]:border-border-hover data-[selected=true]:bg-bg-surface-alt'
        ),
        success: cn(
          'hover:border-border-hover hover:text-text-primary hover:bg-bg-hover',
          'data-[selected=true]:text-success-light data-[selected=true]:border-border-hover data-[selected=true]:bg-bg-surface-alt'
        ),
      },
    },
    defaultVariants: {
      variant: 'default',
    },
  }
);

// =============================================================================
// TYPES
// =============================================================================

export interface FilterChipProps
  extends Omit<React.ButtonHTMLAttributes<HTMLButtonElement>, 'children'>,
    VariantProps<typeof filterChipVariants> {
  /**
   * Filter label text
   */
  label: string;

  /**
   * Whether the filter is currently selected/active
   */
  selected?: boolean;

  /**
   * Optional count to display in badge (e.g., "Loss only (12)")
   */
  count?: number;

  /**
   * Click handler
   */
  onClick?: () => void;

  /**
   * Additional className for custom styling
   */
  className?: string;
}

// =============================================================================
// COMPONENT
// =============================================================================

const FilterChip = React.forwardRef<HTMLButtonElement, FilterChipProps>(
  (
    {
      label,
      selected = false,
      count,
      variant = 'default',
      onClick,
      className,
      disabled,
      ...props
    },
    ref
  ) => {
    // Handle keyboard navigation
    const handleKeyDown = (event: React.KeyboardEvent<HTMLButtonElement>) => {
      if (disabled) {
        event.preventDefault();
        return;
      }

      // Standard button activation keys
      if (event.key === 'Enter' || event.key === ' ') {
        event.preventDefault();
        onClick?.();
      }

      props.onKeyDown?.(event);
    };

    // Build aria-label with count
    const ariaLabel = count !== undefined
      ? `${label} (${count}${selected ? ', selected' : ''})`
      : `${label}${selected ? ', selected' : ''}`;

    return (
      <button
        ref={ref}
        type="button"
        role="checkbox"
        aria-checked={selected}
        aria-label={ariaLabel}
        data-selected={selected}
        className={cn(filterChipVariants({ variant }), className)}
        onClick={onClick}
        onKeyDown={handleKeyDown}
        disabled={disabled}
        {...props}
      >
        <span className="whitespace-nowrap">
          {label}
          {count !== undefined && count > 0 && (
            <span
              className={cn(
                'ml-1 text-2xs font-semibold',
                selected ? 'opacity-90' : 'opacity-60'
              )}
            >
              ({count})
            </span>
          )}
        </span>
      </button>
    );
  }
);

FilterChip.displayName = 'FilterChip';

export { FilterChip, filterChipVariants };

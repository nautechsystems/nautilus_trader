/**
 * IconButton Component
 *
 * Icon-only button variant with square aspect ratio and tooltip support.
 * Requires aria-label for accessibility compliance.
 *
 * @example
 * ```tsx
 * <IconButton
 *   variant="primary"
 *   size="md"
 *   aria-label="Close dialog"
 *   onClick={handleClose}
 * >
 *   <XIcon />
 * </IconButton>
 *
 * <IconButton
 *   variant="danger"
 *   size="sm"
 *   aria-label="Delete item"
 *   loading
 * >
 *   <TrashIcon />
 * </IconButton>
 * ```
 */

import * as React from 'react';
import { cva, type VariantProps } from 'class-variance-authority';
import { cn } from '@/lib/utils';
import { colors } from '@/lib/tokens';
import { useDensityMode } from '@/hooks/useMobileLayout';

// =============================================================================
// VARIANTS
// =============================================================================

const iconButtonVariants = cva(
  // Base styles - square aspect ratio, centered content
  'inline-flex items-center justify-center rounded-[3px] font-semibold transition-colors duration-150 focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-offset-0 disabled:pointer-events-none disabled:opacity-60 shrink-0',
  {
    variants: {
      variant: {
        default: 'bg-bg-hover text-text-primary border border-border hover:bg-bg-active hover:border-border-hover focus-visible:ring-border-focus',
        destructive: 'bg-danger text-bg-base border border-danger-dark hover:bg-danger-dark focus-visible:ring-danger',
        outline: 'bg-transparent text-text-primary border border-border hover:border-border-hover hover:bg-bg-hover focus-visible:ring-border-focus',
        secondary: 'bg-bg-surface text-text-secondary border border-border hover:text-text-primary hover:border-border-hover hover:bg-bg-hover focus-visible:ring-border-focus',
        ghost: 'bg-transparent text-text-secondary hover:text-text-primary hover:bg-bg-hover border border-transparent focus-visible:ring-border-focus',
        link: 'text-accent hover:text-accent-hover underline underline-offset-4 border border-transparent focus-visible:ring-border-focus',
        success: 'bg-success text-bg-base border border-success-dark hover:bg-success-dark focus-visible:ring-success',
        warning: 'bg-warning text-bg-base border border-warning-dark hover:bg-warning-dark focus-visible:ring-warning',
      },
      size: {
        xs: 'h-7 w-7 text-[11px]', // 20px square
        sm: 'h-8 w-8 text-[12px]', // 24px square
        md: 'h-9 w-9 text-[13px]', // 32px square
        lg: 'h-10 w-10 text-[14px]', // 40px square
      },
    },
    defaultVariants: {
      variant: 'default',
      size: 'md',
    },
  }
);

type IconButtonSize = NonNullable<VariantProps<typeof iconButtonVariants>['size']>;

const mobileIconButtonOverrides: Record<IconButtonSize, string> = {
  xs: '!h-9 !w-9',
  sm: '!h-9 !w-9',
  md: '!h-10 !w-10',
  lg: '!h-11 !w-11',
};

// =============================================================================
// SPINNER COMPONENT
// =============================================================================

/**
 * Loading spinner for icon button loading state
 */
const Spinner = ({ className }: { className?: string }) => (
  <svg
    className={cn('animate-spin', className)}
    xmlns="http://www.w3.org/2000/svg"
    fill="none"
    viewBox="0 0 24 24"
    aria-hidden="true"
  >
    <circle
      className="opacity-25"
      cx="12"
      cy="12"
      r="10"
      stroke="currentColor"
      strokeWidth="4"
    />
    <path
      className="opacity-75"
      fill="currentColor"
      d="M4 12a8 8 0 018-8V0C5.373 0 0 5.373 0 12h4zm2 5.291A7.962 7.962 0 014 12H0c0 3.042 1.135 5.824 3 7.938l3-2.647z"
    />
  </svg>
);

// =============================================================================
// ICON BUTTON COMPONENT
// =============================================================================

export interface IconButtonProps
  extends Omit<React.ButtonHTMLAttributes<HTMLButtonElement>, 'children'>,
    VariantProps<typeof iconButtonVariants> {
  /**
   * Icon content (single ReactNode - typically an icon component)
   */
  children: React.ReactNode;

  /**
   * Loading state - shows spinner and disables interaction
   */
  loading?: boolean;

  /**
   * Additional className for custom styling
   */
  className?: string;

  /**
   * Required aria-label for accessibility (icon-only buttons must be labeled)
   */
  'aria-label': string;

  /**
   * Density override; defaults to layout density
   */
  density?: 'mobile' | 'desktop';
}

const IconButton = React.forwardRef<HTMLButtonElement, IconButtonProps>(
  (
    {
      className,
      variant,
      size,
      loading = false,
      disabled,
      children,
      onClick,
      type = 'button',
      'aria-label': ariaLabel,
      density,
      ...props
    },
    ref
  ) => {
    const resolvedDensity = useDensityMode(density);
    const resolvedSize = size ?? 'md';
    const densityClassName =
      resolvedDensity === 'mobile' ? mobileIconButtonOverrides[resolvedSize] : undefined;
    // Compute spinner size based on button size
    const spinnerSize = {
      xs: 'h-3 w-3',
      sm: 'h-3.5 w-3.5',
      md: 'h-4 w-4',
      lg: 'h-5 w-5',
    }[size ?? 'md'];

    // Handle click - prevent if loading or disabled
    const handleClick = (event: React.MouseEvent<HTMLButtonElement>) => {
      if (loading || disabled) {
        event.preventDefault();
        return;
      }
      onClick?.(event);
    };

    // Handle keyboard activation
    const handleKeyDown = (event: React.KeyboardEvent<HTMLButtonElement>) => {
      if (loading || disabled) {
        event.preventDefault();
        return;
      }

      // Standard button activation keys
      if (event.key === 'Enter' || event.key === ' ') {
        event.preventDefault();
        handleClick(event as any);
      }

      props.onKeyDown?.(event);
    };

    return (
      <button
        ref={ref}
        type={type}
        className={cn(iconButtonVariants({ variant, size, className }), densityClassName)}
        disabled={disabled || loading}
        onClick={handleClick}
        onKeyDown={handleKeyDown}
        aria-label={ariaLabel}
        aria-disabled={disabled || loading}
        aria-busy={loading}
        {...props}
      >
        {loading ? <Spinner className={spinnerSize} /> : children}
      </button>
    );
  }
);

IconButton.displayName = 'IconButton';

export { IconButton, iconButtonVariants };

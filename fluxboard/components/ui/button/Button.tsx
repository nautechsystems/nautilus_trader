/**
 * Button Component
 *
 * Versatile button component with multiple variants, sizes, and states.
 * Built with class-variance-authority for type-safe variant management.
 *
 * @example
 * ```tsx
 * <Button variant="primary" size="md" onClick={handleClick}>
 *   Submit
 * </Button>
 *
 * <Button variant="danger" loading disabled>
 *   Deleting...
 * </Button>
 *
 * <Button variant="ghost" icon={<PlusIcon />}>
 *   Add Item
 * </Button>
 * ```
 */

import * as React from 'react';
import { cva, type VariantProps } from 'class-variance-authority';
import { cn } from '@/lib/utils';
import { colors, spacing } from '@/lib/tokens';
import { useDensityMode } from '@/hooks/useMobileLayout';

// =============================================================================
// VARIANTS
// =============================================================================

const buttonVariants = cva(
  // Base styles (applied to all variants)
  'inline-flex items-center justify-center rounded-[3px] font-semibold tracking-tight transition-colors duration-150 focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-offset-0 disabled:pointer-events-none disabled:opacity-60 whitespace-nowrap',
  {
    variants: {
      variant: {
        default: 'bg-accent text-bg-base border border-accent-muted hover:bg-accent-hover hover:border-accent focus-visible:ring-accent',
        destructive: 'bg-danger text-bg-base border border-danger-dark hover:bg-danger-dark focus-visible:ring-danger',
        outline: 'bg-transparent text-text-primary border border-border hover:border-border-hover hover:bg-bg-hover focus-visible:ring-border-focus',
        secondary: 'bg-bg-surface text-text-primary border border-border hover:border-border-hover hover:bg-bg-hover focus-visible:ring-border-focus',
        ghost: 'bg-transparent text-text-secondary hover:text-text-primary hover:bg-bg-hover border border-transparent focus-visible:ring-border-focus',
        link: 'text-accent hover:text-accent-hover underline underline-offset-4 border border-transparent focus-visible:ring-border-focus',
        success: 'bg-success text-bg-base border border-success-dark hover:bg-success-dark focus-visible:ring-success',
        warning: 'bg-warning text-bg-base border border-warning-dark hover:bg-warning-dark focus-visible:ring-warning',
      },
      size: {
        xs: 'h-7 px-3 text-[11px] gap-1.5',
        sm: 'h-8 px-3.5 text-[12px] gap-1.5',
        md: 'h-10 px-4 text-[13px] gap-2',
        lg: 'h-11 px-5 text-[14px] gap-2.5',
      },
    },
    defaultVariants: {
      variant: 'default',
      size: 'md',
    },
  }
);

type ButtonSize = NonNullable<VariantProps<typeof buttonVariants>['size']>;

const mobileDensityOverrides: Record<ButtonSize, string> = {
  xs: '!h-9 !px-3 text-[11px]',
  sm: '!h-9 !px-3 text-[12px]',
  md: '!h-10 !px-4 text-[13px]',
  lg: '!h-11 !px-5 text-[14px]',
};

// =============================================================================
// SPINNER COMPONENT
// =============================================================================

/**
 * Loading spinner for button loading state
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
// BUTTON COMPONENT
// =============================================================================

export interface ButtonProps
  extends React.ButtonHTMLAttributes<HTMLButtonElement>,
    VariantProps<typeof buttonVariants> {
  /**
   * Icon to display before children (ReactNode for flexibility)
   */
  icon?: React.ReactNode;

  /**
   * Loading state - shows spinner and disables interaction
   */
  loading?: boolean;

  /**
   * Additional className for custom styling
   */
  className?: string;

  /**
   * Density override; defaults to layout density
   */
  density?: 'mobile' | 'desktop';
}

const Button = React.forwardRef<HTMLButtonElement, ButtonProps>(
  (
    {
      className,
      variant,
      size,
      icon,
      loading = false,
      disabled,
      children,
      onClick,
      type = 'button',
      density,
      ...props
    },
    ref
  ) => {
    const resolvedDensity = useDensityMode(density);
    const resolvedSize = size ?? 'md';
    const densityClassName =
      resolvedDensity === 'mobile' ? mobileDensityOverrides[resolvedSize] : undefined;
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
        className={cn(buttonVariants({ variant, size, className }), densityClassName)}
        disabled={disabled || loading}
        onClick={handleClick}
        onKeyDown={handleKeyDown}
        aria-disabled={disabled || loading}
        aria-busy={loading}
        {...props}
      >
        {loading ? (
          <>
            <Spinner className={spinnerSize} />
            {children && <span>{children}</span>}
          </>
        ) : (
          <>
            {icon && <span className="inline-flex shrink-0">{icon}</span>}
            {children && <span>{children}</span>}
          </>
        )}
      </button>
    );
  }
);

Button.displayName = 'Button';

export { Button, buttonVariants };

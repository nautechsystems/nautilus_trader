/**
 * Dialog Component
 *
 * Modal dialog component built on Radix UI with Fluxboard density styling.
 * Provides accessible dialogs with focus trap, keyboard navigation, and backdrop overlay.
 *
 * @example
 * ```tsx
 * <Dialog isOpen={isOpen} onClose={handleClose} title="Confirm Action" size="md">
 *   <p>Are you sure you want to proceed?</p>
 *   <DialogFooter>
 *     <Button variant="ghost" onClick={handleClose}>Cancel</Button>
 *     <Button variant="danger" onClick={handleConfirm}>Confirm</Button>
 *   </DialogFooter>
 * </Dialog>
 * ```
 */

import * as React from 'react';
import * as DialogPrimitive from '@radix-ui/react-dialog';
import { X } from 'lucide-react';
import { cn } from '@/lib/utils';
import { colors, spacing, borderRadius, elevation } from '@/lib/tokens';
import { useMobileLayout } from '@/hooks/useMobileLayout';

// =============================================================================
// DIALOG COMPONENT
// =============================================================================

export interface DialogProps {
  /**
   * Whether the dialog is open
   */
  isOpen: boolean;

  /**
   * Callback when dialog should close (Esc key, backdrop click, close button)
   */
  onClose: () => void;

  /**
   * Dialog title (displayed in header)
   */
  title: string;

  /**
   * Dialog content
   */
  children: React.ReactNode;

  /**
   * Optional footer content (usually action buttons)
   */
  footer?: React.ReactNode;

  /**
   * Dialog size
   * @default "md"
   */
  size?: 'sm' | 'md' | 'lg' | 'xl';

  /**
   * Additional className for dialog content
   */
  className?: string;

  /**
   * Prevent closing when clicking backdrop
   * @default false
   */
  preventBackdropClose?: boolean;

  /**
   * Dialog variant
   * @default "modal"
   */
  variant?: 'modal' | 'sheet';
}

/**
 * Size to max-width mapping
 */
const sizeClasses = {
  sm: 'max-w-sm',   // 384px
  md: 'max-w-md',   // 448px
  lg: 'max-w-lg',   // 512px
  xl: 'max-w-xl',   // 576px
};

export const Dialog = React.forwardRef<HTMLDivElement, DialogProps>(
  (
    {
      isOpen,
      onClose,
      title,
      children,
      footer,
      size = 'md',
      className,
      preventBackdropClose = false,
      variant = 'modal',
    },
    ref
  ) => {
    const { isMobile } = useMobileLayout();
    const isSheet = variant === 'sheet' && isMobile;
    // Handle backdrop click
    const handlePointerDownOutside = (event: Event) => {
      if (preventBackdropClose) {
        event.preventDefault();
      }
    };

    // Handle Escape key
    const handleEscapeKeyDown = (event: KeyboardEvent) => {
      if (!preventBackdropClose) {
        onClose();
      } else {
        event.preventDefault();
      }
    };

    return (
      <DialogPrimitive.Root open={isOpen} onOpenChange={(open) => !open && onClose()}>
        {/* Backdrop Overlay */}
        <DialogPrimitive.Overlay
          className={cn(
            'fixed inset-0 bg-black/60 backdrop-blur-sm',
            'data-[state=open]:animate-in data-[state=closed]:animate-out',
            'data-[state=closed]:fade-out-0 data-[state=open]:fade-in-0',
            'transition-all duration-200'
          )}
          style={{ zIndex: elevation.overlay }}
        />

        {/* Dialog Content */}
        <DialogPrimitive.Content
          ref={ref}
          className={cn(
            isSheet
              ? 'fixed inset-x-0 bottom-0 top-auto translate-x-0 translate-y-0'
              : 'fixed left-[50%] top-[50%] translate-x-[-50%] translate-y-[-50%]',
            'w-full',
            isSheet ? 'max-w-none' : sizeClasses[size],
            'max-h-[85vh] overflow-hidden',
            'flex flex-col',
            isSheet ? 'rounded-t-2xl rounded-b-none' : 'rounded-lg',
            'shadow-xl',
            'focus:outline-none',
            'data-[state=open]:animate-in data-[state=closed]:animate-out',
            isSheet
              ? 'data-[state=closed]:slide-out-to-bottom data-[state=open]:slide-in-from-bottom'
              : 'data-[state=closed]:fade-out-0 data-[state=open]:fade-in-0 data-[state=closed]:zoom-out-95 data-[state=open]:zoom-in-95 data-[state=closed]:slide-out-to-left-1/2 data-[state=closed]:slide-out-to-top-[48%] data-[state=open]:slide-in-from-left-1/2 data-[state=open]:slide-in-from-top-[48%]',
            'duration-200',
            className
          )}
          style={{
            backgroundColor: colors.bg.surface,
            border: `1px solid ${colors.border.DEFAULT}`,
            zIndex: elevation.modal,
          }}
          onPointerDownOutside={handlePointerDownOutside}
          onEscapeKeyDown={handleEscapeKeyDown}
        >
          {/* Header */}
          <div
            className="flex items-center justify-between px-4 py-3 border-b"
            style={{ borderColor: colors.border.DEFAULT }}
          >
            <DialogPrimitive.Title
              className="text-md font-semibold"
              style={{ color: colors.text.primary }}
            >
              {title}
            </DialogPrimitive.Title>

            {/* Close Button */}
            <DialogPrimitive.Close
              className={cn(
                'rounded-sm opacity-70 ring-offset-background transition-opacity',
                'hover:opacity-100',
                'focus:outline-none focus:ring-2 focus:ring-offset-2',
                'disabled:pointer-events-none',
                'h-5 w-5 inline-flex items-center justify-center'
              )}
              style={{
                color: colors.text.secondary,
                '--tw-ring-color': colors.border.focus,
                '--tw-ring-offset-color': colors.bg.surface,
              } as React.CSSProperties}
              onClick={onClose}
              aria-label="Close dialog"
            >
              <X className="h-4 w-4" />
            </DialogPrimitive.Close>
          </div>

          {/* Body */}
          <div
            className="flex-1 overflow-y-auto px-4 py-3 min-h-0"
            style={{ color: colors.text.secondary }}
          >
            {children}
          </div>

          {/* Footer (optional) */}
          {footer && (
            <div
              className="flex items-center justify-end gap-2 px-4 py-3 border-t"
              style={{ borderColor: colors.border.DEFAULT }}
            >
              {footer}
            </div>
          )}
        </DialogPrimitive.Content>
      </DialogPrimitive.Root>
    );
  }
);

Dialog.displayName = 'Dialog';

// =============================================================================
// DIALOG FOOTER HELPER
// =============================================================================

/**
 * Dialog footer container with consistent spacing
 */
export interface DialogFooterProps {
  children: React.ReactNode;
  className?: string;
}

export const DialogFooter: React.FC<DialogFooterProps> = ({ children, className }) => {
  return (
    <div className={cn('flex items-center justify-end gap-2', className)}>
      {children}
    </div>
  );
};

DialogFooter.displayName = 'DialogFooter';

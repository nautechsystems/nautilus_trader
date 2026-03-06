/**
 * Toast Component
 *
 * Toast notification system built on Sonner with Fluxboard density styling.
 * Provides accessible, auto-dismissing notifications for success, error, warning, and info.
 *
 * @example
 * ```tsx
 * // In your app root (App.tsx or layout):
 * import { Toaster } from '@/components/ui/toast/Toast';
 *
 * function App() {
 *   return (
 *     <>
 *       <Toaster />
 *       <YourApp />
 *     </>
 *   );
 * }
 *
 * // In your components:
 * import { toast } from '@/components/ui/toast/Toast';
 *
 * toast.success('Operation completed!');
 * toast.error('Something went wrong', { description: 'Please try again' });
 * toast.warning('Warning message');
 * toast.info('Info message');
 * ```
 */

import * as React from 'react';
import { Toaster as SonnerToaster, toast as sonnerToast, type ExternalToast } from 'sonner';
import { CheckCircle2, XCircle, AlertTriangle, Info } from 'lucide-react';
import { colors, elevation, borderRadius, animation } from '@/lib/tokens';

// =============================================================================
// TOASTER COMPONENT
// =============================================================================

export interface ToasterProps {
  /**
   * Position of toasts
   * @default "top-right"
   */
  position?: 'top-left' | 'top-center' | 'top-right' | 'bottom-left' | 'bottom-center' | 'bottom-right';

  /**
   * Maximum number of toasts to show at once
   * @default 3
   */
  visibleToasts?: number;

  /**
   * Default toast duration in milliseconds
   * @default 4000
   */
  duration?: number;

  /**
   * Whether to show close button
   * @default true
   */
  closeButton?: boolean;

  /**
   * Rich colors for semantic toasts
   * @default true
   */
  richColors?: boolean;

  /**
   * Expand toasts by default
   * @default false
   */
  expand?: boolean;
}

/**
 * Toaster component that renders the toast container
 * Place this once in your app root
 */
export const Toaster: React.FC<ToasterProps> = ({
  position = 'top-right',
  visibleToasts = 3,
  duration = 4000,
  closeButton = true,
  richColors = true,
  expand = false,
}) => {
  return (
    <SonnerToaster
      position={position}
      visibleToasts={visibleToasts}
      duration={duration}
      closeButton={closeButton}
      richColors={richColors}
      expand={expand}
      toastOptions={{
        style: {
          background: colors.bg.surface,
          border: `1px solid ${colors.border.DEFAULT}`,
          color: colors.text.secondary,
          borderRadius: borderRadius.DEFAULT,
          fontSize: '12px',
        },
        className: 'toast',
      }}
      style={{
        zIndex: elevation.toast,
      }}
    />
  );
};

Toaster.displayName = 'Toaster';

// =============================================================================
// TOAST API
// =============================================================================

export interface ToastOptions extends ExternalToast {
  /**
   * Toast description (secondary text)
   */
  description?: string;

  /**
   * Custom icon to display
   */
  icon?: React.ReactNode;

  /**
   * Custom action button
   */
  action?: {
    label: string;
    onClick: () => void;
  };

  /**
   * Duration in milliseconds (overrides default)
   */
  duration?: number;

  /**
   * Whether toast can be dismissed
   * @default true
   */
  dismissible?: boolean;
}

/**
 * Show a success toast
 */
function success(message: string, options?: ToastOptions) {
  return sonnerToast.success(message, {
    ...options,
    icon: options?.icon || <CheckCircle2 className="h-4 w-4" style={{ color: colors.semantic.success.DEFAULT }} />,
  });
}

/**
 * Show an error toast
 */
function error(message: string, options?: ToastOptions) {
  return sonnerToast.error(message, {
    ...options,
    icon: options?.icon || <XCircle className="h-4 w-4" style={{ color: colors.semantic.danger.DEFAULT }} />,
  });
}

/**
 * Show a warning toast
 */
function warning(message: string, options?: ToastOptions) {
  return sonnerToast.warning(message, {
    ...options,
    icon: options?.icon || <AlertTriangle className="h-4 w-4" style={{ color: colors.semantic.warning.DEFAULT }} />,
  });
}

/**
 * Show an info toast
 */
function info(message: string, options?: ToastOptions) {
  return sonnerToast.info(message, {
    ...options,
    icon: options?.icon || <Info className="h-4 w-4" style={{ color: colors.semantic.info.DEFAULT }} />,
  });
}

/**
 * Show a generic toast (no icon)
 */
function message(text: string, options?: ToastOptions) {
  return sonnerToast(text, options);
}

/**
 * Show a promise toast that updates based on promise state
 */
function promise<T>(
  promise: Promise<T>,
  messages: {
    loading: string;
    success: string | ((data: T) => string);
    error: string | ((error: any) => string);
  },
  options?: ToastOptions
) {
  return (sonnerToast.promise as any)(promise, messages, options);
}

/**
 * Dismiss a specific toast by ID
 */
function dismiss(toastId?: string | number) {
  return sonnerToast.dismiss(toastId);
}

/**
 * Custom toast with full control
 */
function custom(component: React.ReactNode, options?: ToastOptions) {
  return sonnerToast.custom(() => component as React.ReactElement, options);
}

/**
 * Loading toast (shows indefinitely until dismissed)
 */
function loading(message: string, options?: ToastOptions) {
  return sonnerToast.loading(message, options);
}

// =============================================================================
// TOAST API EXPORT
// =============================================================================

/**
 * Toast API for showing notifications
 *
 * @example
 * ```tsx
 * toast.success('Settings saved');
 * toast.error('Failed to save', { description: 'Please try again' });
 * toast.warning('Low balance');
 * toast.info('New feature available');
 *
 * // With action button
 * toast.success('File uploaded', {
 *   action: {
 *     label: 'View',
 *     onClick: () => console.log('View file'),
 *   },
 * });
 *
 * // Promise toast
 * toast.promise(
 *   saveData(),
 *   {
 *     loading: 'Saving...',
 *     success: 'Saved successfully',
 *     error: 'Failed to save',
 *   }
 * );
 * ```
 */
export const toast = {
  success,
  error,
  warning,
  info,
  message,
  promise,
  dismiss,
  custom,
  loading,
};

// =============================================================================
// RE-EXPORT TYPES
// =============================================================================

export type { ExternalToast } from 'sonner';

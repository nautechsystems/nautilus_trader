/**
 * Custom hook for copy-to-clipboard functionality with toast feedback
 */

import { useCallback } from 'react';
import { toast, type ToastOptions } from '@/components/ui/toast/Toast';

interface CopyToClipboardOptions {
  successMessage?: string;
  errorMessage?: string;
  /**
   * Whether to include a preview of the copied text in the toast description.
   * Defaults to `true`.
   */
  showPreview?: boolean;
  /** Optional toast overrides for success state. */
  successToastOptions?: ToastOptions;
  /** Optional toast overrides for error state. */
  errorToastOptions?: ToastOptions;
  /** Callback invoked after a successful copy action. */
  onSuccess?: () => void;
  /** Callback invoked when the copy action fails. */
  onError?: (error: Error) => void;
}

function fallbackCopyText(text: string) {
  if (typeof document === 'undefined' || !document.body) {
    throw new Error('Clipboard not supported in this environment');
  }

  const textarea = document.createElement('textarea');
  textarea.value = text;
  textarea.setAttribute('readonly', '');
  textarea.style.position = 'fixed';
  textarea.style.top = '-9999px';
  textarea.style.opacity = '0';

  document.body.appendChild(textarea);

  textarea.focus();
  textarea.select();

  const succeeded = typeof document.execCommand === 'function' && document.execCommand('copy');

  document.body.removeChild(textarea);

  if (!succeeded) {
    throw new Error('Fallback copy command was unsuccessful');
  }
}

export function useCopyToClipboard() {
  const copyToClipboard = useCallback(
    async (text: string, options: CopyToClipboardOptions = {}) => {
      const {
        successMessage = 'Copied to clipboard',
        errorMessage = 'Failed to copy',
        showPreview = true,
        successToastOptions,
        errorToastOptions,
        onSuccess,
        onError,
      } = options;

      try {
        const canUseNavigatorClipboard =
          typeof navigator !== 'undefined' &&
          !!navigator.clipboard &&
          typeof navigator.clipboard.writeText === 'function';

        if (canUseNavigatorClipboard) {
          await navigator.clipboard.writeText(text);
        } else {
          fallbackCopyText(text);
        }

        const successOptions: ToastOptions = {
          ...successToastOptions,
        };

        if (successOptions.description === undefined && showPreview) {
          successOptions.description = text.length > 50 ? `${text.slice(0, 50)}…` : text;
        }

        toast.success(successMessage, successOptions);
        onSuccess?.();
        return true;
      } catch (error) {
        console.error('Copy to clipboard failed:', error);

        const normalizedError = error instanceof Error ? error : new Error(String(error));

        const errorOptions: ToastOptions = {
          ...errorToastOptions,
        };

        toast.error(errorMessage, errorOptions);
        onError?.(normalizedError);
        return false;
      }
    },
    []
  );

  return copyToClipboard;
}

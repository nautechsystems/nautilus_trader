/**
 * SaveAllButton - Bulk save button with progress indicator
 *
 * Triggers save for all dirty strategies with bounded concurrency.
 * Shows progress during bulk save operation.
 */

import { memo } from 'react';

export interface SaveAllButtonProps {
  /** Number of strategies with unsaved changes */
  dirtyCount: number;
  /** Currently saving */
  isSaving: boolean;
  /** Has any validation errors */
  hasErrors: boolean;
  /** Save progress (null when not saving) */
  progress: { completed: number; failed: number; total: number } | null;
  /** Save handler */
  onSave: () => void;
}

export const SaveAllButton = memo(function SaveAllButton({
  dirtyCount,
  isSaving,
  hasErrors,
  progress,
  onSave,
}: SaveAllButtonProps) {
  return (
    <div className="flex items-center gap-2">
      <button
        onClick={onSave}
        disabled={dirtyCount === 0 || isSaving || hasErrors}
        className={`px-2.5 py-[6px] rounded text-xs font-semibold tracking-wide transition-colors ${
          dirtyCount > 0 && !isSaving && !hasErrors
            ? 'bg-emerald-600 text-neutral-900 hover:bg-emerald-500'
            : 'bg-neutral-700 text-neutral-400 cursor-not-allowed'
        }`}
      >
        Save All {dirtyCount > 0 && `(${dirtyCount})`}
      </button>

      {isSaving && progress && (
        <span className="text-sm text-neutral-400">
          Saving {progress.completed + progress.failed}/{progress.total}...
        </span>
      )}
    </div>
  );
});

SaveAllButton.displayName = 'SaveAllButton';

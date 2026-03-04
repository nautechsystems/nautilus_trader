/**
 * SaveButton - Individual strategy save button with dirty indicator
 *
 * Shows save button only when params are dirty and valid.
 * Displays loading state during save operation.
 */

import { memo } from 'react';

export interface SaveButtonProps {
  /** Strategy ID being saved */
  strategyId: string;
  /** Has unsaved changes */
  isDirty: boolean;
  /** Currently saving */
  isSaving: boolean;
  /** Has validation errors */
  hasError: boolean;
  /** Save handler */
  onSave: () => void;
}

export const SaveButton = memo(function SaveButton({
  isDirty,
  isSaving,
  hasError,
  onSave,
}: SaveButtonProps) {
  return (
    <button
      onClick={onSave}
      disabled={!isDirty || isSaving || hasError}
      className={`rounded-sm px-3 py-1 text-[12px] font-semibold transition-all duration-150 ${
        isDirty && !isSaving && !hasError
          ? 'bg-emerald-600/80 text-neutral-900 hover:bg-emerald-500'
          : 'bg-neutral-800/80 text-neutral-500 cursor-not-allowed'
      }`}
    >
      {isSaving ? '...' : 'Save'}
    </button>
  );
});

SaveButton.displayName = 'SaveButton';

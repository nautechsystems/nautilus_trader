/**
 * AutoModeToggle - Auto-refresh toggle with pause indicator
 *
 * Enables/disables auto-refresh with visual feedback when paused.
 * Shows pause reason (editing, unsaved changes).
 */

import { memo } from 'react';

export interface AutoModeToggleProps {
  /** Auto-refresh enabled */
  auto: boolean;
  /** Auto-refresh currently active (not paused) */
  isActive: boolean;
  /** Polling interval in milliseconds */
  intervalMs: number;
  /** Has input focus (pauses auto-refresh) */
  hasInputFocus: boolean;
  /** Has unsaved changes (pauses auto-refresh) */
  hasDirty: boolean;
  /** Toggle handler */
  onToggle: (enabled: boolean) => void;
}

export const AutoModeToggle = memo(function AutoModeToggle({
  auto,
  isActive,
  intervalMs,
  hasInputFocus,
  hasDirty,
  onToggle,
}: AutoModeToggleProps) {
  const intervalSeconds = (intervalMs / 1000).toFixed(0);
  const isPaused = auto && !isActive;
  const pauseReason = hasInputFocus ? '(editing)' : hasDirty ? '(unsaved)' : '';

  return (
    <div className="flex items-center gap-2">
      <label className="flex items-center gap-2 text-xs cursor-pointer">
        <input
          type="checkbox"
          checked={auto}
          onChange={(e) => onToggle(e.target.checked)}
          className="w-4 h-4"
        />
        <span className={`font-medium ${isPaused ? 'text-yellow-400' : 'text-neutral-400'}`}>
          Auto ({intervalSeconds}s)
        </span>
      </label>

      {isPaused && (
        <span className="text-xs text-yellow-400 px-2 py-1 bg-yellow-900/20 rounded border border-yellow-700/30">
          Paused {pauseReason}
        </span>
      )}
    </div>
  );
});

AutoModeToggle.displayName = 'AutoModeToggle';

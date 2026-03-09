/**
 * ParamsToolbar - Main toolbar for Params panel
 *
 * Contains: Save All, Refresh, Customize, Sort controls, Auto-refresh toggle.
 * Extracted from monolithic Params.tsx for better testability.
 */

import { memo } from 'react';
import { SaveAllButton } from './SaveAllButton';
import { AutoModeToggle } from './AutoModeToggle';

export interface ParamsToolbarProps {
  // Save All
  dirtyCount: number;
  isSaving: boolean;
  hasErrors: boolean;
  saveProgress: { completed: number; failed: number; total: number } | null;
  onSaveAll: () => void;

  // View controls
  isCompactView: boolean;
  onToggleViewMode: () => void;

  // Refresh
  loading: boolean;
  refreshing: boolean;
  onRefresh: () => void;

  // Customize columns
  customizeColumns: boolean;
  onToggleCustomize: () => void;
  canResetColumns: boolean;
  onResetColumns: () => void;

  // Sort
  isSortActive: boolean;
  onClearSort: () => void;

  // Selection
  selectedCount: number;
  onClearSelection: () => void;

  // Auto-refresh
  auto: boolean;
  pollingEnabled: boolean;
  intervalMs: number;
  hasInputFocus: boolean;
  hasDirtyParams: boolean;
  onToggleAuto: (enabled: boolean) => void;

  // Stats
  strategyCount: number;
}

export const ParamsToolbar = memo(function ParamsToolbar({
  dirtyCount,
  isSaving,
  hasErrors,
  saveProgress,
  onSaveAll,
  isCompactView,
  onToggleViewMode,
  loading,
  refreshing,
  onRefresh,
  customizeColumns,
  onToggleCustomize,
  canResetColumns,
  onResetColumns,
  isSortActive,
  onClearSort,
  selectedCount,
  onClearSelection,
  auto,
  pollingEnabled,
  intervalMs,
  hasInputFocus,
  hasDirtyParams,
  onToggleAuto,
  strategyCount,
}: ParamsToolbarProps) {
  return (
    <div className="sticky top-0 z-20 bg-[#12161d] border-b border-neutral-800 px-3 py-1.5 flex items-center gap-2">
      <SaveAllButton
        dirtyCount={dirtyCount}
        isSaving={isSaving}
        hasErrors={hasErrors}
        progress={saveProgress}
        onSave={onSaveAll}
      />

      {loading && <div className="text-xs text-yellow-400 animate-pulse">Loading...</div>}

      <div className="flex-1" />

      <button
        onClick={onToggleViewMode}
        className={`px-2.5 py-[6px] rounded text-xs transition-colors ${
          isCompactView
            ? 'bg-emerald-600/80 text-neutral-900 hover:bg-emerald-500'
            : 'bg-neutral-700 text-neutral-300 hover:bg-neutral-600'
        }`}
      >
        {isCompactView ? 'Full View' : 'Compact View'}
      </button>

      <button
        onClick={onRefresh}
        disabled={loading || refreshing}
        className="px-2.5 py-[6px] rounded text-xs transition-colors bg-neutral-700 hover:bg-neutral-600 disabled:opacity-50 disabled:cursor-not-allowed"
      >
        Refresh
      </button>

      {refreshing && (
        <span className="text-xs text-yellow-400 animate-pulse" role="status" aria-live="polite">
          Loading...
        </span>
      )}

      <button
        onClick={onToggleCustomize}
        className={`px-2.5 py-[6px] rounded text-xs transition-colors ${
          customizeColumns
            ? 'bg-neutral-600 text-neutral-100 hover:bg-neutral-500'
            : 'bg-neutral-700 text-neutral-300 hover:bg-neutral-600'
        }`}
        title="Enable column drag and visibility controls"
      >
        {customizeColumns ? 'Done' : 'Customize'}
      </button>

      {customizeColumns && (
        <button
          onClick={onResetColumns}
          disabled={!canResetColumns}
          className="px-2.5 py-[6px] rounded text-xs transition-colors bg-neutral-700 hover:bg-neutral-600 disabled:opacity-50 disabled:cursor-not-allowed"
          title="Restore default column order"
        >
          Reset Columns
        </button>
      )}

      <button
        onClick={onClearSort}
        disabled={!isSortActive}
        className="px-2.5 py-[6px] bg-neutral-700 hover:bg-neutral-600 disabled:opacity-50 disabled:cursor-not-allowed rounded text-xs transition-colors"
        title="Remove active sorting"
      >
        Clear Sort
      </button>

      {selectedCount > 0 && (
        <div className="flex items-center gap-1.5 rounded border border-emerald-500/40 bg-emerald-500/10 px-2 py-0.5 text-[11px] text-emerald-200">
          <span>{selectedCount} selected</span>
          <button
            type="button"
            onClick={onClearSelection}
            className="font-semibold text-emerald-300 underline-offset-2 hover:text-emerald-100 hover:underline"
          >
            Clear
          </button>
        </div>
      )}

      <AutoModeToggle
        auto={auto}
        isActive={pollingEnabled}
        intervalMs={intervalMs}
        hasInputFocus={hasInputFocus}
        hasDirty={hasDirtyParams}
        onToggle={onToggleAuto}
      />

      <span className="text-sm text-neutral-400">{strategyCount} strategies</span>
    </div>
  );
});

ParamsToolbar.displayName = 'ParamsToolbar';

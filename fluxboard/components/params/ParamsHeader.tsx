/**
 * ParamsHeader - Sticky toolbar for Params panel
 *
 * Provides consistent header UI with controls for:
 * - Save All (with dirty count)
 * - Compact/Relaxed view toggle
 * - Column customization
 * - Clear Sort
 * - Auto-refresh toggle
 * - Freshness indicator
 */

import { colors, typography, spacing } from '../../lib/tokens';
import type { ParamsSortState } from '../../stores';

export interface ParamsHeaderProps {
  // Save state
  hasDirtyParams: boolean;
  dirtyCount: number;
  isSaving: boolean;
  hasErrors: boolean;
  onSaveAll: () => void;
  onRevertAll?: () => void;

  // Save progress (optional)
  saveProgress?: {
    completed: number;
    failed: number;
    total: number;
  };

  // View controls
  advancedMode: boolean;
  onToggleAdvanced: () => void;

  // Column customization
  customizeMode: boolean;
  onToggleCustomize: () => void;
  onResetColumns?: () => void;
  canResetColumns?: boolean;

  // Sort controls
  sortState: ParamsSortState;
  onClearSort: () => void;

  // Auto-refresh
  autoRefresh: boolean;
  onToggleAuto: (next: boolean) => void;
  autoRefreshActive: boolean;
  autoRefreshPauseLabel?: string;
  autoRefreshIntervalSec?: number;

  // Freshness
  lastFetchedAt: number | null;
  isStale: boolean;

  // Selection actions
  selectedCount: number;
  selectedDirtyCount: number;
  isSaveSelectedInProgress: boolean;
  onClearSelection: () => void;
  onSaveSelected: () => void;

  // Loading states
  loading?: boolean;
  refreshing?: boolean;

  // Manual refresh
  onRefresh?: () => void;
}

/**
 * Format relative time string
 */
function formatRelativeTime(timestamp: number | null): string {
  if (!timestamp) return 'never';
  const seconds = Math.floor((Date.now() - timestamp) / 1000);
  if (seconds < 5) return 'just now';
  if (seconds < 60) return `${seconds}s ago`;
  const minutes = Math.floor(seconds / 60);
  if (minutes < 60) return `${minutes}m ago`;
  const hours = Math.floor(minutes / 60);
  return `${hours}h ago`;
}

/**
 * Format absolute time for tooltip
 */
function formatAbsoluteTime(timestamp: number | null): string {
  if (!timestamp) return 'Never updated';
  const date = new Date(timestamp);
  return date.toLocaleString();
}

export function ParamsHeader({
  hasDirtyParams,
  dirtyCount,
  isSaving,
  hasErrors,
  onSaveAll,
  onRevertAll,
  saveProgress,
  advancedMode,
  onToggleAdvanced,
  customizeMode,
  onToggleCustomize,
  onResetColumns,
  canResetColumns = false,
  sortState,
  onClearSort,
  autoRefresh,
  onToggleAuto,
  autoRefreshActive,
  autoRefreshPauseLabel,
  autoRefreshIntervalSec,
  lastFetchedAt,
  isStale,
  selectedCount,
  selectedDirtyCount,
  isSaveSelectedInProgress,
  onClearSelection,
  onSaveSelected,
  loading,
  refreshing,
  onRefresh,
}: ParamsHeaderProps) {
  const isSortActive = Boolean(sortState.key);
  const relativeTime = formatRelativeTime(lastFetchedAt);
  const absoluteTime = formatAbsoluteTime(lastFetchedAt);
  const autoToggleTitle = !autoRefresh
    ? 'Enable auto-refresh (10 second interval)'
    : autoRefreshActive
      ? 'Auto-refresh parameters every 10 seconds'
      : autoRefreshPauseLabel ?? 'Auto-refresh paused';

  return (
    <div
      className="sticky top-0 z-30 flex items-center h-10 border-b backdrop-blur"
      style={{
        backgroundColor: `${colors.bg.surface}f2`, // 95% opacity
        borderBottomColor: `${colors.border.DEFAULT}4d`, // 30% opacity
        padding: `0 ${spacing.gap.md}`,
        gap: spacing.gap.xs,
      }}
    >
      {/* Left Section - Primary Actions */}
      <div className="flex items-center" style={{ gap: spacing.gap.xs }}>
        {/* Save All Button */}
        <button
          onClick={onSaveAll}
          disabled={!hasDirtyParams || isSaving || hasErrors}
          className="h-6 px-2.5 rounded-md transition-colors text-[11px] font-semibold tracking-wide disabled:opacity-50 disabled:cursor-not-allowed"
          style={{
            backgroundColor: hasDirtyParams && !isSaving && !hasErrors ? colors.semantic.success.DEFAULT : colors.neutral[700],
            color: hasDirtyParams && !isSaving && !hasErrors ? colors.neutral[900] : colors.neutral[400],
          }}
          title={hasErrors ? 'Fix errors before saving' : 'Save all unsaved changes'}
          aria-label={`Save all changes${dirtyCount > 0 ? ` (${dirtyCount} unsaved)` : ''}`}
        >
          Save All {dirtyCount > 0 && `(${dirtyCount})`}
        </button>

        {onRevertAll && (
          <button
            onClick={onRevertAll}
            disabled={!hasDirtyParams}
            className="h-6 px-2.5 rounded-md border text-[11px] font-medium disabled:opacity-40 disabled:cursor-not-allowed"
            style={{
              borderColor: colors.semantic.warning.DEFAULT,
              color: hasDirtyParams ? colors.semantic.warning.light : colors.neutral[500],
            }}
            aria-label="Revert all dirty rows"
          >
            Revert All
          </button>
        )}

        {/* Selection Actions */}
        {selectedCount > 0 && (
          <div
            className="flex items-center gap-2 rounded-md px-2 py-1 h-6"
            style={{
              backgroundColor: `${colors.semantic.success.DEFAULT}1a`,
              border: `1px solid ${colors.semantic.success.DEFAULT}55`,
            }}
          >
            <span
              className="text-[11px] font-medium"
              style={{ color: colors.semantic.success.light }}
            >
              {`${selectedCount} ${selectedCount === 1 ? 'strategy' : 'strategies'} selected`}
            </span>
            <button
              type="button"
              onClick={onClearSelection}
              className="text-[11px] font-semibold hover:underline"
              style={{
                color: colors.semantic.success.light,
                textUnderlineOffset: '2px',
              }}
            >
              Clear
            </button>
            <button
              type="button"
              onClick={onSaveSelected}
              disabled={isSaveSelectedInProgress || selectedDirtyCount === 0}
              className="h-6 px-2 rounded-md transition-colors text-[11px] font-semibold disabled:opacity-50 disabled:cursor-not-allowed"
              style={{
                color: colors.bg.base,
                backgroundColor: colors.semantic.success.DEFAULT,
              }}
            >
              Save Selected
            </button>
            {selectedDirtyCount > 0 && (
              <span style={{ fontSize: typography.fontSize.xs, color: colors.text.muted }}>
                {selectedDirtyCount} unsaved
              </span>
            )}
          </div>
        )}

        {/* Save Progress */}
        {isSaving && saveProgress && (
          <span style={{ fontSize: typography.fontSize.xs, color: colors.text.muted }}>
            {saveProgress.completed + saveProgress.failed}/{saveProgress.total}
          </span>
        )}

        {/* Loading Indicator */}
        {(loading || refreshing) && (
          <div
            className="animate-pulse"
            style={{ fontSize: typography.fontSize.xs, color: colors.semantic.warning.DEFAULT }}
            role="status"
            aria-live="polite"
          >
            Loading...
          </div>
        )}

        {onRefresh && (
          <button
            onClick={onRefresh}
            disabled={refreshing}
            className="h-6 px-2 rounded-md transition-colors text-[11px] font-semibold disabled:opacity-50 disabled:cursor-not-allowed"
            style={{ backgroundColor: colors.neutral[700], color: colors.neutral[200] }}
            aria-label="Refresh parameters"
          >
            Refresh
          </button>
        )}
      </div>

      {/* Spacer */}
      <div className="flex-1" />

      {/* Right Section - View Controls */}
      <div className="flex items-center" style={{ gap: spacing.gap.xs }}>
        {/* Advanced Params Toggle */}
        <button
          onClick={onToggleAdvanced}
          className="h-6 px-2.5 rounded-md transition-colors text-[11px] font-medium"
          style={{
            backgroundColor: advancedMode ? colors.neutral[600] : colors.neutral[700],
            color: advancedMode ? colors.neutral[100] : colors.neutral[300],
          }}
          title={advancedMode ? 'Hide advanced parameters' : 'Show advanced parameters'}
          aria-label="Advanced Params"
          aria-pressed={advancedMode}
        >
          Advanced Params
        </button>

        {/* Customize Columns Toggle */}
        <button
          onClick={onToggleCustomize}
          className="h-6 px-2.5 rounded-md transition-colors text-[11px] font-medium"
          style={{
            backgroundColor: customizeMode ? colors.neutral[600] : colors.neutral[700],
            color: customizeMode ? colors.neutral[100] : colors.neutral[300],
          }}
          title="Enable column drag and visibility controls"
          aria-label="Customize columns"
          aria-pressed={customizeMode}
        >
          {customizeMode ? 'Done' : 'Customize'}
        </button>

        {/* Reset Columns (only visible in customize mode) */}
        {customizeMode && onResetColumns && (
          <button
            onClick={onResetColumns}
            disabled={!canResetColumns}
            className="h-6 px-2.5 rounded-md transition-colors text-[11px] font-medium disabled:opacity-50 disabled:cursor-not-allowed"
            style={{
              backgroundColor: colors.neutral[700],
              color: colors.neutral[300],
            }}
            title="Restore default column order"
            aria-label="Reset columns to default"
          >
            Reset
          </button>
        )}

        {/* Clear Sort */}
        <button
          onClick={onClearSort}
          disabled={!isSortActive}
          className="h-6 px-2.5 rounded-md transition-colors text-[11px] font-medium disabled:opacity-50 disabled:cursor-not-allowed"
          style={{
            backgroundColor: colors.neutral[700],
            color: colors.neutral[300],
          }}
          title={isSortActive ? 'Remove active sorting' : 'No active sorting'}
          aria-label="Clear sort"
        >
          Clear Sort
        </button>

        {/* Auto-refresh Toggle */}
        <label
          className="flex items-center h-6 px-1.5 rounded-md cursor-pointer select-none"
          style={{
            backgroundColor: colors.neutral[700],
            gap: spacing.gap.xs,
          }}
          title={autoToggleTitle}
        >
          <input
            type="checkbox"
            checked={autoRefresh}
            onChange={(e) => onToggleAuto(e.target.checked)}
            className="w-3 h-3 rounded border-neutral-600 bg-neutral-800 text-emerald-500 focus:ring-2 focus:ring-emerald-500/40"
            aria-label="Auto-refresh toggle"
          />
          <span
            style={{
              fontSize: typography.fontSize.xs,
              color: autoRefresh && !autoRefreshActive ? colors.semantic.warning.light : colors.neutral[300],
            }}
          >
            {`Auto (${Math.round((autoRefreshIntervalSec ?? 10))}s)`}
          </span>
        </label>

        {autoRefresh && !autoRefreshActive && autoRefreshPauseLabel && (
          <span
            className="rounded px-1.5 py-0.5 text-[10px]"
            style={{
              color: colors.semantic.warning.light,
              backgroundColor: `${colors.semantic.warning.bg}50`,
              border: `1px solid ${colors.semantic.warning.dark}60`,
            }}
            role="status"
            aria-live="polite"
          >
            {autoRefreshPauseLabel}
          </span>
        )}

        {/* Freshness Indicator */}
        <div
          className="flex items-center h-6 px-1.5 rounded-md"
          style={{
            backgroundColor: colors.neutral[800],
            gap: spacing.gap.xs,
          }}
          title={absoluteTime}
        >
          {/* Latency Dot */}
          <div
            className={`w-1.5 h-1.5 rounded-full ${isStale ? 'animate-pulse' : ''}`}
            style={{
              backgroundColor: isStale ? colors.semantic.danger.DEFAULT : colors.semantic.success.DEFAULT,
            }}
            role="status"
            aria-label={isStale ? 'Data stale' : 'Data fresh'}
          />
          {/* Relative Time */}
          <span style={{ fontSize: typography.fontSize.xs, color: colors.neutral[400] }}>
            {relativeTime}
          </span>
        </div>
      </div>
    </div>
  );
}

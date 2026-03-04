/**
 * Params - Dense parameter grid with inline editing and validation.
 *
 * Features:
 * - Dense single-row layout with all key params inline
 * - Strategy / Status / Dirty filters for fast scanning
 * - Read-only Run indicator + Trading toggle per strategy
 * - Field-level validation on blur with dirty tracking per cell
 * - Row Save / Revert plus Save All with bounded concurrency
 * - Conflict detection + diff modal when backend changes collide with local edits
 * - Keyboard navigation (Tab, Enter, Esc, arrow keys) across the grid
 */

import { useEffect, useState, useMemo, useCallback, memo, useRef, useLayoutEffect } from 'react';
import { Check, RotateCcw, Power, Copy } from 'lucide-react';
import type {
  DragEvent as ReactDragEvent,
  MouseEvent as ReactMouseEvent
} from 'react';
import { useVirtualizer, type Virtualizer } from '@tanstack/react-virtual';
import { toast } from 'sonner';
import { shallow } from 'zustand/shallow';
import { api } from './api';
import type { ParamSchema, ValidationErrors, ParamDef, StrategyMeta } from './types';
import { validateParam, validateParams } from './utils/validation';
import ParamCell from './components/params/ParamCell';
import HeaderWithHelp from './components/params/HeaderWithHelp';
import ParamHelpModal from './components/params/ParamHelpModal';
import { ParamDiffModal } from './components/params/ParamDiffModal';
import ConfigViewer from './components/params/ConfigViewer';
import { ParamsHeader } from './components/params/ParamsHeader';
import { useParamsStore, bumpGlobalResync, type ParamsSortState } from './stores';
import { INTERVALS } from './constants';
import { usePolling } from './hooks';
import { useMobileLayout } from './hooks/useMobileLayout';
import { useIsMobile } from './hooks/useIsMobile';
import { countDirtyCells, countDirtyInSelection } from './utils/paramsState';
import { diffRemoteChanges } from './utils/rowState';
import { revertParamValues, clearDirtyForStrategies, clearErrorsForStrategies } from './utils/paramsRevert';
import { usePanelHeaderSlots } from './components/layout/PanelWrapper';
import { PanelBody } from './components/shared/PanelBody';
import { PageShell } from './components/layout/PageShell';
import { TableFilter, type FilterValues, type ColumnFilter } from './components/shared/TableFilter';
import { SimpleTooltip } from './components/ui/tooltip';
import { Switch } from './components/ui';
import { Button } from './components/ui/button/Button';
import { IconButton } from './components/ui/button/IconButton';
import { colors, STALE_THRESHOLDS, spacing, typography } from './lib/tokens';
import { deriveStrategyStatus } from './utils/strategyStatus';
import { StatusPill } from './components/shared/StatusPill';
import type { StatusDescriptor } from './components/shared/status';
import { resolvePathProfile, type PathProfile } from './config/uiProfiles';
import {
  buildProfileDefaultColumnOrder,
  deriveStrategyProfile,
  getProfileHiddenKeys,
  getProfilePriorityKeys,
  getProfileLabel,
  listParamsProfiles,
  PROFILE_TO_APPLIES_TO,
  type ParamsProfileId,
} from './config/paramsProfiles';

// =============================================================================
// FILTER CONFIGURATION
// =============================================================================

const BASE_PARAMS_FILTERS: ColumnFilter[] = [
  { key: 'strategy', label: 'Strategy', type: 'text', placeholder: 'Search strategies, params...' },
  { key: 'status', label: 'Status', type: 'select', options: ['Running', 'Stopped'] },
  { key: 'dirty', label: 'Dirty Only', type: 'select', options: ['Yes'] },
  { key: 'class', label: 'Class', type: 'select', options: [] },
  {
    key: 'venue_prefix',
    label: 'Venue',
    type: 'select',
    options: [],
  },
  { key: 'chain', label: 'Chain', type: 'select', options: [] },
];

const SORT_KEYS = {
  STRATEGY: '__strategy__',
  TRADING: '__trading__'
} as const;

const AUTO_PAUSE_LABELS = {
  editing: 'Paused (editing)',
  unsaved: 'Paused (unsaved changes)',
  loading: 'Paused (loading)',
  disabled: 'Paused'
} as const;

type AutoPauseReason = keyof typeof AUTO_PAUSE_LABELS;

type Direction = 'up' | 'down' | 'left' | 'right';
type ParamGroup = 'execution' | 'edges' | 'meta' | 'other';
const EXECUTION_KEYS = new Set<string>(['qty', 'cooldown', 'slippage_bps', 'slippage_pct']);
const EDGE_KEYS = new Set<string>([
  'cex_bid_edge',
  'cex_ask_edge',
  'inv_mult',
  'max_delta',
  'pool_edge',
  'dex_edge',
  'book_edge'
]);
const META_KEYS = new Set<string>([
  'deadline_s',
  'max_attempts',
  'max_errors',
  'error_window_s',
  'error_window_ms',
  'max_time_s',
  'max_time_ms',
  'max_age_ms',
  'freshness_mode',
  'cb_threshold',
  'cb_window_trades',
  'cb_cooldown_s'
]);

// Keys hidden in compact view.
const COMPACT_HIDDEN_KEYS = new Set<string>([
  'deadline_s',
  'max_age_ms',
  'freshness_mode',
  'max_errors',
  'error_window_s',
  'cb_threshold',
  'cb_window_trades',
  'cb_cooldown_s'
]);

// Mobile view intentionally shows only a small subset per strategy; skew params should be visible
// for live operations without requiring the full desktop grid.
const MOBILE_PARAM_LIMIT = 5;
const isSizeKey = (key: string) => key === 'qty' || key.startsWith('notional') || key.startsWith('max_');
const isEdgeKey = (key: string) => key.includes('edge') || key.startsWith('spread_');

function selectMobileParams(hotParams?: string[]): string[] {
  if (!hotParams || hotParams.length === 0) {
    return ['qty', 'cex_ask_edge', 'cex_bid_edge', 'cooldown'].slice(0, MOBILE_PARAM_LIMIT);
  }

  const normalized = hotParams
    .filter(Boolean)
    .map((k) => k.trim())
    .filter(Boolean);

  const scored = normalized
    .filter((key) => key !== 'bot_on')
    .map((key, index) => ({
      key,
      index,
      score: isSizeKey(key) ? 1 : isEdgeKey(key) ? 2 : 3,
    }));

  scored.sort((a, b) => (a.score === b.score ? a.index - b.index : a.score - b.score));

  const limited = scored.slice(0, MOBILE_PARAM_LIMIT).map((s) => s.key);
  return limited.length > 0 ? limited : normalized.slice(0, MOBILE_PARAM_LIMIT);
}

const STRATEGY_COLUMN_MIN_WIDTH = 240;
const STRATEGY_COLUMN_MAX_WIDTH = 520;
const RUN_COLUMN_WIDTH = 40;
const TRADE_COLUMN_WIDTH = 44;
const PARAM_COLUMN_WIDTH = 96;
const HEADER_HEIGHT = 32;
const PINNED_LEFT_OFFSETS = {
  strategy: 0,
} as const;

function clampInt(value: number, min: number, max: number): number {
  if (!Number.isFinite(value)) return min;
  return Math.max(min, Math.min(max, Math.trunc(value)));
}

type DragPosition = 'before' | 'after';
type DragState = 'idle' | 'dragging' | 'over-before' | 'over-after';
type BulkChangeOp = {
  columnKey: string;
  affectedIds: string[];
  previousValues: Record<string, string | undefined>;
  newValue: string;
  undoable: boolean;
};

// Stable empty values to prevent unnecessary re-renders
const STATIC_COLUMN_COUNT = 3;
const EMPTY_PARAMS: Record<string, string> = {};
const EMPTY_DIRTY_SET: Set<string> = new Set();
const EMPTY_ERRORS: ValidationErrors = {};
const EMPTY_CONFLICT_KEYS: Set<string> = new Set();
const DENSE_ROW_HEIGHT = 28;
const DENSE_CELL_PADDING = 'py-[5px]';

type StrategyRow = {
  strategy_id: string;
  running?: boolean | null;  // true=running, false=stopped, null=unknown
  params: Record<string, string>;
  meta?: StrategyMeta;
  hot_params?: string[];
};

type RunState = 'running' | 'stopped' | 'unknown';

const RUN_DOT_COLORS: Record<RunState, { color: string; halo: string }> = {
  running: { color: colors.semantic.success.DEFAULT, halo: colors.semantic.success.bg },
  stopped: { color: colors.semantic.danger.DEFAULT, halo: colors.semantic.danger.bg },
  unknown: { color: colors.text.muted, halo: 'rgba(128, 131, 139, 0.18)' },
};

const ROW_STATE_TAGS: Record<string, StatusDescriptor> = {
  conflict: { status: 'critical', label: 'Conflict' },
  updated: { status: 'info', label: 'Updated' },
  error: { status: 'critical', label: 'Error' },
};

// Memoized strategy row to prevent unnecessary re-renders
type StrategyRowProps = {
  strategy: StrategyRow;
  idx: number;
  strategyColumnWidth: number;
  orderedParamDefs: ParamDef[];
  stratParams: Record<string, string>;
  stratDirty: Set<string>;
  stratErrors: ValidationErrors;
  isSaving: boolean;
  isFlashing: boolean;
  isSelected: boolean;
  isAnchor: boolean;
  focusedParamKey: string | null;
  isRemoteUpdated: boolean;
  conflictKeys: Set<string>;
  measureRow?: (el: HTMLTableRowElement | null) => void;
  onParamChange: (strategyId: string, paramKey: string, value: string) => void;
  onParamBlur: (strategyId: string, paramKey: string) => void;
  onParamFocus: (strategyId: string, paramKey: string, rowIndex: number, columnIndex: number) => void;
  onParamBlurForFocus: () => void;
  onSave: (strategyId: string) => void;
  onRevert: (strategyId: string) => void;
  onConflictKeepMine: (strategyId: string) => void;
  onConflictUseRemote: (strategyId: string) => void;
  onConflictDiff: (strategyId: string) => void;
  onConfigView: (strategyId: string) => void;
  onRowMouseDown: (strategyId: string, rowIndex: number, event: ReactMouseEvent<HTMLTableCellElement>) => void;
  onRowMouseEnter: (strategyId: string, rowIndex: number) => void;
  onRowMouseUp: () => void;
  onCellNavigate: (rowIndex: number, columnIndex: number, direction: Direction) => void;
  highlightedParamKey: string | null;
};

// Custom comparison function to prevent unnecessary re-renders
// Only re-render if the actual data for this row changes, not if callbacks change
function arePropsEqual(prev: StrategyRowProps, next: StrategyRowProps): boolean {
  // Strategy identity must match
  if (prev.strategy.strategy_id !== next.strategy.strategy_id) return false;

  if (prev.strategyColumnWidth !== next.strategyColumnWidth) return false;

  // Running status changed
  if (prev.strategy.running !== next.strategy.running) return false;

  // Index changed (affects row styling)
  if (prev.idx !== next.idx) return false;

  if (prev.isSelected !== next.isSelected) return false;
  if (prev.isAnchor !== next.isAnchor) return false;
  if (prev.focusedParamKey !== next.focusedParamKey) return false;
  if (prev.isRemoteUpdated !== next.isRemoteUpdated) return false;
  if (prev.highlightedParamKey !== next.highlightedParamKey) return false;

  if (prev.conflictKeys.size !== next.conflictKeys.size) return false;
  for (const key of prev.conflictKeys) {
    if (!next.conflictKeys.has(key)) return false;
  }

  // Param definitions changed (identity check first, then fall through to value checks)
  if (prev.orderedParamDefs !== next.orderedParamDefs) return false;

  // Saving or flashing state changed
  if (prev.isSaving !== next.isSaving) return false;
  if (prev.isFlashing !== next.isFlashing) return false;

  // Compact mode changed (affects row padding)
  const prevBotOn = prev.stratParams['bot_on'] ?? prev.strategy.params?.bot_on;
  const nextBotOn = next.stratParams['bot_on'] ?? next.strategy.params?.bot_on;
  if (prevBotOn !== nextBotOn) return false;

  // Dirty state changed (affects Save button visibility)
  if (prev.stratDirty.size !== next.stratDirty.size) return false;
  for (const key of prev.stratDirty) {
    if (!next.stratDirty.has(key)) return false;
  }

  // Errors changed (affects validation display)
  const prevErrorKeys = Object.keys(prev.stratErrors);
  const nextErrorKeys = Object.keys(next.stratErrors);
  if (prevErrorKeys.length !== nextErrorKeys.length) return false;
  for (const key of prevErrorKeys) {
    if (prev.stratErrors[key] !== next.stratErrors[key]) return false;
  }

  // Param values changed (the actual data)
  for (const paramDef of prev.orderedParamDefs) {
    if (prev.stratParams[paramDef.key] !== next.stratParams[paramDef.key]) return false;
  }

  // Callbacks are ignored - they don't affect what's rendered
  return true;
}

const MemoizedStrategyRow = memo(function StrategyRow({
  strategy,
  idx,
  strategyColumnWidth,
  orderedParamDefs,
  stratParams,
  stratDirty,
  stratErrors,
  isSaving,
  isFlashing,
  isSelected,
  isAnchor,
  focusedParamKey,
  onParamChange,
  onParamBlur,
  onParamFocus,
  onParamBlurForFocus,
  onSave,
  onRevert,
  onConflictKeepMine,
  onConflictUseRemote,
  onConflictDiff,
  onConfigView,
  onRowMouseDown,
  onRowMouseEnter,
  onRowMouseUp,
  onCellNavigate,
  highlightedParamKey,
  isRemoteUpdated,
  conflictKeys,
  measureRow
}: StrategyRowProps) {
  const isDirty = stratDirty.size > 0;
  const hasError = Object.keys(stratErrors).length > 0;
  const isConflict = conflictKeys.size > 0;
  const measureRowRef = useMemo(
    () => (measureRow ? (node: HTMLTableRowElement | null) => measureRow(node) : undefined),
    [measureRow]
  );

  const handleSave = useCallback(() => {
    onSave(strategy.strategy_id);
  }, [strategy.strategy_id, onSave]);

  const handleRevert = useCallback(() => {
    onRevert(strategy.strategy_id);
  }, [strategy.strategy_id, onRevert]);

  const handleConfigView = useCallback(() => {
    onConfigView(strategy.strategy_id);
  }, [strategy.strategy_id, onConfigView]);

  const flashingClass = isFlashing ? 'animate-flash' : '';
  const rowBgColor = isSelected ? colors.bg.active : colors.bg.surface;
  const pinnedBackground = rowBgColor;

  const cellPadding = DENSE_CELL_PADDING;

  const tradingValue = stratParams['bot_on'] ?? strategy.params?.bot_on ?? '0';
  const tradingStatus = useMemo(
    () => deriveStrategyStatus({ running: strategy.running, trading: tradingValue }),
    [strategy.running, tradingValue]
  );
  const tradingEnabled = tradingStatus.tradingEnabled;
  const tradingDirty = stratDirty.has('bot_on');

  const runState: RunState =
    strategy.running === true ? 'running' : strategy.running === false ? 'stopped' : 'unknown';
  const runLabel =
    runState === 'running' ? 'Runner On' : runState === 'stopped' ? 'Runner Off' : 'Runner Unknown';
  const runDotColors = RUN_DOT_COLORS[runState];

  const tradingTooltipLines = [
    tradingEnabled ? 'Trading enabled' : 'Trading disabled',
    tradingStatus.coolingDown ? 'Cooling: Yes' : 'Cooling: No',
    `bot_on=${tradingValue ?? '—'}`
  ];
  const tradingTooltip = tradingTooltipLines.join('\n');

  const handleTradingChange = useCallback((nextChecked: boolean) => {
    const nextValue = nextChecked ? '1' : '0';
    if (nextValue === tradingValue) return;
    onParamChange(strategy.strategy_id, 'bot_on', nextValue);
    onParamBlur(strategy.strategy_id, 'bot_on');
  }, [onParamBlur, onParamChange, strategy.strategy_id, tradingValue]);

  const handleSaveClick = useCallback((event: React.MouseEvent) => {
    event.stopPropagation();
    handleSave();
  }, [handleSave]);

  const handleRevertClick = useCallback((event: React.MouseEvent) => {
    event.stopPropagation();
    handleRevert();
  }, [handleRevert]);

  const stateTags: Array<{ key: string; descriptor: StatusDescriptor }> = [];
  if (isConflict) {
    stateTags.push({ key: 'conflict', descriptor: ROW_STATE_TAGS.conflict });
  }
  if (!isDirty && isRemoteUpdated) {
    stateTags.push({ key: 'updated', descriptor: ROW_STATE_TAGS.updated });
  }
  if (hasError) {
    stateTags.push({ key: 'error', descriptor: ROW_STATE_TAGS.error });
  }

  return (
    <tr
      ref={measureRowRef}
      role="row"
      aria-selected={isSelected}
      className={`${flashingClass} transition-colors duration-150 ease-out`}
      style={{
        backgroundColor: rowBgColor,
      }}
      onMouseEnter={(e) => {
        if (!isSelected) {
          e.currentTarget.style.backgroundColor = colors.bg.hover;
        }
      }}
      onMouseLeave={(e) => {
        if (!isSelected) {
          e.currentTarget.style.backgroundColor = rowBgColor;
        }
      }}
    >
      <td
        className={`sticky px-3 ${cellPadding} border-b backdrop-blur-sm ${isAnchor ? 'border-l-2' : ''}`}
        style={{
          backgroundColor: pinnedBackground || 'rgba(13,14,16,0.92)',
          borderColor: colors.border.DEFAULT,
          borderLeftColor: isAnchor ? colors.accent.DEFAULT : colors.border.DEFAULT,
          left: PINNED_LEFT_OFFSETS.strategy,
          width: strategyColumnWidth,
          minWidth: strategyColumnWidth,
          zIndex: 30,
        }}
        onMouseDown={(event) => onRowMouseDown(strategy.strategy_id, idx, event)}
        onMouseEnter={() => onRowMouseEnter(strategy.strategy_id, idx)}
        onMouseUp={onRowMouseUp}
      >
        <div className="flex items-center gap-3">
          <button
            type="button"
            onClick={(event) => {
              event.stopPropagation();
              handleConfigView();
            }}
            className="truncate text-left text-[13px] font-mono font-medium text-text-primary hover:text-text-secondary transition-colors"
          >
            {strategy.strategy_id}
          </button>
          {isDirty && (
            <span
              className="h-1.5 w-1.5 rounded-full bg-amber-400 ring-4 ring-amber-400/20"
              aria-label="Row has unsaved changes"
              title="Unsaved changes"
            />
          )}
          <div className="ml-auto flex items-center gap-2 text-[10px] text-zinc-500">
            {stateTags.map((tag) => (
              <StatusPill
                key={`${strategy.strategy_id}-${tag.key}`}
                status={tag.descriptor.status}
                label={tag.descriptor.label}
                size="xs"
                tone="subtle"
              />
            ))}
            {isSaving ? (
              <div className="w-3 h-3 animate-spin rounded-full border-[2px] border-neutral-600 border-t-neutral-200" />
            ) : (
              isDirty && (
                <>
                  <IconButton
                    variant="success"
                    size="xs"
                    onClick={handleSaveClick}
                    aria-label="Save row changes"
                    title="Save row changes"
                  >
                    <Check className="w-3 h-3" />
                  </IconButton>
                  <IconButton
                    variant="warning"
                    size="xs"
                    onClick={handleRevertClick}
                    aria-label="Revert row changes"
                    title="Revert row changes"
                  >
                    <RotateCcw className="w-3 h-3" />
                  </IconButton>
                </>
              )
            )}
          </div>
        </div>
        {isConflict && (
          <div className="mt-2 rounded border border-rose-500/40 bg-rose-950/30 p-2 text-[11px] text-rose-100">
            <p className="font-semibold">Remote update detected while editing this row.</p>
            <div className="mt-2 flex flex-wrap items-center gap-2">
              <Button
                variant="success"
                size="xs"
                onClick={() => onConflictKeepMine(strategy.strategy_id)}
              >
                Keep Mine
              </Button>
              <Button
                variant="outline"
                size="xs"
                onClick={() => onConflictUseRemote(strategy.strategy_id)}
              >
                Use Remote
              </Button>
              <Button
                variant="secondary"
                size="xs"
                onClick={() => onConflictDiff(strategy.strategy_id)}
              >
                Diff
              </Button>
            </div>
          </div>
        )}
      </td>

      {/* Run indicator (read-only, comes from metrics) */}
      <td
        className={`px-2 ${cellPadding} border-b text-center`}
        style={{ width: RUN_COLUMN_WIDTH, minWidth: RUN_COLUMN_WIDTH, borderColor: colors.border.DEFAULT }}
      >
        <SimpleTooltip content={runLabel} delay={150}>
          <span className="inline-flex w-full justify-center">
            <span
              role="status"
              aria-label={`${runLabel} for ${strategy.strategy_id}`}
              data-testid={`run-indicator-${strategy.strategy_id}`}
              data-state={runState}
              className="block rounded-full"
              style={{
                width: 10,
                height: 10,
                backgroundColor: runDotColors.color,
                boxShadow: `0 0 0 4px ${runDotColors.halo}`,
              }}
            />
          </span>
        </SimpleTooltip>
      </td>

      {/* Trading toggle – left/right switch */}
      <td
        className={`px-2 ${cellPadding} border-b text-center`}
        style={{ width: TRADE_COLUMN_WIDTH, minWidth: TRADE_COLUMN_WIDTH, borderColor: colors.border.DEFAULT }}
      >
        <SimpleTooltip content={tradingTooltip} delay={150}>
          <Switch
            size="sm"
            checked={tradingEnabled}
            onCheckedChange={handleTradingChange}
            disabled={isSaving}
            aria-label={`Toggle trading for ${strategy.strategy_id}`}
            data-testid={`trading-toggle-${strategy.strategy_id}`}
          />
        </SimpleTooltip>
      </td>

      {orderedParamDefs.map((paramDef, columnIdx) => {
        const value = stratParams[paramDef.key] || '';
        const dirty = stratDirty.has(paramDef.key);
        const error = stratErrors[paramDef.key];
        const group = getParamGroup(paramDef.key);
        const prevGroup = columnIdx > 0 ? getParamGroup(orderedParamDefs[columnIdx - 1].key) : null;
        const groupDivider =
          columnIdx === 0 || group !== prevGroup ? 'border-l' : '';
        const focusOutline =
          focusedParamKey === paramDef.key && isSelected
            ? 'outline outline-1 outline-sky-500/60 outline-offset-[1px]'
            : '';
        const bulkHighlight = highlightedParamKey === paramDef.key ? 'bg-emerald-900/20' : '';

        return (
          <td
            key={paramDef.key}
            className={`px-2 ${cellPadding} border-b align-middle ${groupDivider} ${focusOutline} ${bulkHighlight}`}
            style={{
              width: PARAM_COLUMN_WIDTH,
              minWidth: PARAM_COLUMN_WIDTH,
              borderColor: colors.border.DEFAULT,
            }}
          >
            <ParamCell
              value={value}
              paramDef={paramDef}
              dirty={dirty}
              error={error}
              saving={isSaving}
              onChange={(newValue) => onParamChange(strategy.strategy_id, paramDef.key, newValue)}
              onFocus={() => onParamFocus(strategy.strategy_id, paramDef.key, idx, columnIdx)}
              onBlur={() => {
                onParamBlur(strategy.strategy_id, paramDef.key);
                onParamBlurForFocus();
              }}
              onSave={handleSave}
              onNavigate={(direction) => onCellNavigate(idx, columnIdx, direction)}
              density="dense"
              dataAttrs={{
                'data-row': idx,
                'data-col': columnIdx,
                'data-param': paramDef.key,
                'data-strategy': strategy.strategy_id
              }}
            />
          </td>
        );
      })}
    </tr>
  );
}, arePropsEqual);

function reconcileColumnOrder(order: string[] | null | undefined, defaultOrder: string[]): string[] {
  const result: string[] = [];
  const seen = new Set<string>();
  const defaultSet = new Set(defaultOrder);
  const source = Array.isArray(order) && order.length > 0 ? order : defaultOrder;

  source.forEach((key) => {
    if (!defaultSet.has(key)) return;
    if (seen.has(key)) return;
    seen.add(key);
    result.push(key);
  });

  defaultOrder.forEach((key) => {
    if (!seen.has(key)) {
      seen.add(key);
      result.push(key);
    }
  });

  return result;
}

function arraysShallowEqual(a: string[] | null | undefined, b: string[]): boolean {
  if (!a) return false;
  if (a.length !== b.length) return false;
  for (let i = 0; i < a.length; i += 1) {
    if (a[i] !== b[i]) return false;
  }
  return true;
}

function uniqueSortedOptions(values: Array<string | undefined>): string[] {
  const normalized = values
    .map((value) => value?.trim())
    .filter((value): value is string => Boolean(value));
  return Array.from(new Set(normalized)).sort((a, b) =>
    a.localeCompare(b, undefined, { sensitivity: 'base' })
  );
}

function moveColumn(order: string[], sourceKey: string, targetKey: string, position: DragPosition): string[] {
  if (sourceKey === targetKey) {
    return order.slice();
  }

  const filtered = order.filter((key) => key !== sourceKey);
  let targetIndex = filtered.indexOf(targetKey);

  if (targetIndex === -1) {
    filtered.push(sourceKey);
    return filtered;
  }

  if (position === 'after') {
    targetIndex += 1;
  }

  filtered.splice(targetIndex, 0, sourceKey);
  return filtered;
}

function hasValue(value?: string | null): value is string {
  return typeof value === 'string' && value.trim() !== '';
}

function compareMissingValues(a?: string, b?: string): number | null {
  const aHas = hasValue(a);
  const bHas = hasValue(b);

  if (aHas && bHas) return null;
  if (!aHas && !bHas) return 0;
  if (!aHas) return 1;
  return -1;
}

function normalizeBoolForSort(value: string): number {
  const normalized = value.trim().toLowerCase();
  if (normalized === '1' || normalized === 'true' || normalized === 'yes' || normalized === 'on') {
    return 1;
  }
  if (normalized === '0' || normalized === 'false' || normalized === 'no' || normalized === 'off') {
    return 0;
  }
  return 0;
}

const HEADER_HINTS: Record<string, string> = {
  deadline_s: 'dl: Delay limit before cancelling order (seconds)',
  cooldown: 'co: Cooldown window between fills (seconds)',
  des_qty_global:
    'MakerV3 global target inventory (base units). Used with max_qty_global/max_skew_bps_global for global skew.',
  max_qty_global:
    'MakerV3 global inventory cap (base units). <= 0 disables global inventory ratio/skew.',
  max_skew_bps_global:
    'MakerV3 global skew cap (bps). Combined with max_skew_bps_local for total maker skew.',
  des_qty_local:
    'MakerV3 local target inventory (base units) for additive local skew (local venue/instrument/base bucket).',
  max_qty_local:
    'MakerV3 local inventory cap (base units). <= 0 disables local component.',
  max_skew_bps_local:
    'MakerV3 local skew cap (bps). Added to global skew for maker-leg total skew.',
  des_qty:
    'Legacy alias for des_qty_global.',
  max_qty:
    'Legacy alias for max_qty_global.',
  max_skew_bps:
    'Legacy alias for max_skew_bps_global.',
  local_des_qty:
    'Legacy alias for des_qty_local.',
  local_max_qty:
    'Legacy alias for max_qty_local.',
  local_max_skew_bps:
    'Legacy alias for max_skew_bps_local.',
  inv_mult:
    'Legacy MakerV2: Skews bid/ask edges based on inventory ratio (recommended <= 1.0). ' +
    'r=clamp(unhedged_delta/max_delta, -1..1); s=abs(r)*inv_mult. ' +
    'unhedged>0 => bid_edge*(1+s), ask_edge*(1-s); unhedged<0 => bid_edge*(1-s), ask_edge*(1+s). ' +
    'Example (bps): bid=10, ask=10, unhedged=+50, max=100, mult=1 => bid=15, ask=5.',
  max_delta:
    'Legacy MakerV2: Used by inv_mult via r=clamp(unhedged_delta/max_delta, -1..1); <= 0 disables.',
  max_attempts: 'ma: Maximum retry attempts before abort',
  max_errors: 'err: Allowed errors within the window',
  error_window_s: 'err window: Sliding seconds window for errors',
  cb_threshold: 'cb: Set > 1.0 to disable the failure-rate gate (default 2.0).',
  quote_fail_critical_after_count:
    'MakerV3: escalate repeated quote failures to CRITICAL after this streak count.',
  quote_fail_critical_after_s:
    'MakerV3: escalate repeated quote failures to CRITICAL after this elapsed streak duration (seconds).',
  max_time_s: 'mt: Max execution time (seconds)',
  max_time_ms: 'mt: Max execution time (milliseconds)'
};

function getParamGroup(key: string): ParamGroup {
  if (EXECUTION_KEYS.has(key)) return 'execution';
  if (EDGE_KEYS.has(key)) return 'edges';
  if (META_KEYS.has(key)) return 'meta';
  return 'other';
}

export default function Params({
  dense = false,
  onRemove,
  showHeader = true,
  variant = 'desktop',
}: {
  dense?: boolean;
  onRemove?: () => void;
  showHeader?: boolean;
  variant?: 'desktop' | 'mobile';
} = {}) {
  const {
    auto,
    setAuto,
    viewMode,
    setViewMode,
    activeProfile,
    setActiveProfile,
    columnPrefs,
    setColumnOrder: persistColumnOrder,
    setColumnVisibility,
    resetColumnVisibility,
    sortState,
    setSortState,
    clearSort,
    selectedStrategies,
    setSelectedStrategies,
    clearSelection,
    lastFocusedCell,
    setLastFocusedCell,
    storeLastUpdate,
    setStoreLastUpdate
  } = useParamsStore(
    (state) => ({
      auto: state.auto,
      setAuto: state.setAuto,
      viewMode: state.viewMode,
      setViewMode: state.setViewMode,
      activeProfile: state.activeProfile,
      setActiveProfile: state.setActiveProfile,
      columnPrefs: state.columnPrefs,
      setColumnOrder: state.setColumnOrder,
      setColumnVisibility: state.setColumnVisibility,
      resetColumnVisibility: state.resetColumnVisibility,
      sortState: state.sortState,
      setSortState: state.setSortState,
      clearSort: state.clearSort,
      selectedStrategies: state.selectedStrategies,
      setSelectedStrategies: state.setSelectedStrategies,
      clearSelection: state.clearSelection,
      lastFocusedCell: state.lastFocusedCell,
      setLastFocusedCell: state.setLastFocusedCell,
      storeLastUpdate: state.lastUpdate,
      setStoreLastUpdate: state.setLastUpdate
    }),
    shallow
  );
  const pathProfile = useMemo<PathProfile>(() => {
    const pathname =
      typeof window !== 'undefined' && typeof window.location?.pathname === 'string'
        ? window.location.pathname
        : '/';
    const firstSegment = pathname
      .split('/')
      .filter(Boolean)[0];
    return resolvePathProfile(firstSegment);
  }, []);
  const didInitRouteProfileRef = useRef(false);
  useEffect(() => {
    if (didInitRouteProfileRef.current) return;
    didInitRouteProfileRef.current = true;
    if (pathProfile === 'tokenmm' && activeProfile !== 'maker_v3') {
      setActiveProfile('maker_v3');
    }
  }, [pathProfile, activeProfile, setActiveProfile]);
  const { isMobile: layoutIsMobile } = useMobileLayout();
  const isBreakpointMobile = useIsMobile();
  const isMobileView = variant === 'mobile' || (variant !== 'desktop' && isBreakpointMobile);

  const [schema, setSchema] = useState<ParamSchema | null>(null);
  const [strategies, setStrategies] = useState<StrategyRow[]>([]);
  const [draggingKey, setDraggingKey] = useState<string | null>(null);
  const [dragTarget, setDragTarget] = useState<{ key: string; position: DragPosition } | null>(null);
  useEffect(() => {
    if (layoutIsMobile && viewMode !== 'compact') {
      setViewMode('compact');
    }
  }, [layoutIsMobile, viewMode, setViewMode]);
  const [paramValues, setParamValues] = useState<Map<string, Record<string, string>>>(new Map());
  const [originalValues, setOriginalValues] = useState<Map<string, Record<string, string>>>(new Map());
  const [dirtyParams, setDirtyParams] = useState<Map<string, Set<string>>>(new Map());
  const [errorParams, setErrorParams] = useState<Map<string, ValidationErrors>>(new Map());
  const [bulkDrafts, setBulkDrafts] = useState<Record<string, string>>({});
  const [bulkActiveParam, setBulkActiveParam] = useState<string | null>(null);
  const [lastBulkChangeOp, setLastBulkChangeOp] = useState<BulkChangeOp | null>(null);
  const [undoInFlight, setUndoInFlight] = useState(false);
  const [saving, setSaving] = useState<Set<string>>(new Set());
  const [loading, setLoading] = useState(false);
  const [saveAllProgress, setSaveAllProgress] = useState<{ completed: number; failed: number; total: number } | null>(null);
  const [refreshing, setRefreshing] = useState(false);
  const [lastUpdate, setLastUpdate] = useState<number>(storeLastUpdate ?? Date.now());
  const [initialLoadDone, setInitialLoadDone] = useState(false);
  const [initialLoadSuccess, setInitialLoadSuccess] = useState(false);
  const [loadError, setLoadError] = useState<string | null>(null);
  const [headerHeight, setHeaderHeight] = useState(HEADER_HEIGHT);
  const [flashingRows, setFlashingRows] = useState<Set<string>>(new Set());
  const [remoteUpdatedRows, setRemoteUpdatedRows] = useState<Set<string>>(new Set());
  const [conflictRows, setConflictRows] = useState<Map<string, Set<string>>>(new Map());
  const [hasInputFocus, setHasInputFocus] = useState(false);
  const [anchorStrategyId, setAnchorStrategyId] = useState<string | null>(null);
  const [customizeColumns, setCustomizeColumns] = useState(false);
  // Filter state using TableFilter component
  const [filterValues, setFilterValues] = useState<FilterValues>({});
  const [mobileFiltersOpen, setMobileFiltersOpen] = useState(false);
  const filterColumns = useMemo(() => {
    const classOptions = uniqueSortedOptions(strategies.map((s) => s.meta?.class));
    const venueOptions = uniqueSortedOptions(strategies.map((s) => s.meta?.venue_prefix));
    const chainOptions = uniqueSortedOptions(strategies.map((s) => s.meta?.chain));
    return BASE_PARAMS_FILTERS.map((column) => {
      if (column.key === 'class') return { ...column, options: classOptions };
      if (column.key === 'venue_prefix') return { ...column, options: venueOptions };
      if (column.key === 'chain') return { ...column, options: chainOptions };
      return column;
    });
  }, [strategies]);
  const profileIds = useMemo(() => listParamsProfiles(), []);
  const profilePriorityKeySet = useMemo(
    () => new Set(getProfilePriorityKeys(activeProfile)),
    [activeProfile]
  );
  const profileHiddenKeySet = useMemo(
    () => new Set(getProfileHiddenKeys(activeProfile)),
    [activeProfile]
  );
  const profileStrategyKeySet = useMemo(() => {
    const keys = new Set<string>();
    strategies.forEach((strategy) => {
      if (deriveStrategyProfile(strategy) !== activeProfile) return;
      Object.keys(strategy.params || {}).forEach((key) => keys.add(key));
      (strategy.hot_params || []).forEach((key) => {
        const normalized = String(key || '').trim();
        if (normalized) keys.add(normalized);
      });
    });
    return keys;
  }, [strategies, activeProfile]);
  const defaultColumnOrder = useMemo(
    () => (schema ? buildProfileDefaultColumnOrder(schema, activeProfile) : []),
    [schema, activeProfile]
  );

  // Modal states
  const [helpModalParam, setHelpModalParam] = useState<string | null>(null);
  const [configViewerStrategy, setConfigViewerStrategy] = useState<string | null>(null);
  const [diffStrategyId, setDiffStrategyId] = useState<string | null>(null);

  // Refs for derived state
  const dirtyRef = useRef(dirtyParams);
  const selectionRef = useRef(selectedStrategies);
  const anchorIndexRef = useRef<number | null>(null);
  const dragSelectingRef = useRef(false);
  const tableRef = useRef<HTMLTableElement | null>(null);
  const headerRowRef = useRef<HTMLTableRowElement | null>(null);
  const scrollContainerRef = useRef<HTMLDivElement | null>(null);
  const rowVirtualizerRef = useRef<Virtualizer<HTMLDivElement, HTMLTableRowElement> | null>(null);
  const strategyIndexRef = useRef<Map<string, number>>(new Map());
  const flashTimersRef = useRef<Map<string, ReturnType<typeof setTimeout>>>(new Map());
  const remoteUpdateTimersRef = useRef<Map<string, ReturnType<typeof setTimeout>>>(new Map());
  const didAutoSelectSingleRef = useRef(false);
  const originalValuesRef = useRef<Map<string, Record<string, string>>>(new Map());
  const paramValuesRef = useRef(paramValues);
  const schemaCacheRef = useRef<ParamSchema | null>(null);
  const conflictRowsRef = useRef(conflictRows);

  // Sync CSS variable with actual header height to keep bulk row flush under the header.
  useLayoutEffect(() => {
    let observer: ResizeObserver | null = null;
    let raf: number | null = null;

    const attach = () => {
      const headerEl = headerRowRef.current;
      const tableEl = tableRef.current;

      if (!headerEl || !tableEl || typeof ResizeObserver === 'undefined') {
        raf = typeof requestAnimationFrame !== 'undefined' ? requestAnimationFrame(attach) : null;
        return;
      }

      const updateHeight = (observedHeight?: number) => {
        const measured = observedHeight ?? headerEl.getBoundingClientRect().height;
        const height = measured && Number.isFinite(measured) && measured > 0 ? measured : HEADER_HEIGHT;
        setHeaderHeight(height);
        tableEl.style.setProperty('--params-header-height', `${height}px`);
      };

      updateHeight();
      observer = new ResizeObserver((entries) => {
        const entryHeight = entries?.[0]?.contentRect?.height;
        updateHeight(entryHeight);
      });
      observer.observe(headerEl);
    };

    attach();

    return () => {
      if (observer) observer.disconnect();
      if (raf && typeof cancelAnimationFrame !== 'undefined') {
        cancelAnimationFrame(raf);
      }
    };
  }, []);

  const markLastUpdate = useCallback(
    (timestamp?: number) => {
      const next = timestamp ?? Date.now();
      setLastUpdate(next);
      setStoreLastUpdate(next);
      return next;
    },
    [setStoreLastUpdate]
  );

  useEffect(() => {
    dirtyRef.current = dirtyParams;
  }, [dirtyParams]);

  useEffect(() => {
    paramValuesRef.current = paramValues;
  }, [paramValues]);

  useEffect(() => {
    originalValuesRef.current = originalValues;
  }, [originalValues]);

  useEffect(() => {
    conflictRowsRef.current = conflictRows;
  }, [conflictRows]);

  useEffect(() => {
    selectionRef.current = selectedStrategies;
    if (selectedStrategies.length === 0) {
      setAnchorStrategyId(null);
      anchorIndexRef.current = null;
    } else if (anchorStrategyId && !selectedStrategies.includes(anchorStrategyId)) {
      const fallback = selectedStrategies[selectedStrategies.length - 1];
      setAnchorStrategyId(fallback);
    }
  }, [selectedStrategies, anchorStrategyId]);

  useEffect(() => {
    if (!customizeColumns) {
      setDraggingKey(null);
      setDragTarget(null);
    }
  }, [customizeColumns]);

  useEffect(() => {
    if (diffStrategyId && !conflictRows.has(diffStrategyId)) {
      setDiffStrategyId(null);
    }
  }, [diffStrategyId, conflictRows]);

  const columnOrder = useMemo(() => {
    const forceCanonicalOrder = activeProfile === 'maker_v3';
    if (forceCanonicalOrder) {
      return defaultColumnOrder;
    }
    if (!schema) {
      return columnPrefs.order && columnPrefs.order.length > 0
        ? columnPrefs.order
        : defaultColumnOrder;
    }
    if (!columnPrefs.order || columnPrefs.order.length === 0) {
      return defaultColumnOrder;
    }
    return reconcileColumnOrder(columnPrefs.order, defaultColumnOrder);
  }, [schema, columnPrefs.order, defaultColumnOrder, activeProfile]);

  useEffect(() => {
    if (!schema) return;
    const forceCanonicalOrder = activeProfile === 'maker_v3';
    if (forceCanonicalOrder) {
      const persistedOrder = Array.isArray(columnPrefs.order) ? columnPrefs.order : [];
      if (!arraysShallowEqual(persistedOrder, defaultColumnOrder)) {
        persistColumnOrder([...defaultColumnOrder]);
      }
      return;
    }
    if (!columnPrefs.order || columnPrefs.order.length === 0) {
      persistColumnOrder(defaultColumnOrder);
      return;
    }
    const reconciled = reconcileColumnOrder(columnPrefs.order, defaultColumnOrder);
    if (!arraysShallowEqual(columnPrefs.order, reconciled)) {
      persistColumnOrder(reconciled);
    }
  }, [schema, columnPrefs.order, defaultColumnOrder, persistColumnOrder, activeProfile]);

  const scheduleFlashClear = useCallback((strategyId: string, delay = 500) => {
    const existing = flashTimersRef.current.get(strategyId);
    if (existing) {
      clearTimeout(existing);
    }
    const timer = setTimeout(() => {
      setFlashingRows(prev => {
        if (!prev.has(strategyId)) {
          return prev;
        }
        const next = new Set(prev);
        next.delete(strategyId);
        return next;
      });
      flashTimersRef.current.delete(strategyId);
    }, delay);
    flashTimersRef.current.set(strategyId, timer);
  }, []);

  const scheduleRemoteUpdateClear = useCallback((strategyId: string, delay = 1200) => {
    const existing = remoteUpdateTimersRef.current.get(strategyId);
    if (existing) {
      clearTimeout(existing);
    }
    const timer = setTimeout(() => {
      setRemoteUpdatedRows((prev) => {
        if (!prev.has(strategyId)) {
          return prev;
        }
        const next = new Set(prev);
        next.delete(strategyId);
        return next;
      });
      remoteUpdateTimersRef.current.delete(strategyId);
    }, delay);
    remoteUpdateTimersRef.current.set(strategyId, timer);
  }, []);

  useEffect(() => {
    return () => {
      flashTimersRef.current.forEach((timer) => clearTimeout(timer));
      flashTimersRef.current.clear();
      remoteUpdateTimersRef.current.forEach((timer) => clearTimeout(timer));
      remoteUpdateTimersRef.current.clear();
    };
  }, []);

  const loadData = useCallback(async () => {
    // Only show loading spinner on initial load, not on autorefresh
    if (!initialLoadDone) {
      setLoading(true);
      setLoadError(null);
    }
    try {
      const prevOriginals = originalValuesRef.current;
      const prevParamValues = paramValuesRef.current;
      let schemaData: ParamSchema;
      let paramsData: import('./types').ParamsResponse[];

      if (!schemaCacheRef.current) {
        const [schemaResp, paramsResp] = await Promise.all([
          api.getParamSchema(),
          api.getParams()
        ]);

        if (!schemaResp || !schemaResp.params) {
          throw new Error('Invalid schema response: missing params');
        }
        if (!Array.isArray(paramsResp)) {
          throw new Error('Invalid params response: expected array');
        }

        schemaCacheRef.current = schemaResp;
        schemaData = schemaResp;
        paramsData = paramsResp;
      } else {
        schemaData = schemaCacheRef.current;
        const paramsResp = await api.getParams();
        if (!Array.isArray(paramsResp)) {
          throw new Error('Invalid params response: expected array');
        }
        paramsData = paramsResp;
      }

      if (!schema) {
        setSchema(schemaData);
      }

      // Transform params data with validation
      const strategyRows: StrategyRow[] = paramsData
        .filter(p => {
          if (!p.strategy_id) {
            console.warn('[params] Strategy missing strategy_id, skipping:', p);
            return false;
          }
          return true;
        })
        .map(p => ({
          strategy_id: p.strategy_id!,
          running: (p.running !== undefined && p.running !== null) ? p.running : null,
          params: p.params || {},
          meta: p.meta,
          hot_params: (p as any).hot_params || (p.meta as any)?.hot_params,
        }));

      const schemaDefaults: Record<string, string> = {};
      if (schemaData?.params) {
        for (const [key, def] of Object.entries(schemaData.params)) {
          if (def.default === undefined || def.default === null) {
            continue;
          }
          schemaDefaults[key] = String(def.default);
        }
      }

      // Initialize param values and originals
      const paramsMap = new Map<string, Record<string, string>>();
      const origMap = new Map<string, Record<string, string>>();

      strategyRows.forEach(({ strategy_id, params }) => {
        const mergedParams = { ...schemaDefaults, ...params };
        paramsMap.set(strategy_id, mergedParams);
        origMap.set(strategy_id, { ...mergedParams });
      });

      const { remoteUpdated, conflictingDirty } = diffRemoteChanges(prevOriginals, origMap, dirtyRef.current);

      // Only update state if data has actually changed to prevent unnecessary re-renders
      setStrategies(prevStrategies => {
        // Check if strategies changed (length, strategy_id, or running status)
        if (prevStrategies.length !== strategyRows.length) {
          return strategyRows;
        }
        const changed = strategyRows.some((row, idx) =>
          row.strategy_id !== prevStrategies[idx]?.strategy_id ||
          row.running !== prevStrategies[idx]?.running
        );
        return changed ? strategyRows : prevStrategies;
      });

      setParamValues(prevParamsMap => {
        // On initial load, always use new data
        if (!initialLoadDone) {
          return paramsMap;
        }

        // Smart merge: preserve dirty params, only update clean params
        const mergedMap = new Map<string, Record<string, string>>();

        for (const [strategyId, newParams] of paramsMap.entries()) {
          const prevParams = prevParamsMap.get(strategyId);
          const strategyDirtyParams = dirtyRef.current.get(strategyId);

          if (!prevParams || !strategyDirtyParams || strategyDirtyParams.size === 0) {
            // No previous data or no dirty params - use new data as-is
            mergedMap.set(strategyId, newParams);
          } else {
            // Merge: preserve dirty params from previous state, use new data for clean params
            const merged = { ...newParams };
            strategyDirtyParams.forEach(paramKey => {
              if (prevParams[paramKey] !== undefined) {
                merged[paramKey] = prevParams[paramKey];
              }
            });
            mergedMap.set(strategyId, merged);
          }
        }

        // Check if anything changed
        if (mergedMap.size !== prevParamsMap.size) {
          return mergedMap;
        }
        let hasChanges = false;
        for (const [strategyId, params] of mergedMap.entries()) {
          const prevParams = prevParamsMap.get(strategyId);
          if (!prevParams) {
            hasChanges = true;
            break;
          }
          for (const [key, value] of Object.entries(params)) {
            if (prevParams[key] !== value) {
              hasChanges = true;
              break;
            }
          }
          if (hasChanges) break;
        }
        return hasChanges ? mergedMap : prevParamsMap;
      });

      setOriginalValues(prevOrigMap => {
        // Check if any original values changed
        if (origMap.size !== prevOrigMap.size) {
          return origMap;
        }
        let hasChanges = false;
        for (const [strategyId, params] of origMap.entries()) {
          const prevParams = prevOrigMap.get(strategyId);
          if (!prevParams) {
            hasChanges = true;
            break;
          }
          for (const [key, value] of Object.entries(params)) {
            if (prevParams[key] !== value) {
              hasChanges = true;
              break;
            }
          }
          if (hasChanges) break;
        }
        return hasChanges ? origMap : prevOrigMap;
      });
      originalValuesRef.current = origMap;

      if (remoteUpdated.size > 0) {
        setRemoteUpdatedRows((prev) => {
          const next = new Set(prev);
          remoteUpdated.forEach((id) => next.add(id));
          return next;
        });
        remoteUpdated.forEach((id) => {
          scheduleRemoteUpdateClear(id);
        });
      }

      if (conflictingDirty.size > 0) {
        setConflictRows(conflictingDirty);
      } else {
        setConflictRows((prev) => (prev.size === 0 ? prev : new Map()));
      }
      // Mark initial load as successful
      markLastUpdate();
      if (!initialLoadDone) {
        setInitialLoadSuccess(true);
        setInitialLoadDone(true);
        setLoadError(null);
        setLoading(false);
      }
    } catch (e) {
      const errorMsg = e instanceof Error ? e.message : String(e);
      console.error('[params] Failed to load:', errorMsg, e);

      if (!initialLoadDone) {
        // Initial load failed - show error and don't enable autorefresh
        setLoadError(errorMsg);
        toast.error(`Failed to load parameters: ${errorMsg}`);
        setInitialLoadSuccess(false);
        setInitialLoadDone(true);
        setLoading(false);
      } else {
        // Autorefresh failed - log but don't show intrusive error
        console.warn('[params] Autorefresh failed, will retry on next interval');
      }
    }
  }, [initialLoadDone, schema, markLastUpdate]); // dirtyParams removed - use dirtyRef instead

  // Initial load on component mount
  useEffect(() => {
    loadData();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []); // Empty deps = run once on mount

  // Auto-refresh with usePolling hook
  // CRITICAL: Only enable autorefresh after initial load succeeds
  // Pauses when: user is editing, has unsaved changes, or initial load hasn't succeeded
  const pollingEnabled = auto && initialLoadSuccess && !hasInputFocus && dirtyParams.size === 0;
  usePolling(loadData, INTERVALS.PARAMS_POLL, pollingEnabled);

  // Timeout safety: prevent infinite spinner on silent failures
  useEffect(() => {
    if (initialLoadSuccess || loadError) return; // Already resolved

    const timeout = setTimeout(() => {
      if (!initialLoadSuccess && !loadError) {
        console.error('[params] Timeout waiting for initial load after 10s');
        setLoadError('Load timeout - check browser console and network tab');
        setInitialLoadDone(true);
      }
    }, 10000); // 10 second timeout

    return () => clearTimeout(timeout);
  }, [initialLoadSuccess, loadError]);

  // Handle param change
  const handleParamChange = useCallback((strategyId: string, paramKey: string, value: string) => {
    const selected = new Set(selectionRef.current);
    const targets =
      selected.size > 1
        ? Array.from(selected)
        : selected.size === 1 && selected.has(strategyId)
        ? Array.from(selected)
        : [strategyId];

    setParamValues(prev => {
      const newMap = new Map(prev);
      targets.forEach((id) => {
        const stratParams = { ...(newMap.get(id) || {}) };
        stratParams[paramKey] = value;
        newMap.set(id, stratParams);
      });
      return newMap;
    });

    setDirtyParams(prev => {
      const newMap = new Map(prev);
      targets.forEach((id) => {
        const stratDirty = new Set(newMap.get(id) || []);
        const original = originalValues.get(id)?.[paramKey] ?? '';

        if (value !== original) {
          stratDirty.add(paramKey);
        } else {
          stratDirty.delete(paramKey);
        }

        if (stratDirty.size > 0) {
          newMap.set(id, stratDirty);
        } else {
          newMap.delete(id);
        }
      });
      return newMap;
    });

    setRemoteUpdatedRows((prev) => {
      if (prev.size === 0) return prev;
      const next = new Set(prev);
      targets.forEach((id) => next.delete(id));
      return next;
    });
  }, [originalValues]);

  // Handle param blur (validate)
  const handleParamBlur = useCallback((strategyId: string, paramKey: string) => {
    if (!schema) return;
    const selected = new Set(selectionRef.current);
    const targets =
      selected.size > 1
        ? Array.from(selected)
        : selected.size === 1 && selected.has(strategyId)
        ? Array.from(selected)
        : [strategyId];

    setErrorParams(prev => {
      const newMap = new Map(prev);

      targets.forEach((id) => {
        const value = paramValues.get(id)?.[paramKey];
        const paramDef = schema.params[paramKey];
        if (!paramDef || value === undefined) return;

        const result = validateParam(paramKey, value, paramDef);
        const stratErrors = { ...(newMap.get(id) || {}) };

        if (!result.valid && result.error) {
          stratErrors[paramKey] = result.error;
        } else {
          delete stratErrors[paramKey];
        }

        if (Object.keys(stratErrors).length > 0) {
          newMap.set(id, stratErrors);
        } else {
          newMap.delete(id);
        }
      });

      return newMap;
    });
  }, [schema, paramValues]);

  // Handle param focus/blur for autorefresh pausing
  const handleParamFocus = useCallback((strategyId: string, paramKey: string, rowIndex: number, _columnIndex: number) => {
    setHasInputFocus(true);
    setLastFocusedCell({ strategyId, paramKey });

    const currentSelection = selectionRef.current;
    if (currentSelection.length === 0 || !currentSelection.includes(strategyId)) {
      const nextSelection = [strategyId];
      selectionRef.current = nextSelection;
      setSelectedStrategies(nextSelection);
    }
    anchorIndexRef.current = rowIndex;
    setAnchorStrategyId(strategyId);
  }, [setLastFocusedCell, setSelectedStrategies]);

  const handleParamBlurForFocus = useCallback(() => {
    setHasInputFocus(false);
    setLastFocusedCell(null);
  }, [setLastFocusedCell]);

  const applyRemoteKeys = useCallback((strategyId: string, keys: string[]) => {
    if (!strategyId || keys.length === 0) return;
    const remote = originalValuesRef.current.get(strategyId);
    if (!remote) return;

    setParamValues(prev => {
      const next = new Map(prev);
      const current = { ...(next.get(strategyId) || {}) };
      keys.forEach((key) => {
        if (remote[key] !== undefined) {
          current[key] = remote[key];
        } else {
          delete current[key];
        }
      });
      next.set(strategyId, current);
      return next;
    });

    setDirtyParams(prev => {
      const next = new Map(prev);
      const stratDirty = new Set(next.get(strategyId) || []);
      keys.forEach((key) => stratDirty.delete(key));
      if (stratDirty.size === 0) {
        next.delete(strategyId);
      } else {
        next.set(strategyId, stratDirty);
      }
      return next;
    });
  }, []);

  const revertStrategies = useCallback((strategyIds: string[]) => {
    if (!strategyIds || strategyIds.length === 0) return;
    const uniqueIds = Array.from(new Set(strategyIds));
    setParamValues(prev => revertParamValues(uniqueIds, originalValuesRef.current, prev));
    setDirtyParams(prev => clearDirtyForStrategies(uniqueIds, prev));
    setErrorParams(prev => clearErrorsForStrategies(uniqueIds, prev));
    setConflictRows(prev => {
      if (prev.size === 0) return prev;
      const next = new Map(prev);
      let changed = false;
      uniqueIds.forEach((id) => {
        if (next.delete(id)) {
          changed = true;
        }
      });
      return changed ? next : prev;
    });
    setRemoteUpdatedRows(prev => {
      if (prev.size === 0) return prev;
      const next = new Set(prev);
      let changed = false;
      uniqueIds.forEach((id) => {
        if (next.delete(id)) {
          changed = true;
        }
      });
      return changed ? next : prev;
    });
  }, []);

  const handleSortToggle = useCallback((key: string) => {
    if (sortState.key !== key) {
      setSortState({ key, direction: 'asc' });
      return;
    }
    if (sortState.direction === 'asc') {
      setSortState({ key, direction: 'desc' });
      return;
    }
    setSortState({ key: null, direction: null });
  }, [sortState, setSortState]);

  const handleClearSort = useCallback(() => {
    clearSort();
  }, [clearSort]);

  const handleRevertRow = useCallback((strategyId: string) => {
    revertStrategies([strategyId]);
  }, [revertStrategies]);

  const handleRevertAll = useCallback(() => {
    const dirtyIds = Array.from(dirtyParams.keys());
    revertStrategies(dirtyIds);
  }, [dirtyParams, revertStrategies]);

  const handleKeepMine = useCallback((strategyId: string) => {
    setConflictRows(prev => {
      if (!prev.has(strategyId)) return prev;
      const next = new Map(prev);
      next.delete(strategyId);
      return next;
    });
    setDiffStrategyId((prev) => (prev === strategyId ? null : prev));
  }, []);

  const handleUseRemote = useCallback((strategyId: string, customKeys?: Iterable<string>) => {
    const conflictSet = customKeys ? Array.from(customKeys) : Array.from(conflictRowsRef.current.get(strategyId) ?? []);
    if (conflictSet.length === 0) return;
    applyRemoteKeys(strategyId, conflictSet);
    setConflictRows(prev => {
      if (!prev.has(strategyId)) return prev;
      const next = new Map(prev);
      next.delete(strategyId);
      return next;
    });
    setRemoteUpdatedRows(prev => {
      if (!prev.has(strategyId)) return prev;
      const next = new Set(prev);
      next.delete(strategyId);
      return next;
    });
    const timer = remoteUpdateTimersRef.current.get(strategyId);
    if (timer) {
      clearTimeout(timer);
      remoteUpdateTimersRef.current.delete(strategyId);
    }
    setDiffStrategyId((prev) => (prev === strategyId ? null : prev));
  }, [applyRemoteKeys]);

  const handleOpenDiff = useCallback((strategyId: string) => {
    setDiffStrategyId(strategyId);
  }, []);

  const handleResetColumns = useCallback(() => {
    persistColumnOrder([...defaultColumnOrder]);
    resetColumnVisibility();
    setDraggingKey(null);
    setDragTarget(null);
  }, [defaultColumnOrder, persistColumnOrder, resetColumnVisibility]);

  const handleColumnDragStart = useCallback((key: string, event: ReactDragEvent<HTMLElement>) => {
    if (!customizeColumns) return;
    setDraggingKey(key);
    setDragTarget(null);
    if (event.dataTransfer) {
      event.dataTransfer.effectAllowed = 'move';
      try {
        event.dataTransfer.setData('text/plain', key);
      } catch {
        // Ignore quota-related failures
      }
    }
  }, [customizeColumns]);

  const handleColumnDragOver = useCallback((key: string, event: ReactDragEvent<HTMLElement>) => {
    if (!customizeColumns || !draggingKey) return;
    event.preventDefault();

    if (draggingKey === key) {
      if (event.dataTransfer) {
        event.dataTransfer.dropEffect = 'none';
      }
      setDragTarget(null);
      return;
    }

    const rect = (event.currentTarget as HTMLElement).getBoundingClientRect();
    const position: DragPosition = event.clientX - rect.left > rect.width / 2 ? 'after' : 'before';

    setDragTarget(prev => {
      if (prev && prev.key === key && prev.position === position) {
        return prev;
      }
      return { key, position };
    });

    if (event.dataTransfer) {
      event.dataTransfer.dropEffect = 'move';
    }
  }, [customizeColumns, draggingKey]);

  const handleColumnDragLeave = useCallback((key: string, event: ReactDragEvent<HTMLElement>) => {
    if (!customizeColumns) return;
    const related = event.relatedTarget as Node | null;
    if (related && (event.currentTarget as HTMLElement).contains(related)) {
      return;
    }
    setDragTarget(prev => (prev && prev.key === key ? null : prev));
  }, [customizeColumns]);

  const handleColumnDrop = useCallback((key: string, event: ReactDragEvent<HTMLElement>) => {
    if (!customizeColumns) return;
    event.preventDefault();
    event.stopPropagation();

    const source = (event.dataTransfer && event.dataTransfer.getData('text/plain')) || draggingKey;
    if (!source) {
      setDragTarget(null);
      setDraggingKey(null);
      return;
    }

    const rect = (event.currentTarget as HTMLElement).getBoundingClientRect();
    const position: DragPosition = event.clientX - rect.left > rect.width / 2 ? 'after' : 'before';

    const baseOrder = columnOrder && columnOrder.length > 0 ? columnOrder : defaultColumnOrder;
    const nextOrder = moveColumn(baseOrder, source, key, position);
    if (!arraysShallowEqual(baseOrder, nextOrder)) {
      persistColumnOrder(nextOrder);
    }

    setDragTarget(null);
    setDraggingKey(null);
  }, [customizeColumns, draggingKey, columnOrder, defaultColumnOrder, persistColumnOrder]);

  const handleColumnDragEnd = useCallback(() => {
    setDraggingKey(null);
    setDragTarget(null);
  }, []);

  const dragStateForKey = (key: string): DragState => {
    if (draggingKey === key) return 'dragging';
    if (dragTarget && dragTarget.key === key) {
      return dragTarget.position === 'after' ? 'over-after' : 'over-before';
    }
    return 'idle';
  };

  const runAfterFrame = useCallback((fn: () => void) => {
    if (typeof window !== 'undefined' && typeof window.requestAnimationFrame === 'function') {
      window.requestAnimationFrame(fn);
    } else {
      setTimeout(fn, 0);
    }
  }, []);

  const focusCellAt = useCallback((rowIndex: number, columnIndex: number) => {
    rowVirtualizerRef.current?.scrollToIndex(rowIndex, { align: 'auto' });
    runAfterFrame(() => {
      const table = tableRef.current;
      if (!table) return;
      const selector = `[data-row="${rowIndex}"][data-col="${columnIndex}"]`;
      table.querySelector<HTMLElement>(selector)?.focus();
    });
  }, [runAfterFrame]);

  const focusParamCell = useCallback((strategyId: string, paramKey: string) => {
    const strategyIndex = strategyIndexRef.current.get(strategyId);
    if (strategyIndex !== undefined) {
      rowVirtualizerRef.current?.scrollToIndex(strategyIndex, { align: 'auto' });
    }
    runAfterFrame(() => {
      const table = tableRef.current;
      if (!table) return;
      const selector = `[data-strategy="${strategyId}"][data-param="${paramKey}"]`;
      table.querySelector<HTMLElement>(selector)?.focus();
    });
  }, [runAfterFrame]);

  // Save single strategy
  const saveStrategy = useCallback(async (strategyId: string): Promise<void> => {
    const params = paramValues.get(strategyId);
    if (!params || !schema) return;

    // Validate all params before save
    const dirtyKeys = Array.from(dirtyParams.get(strategyId) || []);
    const paramsToSave: Record<string, string> = {};
    dirtyKeys.forEach(key => {
      paramsToSave[key] = params[key];
    });

    const validation = validateParams(paramsToSave, schema);
    if (!validation.valid) {
      const firstError = Object.values(validation.errors)[0];
      toast.error(`Validation failed: ${firstError}`);
      const firstErrorKey = Object.keys(validation.errors)[0];
      if (firstErrorKey) {
        focusParamCell(strategyId, firstErrorKey);
      }
      return;
    }

    setSaving(prev => new Set(prev).add(strategyId));

    try {
      await api.patchStrategyParams(strategyId, paramsToSave, 'fluxboard');

      // Clear dirty and errors on success
      setDirtyParams(prev => {
        const newMap = new Map(prev);
        newMap.delete(strategyId);
        return newMap;
      });

      setErrorParams(prev => {
        const newMap = new Map(prev);
        newMap.delete(strategyId);
        return newMap;
      });

      // Update original values
      setOriginalValues(prev => {
        const newMap = new Map(prev);
        newMap.set(strategyId, { ...params });
        return newMap;
      });

      setFlashingRows(prev => new Set(prev).add(strategyId));
      scheduleFlashClear(strategyId);

      // Auto-clear selection after successful save
      clearSelection();

      bumpGlobalResync('params-save');
      toast.success(`Saved ${strategyId}`);
    } catch (e) {
      const msg = e instanceof Error ? e.message : String(e);
      console.error(`[params] Save failed for ${strategyId}:`, msg);
      toast.error(`Failed to save ${strategyId}: ${msg}`);
      throw e;
    } finally {
      setSaving(prev => {
        const newSet = new Set(prev);
        newSet.delete(strategyId);
        return newSet;
      });
    }
  }, [paramValues, schema, dirtyParams, scheduleFlashClear, clearSelection, focusParamCell]);

  type BulkUpdate = { strategy_id: string; params: Record<string, string> };

  const collectBulkUpdates = useCallback(
    (strategyIds: string[]): BulkUpdate[] => {
      const updates: BulkUpdate[] = [];
      strategyIds.forEach((strategyId) => {
        const stratParams = paramValues.get(strategyId);
        if (!stratParams) return;
        const dirtyKeys = Array.from(dirtyParams.get(strategyId) || []);
        if (dirtyKeys.length === 0) return;

        const paramsToSave: Record<string, string> = {};
        dirtyKeys.forEach((key) => {
          if (stratParams[key] !== undefined) {
            paramsToSave[key] = stratParams[key];
          }
        });

        if (Object.keys(paramsToSave).length > 0) {
          updates.push({ strategy_id: strategyId, params: paramsToSave });
        }
      });
      return updates;
    },
    [dirtyParams, paramValues]
  );

  const ensureNoValidationErrors = useCallback(
    (strategyIds: string[]) => {
      for (const strategyId of strategyIds) {
        const errors = errorParams.get(strategyId);
        if (errors && Object.keys(errors).length > 0) {
          const firstErrorKey = Object.keys(errors)[0];
          if (firstErrorKey) {
            focusParamCell(strategyId, firstErrorKey);
          }
          toast.error('Fix validation errors before saving');
          return false;
        }
      }
      return true;
    },
    [errorParams, focusParamCell]
  );

  const performBulkSave = useCallback(
    async (updates: BulkUpdate[]): Promise<{ allSuccessful: boolean }> => {
      if (updates.length === 0) {
        toast.error('Nothing to save');
        return { allSuccessful: false };
      }

      setSaveAllProgress({ completed: 0, failed: 0, total: updates.length });
      setSaving((prev) => {
        const next = new Set(prev);
        updates.forEach(({ strategy_id }) => next.add(strategy_id));
        return next;
      });

      try {
        const result = await api.updateParams(updates, 'fluxboard');
        const failedIds = new Set(result.errors?.map((entry) => entry.strategy_id) || []);
        const successfulUpdates = updates.filter(({ strategy_id }) => !failedIds.has(strategy_id));

        setSaveAllProgress({
          completed: successfulUpdates.length,
          failed: failedIds.size,
          total: updates.length
        });

        if (successfulUpdates.length > 0) {
          const successIds = new Set(successfulUpdates.map(({ strategy_id }) => strategy_id));

          setDirtyParams((prev) => {
            const next = new Map(prev);
            successIds.forEach((id) => next.delete(id));
            return next;
          });

          setErrorParams((prev) => {
            const next = new Map(prev);
            successIds.forEach((id) => next.delete(id));
            return next;
          });

          setOriginalValues((prev) => {
            const next = new Map(prev);
            successfulUpdates.forEach(({ strategy_id }) => {
              const current = paramValues.get(strategy_id);
              if (current) {
                next.set(strategy_id, { ...current });
              }
            });
            return next;
          });

          setFlashingRows((prev) => {
            const next = new Set(prev);
            successfulUpdates.forEach(({ strategy_id }) => next.add(strategy_id));
            return next;
          });
          successfulUpdates.forEach(({ strategy_id }) => scheduleFlashClear(strategy_id));
          bumpGlobalResync('params-save');
        }

        if (failedIds.size > 0) {
          const errorSummary = (result.errors || [])
            .map((entry) => `${entry.strategy_id}: ${entry.error}`)
            .join('; ');
          toast.error(`Failed to save ${failedIds.size} strategies: ${errorSummary}`);
        }

        if (failedIds.size === 0) {
          toast.success(`Saved all ${updates.length} strategies`);
        } else if (updates.length !== failedIds.size) {
          toast.success(`Saved ${updates.length - failedIds.size} strategies`);
        }

        return { allSuccessful: failedIds.size === 0 };
      } catch (error) {
        const msg = error instanceof Error ? error.message : String(error);
        console.error('[save-all] Bulk save failed:', msg);
        toast.error(`Failed to save strategies: ${msg}`);
        return { allSuccessful: false };
      } finally {
        setSaveAllProgress(null);
        setSaving((prev) => {
          const next = new Set(prev);
          updates.forEach(({ strategy_id }) => next.delete(strategy_id));
          return next;
        });
      }
    },
    [paramValues, scheduleFlashClear]
  );

  // Save All with bounded concurrency
  const handleSaveAll = useCallback(async () => {
    const dirtyIds = Array.from(dirtyParams.keys());
    if (dirtyIds.length === 0) return;

    if (!ensureNoValidationErrors(dirtyIds)) {
      return;
    }

    const updates = collectBulkUpdates(dirtyIds);
    await performBulkSave(updates);
  }, [dirtyParams, ensureNoValidationErrors, collectBulkUpdates, performBulkSave]);

  const saveAllSelected = useCallback(async () => {
    const targetIds = selectedStrategies.filter((id) => dirtyParams.has(id));
    if (targetIds.length === 0) {
      toast.error('No dirty params in selection');
      return;
    }
    if (!ensureNoValidationErrors(targetIds)) {
      return;
    }
    const updates = collectBulkUpdates(targetIds);
    if (updates.length === 0) {
      toast.error('No dirty params in selection');
      return;
    }
    const result = await performBulkSave(updates);
    // Auto-clear selection only after fully successful save
    if (result.allSuccessful) {
      clearSelection();
    }
  }, [selectedStrategies, dirtyParams, ensureNoValidationErrors, collectBulkUpdates, performBulkSave, clearSelection]);

  const handleRefresh = useCallback(async () => {
    setRefreshing(true);
    try {
      await loadData();
      markLastUpdate();
    } catch (e) {
      if (import.meta.env?.DEV) {
        console.error('[params] Refresh failed:', e);
      }
    } finally {
      setRefreshing(false);
    }
  }, [loadData, markLastUpdate]);

  // Navigation warning
  useEffect(() => {
    const handleBeforeUnload = (e: BeforeUnloadEvent) => {
      if (dirtyParams.size > 0) {
        e.preventDefault();
        e.returnValue = '';
      }
    };

    window.addEventListener('beforeunload', handleBeforeUnload);
    return () => window.removeEventListener('beforeunload', handleBeforeUnload);
  }, [dirtyParams]);

  // Get ordered param defs
  const orderedParamDefs = useMemo(() => {
    if (!schema) return [];
    const order = columnOrder && columnOrder.length > 0 ? columnOrder : defaultColumnOrder;
    const appliesToFilters = (PROFILE_TO_APPLIES_TO[activeProfile] || []).map((value) =>
      value.trim().toLowerCase()
    );
    const hasAppliesToFilters = appliesToFilters.length > 0;
    return order
      .map((key) => schema.params[key])
      .filter((def): def is ParamDef => Boolean(def))
      .filter((def) => !def.deprecated)
      .filter((def) => def.key !== 'bot_on')
      .filter((def) => !profileHiddenKeySet.has(def.key))
      .filter((def) => {
        const appliesTo = Array.isArray(def.applies_to)
          ? def.applies_to
              .map((value) => String(value || '').trim().toLowerCase())
              .filter(Boolean)
          : [];
        if (appliesTo.length === 0) return true;
        if (profilePriorityKeySet.has(def.key) || profileStrategyKeySet.has(def.key)) return true;
        if (!hasAppliesToFilters) return false;
        return appliesToFilters.some((candidate) => appliesTo.includes(candidate));
      })
      .filter((def) => {
        const forcedVisibility = columnPrefs.visibility?.[def.key];
        if (forcedVisibility === false) return false;
        if (viewMode === 'compact' && COMPACT_HIDDEN_KEYS.has(def.key) && forcedVisibility !== true) {
          return false;
        }
        return true;
      });
  }, [
    schema,
    columnOrder,
    defaultColumnOrder,
    columnPrefs.visibility,
    profileHiddenKeySet,
    profilePriorityKeySet,
    profileStrategyKeySet,
    viewMode,
    activeProfile,
  ]);
  const familyScopedStrategies = useMemo(
    () => strategies.filter((strategy) => deriveStrategyProfile(strategy) === activeProfile),
    [strategies, activeProfile]
  );

  // Sorted strategies list (applies when a sort key is active)
  const visibleStrategies = useMemo(() => {
    // Apply interactive filters on top of family-scoped rows.
    let filtered = familyScopedStrategies;

    // Strategy search filter (searches strategy ID and parameter values)
    const strategyQuery = filterValues.strategy?.trim();
    if (strategyQuery) {
      const query = strategyQuery.toLowerCase();
      filtered = filtered.filter(s => {
        // Search strategy ID
        if (s.strategy_id.toLowerCase().includes(query)) return true;

        // Search parameter values
        const params = paramValues.get(s.strategy_id);
          if (params) {
            // Search through all parameter values
            for (const [key, value] of Object.entries(params)) {
              if (String(value ?? '').toLowerCase().includes(query)) return true;

              // Also search parameter labels (user-friendly names)
              if (schema) {
                const paramDef = schema.params[key];
              if (paramDef && paramDef.label.toLowerCase().includes(query)) return true;
            }
          }
        }

        return false;
      });
    }

    // Status filter
    const statusValue = filterValues.status;
    if (statusValue === 'Running') {
      filtered = filtered.filter((s) => s.running === true);
    } else if (statusValue === 'Stopped') {
      filtered = filtered.filter((s) => s.running === false || s.running === null || s.running === undefined);
    }

    // Dirty params filter
    if (filterValues.dirty === 'Yes') {
      filtered = filtered.filter(s => {
        const dirty = dirtyParams.get(s.strategy_id);
        return dirty && dirty.size > 0;
      });
    }

    // Classification filters (static metadata from configs/strategies.ini)
    const classFilter = filterValues.class;
    if (classFilter) {
      const needle = classFilter.toLowerCase();
      filtered = filtered.filter((s) => (s.meta?.class || '').toLowerCase() === needle);
    }

    const venueFilter = filterValues.venue_prefix;
    if (venueFilter) {
      const needle = venueFilter.toLowerCase();
      filtered = filtered.filter((s) => (s.meta?.venue_prefix || '').toLowerCase() === needle);
    }

    const chainFilter = filterValues.chain;
    if (chainFilter) {
      const needle = chainFilter.toLowerCase();
      filtered = filtered.filter((s) => (s.meta?.chain || '').toLowerCase() === needle);
    }

    // Apply sorting
    if (!sortState.key || !sortState.direction) return filtered;
    const copy = [...filtered];

    copy.sort((a, b) => {
      if (sortState.key === SORT_KEYS.STRATEGY) {
        const result = a.strategy_id.localeCompare(b.strategy_id);
        return sortState.direction === 'asc' ? result : -result;
      }

      if (sortState.key === SORT_KEYS.TRADING) {
        const valueForTrading = (row: StrategyRow) => {
          // Use persisted value when Trading toggle is dirty to avoid rows jumping mid-edit
          const dirty = dirtyParams.get(row.strategy_id);
          const sourceParams =
            dirty?.has('bot_on')
              ? (originalValues.get(row.strategy_id) || row.params || EMPTY_PARAMS)
              : (paramValues.get(row.strategy_id) || row.params || EMPTY_PARAMS);
          const trading = sourceParams['bot_on'] ?? row.params?.bot_on ?? '0';
          return trading === '1' ? 1 : 0;
        };
        const diff = valueForTrading(a) - valueForTrading(b);
        if (diff === 0) {
          return a.strategy_id.localeCompare(b.strategy_id);
        }
        return sortState.direction === 'asc' ? diff : -diff;
      }

      if (!schema || !sortState.key) return 0;
      const paramDef = schema.params[sortState.key];
      if (!paramDef) return 0;

      const aParams = paramValues.get(a.strategy_id) || EMPTY_PARAMS;
      const bParams = paramValues.get(b.strategy_id) || EMPTY_PARAMS;
      const aVal = aParams[sortState.key];
      const bVal = bParams[sortState.key];

      const missing = compareMissingValues(aVal, bVal);
      if (missing !== null) {
        return missing;
      }

      let result = 0;
      switch (paramDef.type) {
        case 'int':
        case 'float': {
          const aNum = Number(aVal);
          const bNum = Number(bVal);
          const aValid = Number.isFinite(aNum);
          const bValid = Number.isFinite(bNum);
          if (!aValid && !bValid) {
            result = 0;
          } else if (!aValid) {
            result = 1;
          } else if (!bValid) {
            result = -1;
          } else if (aNum === bNum) {
            result = 0;
          } else {
            result = aNum > bNum ? 1 : -1;
          }
          break;
        }
        case 'bool': {
          const aBool = normalizeBoolForSort(aVal!);
          const bBool = normalizeBoolForSort(bVal!);
          if (aBool === bBool) {
            result = 0;
          } else {
            result = aBool > bBool ? 1 : -1;
          }
          break;
        }
        default: {
          result = String(aVal).localeCompare(String(bVal), undefined, { sensitivity: 'base' });
        }
      }

      if (result === 0) {
        result = a.strategy_id.localeCompare(b.strategy_id);
      }

      return sortState.direction === 'asc' ? result : -result;
    });

    return copy;
  }, [familyScopedStrategies, sortState, schema, paramValues, filterValues, dirtyParams, originalValues]);

  const bulkTargetIds = useMemo(() => visibleStrategies.map((s) => s.strategy_id), [visibleStrategies]);
  const bulkFocusOn = useCallback((paramKey?: string) => {
    setHasInputFocus(true);
    if (paramKey) setBulkActiveParam(paramKey);
  }, []);

  const bulkFocusOff = useCallback(() => {
    setHasInputFocus(false);
    setBulkActiveParam(null);
  }, []);

  const setBulkDraft = useCallback((paramKey: string, value: string) => {
    setBulkDrafts((prev) => ({ ...prev, [paramKey]: value }));
    setBulkActiveParam(paramKey);
  }, []);

  const applyBulkDraft = useCallback((paramKey: string, overrideValue?: string) => {
    if (!schema) return;
    const paramDef = schema.params[paramKey];
    if (!paramDef) return;
    if (bulkTargetIds.length === 0) return;

    const draftValue = overrideValue ?? bulkDrafts[paramKey] ?? '';
    const prevValues: Record<string, string | undefined> = {};

    bulkTargetIds.forEach((id) => {
      const currentVal = paramValues.get(id)?.[paramKey];
      prevValues[id] = currentVal;
    });

    setParamValues((prev) => {
      const next = new Map(prev);
      bulkTargetIds.forEach((id) => {
        const current = { ...(next.get(id) || {}) };
        current[paramKey] = draftValue;
        next.set(id, current);
      });
      return next;
    });

    setDirtyParams((prev) => {
      const next = new Map(prev);
      bulkTargetIds.forEach((id) => {
        const stratDirty = new Set(next.get(id) || []);
        const original = originalValues.get(id)?.[paramKey] ?? '';
        if (draftValue !== original) {
          stratDirty.add(paramKey);
        } else {
          stratDirty.delete(paramKey);
        }
        if (stratDirty.size > 0) {
          next.set(id, stratDirty);
        } else {
          next.delete(id);
        }
      });
      return next;
    });

    setErrorParams((prev) => {
      const next = new Map(prev);
      bulkTargetIds.forEach((id) => {
        const value = draftValue;
        const result = validateParam(paramKey, value, paramDef);
        const stratErrors = { ...(next.get(id) || {}) };
        if (!result.valid && result.error) {
          stratErrors[paramKey] = result.error;
        } else {
          delete stratErrors[paramKey];
        }
        if (Object.keys(stratErrors).length > 0) {
          next.set(id, stratErrors);
        } else {
          next.delete(id);
        }
      });
      return next;
    });

    setRemoteUpdatedRows((prev) => {
      if (prev.size === 0) return prev;
      const next = new Set(prev);
      bulkTargetIds.forEach((id) => next.delete(id));
      return next;
    });

    const op: BulkChangeOp = {
      columnKey: paramKey,
      affectedIds: [...bulkTargetIds],
      previousValues: prevValues,
      newValue: draftValue,
      undoable: true,
    };
    setLastBulkChangeOp(op);
    setBulkActiveParam(null);

    toast.success('Bulk change applied', {
      description: `Updated \"${paramDef.label}\" for ${bulkTargetIds.length} strategies.`,
      action: {
        label: 'Undo',
        onClick: () => {
          setLastBulkChangeOp((prev) => prev ?? op);
          void triggerUndoBulkChange();
        }
      }
    });
  }, [schema, bulkTargetIds, bulkDrafts, paramValues, originalValues]);

  const triggerUndoBulkChange = useCallback(async () => {
    if (!lastBulkChangeOp || !lastBulkChangeOp.undoable) return;
    setUndoInFlight(true);
    const { columnKey, affectedIds, previousValues } = lastBulkChangeOp;

    setParamValues((prev) => {
      const next = new Map(prev);
      affectedIds.forEach((id) => {
        const current = { ...(next.get(id) || {}) };
        const prevVal = previousValues[id];
        if (prevVal === undefined) {
          delete current[columnKey];
        } else {
          current[columnKey] = prevVal;
        }
        next.set(id, current);
      });
      return next;
    });

    setDirtyParams((prev) => {
      const next = new Map(prev);
      affectedIds.forEach((id) => {
        const stratDirty = new Set(next.get(id) || []);
        const original = originalValues.get(id)?.[columnKey] ?? '';
        const prevVal = previousValues[id] ?? '';
        if (prevVal !== original) {
          stratDirty.add(columnKey);
        } else {
          stratDirty.delete(columnKey);
        }
        if (stratDirty.size > 0) {
          next.set(id, stratDirty);
        } else {
          next.delete(id);
        }
      });
      return next;
    });

    if (schema?.params[columnKey]) {
      const paramDef = schema.params[columnKey];
      setErrorParams((prev) => {
        const next = new Map(prev);
        affectedIds.forEach((id) => {
          const value = previousValues[id];
          const result = validateParam(columnKey, value ?? '', paramDef);
          const stratErrors = { ...(next.get(id) || {}) };
          if (!result.valid && result.error) {
            stratErrors[columnKey] = result.error;
          } else {
            delete stratErrors[columnKey];
          }
          if (Object.keys(stratErrors).length > 0) {
            next.set(id, stratErrors);
          } else {
            next.delete(id);
          }
        });
        return next;
      });
    }

    setLastBulkChangeOp((prev) => (prev ? { ...prev, undoable: false } : prev));
    setUndoInFlight(false);
    toast.success('Bulk change undone');
  }, [lastBulkChangeOp, originalValues, schema]);

  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      const isMac = typeof navigator !== 'undefined' && navigator.platform.toLowerCase().includes('mac');
      const modifier = isMac ? event.metaKey : event.ctrlKey;
      if (!modifier || event.key.toLowerCase() !== 'z') return;

      const active = document.activeElement;
      const tag = active?.tagName?.toLowerCase();
      const isTextField = tag === 'input' || tag === 'textarea' || active?.getAttribute('contenteditable') === 'true';
      if (isTextField) return; // let native undo inside inputs

      if (lastBulkChangeOp && lastBulkChangeOp.undoable && !undoInFlight) {
        event.preventDefault();
        void triggerUndoBulkChange();
      }
    };

    window.addEventListener('keydown', onKeyDown);
    return () => window.removeEventListener('keydown', onKeyDown);
  }, [lastBulkChangeOp, undoInFlight, triggerUndoBulkChange]);

  const strategyIndexMap = useMemo(() => {
    const map = new Map<string, number>();
    visibleStrategies.forEach((strategy, index) => {
      map.set(strategy.strategy_id, index);
    });
    return map;
  }, [visibleStrategies]);

  useEffect(() => {
    strategyIndexRef.current = strategyIndexMap;
  }, [strategyIndexMap]);

  const rowVirtualizer = useVirtualizer<HTMLDivElement, HTMLTableRowElement>({
    count: visibleStrategies.length,
    getScrollElement: () => scrollContainerRef.current,
    estimateSize: () => DENSE_ROW_HEIGHT,
    overscan: 8,
  });
  rowVirtualizerRef.current = rowVirtualizer;

  const virtualRows = rowVirtualizer.getVirtualItems();
  const fallbackVirtualization = virtualRows.length === 0 && visibleStrategies.length > 0;
  const rowsToRender = fallbackVirtualization
    ? visibleStrategies.map((_, index) => ({ index, start: 0, end: 0 }))
    : virtualRows;
  const paddingTop = fallbackVirtualization || rowsToRender.length === 0 ? 0 : rowsToRender[0]!.start ?? 0;
  const paddingBottom =
    fallbackVirtualization || rowsToRender.length === 0
      ? 0
      : rowVirtualizer.getTotalSize() - rowsToRender[rowsToRender.length - 1]!.end!;



  useEffect(() => {
    if (!anchorStrategyId) {
      anchorIndexRef.current = null;
      return;
    }
    const idx = visibleStrategies.findIndex((row) => row.strategy_id === anchorStrategyId);
    anchorIndexRef.current = idx >= 0 ? idx : null;
  }, [visibleStrategies, anchorStrategyId]);

  useEffect(() => {
    if (selectedStrategies.length === 0) return;
    const existingIds = new Set(familyScopedStrategies.map((row) => row.strategy_id));
    const filtered = selectedStrategies.filter((id) => existingIds.has(id));
    if (filtered.length !== selectedStrategies.length) {
      selectionRef.current = filtered;
      if (filtered.length === 0) {
        clearSelection();
      } else {
        setSelectedStrategies(filtered);
      }
    }
  }, [familyScopedStrategies, selectedStrategies, setSelectedStrategies, clearSelection]);

  useEffect(() => {
    if (visibleStrategies.length !== 1) {
      didAutoSelectSingleRef.current = false;
      return;
    }
    if (selectedStrategies.length > 0 || didAutoSelectSingleRef.current) {
      return;
    }
    const onlyStrategyId = visibleStrategies[0]?.strategy_id;
    if (!onlyStrategyId) {
      return;
    }
    selectionRef.current = [onlyStrategyId];
    setSelectedStrategies([onlyStrategyId]);
    setAnchorStrategyId(onlyStrategyId);
    anchorIndexRef.current = 0;
    didAutoSelectSingleRef.current = true;
  }, [visibleStrategies, selectedStrategies, setSelectedStrategies, setAnchorStrategyId]);

  useEffect(() => {
    const handleMouseUp = () => {
      if (dragSelectingRef.current) {
        dragSelectingRef.current = false;
      }
    };
    window.addEventListener('mouseup', handleMouseUp);
    return () => window.removeEventListener('mouseup', handleMouseUp);
  }, []);

  const handleRowMouseDown = useCallback(
    (strategyId: string, rowIndex: number, event: ReactMouseEvent<HTMLTableCellElement>) => {
      if (event.button !== 0) return;

      let nextSelection: string[] = [];

      if (event.shiftKey) {
        if (anchorIndexRef.current === null) {
          anchorIndexRef.current = rowIndex;
          setAnchorStrategyId(strategyId);
        }
        const anchor = anchorIndexRef.current ?? rowIndex;
        const start = Math.min(anchor, rowIndex);
        const end = Math.max(anchor, rowIndex);
        nextSelection = visibleStrategies.slice(start, end + 1).map((row) => row.strategy_id);
      } else if (event.metaKey || event.ctrlKey) {
        const current = selectionRef.current;
        if (current.includes(strategyId)) {
          nextSelection = current.filter((id) => id !== strategyId);
        } else {
          nextSelection = [...current, strategyId];
        }
        anchorIndexRef.current = rowIndex;
        setAnchorStrategyId(strategyId);
      } else {
        nextSelection = [strategyId];
        anchorIndexRef.current = rowIndex;
        setAnchorStrategyId(strategyId);
      }

      if (nextSelection.length === 0) {
        selectionRef.current = [];
        clearSelection();
      } else {
        selectionRef.current = nextSelection;
        setSelectedStrategies(nextSelection);
      }

      // Only enable drag-to-select when Shift is held to avoid accidental
      // multi-selection on small pointer movements between rows.
      dragSelectingRef.current = event.shiftKey === true;
    },
    [visibleStrategies, setSelectedStrategies, clearSelection, setAnchorStrategyId]
  );

  const handleRowMouseEnter = useCallback(
    (strategyId: string, rowIndex: number) => {
      if (!dragSelectingRef.current) return;
      if (anchorIndexRef.current === null) {
        anchorIndexRef.current = rowIndex;
        setAnchorStrategyId(strategyId);
      }

      const anchor = anchorIndexRef.current ?? rowIndex;
      const start = Math.min(anchor, rowIndex);
      const end = Math.max(anchor, rowIndex);
      const range = visibleStrategies.slice(start, end + 1).map((row) => row.strategy_id);
      selectionRef.current = range;
      setSelectedStrategies(range);
    },
    [visibleStrategies, setSelectedStrategies, setAnchorStrategyId]
  );

  const handleRowMouseUp = useCallback(() => {
    dragSelectingRef.current = false;
  }, []);

  const strategyColumnWidth = useMemo(() => {
    // Dynamic sizing: keep Strategy IDs readable when multiple maker variants exist.
    // Use a simple monospace estimate; we clamp to keep the grid usable.
    const maxLen = strategies.reduce((acc, s) => Math.max(acc, (s.strategy_id || '').length), 0);
    const pxPerChar = 8.2; // ~13px monospace in this UI
    const paddingPx = 56;  // sort arrow + padding + breathing room
    const estimated = Math.ceil(maxLen * pxPerChar + paddingPx);
    return clampInt(estimated, STRATEGY_COLUMN_MIN_WIDTH, STRATEGY_COLUMN_MAX_WIDTH);
  }, [strategies]);

  const totalColumns = orderedParamDefs.length;
  const totalRows = visibleStrategies.length;
  const totalGridWidth =
    strategyColumnWidth +
    RUN_COLUMN_WIDTH +
    TRADE_COLUMN_WIDTH +
    totalColumns * PARAM_COLUMN_WIDTH;
  const tableWidthPx = `${totalGridWidth}px`;

  const handleCellNavigate = useCallback(
    (rowIndex: number, columnIndex: number, direction: Direction) => {
      if (totalColumns === 0 || totalRows === 0) return;
      let nextRow = rowIndex;
      let nextCol = columnIndex;

      switch (direction) {
        case 'left':
          if (columnIndex > 0) nextCol = columnIndex - 1;
          break;
        case 'right':
          if (columnIndex < totalColumns - 1) nextCol = columnIndex + 1;
          break;
        case 'up':
          if (rowIndex > 0) nextRow = rowIndex - 1;
          break;
        case 'down':
          if (rowIndex < totalRows - 1) nextRow = rowIndex + 1;
          break;
        default:
          break;
      }

      if (nextRow === rowIndex && nextCol === columnIndex) return;
      focusCellAt(nextRow, nextCol);
    },
    [focusCellAt, totalColumns, totalRows]
  );

  const showAdvanced = viewMode === 'full';
  const handleToggleAdvanced = useCallback(() => {
    const nextMode = showAdvanced ? 'compact' : 'full';
    setViewMode(nextMode);
    COMPACT_HIDDEN_KEYS.forEach((key) => {
      setColumnVisibility(key, nextMode === 'compact' ? false : true);
    });
  }, [showAdvanced, setViewMode, setColumnVisibility]);

  const handleClearSelectionToolbar = useCallback(() => {
    selectionRef.current = [];
    clearSelection();
    setAnchorStrategyId(null);
    anchorIndexRef.current = null;
  }, [clearSelection]);


  const selectedSet = useMemo(() => new Set(selectedStrategies), [selectedStrategies]);
  const selectedCount = selectedStrategies.length;
  const selectionAnnouncement = selectedCount === 0
    ? 'No strategies selected'
    : `${selectedCount} ${selectedCount === 1 ? 'strategy' : 'strategies'} selected`;

  const allowColumnDrag = customizeColumns && orderedParamDefs.length > 1;
  const canResetColumns = Boolean(columnOrder && !arraysShallowEqual(columnOrder, defaultColumnOrder));
  const isSortActive = Boolean(sortState.key && sortState.direction);
  const dirtyCount = countDirtyCells(dirtyParams);
  const hasDirtyParams = dirtyCount > 0;
  const autoRefreshActive = pollingEnabled;
  const autoPauseReason: AutoPauseReason | null = auto && !autoRefreshActive
    ? (hasInputFocus
      ? 'editing'
      : hasDirtyParams
        ? 'unsaved'
        : !initialLoadSuccess
          ? 'loading'
          : 'disabled')
    : null;
  const autoPauseLabel = autoPauseReason ? AUTO_PAUSE_LABELS[autoPauseReason] : null;
  const selectedDirtyCount = countDirtyInSelection(dirtyParams, selectedStrategies);
  const isSavingAll = saveAllProgress !== null;
  const hasErrors = Array.from(errorParams.values()).some(errors => Object.keys(errors).length > 0);
  const familyCounts = useMemo<Record<ParamsProfileId, number>>(
    () => strategies.reduce(
      (acc, strategy) => {
        const profile = deriveStrategyProfile(strategy);
        acc[profile] += 1;
        return acc;
      },
      {
        taker: 0,
        maker_v2: 0,
        maker_v3: 0,
      } as Record<ParamsProfileId, number>
    ),
    [strategies]
  );

  const diffEntries = useMemo(() => {
    if (!diffStrategyId) return [];
    const keys = conflictRows.get(diffStrategyId);
    if (!keys || keys.size === 0) return [];
    const local = paramValues.get(diffStrategyId) || EMPTY_PARAMS;
    const remote = originalValues.get(diffStrategyId) || EMPTY_PARAMS;
    return Array.from(keys).map((key) => ({ key, mine: local[key] ?? '', remote: remote[key] ?? '' }));
  }, [diffStrategyId, conflictRows, paramValues, originalValues]);

  const familyFilterControl = useMemo(
    () => (
      <div
        className="flex items-center flex-wrap"
        style={{
          gap: spacing.gap.sm,
          fontSize: typography.fontSize.xs,
          color: colors.text.muted,
        }}
      >
        <label className="flex items-center" style={{ gap: spacing.gap.xs }}>
          <span>Family</span>
          <select
            value={activeProfile}
            onChange={(event) => setActiveProfile(event.target.value as ParamsProfileId)}
            className="rounded border px-2 py-1 bg-bg-surface text-text-primary"
            style={{ borderColor: colors.border.DEFAULT }}
            aria-label="Params family"
          >
            {profileIds.map((profile) => (
              <option key={`params-family-${profile}`} value={profile}>
                {`${getProfileLabel(profile)} (${familyCounts[profile]})`}
              </option>
            ))}
          </select>
        </label>
      </div>
    ),
    [activeProfile, familyCounts, profileIds, setActiveProfile]
  );

  const panelHeaderActions = useMemo(() => {
    if (showHeader) return null;
    return (
      <div className="flex flex-wrap items-center gap-2 text-[11px]">
        <Button
          variant={hasDirtyParams && !isSavingAll && !hasErrors ? 'success' : 'secondary'}
          size="xs"
          onClick={handleSaveAll}
          disabled={!hasDirtyParams || isSavingAll || hasErrors}
          loading={isSavingAll}
        >
          Save All{dirtyCount > 0 && ` (${dirtyCount})`}
        </Button>
        {handleRevertAll && (
          <Button
            variant="warning"
            size="xs"
            onClick={handleRevertAll}
            disabled={!hasDirtyParams}
          >
            Revert All
          </Button>
        )}
        <Button
          variant="secondary"
          size="xs"
          onClick={handleRefresh}
          disabled={loading || refreshing}
          loading={refreshing}
        >
          Refresh
        </Button>
        <Button
          variant={showAdvanced ? 'default' : 'secondary'}
          size="xs"
          onClick={handleToggleAdvanced}
          aria-label="Advanced Parameters"
          aria-pressed={showAdvanced}
        >
          Advanced Params
        </Button>
        <Button
          variant={customizeColumns ? 'default' : 'secondary'}
          size="xs"
          onClick={() => setCustomizeColumns((prev) => !prev)}
        >
          {customizeColumns ? 'Done' : 'Customize'}
        </Button>
        {customizeColumns && (
          <Button
            variant="secondary"
            size="xs"
            onClick={handleResetColumns}
            disabled={!canResetColumns}
          >
            Reset
          </Button>
        )}
        <Button
          variant="secondary"
          size="xs"
          onClick={handleClearSort}
          disabled={!isSortActive}
        >
          Clear Sort
        </Button>
        <label className="flex items-center gap-1">
          <input
            type="checkbox"
            checked={auto}
            onChange={(e) => setAuto(e.target.checked)}
            className="w-3 h-3"
          />
          <span className="font-medium" style={{ color: auto && !autoRefreshActive ? colors.semantic.warning.DEFAULT : colors.text.muted }}>
            Auto
          </span>
        </label>
        {autoPauseLabel && (
          <StatusPill
            status="info"
            label="Auto Paused"
            subLabel={autoPauseLabel}
            size="xs"
            tone="subtle"
          />
        )}
      </div>
    );
  }, [showHeader, handleSaveAll, hasDirtyParams, isSavingAll, hasErrors, dirtyCount, handleRevertAll, handleRefresh, loading, refreshing, handleToggleAdvanced, showAdvanced, customizeColumns, handleResetColumns, canResetColumns, handleClearSort, isSortActive, auto, autoRefreshActive, autoPauseLabel, setAuto]);

  const panelHeaderSlots = usePanelHeaderSlots();

  useEffect(() => {
    if (!panelHeaderSlots) return;
    if (showHeader) {
      panelHeaderSlots.setActions(null);
      panelHeaderSlots.setTitleActions(null);
      return;
    }
    panelHeaderSlots.setActions(panelHeaderActions);
    panelHeaderSlots.setTitleActions(null);
    return () => {
      panelHeaderSlots.setActions(null);
      panelHeaderSlots.setTitleActions(null);
    };
  }, [panelHeaderSlots, panelHeaderActions, showHeader]);

  // Show loading screen only during initial load
  if (loading || (!initialLoadSuccess && !loadError)) {
    return (
      <div className="flex flex-col items-center justify-center h-screen bg-neutral-950 text-neutral-100 gap-4">
        <div className="text-neutral-400">Loading parameters...</div>
        <div className="text-xs text-neutral-500">Ensuring data loads before autorefresh starts</div>
      </div>
    );
  }

  // Show error screen if initial load failed
  if (loadError && !initialLoadSuccess) {
    return (
      <div className="flex flex-col items-center justify-center h-screen bg-neutral-950 text-neutral-100 gap-4">
        <div className="text-red-400 text-lg">Failed to load parameters</div>
        <div className="text-neutral-400 text-sm max-w-md text-center">{loadError}</div>
        <button
          onClick={() => {
            setLoadError(null);
            setInitialLoadDone(false);
            setInitialLoadSuccess(false);
            loadData();
          }}
          className="px-4 py-2 bg-blue-600 hover:bg-blue-700 rounded text-sm"
        >
          Retry
        </button>
      </div>
    );
  }

  // Ensure schema is loaded (defensive check)
  if (!schema) {
    return (
      <div className="flex items-center justify-center h-screen bg-neutral-950 text-neutral-100">
        <div className="text-neutral-400">Schema not loaded</div>
      </div>
    );
  }

  const mobileContent = (
    <div className="flex flex-col h-full" style={{ backgroundColor: colors.bg.base }}>
      <header
        className="sticky top-0 z-20"
        style={{
          backgroundColor: colors.bg.surface,
          borderBottom: `1px solid ${colors.border.DEFAULT}`,
          padding: `${spacing.gap.sm} ${spacing.gap.md}`,
        }}
      >
        <div className="flex flex-wrap items-center gap-2 text-[11px]">
          <Button
            variant={hasDirtyParams && !isSavingAll && !hasErrors ? 'success' : 'secondary'}
            size="sm"
            onClick={handleSaveAll}
            disabled={!hasDirtyParams || isSavingAll || hasErrors}
            loading={isSavingAll}
            className="flex-1"
          >
            Save All{dirtyCount > 0 ? ` (${dirtyCount})` : ''}
          </Button>
          <Button
            variant="warning"
            size="sm"
            onClick={handleRevertAll}
            disabled={!hasDirtyParams}
            className="flex-1"
          >
            Revert All
          </Button>
          <Button
            variant="secondary"
            size="xs"
            onClick={handleRefresh}
            disabled={loading || refreshing}
            loading={refreshing}
          >
            Refresh
          </Button>
          <Button
            variant={showAdvanced ? 'default' : 'secondary'}
            size="xs"
            onClick={handleToggleAdvanced}
            aria-pressed={showAdvanced}
          >
            Advanced
          </Button>
          <Button
            variant="ghost"
            size="xs"
            onClick={handleClearSort}
            disabled={!isSortActive}
          >
            Clear
          </Button>
          <label className="flex items-center gap-1 text-[11px] ml-auto px-1 py-0.5">
            <input
              type="checkbox"
              checked={auto}
              onChange={(e) => setAuto(e.target.checked)}
              className="w-4 h-4"
              aria-label="Auto-refresh params"
            />
            <span style={{ color: colors.text.muted }}>Auto</span>
          </label>
          <Button
            variant="ghost"
            size="xs"
            onClick={() => {
              if (typeof window !== 'undefined') {
                window.location.href = '/params';
              }
            }}
          >
            Full grid
          </Button>
        </div>
        <div className="mt-2 flex flex-wrap items-center gap-1 text-[11px]">
          <span style={{ color: colors.text.muted }}>Open Filters to choose family</span>
        </div>
      </header>

      <div
        className="border-b"
        style={{ borderColor: colors.border.DEFAULT, backgroundColor: colors.bg.surfaceAlt }}
      >
        <button
          type="button"
          onClick={() => setMobileFiltersOpen((open) => !open)}
          className="w-full text-left px-3 py-2 text-sm font-medium"
          style={{
            color: colors.text.secondary,
            backgroundColor: colors.bg.surfaceAlt,
            borderBottom: mobileFiltersOpen ? `1px solid ${colors.border.DEFAULT}` : 'none',
          }}
        >
          Filters
        </button>
        {mobileFiltersOpen && (
          <div className="px-3 pb-3 pt-1" style={{ backgroundColor: colors.bg.surface }}>
            <TableFilter
              columns={filterColumns}
              value={filterValues}
              onFilterChange={setFilterValues}
              dense
              customControls={familyFilterControl}
            />
          </div>
        )}
      </div>

      <div className="flex-1 overflow-y-auto px-3 py-2 space-y-2">
        {visibleStrategies.length === 0 ? (
          <div
            className="text-center text-sm"
            style={{ color: colors.text.muted, padding: spacing.gap.lg }}
          >
            {strategies.length === 0 ? 'No strategies configured' : 'No strategies match filters'}
          </div>
        ) : (
          visibleStrategies.map((strategy) => {
            const stratParams = paramValues.get(strategy.strategy_id) || EMPTY_PARAMS;
            const stratDirty = dirtyParams.get(strategy.strategy_id) || EMPTY_DIRTY_SET;
            const stratErrors = errorParams.get(strategy.strategy_id) || EMPTY_ERRORS;
            const hotList = selectMobileParams(strategy.hot_params || (strategy.meta as any)?.hot_params);
            const tradingValue = stratParams['bot_on'] ?? strategy.params?.bot_on ?? '0';
            const runChecked = tradingValue === '1';

            return (
              <div
                key={strategy.strategy_id}
                className="rounded-lg"
                style={{
                  border: `1px solid ${colors.border.DEFAULT}`,
                  backgroundColor: colors.bg.surface,
                  padding: `${spacing.gap.sm} ${spacing.gap.md}`,
                }}
              >
                <div className="flex items-center justify-between gap-2">
                  <div className="text-xs font-medium truncate" title={strategy.strategy_id}>
                    {strategy.strategy_id}
                  </div>
                  <label
                    className="flex items-center gap-2 text-[11px] px-1 py-0.5"
                    style={{ color: colors.text.muted }}
                  >
                    <span>Run</span>
                    <input
                      type="checkbox"
                      className="w-4 h-4"
                      aria-label={`Run ${strategy.strategy_id}`}
                      checked={runChecked}
                      onChange={(e) => {
                        const value = e.target.checked ? '1' : '0';
                        handleParamChange(strategy.strategy_id, 'bot_on', value);
                        handleParamBlur(strategy.strategy_id, 'bot_on');
                      }}
                    />
                  </label>
                </div>

                <div className="flex flex-wrap gap-2 mt-2">
                  {hotList.map((key) => {
                    const value = stratParams[key] ?? '';
                    const dirty = stratDirty.has(key);
                    const error = stratErrors[key];
                    const label = schema.params[key]?.label || key;
                    return (
                      <div
                        key={key}
                        className="rounded-md flex flex-col gap-1"
                        style={{
                          border: `1px solid ${dirty ? colors.semantic.warning.DEFAULT : colors.border.hover}`,
                          backgroundColor: colors.bg.surfaceAlt,
                          padding: `${spacing.padding.xs} ${spacing.gap.sm}`,
                          minWidth: 120,
                          flex: '1 1 45%',
                        }}
                      >
                        <span
                          className="uppercase"
                          style={{
                            fontSize: typography.fontSize['2xs'],
                            color: colors.text.muted,
                            letterSpacing: '0.04em',
                          }}
                        >
                          {label}
                        </span>
                        <input
                          value={value}
                          onChange={(e) => handleParamChange(strategy.strategy_id, key, e.target.value)}
                          onBlur={() => handleParamBlur(strategy.strategy_id, key)}
                          className="w-full rounded-sm bg-transparent"
                          style={{
                            border: `1px solid ${dirty ? colors.semantic.warning.DEFAULT : colors.border.DEFAULT}`,
                            padding: `${spacing.padding.xs} ${spacing.padding.sm}`,
                            color: colors.text.primary,
                            fontSize: typography.fontSize.sm,
                          }}
                          aria-label={`${label} value for ${strategy.strategy_id}`}
                          inputMode="decimal"
                          autoComplete="off"
                        />
                        {error && (
                          <span style={{ color: colors.semantic.danger.DEFAULT, fontSize: typography.fontSize['2xs'] }}>
                            {error}
                          </span>
                        )}
                      </div>
                    );
                  })}
                </div>
              </div>
            );
          })
        )}
      </div>
    </div>
  );

  const desktopContent = (
    <div className="flex flex-col h-full">
      {showHeader && (
        <ParamsHeader
          hasDirtyParams={hasDirtyParams}
          dirtyCount={dirtyCount}
          isSaving={isSavingAll}
          hasErrors={hasErrors}
          onSaveAll={handleSaveAll}
          onRevertAll={handleRevertAll}
          saveProgress={saveAllProgress ?? undefined}
          advancedMode={showAdvanced}
          onToggleAdvanced={handleToggleAdvanced}
          customizeMode={customizeColumns}
          onToggleCustomize={() => setCustomizeColumns((prev) => !prev)}
          onResetColumns={handleResetColumns}
          canResetColumns={canResetColumns}
          sortState={sortState}
          onClearSort={handleClearSort}
          autoRefresh={auto}
          onToggleAuto={setAuto}
          autoRefreshActive={autoRefreshActive}
          autoRefreshPauseLabel={autoPauseLabel ?? undefined}
          autoRefreshIntervalSec={INTERVALS.PARAMS_POLL / 1000}
          lastFetchedAt={lastUpdate}
          isStale={lastUpdate ? Date.now() - lastUpdate > STALE_THRESHOLDS.FAST : false}
          loading={loading}
          refreshing={refreshing}
          onRefresh={handleRefresh}
        />
      )}
      <span className="sr-only" aria-live="polite">
        {selectionAnnouncement}
      </span>

      {/* Bulk Actions Toolbar (appears when strategies selected) */}
      {selectedCount > 0 && (
        <div
          className="sticky top-12 z-20 flex items-center gap-3 px-4 py-2 border-b"
          style={{
            backgroundColor: colors.bg.surface,
            borderBottomColor: colors.border.DEFAULT,
          }}
        >
          <div
            className="flex items-center gap-2 rounded-full px-3 py-1"
            style={{
              backgroundColor: `${colors.semantic.success.DEFAULT}1a`,
              border: `1px solid ${colors.semantic.success.DEFAULT}55`,
            }}
            title="Shift-click to select range; Ctrl/Cmd-click to toggle rows"
          >
            <span style={{ fontSize: typography.fontSize.sm, color: colors.semantic.success.light, fontWeight: typography.fontWeight.semibold }}>
              {selectedCount} selected
            </span>
            <button
              type="button"
              onClick={handleClearSelectionToolbar}
              className="text-[12px] font-semibold hover:underline"
              style={{
                color: colors.semantic.success.light,
                textUnderlineOffset: '2px',
              }}
            >
              Clear
            </button>
          </div>

          <button
            type="button"
            onClick={saveAllSelected}
            disabled={isSavingAll || selectedDirtyCount === 0}
            className="px-3 py-1.5 rounded-md transition-colors disabled:opacity-50 disabled:cursor-not-allowed"
            style={{
              fontSize: typography.fontSize.sm,
              color: colors.bg.base,
              backgroundColor: colors.semantic.success.DEFAULT,
            }}
          >
            Save Selected
          </button>

          <div className="flex-1" />

          {dirtyCount > 0 && (
            <span style={{ fontSize: typography.fontSize.xs, color: colors.text.muted }}>
              {dirtyCount} unsaved changes
            </span>
          )}
        </div>
      )}

      {/* Filter Controls - using reusable TableFilter component */}
      <TableFilter
        columns={filterColumns}
        value={filterValues}
        onFilterChange={setFilterValues}
        customControls={familyFilterControl}
      />

      {/* Table */}
      {/* Allow vertical scroll inside the page area and horizontal scroll for wide grids. */}
      <PanelBody ref={scrollContainerRef}>
        <table
          ref={tableRef}
          className="table-fixed text-[13px] font-mono terminal-table params-table"
          style={{
            minWidth: tableWidthPx,
            width: tableWidthPx,
            borderCollapse: 'collapse',
          }}
        >
          <thead className="params-thead">
            <tr
              data-testid="params-header-row"
              className="params-header-row"
              ref={headerRowRef}
              style={{
                position: 'sticky',
                top: 0,
                zIndex: 50,
                backgroundColor: '#111214',
              }}
            >
              <th
                className="sticky top-0 z-30 terminal-th text-left backdrop-blur"
                style={{
                  fontSize: typography.fontSize.xs,
                  fontWeight: typography.fontWeight.semibold,
                  color: colors.text.muted,
                  left: PINNED_LEFT_OFFSETS.strategy,
                  width: strategyColumnWidth,
                  minWidth: strategyColumnWidth,
                  zIndex: 60,
                  backgroundColor: '#111214',
                }}
                aria-sort={sortState.key === SORT_KEYS.STRATEGY && sortState.direction ? (sortState.direction === 'asc' ? 'ascending' : 'descending') : 'none'}
              >
                <button
                  type="button"
                  onClick={() => handleSortToggle(SORT_KEYS.STRATEGY)}
                  className="flex items-center gap-1 w-full text-left font-semibold text-text-secondary hover:text-text-primary focus:outline-none focus-visible:ring-1 focus-visible:ring-border-focus rounded-[2px]"
                  aria-label="Sort by strategy ID"
                  title="Sort by strategy ID"
                >
                  <span>Strategy</span>
                  <span className="text-text-muted">
                    {sortState.key === SORT_KEYS.STRATEGY && sortState.direction
                      ? (sortState.direction === 'asc' ? '↑' : '↓')
                      : '↕'}
                  </span>
                </button>
              </th>
              <th
                className="terminal-th text-center whitespace-nowrap"
                style={{
                  fontSize: typography.fontSize.xs,
                  fontWeight: typography.fontWeight.semibold,
                  color: colors.text.muted,
                  width: RUN_COLUMN_WIDTH,
                  minWidth: RUN_COLUMN_WIDTH,
                  backgroundColor: '#111214',
                }}
              >
                Run
              </th>
              <th
                className="terminal-th text-left whitespace-nowrap"
                style={{
                  fontSize: typography.fontSize.xs,
                  fontWeight: typography.fontWeight.semibold,
                  color: colors.text.muted,
                  width: TRADE_COLUMN_WIDTH,
                  minWidth: TRADE_COLUMN_WIDTH,
                  backgroundColor: '#111214',
                }}
                aria-sort={
                  sortState.key === SORT_KEYS.TRADING && sortState.direction
                    ? sortState.direction === 'asc'
                      ? 'ascending'
                      : 'descending'
                    : 'none'
                }
              >
                <button
                  type="button"
                  onClick={() => handleSortToggle(SORT_KEYS.TRADING)}
                  className="flex items-center gap-1 font-semibold text-text-secondary hover:text-text-primary focus:outline-none focus-visible:ring-1 focus-visible:ring-border-focus rounded-[2px]"
                  aria-label="Sort by trading toggle"
                  title="Sort by trading toggle"
                >
                  <span>Trading</span>
                  <span className="text-text-muted">
                    {sortState.key === SORT_KEYS.TRADING && sortState.direction
                      ? (sortState.direction === 'asc' ? '↑' : '↓')
                      : '↕'}
                  </span>
                </button>
              </th>
              {/* Render headers for all columns in orderedParamDefs (respects view mode) */}
              {orderedParamDefs.map((paramDef, columnIndex) => {
                const sortActive = sortState.key === paramDef.key;
                const group = getParamGroup(paramDef.key);
                const prevGroup = columnIndex > 0 ? getParamGroup(orderedParamDefs[columnIndex - 1]?.key) : null;
                const dividerClass =
                  columnIndex === 0 || group !== prevGroup ? 'border-l' : '';
                const headerHint = HEADER_HINTS[paramDef.key];
                return (
                  <HeaderWithHelp
                    key={paramDef.key}
                    paramDef={paramDef}
                    onModalOpen={() => setHelpModalParam(paramDef.key)}
                    sortable
                    sortActive={sortActive}
                    sortDirection={sortActive ? sortState.direction : null}
                    onSortToggle={() => handleSortToggle(paramDef.key)}
                    dragEnabled={allowColumnDrag}
                    dragState={dragStateForKey(paramDef.key)}
                    onDragStart={(event) => handleColumnDragStart(paramDef.key, event as ReactDragEvent<HTMLElement>)}
                    onDragEnter={(event) => handleColumnDragOver(paramDef.key, event as ReactDragEvent<HTMLElement>)}
                    onDragOver={(event) => handleColumnDragOver(paramDef.key, event as ReactDragEvent<HTMLElement>)}
                    onDragLeave={(event) => handleColumnDragLeave(paramDef.key, event as ReactDragEvent<HTMLElement>)}
                    onDrop={(event) => handleColumnDrop(paramDef.key, event as ReactDragEvent<HTMLElement>)}
                    onDragEnd={handleColumnDragEnd}
                    className={`sticky top-0 z-10 terminal-th text-left ${dividerClass}`}
                    hint={headerHint}
                    style={{
                      width: PARAM_COLUMN_WIDTH,
                      minWidth: PARAM_COLUMN_WIDTH,
                      borderColor: colors.border.DEFAULT,
                      backgroundColor: '#111214',
                      color: colors.text.muted,
                      ...(dividerClass ? { borderLeftColor: colors.border.DEFAULT } : {}),
                    }}
                  />
                );
              })}
            </tr>
          </thead>
          <tbody>
            <tr
              data-testid="bulk-row"
              className="params-bulk-row"
              style={{
                position: 'sticky',
                top: `${headerHeight}px`,
                zIndex: 49,
                backgroundColor: colors.bg.surface,
                borderBottom: `1px solid ${colors.border.DEFAULT}`,
                boxShadow: `0 1px 0 ${colors.border.DEFAULT}`,
              }}
            >
              <th
                className={`sticky px-3 ${DENSE_CELL_PADDING} border-b backdrop-blur-sm text-left`}
                style={{
                  backgroundColor: colors.bg.surface,
                  borderColor: colors.border.DEFAULT,
                  borderLeft: `3px solid ${colors.border.DEFAULT}`,
                  left: PINNED_LEFT_OFFSETS.strategy,
                  width: strategyColumnWidth,
                  minWidth: strategyColumnWidth,
                  zIndex: 35,
                  fontWeight: typography.fontWeight.medium,
                }}
                scope="row"
              >
                <div className="flex items-center justify-between gap-2">
                  <div className="flex flex-col">
                    <span className="text-[12px] font-medium text-text-secondary">All (filtered)</span>
                    <span className="text-[11px] text-text-muted">
                      {bulkTargetIds.length > 0
                        ? `Applies to ${bulkTargetIds.length} strategies`
                        : 'No strategies match current filters'}
                    </span>
                  </div>
                  <SimpleTooltip content="Type a value then press Enter to apply to all filtered strategies. Esc cancels.">
                    <span className="text-[11px] text-text-muted cursor-help">ℹ︎</span>
                  </SimpleTooltip>
                </div>
                {bulkActiveParam && (
                  <div className="mt-1 text-[11px] text-text-tertiary">
                    Enter = apply · Esc = cancel (column: {bulkActiveParam})
                  </div>
                )}
              </th>
              <td
                className={`px-2 ${DENSE_CELL_PADDING} border-b text-center`}
                style={{ width: RUN_COLUMN_WIDTH, minWidth: RUN_COLUMN_WIDTH, borderColor: colors.border.DEFAULT, backgroundColor: colors.bg.surface }}
              >
                <span className="text-text-muted text-[11px]">—</span>
              </td>
              <td
                className={`px-2 ${DENSE_CELL_PADDING} border-b text-center`}
                style={{ width: TRADE_COLUMN_WIDTH, minWidth: TRADE_COLUMN_WIDTH, borderColor: colors.border.DEFAULT, backgroundColor: colors.bg.surface }}
              >
                <Switch
                  size="sm"
                  checked={(bulkDrafts['bot_on'] ?? '') === '1'}
                  onCheckedChange={(next) => {
                    const value = next ? '1' : '0';
                    setBulkDraft('bot_on', value);
                    applyBulkDraft('bot_on', value);
                  }}
                  onFocus={() => bulkFocusOn('bot_on')}
                  onBlur={bulkFocusOff}
                  aria-label="Toggle trading for all filtered strategies"
                  data-testid="bulk-trading-toggle"
                />
              </td>
              {orderedParamDefs.map((paramDef, columnIdx) => {
                const value = bulkDrafts[paramDef.key] ?? '';
                const group = getParamGroup(paramDef.key);
                const prevGroup = columnIdx > 0 ? getParamGroup(orderedParamDefs[columnIdx - 1].key) : null;
                const groupDivider =
                  columnIdx === 0 || group !== prevGroup ? 'border-l' : '';
                return (
                  <td
                    key={`bulk-${paramDef.key}`}
                    className={`px-2 ${DENSE_CELL_PADDING} border-b align-middle ${groupDivider}`}
                    style={{
                      width: PARAM_COLUMN_WIDTH,
                      minWidth: PARAM_COLUMN_WIDTH,
                      borderColor: colors.border.DEFAULT,
                      backgroundColor: colors.bg.surface,
                      position: 'relative',
                    }}
                  >
                    <ParamCell
                      value={value}
                      paramDef={paramDef}
                      dirty={false}
                      error={undefined}
                      saving={false}
                      onChange={(newValue) => setBulkDraft(paramDef.key, newValue)}
                      onFocus={() => bulkFocusOn(paramDef.key)}
                      onBlur={bulkFocusOff}
                      onSave={() => applyBulkDraft(paramDef.key)}
                      density="dense"
                      dataAttrs={{
                        'data-testid': `bulk-param-${paramDef.key}`,
                        'data-param': paramDef.key,
                      }}
                    />
                  </td>
                );
              })}
            </tr>

            {paddingTop > 0 && (
              <tr aria-hidden className="pointer-events-none">
                <td colSpan={STATIC_COLUMN_COUNT + orderedParamDefs.length} style={{ height: paddingTop }} />
              </tr>
            )}

            {visibleStrategies.length === 0 ? (
              <tr>
                <td
                  colSpan={STATIC_COLUMN_COUNT + orderedParamDefs.length}
                  className="text-center py-12"
                >
                  <div className="flex flex-col items-center gap-3">
                    <div style={{ fontSize: typography.fontSize.lg, color: colors.text.tertiary }}>
                      {strategies.length === 0 ? 'No strategies configured' : 'No strategies match filters'}
                    </div>
                    {(filterValues.strategy || filterValues.status || filterValues.dirty) && (
                      <button
                        onClick={() => setFilterValues({})}
                        className="px-4 py-2 rounded border hover:bg-neutral-700/50 transition-colors"
                        style={{
                          fontSize: typography.fontSize.sm,
                          color: colors.semantic.success.light,
                          borderColor: colors.border.DEFAULT,
                        }}
                      >
                        Clear all filters
                      </button>
                    )}
                  </div>
                </td>
              </tr>
            ) : (
              <>
                {rowsToRender.map((virtualRow) => {
                  const strategy = visibleStrategies[virtualRow.index];
                  if (!strategy) {
                    return null;
                  }
                  return (
                    <MemoizedStrategyRow
                      key={strategy.strategy_id}
                      strategy={strategy}
                      idx={virtualRow.index}
                      strategyColumnWidth={strategyColumnWidth}
                      orderedParamDefs={orderedParamDefs}
                      stratParams={paramValues.get(strategy.strategy_id) || EMPTY_PARAMS}
                      stratDirty={dirtyParams.get(strategy.strategy_id) || EMPTY_DIRTY_SET}
                      stratErrors={errorParams.get(strategy.strategy_id) || EMPTY_ERRORS}
                      isSaving={saving.has(strategy.strategy_id)}
                      isFlashing={flashingRows.has(strategy.strategy_id)}
                      isSelected={selectedSet.has(strategy.strategy_id)}
                      isAnchor={anchorStrategyId === strategy.strategy_id}
                      isRemoteUpdated={remoteUpdatedRows.has(strategy.strategy_id)}
                      conflictKeys={conflictRows.get(strategy.strategy_id) || EMPTY_CONFLICT_KEYS}
                      focusedParamKey={
                        lastFocusedCell && lastFocusedCell.strategyId === strategy.strategy_id
                          ? lastFocusedCell.paramKey
                          : null
                      }
                      measureRow={fallbackVirtualization ? undefined : rowVirtualizer.measureElement}
                      onParamChange={handleParamChange}
                      onParamBlur={handleParamBlur}
                      onParamFocus={handleParamFocus}
                      onParamBlurForFocus={handleParamBlurForFocus}
                      onSave={saveStrategy}
                      onRevert={handleRevertRow}
                      onConflictKeepMine={handleKeepMine}
                      onConflictUseRemote={handleUseRemote}
                      onConflictDiff={handleOpenDiff}
                      onConfigView={setConfigViewerStrategy}
                      onRowMouseDown={handleRowMouseDown}
                      onRowMouseEnter={handleRowMouseEnter}
                      onRowMouseUp={handleRowMouseUp}
                      onCellNavigate={handleCellNavigate}
                      highlightedParamKey={bulkActiveParam}
                    />
                  );
                })}
                {paddingBottom > 0 && (
                  <tr aria-hidden className="pointer-events-none">
                    <td colSpan={STATIC_COLUMN_COUNT + orderedParamDefs.length} style={{ height: paddingBottom }} />
                  </tr>
                )}
              </>
            )}
          </tbody>
        </table>
      </PanelBody>

      {/* Help Modal */}
      {helpModalParam && schema.params[helpModalParam] && (
        <ParamHelpModal
          paramDef={schema.params[helpModalParam]}
          open={!!helpModalParam}
          onClose={() => setHelpModalParam(null)}
        />
      )}

      {/* Config Viewer */}
      {configViewerStrategy && (
        <ConfigViewer
          strategyId={configViewerStrategy}
          open={!!configViewerStrategy}
          onClose={() => setConfigViewerStrategy(null)}
        />
      )}

      <ParamDiffModal
        open={Boolean(diffStrategyId)}
        strategyId={diffStrategyId}
        entries={diffEntries}
        onApplyRemote={() => {
          if (diffStrategyId) {
            handleUseRemote(diffStrategyId);
          }
        }}
        onKeepMine={() => {
          if (diffStrategyId) {
            handleKeepMine(diffStrategyId);
          }
        }}
        onClose={() => setDiffStrategyId(null)}
      />
    </div>
  );

  const content = isMobileView ? mobileContent : desktopContent;

  if (showHeader) {
    return (
      <PageShell>
        {content}
      </PageShell>
    );
  }

  return content;
}

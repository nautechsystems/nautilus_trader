// PnL Report - Analyze arbitrage trading performance grouped by signal_id

import { useEffect, useState, useMemo, useCallback, useRef, type CSSProperties, lazy, Suspense, startTransition } from 'react';
import { DataTable, type DataTableProps } from './components/ui/table/DataTable';
import { useMobileLayout } from '@/hooks/useMobileLayout';
import { type ColumnDef, type SortingState, type Row } from '@tanstack/react-table';
import * as formatters from './utils/format';
import { formatLocal } from './utils/time';
import { api } from './api';
import type { PnLParams, PnLReport, PnLGroup, PnLDeltaResponse, PnLInventoryParams, PnLInventoryReport } from './types';
import { toast } from 'sonner';
import { useCopyToClipboard } from './hooks/useCopyToClipboard';
import dayjs from 'dayjs';
import utc from 'dayjs/plugin/utc';

dayjs.extend(utc);
// Eager-load core UI atoms used above the fold for better performance
import { Button, SimpleTooltip, Switch } from './components/ui';
import { Pager } from './components/shared/Pager';
import { PanelBody } from './components/shared/PanelBody';
// Disconnect socket.io when PnL page is active (PnL doesn't need real-time data)
import { disconnectSocket, connectSocket } from './sockets';
import { colors, spacing, typography, borderRadius, STALE_THRESHOLDS, severity, semanticTokens } from './lib/tokens';
import { statusFromMark, StatusDescriptor } from './components/shared/status';
import { isPnlDecisionDetailsEnabled } from './config/featureFlags';
import { PageShell } from './components/layout/PageShell';
// Lazy-load only heavy panels/components
const LoadingState = lazy(async () => ({ default: (await import('./components/shared/LoadingState')).LoadingState }));
const EmptyState = lazy(async () => ({ default: (await import('./components/shared/EmptyState')).EmptyState }));
const PanelHeader = lazy(async () => ({ default: (await import('./components/shared/PanelHeader')).PanelHeader }));
const TableFilter = lazy(async () => ({ default: (await import('./components/shared/TableFilter')).TableFilter }));
type ColumnFilter = import('./components/shared/TableFilter').ColumnFilter;
type FilterValues = import('./components/shared/TableFilter').FilterValues;

const panelStyleBase: CSSProperties = {
  backgroundColor: semanticTokens.surface,
  border: `1px solid ${colors.border.DEFAULT}`,
  borderRadius: borderRadius.lg,
  padding: spacing.gap.lg,
};

const inputStyleBase: CSSProperties = {
  backgroundColor: semanticTokens.surface,
  color: semanticTokens.textPrimary,
  border: `1px solid ${colors.border.DEFAULT}`,
  borderRadius: borderRadius.sm,
  padding: `${spacing.padding.normal} ${spacing.gap.sm}`,
  fontSize: typography.fontSize.sm,
  fontFamily: typography.fontFamily.sans,
  transition: 'border-color 0.2s',
};

const labelStyleBase: CSSProperties = {
  fontSize: typography.fontSize.xs,
  fontWeight: typography.fontWeight.medium,
  color: semanticTokens.textMuted,
  textTransform: 'uppercase',
  letterSpacing: '0.04em',
};

const tableHeaderStyleBase: CSSProperties = {
  backgroundColor: semanticTokens.surface,
  color: semanticTokens.textMuted,
  fontSize: typography.fontSize.xs,
  fontWeight: typography.fontWeight.medium,
  textTransform: 'uppercase',
  letterSpacing: '0.03em',
};

const statusDot = (color: string): CSSProperties => ({
  width: 8,
  height: 8,
  borderRadius: '999px',
  backgroundColor: color,
});

const microBadgeStyle = (tone: keyof typeof semanticTokens.status): CSSProperties => ({
  display: 'inline-flex',
  alignItems: 'center',
  gap: 6,
  padding: '2px 6px',
  borderRadius: borderRadius.md,
  border: `1px solid ${colors.border.DEFAULT}`,
  backgroundColor: semanticTokens.surfaceAlt,
  color: tone === 'success'
    ? semanticTokens.status.success
    : tone === 'danger'
      ? semanticTokens.status.danger
      : tone === 'warning'
        ? semanticTokens.status.warning
        : colors.text.primary,
  fontSize: typography.fontSize['2xs'],
  fontWeight: typography.fontWeight.medium,
});

const tinyTagStyle = (tone: 'success' | 'danger' | 'warning' | 'info' | 'neutral'): CSSProperties => {
  const toneColor = tone === 'success'
    ? semanticTokens.status.success
    : tone === 'danger'
      ? semanticTokens.status.danger
      : tone === 'warning'
        ? semanticTokens.status.warning
        : tone === 'info'
          ? colors.semantic.info.light
          : semanticTokens.textMuted;
  return {
    display: 'inline-flex',
    alignItems: 'center',
    gap: 4,
    padding: '2px 6px',
    borderRadius: borderRadius.md,
    border: `1px solid ${colors.border.DEFAULT}`,
    backgroundColor: semanticTokens.surfaceAlt,
    color: toneColor,
    fontSize: typography.fontSize['2xs'],
    fontWeight: typography.fontWeight.medium,
  } as CSSProperties;
};

const segmentedControlStyle = (active: boolean): CSSProperties => ({
  border: `1px solid ${active ? colors.border.hover : colors.border.DEFAULT}`,
  backgroundColor: active ? semanticTokens.surfaceAlt : 'transparent',
  color: active ? semanticTokens.textPrimary : semanticTokens.textMuted,
  borderRadius: borderRadius.sm,
  padding: '6px 10px',
  fontSize: typography.fontSize.sm,
  boxShadow: active ? `inset 0 -2px 0 ${semanticTokens.status.success}` : undefined,
});

const PNL_STALE_THRESHOLD_MS = 5 * 60 * 1000; // PnL reports considered stale after 5 minutes without refresh

const groupRowId = (row: PnLGroup): string => {
  return `${row.symbol}_${row.signal_id ?? 'null'}_${row.start_time}`;
};

const orderedStringify = (value: unknown): string => {
  if (value === null || typeof value !== 'object') {
    return JSON.stringify(value);
  }
  if (Array.isArray(value)) {
    return `[${value.map(orderedStringify).join(',')}]`;
  }
  const entries = Object.entries(value as Record<string, unknown>).sort(([a], [b]) =>
    a.localeCompare(b)
  );
  return `{${entries.map(([key, val]) => `${JSON.stringify(key)}:${orderedStringify(val)}`).join(',')}}`;
};

const hashString = (input: string): string => {
  let hash = 0;
  for (let i = 0; i < input.length; i++) {
    hash = (hash << 5) - hash + input.charCodeAt(i);
    hash |= 0;
  }
  return hash.toString();
};

const buildReportSignature = (report: PnLReport): string => {
  const segments = [
    orderedStringify(report.summary),
    orderedStringify(
      report.groups.map((g) => ({
        symbol: g.symbol,
        signal_id: g.signal_id,
        start_time: g.start_time,
        end_time: g.end_time,
        pnl_bps: g.pnl_bps,
        pnl_usd: g.pnl_usd,
        hedged_qty: g.hedged_qty,
      }))
    ),
    orderedStringify(report.by_symbol ?? {}),
    orderedStringify(report.unhedged ?? {}),
  ];
  return hashString(segments.join('||'));
};

type TimeWindowMode = '15m' | '1h' | '4h' | '24h' | 'custom' | 'last' | 'range' | 'all';
type SortColumn = keyof PnLGroup;
type SortDirection = 'asc' | 'desc';
type PnLFilter = 'all' | 'positive' | 'negative';
type PnLRowContext = { row: Row<PnLGroup> };
type BySymbolSortKey =
  | 'symbol'
  | 'gross_bps'
  | 'net_bps'
  | 'm2m_usd'
  | 'gross_flow'
  | 'matched_notional'
  | 'buy_notional'
  | 'sell_notional';

// Time window presets (in minutes)
const TIME_PRESETS: Record<string, number> = {
  '15m': 15,
  '1h': 60,
  '4h': 240,
  '24h': 1440,
};

const TIME_OPTION_LABELS: Array<{ key: TimeWindowMode; label: string }> = [
  { key: '15m', label: '15m' },
  { key: '1h', label: '1h' },
  { key: '4h', label: '4h' },
  { key: '24h', label: '24h' },
  { key: 'custom', label: 'Custom' },
  { key: 'last', label: 'Last N' },
  { key: 'range', label: 'Range' },
  { key: 'all', label: 'All' },
];

const BY_SYMBOL_TAGS = {
  loss: { status: 'critical' as const, label: 'Loss' },
  stale: { status: 'warning' as const, label: 'Stale FV' },
  coverage: { status: 'warning' as const, label: 'Low Cov' },
};
const FV_AGE_CAP_MS = 60_000; // Cap FV age display to keep tooltips readable

const formatFvAge = (ageMs?: number | null): string | null => {
  if (!ageMs || ageMs <= 0) return null;
  if (ageMs > FV_AGE_CAP_MS) return '>60s';
  if (ageMs >= 1000) return `${(ageMs / 1000).toFixed(1)}s`;
  return `${ageMs}ms`;
};

const VAR_MULT = 0.03; // 3% volatility assumption
const USD_MIN = 50.0;
const DEFAULT_GROUPS_PAGE_SIZE = 100; // Default page size for PnL Groups table
const AUTO_EXPAND_THRESHOLD = 100; // Auto-expand groups section if <= this many groups
const BY_SYMBOL_MAX_ROWS = 200; // Cap rows rendered for by_symbol to prevent UI jank
const BY_SYMBOL_ROW_HEIGHT = 40; // Fixed row height for virtualization
const BY_SYMBOL_OVERSCAN = 6; // Overscan rows for smooth scrolling
const GROUP_ROW_HEIGHT = 44; // Target row height for groups table (px)
const GROUP_MIN_VISIBLE_ROWS = 6; // Aim to show at least 6 rows before scrolling

const GROUP_TABLE_FILTERS: ColumnFilter[] = [
  { key: 'symbol', label: 'Symbol', type: 'text', placeholder: 'BTC, ETH…' },
  { key: 'signal', label: 'Signal', type: 'text', placeholder: 'Strategy ID…' },
];

const GROUP_TABLE_PRIMARY_COLUMNS = ['symbol', 'signal_id', 'pnl_bps'];


const REFRESH_SECONDS = 30;

// UtcDateTimeField component for keyboard-centric UTC datetime input
interface UtcDateTimeFieldProps {
  label: string;
  value: string; // UTC ISO string
  inputValue: string; // Local input state (YYYY-MM-DD HH:mm)
  onInputChange: (value: string) => void;
  onValueChange: (utcIso: string) => void;
  onError: (error: string) => void;
  error: string;
  formatUtcForInput: (utcIso: string) => string;
  parseUtcInput: (input: string) => string | null;
  formatUtcAsLocalHelper: (utcIso: string) => string;
  inputStyle: CSSProperties;
  labelStyle: CSSProperties;
}

function UtcDateTimeField({
  label,
  value,
  inputValue,
  onInputChange,
  onValueChange,
  onError,
  error,
  formatUtcForInput,
  parseUtcInput,
  formatUtcAsLocalHelper,
  inputStyle,
  labelStyle,
}: UtcDateTimeFieldProps) {
  const inputRef = useRef<HTMLInputElement>(null);
  const lastValidValue = useRef<string>(value);

  // Sync input value when UTC value changes externally (e.g., from preset)
  useEffect(() => {
    if (value && value !== lastValidValue.current) {
      const formatted = formatUtcForInput(value);
      onInputChange(formatted);
      lastValidValue.current = value;
    }
  }, [value, formatUtcForInput, onInputChange]);

  // Initialize input value from UTC value
  useEffect(() => {
    if (value && !inputValue) {
      const formatted = formatUtcForInput(value);
      onInputChange(formatted);
      lastValidValue.current = value;
    }
  }, [value, inputValue, formatUtcForInput, onInputChange]);

  const handleBlur = useCallback(() => {
    if (!inputValue.trim()) {
      // Empty input - revert to last valid
      if (lastValidValue.current) {
        onInputChange(formatUtcForInput(lastValidValue.current));
      }
      onError('');
      return;
    }

    const parsed = parseUtcInput(inputValue);
    if (parsed) {
      onValueChange(parsed);
      lastValidValue.current = parsed;
      onError('');
    } else {
      onError('Invalid datetime (expected YYYY-MM-DD HH:mm)');
      // Revert to last valid value
      if (lastValidValue.current) {
        onInputChange(formatUtcForInput(lastValidValue.current));
      }
    }
  }, [inputValue, parseUtcInput, formatUtcForInput, onValueChange, onError, onInputChange]);

  const handleKeyDown = useCallback((e: React.KeyboardEvent<HTMLInputElement>) => {
    if (e.key === 'Enter') {
      e.preventDefault();
      inputRef.current?.blur();
      return;
    }

    // Keyboard shortcuts: Arrow keys with modifiers
    if (e.key === 'ArrowUp' || e.key === 'ArrowDown') {
      const currentValue = inputRef.current?.value || '';
      if (!currentValue.trim()) return;

      const parsed = parseUtcInput(currentValue);
      if (!parsed) return;

      e.preventDefault();
      const dt = dayjs.utc(parsed);
      if (!dt.isValid()) return;

      let adjusted = dt;
      if (e.shiftKey) {
        // Shift + Arrow: ±1 hour
        adjusted = e.key === 'ArrowUp' ? dt.add(1, 'hour') : dt.subtract(1, 'hour');
      } else if (e.ctrlKey || e.metaKey) {
        // Ctrl/Cmd + Arrow: ±1 day
        adjusted = e.key === 'ArrowUp' ? dt.add(1, 'day') : dt.subtract(1, 'day');
      } else {
        // Arrow only: ±1 minute
        adjusted = e.key === 'ArrowUp' ? dt.add(1, 'minute') : dt.subtract(1, 'minute');
      }

      const newUtcIso = adjusted.toISOString();
      const formatted = formatUtcForInput(newUtcIso);
      onInputChange(formatted);
      onValueChange(newUtcIso);
      lastValidValue.current = newUtcIso;
      onError('');
    }
  }, [parseUtcInput, formatUtcForInput, onInputChange, onValueChange, onError]);

  return (
    <div className="flex flex-col" style={{ gap: spacing.gap.xs }}>
      <span style={labelStyle}>{label}</span>
      <div className="relative">
        <input
          ref={inputRef}
          type="text"
          value={inputValue}
          onChange={(e) => onInputChange(e.target.value)}
          onBlur={handleBlur}
          onKeyDown={handleKeyDown}
          placeholder="2025-11-12 10:27"
          style={{
            ...inputStyle,
            width: '200px',
            fontFamily: typography.fontFamily.mono,
            ...(error ? { borderColor: colors.semantic.danger.DEFAULT } : {}),
          }}
        />
      </div>
      {error ? (
        <span style={{ fontSize: typography.fontSize.xs, color: colors.semantic.danger.light }}>
          {error}
        </span>
      ) : value ? (
        <span style={{ fontSize: typography.fontSize.xs, color: colors.text.muted }}>
          {formatUtcAsLocalHelper(value)}
        </span>
      ) : null}
    </div>
  );
}

export default function PnL() {
  const { isMobile } = useMobileLayout();
  // Form state
  const [timeWindowMode, setTimeWindowMode] = useState<TimeWindowMode>('24h');
  const [minutes, setMinutes] = useState<number>(60);
  const [last, setLast] = useState<number>(100);
  // UTC-based datetime range (ISO strings)
  const [startUtc, setStartUtc] = useState<string>('');
  const [endUtc, setEndUtc] = useState<string>('');
  // Local input state for typing (YYYY-MM-DD HH:mm format)
  const [startInput, setStartInput] = useState<string>('');
  const [endInput, setEndInput] = useState<string>('');
  // Error states
  const [startError, setStartError] = useState<string>('');
  const [endError, setEndError] = useState<string>('');
  const [base, setBase] = useState<string | null>(null);
  const decisionDetailsEnabled = useMemo(() => isPnlDecisionDetailsEnabled(), []);

  // Helper: Get current UTC time as ISO string
  const getNowUtc = useCallback((): string => {
    return dayjs.utc().toISOString();
  }, []);

  // Helper: Format UTC ISO string to YYYY-MM-DD HH:mm format for input display
  const formatUtcForInput = useCallback((utcIso: string): string => {
    if (!utcIso) return '';
    try {
      const dt = dayjs.utc(utcIso);
      if (!dt.isValid()) return '';
      return dt.format('YYYY-MM-DD HH:mm');
    } catch {
      return '';
    }
  }, []);

  // Helper: Parse YYYY-MM-DD HH:mm string as UTC and return ISO string
  const parseUtcInput = useCallback((input: string): string | null => {
    if (!input || !input.trim()) return null;
    try {
      // Parse as UTC using dayjs
      const dt = dayjs.utc(input.trim(), 'YYYY-MM-DD HH:mm', true);
      if (!dt.isValid()) return null;
      return dt.toISOString();
    } catch {
      return null;
    }
  }, []);

  // Helper: Format UTC ISO string as local time helper text
  const formatUtcAsLocalHelper = useCallback((utcIso: string): string => {
    if (!utcIso) return '';
    try {
      const utcDate = dayjs.utc(utcIso);
      if (!utcDate.isValid()) return '';
      const localDate = utcDate.local();
      const offset = localDate.utcOffset();
      const offsetHours = Math.floor(Math.abs(offset) / 60);
      const offsetMinutes = Math.abs(offset) % 60;
      const offsetSign = offset >= 0 ? '+' : '-';
      const offsetStr = `UTC${offsetSign}${String(offsetHours).padStart(2, '0')}:${String(offsetMinutes).padStart(2, '0')}`;

      return `Local: ${localDate.format('YYYY-MM-DD HH:mm')} (${offsetStr})`;
    } catch {
      return '';
    }
  }, []);

  // Initialize range times when switching to range mode
  // If coming from a preset mode, use that preset's time window
  // Otherwise default to last 24 hours in UTC
  useEffect(() => {
    if (timeWindowMode === 'range') {
      // Only initialize if both are empty (first time switching to range)
      if (!startUtc || !endUtc) {
        const nowUtc = dayjs.utc();
        const endUtcIso = nowUtc.toISOString();
        const startUtcIso = nowUtc.subtract(24, 'hour').toISOString();
        setEndUtc(endUtcIso);
        setStartUtc(startUtcIso);
        // Input values will be synced by UtcDateTimeField useEffect
      }
    }
  }, [timeWindowMode, startUtc, endUtc]);
  const [dexFeeBps, setDexFeeBps] = useState<number>(2.0);
  const [cexFeeBps, setCexFeeBps] = useState<number>(5.0);
  const [dex, setDex] = useState<string>('rooster');
  const [cex, setCex] = useState<string>('bybit');
  const [showAdvanced, setShowAdvanced] = useState<boolean>(false);

  type PnLMode = 'spread' | 'inventory';
  const [pnlMode, setPnlMode] = useState<PnLMode>('spread');

  // Report state
  const [report, setReport] = useState<PnLReport | null>(null);
  const [inventoryReport, setInventoryReport] = useState<PnLInventoryReport | null>(null);
  const reportSignatureRef = useRef<string>('');
  const lastParamsRef = useRef<string>('');
  const hasReportRef = useRef(false);
  const groupHashesRef = useRef<Record<string, string>>({});
  const symbolHashesRef = useRef<Record<string, string>>({});
  const unhedgedHashesRef = useRef<Record<string, string>>({});
  const lastEtagRef = useRef<string | null>(null);
  const loadingRef = useRef<boolean>(false);
  const prevAutoRefreshRef = useRef<boolean>(false);
  const hasRunInitialReportRef = useRef<boolean>(false);
  const [loading, setLoading] = useState<boolean>(false);
  const [csvLoading, setCsvLoading] = useState<boolean>(false);
  const [error, setError] = useState<string | null>(null);
  const [lastUpdate, setLastUpdate] = useState<Date | null>(null);

  // Inventory filters
  const [invSuite, setInvSuite] = useState<string>('hl_ibkr');
  const [invStrategyClass, setInvStrategyClass] = useState<string>('basis_arb');
  const [invStrategyIds, setInvStrategyIds] = useState<string>('');
  const [invSymbols, setInvSymbols] = useState<string>('');
  const [invExchanges, setInvExchanges] = useState<string>('');
  const [invIncludeInvalid, setInvIncludeInvalid] = useState<boolean>(false);

  // UI state
  const [autoRefresh, setAutoRefresh] = useState<boolean>(false);
  const [refreshCountdown, setRefreshCountdown] = useState<number>(REFRESH_SECONDS);
  const [groupsExpanded, setGroupsExpanded] = useState<boolean>(false);
  const [sortColumn, setSortColumn] = useState<SortColumn>('start_time');
  const [sortDirection, setSortDirection] = useState<SortDirection>('desc');
  const [groupFilters, setGroupFilters] = useState<FilterValues>({});
  const [pnlFilter, setPnlFilter] = useState<PnLFilter>('all');
  const [groupsPage, setGroupsPage] = useState<number>(1);
  const [groupsPageSize, setGroupsPageSize] = useState<number>(DEFAULT_GROUPS_PAGE_SIZE);
  const [availableSymbols, setAvailableSymbols] = useState<string[]>([]);
  const [symbolsLoading, setSymbolsLoading] = useState<boolean>(true);
  const [unitPrimary, setUnitPrimary] = useState<'bps' | 'usd'>('bps');
  const [showSmallUnhedged, setShowSmallUnhedged] = useState(false);
  const [netUnhedgedAcrossVenues, setNetUnhedgedAcrossVenues] = useState(true);
  const [bySymbolFilter, setBySymbolFilter] = useState<'all' | 'loss' | 'stale_fv' | 'low_coverage'>('all');
  const [showGroupsPnlUsd, setShowGroupsPnlUsd] = useState(false);
  const [showBySymbolMore, setShowBySymbolMore] = useState(false);
  const [showAllBySymbolRows, setShowAllBySymbolRows] = useState(false);
  const [bySymbolSort, setBySymbolSort] = useState<{ column: BySymbolSortKey; direction: SortDirection }>({
    column: 'net_bps',
    direction: 'desc',
  });

  // Scroll preservation ref
  const scrollPosRef = useRef<number>(0);

  // Table container refs for scroll preservation
  const bySymbolTableRef = useRef<HTMLDivElement>(null);
  const groupsTableContainerRef = useRef<HTMLDivElement>(null);

  // Store table scroll positions during capture
  const tableScrollPosRef = useRef<{
    bySymbolScrollTop: number;
    groupsScrollTop: number;
  }>({
    bySymbolScrollTop: 0,
    groupsScrollTop: 0,
  });

  // Store by_symbol data to prevent column recreation on every refresh
  const bySymbolMapRef = useRef<Record<string, any>>({});

  // Build params object from form state
  const buildParams = useCallback((): PnLParams => {
    let minutesValue: number | null = null;
    let startTimeValue: string | null = null;
    let endTimeValue: string | null = null;

    // Handle preset time modes
    if (timeWindowMode in TIME_PRESETS) {
      minutesValue = TIME_PRESETS[timeWindowMode];
    } else if (timeWindowMode === 'custom') {
      minutesValue = minutes;
    } else if (timeWindowMode === 'range') {
      // Use UTC values directly (already ISO strings)
      // CRITICAL: Always compute values synchronously here, don't rely on state
      // that might not be updated yet (React state updates are async)
      if (startUtc && endUtc && startUtc.trim() !== '' && endUtc.trim() !== '') {
        startTimeValue = startUtc;
        endTimeValue = endUtc;
      } else {
        // Fallback: if values are empty, compute default range synchronously
        // This ensures we always have valid values when in range mode
        const nowUtc = dayjs.utc();
        startTimeValue = nowUtc.subtract(24, 'hour').toISOString();
        endTimeValue = nowUtc.toISOString();
      }
    }

    const params = {
      minutes: timeWindowMode === 'all' || timeWindowMode === 'range' ? null : minutesValue,
      last: timeWindowMode === 'last' ? last : null,
      start_time: timeWindowMode === 'range' ? startTimeValue : null,
      end_time: timeWindowMode === 'range' ? endTimeValue : null,
      base: base, // Only send canonical base to backend; UI filters are client-only
      // Note: Time windowing is controlled by `minutes`, `last`, or `start_time`/`end_time`; backend does not use `window_s`.
      dex_fee_bps: dexFeeBps,
      cex_fee_bps: cexFeeBps,
      // Use auto-detection so PLUME includes all DEX venues (rooster, pancakeswap, etc.)
      dex: 'auto',
      cex,
    };

    // Debug logging for range mode
    if (timeWindowMode === 'range') {
      console.log('[PnL] Range mode params:', JSON.stringify({
        timeWindowMode,
        startUtc,
        endUtc,
        startTimeValue,
        endTimeValue,
        start_time: params.start_time,
        end_time: params.end_time,
        minutes: params.minutes,
        last: params.last
      }, null, 2));
    }

    return params;
  }, [timeWindowMode, minutes, last, startUtc, endUtc, base, dexFeeBps, cexFeeBps, cex]);

  const buildInventoryParams = useCallback((): PnLInventoryParams => {
    let minutesValue: number | null = null;
    let startTimeValue: string | null = null;
    let endTimeValue: string | null = null;

    if (timeWindowMode in TIME_PRESETS) {
      minutesValue = TIME_PRESETS[timeWindowMode];
    } else if (timeWindowMode === 'custom') {
      minutesValue = minutes;
    } else if (timeWindowMode === 'range') {
      if (startUtc && endUtc && startUtc.trim() !== '' && endUtc.trim() !== '') {
        startTimeValue = startUtc;
        endTimeValue = endUtc;
      } else {
        const nowUtc = dayjs.utc();
        startTimeValue = nowUtc.subtract(24, 'hour').toISOString();
        endTimeValue = nowUtc.toISOString();
      }
    }

    const parseList = (s: string): string[] =>
      (s || '')
        .split(',')
        .map((x) => x.trim())
        .filter(Boolean);

    return {
      minutes: timeWindowMode === 'range' || timeWindowMode === 'all' ? null : minutesValue,
      last: timeWindowMode === 'last' ? last : null,
      start_time: timeWindowMode === 'range' ? startTimeValue : null,
      end_time: timeWindowMode === 'range' ? endTimeValue : null,
      all: timeWindowMode === 'all',
      suite: invSuite?.trim() ? invSuite.trim() : null,
      strategy_class: invStrategyClass?.trim() ? invStrategyClass.trim() : null,
      strategy_ids: parseList(invStrategyIds),
      symbols: parseList(invSymbols).map((x) => x.toUpperCase()),
      exchanges: parseList(invExchanges).map((x) => x.toLowerCase()),
      include_invalid_rows: invIncludeInvalid,
      fx_quote_to_usd: true,
    };
  }, [
    timeWindowMode,
    minutes,
    last,
    startUtc,
    endUtc,
    invSuite,
    invStrategyClass,
    invStrategyIds,
    invSymbols,
    invExchanges,
    invIncludeInvalid,
  ]);

  // Update range when preset buttons are clicked (if range mode is active)
  // Also initialize range when switching TO range mode from a preset
  const handleTimeWindowModeChange = useCallback((mode: TimeWindowMode) => {
    const nowUtc = dayjs.utc();
    const endUtcIso = nowUtc.toISOString();

    // Compute the time window for the new mode
    let startUtcIso: string | null = null;

    if (mode in TIME_PRESETS) {
      const minutesValue = TIME_PRESETS[mode];
      startUtcIso = nowUtc.subtract(minutesValue, 'minute').toISOString();
    } else if (mode === 'custom') {
      startUtcIso = nowUtc.subtract(minutes, 'minute').toISOString();
    } else if (mode === 'last') {
      // For "last N", end is now, start not meaningful
      startUtcIso = null;
    } else if (mode === 'all') {
      // For "all", clear range
      startUtcIso = null;
    } else if (mode === 'range') {
      // When switching TO range mode, use current preset's window if available
      // OR use existing range values if they're already set
      if (startUtc && endUtc) {
        // Keep existing range values
        startUtcIso = startUtc;
      } else if (timeWindowMode in TIME_PRESETS) {
        const minutesValue = TIME_PRESETS[timeWindowMode];
        startUtcIso = nowUtc.subtract(minutesValue, 'minute').toISOString();
      } else if (timeWindowMode === 'custom') {
        startUtcIso = nowUtc.subtract(minutes, 'minute').toISOString();
      } else {
        // Default to 24 hours
        startUtcIso = nowUtc.subtract(24, 'hour').toISOString();
      }
    }

    // Update range values (for consistency and for when switching to range mode)
    if (mode === 'all') {
      setEndUtc('');
      setStartUtc('');
      setEndInput('');
      setStartInput('');
    } else if (mode === 'range') {
      // Switching to range mode - ALWAYS initialize with computed values
      // This ensures values are set synchronously before any refresh
      if (startUtcIso) {
        setEndUtc(endUtcIso);
        setStartUtc(startUtcIso);
        // Input values will be synced by UtcDateTimeField useEffect
      } else {
        // Fallback: ensure we always have values in range mode
        const nowUtc = dayjs.utc();
        const defaultEndUtc = nowUtc.toISOString();
        const defaultStartUtc = nowUtc.subtract(24, 'hour').toISOString();
        setEndUtc(defaultEndUtc);
        setStartUtc(defaultStartUtc);
      }
    } else if (startUtcIso) {
      // Preset or custom mode - update range for consistency
      setEndUtc(endUtcIso);
      setStartUtc(startUtcIso);
      // Input values will be synced by UtcDateTimeField useEffect
    } else if (mode === 'last') {
      // Last N mode - set end to now
      setEndUtc(endUtcIso);
      // Input values will be synced by UtcDateTimeField useEffect
    }

    setTimeWindowMode(mode);
  }, [minutes, timeWindowMode, startUtc, endUtc]);

  // Formatting helpers
  const fmtDualPnL = (bps: number | undefined, usd?: number) => {
    return formatters.fmtDualPnL(bps, usd);
  };

  const fmtPrimary = useCallback((bps: number | undefined, usd?: number) => {
    if (unitPrimary === 'usd' && usd !== undefined) {
      return formatters.fmtMoney(usd, 2);
    }
    return formatters.fmtFixed(bps, 2);
  }, [unitPrimary]);

  const money = (v: unknown) => {
    const n = Number(v);
    return Number.isFinite(n) ? `$${n.toLocaleString(undefined, { maximumFractionDigits: 2 })}` : '—';
  };

  const commitReport = useCallback((nextReport: PnLReport | null) => {
    if (!nextReport) {
      return false;
    }
    // Server MUST provide report_signature - never compute it client-side (too expensive for large datasets)
    const signature = (nextReport as any).report_signature;
    if (!signature) {
      // If server doesn't provide signature, skip deduplication check but still apply report
      // This is acceptable since server should always provide it
      console.warn('[PnL] Report missing report_signature, applying anyway');
    } else if (reportSignatureRef.current === signature) {
      return false;
    }

    if (signature) {
      reportSignatureRef.current = signature;
    }

    // Use startTransition to defer React state update - prevents blocking UI during large data processing
    startTransition(() => {
      setReport(nextReport);
      setLastUpdate(new Date());
    });

    hasReportRef.current = true;
    if (nextReport.group_hashes) {
      groupHashesRef.current = nextReport.group_hashes;
    }
    if (nextReport.symbol_hashes) {
      symbolHashesRef.current = nextReport.symbol_hashes;
    }
    if (nextReport.unhedged_hashes) {
      unhedgedHashesRef.current = nextReport.unhedged_hashes;
    }
    return true;
  }, []);

  const mergeDelta = useCallback((delta: PnLDeltaResponse) => {
    if (!report) {
      return false;
    }

    const groupMap = new Map<string, PnLGroup>();
    for (const existing of report.groups) {
      groupMap.set(groupRowId(existing), existing);
    }

    delta.groups?.remove?.forEach((id) => {
      groupMap.delete(id);
    });
    delta.groups?.update?.forEach((g) => {
      groupMap.set(groupRowId(g), g);
    });
    delta.groups?.add?.forEach((g) => {
      groupMap.set(groupRowId(g), g);
    });

    const bySymbolCurrent = { ...(report.by_symbol ?? {}) };
    delta.by_symbol?.remove?.forEach((symbol) => {
      delete bySymbolCurrent[symbol];
    });
    Object.entries(delta.by_symbol?.update ?? {}).forEach(([symbol, data]) => {
      bySymbolCurrent[symbol] = data;
    });

    const unhedgedCurrent = { ...(report.unhedged ?? {}) };
    delta.unhedged?.remove?.forEach((key) => {
      delete unhedgedCurrent[key];
    });
    Object.entries(delta.unhedged?.update ?? {}).forEach(([key, value]) => {
      unhedgedCurrent[key] = value;
    });

    const nextReport: PnLReport = {
      ...report,
      summary: delta.summary ?? report.summary,
      groups: Array.from(groupMap.values()),
      by_symbol: bySymbolCurrent,
      unhedged: unhedgedCurrent,
      asof: delta.asof ?? report.asof,
      asof_ts: delta.asof_ts ?? report.asof_ts,
      fv_map: delta.fv_map ?? report.fv_map,
      fx_map: delta.fx_map ?? report.fx_map,
      timing: delta.timing ?? report.timing,
      group_hashes: delta.group_hashes ?? report.group_hashes,
      symbol_hashes: delta.symbol_hashes ?? report.symbol_hashes,
      unhedged_hashes: delta.unhedged_hashes ?? report.unhedged_hashes,
    };

    return commitReport(nextReport);
  }, [commitReport, report]);

  type FetchResult = 'applied' | 'noop';
  type DeltaResult = FetchResult | 'failed';

  const fetchFullReport = useCallback(async ({ params, serialized, forceEtagReset }: { params?: PnLParams; serialized?: string; forceEtagReset?: boolean } = {}): Promise<FetchResult> => {
    const requestParams = params ?? buildParams();
    const paramsSerialized = serialized ?? JSON.stringify(requestParams);
    const response = await api.runPnLReport(requestParams, {
      etag: forceEtagReset ? null : lastEtagRef.current,
    });

    if (response.status === 304 || !response.report) {
      return 'noop';
    }

    lastEtagRef.current = response.etag ?? response.report?.report_signature ?? null;
    const applied = commitReport(response.report);
    if (applied) {
      lastParamsRef.current = paramsSerialized;
      return 'applied';
    }
    return 'noop';
  }, [buildParams, commitReport]);

  const fetchDeltaReport = useCallback(async (params: PnLParams, serialized: string): Promise<DeltaResult> => {
    if (!report) {
      return 'failed';
    }
    try {
      const delta = await api.runPnLDelta({
        ...params,
        known_groups: groupHashesRef.current,
        known_symbols: symbolHashesRef.current,
        known_unhedged: unhedgedHashesRef.current,
      }, { etag: lastEtagRef.current });

      // Handle 304 Not Modified
      if ((delta as any)?.status === 304) {
        return 'noop';
      }

      if ((delta as any).reset_required) {
        return 'failed';
      }

      const changed = mergeDelta(delta as any);
      if (changed) {
        lastParamsRef.current = serialized;
        if ((delta as any).report_signature) {
          lastEtagRef.current = (delta as any).report_signature;
        }
        return 'applied';
      }
      return 'noop';
    } catch (err) {
      if (import.meta.env?.DEV) {
        console.warn('[PnL] Delta refresh failed, falling back to full fetch', err);
      }
      return 'failed';
    }
  }, [mergeDelta, report]);

  type RunOptions = { forceFull?: boolean };

  const runReport = useCallback(async (options: RunOptions = {}) => {
    setError(null);
    if (pnlMode === 'inventory') {
      setLoading(true);
      try {
        const invParams = buildInventoryParams();
        const inv = await api.runPnLInventoryReport(invParams);
        setInventoryReport(inv);
        setReport(null);
        setLastUpdate(new Date());
      } catch (e) {
        const msg = e instanceof Error ? e.message : 'Failed to run inventory PnL report';
        setError(msg);
        toast.error(msg);
      } finally {
        setLoading(false);
        setRefreshCountdown(REFRESH_SECONDS);
      }
      return;
    }

    const params = buildParams();
    const serializedParams = JSON.stringify(params);
    const paramsChanged = serializedParams !== lastParamsRef.current;
    const needsFull = !!options.forceFull || !hasReportRef.current || paramsChanged;

    // Only capture scroll if we expect to update UI (will apply changes)
    const captureScroll = () => {
      scrollPosRef.current = window.scrollY;
      tableScrollPosRef.current.bySymbolScrollTop = bySymbolTableRef.current?.scrollTop ?? 0;
      tableScrollPosRef.current.groupsScrollTop = groupsTableContainerRef.current?.scrollTop ?? 0;
    };

    try {
      if (needsFull) {
        const resetEtag = paramsChanged; // Do not reset ETag on manual Run unless params changed
        setLoading(true);
        captureScroll();
        const result = await fetchFullReport({ params, serialized: serializedParams, forceEtagReset: resetEtag });
        // Only keep loading if we actually applied changes
        if (result === 'noop') {
          setLoading(false);
          // Update lastUpdate even on 304/noop to reflect successful refresh check
          setLastUpdate(new Date());
        }
      } else {
        const deltaResult = await fetchDeltaReport(params, serializedParams);
        if (deltaResult === 'applied') {
          // Delta applied changes - show loading and capture scroll
          setLoading(true);
          captureScroll();
        } else if (deltaResult === 'failed') {
          // Delta failed, falling back to full fetch
          setLoading(true);
          captureScroll();
          const fullResult = await fetchFullReport({ params, serialized: serializedParams });
          // Update lastUpdate even if full fetch returns noop (304)
          if (fullResult === 'noop') {
            setLastUpdate(new Date());
          }
        } else if (deltaResult === 'noop') {
          // Delta returned 304 or no changes - update lastUpdate to reflect successful check
          setLastUpdate(new Date());
        }
        // If deltaResult === 'noop', skip loading state entirely
      }
    } catch (e) {
      const msg = e instanceof Error ? e.message : 'Failed to run PnL report';
      setError(msg);
      toast.error(msg);
    } finally {
      setLoading(false);
      setRefreshCountdown(REFRESH_SECONDS);
    }
  }, [pnlMode, buildInventoryParams, buildParams, fetchDeltaReport, fetchFullReport]);

  // Download CSV
  const downloadCSV = useCallback(async () => {
    if (pnlMode === 'inventory') {
      toast.error('CSV export is only available in Spread mode');
      return;
    }
    setCsvLoading(true);
    try {
      const params = buildParams();
      await api.downloadPnLCSV(params);
    } catch (e) {
      // Error already handled in API client
    } finally {
      setCsvLoading(false);
    }
  }, [pnlMode, buildParams]);

  // Sync loading ref with state for use in interval callbacks
  useEffect(() => {
    loadingRef.current = loading;
  }, [loading]);

  // Auto-refresh effect with countdown
  useEffect(() => {
    if (!autoRefresh) {
      prevAutoRefreshRef.current = false;
      setRefreshCountdown(REFRESH_SECONDS);
      return;
    }

    // When auto-refresh is first enabled (transition from false to true), trigger immediately
    if (!prevAutoRefreshRef.current) {
      setRefreshCountdown(REFRESH_SECONDS);
      prevAutoRefreshRef.current = true;
      // Trigger immediate refresh when auto-refresh is enabled
      if (!loadingRef.current) {
        runReport();
      }
    }

    // Countdown timer (updates every second)
    const countdownInterval = setInterval(() => {
      setRefreshCountdown(prev => {
        // Don't trigger if already loading or if countdown hasn't reached 0
        if (prev <= 1 && !loadingRef.current) {
          runReport();
          return REFRESH_SECONDS;
        }
        // Don't decrement countdown while loading
        if (loadingRef.current) {
          return prev;
        }
        return prev - 1;
      });
    }, 1000);

    return () => clearInterval(countdownInterval);
  }, [autoRefresh, runReport]);

  // Restore scroll position after report updates (only when we captured scroll)
  useEffect(() => {
    if (report && scrollPosRef.current > 0 && loading === false) {
      // Double-RAF for guaranteed DOM paint completion
      requestAnimationFrame(() => {
        requestAnimationFrame(() => {
          try {
            window.scrollTo({ top: scrollPosRef.current, behavior: 'auto' });

            // Restore table scroll positions
            if (bySymbolTableRef.current) {
              bySymbolTableRef.current.scrollTop = tableScrollPosRef.current.bySymbolScrollTop;
            }
            if (groupsTableContainerRef.current) {
              groupsTableContainerRef.current.scrollTop = tableScrollPosRef.current.groupsScrollTop;
            }
          } catch (e) {
            if (import.meta.env?.DEV) {
              console.warn('[PnL] Scroll restoration failed:', e);
            }
          }
        });
      });
    }
  }, [report, loading]);

  // Update bySymbolMapRef when report changes (prevents column recreation)
  useEffect(() => {
    bySymbolMapRef.current =
      report?.by_symbol && typeof report.by_symbol === 'object'
        ? report.by_symbol
        : {};
  }, [report?.by_symbol]);

  // Load available symbols on mount
  useEffect(() => {
    const loadSymbols = async () => {
      setSymbolsLoading(true);
      try {
        const symbols = await api.getAvailableSymbols();
        setAvailableSymbols(symbols);
      } catch (e) {
        if (import.meta.env?.DEV) {
          console.error('Failed to load symbols:', e);
        }
        // Keep empty array, will use fallback in render
      } finally {
        setSymbolsLoading(false);
      }
    };

    loadSymbols();
  }, []); // Run once on mount

  // Disconnect socket.io when PnL page is active (PnL doesn't need real-time data)
  // This prevents socket.io polling from blocking PnL API requests
  useEffect(() => {
    // Force disconnect immediately when PnL page loads
    // This is critical because socket.io long-polling can block HTTP requests
    if (import.meta.env?.DEV) {
      console.log('[PnL] Disconnecting socket.io to prevent blocking');
    }

    // Disconnect synchronously before any API calls
    disconnectSocket();

    // Small delay to ensure disconnect completes before any API requests
    const timeoutId = setTimeout(() => {
      if (import.meta.env?.DEV) {
        console.log('[PnL] Socket disconnect completed');
      }
    }, 100);

    // Reconnect when component unmounts (user navigates away to other pages)
    return () => {
      clearTimeout(timeoutId);
      if (import.meta.env?.DEV) {
        console.log('[PnL] Reconnecting socket.io for other pages');
      }
      connectSocket();
    };
  }, []);

  // Auto-run 24h report on mount
  useEffect(() => {
    if (!hasRunInitialReportRef.current) {
      hasRunInitialReportRef.current = true;
      // Small delay to ensure socket disconnect completes and symbols are loaded
      const timeoutId = setTimeout(() => {
        runReport();
      }, 200);
      return () => clearTimeout(timeoutId);
    }
  }, [runReport]);

  // Stable report signature for memoization
  const reportKey = useMemo(() => {
    return report ? ((report as any).report_signature ?? reportSignatureRef.current) : '';
  }, [report]);

  // Filtered groups (split from filteredAndSortedGroups for better performance)
  const filteredGroups = useMemo(() => {
    if (!report?.groups) return [];

    let filtered = report.groups;

    // Apply PnL filter
    if (pnlFilter === 'positive') {
      filtered = filtered.filter(g => g.pnl_bps >= 0);
    } else if (pnlFilter === 'negative') {
      filtered = filtered.filter(g => g.pnl_bps < 0);
    }

    // Apply symbol filter (from table filters / symbol cards)
    const symbolSearch = (groupFilters.symbol ?? '').trim().toLowerCase();
    if (symbolSearch) {
      filtered = filtered.filter((g) =>
        g.symbol.toLowerCase().includes(symbolSearch)
      );
    }

    const signalSearch = (groupFilters.signal ?? '').trim().toLowerCase();
    if (signalSearch) {
      filtered = filtered.filter((g) =>
        (g.signal_id || '').toLowerCase().includes(signalSearch)
      );
    }

    return filtered;
  }, [reportKey, groupFilters, pnlFilter]);

  // Sorted groups (memoized with stable report key to avoid unnecessary re-sorts)
  const filteredAndSortedGroups = useMemo(() => {
    const sorted = [...filteredGroups].sort((a, b) => {
      const aVal = a[sortColumn];
      const bVal = b[sortColumn];

      if (aVal === null || aVal === undefined) return 1;
      if (bVal === null || bVal === undefined) return -1;

      if (typeof aVal === 'string' && typeof bVal === 'string') {
        return sortDirection === 'asc' ? aVal.localeCompare(bVal) : bVal.localeCompare(aVal);
      }

      if (typeof aVal === 'number' && typeof bVal === 'number') {
        return sortDirection === 'asc' ? aVal - bVal : bVal - aVal;
      }

      // Log unexpected type mismatches in dev mode
      if (import.meta.env?.DEV) {
        console.warn(`[PnL] Unexpected sort comparison: ${typeof aVal} vs ${typeof bVal} for column ${sortColumn}`);
      }
      return 0;
    });

    return sorted;
  }, [reportKey, filteredGroups, sortColumn, sortDirection]);

  // Paginated groups
  const paginatedGroups = useMemo(() => {
    const start = (groupsPage - 1) * groupsPageSize;
    const end = start + groupsPageSize;
    return filteredAndSortedGroups.slice(start, end);
  }, [filteredAndSortedGroups, groupsPage, groupsPageSize]);

  useEffect(() => {
    setGroupsPage(1);
  }, [groupFilters, pnlFilter]);

  // Export groups table to clipboard (markdown format) - defined here before groupFilterControls
  const copyToClipboard = useCopyToClipboard();

  const exportToClipboard = useCallback(async () => {
    if (!paginatedGroups.length) {
      toast.error('No data to export');
      return;
    }

    const headers = ['Symbol', 'Signal ID', 'Start Time', 'DEX Side', 'DEX VWAP', 'CEX Side', 'CEX VWAP', 'Hedged Qty', 'PnL (bps)'];
    const headerRow = `| ${headers.join(' | ')} |`;
    const separator = `| ${headers.map(() => '---').join(' | ')} |`;

    const rows = paginatedGroups.map(g =>
      `| ${g.symbol} | ${g.signal_id || '-'} | ${formatLocal(g.start_time)} | ${g.dex_side} | ${formatters.fmtFixed(g.dex_vwap, 6)} | ${g.cex_side} | ${formatters.fmtFixed(g.cex_vwap, 6)} | ${formatters.fmtFixed(g.hedged_qty, 4)} | ${formatters.fmtFixed(g.pnl_bps, 2)} |`
    );

    const markdown = [headerRow, separator, ...rows].join('\n');

    await copyToClipboard(markdown, {
      successMessage: `Copied ${paginatedGroups.length} rows to clipboard`,
      errorMessage: 'Failed to copy PnL page to clipboard',
      showPreview: false,
    });
  }, [copyToClipboard, paginatedGroups]);

  const activeSymbolFilter = (groupFilters.symbol ?? '').trim();

  // Calculate filter counts for badges (single-pass for performance, memoized with stable key)
  const filterCounts = useMemo(() => {
    if (!report?.by_symbol || typeof report.by_symbol !== 'object') {
      return { all: 0, loss: 0, stale_fv: 0, low_coverage: 0 };
    }
    return Object.values(report.by_symbol).reduce(
      (counts, d) => {
        if (d && typeof d === 'object') {
          counts.all++;
          if (d.is_loss) counts.loss++;
          if (d.is_fv_stale) counts.stale_fv++;
          if (d.is_coverage_low) counts.low_coverage++;
        }
        return counts;
      },
      { all: 0, loss: 0, stale_fv: 0, low_coverage: 0 }
    );
  }, [reportKey]);

  // Pre-filter by_symbol data for performance (memoized with stable report key)
  const filteredBySymbol = useMemo(() => {
    if (!report?.by_symbol || typeof report.by_symbol !== 'object') {
      return {};
    }
    if (bySymbolFilter === 'all') return report.by_symbol;
    return Object.fromEntries(
      Object.entries(report.by_symbol).filter(([, data]) => {
        if (!data || typeof data !== 'object') return false;
        if (bySymbolFilter === 'loss' && !data.is_loss) return false;
        if (bySymbolFilter === 'stale_fv' && !data.is_fv_stale) return false;
        if (bySymbolFilter === 'low_coverage' && !data.is_coverage_low) return false;
        return true;
      })
    );
  }, [reportKey, bySymbolFilter]);

  // Sorted by_symbol entries with optional truncation
  const bySymbolEntries = useMemo(() => {
    const entries = Object.entries(filteredBySymbol);
    const getValue = (entry: [string, any]) => {
      const [symbol, data] = entry;
      const safeNumber = (val: unknown) => (Number.isFinite(Number(val)) ? Number(val) : 0);
      switch (bySymbolSort.column) {
        case 'symbol':
          return symbol;
        case 'gross_bps':
          return safeNumber((data as any)?.gross_bps);
        case 'net_bps':
          return safeNumber((data as any)?.net_bps ?? (data as any)?.avg_realized_pnl_bps);
        case 'm2m_usd':
          return safeNumber((data as any)?.m2m_usd);
        case 'matched_notional':
          return safeNumber((data as any)?.matched_notional);
        case 'buy_notional':
          return safeNumber((data as any)?.buy_notional);
        case 'sell_notional':
          return safeNumber((data as any)?.sell_notional);
        case 'gross_flow':
        default:
          return safeNumber((data as any)?.gross_flow);
      }
    };

    const sorted = entries.sort((a, b) => {
      const aVal = getValue(a);
      const bVal = getValue(b);
      let result = 0;

      if (typeof aVal === 'string' || typeof bVal === 'string') {
        result = String(aVal).localeCompare(String(bVal));
      } else {
        result = aVal - bVal;
      }

      if (result === 0) {
        result = a[0].localeCompare(b[0]);
      }

      return bySymbolSort.direction === 'asc' ? result : -result;
    });

    return showAllBySymbolRows ? sorted : sorted.slice(0, BY_SYMBOL_MAX_ROWS);
  }, [filteredBySymbol, bySymbolSort, showAllBySymbolRows]);

  // Note: virtualization can be added here if dataset grows beyond the default cap.

  const headerLabel = useCallback((label: string, align: 'left' | 'center' | 'right' = 'left') => (
    <span
      style={{
        display: 'inline-flex',
        width: '100%',
        justifyContent:
          align === 'right'
            ? 'flex-end'
            : align === 'center'
              ? 'center'
              : 'flex-start',
        textAlign: align,
      }}
    >
      {label}
    </span>
  ), []);

  const groupColumns = useMemo<ColumnDef<PnLGroup>[]>(() => {
    return [
      {
        accessorKey: 'symbol',
        header: headerLabel('Symbol'),
        size: 100,
        minSize: 90,
        enableSorting: true,
        cell: ({ row }: PnLRowContext) => {
          const symbol = row.original.symbol;
          const meta = bySymbolMapRef.current[symbol] ?? {};
          const fvSource = typeof meta.fv_source === 'string' ? meta.fv_source : null;
          const fvAge = typeof meta.fv_age_ms === 'number' ? meta.fv_age_ms : null;
          const fxSynth = Boolean(meta.fx_synth);
          const fxMissing = Boolean(meta.fx_missing);

          const sourcePalette = fvSource === 'snapshot'
            ? severity.success
            : fvSource === 'strategy'
              ? severity.info
              : severity.warning;

          return (
            <div
              className="flex items-center"
              style={{ gap: spacing.gap.xs, whiteSpace: 'nowrap', overflow: 'hidden', textOverflow: 'ellipsis' }}
            >
              <span
                style={{
                  fontFamily: typography.fontFamily.mono,
                  color: colors.text.primary,
                  fontSize: typography.fontSize.sm,
                  fontWeight: typography.fontWeight.semibold,
                }}
              >
                {symbol}
              </span>
              {fvSource && (
                <span
                  style={tinyTagStyle(
                    fvSource === 'snapshot'
                      ? 'success'
                      : fvSource === 'strategy'
                        ? 'info'
                        : 'warning'
                  )}
                  title={fvAge !== null ? `Age: ${fvAge}ms` : undefined}
                >
                  {fvSource}
                </span>
              )}
              {fxSynth && (
                <span style={tinyTagStyle('info')}>fx:synth</span>
              )}
              {fxMissing && (
                <span style={tinyTagStyle('danger')}>fx:missing</span>
              )}
            </div>
          );
        },
      },
      {
        accessorKey: 'signal_id',
        header: headerLabel('Signal'),
        size: 240,
        minSize: 180,
        enableSorting: true,
        cell: ({ row }: PnLRowContext) => {
          const signal = row.original.signal_id || '—';
          return (
            <SimpleTooltip content={signal} delay={200}>
              <span
                style={groupCellStyle({ mono: true, muted: true })}
              >
                {signal}
              </span>
            </SimpleTooltip>
          );
        },
      },
      {
        accessorKey: 'start_time',
        header: headerLabel('Start Time'),
        size: 190,
        minSize: 170,
        enableSorting: true,
        cell: ({ row }: PnLRowContext) => (
          <span
            style={groupCellStyle({ muted: true })}
          >
            {formatLocal(row.original.start_time)}
          </span>
        ),
      },
      {
        accessorKey: 'dex_side',
        header: headerLabel('DEX Side', 'center'),
        size: 82,
        minSize: 72,
        enableSorting: false,
        cell: ({ row }: PnLRowContext) => (
          <div style={{ minWidth: 72, textAlign: 'center' }}>
            <span style={tinyTagStyle(row.original.dex_side === 'buy' ? 'success' : 'danger')}>
              {row.original.dex_side}
            </span>
          </div>
        ),
      },
      {
        accessorKey: 'dex_vwap',
        header: headerLabel('DEX VWAP', 'right'),
        size: 120,
        minSize: 110,
        enableSorting: true,
        cell: ({ row }: PnLRowContext) => (
          <span
            style={groupCellStyle({ align: 'right', mono: true })}
          >
            {formatters.fmtFixed(row.original.dex_vwap, 6)}
          </span>
        ),
      },
      {
        accessorKey: 'cex_side',
        header: headerLabel('CEX Side', 'center'),
        size: 82,
        minSize: 72,
        enableSorting: false,
        cell: ({ row }: PnLRowContext) => (
          <div style={{ minWidth: 72, textAlign: 'center' }}>
            <span style={tinyTagStyle(row.original.cex_side === 'buy' ? 'success' : 'danger')}>
              {row.original.cex_side}
            </span>
          </div>
        ),
      },
      {
        accessorKey: 'cex_vwap',
        header: headerLabel('CEX VWAP', 'right'),
        size: 120,
        minSize: 110,
        enableSorting: true,
        cell: ({ row }: PnLRowContext) => (
          <span
            style={groupCellStyle({ align: 'right', mono: true })}
          >
            {formatters.fmtFixed(row.original.cex_vwap, 6)}
          </span>
        ),
      },
      {
        id: 'fv_now',
        header: headerLabel('FV(now)', 'right'),
        size: 120,
        minSize: 110,
        enableSorting: false,
        cell: ({ row }: PnLRowContext) => {
          const meta = bySymbolMapRef.current[row.original.symbol] ?? {};
          const fvNow = typeof meta.fv_now === 'number' ? meta.fv_now : null;
          const fvAge = typeof meta.fv_age_ms === 'number' ? meta.fv_age_ms : null;
          const source = typeof meta.fv_source === 'string' ? meta.fv_source : undefined;
          const ageLabel = formatFvAge(fvAge);
          const titleParts = [
            source ? `Source: ${source}` : null,
            ageLabel ? `Age: ${ageLabel}` : null,
          ].filter(Boolean);
          return (
            <span
              style={groupCellStyle({ align: 'right', mono: true })}
              title={titleParts.length ? titleParts.join(' | ') : undefined}
            >
              {fvNow !== null ? formatters.fmtFixed(fvNow, 6) : '—'}
            </span>
          );
        },
      },
      {
        accessorKey: 'hedged_qty',
        header: headerLabel('Hedged Qty', 'right'),
        size: 120,
        minSize: 110,
        enableSorting: true,
        cell: ({ row }: PnLRowContext) => (
          <span
            style={groupCellStyle({ align: 'right', mono: true })}
          >
            {formatters.fmtQty(row.original.hedged_qty, 4)}
          </span>
        ),
      },
      {
        accessorKey: 'pnl_bps',
        header: headerLabel('PnL (bps/$)', 'right'),
        size: 155,
        minSize: 130,
        enableSorting: true,
        cell: ({ row }: PnLRowContext) => {
          const pnlBps = row.original.pnl_bps ?? 0;
          const pnlUsd = row.original.pnl_usd;
          const palette = pnlBps >= 0 ? severity.success : severity.critical;
          return (
            <span
              style={groupCellStyle({
                align: 'right',
                mono: true,
                emphasis: true,
                color: palette.color ?? '#10b981',
              })}
            >
              {formatters.fmtDualPnL(pnlBps, pnlUsd)}
            </span>
          );
        },
      },
      ...(showGroupsPnlUsd ? [{
        accessorKey: 'pnl_usd',
        header: headerLabel('PnL ($)', 'right'),
        size: 150,
        minSize: 130,
        enableSorting: true,
        cell: ({ row }: PnLRowContext) => {
          const val = row.original.pnl_usd;
          if (val === undefined || val === null) return '—';
          const palette = val >= 0 ? severity.success : severity.critical;
          return (
            <span
              style={groupCellStyle({
                align: 'right',
                mono: true,
                emphasis: true,
                color: palette.color ?? '#10b981',
              })}
            >
              {formatters.fmtMoney(val, 2)}
            </span>
          );
        },
      }] : []),
      ...(decisionDetailsEnabled ? [{
        id: 'decision_delta',
        header: 'Decision vs Realized',
        enableSorting: false,
        cell: ({ row }: PnLRowContext) => {
          const edgeValue = Number(row.original.decision_edge_bps_net);
          const realizedValue = Number(row.original.pnl_bps);
          const requiredValue = Number(row.original.decision_required_bps);
          const delta =
            Number.isFinite(edgeValue) && Number.isFinite(realizedValue)
              ? realizedValue - edgeValue
              : null;
          const color =
            delta == null
              ? colors.text.muted
              : delta >= 0
                ? severity.success.color
                : severity.critical.color;
          return (
            <div
              style={{
                display: 'flex',
                flexDirection: 'column',
                gap: spacing.gap.xs,
                fontSize: typography.fontSize.xs,
                color: colors.text.secondary,
              }}
            >
              <div>edge: {Number.isFinite(edgeValue) ? formatters.fmtFixed(edgeValue, 2) : '—'} bps</div>
              <div>realized: {Number.isFinite(realizedValue) ? formatters.fmtFixed(realizedValue, 2) : '—'} bps</div>
              <div>required: {Number.isFinite(requiredValue) ? formatters.fmtFixed(requiredValue, 2) : '—'} bps</div>
              <div style={{ color }}>
                Δ: {delta == null ? '—' : formatters.fmtFixed(delta, 2)} bps
              </div>
            </div>
          );
        },
      }] : []),
    ];
  }, [showGroupsPnlUsd, decisionDetailsEnabled]);

  const tableSortingState = useMemo<SortingState>(() => {
    return [{ id: sortColumn, desc: sortDirection === 'desc' }];
  }, [sortColumn, sortDirection]);

  const groupCellBase: CSSProperties = {
    display: 'block',
    whiteSpace: 'nowrap',
    overflow: 'hidden',
    textOverflow: 'ellipsis',
    fontSize: typography.fontSize.sm,
    lineHeight: '1.4',
    color: colors.text.secondary,
  };

  const groupCellStyle = ({
    align = 'left',
    mono = false,
    muted = false,
    emphasis = false,
    color,
  }: {
    align?: 'left' | 'center' | 'right';
    mono?: boolean;
    muted?: boolean;
    emphasis?: boolean;
    color?: string;
  }): CSSProperties => ({
    ...groupCellBase,
    textAlign: align,
    fontFamily: mono ? typography.fontFamily.mono : typography.fontFamily.sans,
    color: color ?? (muted ? colors.text.muted : colors.text.secondary),
    fontWeight: emphasis ? typography.fontWeight.semibold : typography.fontWeight.normal,
  });

  const handleTableSortingChange = useCallback(
    (sorting: SortingState) => {
      if (!sorting.length) {
        // Reset to default sort instead of early return to support 3-state cycle
        setSortColumn('start_time');
        setSortDirection('desc');
        return;
      }
      const [first] = sorting;
      setSortColumn(first.id as SortColumn);
      setSortDirection(first.desc ? 'desc' : 'asc');
    },
    []
  );

  // Stable row ID to prevent row remounting on data updates
  const getRowId = useCallback((row: PnLGroup) => {
    return `${row.symbol}_${row.signal_id ?? 'null'}_${row.start_time}`;
  }, []);

  const panelStyle = panelStyleBase;
  const inputStyle = inputStyleBase;
  const labelStyle = labelStyleBase;
  const tableHeaderStyle = tableHeaderStyleBase;

  const groupFilterControls = useMemo(() => (
    <div
      className="flex flex-wrap items-center"
      style={{
        gap: spacing.gap.xs,
        justifyContent: 'space-between',
        alignItems: 'center',
      }}
    >
      <div
        className="flex items-center"
        style={{
          gap: 0,
          border: `1px solid ${colors.border.DEFAULT}`,
          borderRadius: borderRadius.sm,
          overflow: 'hidden',
          backgroundColor: semanticTokens.surfaceAlt,
        }}
      >
        {(['all', 'positive', 'negative'] as PnLFilter[]).map((option, idx) => (
          <Button
            key={option}
            size="xs"
            variant="ghost"
            aria-pressed={pnlFilter === option}
            data-selected={pnlFilter === option}
            style={{
              ...segmentedControlStyle(pnlFilter === option),
              borderLeft: idx === 0 ? undefined : `1px solid ${colors.border.DEFAULT}`,
              borderRadius: 0,
              paddingInline: spacing.gap.sm,
            }}
            onClick={() => setPnlFilter(option)}
          >
            {option === 'all' ? 'All' : option === 'positive' ? '+PnL' : '-PnL'}
          </Button>
        ))}
      </div>
      <div className="flex items-center" style={{ gap: spacing.gap.xs }}>
        <Switch
          size="sm"
          checked={showGroupsPnlUsd}
          onCheckedChange={(checked) => setShowGroupsPnlUsd(Boolean(checked))}
          label="Show PnL ($)"
        />
        <Button
          size="xs"
          variant="ghost"
          onClick={exportToClipboard}
          disabled={!paginatedGroups.length}
        >
          Copy Page
        </Button>
      </div>
    </div>
  ), [pnlFilter, exportToClipboard, paginatedGroups.length, showGroupsPnlUsd]);

  const handleBySymbolSort = useCallback((column: BySymbolSortKey) => {
    setBySymbolSort((prev) => {
      const nextDirection =
        prev.column === column ? (prev.direction === 'asc' ? 'desc' : 'asc') : column === 'symbol' ? 'asc' : 'desc';
      return { column, direction: nextDirection };
    });
  }, []);

  const renderControlPanel = () => {
    const controlPanelStyle: CSSProperties = { ...panelStyle, padding: spacing.gap.md };
    const compactStackStyle: CSSProperties = { gap: spacing.gap.xs };

    return (
      <div style={controlPanelStyle}>
        <div className="flex flex-wrap items-center" style={{ gap: spacing.gap.sm, rowGap: spacing.gap.sm }}>
          <div className="flex flex-col" style={compactStackStyle}>
            <span style={labelStyle}>Mode</span>
            <div
              className="flex items-center"
              style={{
                gap: 0,
                border: `1px solid ${colors.border.DEFAULT}`,
                borderRadius: borderRadius.sm,
                overflow: 'hidden',
                backgroundColor: semanticTokens.surfaceAlt,
              }}
            >
              {(['spread', 'inventory'] as const).map((m, idx) => (
                <Button
                  key={m}
                  size="xs"
                  variant="ghost"
                  aria-pressed={pnlMode === m}
                  data-selected={pnlMode === m}
                  style={{
                    ...segmentedControlStyle(pnlMode === m),
                    borderLeft: idx === 0 ? undefined : `1px solid ${colors.border.DEFAULT}`,
                    borderRadius: 0,
                    paddingInline: spacing.gap.sm,
                  }}
                  onClick={() => {
                    setPnlMode(m);
                    setReport(null);
                    setInventoryReport(null);
                    setError(null);
                    lastEtagRef.current = null;
                    hasReportRef.current = false;
                    groupHashesRef.current = {};
                    symbolHashesRef.current = {};
                    unhedgedHashesRef.current = {};
                    reportSignatureRef.current = '';
                  }}
                >
                  {m === 'spread' ? 'Spread' : 'Inventory (basis lifecycle)'}
                </Button>
              ))}
            </div>
            <div style={{ marginTop: spacing.gap.xs, maxWidth: 320, color: colors.text.MUTED, fontSize: 12, lineHeight: 1.3 }}>
              Spread pairs legs via signal timing. Inventory attributes realized/unrealized PnL over time (fees + carry).
            </div>
          </div>

          <div className="flex flex-col" style={compactStackStyle}>
            <span style={labelStyle}>Time Window</span>
            <div className="flex flex-wrap" style={{ gap: spacing.gap.xs, rowGap: spacing.gap.xs }}>
              {TIME_OPTION_LABELS.map(({ key, label }) => (
                <Button
                  key={key}
                  size="xs"
                  variant="ghost"
                  data-selected={timeWindowMode === key}
                  aria-pressed={timeWindowMode === key}
                  style={segmentedControlStyle(timeWindowMode === key)}
                  onClick={() => handleTimeWindowModeChange(key)}
                >
                  {label}
                </Button>
              ))}
            </div>
          </div>

          {timeWindowMode === 'custom' && (
            <div className="flex flex-col" style={compactStackStyle}>
              <span style={labelStyle}>Minutes</span>
              <input
                type="number"
                value={minutes}
                onChange={(e) => {
                  const newMinutes = parseInt(e.target.value, 10) || 60;
                  setMinutes(newMinutes);
                  // Update range values to match custom minutes (in UTC)
                  const nowUtc = dayjs.utc();
                  const endUtcIso = nowUtc.toISOString();
                  const startUtcIso = nowUtc.subtract(newMinutes, 'minute').toISOString();
                  setEndUtc(endUtcIso);
                  setStartUtc(startUtcIso);
                }}
                style={{ ...inputStyle, width: '96px' }}
                min={1}
              />
            </div>
          )}

          {timeWindowMode === 'last' && (
            <div className="flex flex-col" style={compactStackStyle}>
              <span style={labelStyle}>Last N</span>
              <input
                type="number"
                value={last}
                onChange={(e) => setLast(parseInt(e.target.value, 10) || 100)}
                style={{ ...inputStyle, width: '96px' }}
                min={1}
              />
            </div>
          )}

          {timeWindowMode === 'range' && (
            <>
              <UtcDateTimeField
                label="Start (UTC)"
                value={startUtc}
                inputValue={startInput}
                onInputChange={setStartInput}
                onValueChange={(utcIso: string) => {
                  setStartUtc(utcIso);
                  setStartError('');
                }}
                onError={setStartError}
                error={startError}
                formatUtcForInput={formatUtcForInput}
                parseUtcInput={parseUtcInput}
                formatUtcAsLocalHelper={formatUtcAsLocalHelper}
                inputStyle={inputStyle}
                labelStyle={labelStyle}
              />
              <UtcDateTimeField
                label="End (UTC)"
                value={endUtc}
                inputValue={endInput}
                onInputChange={setEndInput}
                onValueChange={(utcIso: string) => {
                  setEndUtc(utcIso);
                  setEndError('');
                }}
                onError={setEndError}
                error={endError}
                formatUtcForInput={formatUtcForInput}
                parseUtcInput={parseUtcInput}
                formatUtcAsLocalHelper={formatUtcAsLocalHelper}
                inputStyle={inputStyle}
                labelStyle={labelStyle}
              />
            </>
          )}

          {pnlMode === 'spread' ? (
            <>
              <div className="flex flex-col" style={compactStackStyle}>
                <span style={labelStyle}>Base</span>
                <select
                  value={base ?? 'all'}
                  onChange={(e) => setBase(e.target.value === 'all' ? null : e.target.value)}
                  style={{ ...inputStyle, width: '140px' }}
                  disabled={symbolsLoading}
                >
                  <option value="all">
                    {symbolsLoading ? 'Loading…' : 'All Bases'}
                  </option>
                  {availableSymbols.map((symbol) => (
                    <option key={symbol} value={symbol}>
                      {symbol}
                    </option>
                  ))}
                </select>
              </div>

              <Button
                size="xs"
                variant="ghost"
                onClick={() => setShowAdvanced((prev) => !prev)}
              >
                {showAdvanced ? 'Hide Advanced' : 'Show Advanced'}
              </Button>
            </>
          ) : (
            <>
              <div className="flex flex-col" style={compactStackStyle}>
                <span style={labelStyle}>Suite</span>
                <input
                  type="text"
                  value={invSuite}
                  onChange={(e) => setInvSuite(e.target.value)}
                  style={{ ...inputStyle, width: '140px' }}
                  placeholder="hl_ibkr"
                />
              </div>
              <div className="flex flex-col" style={compactStackStyle}>
                <span style={labelStyle}>Strategy Class</span>
                <input
                  type="text"
                  value={invStrategyClass}
                  onChange={(e) => setInvStrategyClass(e.target.value)}
                  style={{ ...inputStyle, width: '140px' }}
                  placeholder="basis_arb"
                />
              </div>
              <div className="flex flex-col" style={compactStackStyle}>
                <span style={labelStyle}>Strategy IDs</span>
                <input
                  type="text"
                  value={invStrategyIds}
                  onChange={(e) => setInvStrategyIds(e.target.value)}
                  style={{ ...inputStyle, width: '200px' }}
                  placeholder="hl_ibkr_nvda,..."
                />
              </div>
              <div className="flex flex-col" style={compactStackStyle}>
                <span style={labelStyle}>Symbols</span>
                <input
                  type="text"
                  value={invSymbols}
                  onChange={(e) => setInvSymbols(e.target.value)}
                  style={{ ...inputStyle, width: '180px' }}
                  placeholder="US.NVDA/USD"
                />
              </div>
              <div className="flex flex-col" style={compactStackStyle}>
                <span style={labelStyle}>Exchanges</span>
                <input
                  type="text"
                  value={invExchanges}
                  onChange={(e) => setInvExchanges(e.target.value)}
                  style={{ ...inputStyle, width: '160px' }}
                  placeholder="hl,ibkr"
                />
              </div>
              <div className="flex flex-col" style={compactStackStyle}>
                <span style={labelStyle}>Invalid Rows</span>
                <Switch
                  size="sm"
                  checked={invIncludeInvalid}
                  onCheckedChange={(checked) => setInvIncludeInvalid(Boolean(checked))}
                  label="Show"
                />
              </div>
            </>
          )}
        </div>

        {pnlMode === 'spread' && showAdvanced && (
          <div
            className="flex flex-wrap items-center"
            style={{
              gap: spacing.gap.sm,
              marginTop: spacing.gap.sm,
              paddingTop: spacing.gap.sm,
              borderTop: `1px solid ${colors.border.DEFAULT}`,
            }}
          >
            <div className="flex flex-col" style={compactStackStyle}>
              <span style={labelStyle}>DEX Fee (bps)</span>
              <input
                type="number"
                step="0.1"
                value={dexFeeBps}
                onChange={(e) => setDexFeeBps(parseFloat(e.target.value) || 2.0)}
                style={{ ...inputStyle, width: '96px' }}
              />
            </div>
            <div className="flex flex-col" style={compactStackStyle}>
              <span style={labelStyle}>CEX Fee (bps)</span>
              <input
                type="number"
                step="0.1"
                value={cexFeeBps}
                onChange={(e) => setCexFeeBps(parseFloat(e.target.value) || 5.0)}
                style={{ ...inputStyle, width: '96px' }}
              />
            </div>
            <div className="flex flex-col" style={compactStackStyle}>
              <span style={labelStyle}>DEX</span>
              <input
                type="text"
                value={dex}
                onChange={(e) => setDex(e.target.value)}
                style={{ ...inputStyle, width: '128px' }}
              />
            </div>
            <div className="flex flex-col" style={compactStackStyle}>
              <span style={labelStyle}>CEX</span>
              <input
                type="text"
                value={cex}
                onChange={(e) => setCex(e.target.value)}
                style={{ ...inputStyle, width: '128px' }}
              />
            </div>
          </div>
        )}
      </div>
    );
  };

  const renderSummarySection = () => {
    if (!report) {
      return null;
    }

    const summary = report.summary;
    const netPositive = summary.net_pnl_bps >= 5;
    const netNegative = summary.net_pnl_bps < 0;
    const netColor = netPositive
      ? semanticTokens.status.success
      : netNegative
        ? semanticTokens.status.danger
        : colors.text.secondary;

    const cardStyle: CSSProperties = {
      backgroundColor: semanticTokens.surfaceAlt,
      borderRadius: borderRadius.md,
      padding: spacing.gap.sm,
      border: `1px solid ${colors.border.DEFAULT}`,
      display: 'flex',
      flexDirection: 'column',
      gap: 4,
    };

    const labelTight: CSSProperties = {
      ...labelStyle,
      color: semanticTokens.textMuted,
      fontSize: typography.fontSize['2xs'],
    };

    return (
      <div style={{ ...panelStyle, padding: spacing.gap.md }}>
        <div className="flex flex-wrap items-center justify-between" style={{ gap: spacing.gap.sm }}>
          <div className="flex items-center" style={{ gap: spacing.gap.sm }}>
            <h2
              style={{
                fontSize: typography.fontSize.lg,
                fontWeight: typography.fontWeight.semibold,
                color: semanticTokens.textPrimary,
                margin: 0,
              }}
            >
              {activeSymbolFilter
                ? `${activeSymbolFilter} Summary`
                : base
                  ? `${base} Summary`
                  : 'Overall Summary'}
            </h2>
            {report.asof && (
              <span
                style={{
                  fontSize: typography.fontSize.xs,
                  color: semanticTokens.textMuted,
                }}
                title={(() => {
                  // Show local time in tooltip
                  try {
                    const utcDate = new Date(report.asof);
                    if (!Number.isNaN(utcDate.getTime())) {
                      return `Local: ${formatLocal(report.asof)}`;
                    }
                  } catch {}
                  return '';
                })()}
              >
                as of {(() => {
                  // Format UTC timestamp explicitly
                  try {
                    const utcDate = new Date(report.asof);
                    if (Number.isNaN(utcDate.getTime())) return formatLocal(report.asof);
                    // Format as MM/DD/YY, HH:mm:ss AM/PM UTC
                    const month = String(utcDate.getUTCMonth() + 1).padStart(2, '0');
                    const day = String(utcDate.getUTCDate()).padStart(2, '0');
                    const year = String(utcDate.getUTCFullYear()).slice(-2);
                    const hours = utcDate.getUTCHours();
                    const minutes = String(utcDate.getUTCMinutes()).padStart(2, '0');
                    const seconds = String(utcDate.getUTCSeconds()).padStart(2, '0');
                    const ampm = hours >= 12 ? 'PM' : 'AM';
                    const hours12 = hours % 12 || 12;
                    return `${month}/${day}/${year}, ${hours12}:${minutes}:${seconds} ${ampm} UTC`;
                  } catch {
                    return formatLocal(report.asof) + ' UTC';
                  }
                })()}
              </span>
            )}
          </div>
          <div className="flex items-center" style={{ gap: spacing.gap.xs }}>
            <Switch
              size="sm"
              checked={unitPrimary === 'usd'}
              onCheckedChange={(checked) => setUnitPrimary(checked ? 'usd' : 'bps')}
              label={unitPrimary === 'usd' ? 'Unit: USD' : 'Unit: bps'}
            />
          </div>
        </div>

        <div
          className="grid"
          style={{
            marginTop: spacing.gap.sm,
            gap: spacing.gap.md,
            gridTemplateColumns: 'repeat(auto-fit, minmax(160px, 1fr))',
          }}
        >
          <div style={cardStyle}>
            <span style={labelTight}>Gross</span>
            <div
              style={{
                fontSize: '20px',
                fontWeight: typography.fontWeight.semibold,
                color: semanticTokens.textPrimary,
              }}
            >
              {fmtPrimary(summary.weighted_pnl_bps, summary.weighted_pnl_usd)}
            </div>
            <div style={{ color: semanticTokens.textMuted, fontSize: typography.fontSize.xs }}>
              {unitPrimary === 'usd'
                ? `${formatters.fmtFixed(summary.weighted_pnl_bps, 2)} bps`
                : `${formatters.fmtMoney(summary.weighted_pnl_usd, 2)}`}
            </div>
          </div>

          <div style={cardStyle}>
            <span style={labelTight}>Net</span>
            <div
              style={{
                fontSize: '20px',
                fontWeight: typography.fontWeight.semibold,
                color: netColor,
              }}
            >
              {fmtPrimary(summary.net_pnl_bps, summary.net_pnl_usd)}
            </div>
            <div
              style={{
                color: semanticTokens.textMuted,
                fontSize: typography.fontSize.xs,
                display: 'flex',
                flexWrap: 'wrap',
                gap: spacing.gap.xs,
                alignItems: 'center',
              }}
            >
              <span>
                {unitPrimary === 'usd'
                  ? `${formatters.fmtFixed(summary.net_pnl_bps, 2)} bps`
                  : `${formatters.fmtMoney(summary.net_pnl_usd, 2)}`}
              </span>
              <span style={microBadgeStyle('success')}>
                <span style={statusDot(semanticTokens.status.success)} aria-hidden="true" />
                <span>Fills {summary.fills_grouped}/{summary.fills_total}</span>
              </span>
            </div>
          </div>

          <div style={cardStyle}>
            <span style={labelTight}>Hedge</span>
            <div
              style={{
                fontSize: '20px',
                fontWeight: typography.fontWeight.semibold,
                color: colors.text.secondary,
              }}
            >
              {formatters.fmtFixed((summary.hedge_ratio || 0) * 100, 1)}%
            </div>
            <div style={{ color: semanticTokens.textMuted, fontSize: typography.fontSize.xs }}>
              Qty: {formatters.fmtQty(summary.total_hedged_qty, 2)}
            </div>
          </div>

          <div style={cardStyle}>
            <span style={labelTight}>Gross Flow</span>
            <div
              style={{
                fontSize: '20px',
                fontWeight: typography.fontWeight.semibold,
                color: colors.text.secondary,
              }}
            >
              {formatters.fmtMoney(summary.gross_traded_notional_usd ?? 0, 0)}
            </div>
          </div>

          <div style={cardStyle}>
            <div className="flex items-center justify-between" style={{ gap: spacing.gap.xs }}>
              <span style={labelTight}>Matched Notional</span>
            </div>
            <div
              style={{
                fontSize: '20px',
                fontWeight: typography.fontWeight.semibold,
                color: colors.text.secondary,
              }}
            >
              {formatters.fmtMoney(
                summary.matched_notional_usd ?? summary.total_notional,
                0
              )}
            </div>
            {typeof summary.hedge_ratio === 'number' && (
              <div style={{ color: semanticTokens.textMuted, fontSize: typography.fontSize.xs }}>
                Hedge {formatters.fmtFixed((summary.hedge_ratio || 0) * 100, 1)}% of flow
              </div>
            )}
          </div>

          <div style={cardStyle}>
            <div className="flex items-center justify-between" style={{ gap: spacing.gap.xs }}>
              <span style={labelTight}>Fees</span>
              <div
                style={{ fontSize: typography.fontSize.xs, color: semanticTokens.textMuted }}
                title="Total trading fees (CEX taker + DEX swap costs)"
              >
                ℹ️
              </div>
            </div>
            <div
              style={{
                fontSize: '20px',
                fontWeight: typography.fontWeight.semibold,
                color: colors.text.muted,
              }}
            >
              {summary.fees_bps !== undefined && summary.fees_bps !== null
                ? `${formatters.fmtFixed(summary.fees_bps, 2)} bps`
                : '—'}
            </div>
            {summary.fees_usd !== undefined && summary.fees_usd !== null && (
              <div style={{ color: semanticTokens.textMuted, fontSize: typography.fontSize.xs }}>
                ${Math.abs(summary.fees_usd).toFixed(2)}
              </div>
            )}
          </div>
        </div>
      </div>
    );
  };

  const renderSymbolSummarySection = () => {
    if (!report?.by_symbol || Object.keys(report.by_symbol).length === 0) {
      return null;
    }

    const renderSortableHeaderLabel = (
      label: string,
      sortKey: BySymbolSortKey | null,
      align: CSSProperties['textAlign'],
      tooltip?: string | null
    ) => {
      const isActive = sortKey ? bySymbolSort.column === sortKey : false;
      const icon = isActive ? (bySymbolSort.direction === 'desc' ? '↓' : '↑') : sortKey ? '↕' : '';
      const content = (
        <button
          type="button"
          onClick={sortKey ? () => handleBySymbolSort(sortKey) : undefined}
          aria-pressed={isActive}
          style={{
            display: 'inline-flex',
            alignItems: 'center',
            gap: 4,
            justifyContent: align === 'right' ? 'flex-end' : align === 'center' ? 'center' : 'flex-start',
            width: '100%',
            background: 'transparent',
            border: 'none',
            padding: 0,
            color: colors.text.muted,
            cursor: sortKey ? 'pointer' : 'default',
            fontWeight: isActive ? typography.fontWeight.semibold : typography.fontWeight.medium,
          }}
        >
          <span>{label}</span>
          {sortKey ? (
            <span aria-hidden="true" style={{ opacity: isActive ? 1 : 0.5 }}>
              {icon}
            </span>
          ) : null}
        </button>
      );

      if (tooltip) {
        return (
          <SimpleTooltip content={tooltip} side="top" delay={300}>
            {content}
          </SimpleTooltip>
        );
      }
      return content;
    };

    return (
      <div style={{ ...panelStyle, padding: spacing.gap.md }}>
        <h3
          style={{
            fontSize: typography.fontSize.md,
            fontWeight: typography.fontWeight.semibold,
            color: semanticTokens.textPrimary,
            marginBottom: spacing.gap.md,
          }}
        >
          By Symbol
        </h3>
        <div style={{ display: 'flex', gap: spacing.gap.sm, marginBottom: spacing.gap.md, flexWrap: 'wrap', alignItems: 'center' }}>
          <span style={{ fontSize: typography.fontSize.sm, color: semanticTokens.textMuted }}>
            All ({filterCounts.all})
          </span>
          {([
            { key: 'loss' as const, label: 'Loss only', count: filterCounts.loss, tone: semanticTokens.status.danger },
            { key: 'stale_fv' as const, label: 'Stale FV', count: filterCounts.stale_fv, tone: semanticTokens.status.warning },
            { key: 'low_coverage' as const, label: 'Low coverage', count: filterCounts.low_coverage, tone: semanticTokens.status.warning },
          ]).map(({ key, label, count, tone }) => {
            const active = bySymbolFilter === key;
            return (
              <label
                key={key}
                className="flex items-center"
                style={{
                  gap: spacing.gap.xs,
                  color: active ? semanticTokens.textPrimary : semanticTokens.textMuted,
                  fontSize: typography.fontSize.sm,
                  cursor: 'pointer',
                }}
              >
                <input
                  type="checkbox"
                  checked={active}
                  onChange={() => setBySymbolFilter(active ? 'all' : key)}
                  style={{ accentColor: tone, cursor: 'pointer' }}
                />
                <span>
                  {label}
                  {count ? ` (${count})` : ''}
                </span>
              </label>
            );
          })}
          {bySymbolFilter !== 'all' && (
            <Button
              variant="ghost"
              size="xs"
              onClick={() => setBySymbolFilter('all')}
              style={{ marginLeft: spacing.gap.xs }}
            >
              Clear
            </Button>
          )}
          <Button
            variant="ghost"
            size="xs"
            onClick={() => setShowBySymbolMore(!showBySymbolMore)}
            style={{ paddingInline: 0, height: 'auto', color: semanticTokens.textMuted }}
          >
            {showBySymbolMore ? 'Show less' : 'Show more'}
          </Button>
          <span style={{ fontSize: typography.fontSize.xs, color: semanticTokens.textMuted }}>
            Showing {showAllBySymbolRows ? bySymbolEntries.length : Math.min(bySymbolEntries.length, BY_SYMBOL_MAX_ROWS)} of {bySymbolEntries.length}
          </span>
          {bySymbolEntries.length > BY_SYMBOL_MAX_ROWS && (
            <Button
              variant="ghost"
              size="xs"
              onClick={() => setShowAllBySymbolRows((v) => !v)}
            >
              {showAllBySymbolRows ? 'Show Top Only' : 'Show All Rows'}
            </Button>
          )}
        </div>
        <div ref={bySymbolTableRef} style={{ overflowX: 'auto', overflowY: 'auto' }}>
          <table
            style={{
              width: '100%',
              borderCollapse: 'separate',
              borderSpacing: 0,
              fontSize: typography.fontSize.xs,
              fontVariantNumeric: 'tabular-nums',
            }}
          >
            <thead>
              {/* Group header row */}
              <tr
                style={{
                  position: 'sticky',
                  top: 0,
                  backgroundColor: colors.bg.surface,
                  zIndex: 2,
                }}
              >
                <th
                  rowSpan={2}
                  style={{
                    ...tableHeaderStyle,
                    textAlign: 'left',
                    padding: `${spacing.padding.normal} ${spacing.gap.sm}`,
                    borderBottom: `1px solid ${colors.border.DEFAULT}`,
                    borderRight: `1px solid ${colors.border.DEFAULT}`,
                    position: 'sticky',
                    left: 0,
                    backgroundColor: colors.bg.surface,
                    zIndex: 3,
                  }}
                  aria-sort={
                    bySymbolSort.column === 'symbol'
                      ? bySymbolSort.direction === 'asc'
                        ? 'ascending'
                        : 'descending'
                      : 'none'
                  }
                >
                  {renderSortableHeaderLabel('Symbol', 'symbol', 'left')}
                </th>
                <th
                  colSpan={2}
                  style={{
                    ...tableHeaderStyle,
                    textAlign: 'center',
                    padding: `${spacing.padding.normal} ${spacing.gap.sm}`,
                    borderBottom: `1px solid ${colors.border.DEFAULT}`,
                    borderRight: `1px solid ${colors.border.DEFAULT}`,
                    fontSize: typography.fontSize['2xs'],
                    fontWeight: typography.fontWeight.semibold,
                    textTransform: 'uppercase',
                    color: colors.text.muted,
                    backgroundColor: colors.table.groupHeader.qty,
                  }}
                >
                  Qty
                </th>
                <th
                  colSpan={3}
                  style={{
                    ...tableHeaderStyle,
                    textAlign: 'center',
                    padding: `${spacing.padding.normal} ${spacing.gap.sm}`,
                    borderBottom: `1px solid ${colors.border.DEFAULT}`,
                    borderRight: `1px solid ${colors.border.DEFAULT}`,
                    fontSize: typography.fontSize['2xs'],
                    fontWeight: typography.fontWeight.semibold,
                    textTransform: 'uppercase',
                    color: colors.text.muted,
                    backgroundColor: colors.table.groupHeader.prices,
                  }}
                >
                  Prices
                </th>
                <th
                  colSpan={3}
                  style={{
                    ...tableHeaderStyle,
                    textAlign: 'center',
                    padding: `${spacing.padding.normal} ${spacing.gap.sm}`,
                    borderBottom: `1px solid ${colors.border.DEFAULT}`,
                    borderRight: `1px solid ${colors.border.DEFAULT}`,
                    fontSize: typography.fontSize['2xs'],
                    fontWeight: typography.fontWeight.semibold,
                    textTransform: 'uppercase',
                    color: colors.text.muted,
                    backgroundColor: colors.table.groupHeader.pnl,
                  }}
                  >
                    PnL
                  </th>
                {decisionDetailsEnabled && (
                  <th
                    colSpan={1}
                    style={{
                      ...tableHeaderStyle,
                      textAlign: 'center',
                      padding: `${spacing.padding.normal} ${spacing.gap.sm}`,
                      borderBottom: `1px solid ${colors.border.DEFAULT}`,
                      borderRight: `1px solid ${colors.border.DEFAULT}`,
                      fontSize: typography.fontSize['2xs'],
                      fontWeight: typography.fontWeight.semibold,
                      textTransform: 'uppercase',
                      color: colors.text.muted,
                      backgroundColor: colors.table.groupHeader.details,
                    }}
                  >
                    Decision
                  </th>
                )}
                <th
                  colSpan={2}
                  style={{
                    ...tableHeaderStyle,
                    textAlign: 'center',
                    padding: `${spacing.padding.normal} ${spacing.gap.sm}`,
                    borderBottom: `1px solid ${colors.border.DEFAULT}`,
                    borderRight: showBySymbolMore ? `1px solid ${colors.border.DEFAULT}` : 'none',
                    fontSize: typography.fontSize['2xs'],
                    fontWeight: typography.fontWeight.semibold,
                    textTransform: 'uppercase',
                    color: colors.text.muted,
                    backgroundColor: colors.table.groupHeader.ops,
                  }}
                >
                  Ops
                </th>
                {showBySymbolMore && (
                  <th
                    colSpan={4}
                    style={{
                      ...tableHeaderStyle,
                      textAlign: 'center',
                      padding: `${spacing.padding.normal} ${spacing.gap.sm}`,
                      borderBottom: `1px solid ${colors.border.DEFAULT}`,
                      fontSize: typography.fontSize['2xs'],
                      fontWeight: typography.fontWeight.semibold,
                      textTransform: 'uppercase',
                      color: colors.text.muted,
                      backgroundColor: colors.table.groupHeader.details,
                    }}
                  >
                    Details
                  </th>
                )}
              </tr>
              {/* Column header row */}
              <tr
                style={{
                  position: 'sticky',
                  top: 32,
                  backgroundColor: colors.bg.surface,
                  zIndex: 2,
                }}
              >
                {[
                  { key: 'buy_qty', label: 'Buy Qty', align: 'right' as const, group: 'qty', tooltip: null, sortKey: null as BySymbolSortKey | null },
                  { key: 'sell_qty', label: 'Sell Qty', align: 'right' as const, group: 'qty', tooltip: null, sortKey: null as BySymbolSortKey | null },
                  { key: 'avg_buy', label: 'Avg Buy', align: 'right' as const, group: 'prices', tooltip: 'Volume-weighted average buy price', sortKey: null as BySymbolSortKey | null },
                  { key: 'avg_sell', label: 'Avg Sell', align: 'right' as const, group: 'prices', tooltip: 'Volume-weighted average sell price', sortKey: null as BySymbolSortKey | null },
                  { key: 'fv_now', label: 'FV(now)', align: 'right' as const, group: 'prices', tooltip: 'Fair Value (current theoretical price)', sortKey: null as BySymbolSortKey | null },
                  { key: 'gross', label: 'Gross (bps/$)', align: 'right' as const, group: 'pnl', tooltip: 'Gross PnL in basis points (1 bps = 0.01%) and USD', sortKey: 'gross_bps' as BySymbolSortKey },
                  { key: 'net', label: 'Net (bps/$)', align: 'right' as const, group: 'pnl', tooltip: 'Net PnL (after fees) in basis points and USD', sortKey: 'net_bps' as BySymbolSortKey },
                  { key: 'm2m', label: 'M2M ($)', align: 'right' as const, group: 'pnl', tooltip: 'Mark-to-Market PnL (unrealized position value)', sortKey: 'm2m_usd' as BySymbolSortKey },
                  ...(decisionDetailsEnabled ? [
                    { key: 'decision', label: 'Decision vs Realized', align: 'left' as const, group: 'decision', tooltip: 'Logged decision edge vs realized PnL (bps)', sortKey: null as BySymbolSortKey | null },
                  ] : []),
                  { key: 'flow', label: 'Flow ($)', align: 'right' as const, group: 'ops', tooltip: 'Total dollar flow (gross notional traded)', sortKey: 'gross_flow' as BySymbolSortKey },
                  { key: 'coverage', label: 'Coverage', align: 'right' as const, group: 'ops', tooltip: 'Hedge coverage ratio (matched qty / max qty)', sortKey: null as BySymbolSortKey | null },
                  ...(showBySymbolMore ? [
                    { key: 'matched_notional', label: 'Matched ($)', align: 'right' as const, group: 'details', tooltip: 'Dollar value of matched trades', sortKey: 'matched_notional' as BySymbolSortKey },
                    { key: 'buy_notional', label: 'Buy Not. ($)', align: 'right' as const, group: 'details', tooltip: 'Total buy notional', sortKey: 'buy_notional' as BySymbolSortKey },
                    { key: 'sell_notional', label: 'Sell Not. ($)', align: 'right' as const, group: 'details', tooltip: 'Total sell notional', sortKey: 'sell_notional' as BySymbolSortKey },
                    { key: 'flags', label: 'Flags', align: 'center' as const, group: 'details', tooltip: null, sortKey: null as BySymbolSortKey | null },
                  ] : []),
                ].map(({ key, label, align, tooltip, sortKey }) => (
                  <th
                    key={key}
                    style={{
                      ...tableHeaderStyle,
                      textAlign: align,
                      padding: `${spacing.padding.normal} ${spacing.gap.sm}`,
                      borderBottom: `1px solid ${colors.border.DEFAULT}`,
                      fontSize: typography.fontSize['2xs'],
                      backgroundColor: sortKey && bySymbolSort.column === sortKey ? colors.bg.hover : colors.bg.surface,
                    }}
                    aria-sort={
                      sortKey
                        ? bySymbolSort.column === sortKey
                          ? bySymbolSort.direction === 'asc'
                            ? 'ascending'
                            : 'descending'
                          : 'none'
                        : undefined
                    }
                  >
                    {renderSortableHeaderLabel(
                      label,
                      sortKey,
                      align,
                      tooltip ?? undefined
                    )}
                  </th>
                ))}
              </tr>
            </thead>
            <tbody>
              { (showAllBySymbolRows ? bySymbolEntries : bySymbolEntries.slice(0, BY_SYMBOL_MAX_ROWS)).map(([symbol, data], index) => {
                if (!data || typeof data !== 'object') return null;

                const buyQty = Number(data.buy_qty) || 0;
                const sellQty = Number(data.sell_qty) || 0;
                const vwapBuy = Number(data.vwap_buy) || 0;
                const vwapSell = Number(data.vwap_sell) || 0;
                const fvNow = Number(data.fv_now) || 0;
                const fvSource = typeof data.fv_source === 'string' ? data.fv_source : null;
                const fvAgeLabel = formatFvAge(
                  typeof data.fv_age_ms === 'number' ? data.fv_age_ms : null
                );
                const m2mUsd = Number(data.m2m_usd) || 0;
                const flowUsd = Number(data.gross_flow) || 0;
                const avgDecisionEdge = Number(data.avg_decision_edge_bps_net);
                const avgRealizedBps = Number(
                  data.avg_realized_pnl_bps ?? data.net_bps ?? Number.NaN,
                );
                const decisionDelta = (
                  Number.isFinite(avgDecisionEdge) && Number.isFinite(avgRealizedBps)
                    ? avgRealizedBps - avgDecisionEdge
                    : null
                );
                const coverage = Number(data.coverage) || 0;

                const highlight = data.is_loss;
                const isEven = index % 2 === 0;
                const baseRowBg = isEven ? semanticTokens.surface : semanticTokens.surfaceAlt;
                const rowBg = highlight ? colors.semantic.warning.bg : baseRowBg;

                return (
                  <tr
                    key={symbol}
                    style={{
                      borderBottom: `1px solid ${colors.border.DEFAULT}`,
                      backgroundColor: rowBg,
                      height: '40px',
                    }}
                    className="hover:bg-bg-hover/70"
                  >
                    <td
                      style={{
                        padding: `${spacing.padding.normal} ${spacing.gap.sm}`,
                        color: colors.text.secondary,
                        position: 'sticky',
                        left: 0,
                        backgroundColor: rowBg,
                        borderRight: `1px solid ${colors.border.DEFAULT}`,
                        zIndex: 1,
                      }}
                    >
                      <div className="flex items-center" style={{ gap: spacing.gap.xs }}>
                        <span style={{ fontFamily: typography.fontFamily.mono }}>{symbol}</span>
                        {data.row_type && (
                          <span
                            style={{
                              ...tinyTagStyle(
                                data.row_type === 'hedge'
                                  ? 'success'
                                  : data.row_type === 'dex'
                                    ? 'info'
                                    : 'neutral'
                              ),
                              opacity: data.row_type === 'md' || data.row_type === 'fcsynth' ? 0.75 : 1,
                            }}
                            title={
                              data.row_type === 'hedge'
                                ? 'Hedge leg—realized PnL shown on non-hedge legs (DEX/trade)'
                                : data.row_type === 'dex'
                                  ? 'DEX leg - realized PnL shown here'
                                  : data.row_type === 'md' || data.row_type === 'fcsynth'
                                    ? 'Derived row—no executed legs; realized PnL shown only on non-hedge legs (dex/trade)'
                                    : 'Trade row - realized PnL shown here'
                            }
                          >
                            {data.row_type}
                          </span>
                        )}
                        {data.fv_source && (
                          <span
                            style={tinyTagStyle(
                              data.fv_source === 'snapshot'
                                ? 'success'
                                : data.fv_source === 'strategy'
                                  ? 'info'
                                  : 'warning'
                            )}
                          >
                            {data.fv_source}
                          </span>
                        )}
                        {data.fx_synth && (
                          <span style={tinyTagStyle('info')}>fx:synth</span>
                        )}
                        {data.fx_missing && (
                          <span style={tinyTagStyle('danger')}>fx:missing</span>
                        )}
                        {data.is_fv_stale && (
                          <SimpleTooltip
                            content={`Stale FV (${data.fv_age_ms ? `${Math.round(data.fv_age_ms / 60000)}min old` : 'old'})`}
                            side="top"
                            delay={200}
                          >
                            <span
                              style={{
                                display: 'inline-block',
                                width: '6px',
                                height: '6px',
                                borderRadius: '50%',
                                backgroundColor: severity.warning.color,
                                cursor: 'help',
                              }}
                              aria-label="Stale fair value"
                            />
                          </SimpleTooltip>
                        )}
                      </div>
                    </td>
                    <td style={{ padding: `${spacing.padding.normal} ${spacing.gap.sm}`, textAlign: 'right', fontFamily: typography.fontFamily.mono }}>
                      {formatters.fmtQty(buyQty, 2)}
                    </td>
                    <td style={{ padding: `${spacing.padding.normal} ${spacing.gap.sm}`, textAlign: 'right', fontFamily: typography.fontFamily.mono }}>
                      {formatters.fmtQty(sellQty, 2)}
                    </td>
                    <td style={{ padding: `${spacing.padding.normal} ${spacing.gap.sm}`, textAlign: 'right', fontFamily: typography.fontFamily.mono }}>
                      {formatters.fmtFixed(vwapBuy, 6)}
                    </td>
                    <td style={{ padding: `${spacing.padding.normal} ${spacing.gap.sm}`, textAlign: 'right', fontFamily: typography.fontFamily.mono }}>
                      {formatters.fmtFixed(vwapSell, 6)}
                    </td>
                    <td
                      style={{ padding: `${spacing.padding.normal} ${spacing.gap.sm}`, textAlign: 'right', fontFamily: typography.fontFamily.mono }}
                      title={[
                        fvSource ? `Source: ${fvSource}` : null,
                        fvAgeLabel ? `Age: ${fvAgeLabel}` : null,
                      ].filter(Boolean).join(' | ') || undefined}
                    >
                      <div className="flex items-center justify-end" style={{ gap: spacing.gap.xs }}>
                        <span>{formatters.fmtFixed(fvNow, 6)}</span>
                        {fvSource && (
                          <span
                            style={tinyTagStyle(
                              fvSource === 'snapshot'
                                ? 'success'
                                : fvSource === 'strategy'
                                  ? 'info'
                                  : 'neutral'
                            )}
                          >
                            {fvSource}
                          </span>
                        )}
                      </div>
                    </td>
                    <td style={{
                      padding: `${spacing.padding.normal} ${spacing.gap.sm}`,
                      textAlign: 'right',
                      fontFamily: typography.fontFamily.mono,
                      color: data.row_type === 'hedge' || data.row_type === 'md' || data.row_type === 'fcsynth' ? colors.text.muted : colors.text.primary,
                      opacity: data.row_type === 'hedge' || data.row_type === 'md' || data.row_type === 'fcsynth' ? 0.5 : 1,
                    }}>
                      {data.row_type === 'hedge' || data.row_type === 'md' || data.row_type === 'fcsynth' ? (
                        <span title={data.row_type === 'hedge' ? 'Hedge leg—realized PnL shown on non-hedge legs (DEX/trade)' : 'Derived row—no executed legs; realized PnL shown only on non-hedge legs (dex/trade)'}>
                          {formatters.fmtDualPnL(0, 0)}
                        </span>
                      ) : (
                        // Show PnL for dex/trade rows, or if row_type is undefined/null (fallback to showing values)
                        formatters.fmtDualPnL(data.gross_bps ?? 0, data.gross_usd ?? 0)
                      )}
                    </td>
                    <td style={{
                      padding: `${spacing.padding.normal} ${spacing.gap.sm}`,
                      textAlign: 'right',
                      fontFamily: typography.fontFamily.mono,
                      color: data.row_type === 'hedge' || data.row_type === 'md' || data.row_type === 'fcsynth'
                        ? colors.text.muted
                        : ((data.net_bps ?? 0) >= 0 ? severity.success.color : severity.critical.color),
                      opacity: data.row_type === 'hedge' || data.row_type === 'md' || data.row_type === 'fcsynth' ? 0.5 : 1,
                    }}>
                      {data.row_type === 'hedge' || data.row_type === 'md' || data.row_type === 'fcsynth' ? (
                        <span title={data.row_type === 'hedge' ? 'Hedge leg—realized PnL shown on non-hedge legs (DEX/trade)' : 'Derived row—no executed legs; realized PnL shown only on non-hedge legs (dex/trade)'}>
                          {formatters.fmtDualPnL(0, 0)}
                        </span>
                      ) : (
                        // Show PnL for dex/trade rows, or if row_type is undefined/null (fallback to showing values)
                        formatters.fmtDualPnL(data.net_bps ?? 0, data.net_usd ?? 0)
                      )}
                    </td>
                    <td style={{ padding: `${spacing.padding.normal} ${spacing.gap.sm}`, textAlign: 'right', fontFamily: typography.fontFamily.mono, color: m2mUsd >= 0 ? severity.success.color : severity.critical.color }}>
                      {formatters.fmtMoney(m2mUsd, 2)}
                    </td>
                    {decisionDetailsEnabled && (
                      <td
                        style={{
                          padding: `${spacing.padding.normal} ${spacing.gap.sm}`,
                          textAlign: 'left',
                          fontFamily: typography.fontFamily.mono,
                        }}
                      >
                        <div
                          style={{
                            display: 'flex',
                            flexDirection: 'column',
                            gap: spacing.gap.xs,
                            fontSize: typography.fontSize['2xs'],
                            color: colors.text.secondary,
                          }}
                        >
                          <div>
                            edge {Number.isFinite(avgDecisionEdge) ? formatters.fmtFixed(avgDecisionEdge, 2) : '—'} bps
                          </div>
                          <div>
                            realized {Number.isFinite(avgRealizedBps) ? formatters.fmtFixed(avgRealizedBps, 2) : '—'} bps
                          </div>
                          <div
                            style={{
                              color:
                                decisionDelta == null
                                  ? colors.text.muted
                                  : decisionDelta >= 0
                                    ? severity.success.color
                                    : severity.critical.color,
                            }}
                          >
                            Δ {decisionDelta == null ? '—' : formatters.fmtFixed(decisionDelta, 2)} bps
                          </div>
                          <div style={{ color: colors.text.muted }}>
                            n={data.decision_edge_sample_size ?? 0}
                          </div>
                        </div>
                      </td>
                    )}
                    <td style={{ padding: `${spacing.padding.normal} ${spacing.gap.sm}`, textAlign: 'right', fontFamily: typography.fontFamily.mono }}>
                      {formatters.fmtMoney(flowUsd, 0)}
                    </td>
                    <td
                      style={{
                        padding: `${spacing.padding.normal} ${spacing.gap.sm}`,
                        textAlign: 'right',
                        fontFamily: typography.fontFamily.mono,
                      }}
                    >
                      <span
                        style={{
                          display: 'inline-flex',
                          alignItems: 'center',
                          justifyContent: 'flex-end',
                          gap: spacing.gap.xs,
                        }}
                      >
                        {data.is_coverage_low && (
                          <span style={microBadgeStyle('warning')} title="Coverage below hedging threshold">
                            <span style={statusDot(semanticTokens.status.warning)} aria-hidden="true" />
                            <span>Low</span>
                          </span>
                        )}
                        <span>{formatters.fmtFixed(coverage * 100, 1)}%</span>
                      </span>
                    </td>
                    {showBySymbolMore && (
                      <>
                        <td style={{ padding: `${spacing.padding.normal} ${spacing.gap.sm}`, textAlign: 'right', fontFamily: typography.fontFamily.mono }}>
                          {formatters.fmtMoney(Number(data.matched_notional) || 0, 2)}
                        </td>
                        <td style={{ padding: `${spacing.padding.normal} ${spacing.gap.sm}`, textAlign: 'right', fontFamily: typography.fontFamily.mono }}>
                          {formatters.fmtMoney(Number(data.buy_notional) || 0, 2)}
                        </td>
                        <td style={{ padding: `${spacing.padding.normal} ${spacing.gap.sm}`, textAlign: 'right', fontFamily: typography.fontFamily.mono }}>
                          {formatters.fmtMoney(Number(data.sell_notional) || 0, 2)}
                        </td>
                        <td style={{ padding: `${spacing.padding.normal} ${spacing.gap.sm}`, textAlign: 'center' }}>
                          <div className="flex items-center justify-center" style={{ gap: spacing.gap.xs, flexWrap: 'wrap' }}>
                            {data.is_loss && (
                              <span style={microBadgeStyle('danger')}>{BY_SYMBOL_TAGS.loss.label}</span>
                            )}
                            {data.is_fv_stale && (
                              <span style={microBadgeStyle('warning')}>{BY_SYMBOL_TAGS.stale.label}</span>
                            )}
                            {data.is_coverage_low && (
                              <span style={microBadgeStyle('warning')}>{BY_SYMBOL_TAGS.coverage.label}</span>
                            )}
                          </div>
                        </td>
                      </>
                    )}
                  </tr>
                );
              })}
            </tbody>
          </table>
        </div>
      </div>
    );
  };

  const renderGroupsSection = () => {
    if (!report) {
      return null;
    }

    const groupsPanelStyle: CSSProperties = {
      ...panelStyle,
      display: 'flex',
      flexDirection: 'column',
      gap: spacing.gap.sm,
    };

    const tableContainerMinHeight = GROUP_ROW_HEIGHT * GROUP_MIN_VISIBLE_ROWS;
    const groupsTableShellStyle: CSSProperties = {
      border: '1px solid ' + colors.border.DEFAULT,
      borderRadius: borderRadius.lg,
      overflow: 'hidden',
      backgroundColor: semanticTokens.surface,
      marginTop: spacing.gap.sm,
    };

    const groupsTableScrollStyle: CSSProperties = {
      overflowX: 'auto',
    };

    return (
      <div style={groupsPanelStyle}>
        <div
          className="flex items-center justify-between"
          style={{
            gap: spacing.gap.sm,
            paddingBottom: spacing.gap.sm,
            borderBottom: `1px solid ${colors.border.DEFAULT}`,
          }}
        >
          <div className="flex items-center" style={{ gap: spacing.gap.sm }}>
            <h2
              style={{
                fontSize: typography.fontSize.md,
                fontWeight: typography.fontWeight.semibold,
                color: colors.semantic.success.light,
                margin: 0,
              }}
            >
              PnL Groups
            </h2>
            <span style={{ color: colors.text.muted, fontSize: typography.fontSize.xs }}>
              ({filteredAndSortedGroups.length} groups)
            </span>
          </div>
          <div className="flex items-center" style={{ gap: spacing.gap.xs }}>
            {activeSymbolFilter && (
              <Button
                size="xs"
                variant="ghost"
                onClick={() => setGroupFilters((prev) => ({ ...prev, symbol: '' }))}
              >
                Clear Symbol Filter
              </Button>
            )}
            <Button
              size="xs"
              variant="ghost"
              onClick={() => setGroupsExpanded((prev) => !prev)}
            >
              {groupsExpanded ? 'Collapse' : 'Expand'}
            </Button>
          </div>
        </div>

        {groupsExpanded ? (
          <>
            <TableFilter
              columns={GROUP_TABLE_FILTERS}
              onFilterChange={setGroupFilters}
              value={groupFilters}
              customControls={groupFilterControls}
            />
            <div style={groupsTableShellStyle}>
              <div ref={groupsTableContainerRef} style={groupsTableScrollStyle}>
                <DataTable
                  className="h-full"
                  data={paginatedGroups}
                  columns={groupColumns}
                  getRowId={getRowId}
                  sortable
                  loading={loading}
                  sortingState={tableSortingState}
                  onSortingStateChange={handleTableSortingChange}
                  dense={false}
                  emptyMessage={
                    groupFilters.symbol || groupFilters.signal || pnlFilter !== 'all'
                      ? 'No groups match current filters. Try clearing filters.'
                      : 'No PnL groups found. Try adjusting the time window or run a new report.'
                  }
                  primaryColumns={isMobile ? GROUP_TABLE_PRIMARY_COLUMNS : undefined}
                />
              </div>
              <Pager
                page={groupsPage}
                pageSize={groupsPageSize}
                total={filteredAndSortedGroups.length}
                onPageChange={setGroupsPage}
                onPageSizeChange={(size) => {
                  setGroupsPageSize(size);
                  setGroupsPage(1);
                }}
                borderPosition="top"
                layout="split"
                itemLabel="groups"
                rangeFormat="of"
              />
            </div>
          </>
        ) : (
          <div
            style={{
              paddingTop: spacing.gap.sm,
              color: colors.text.muted,
              fontSize: typography.fontSize.sm,
            }}
          >
            Collapsed — expand to view group filters and table.
          </div>
        )}
      </div>
    );
  };

  const renderUnhedgedSection = () => {
    const hasUnhedged = Boolean(report?.unhedged && Object.keys(report.unhedged).length > 0);
    if (!hasUnhedged) {
      return null;
    }

    return (
      <div
        style={{
          ...panelStyle,
          borderColor: severity.warning.border,
        }}
      >
        <div className="flex flex-wrap items-center justify-between" style={{ gap: spacing.gap.sm, paddingBottom: spacing.gap.sm, borderBottom: `1px solid ${colors.border.DEFAULT}` }}>
          <h2
            style={{
              fontSize: typography.fontSize.md,
              fontWeight: typography.fontWeight.semibold,
              color: severity.warning.text,
              margin: 0,
            }}
          >
            ⚠️ Unhedged Positions
          </h2>
          <div className="flex flex-wrap items-center justify-end" style={{ gap: spacing.gap.sm }}>
            <Switch
              size="sm"
              checked={netUnhedgedAcrossVenues}
              onCheckedChange={(checked) => setNetUnhedgedAcrossVenues(Boolean(checked))}
              label="Net across venues"
            />
            <Switch
              size="sm"
              checked={showSmallUnhedged}
              onCheckedChange={(checked) => setShowSmallUnhedged(Boolean(checked))}
              label={`Show small (<$${USD_MIN})`}
            />
          </div>
        </div>

        {displayedUnhedged.length === 0 ? (
          <div
            style={{
              padding: spacing.gap.md,
              color: colors.text.muted,
              fontSize: typography.fontSize.sm,
            }}
          >
            No unhedged positions with current filters.
          </div>
        ) : (
        <div style={{ marginTop: spacing.gap.md, overflowX: 'auto' }}>
          <table
            style={{
              width: '100%',
              borderCollapse: 'collapse',
              fontSize: typography.fontSize.sm,
              fontVariantNumeric: 'tabular-nums',
            }}
          >
            <thead>
              <tr>
                {['Position', 'Qty', 'FV', 'USD Value', 'VaR (3%)'].map((label, idx) => (
                  <th
                    key={label}
                    style={{
                      ...tableHeaderStyle,
                      textAlign: idx === 0 ? 'left' : 'right',
                      padding: `${spacing.padding.normal} ${spacing.gap.sm}`,
                      borderBottom: '1px solid ' + colors.border.DEFAULT,
                    }}
                  >
                    {label}
                  </th>
                ))}
              </tr>
            </thead>
            <tbody>
              {displayedUnhedged.map((u) => (
                <tr key={u.key} style={{ borderBottom: '1px solid ' + colors.border.DEFAULT }}>
                  <td
                    style={{
                      padding: `${spacing.padding.normal} ${spacing.gap.sm}`,
                      fontFamily: typography.fontFamily.mono,
                      color: severity.warning.text,
                    }}
                  >
                    {u.key.replace(/_net$/, '')}
                    {netUnhedgedAcrossVenues && ' (net)'}
                  </td>
                  <td
                    style={{
                      padding: `${spacing.padding.normal} ${spacing.gap.sm}`,
                      textAlign: 'right',
                      fontFamily: typography.fontFamily.mono,
                      color: u.side === 'long' ? severity.success.color : severity.critical.color,
                    }}
                  >
                    {formatters.fmtQty(u.qty, 4)}
                  </td>
                  <td
                    style={{
                      padding: `${spacing.padding.normal} ${spacing.gap.sm}`,
                      textAlign: 'right',
                      fontFamily: typography.fontFamily.mono,
                      color: colors.text.muted,
                    }}
                  >
                    {formatters.fmtMoney(u.fv, 4)}
                  </td>
                  <td
                    style={{
                      padding: `${spacing.padding.normal} ${spacing.gap.sm}`,
                      textAlign: 'right',
                      fontFamily: typography.fontFamily.mono,
                      color: colors.text.secondary,
                    }}
                  >
                    {formatters.fmtMoney(u.usdValue, 2)}
                  </td>
                  <td
                    style={{
                      padding: `${spacing.padding.normal} ${spacing.gap.sm}`,
                      textAlign: 'right',
                      fontFamily: typography.fontFamily.mono,
                      color: severity.warning.color,
                    }}
                  >
                    {formatters.fmtMoney(u.varLite, 2)}
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
        )}
      </div>
    );
  };

  const enrichedUnhedged = useMemo(() => {
    if (!report?.unhedged) return [];

    // Helper: find an FV for a base symbol across any quote in fv_map
    const findFvForBase = (base: string): number => {
      if (!report?.fv_map) return 0;
      // Prefer exact base key if present (rare but supported)
      const direct = (report.fv_map as any)[base];
      if (direct && typeof direct.mid === 'number') return direct.mid;
      // Otherwise, look for any entry whose symbol starts with `${base}/`
      for (const [sym, fvRec] of Object.entries(report.fv_map)) {
        if (typeof sym === 'string' && sym.split('/')[0] === base) {
          const m = (fvRec as any)?.mid;
          if (typeof m === 'number' && Number.isFinite(m)) return m;
        }
      }
      return 0;
    };

    return Object.entries(report.unhedged)
      .map(([key, value]) => {
        const qty = typeof value === 'number' ? value : (value as any).qty;
        const base = key.split('_')[0];
        const fv = findFvForBase(base);
        const usdValue = Math.abs((Number(qty) || 0) * fv);
        const varLite = usdValue * VAR_MULT;

        return {
          key,
          qty: Number(qty) || 0,
          fv,
          usdValue,
          varLite,
          side: (Number(qty) || 0) > 0 ? 'long' : 'short',
        };
      })
      .sort((a, b) => b.usdValue - a.usdValue);
  }, [report?.unhedged, report?.fv_map]);

  // Net unhedged across venues for the same base token to reduce noise
  const nettedUnhedged = useMemo(() => {
    if (!report?.unhedged) return [] as Array<{ key: string; qty: number; fv: number; usdValue: number; varLite: number; side: 'long' | 'short' }>;

    const sums = new Map<string, number>();
    for (const [key, value] of Object.entries(report.unhedged)) {
      const qty = typeof value === 'number' ? Number(value) : Number((value as any).qty);
      const base = key.split('_')[0];
      sums.set(base, (sums.get(base) || 0) + (Number.isFinite(qty) ? qty : 0));
    }

    const rows: Array<{ key: string; qty: number; fv: number; usdValue: number; varLite: number; side: 'long' | 'short' }> = [];
    for (const [base, qty] of sums.entries()) {
      const fv = (() => {
        // reuse the helper from enrichedUnhedged scope
        if (!report?.fv_map) return 0;
        const direct = (report.fv_map as any)[base];
        if (direct && typeof direct.mid === 'number') return direct.mid;
        for (const [sym, fvRec] of Object.entries(report.fv_map)) {
          if (typeof sym === 'string' && sym.split('/')[0] === base) {
            const m = (fvRec as any)?.mid;
            if (typeof m === 'number' && Number.isFinite(m)) return m;
          }
        }
        return 0;
      })();

      const usdValue = Math.abs(qty * fv);
      const varLite = usdValue * VAR_MULT;
      rows.push({ key: `${base}_net`, qty, fv, usdValue, varLite, side: qty > 0 ? 'long' : 'short' });
    }
    return rows.sort((a, b) => b.usdValue - a.usdValue);
  }, [report?.unhedged, report?.fv_map]);

  const displayedUnhedged = useMemo(() => {
    const src = netUnhedgedAcrossVenues ? nettedUnhedged : enrichedUnhedged;
    if (showSmallUnhedged) return src;
    return src.filter(u => u.usdValue >= USD_MIN);
  }, [enrichedUnhedged, nettedUnhedged, netUnhedgedAcrossVenues, showSmallUnhedged]);

  // Smart auto-expand: expand if <= AUTO_EXPAND_THRESHOLD groups (1 page worth)
  useEffect(() => {
    if (report && report.groups.length > 0 && report.groups.length <= AUTO_EXPAND_THRESHOLD) {
      setGroupsExpanded(true);
    }
  }, [report]);

  // Format time ago
  const timeAgo = (date: Date | null): string => {
    if (!date) return '';
    const seconds = Math.floor((Date.now() - date.getTime()) / 1000);
    if (seconds < 60) return `${seconds}s ago`;
    const minutes = Math.floor(seconds / 60);
    if (minutes < 60) return `${minutes}m ago`;
    const hours = Math.floor(minutes / 60);
    return `${hours}h ago`;
  };

  const headerActions = (
    <div className="flex items-center" style={{ gap: spacing.gap.sm }}>
      {autoRefresh && (
        <span style={{ fontSize: typography.fontSize.xs, color: colors.text.muted }}>
          Next refresh in {refreshCountdown}s
        </span>
      )}
      {lastUpdate && (
        <span style={{ fontSize: typography.fontSize.xs, color: colors.text.muted }}>
          {timeAgo(lastUpdate)}
        </span>
      )}
      <Switch
        size="sm"
        checked={autoRefresh}
        onCheckedChange={(checked) => setAutoRefresh(checked)}
        label="Auto-refresh"
        aria-label="Auto-refresh"
        role="switch"
      />
      {pnlMode === 'spread' && (
        <Button size="xs" variant="ghost" onClick={downloadCSV} loading={csvLoading}>
          CSV
        </Button>
      )}
    </div>
  );

  const content = (
    <div className="flex flex-col h-full overflow-hidden" style={{ color: colors.text.secondary }}>
      <PanelHeader
        title={pnlMode === 'spread' ? 'PnL (Spread)' : 'PnL (Inventory)'}
        onRefresh={() => runReport({ forceFull: true })}
        refreshing={loading}
        lastUpdate={lastUpdate ? lastUpdate.getTime() : undefined}
        staleThresholdMs={PNL_STALE_THRESHOLD_MS}
        actions={headerActions}
      />

      <PanelBody>
        <div className="flex flex-col h-full min-h-0" style={{ padding: spacing.gap.lg, gap: spacing.gap.lg }}>
          {renderControlPanel()}

          {loading ? (
            <LoadingState message="Running PnL report..." />
          ) : pnlMode === 'spread' ? (
            !report ? (
              <EmptyState message="Run a report to see PnL analysis" />
            ) : (
              <div className="flex flex-col h-full min-h-0" style={{ gap: spacing.gap.lg }}>
                {error && (
                  <div
                    style={{
                      backgroundColor: severity.critical.bg,
                      border: '1px solid ' + severity.critical.border,
                      color: severity.critical.text,
                      borderRadius: borderRadius.md,
                      padding: spacing.gap.sm,
                      fontSize: typography.fontSize.xs,
                    }}
                  >
                    {error}
                  </div>
                )}

                <div className="flex flex-col" style={{ gap: spacing.gap.lg }}>
                  {renderSummarySection()}
                  {renderSymbolSummarySection()}
                </div>

                <div className="flex flex-col flex-1 min-h-0" style={{ gap: spacing.gap.lg }}>
                  {renderGroupsSection()}
                  {renderUnhedgedSection()}
                </div>

                {report.groups.length === 0 && (
                  <EmptyState message="No PnL groups found. Try adjusting the time window or symbol filter." />
                )}
              </div>
            )
          ) : (
            !inventoryReport ? (
              <EmptyState message="Run a report to see inventory PnL" />
            ) : (
              <div className="flex flex-col h-full min-h-0" style={{ gap: spacing.gap.lg }}>
                {error && (
                  <div
                    style={{
                      backgroundColor: severity.critical.bg,
                      border: '1px solid ' + severity.critical.border,
                      color: severity.critical.text,
                      borderRadius: borderRadius.md,
                      padding: spacing.gap.sm,
                      fontSize: typography.fontSize.xs,
                    }}
                  >
                    {error}
                  </div>
                )}

                <div style={{ ...panelStyle, padding: spacing.gap.md }}>
                  <h2
                    style={{
                      fontSize: typography.fontSize.lg,
                      fontWeight: typography.fontWeight.semibold,
                      color: semanticTokens.textPrimary,
                      margin: 0,
                      marginBottom: spacing.gap.sm,
                    }}
                  >
                    Inventory Summary
                  </h2>
                  <div className="flex flex-wrap" style={{ gap: spacing.gap.sm }}>
                    {([
                      ['Realized', inventoryReport.summary.realized_pnl_usd],
                      ['Unrealized', inventoryReport.summary.unrealized_pnl_usd],
                      ['Fees (Δ)', inventoryReport.summary.fees_delta_usd],
                      ['Carry (Δ)', inventoryReport.summary.carry_delta_usd],
                      ['Net', inventoryReport.summary.net_pnl_usd],
                    ] as const).map(([label, value]) => (
                      <div
                        key={label}
                        style={{
                          backgroundColor: semanticTokens.surfaceAlt,
                          borderRadius: borderRadius.md,
                          padding: spacing.gap.sm,
                          border: `1px solid ${colors.border.DEFAULT}`,
                          minWidth: 160,
                        }}
                      >
                        <div style={{ ...labelStyle, fontSize: typography.fontSize['2xs'] }}>{label}</div>
                        <div style={{ fontSize: typography.fontSize.base, fontWeight: typography.fontWeight.semibold }}>
                          {formatters.fmtUsd(value)}
                        </div>
                      </div>
                    ))}
                  </div>
                </div>

                <div style={{ ...panelStyle, padding: spacing.gap.md, overflow: 'hidden' }}>
                  <h2
                    style={{
                      fontSize: typography.fontSize.lg,
                      fontWeight: typography.fontWeight.semibold,
                      color: semanticTokens.textPrimary,
                      margin: 0,
                      marginBottom: spacing.gap.sm,
                    }}
                  >
                    By Risk Bucket
                  </h2>
                  <div style={{ overflow: 'auto', maxHeight: 420 }}>
                    <table style={{ width: '100%', borderCollapse: 'collapse' }}>
                      <thead>
                        <tr>
                          {['Bucket', 'Net', 'Realized', 'Unrealized', 'Fees Δ', 'Carry Δ', 'Legs'].map((h) => (
                            <th
                              key={h}
                              style={{
                                ...tableHeaderStyle,
                                textAlign: 'left',
                                padding: `${spacing.gap.xs} ${spacing.gap.sm}`,
                                borderBottom: `1px solid ${colors.border.DEFAULT}`,
                                position: 'sticky',
                                top: 0,
                                background: semanticTokens.surface,
                              }}
                            >
                              {h}
                            </th>
                          ))}
                        </tr>
                      </thead>
                      <tbody>
                        {inventoryReport.by_risk_bucket.map((b) => (
                          <tr key={b.bucket}>
                            <td style={{ padding: `${spacing.gap.xs} ${spacing.gap.sm}`, borderBottom: `1px solid ${colors.border.DEFAULT}` }}>
                              <div style={{ fontWeight: typography.fontWeight.medium }}>{b.bucket}</div>
                            </td>
                            <td style={{ padding: `${spacing.gap.xs} ${spacing.gap.sm}`, borderBottom: `1px solid ${colors.border.DEFAULT}` }}>{formatters.fmtUsd(b.net_pnl_usd)}</td>
                            <td style={{ padding: `${spacing.gap.xs} ${spacing.gap.sm}`, borderBottom: `1px solid ${colors.border.DEFAULT}` }}>{formatters.fmtUsd(b.realized_pnl_usd)}</td>
                            <td style={{ padding: `${spacing.gap.xs} ${spacing.gap.sm}`, borderBottom: `1px solid ${colors.border.DEFAULT}` }}>{formatters.fmtUsd(b.unrealized_pnl_usd)}</td>
                            <td style={{ padding: `${spacing.gap.xs} ${spacing.gap.sm}`, borderBottom: `1px solid ${colors.border.DEFAULT}` }}>{formatters.fmtUsd(b.fees_delta_usd)}</td>
                            <td style={{ padding: `${spacing.gap.xs} ${spacing.gap.sm}`, borderBottom: `1px solid ${colors.border.DEFAULT}` }}>{formatters.fmtUsd(b.carry_delta_usd)}</td>
                            <td style={{ padding: `${spacing.gap.xs} ${spacing.gap.sm}`, borderBottom: `1px solid ${colors.border.DEFAULT}` }}>
                              <details>
                                <summary style={{ cursor: 'pointer' }}>{b.legs?.length ?? 0}</summary>
                                <div style={{ marginTop: spacing.gap.xs, fontSize: typography.fontSize.xs, color: semanticTokens.textMuted }}>
                                  {(b.legs || []).map((leg) => (
                                    <div key={leg.position_key}>
                                      {leg.position_key}: qty {leg.qty}, m2m {formatters.fmtUsd(leg.m2m_usd)}
                                    </div>
                                  ))}
                                </div>
                              </details>
                            </td>
                          </tr>
                        ))}
                      </tbody>
                    </table>
                  </div>
                </div>

                <div style={{ ...panelStyle, padding: spacing.gap.md, overflow: 'hidden' }}>
                  <h2
                    style={{
                      fontSize: typography.fontSize.lg,
                      fontWeight: typography.fontWeight.semibold,
                      color: semanticTokens.textPrimary,
                      margin: 0,
                      marginBottom: spacing.gap.sm,
                    }}
                  >
                    Positions
                  </h2>
                  <div style={{ overflow: 'auto', maxHeight: 420 }}>
                    <table style={{ width: '100%', borderCollapse: 'collapse' }}>
                      <thead>
                        <tr>
                          {['Position', 'Qty', 'Net', 'Realized', 'Unrealized', 'FV'].map((h) => (
                            <th
                              key={h}
                              style={{
                                ...tableHeaderStyle,
                                textAlign: 'left',
                                padding: `${spacing.gap.xs} ${spacing.gap.sm}`,
                                borderBottom: `1px solid ${colors.border.DEFAULT}`,
                                position: 'sticky',
                                top: 0,
                                background: semanticTokens.surface,
                              }}
                            >
                              {h}
                            </th>
                          ))}
                        </tr>
                      </thead>
                      <tbody>
                        {inventoryReport.positions.map((p) => (
                          <tr key={p.position_key}>
                            <td style={{ padding: `${spacing.gap.xs} ${spacing.gap.sm}`, borderBottom: `1px solid ${colors.border.DEFAULT}` }}>
                              <div style={{ fontWeight: typography.fontWeight.medium }}>{p.position_key}</div>
                              <div style={{ fontSize: typography.fontSize.xs, color: semanticTokens.textMuted }}>{p.symbol}</div>
                            </td>
                            <td style={{ padding: `${spacing.gap.xs} ${spacing.gap.sm}`, borderBottom: `1px solid ${colors.border.DEFAULT}` }}>{formatters.fmtNumber(p.qty, 4)}</td>
                            <td style={{ padding: `${spacing.gap.xs} ${spacing.gap.sm}`, borderBottom: `1px solid ${colors.border.DEFAULT}` }}>{formatters.fmtUsd(p.net_pnl_usd)}</td>
                            <td style={{ padding: `${spacing.gap.xs} ${spacing.gap.sm}`, borderBottom: `1px solid ${colors.border.DEFAULT}` }}>{formatters.fmtUsd(p.realized_pnl_usd)}</td>
                            <td style={{ padding: `${spacing.gap.xs} ${spacing.gap.sm}`, borderBottom: `1px solid ${colors.border.DEFAULT}` }}>{formatters.fmtUsd(p.unrealized_pnl_usd)}</td>
                            <td style={{ padding: `${spacing.gap.xs} ${spacing.gap.sm}`, borderBottom: `1px solid ${colors.border.DEFAULT}` }}>
                              {typeof p.fv === 'number' ? formatters.fmtNumber(p.fv, 6) : 'n/a'}
                            </td>
                          </tr>
                        ))}
                      </tbody>
                    </table>
                  </div>
                </div>

                {inventoryReport.invalid_rows && inventoryReport.invalid_rows.length > 0 && (
                  <div style={{ ...panelStyle, padding: spacing.gap.md }}>
                    <h2
                      style={{
                        fontSize: typography.fontSize.lg,
                        fontWeight: typography.fontWeight.semibold,
                        color: semanticTokens.textPrimary,
                        margin: 0,
                        marginBottom: spacing.gap.sm,
                      }}
                    >
                      Invalid Rows ({inventoryReport.invalid_rows.length})
                    </h2>
                    <div style={{ fontSize: typography.fontSize.xs, color: semanticTokens.textMuted, marginBottom: spacing.gap.sm }}>
                      Reasons: {Object.entries(inventoryReport.meta.invalid_reason_counts ?? {}).map(([k, v]) => `${k}=${v}`).join(', ') || 'n/a'}
                    </div>
                    <div style={{ maxHeight: 220, overflow: 'auto', fontSize: typography.fontSize.xs }}>
                      {inventoryReport.invalid_rows.slice(0, 50).map((r, idx) => (
                        <div key={`${r.reason}-${idx}`} style={{ padding: `${spacing.gap.xs} 0`, borderBottom: `1px solid ${colors.border.DEFAULT}` }}>
                          <span style={{ fontWeight: typography.fontWeight.medium }}>{r.reason}</span>{' '}
                          <span style={{ color: semanticTokens.textMuted }}>{r.row_id || ''}</span>
                        </div>
                      ))}
                    </div>
                  </div>
                )}
              </div>
            )
          )}
        </div>
      </PanelBody>
    </div>
  );

  return (
    <PageShell>
      {content}
    </PageShell>
  );
}

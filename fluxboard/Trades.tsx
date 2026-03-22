// Trades blotter with server-side pagination, filtering, and live updates

import { useEffect, useState, useCallback, useRef, useMemo } from 'react';
import { api, deriveCanonicalNaming } from './api';
import { socket } from './sockets';
import {
  useTradesStore,
  selectTradesRows,
  selectTradesLastSeq,
  useResyncStore,
  selectResyncId,
  markGlobalResyncApplied,
  shallow,
} from './stores';
import {
  createRealtimeSurfaceController,
  useRealtimeSurfaceController,
  type RealtimeRowDelta,
} from './hooks/useRealtimeSurfaceController';
import { useResyncStatus } from './hooks/useResyncStatus';
import { TableFilter, type FilterValues, type ColumnFilter } from './components/shared/TableFilter';
import { PanelHeader } from './components/shared/PanelHeader';
import type { TradeRow, TradeEvent } from './types';
import { playTradeClick } from './utils/sound';
import { getSoundMuted, setSoundMuted } from './utils/storage';
import { TradesTable, type TradesTableScrollState } from './components/trades/TradesTable';
import { TradesPerfHarness } from './components/trades/PerfHarness';
import { SOUND } from './constants';
import { Button } from './components/ui/button/Button';
import { colors, spacing, typography, STALE_THRESHOLDS, borderRadius } from './lib/tokens';
import { usePanelHeaderSlots } from './components/layout/PanelWrapper';
import { exportCSV, generateTimestampFilename } from './utils/export';
import { isTradesDecisionDetailsEnabled } from './config/featureFlags';
import { computeTradesRollups } from './components/trades/rollups';
import { RealtimeSurfaceState, type RealtimeSnapshotRevision } from './lib/realtime/types';

const PERF_RENDER_ENABLED = typeof import.meta !== 'undefined'
  && Boolean(import.meta.env?.DEV)
  && typeof performance !== 'undefined';

const DEV_TRADES_PERF_HARNESS = typeof import.meta !== 'undefined'
  && Boolean(import.meta.env?.DEV)
  && Boolean(import.meta.env?.VITE_TRADES_PERF);

const TRADE_FILTERS: ColumnFilter[] = [
  { key: 'coin', label: 'Coin', type: 'text', placeholder: 'BTC, ETH...' },
  { key: 'market_type', label: 'Market', type: 'select', options: ['spot', 'perp'] },
  { key: 'exchange', label: 'Exchange', type: 'text', placeholder: 'bybit, rooster...' },
  { key: 'side', label: 'Side', type: 'select', options: ['buy', 'sell'] },
  { key: 'signal_id', label: 'Signal', type: 'text', placeholder: 'Strategy ID...' },
];

const FILTER_STORAGE_KEY = 'trades_filters';
const PAGE_SIZE_STORAGE_KEY = 'trades_page_size';

const DEBOUNCE_MS = 300;
const POLL_BASE_MS = 1000; // Base when WS connected
const POLL_BASE_MS_DISCONNECTED = 500; // Faster reconciliation when WS disconnected
const POLL_MAX_MS = 3000; // Cap backoff at 3s to reduce UI staleness
// Reduce default delta payload to improve initial load times over WAN.
// The UI requests more when scrolling back via cursor.
const DELTA_LIMIT = 500;
const MAX_EMPTY_POLLS = 3; // Log warning if this many consecutive polls return 0 trades
const RECONNECT_CATCHUP_MIN_MS = 3000;
const TRADE_HEALTH_STALE_MS = STALE_THRESHOLDS.REALTIME * 2;

const coerceFiniteNumber = (value: unknown): number | undefined => {
  if (typeof value === 'number' && Number.isFinite(value)) {
    return value;
  }
  if (typeof value === 'string') {
    const trimmed = value.trim();
    if (trimmed.length === 0) {
      return undefined;
    }
    const parsed = Number(trimmed);
    return Number.isFinite(parsed) ? parsed : undefined;
  }
  return undefined;
};

const normalizeTradeSide = (value: unknown): string => {
  const text = String(value ?? '').trim().toLowerCase();
  if (text === '1' || text === 'buy' || text === 'bid') return 'buy';
  if (text === '2' || text === 'sell' || text === 'ask') return 'sell';
  return text;
};

const coerceTradeTsMs = (value: unknown): number | undefined => {
  const ts = coerceFiniteNumber(value);
  if (ts === undefined || ts <= 0) return undefined;
  if (ts < 1e12) return Math.trunc(ts * 1000);
  if (ts >= 1e18) return Math.trunc(ts / 1e6);
  if (ts >= 1e15) return Math.trunc(ts / 1e3);
  return Math.trunc(ts);
};

type TradeReplayCursor = {
  tsMs: number;
  rowId: string;
  version: number;
};

type TradeStreamCursor = {
  contractVersion?: number;
  streamId?: string;
  snapshotRevision?: RealtimeSnapshotRevision;
  lastSeq: number;
};

const getTradeRowSortKey = (row: TradeRow): number => {
  if (typeof row.ts === 'number' && Number.isFinite(row.ts)) {
    return row.ts;
  }
  if (typeof row.seq === 'number' && Number.isFinite(row.seq)) {
    return row.seq;
  }
  return 0;
};

const createTradeRowComparator = (
  direction: 'ts_desc' | 'ts_asc',
) => (left: TradeRow, right: TradeRow): number => {
  const leftKey = getTradeRowSortKey(left);
  const rightKey = getTradeRowSortKey(right);
  if (leftKey !== rightKey) {
    return direction === 'ts_desc' ? rightKey - leftKey : leftKey - rightKey;
  }
  if (left.seq !== right.seq) {
    return direction === 'ts_desc' ? right.seq - left.seq : left.seq - right.seq;
  }
  return direction === 'ts_desc'
    ? left.row_id.localeCompare(right.row_id)
    : right.row_id.localeCompare(left.row_id);
};

const extractTradeTimestampMs = (value: any): number | undefined => {
  if (!value || typeof value !== 'object') {
    return undefined;
  }
  return (
    coerceTradeTsMs(value.ts_ms) ??
    coerceTradeTsMs(value.ts) ??
    coerceTradeTsMs(value.timestamp) ??
    coerceTradeTsMs(value.server_ts_ms) ??
    (typeof value.time === 'string' && value.time ? coerceTradeTsMs(Date.parse(value.time)) : undefined)
  );
};

const extractTradeReplayCursor = (value: any): TradeReplayCursor | null => {
  if (!value || typeof value !== 'object') {
    return null;
  }
  const tsMs = extractTradeTimestampMs(value);
  const rowId = String(value.row_id ?? value.trade_id ?? value.entry_id ?? '').trim();
  const versionValue = Number(value.version);
  const version = Number.isFinite(versionValue) && versionValue > 0 ? Math.trunc(versionValue) : 1;
  if (tsMs === undefined || !rowId) {
    return null;
  }
  return { tsMs, rowId, version };
};

const compareTradeReplayCursor = (
  left: TradeReplayCursor,
  right: TradeReplayCursor,
): number => {
  if (left.tsMs !== right.tsMs) {
    return left.tsMs - right.tsMs;
  }
  const rowIdOrder = left.rowId.localeCompare(right.rowId);
  if (rowIdOrder !== 0) {
    return rowIdOrder;
  }
  return left.version - right.version;
};

const getLatestTradeReplayCursor = (
  rows: Array<any> | undefined | null,
): TradeReplayCursor | null => {
  if (!rows?.length) {
    return null;
  }
  let latest: TradeReplayCursor | null = null;
  for (const row of rows) {
    const candidate = extractTradeReplayCursor(row);
    if (!candidate) {
      continue;
    }
    if (!latest || compareTradeReplayCursor(candidate, latest) > 0) {
      latest = candidate;
    }
  }
  return latest;
};

const filterTradeRowsAfterReplayCursor = (
  rows: Array<any> | undefined | null,
  cursor: TradeReplayCursor | null,
): Array<any> => {
  if (!rows?.length || !cursor) {
    return rows ? [...rows] : [];
  }
  return rows.filter((row) => {
    const candidate = extractTradeReplayCursor(row);
    if (!candidate) {
      return true;
    }
    return compareTradeReplayCursor(candidate, cursor) > 0;
  });
};

const normalizeTradeEventLike = (candidate: any): any => {
  if (!candidate || typeof candidate !== 'object') return candidate;
  const row = candidate as Record<string, unknown>;

  const instrumentId = String(row.instrument_id ?? '').trim();
  const symbol = String(row.symbol ?? instrumentId.split('.')[0] ?? '').trim();
  const exchangeText = String(row.exchange ?? row.venue ?? '').trim();
  const coinText = String(row.coin ?? row.asset ?? row.base_currency ?? '').trim();
  const naming = deriveCanonicalNaming(row, {
    exchange: exchangeText,
    symbol,
    asset: coinText,
    isPosition: false,
  });

  Object.assign(row, naming);

  if (!coinText) {
    const derivedCoin = String(naming.inventory_asset ?? naming.base_asset ?? '').trim().toUpperCase();
    if (derivedCoin) row.coin = derivedCoin;
  }
  if (exchangeText) {
    row.exchange = exchangeText.toLowerCase();
  } else {
    const derivedExchange = String(naming.venue ?? naming.venue_root ?? '').trim().toLowerCase();
    if (derivedExchange) row.exchange = derivedExchange;
  }

  row.side = normalizeTradeSide(row.side);

  const orderIdText = String(row.order_id ?? '').trim();
  if (!orderIdText) {
    const fallback = String(row.client_order_id ?? '').trim();
    if (fallback) row.order_id = fallback;
  }

  const signalIdText = String(row.signal_id ?? '').trim();
  if (!signalIdText) {
    const fallback = String(row.strategy_id ?? '').trim();
    if (fallback) row.signal_id = fallback;
  }

  const tsMs =
    coerceTradeTsMs(row.ts_ms) ??
    coerceTradeTsMs(row.ts_event) ??
    coerceTradeTsMs(row.ts) ??
    coerceTradeTsMs(row.timestamp);
  if ((row.ts_ms == null || row.ts_ms === 0) && tsMs !== undefined) {
    row.ts_ms = tsMs;
  }

  const timeText = String(row.time ?? '').trim();
  if (!timeText && tsMs !== undefined) {
    row.time = new Date(tsMs).toISOString();
  }

  if (row.mv == null && row.notional == null) {
    const price = coerceFiniteNumber(row.price);
    const qty = coerceFiniteNumber(row.qty);
    if (price !== undefined && qty !== undefined) {
      row.mv = price * qty;
    }
  }

  return row;
};

type TradeTimestampParts = {
  seq?: number;
  tsMs?: number;
  ts?: number;
  hasReliableTimestamp: boolean;
};

const getTimestampParts = (payload: any): TradeTimestampParts => {
  const seq = coerceFiniteNumber(payload?.seq);
  const tsMs = coerceFiniteNumber(payload?.ts_ms);
  const ts = coerceFiniteNumber(payload?.ts);
  const hasReliableTimestamp = seq !== undefined || tsMs !== undefined || ts !== undefined;
  return { seq, tsMs, ts, hasReliableTimestamp };
};

export const hasReliableTradeTimestamp = (payload: any): boolean =>
  getTimestampParts(payload).hasReliableTimestamp;

const toOptionalText = (value: unknown): string | undefined => {
  const text = String(value ?? '').trim();
  return text || undefined;
};

const toTradeRow = (event: TradeEvent | null | undefined): TradeRow | null => {
  if (!event || typeof event !== 'object' || event.op === 'delete' || !event.row_id) {
    return null;
  }

  const seq = coerceFiniteNumber(event.seq);
  if (seq === undefined) {
    return null;
  }

  const versionValue = coerceFiniteNumber(event.version);
  const version = versionValue !== undefined && versionValue > 0 ? Math.trunc(versionValue) : 1;
  const tsMs = extractTradeTimestampMs(event);
  const ts =
    coerceFiniteNumber(event.ts) ??
    tsMs ??
    seq;
  const timeText = String(event.time ?? '').trim();
  const time = timeText || (tsMs !== undefined ? new Date(tsMs).toISOString() : '');
  const price = coerceFiniteNumber(event.price);
  const qty = coerceFiniteNumber(event.qty);
  const derivedMv = price !== undefined && qty !== undefined ? price * qty : undefined;
  const rawMv = coerceFiniteNumber((event as any).mv ?? (event as any).notional);
  const mv =
    (rawMv === undefined || rawMv === 0) && derivedMv !== undefined && derivedMv !== 0
      ? derivedMv
      : rawMv;

  return {
    time,
    coin: String(event.coin ?? ''),
    exchange: String(event.exchange ?? event.venue ?? '').trim().toLowerCase(),
    venue: toOptionalText((event as any).venue),
    symbol: toOptionalText((event as any).symbol),
    instrument_uid: toOptionalText((event as any).instrument_uid),
    instrument_id: toOptionalText((event as any).instrument_id),
    venue_root: toOptionalText((event as any).venue_root),
    product_type: toOptionalText((event as any).product_type),
    market_type: toOptionalText((event as any).market_type),
    contract_type: toOptionalText((event as any).contract_type),
    raw_symbol: toOptionalText((event as any).raw_symbol),
    base_asset: toOptionalText((event as any).base_asset),
    quote_asset: toOptionalText((event as any).quote_asset),
    pair: toOptionalText((event as any).pair),
    inventory_asset: toOptionalText((event as any).inventory_asset),
    display_name_short: toOptionalText((event as any).display_name_short),
    display_name_long: toOptionalText((event as any).display_name_long),
    side: normalizeTradeSide((event as any).side),
    price: price ?? null,
    qty: qty ?? null,
    mv: mv ?? null,
    fee: coerceFiniteNumber((event as any).fee) ?? null,
    fee_asset_raw: (event as any).fee_asset_raw ?? (event as any).fee_currency ?? null,
    fee_amount_raw: (event as any).fee_amount_raw ?? (event as any).fee_cost ?? null,
    fee_quote: coerceFiniteNumber((event as any).fee_quote) ?? null,
    exch_id: String(
      (event as any).exch_id ??
      (event as any).exec_id ??
      (event as any).exchange_trade_id ??
      (event as any).id ??
      (event as any).tx_hash ??
      (event as any).hash ??
      '',
    ),
    trade_id: String((event as any).trade_id ?? ''),
    signal_id: String((event as any).signal_id ?? ''),
    strategy_id: toOptionalText((event as any).strategy_id),
    order_id: String((event as any).exchange_order_id ?? (event as any).order_id ?? ''),
    decision: (event as any).decision,
    decision_timestamp: (event as any).decision_timestamp,
    gas_used: (event as any).gas_used ?? (event as any).gas,
    gas_units: coerceFiniteNumber((event as any).gas_units ?? (event as any).gas_used ?? (event as any).gas),
    notes: (event as any).notes,
    explorer_url: (event as any).explorer_url,
    placeholder: Boolean(event.placeholder),
    row_id: event.row_id,
    version,
    seq: Math.trunc(seq),
    ts,
  };
};

const PAGE_SIZE_OPTIONS = [50, 100, 200, 500];
const DEFAULT_PAGE_SIZE = 100;

const normalizePageSize = (value: unknown): number => {
  const parsed = parseInt(String(value ?? DEFAULT_PAGE_SIZE), 10);
  return PAGE_SIZE_OPTIONS.includes(parsed) ? parsed : DEFAULT_PAGE_SIZE;
};

const hasActiveFilters = (filters: FilterValues): boolean => {
  return Boolean(
    (filters.coin ?? '').trim()
    || (filters.market_type ?? '').trim()
    || (filters.exchange ?? '').trim()
    || (filters.side ?? '').trim()
    || (filters.signal_id ?? '').trim(),
  );
};

const rowMatchesFilters = (row: any, filters: FilterValues): boolean => {
  if (!filters) return true;
  const coinFilter = (filters.coin ?? '').trim();
  if (coinFilter) {
    const target = coinFilter.toUpperCase();
    const coinValue = String(row?.coin ?? row?.symbol ?? '').toUpperCase();
    const base = coinValue.split('/')[0];
    if (coinValue !== target && base !== target) {
      return false;
    }
  }

  const exchangeFilter = (filters.exchange ?? '').trim().toLowerCase();
  if (exchangeFilter) {
    const exchangeValue = String(row?.venue ?? row?.exchange ?? '').toLowerCase();
    if (exchangeValue !== exchangeFilter) {
      return false;
    }
  }

  const marketTypeFilter = (filters.market_type ?? '').trim().toLowerCase();
  if (marketTypeFilter) {
    const marketTypeValue = String(row?.product_type ?? row?.market_type ?? '').toLowerCase();
    if (marketTypeValue !== marketTypeFilter) {
      return false;
    }
  }

  const sideFilter = (filters.side ?? '').trim().toLowerCase();
  if (sideFilter) {
    const sideValue = String(row?.side ?? '').toLowerCase();
    if (sideValue !== sideFilter) {
      return false;
    }
  }

  const signalFilter = (filters.signal_id ?? '').trim().toLowerCase();
  if (signalFilter) {
    const sigValue = String(row?.signal_id ?? '').toLowerCase();
    if (!sigValue.includes(signalFilter)) {
      return false;
    }
  }

  return true;
};

const filterEventsForFilters = (events: TradeEvent[] | undefined | null, filters: FilterValues): TradeEvent[] => {
  if (!events?.length) {
    return [];
  }
  if (!hasActiveFilters(filters)) {
    return events;
  }
  return events.filter((event) => rowMatchesFilters(event, filters));
};

const loadStoredFilters = (): FilterValues => {
  if (typeof window === 'undefined' || !window?.sessionStorage) {
    return {};
  }
  try {
    const raw = window.sessionStorage.getItem(FILTER_STORAGE_KEY);
    if (!raw) return {};
    const parsed = JSON.parse(raw);
    if (!parsed || typeof parsed !== 'object') {
      return {};
    }
    const allowed = new Set(TRADE_FILTERS.map((f) => f.key));
    const sanitized: FilterValues = {};
    Object.entries(parsed as Record<string, unknown>).forEach(([key, value]) => {
      if (allowed.has(key) && typeof value === 'string') {
        sanitized[key] = value;
      }
    });
    return sanitized;
  } catch {
    return {};
  }
};

function PageSizeControl({ value, onChange }: { value: number; onChange: (val: number) => void }) {
  return (
    <label
      style={{
        display: 'flex',
        alignItems: 'center',
        gap: spacing.gap.xs,
        color: colors.text.secondary,
        fontSize: typography.fontSize.sm,
      }}
    >
      Page size
      <select
        value={value}
        onChange={(e) => onChange(parseInt(e.target.value, 10))}
        style={{
          backgroundColor: colors.bg.base,
          color: colors.text.primary,
          border: `1px solid ${colors.border.DEFAULT}`,
          borderRadius: borderRadius.md,
          padding: `${spacing.padding.xs} ${spacing.gap.sm}`,
          fontSize: typography.fontSize.sm,
        }}
      >
        {PAGE_SIZE_OPTIONS.map((option) => (
          <option key={option} value={option}>
            {option}
          </option>
        ))}
      </select>
    </label>
  );
}

type FetchOptions = {
  silent?: boolean;
  keepUnread?: boolean;
  cursor?: string | null;
  append?: boolean;
  resyncId?: number;
};

export default function Trades({
  dense = false,
  className = '',
  onRemove,
  showHeader = true,
}: {
  dense?: boolean;
  className?: string;
  tableClassName?: string;
  onRemove?: () => void;
  showHeader?: boolean;
} = {}) {
  if (PERF_RENDER_ENABLED) {
    try {
      performance.mark('Trades.render:start');
    } catch {
      // Ignore unsupported environments
    }
  }

  const storeRows = useTradesStore(selectTradesRows, shallow);
  const lastSeq = useTradesStore(selectTradesLastSeq);
  const setSnapshot = useTradesStore((state) => state.setSnapshot);
  const applyDelta = useTradesStore((state) => state.applyDelta);
  const resyncId = useResyncStore(selectResyncId);
  const { isResyncing } = useResyncStatus();
  const decisionDetailsEnabled = useMemo(() => isTradesDecisionDetailsEnabled(), []);

  const [pageSize, setPageSize] = useState(() => {
    if (typeof window === 'undefined' || !window?.sessionStorage) {
      return DEFAULT_PAGE_SIZE;
    }
    const stored = window.sessionStorage.getItem(PAGE_SIZE_STORAGE_KEY);
    return normalizePageSize(stored);
  });
  const [page, setPage] = useState<number>(1);
  const [total, setTotal] = useState(0);
  const [hasMore, setHasMore] = useState<boolean | null>(null);
  const [hasMorePage, setHasMorePage] = useState<number | null>(null);
  const [loading, setLoading] = useState(false);
  const [lastUpdate, setLastUpdate] = useState<number>(Date.now());
  const [filters, setFilters] = useState<FilterValues>(() => loadStoredFilters());
  const [sort, setSort] = useState<'ts_desc' | 'ts_asc'>('ts_desc');
  const [soundMuted, setSoundMutedState] = useState(() => getSoundMuted());
  const [unread, setUnread] = useState(0);
  const [pollDelay, setPollDelay] = useState(POLL_BASE_MS);
  const [socketConnected, setSocketConnected] = useState(true);
  const [isViewingLatest, setIsViewingLatest] = useState(true);
  const [perfHarnessActive, setPerfHarnessActive] = useState(false);
  const [surfaceState, setSurfaceState] = useState<RealtimeSurfaceState>(RealtimeSurfaceState.SYNCING);

  const abortRef = useRef<AbortController | null>(null);
  const mountedRef = useRef<boolean>(true);
  const debounceRef = useRef<number | null>(null);
  const pollTimeoutRef = useRef<number | null>(null);
  const latestSeqRef = useRef<number>(0);
  const mutedRef = useRef<boolean>(soundMuted);
  const pollDelayRef = useRef<number>(POLL_BASE_MS);
  const isViewingLatestRef = useRef<boolean>(true);
  const isAtTopRef = useRef<boolean>(true);
  const isUserScrollingRef = useRef<boolean>(false);
  const scrollElementRef = useRef<HTMLDivElement | null>(null);

  if (DEV_TRADES_PERF_HARNESS && perfHarnessActive) {
    return <TradesPerfHarness onClose={() => setPerfHarnessActive(false)} />;
  }
  const emptyPollCountRef = useRef<number>(0); // Track consecutive empty delta polls
  const socketConnectedRef = useRef<boolean>(true); // Track Socket.IO connection state
  const lastSoundSeqRef = useRef<number>(0); // Deduplicate sound playback by sequence
  const refreshTimeoutRef = useRef<number | null>(null); // Throttle snapshot refreshes
  const lastResetAtRef = useRef<number>(0); // Guard against reset thrash
  const pageSizeRef = useRef<number>(pageSize);
  const pageRef = useRef<number>(page);
  const requestSeqRef = useRef<number>(0);
  const activeForegroundRequestRef = useRef<number | null>(null);
  const filtersRef = useRef<FilterValues>(filters);
  const sortRef = useRef<'ts_desc' | 'ts_asc'>(sort);
  const lastSoundAtRef = useRef<number>(0);
  const applyDeltaRef = useRef(applyDelta);
  const queueSnapshotRefreshRef = useRef<typeof queueSnapshotRefresh | null>(null);
  const playSoundForSeqRef = useRef<typeof playSoundForSeq | null>(null);
  const isActiveRef = useRef<boolean>(true);
  const catchingUpRef = useRef<boolean>(false);
  const resyncIdRef = useRef<number>(resyncId);
  const reconnectCatchupInFlightRef = useRef<boolean>(false);
  const lastReconnectCatchupAtRef = useRef<number>(0);
  const latestTradeTsMsRef = useRef<number>(0);
  const latestTradeReplayCursorRef = useRef<TradeReplayCursor | null>(null);
  const streamCursorRef = useRef<TradeStreamCursor>({ lastSeq });
  const gapRecoveryTargetSeqRef = useRef<number | null>(null);
  const lastUpdateRef = useRef<number>(lastUpdate);
  const loadingRef = useRef<boolean>(loading);
  const isResyncingRef = useRef<boolean>(isResyncing);
  const surfaceStateRef = useRef<RealtimeSurfaceState>(RealtimeSurfaceState.SYNCING);

  const tradesController = useMemo(() => createRealtimeSurfaceController<TradeRow>({
    getRowId: (row) => row.row_id,
    compareRows: createTradeRowComparator(sort),
    initialRows: sort === 'ts_desc' ? storeRows : [...storeRows].reverse(),
    batchSchedule: (flush) => {
      if (typeof window === 'undefined') {
        flush();
        return () => {};
      }
      const id = window.requestAnimationFrame(flush);
      return () => window.cancelAnimationFrame(id);
    },
  }), [sort]);
  const tradesControllerRef = useRef(tradesController);
  const controllerState = useRealtimeSurfaceController(
    tradesController,
    (snapshot) => ({
      rows: snapshot.rows as TradeRow[],
      dataVersion: snapshot.dataVersion,
    }),
  );

  const trimControllerRows = useCallback(() => {
    const overflowRows = tradesControllerRef.current.getSnapshot().rows.slice(pageSizeRef.current);
    if (!overflowRows.length) {
      return;
    }
    tradesControllerRef.current.applyDelta(
      overflowRows.map((row) => ({ kind: 'delete', id: row.row_id } satisfies RealtimeRowDelta<TradeRow>)),
    );
  }, []);

  const applyControllerSnapshot = useCallback((rows: TradeEvent[] | undefined | null) => {
    const nextRows = (rows ?? [])
      .map((row) => toTradeRow(row))
      .filter((row): row is TradeRow => Boolean(row));
    tradesControllerRef.current.applySnapshot(nextRows);
  }, []);

  const applyControllerDelta = useCallback((events: TradeEvent[] | undefined | null) => {
    if (!events?.length) {
      return;
    }
    const deltas = events.flatMap((event) => {
      if (event.op === 'delete') {
        return [{ kind: 'delete', id: event.row_id } satisfies RealtimeRowDelta<TradeRow>];
      }
      const row = toTradeRow(event);
      return row
        ? [{ kind: 'upsert', row } satisfies RealtimeRowDelta<TradeRow>]
        : [];
    });
    if (!deltas.length) {
      return;
    }
    tradesControllerRef.current.applyDelta(deltas);
    trimControllerRows();
  }, [trimControllerRows]);

  const syncSurfaceState = useCallback(() => {
    let nextState: RealtimeSurfaceState;
    if (loadingRef.current && tradesControllerRef.current.getSnapshot().rows.length === 0) {
      nextState = RealtimeSurfaceState.SYNCING;
    } else if (
      catchingUpRef.current
      || isResyncingRef.current
      || !streamCursorRef.current.streamId
      || streamCursorRef.current.snapshotRevision == null
    ) {
      nextState = RealtimeSurfaceState.RECOVERING;
    } else {
      const ageMs = Date.now() - lastUpdateRef.current;
      if (ageMs > TRADE_HEALTH_STALE_MS) {
        nextState = RealtimeSurfaceState.STALE;
      } else if (ageMs > STALE_THRESHOLDS.REALTIME) {
        nextState = RealtimeSurfaceState.LAGGING;
      } else {
        nextState = RealtimeSurfaceState.LIVE;
      }
    }

    surfaceStateRef.current = nextState;
    setSurfaceState((current) => (current === nextState ? current : nextState));
    return nextState;
  }, []);

  const advanceTradeReplayCursor = useCallback((rows: Array<any> | undefined | null) => {
    const latestCursor = getLatestTradeReplayCursor(rows);
    if (!latestCursor) {
      return;
    }
    const currentCursor = latestTradeReplayCursorRef.current;
    if (!currentCursor || compareTradeReplayCursor(latestCursor, currentCursor) > 0) {
      latestTradeReplayCursorRef.current = latestCursor;
    }
    latestTradeTsMsRef.current = Math.max(latestTradeTsMsRef.current, latestCursor.tsMs);
  }, []);

  useEffect(() => {
    if (!PERF_RENDER_ENABLED) {
      return;
    }
    try {
      performance.measure('Trades.render', 'Trades.render:start');
      performance.clearMarks('Trades.render:start');
    } catch {
      // Ignore unsupported environments
    }
  });

  useEffect(() => {
    tradesControllerRef.current = tradesController;
    return () => {
      tradesController.destroy();
    };
  }, [tradesController]);

  useEffect(() => {
    mutedRef.current = soundMuted;
  }, [soundMuted]);

  useEffect(() => {
    loadingRef.current = loading;
    syncSurfaceState();
  }, [loading, syncSurfaceState]);

  useEffect(() => {
    isResyncingRef.current = isResyncing;
    syncSurfaceState();
  }, [isResyncing, syncSurfaceState]);

  useEffect(() => {
    lastUpdateRef.current = lastUpdate;
    syncSurfaceState();
  }, [lastUpdate, syncSurfaceState]);

  useEffect(() => {
    syncSurfaceState();
  }, [syncSurfaceState]);

  useEffect(() => {
    mountedRef.current = true;
    return () => {
      mountedRef.current = false;
      abortRef.current?.abort();
      abortRef.current = null;

      if (typeof window !== 'undefined') {
        if (refreshTimeoutRef.current !== null) {
          window.clearTimeout(refreshTimeoutRef.current);
          refreshTimeoutRef.current = null;
        }
        if (pollTimeoutRef.current !== null) {
          window.clearTimeout(pollTimeoutRef.current);
          pollTimeoutRef.current = null;
        }
        if (debounceRef.current !== null) {
          window.clearTimeout(debounceRef.current);
          debounceRef.current = null;
        }
      }
    };
  }, []);

  useEffect(() => {
    if (typeof window === 'undefined') {
      return undefined;
    }
    const id = window.setInterval(() => {
      syncSurfaceState();
    }, 1_000);
    return () => {
      window.clearInterval(id);
    };
  }, [syncSurfaceState]);

  useEffect(() => {
    latestSeqRef.current = lastSeq;
    streamCursorRef.current.lastSeq = Math.max(streamCursorRef.current.lastSeq, lastSeq);
    syncSurfaceState();
  }, [lastSeq, syncSurfaceState]);

  useEffect(() => {
    pollDelayRef.current = pollDelay;
  }, [pollDelay]);

  useEffect(() => {
    filtersRef.current = filters;
  }, [filters]);

  useEffect(() => {
    sortRef.current = sort;
  }, [sort]);

  useEffect(() => {
    applyDeltaRef.current = applyDelta;
  }, [applyDelta]);

  useEffect(() => {
    resyncIdRef.current = resyncId;
  }, [resyncId]);

  const rowsToRender = controllerState.rows;

  const recomputeIsViewingLatest = useCallback(
    (atTopOverride?: boolean) => {
      const atTop = typeof atTopOverride === 'boolean' ? atTopOverride : isAtTopRef.current;
      const latest = atTop && pageRef.current === 1 && sortRef.current === 'ts_desc';
      isViewingLatestRef.current = latest;
      setIsViewingLatest(latest);
      if (latest) {
        setUnread(0);
      }
      return latest;
    },
    [setIsViewingLatest, setUnread],
  );

  const fetchPage = useCallback(
    async (options: FetchOptions = {}) => {
      if (refreshTimeoutRef.current !== null) {
        if (typeof window !== 'undefined') {
          window.clearTimeout(refreshTimeoutRef.current);
        }
        refreshTimeoutRef.current = null;
      }

      abortRef.current?.abort();
      const ac = new AbortController();
      abortRef.current = ac;
      const requestSeq = requestSeqRef.current + 1;
      requestSeqRef.current = requestSeq;
      const isForegroundRequest = !options.silent;

      if (isForegroundRequest) {
        activeForegroundRequestRef.current = requestSeq;
        setLoading(true);
      }

      const params: Record<string, string | number | undefined> = {
        ...filtersRef.current,
        sort: sortRef.current,
      };
      const requestResyncId = options.resyncId ?? resyncIdRef.current;
      const requestPage = pageRef.current;
      // Offset-based pagination: pageRef defines which slice to fetch

      try {
        const response = await api.getTrades(requestPage, pageSizeRef.current, params, { signal: ac.signal });
        if (abortRef.current !== ac) {
          return;
        }
        if (!mountedRef.current) {
          return;
        }

        // Snapshot for the current page slice
        const snapshotResult = setSnapshot(response.rows || [], pageSizeRef.current, requestResyncId);
        if (snapshotResult?.applied) {
          applyControllerSnapshot(response.rows);
        }
        if (snapshotResult?.applied) {
          markGlobalResyncApplied('trades', requestResyncId);
        }
        // Update latest-viewing flag based on page and scroll state
        recomputeIsViewingLatest();

        const totalCount = response.total ?? response.total_records ?? 0;
        setTotal(totalCount);
        setHasMore(typeof response.has_more === 'boolean' ? response.has_more : null);
        setHasMorePage(requestPage);

        if (!options.keepUnread) {
          setUnread(0);
        }

        advanceTradeReplayCursor(response.rows);
        const snapshotRowsMaxSeq = (response.rows || []).reduce(
          (max, row) => Math.max(max, coerceFiniteNumber((row as any)?.seq) ?? 0),
          0,
        );
        const currentStreamCursor = streamCursorRef.current;
        const responseStreamId =
          typeof response.stream_id === 'string' && response.stream_id.trim()
            ? response.stream_id.trim()
            : currentStreamCursor.streamId;
        const responseSnapshotRevision =
          response.snapshot_revision ?? currentStreamCursor.snapshotRevision;
        const snapshotLastSeq =
          typeof response.last_seq === 'number'
            ? Math.max(0, response.last_seq, snapshotRowsMaxSeq)
            : snapshotRowsMaxSeq;
        const snapshotEpochChanged =
          (currentStreamCursor.streamId && responseStreamId && responseStreamId !== currentStreamCursor.streamId)
          || (
            currentStreamCursor.snapshotRevision != null
            && responseSnapshotRevision != null
            && responseSnapshotRevision !== currentStreamCursor.snapshotRevision
          );
        const nextLastSeq = snapshotEpochChanged
          ? snapshotLastSeq
          : Math.max(currentStreamCursor.lastSeq, snapshotLastSeq);
        streamCursorRef.current = {
          contractVersion:
            typeof response.contract_version === 'number'
              ? response.contract_version
              : currentStreamCursor.contractVersion,
          streamId: responseStreamId,
          snapshotRevision: responseSnapshotRevision,
          lastSeq: nextLastSeq,
        };
        latestSeqRef.current = nextLastSeq;
        gapRecoveryTargetSeqRef.current = null;
        catchingUpRef.current = false;

        const nextUpdate = Date.now();
        lastUpdateRef.current = nextUpdate;
        setLastUpdate(nextUpdate);
        syncSurfaceState();
      } catch (e) {
        if ((e as any).name !== 'AbortError' && abortRef.current === ac) {
          console.error('[trades] load failed:', e);
          catchingUpRef.current = true;
          syncSurfaceState();
        }
      } finally {
        if (abortRef.current === ac) {
          abortRef.current = null;
        }
        if (
          mountedRef.current
          && isForegroundRequest
          && activeForegroundRequestRef.current === requestSeq
        ) {
          activeForegroundRequestRef.current = null;
          setLoading(false);
        }
        if (mountedRef.current) {
          syncSurfaceState();
        }
      }
    },
    [
      applyControllerSnapshot,
      setSnapshot,
      setTotal,
      setUnread,
      setLoading,
      recomputeIsViewingLatest,
      advanceTradeReplayCursor,
      syncSurfaceState,
    ],
  );

  const handleTimeSortChange = useCallback((direction: 'ts_desc' | 'ts_asc') => {
    if (sortRef.current === direction) {
      return;
    }
    setSort(direction);
    sortRef.current = direction;
    pageRef.current = 1;
    setPage(1);
    setHasMore(null);
    setHasMorePage(null);
    isAtTopRef.current = true;
    recomputeIsViewingLatest(true);
    fetchPage();
  }, [fetchPage, recomputeIsViewingLatest]);

  const handleScrollStateChange = useCallback(
    ({ atTop, isScrolling, scrollElement }: TradesTableScrollState) => {
      isAtTopRef.current = atTop;
      isUserScrollingRef.current = isScrolling;
      if (scrollElement) {
        scrollElementRef.current = scrollElement;
      }
      recomputeIsViewingLatest(atTop);
    },
    [recomputeIsViewingLatest],
  );

  useEffect(() => {
    pageSizeRef.current = pageSize;
    pageRef.current = page;
    setHasMore(null);
    setHasMorePage(null);
    if (typeof window !== 'undefined' && window?.sessionStorage) {
      try {
        window.sessionStorage.setItem(PAGE_SIZE_STORAGE_KEY, String(pageSize));
      } catch {}
    }
    fetchPage();
  }, [pageSize, page, fetchPage]);

  const queueSnapshotRefresh = useCallback(
    (keepUnread?: boolean) => {
      if (refreshTimeoutRef.current !== null) {
        return;
      }

      const keepUnreadFlag = keepUnread ?? !isViewingLatestRef.current;

      if (!keepUnreadFlag && isViewingLatestRef.current) {
        return;
      }

      refreshTimeoutRef.current = window.setTimeout(() => {
        refreshTimeoutRef.current = null;
        fetchPage({ silent: true, keepUnread: keepUnreadFlag });
      }, 250);
    },
    [fetchPage],
  );

  const playSoundForSeq = useCallback(
    (seq?: number) => {
      if (typeof seq !== 'number') {
        return;
      }
      if (!isActiveRef.current) {
        return;
      }
      if (seq <= lastSoundSeqRef.current) {
        lastSoundSeqRef.current = seq;
        return;
      }
      const now = Date.now();
      if (now - lastSoundAtRef.current < SOUND.TRADE_CLICK_THROTTLE_MS) {
        lastSoundSeqRef.current = seq;
        return;
      }

      if (!mutedRef.current) {
        playTradeClick();
      }
      lastSoundAtRef.current = now;
      lastSoundSeqRef.current = seq;
    },
    [],
  );

  // Keep callback refs in sync with latest values for socket handler
  // These assignments happen on every render to ensure the socket handler
  // (which has a stable reference) always calls the latest callback versions
  queueSnapshotRefreshRef.current = queueSnapshotRefresh;
  playSoundForSeqRef.current = playSoundForSeq;

  const handleFilterChange = useCallback(
    (newFilters: FilterValues) => {
      setFilters(newFilters);
      if (debounceRef.current) window.clearTimeout(debounceRef.current);
      debounceRef.current = window.setTimeout(() => {
        filtersRef.current = newFilters;
        // Reset to first page on filter changes
        pageRef.current = 1;
        setPage(1);
        setHasMore(null);
        setHasMorePage(null);
        isAtTopRef.current = true;
        recomputeIsViewingLatest(true);
        fetchPage();
      }, DEBOUNCE_MS);
    },
    [fetchPage, recomputeIsViewingLatest],
  );

  // Pagination controls
  const totalPages = useMemo(() => Math.max(1, Math.ceil((total || 0) / Math.max(pageSize, 1))), [total, pageSize]);
  const canPrev = useMemo(() => page > 1, [page]);
  const hasFreshHasMore = useMemo(
    () => hasMorePage === page && typeof hasMore === 'boolean',
    [hasMorePage, page, hasMore],
  );
  const canNext = useMemo(() => {
    if (loading) {
      return false;
    }
    if (hasFreshHasMore) {
      return hasMore;
    }
    return page < totalPages;
  }, [loading, hasFreshHasMore, hasMore, page, totalPages]);
  const showUnboundedPageIndicator = useMemo(
    () => hasFreshHasMore && hasMore === true && page >= totalPages,
    [hasFreshHasMore, hasMore, page, totalPages],
  );
  const goPrev = useCallback(() => {
    if (!canPrev) return;
    const nextPage = Math.max(1, page - 1);
    setLoading(true);
    setHasMore(null);
    setHasMorePage(null);
    setPage(nextPage);
    pageRef.current = nextPage;
    recomputeIsViewingLatest();
  }, [page, canPrev, recomputeIsViewingLatest]);
  const goNext = useCallback(() => {
    if (!canNext) return;
    const nextPage = hasFreshHasMore ? page + 1 : Math.min(totalPages, page + 1);
    setLoading(true);
    setHasMore(null);
    setHasMorePage(null);
    setPage(nextPage);
    pageRef.current = nextPage;
    recomputeIsViewingLatest();
  }, [page, canNext, totalPages, hasFreshHasMore, recomputeIsViewingLatest]);

  useEffect(() => {
    if (typeof window === 'undefined' || !window?.sessionStorage) return;
    try {
      window.sessionStorage.setItem(FILTER_STORAGE_KEY, JSON.stringify(filters));
    } catch {
      // Ignore storage errors
    }
  }, [filters]);

  useEffect(
    () => () => {
      if (abortRef.current) abortRef.current.abort();
    },
    [],
  );

  useEffect(
    () => () => {
      if (debounceRef.current) window.clearTimeout(debounceRef.current);
    },
    [],
  );

  useEffect(
    () => () => {
      if (refreshTimeoutRef.current) {
        window.clearTimeout(refreshTimeoutRef.current);
      }
    },
    [],
  );

  const schedulePoll = useCallback(() => {
    if (!isActiveRef.current) {
      return;
    }
    if (pollTimeoutRef.current) {
      window.clearTimeout(pollTimeoutRef.current);
    }
    // Choose dynamic base depending on socket connectivity
    const dynamicBase = socketConnectedRef.current ? POLL_BASE_MS : POLL_BASE_MS_DISCONNECTED;
    const delay = Math.max(pollDelayRef.current || dynamicBase, dynamicBase);
    pollTimeoutRef.current = window.setTimeout(async () => {
      if (!isActiveRef.current) {
        return;
      }
      const currentSurfaceState = syncSurfaceState();
      const shouldReplay =
        currentSurfaceState === RealtimeSurfaceState.RECOVERING
        || currentSurfaceState === RealtimeSurfaceState.LAGGING
        || currentSurfaceState === RealtimeSurfaceState.STALE;
      if (!shouldReplay) {
        setPollDelay(dynamicBase);
        schedulePoll();
        return;
      }
      try {
        const pollResyncId = resyncIdRef.current;
        const streamCursor = streamCursorRef.current;
        const gapRecoveryTargetSeq = gapRecoveryTargetSeqRef.current;
        const requestedSinceSeq = streamCursor.lastSeq;
        const deltaCursor = {
          sinceSeq: requestedSinceSeq,
          streamId: streamCursor.streamId,
          snapshotRevision: streamCursor.snapshotRevision,
        };
        const delta = await api.getTradesDelta(deltaCursor, DELTA_LIMIT);

        if (!isActiveRef.current) {
          return;
        }
        const responseStreamId =
          typeof delta.stream_id === 'string' && delta.stream_id.trim()
            ? delta.stream_id.trim()
            : streamCursor.streamId;
        const responseSnapshotRevision = delta.snapshot_revision ?? streamCursor.snapshotRevision;
        const deltaRowsMaxSeq = (delta.rows || []).reduce(
          (max, row) => Math.max(max, coerceFiniteNumber((row as any)?.seq) ?? 0),
          0,
        );
        const effectiveDeltaLastSeq =
          typeof delta.last_seq === 'number'
            ? Math.max(delta.last_seq, deltaRowsMaxSeq)
            : deltaRowsMaxSeq;
        const hasStreamMismatch =
          (streamCursor.streamId && responseStreamId && responseStreamId !== streamCursor.streamId)
          || (
            streamCursor.snapshotRevision != null
            && responseSnapshotRevision != null
            && responseSnapshotRevision !== streamCursor.snapshotRevision
          );
        if (hasStreamMismatch) {
          catchingUpRef.current = true;
          await fetchPage({
            keepUnread: !isViewingLatestRef.current,
            silent: true,
            resyncId: pollResyncId,
          });
          return;
        }
        streamCursorRef.current = {
          contractVersion:
            typeof delta.contract_version === 'number'
              ? delta.contract_version
              : streamCursor.contractVersion,
          streamId: responseStreamId,
          snapshotRevision: responseSnapshotRevision,
          lastSeq:
            effectiveDeltaLastSeq > 0
              ? Math.max(streamCursor.lastSeq, effectiveDeltaLastSeq)
              : streamCursor.lastSeq,
        };

        // DEFENSIVE FIX: Validate sequence consistency
        const deltaLastSeq = effectiveDeltaLastSeq > 0 ? effectiveDeltaLastSeq : null;
        const hasNumericLastSeq = deltaLastSeq !== null;
        const seqIsNonRegressive =
          hasNumericLastSeq
          && deltaLastSeq >= requestedSinceSeq;
        const seqAdvanced =
          hasNumericLastSeq
          && deltaLastSeq > requestedSinceSeq;
        const gapRecoveryResolved =
          gapRecoveryTargetSeq == null
          || (
            hasNumericLastSeq
            && deltaLastSeq >= gapRecoveryTargetSeq
          );
        if (hasNumericLastSeq && !seqIsNonRegressive) {
          console.warn(
            `[trades] Delta seq regression detected! Backend last_seq (${deltaLastSeq}) < ` +
            `frontend latestSeq (${requestedSinceSeq}). This suggests missed trades. ` +
            `Socket.IO connected: ${socketConnectedRef.current}`
          );
        }

        if (delta.reset_required) {
          const now = Date.now();
          const sinceLast = now - lastResetAtRef.current;
          if (sinceLast >= 10_000) {
            console.log('[trades] Delta poll requires full reset, fetching snapshot');
            lastResetAtRef.current = now;
            catchingUpRef.current = true;
            await fetchPage({
              keepUnread: !isViewingLatestRef.current,
              silent: true,
              resyncId: pollResyncId,
            });
          } else {
            console.warn('[trades] Reset requested but throttled to prevent thrash');
          }
          if (!isActiveRef.current) {
            return;
          }
          setPollDelay((prev) => {
            const base = socketConnectedRef.current ? POLL_BASE_MS : POLL_BASE_MS_DISCONNECTED;
            pollDelayRef.current = base;
            return base;
          });
          setUnread(0);
          emptyPollCountRef.current = 0; // Reset empty poll counter
        } else {
          let pollAcknowledgedCurrentEpoch = false;
          let appliedCurrentEpoch = false;
          const replayRows = [...(delta.rows || [])];
          advanceTradeReplayCursor(replayRows);
          const rowsForView = filterEventsForFilters(replayRows, filtersRef.current);
          if (rowsForView.length) {
            const isLiveView = isViewingLatestRef.current && sortRef.current === 'ts_desc';
            let liveNewRows = 0;
            if (isLiveView) {
              const stats = applyDeltaRef.current(rowsForView, pageSizeRef.current, pollResyncId);
              liveNewRows = stats?.newRows ?? 0;
              appliedCurrentEpoch = Boolean(stats?.applied);
              if (stats?.applied) {
                applyControllerDelta(rowsForView);
              }
            }

            const filteredUpserts = rowsForView.filter((evt) => evt.op === 'upsert').length;
            if (!isLiveView && filteredUpserts > 0) {
              setUnread((u) => u + filteredUpserts);
            }

            if (typeof delta.last_seq === 'number') {
              latestSeqRef.current = Math.max(latestSeqRef.current, delta.last_seq);
            }
            if (!appliedCurrentEpoch && seqIsNonRegressive && gapRecoveryResolved) {
              pollAcknowledgedCurrentEpoch = true;
            }

            if (liveNewRows > 0) {
              playSoundForSeq(typeof delta.last_seq === 'number' ? delta.last_seq : undefined);
            }

            queueSnapshotRefreshRef.current?.(!isLiveView);
            emptyPollCountRef.current = 0; // Reset empty poll counter on successful sync
            const nextUpdate = Date.now();
            lastUpdateRef.current = nextUpdate;
            setLastUpdate(nextUpdate);
          } else if (replayRows.length) {
            // Rows existed but did not match current filters; treat as successful sync for poll timing.
            emptyPollCountRef.current = 0;
            if (typeof delta.last_seq === 'number') {
              latestSeqRef.current = Math.max(latestSeqRef.current, delta.last_seq);
            }
            if (seqIsNonRegressive && gapRecoveryResolved) {
              pollAcknowledgedCurrentEpoch = true;
            }
          } else {
            // DEFENSIVE FIX: Track consecutive empty polls
            if (seqIsNonRegressive && (gapRecoveryTargetSeq == null || seqAdvanced)) {
              const lastSeq = delta.last_seq as number;
              // Empty rows only reconcile a seq-gap recovery when the backend advances beyond sinceSeq.
              latestSeqRef.current = Math.max(latestSeqRef.current, lastSeq);
              emptyPollCountRef.current = 0;
              if (gapRecoveryResolved) {
                pollAcknowledgedCurrentEpoch = true;
              }
            } else {
              // Empty rows without a usable sequence might indicate a problem.
              emptyPollCountRef.current += 1;
              if (emptyPollCountRef.current >= MAX_EMPTY_POLLS) {
                console.warn(
                  `[trades] ${emptyPollCountRef.current} consecutive empty delta polls. ` +
                  `Socket.IO connected: ${socketConnectedRef.current}. ` +
                  `This may indicate Socket.IO missed events or backend is not emitting trades.`
                );
              }
            }
          }
          const replayResolvedCurrentEpoch =
            gapRecoveryResolved
            && (appliedCurrentEpoch || pollAcknowledgedCurrentEpoch);
          if (replayResolvedCurrentEpoch) {
            markGlobalResyncApplied('trades', pollResyncId);
            const nextUpdate = Date.now();
            lastUpdateRef.current = nextUpdate;
            setLastUpdate(nextUpdate);
          }
          if (
            socketConnectedRef.current
            && replayRows.length < DELTA_LIMIT
            && replayResolvedCurrentEpoch
          ) {
            gapRecoveryTargetSeqRef.current = null;
            catchingUpRef.current = false;
          }
          if (!isActiveRef.current) {
            return;
          }
          setPollDelay((prev) => {
            const base = socketConnectedRef.current ? POLL_BASE_MS : POLL_BASE_MS_DISCONNECTED;
            pollDelayRef.current = base;
            return base;
          });
          syncSurfaceState();
        }
      } catch (err) {
        console.error('[trades] delta poll failed', err);
        if (!isActiveRef.current) {
          return;
        }
        catchingUpRef.current = true;
        setPollDelay((prev) => {
          const next = Math.min((prev || POLL_BASE_MS) * 2, POLL_MAX_MS);
          pollDelayRef.current = next;
          return next;
        });
        syncSurfaceState();
      } finally {
        if (!isActiveRef.current) {
          return;
        }
        schedulePoll();
      }
    }, delay);
  }, [applyControllerDelta, fetchPage, playSoundForSeq, advanceTradeReplayCursor, syncSurfaceState]);

  useEffect(() => {
    schedulePoll();
    return () => {
      if (pollTimeoutRef.current) {
        window.clearTimeout(pollTimeoutRef.current);
        pollTimeoutRef.current = null;
      }
    };
  }, [schedulePoll]);

  useEffect(() => {
    setPollDelay((prev) => {
      pollDelayRef.current = POLL_BASE_MS;
      return POLL_BASE_MS;
    });
  }, [filters, pageSize, sort]);

  useEffect(() => {
    setUnread(0);
  }, [filters]);

  const processTradeMessage = useCallback((msg: any) => {
    if (!msg || typeof msg !== 'object') {
      return;
    }
    try {
      const normalizedMsg = (msg?.trade && typeof msg.trade === 'object')
        ? {
            ...msg.trade,
            op: msg.op,
            row_id: msg.row_id ?? msg.trade?.row_id,
            version: msg.version ?? msg.trade?.version,
            // Prefer the trade-stream seq/ts for dedupe/order; the outer msg.seq is a transport sequence.
            seq: msg.trade?.seq ?? msg.seq,
            ts_ms: msg.trade?.ts_ms ?? msg.ts_ms ?? msg.server_ts_ms,
            strategy_id: msg.strategy_id ?? msg.trade?.strategy_id,
            signal_id: msg.signal_id ?? msg.strategy_id ?? msg.trade?.signal_id ?? msg.trade?.strategy_id,
            stream_id: msg.stream_id ?? msg.trade?.stream_id,
            snapshot_revision: msg.snapshot_revision ?? msg.trade?.snapshot_revision,
            contract_version: msg.contract_version ?? msg.trade?.contract_version,
          }
        : msg;
      const normalizedEventCandidate = normalizeTradeEventLike(normalizedMsg);
      const isPubsubEvent =
        normalizedEventCandidate?.op
        && normalizedEventCandidate?.row_id
        && typeof normalizedEventCandidate?.version === 'number'
        && typeof normalizedEventCandidate?.seq === 'number';
      let event: TradeEvent;
      if (isPubsubEvent) {
        event = normalizedEventCandidate as TradeEvent;
      } else {
        const now = Date.now();
        const timestampParts = getTimestampParts(normalizedEventCandidate);
        if (!timestampParts.hasReliableTimestamp) {
          return;
        }
        const seq: number = timestampParts.seq ?? timestampParts.tsMs ?? timestampParts.ts ?? now;
        const rowIdFromMsg: string | undefined =
          typeof normalizedEventCandidate?.row_id === 'string' && normalizedEventCandidate.row_id ? normalizedEventCandidate.row_id : undefined;
        const rowId: string = rowIdFromMsg || (
          (normalizedEventCandidate
            && (normalizedEventCandidate.exch_id || normalizedEventCandidate.trade_id || normalizedEventCandidate.order_id))
          || `${normalizedEventCandidate?.exchange || ''}:${normalizedEventCandidate?.coin || ''}:${seq}`
        );
        const versionFromMsg: number | undefined =
          typeof normalizedEventCandidate?.version === 'number' && Number.isFinite(normalizedEventCandidate.version)
            ? normalizedEventCandidate.version
            : undefined;
        const parsedPrice = coerceFiniteNumber(normalizedEventCandidate?.price);
        const parsedQty = coerceFiniteNumber(normalizedEventCandidate?.qty);
        const derivedMv =
          parsedPrice !== undefined && parsedQty !== undefined
            ? parsedPrice * parsedQty
            : undefined;
        const rawMv = coerceFiniteNumber(normalizedEventCandidate?.mv ?? normalizedEventCandidate?.notional);
        const normalizedMv =
          (rawMv === undefined || rawMv === 0) && derivedMv !== undefined && derivedMv !== 0
            ? derivedMv
            : rawMv;
        event = {
          op: 'upsert',
          row_id: rowId,
          version: versionFromMsg ?? 1,
          seq,
          ts_ms: extractTradeTimestampMs(normalizedEventCandidate),
          ts: seq,
          time: normalizedEventCandidate?.time,
          coin: normalizedEventCandidate?.coin,
          exchange: normalizedEventCandidate?.exchange,
          instrument_id: normalizedEventCandidate?.instrument_id,
          instrument_uid: normalizedEventCandidate?.instrument_uid,
          venue: normalizedEventCandidate?.venue,
          venue_root: normalizedEventCandidate?.venue_root,
          product_type: normalizedEventCandidate?.product_type,
          market_type: normalizedEventCandidate?.market_type,
          contract_type: normalizedEventCandidate?.contract_type,
          raw_symbol: normalizedEventCandidate?.raw_symbol,
          base_asset: normalizedEventCandidate?.base_asset,
          quote_asset: normalizedEventCandidate?.quote_asset,
          pair: normalizedEventCandidate?.pair,
          inventory_asset: normalizedEventCandidate?.inventory_asset,
          display_name_short: normalizedEventCandidate?.display_name_short,
          display_name_long: normalizedEventCandidate?.display_name_long,
          side: normalizedEventCandidate?.side,
          price: normalizedEventCandidate?.price,
          qty: normalizedEventCandidate?.qty,
          mv: normalizedMv,
          fee: normalizedEventCandidate?.fee,
          exec_id: normalizedEventCandidate?.exch_id,
          trade_id: normalizedEventCandidate?.trade_id,
          order_id: normalizedEventCandidate?.order_id ?? normalizedEventCandidate?.client_order_id,
          signal_id: normalizedEventCandidate?.signal_id ?? normalizedEventCandidate?.strategy_id,
          strategy_id: normalizedEventCandidate?.strategy_id,
          decision: normalizedEventCandidate?.decision,
          notes: normalizedEventCandidate?.notes,
          explorer_url: normalizedEventCandidate?.explorer_url,
          stream_id: normalizedEventCandidate?.stream_id,
          snapshot_revision: normalizedEventCandidate?.snapshot_revision,
          contract_version: normalizedEventCandidate?.contract_version,
        } as TradeEvent;
      }

      const messageResyncId = resyncIdRef.current;
      const eventStreamId = typeof (event as any).stream_id === 'string' && (event as any).stream_id.trim()
        ? (event as any).stream_id.trim()
        : undefined;
      const eventSnapshotRevision = (event as any).snapshot_revision;
      const currentStreamCursor = streamCursorRef.current;
      const hasStreamMismatch =
        (currentStreamCursor.streamId && eventStreamId && eventStreamId !== currentStreamCursor.streamId)
        || (
          currentStreamCursor.snapshotRevision != null
          && eventSnapshotRevision != null
          && eventSnapshotRevision !== currentStreamCursor.snapshotRevision
        );
      if (hasStreamMismatch) {
        catchingUpRef.current = true;
        queueSnapshotRefreshRef.current?.(true);
        syncSurfaceState();
        return;
      }
      const hasSeqGap =
        typeof event.seq === 'number'
        && currentStreamCursor.streamId != null
        && currentStreamCursor.snapshotRevision != null
        && eventStreamId != null
        && eventSnapshotRevision != null
        && eventStreamId === currentStreamCursor.streamId
        && eventSnapshotRevision === currentStreamCursor.snapshotRevision
        && event.seq > currentStreamCursor.lastSeq + 1;
      if (hasSeqGap) {
        gapRecoveryTargetSeqRef.current = Math.max(
          gapRecoveryTargetSeqRef.current ?? 0,
          event.seq,
        );
        catchingUpRef.current = true;
        syncSurfaceState();
        schedulePoll();
        return;
      }
      streamCursorRef.current = {
        contractVersion:
          typeof (event as any).contract_version === 'number'
            ? (event as any).contract_version
            : currentStreamCursor.contractVersion,
        streamId: eventStreamId ?? currentStreamCursor.streamId,
        snapshotRevision: eventSnapshotRevision ?? currentStreamCursor.snapshotRevision,
        lastSeq: currentStreamCursor.lastSeq,
      };
      const eventTsMs = extractTradeTimestampMs(event);
      if (eventTsMs !== undefined) {
        latestTradeTsMsRef.current = Math.max(latestTradeTsMsRef.current, eventTsMs);
      }
      advanceTradeReplayCursor([event]);
      const passesFilters = rowMatchesFilters(event, filtersRef.current);
      const rowVisibleInCurrentStore = Array.isArray(storeRows)
        && storeRows.some((row) => row?.row_id === event.row_id);
      if (!passesFilters) {
        if (typeof event.seq === 'number') {
          latestSeqRef.current = Math.max(latestSeqRef.current, event.seq);
          streamCursorRef.current.lastSeq = Math.max(streamCursorRef.current.lastSeq, event.seq);
        }
        if (rowVisibleInCurrentStore || event.op === 'delete') {
          catchingUpRef.current = true;
          queueSnapshotRefreshRef.current?.(true);
        }
        syncSurfaceState();
        return;
      }

      const isLiveView = isViewingLatestRef.current && sortRef.current === 'ts_desc';
      const op = event.op ?? 'upsert';
      let appliedCurrentEpoch = false;
      if (isLiveView) {
        const stats = applyDeltaRef.current([event], pageSizeRef.current, messageResyncId);
        appliedCurrentEpoch = Boolean(stats?.applied);
        if (stats?.applied) {
          applyControllerDelta([event]);
        }
        if (op === 'upsert' && (stats?.newRows ?? 0) > 0 && typeof event.seq === 'number') {
          playSoundForSeqRef.current?.(event.seq);
        }
      } else if (op === 'upsert') {
        setUnread((u) => u + 1);
      }

      if (typeof event.seq === 'number') {
        latestSeqRef.current = Math.max(latestSeqRef.current, event.seq);
        streamCursorRef.current.lastSeq = Math.max(streamCursorRef.current.lastSeq, event.seq);
      }
      queueSnapshotRefreshRef.current?.(!isLiveView);
      if (appliedCurrentEpoch) {
        markGlobalResyncApplied('trades', messageResyncId);
      }
      const nextUpdate = Date.now();
      lastUpdateRef.current = nextUpdate;
      setLastUpdate(nextUpdate);
      catchingUpRef.current = false;
      syncSurfaceState();
    } catch (err) {
      console.error('[trades] socket trade_update error', err);
    }
  }, [applyControllerDelta, setUnread, advanceTradeReplayCursor, storeRows, syncSurfaceState, schedulePoll]);

  useEffect(() => {
    const pending: any[] = [];
    let rafId: number | null = null;
    let idleTimer: number | null = null;

    const flushPending = () => {
      if (rafId !== null && typeof window !== 'undefined') {
        window.cancelAnimationFrame(rafId);
        rafId = null;
      }
      if (idleTimer !== null && typeof window !== 'undefined') {
        window.clearTimeout(idleTimer);
        idleTimer = null;
      }
      if (!pending.length) {
        return;
      }
      const items = pending.splice(0);
      for (const item of items) {
        processTradeMessage(item);
      }
    };

    const scheduleFrame = () => {
      if (rafId !== null || typeof window === 'undefined') {
        return;
      }
      rafId = window.requestAnimationFrame(() => {
        rafId = null;
        flushPending();
      });
    };

    const scheduleIdleFlush = () => {
      if (idleTimer !== null || typeof window === 'undefined') {
        return;
      }
      idleTimer = window.setTimeout(() => {
        idleTimer = null;
        flushPending();
      }, 150);
    };

    const enqueueTradeMessage = (msg: any) => {
      pending.push(msg);
      if (isUserScrollingRef.current) {
        scheduleIdleFlush();
      } else {
        scheduleFrame();
      }
    };

    const handleTradeUpdate = (msg: any) => {
      enqueueTradeMessage(msg);
    };

    socket.on('trade_update', handleTradeUpdate);
    return () => {
      socket.off('trade_update', handleTradeUpdate);
      flushPending();
      if (rafId !== null && typeof window !== 'undefined') {
        window.cancelAnimationFrame(rafId);
      }
      if (idleTimer !== null && typeof window !== 'undefined') {
        window.clearTimeout(idleTimer);
      }
    };
  }, [processTradeMessage]);

  // DEFENSIVE FIX: Track Socket.IO connection state for debugging
  useEffect(() => {
    const handleConnect = () => {
      console.log('[trades] Socket.IO connected');
      socketConnectedRef.current = true;
      setSocketConnected(true);
      emptyPollCountRef.current = 0; // Reset empty poll counter on reconnect
      syncSurfaceState();

      const now = Date.now();
      if (reconnectCatchupInFlightRef.current) {
        return;
      }
      if (now - lastReconnectCatchupAtRef.current < RECONNECT_CATCHUP_MIN_MS) {
        return;
      }
      lastReconnectCatchupAtRef.current = now;
      reconnectCatchupInFlightRef.current = true;
      catchingUpRef.current = true;

      const reconnectResyncId = useResyncStore.getState().resyncId;
      resyncIdRef.current = reconnectResyncId;
      fetchPage({
        silent: true,
        keepUnread: !isViewingLatestRef.current,
        resyncId: reconnectResyncId,
      }).finally(() => {
        reconnectCatchupInFlightRef.current = false;
        syncSurfaceState();
      });
    };
    const handleDisconnect = (reason: string) => {
      console.warn(`[trades] Socket.IO disconnected: ${reason}`);
      socketConnectedRef.current = false;
      setSocketConnected(false);
      catchingUpRef.current = true;
      syncSurfaceState();
    };

    socket.on('connect', handleConnect);
    socket.on('disconnect', handleDisconnect);

    // Set initial state based on socket.connected
    socketConnectedRef.current = socket.connected;
    setSocketConnected(socket.connected);
    if (!socket.connected) {
      catchingUpRef.current = true;
    }
    syncSurfaceState();

    return () => {
      socket.off('connect', handleConnect);
      socket.off('disconnect', handleDisconnect);
    };
  }, [fetchPage, syncSurfaceState]);

  useEffect(() => {
    isActiveRef.current = true;
    return () => {
      isActiveRef.current = false;
    };
  }, []);

  const clearUnreadAndRefresh = useCallback(() => {
    setUnread(0);
    setPage(1);
    pageRef.current = 1;
    isAtTopRef.current = true;
    recomputeIsViewingLatest(true);
    scrollElementRef.current?.scrollTo({ top: 0 });
    fetchPage();
  }, [fetchPage, recomputeIsViewingLatest]);

  const handleToggleSound = useCallback(() => {
    const newMuted = !soundMuted;
    setSoundMutedState(newMuted);
    setSoundMuted(newMuted);
  }, [soundMuted]);

  const soundToggle = useMemo(() => (
    <Button
      variant="secondary"
      size="xs"
      onClick={handleToggleSound}
      title={
        soundMuted
          ? 'Trade sounds muted (click to enable)'
          : 'Trade sounds enabled (click to mute)'
      }
      style={{ fontSize: typography.fontSize['2xs'] }}
    >
      {soundMuted ? '🔇' : '🔊'}
    </Button>
  ), [handleToggleSound, soundMuted]);

  const unreadBadge = useMemo(() => {
    if (unread <= 0) {
      return null;
    }
    return (
      <Button
        variant="secondary"
        size="xs"
        onClick={clearUnreadAndRefresh}
        title="New trades arrived. Jump to latest."
        style={{
          marginLeft: spacing.gap.xs,
          fontSize: typography.fontSize['2xs'],
        }}
      >
        {unread} new
      </Button>
    );
  }, [clearUnreadAndRefresh, unread]);

  const surfaceStatusMeta = useMemo(() => {
    switch (surfaceState) {
      case RealtimeSurfaceState.LIVE:
        return {
          color: colors.semantic.success.DEFAULT,
          title: 'LIVE',
          label: 'LIVE',
          bannerLabel: null,
          bannerBg: undefined,
        };
      case RealtimeSurfaceState.LAGGING:
        return {
          color: colors.semantic.warning.DEFAULT,
          title: 'Lagging',
          label: 'LAGGING',
          bannerLabel: 'LAGGING - Replaying recent deltas…',
          bannerBg: colors.semantic.warning.bg,
        };
      case RealtimeSurfaceState.STALE:
        return {
          color: colors.semantic.danger.DEFAULT,
          title: 'Stale data',
          label: 'STALE',
          bannerLabel: 'STALE - Recovering from replay…',
          bannerBg: colors.semantic.danger.bg,
        };
      case RealtimeSurfaceState.RECOVERING:
        return {
          color: colors.semantic.warning.DEFAULT,
          title: socketConnected ? 'Recovering' : 'Offline',
          label: socketConnected ? 'RECOVERING' : 'OFFLINE',
          bannerLabel: socketConnected ? 'RECOVERING - Replaying…' : 'OFFLINE - Reconnecting…',
          bannerBg: socketConnected ? colors.semantic.warning.bg : colors.semantic.danger.bg,
        };
      default:
        return {
          color: colors.text.muted,
          title: 'Syncing',
          label: 'SYNCING',
          bannerLabel: null,
          bannerBg: undefined,
        };
    }
  }, [socketConnected, surfaceState]);

  const headerActions = useMemo(
    () => {
      const handleExport = () => {
        // Map current visible rows to flat export objects
        const data = (rowsToRender || []).map((r) => ({
          time: r.time || '',
          coin: r.coin || '',
          market_type: (r as any).product_type || (r as any).market_type || '',
          display_name_short: (r as any).display_name_short || '',
          instrument_id: (r as any).instrument_id || '',
          exchange: r.exchange || '',
          side: r.side || '',
          price: r.price ?? '',
          qty: r.qty ?? '',
          notional: r.mv ?? '',
          fee: r.fee ?? '',
          row_id: r.row_id,
          trade_id: r.trade_id || '',
          order_id: r.order_id || '',
          signal_id: r.signal_id || '',
          strategy_id: r.strategy_id || '',
          decision: (r as any).decision || '',
        }));
        const filename = generateTimestampFilename('trades', 'csv');
        exportCSV(data, filename);
      };
      const perfHarnessTrigger = DEV_TRADES_PERF_HARNESS && !perfHarnessActive ? (
        <Button
          variant="secondary"
          size="xs"
          onClick={() => setPerfHarnessActive(true)}
          title="Launch perf harness"
        >
          Perf harness
        </Button>
      ) : null;

      return (
        <div className="flex items-center" style={{ gap: spacing.gap.xs }}>
          <span
            title={surfaceStatusMeta.title}
            style={{ display: 'inline-flex', alignItems: 'center', gap: 6, color: colors.text.muted, fontSize: typography.fontSize.xs }}
          >
            <span
              aria-label="live-status"
              style={{
                width: 8,
                height: 8,
                borderRadius: 9999,
                backgroundColor: surfaceStatusMeta.color,
                display: 'inline-block',
              }}
            />
            {surfaceStatusMeta.label}
          </span>
          {isResyncing ? (
            <span
              title={`Resync ${resyncId} in progress`}
              style={{ color: colors.semantic.warning.DEFAULT, fontSize: typography.fontSize.xs }}
            >
              RESYNCING #{resyncId}
            </span>
          ) : null}
          {loading ? (
            <span style={{ color: colors.text.muted, fontSize: typography.fontSize.xs }}>Loading…</span>
          ) : null}
          {unreadBadge}
          <Button variant="secondary" size="xs" onClick={handleExport} title="Export current view as CSV">
            Export CSV
          </Button>
          {perfHarnessTrigger}
        </div>
      );
    },
    [loading, unreadBadge, rowsToRender, perfHarnessActive, isResyncing, resyncId, surfaceStatusMeta]
  );

  const panelHeaderSlots = usePanelHeaderSlots();

  useEffect(() => {
    if (!panelHeaderSlots) {
      return;
    }

    if (showHeader) {
      panelHeaderSlots.setTitleActions(null);
      panelHeaderSlots.setActions(null);
      return;
    }

    panelHeaderSlots.setTitleActions(soundToggle);
    panelHeaderSlots.setActions(headerActions);

    return () => {
      panelHeaderSlots.setTitleActions(null);
      panelHeaderSlots.setActions(null);
    };
  }, [panelHeaderSlots, showHeader, soundToggle, headerActions]);

  return (
    <div
      className={`flex flex-col h-full overflow-hidden ${className}`}
      style={{ backgroundColor: colors.bg.base }}
    >
      {showHeader && (
        <PanelHeader
          title="Trades"
          onRefresh={clearUnreadAndRefresh}
          refreshing={loading}
          lastUpdate={lastUpdate}
          staleThresholdMs={STALE_THRESHOLDS.REALTIME}
          titleActions={soundToggle}
          actions={headerActions}
          onRemove={onRemove}
        />
      )}

      {/* Live status banner (Phase 1): show OFFLINE or STALE prominently */}
      {(() => {
        if (!surfaceStatusMeta.bannerLabel || !surfaceStatusMeta.bannerBg) return null;
        return (
          <div
            className="w-full"
            style={{
              backgroundColor: surfaceStatusMeta.bannerBg,
              color: colors.text.secondary,
              padding: `${spacing.gap.xs} ${spacing.gap.md}`,
            }}
            role="status"
            aria-live="polite"
          >
            {surfaceStatusMeta.bannerLabel}
          </div>
        );
      })()}

      {/* When embedded in dashboard (showHeader=false), render actions as toolbar */}
      {!showHeader && !panelHeaderSlots && (
        <div
          className="flex items-center justify-end"
          style={{
            padding: `${spacing.gap.sm} ${spacing.gap.md}`,
            backgroundColor: colors.bg.surface,
            borderBottom: `1px solid ${colors.border.DEFAULT}`,
            gap: spacing.gap.sm,
          }}
        >
          {headerActions}
          {soundToggle}
        </div>
      )}

      <div className="flex-1 flex flex-col overflow-hidden">
        <TableFilter
          columns={TRADE_FILTERS}
          onFilterChange={handleFilterChange}
          value={filters}
          dense={dense}
        />

        <div
          className="flex items-center justify-between"
          style={{
            padding: `${spacing.gap.sm} ${spacing.gap.md}`,
            borderBottom: `1px solid ${colors.border.DEFAULT}`,
          }}
        >
          <PageSizeControl value={pageSize} onChange={setPageSize} />
          <span style={{ color: colors.text.muted, fontSize: typography.fontSize.sm }}>
            Loaded {rowsToRender.length.toLocaleString()} of {total.toLocaleString()}
          </span>
        </div>

        <div className="flex-1 min-h-0">
          <TradesTable
            trades={rowsToRender}
            liveDataVersion={controllerState.dataVersion}
            sortDirection={sort}
            onTimeSortChange={handleTimeSortChange}
            onScrollStateChange={handleScrollStateChange}
            enableDecisionDetails={decisionDetailsEnabled}
          />
        </div>

        <div
          className="flex items-center justify-between"
          style={{
            padding: `${spacing.gap.sm} ${spacing.gap.md}`,
            borderTop: `1px solid ${colors.border.DEFAULT}`,
            gap: spacing.gap.sm,
          }}
        >
          <div className="flex items-center" style={{ gap: spacing.gap.sm }}>
            <Button variant="secondary" size="sm" disabled={!canPrev} onClick={goPrev}>
              ‹ Prev
            </Button>
            <span style={{ color: colors.text.muted, fontSize: typography.fontSize.sm }}>
              {showUnboundedPageIndicator
                ? `Page ${page}`
                : `Page ${Math.min(page, totalPages)} of ${totalPages}`}
            </span>
            <Button variant="secondary" size="sm" disabled={!canNext} onClick={goNext}>
              Next ›
            </Button>
          </div>
          <div className="flex items-center" style={{ gap: spacing.gap.md }}>
            {/* Footer rollups: sums over current view */}
            {(() => {
              const fmt = (v?: number | null) => {
                if (v == null || Number.isNaN(v)) return '0';
                const val = Number(v);
                if (!Number.isFinite(val)) return '0';
                return val.toFixed(val >= 100 ? 2 : 6);
              };
              const view = rowsToRender || [];
              const { qty: q, notional, fee: fees } = computeTradesRollups(view);
              return (
                <span style={{ color: colors.text.muted, fontSize: typography.fontSize.sm }}>
                  Σ qty: {fmt(q)} • Σ notional: {fmt(notional)} • Σ fee: {fmt(fees)}
                </span>
              );
            })()}
            {!isViewingLatest && (
              <Button variant="secondary" size="sm" onClick={clearUnreadAndRefresh}>
                Jump to latest
              </Button>
            )}
          </div>
        </div>
      </div>
    </div>
  );
}

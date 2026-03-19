// Zustand stores for client state management

import { createWithEqualityFn as create } from 'zustand/traditional';
import { persist } from 'zustand/middleware';
import type {
  Trade,
  TradeEvent,
  TradeRow,
  FvSnapshot,
  FxDashboard,
  SignalStrategy,
  BalanceParentRow,
  BalanceChildRow,
  BalancesTotals,
  BalancesPayload,
  Alert,
  RiskGroup,
} from './types';
import type { ParamsProfileId } from './config/paramsProfiles';
import { STORE_LIMITS, INTERVALS } from './constants';
import { mergeSignalLegMaps } from './utils/signalLegs';

const PERF_METRICS_ENABLED = typeof import.meta !== 'undefined'
  && Boolean(import.meta.env?.DEV)
  && typeof performance !== 'undefined';

const noop = () => {};
const markPerf: (name: string) => void = PERF_METRICS_ENABLED
  ? (name: string) => {
      try {
        performance.mark(name);
      } catch {
        // ignore mark errors in unsupported environments
      }
    }
  : noop;

const measurePerf: (name: string, start: string) => void = PERF_METRICS_ENABLED
  ? (name: string, start: string) => {
      try {
        performance.measure(name, start);
        performance.clearMarks(start);
      } catch {
        // ignore measure errors in unsupported environments
      }
    }
  : noop;

/**
 * Shallow equality comparator for preventing unnecessary re-renders
 * when selecting arrays or objects from Zustand stores.
 *
 * Usage:
 * ```typescript
 * import { shallow } from 'zustand/shallow';
 * const rows = useTradesStore(state => state.rows, shallow);
 * ```
 */
export { shallow } from 'zustand/shallow';

export type FluxboardSuite = 'all' | 'dex_arb' | 'equities';

const normalizeFluxboardSuite = (value: unknown): FluxboardSuite => {
  const normalized = String(value || '').trim().toLowerCase();
  if (normalized === 'equities') return 'equities';
  if (normalized === 'all') return 'all';
  return 'dex_arb';
};

type SuiteStore = {
  suite: FluxboardSuite;
  setSuite: (suite: FluxboardSuite) => void;
};

export const useSuiteStore = create<SuiteStore>((set) => ({
  suite: 'all',
  setSuite: (suite) => set({ suite: normalizeFluxboardSuite(suite) }),
}));

function coerceNumber(value: unknown): number | undefined {
  if (value === null || value === undefined) return undefined;
  if (typeof value === 'number') {
    return Number.isFinite(value) ? value : undefined;
  }
  if (typeof value === 'string') {
    const trimmed = value.trim();
    if (trimmed.length === 0) return undefined;
    const num = Number(trimmed);
    return Number.isFinite(num) ? num : undefined;
  }
  return undefined;
}

function coerceResyncId(value: unknown): number | undefined {
  const num = coerceNumber(value);
  if (num === undefined || num < 0) return undefined;
  return Math.trunc(num);
}

function resolveResyncIdFromEvents(events: TradeEvent[] | undefined): number | undefined {
  if (!events?.length) return undefined;
  let maxResyncId: number | undefined;
  for (const event of events) {
    const fromSnake = coerceResyncId((event as any)?.resync_id);
    const fromCamel = coerceResyncId((event as any)?.resyncId);
    const candidate = fromSnake ?? fromCamel;
    if (candidate === undefined) continue;
    if (maxResyncId === undefined || candidate > maxResyncId) {
      maxResyncId = candidate;
    }
  }
  return maxResyncId;
}

function resolveIncomingResyncId(explicitResyncId: unknown, events: TradeEvent[] | undefined): number | undefined {
  const explicit = coerceResyncId(explicitResyncId);
  if (explicit !== undefined) return explicit;
  return resolveResyncIdFromEvents(events);
}

function normalizeTradeSide(value: unknown): string {
  const text = String(value ?? '').trim().toLowerCase();
  if (text === '1' || text === 'buy' || text === 'bid') return 'buy';
  if (text === '2' || text === 'sell' || text === 'ask') return 'sell';
  return text;
}

function coerceTradeTsMs(value: unknown): number | undefined {
  const ts = coerceNumber(value);
  if (ts === undefined || ts <= 0) return undefined;
  if (ts < 1e12) return Math.trunc(ts * 1000);
  if (ts >= 1e18) return Math.trunc(ts / 1e6);
  if (ts >= 1e15) return Math.trunc(ts / 1e3);
  return Math.trunc(ts);
}

function coerceOptionalText(value: unknown): string | undefined {
  const text = String(value ?? '').trim();
  return text || undefined;
}

function deriveCoinFromSymbol(symbolText: string): string | undefined {
  const symbol = symbolText.trim().toUpperCase();
  if (!symbol) return undefined;
  const baseSymbolFromVenue = symbol.split('.')[0] || symbol;
  const baseSymbolFromSlash = baseSymbolFromVenue.split('/')[0] || baseSymbolFromVenue;
  const baseSymbol = baseSymbolFromSlash.split('-')[0] || baseSymbolFromSlash;
  for (const quote of ['USDT', 'USDC', 'USD', 'PERP']) {
    if (baseSymbol.endsWith(quote) && baseSymbol.length > quote.length) {
      return baseSymbol.slice(0, -quote.length);
    }
  }
  return baseSymbol || undefined;
}

function deriveExchangeFromInstrument(instrumentId: string): string | undefined {
  const suffix = instrumentId.split('.').pop()?.trim().toLowerCase();
  return suffix || undefined;
}

function normalizeTrade(event: TradeEvent): TradeRow | null {
  if (!event || typeof event !== 'object') return null;
  if (event.op !== 'upsert') return null;
  if (!event.row_id) return null;

  const seq = coerceNumber((event as any).seq);
  if (seq === undefined) return null;

  const versionCandidate = coerceNumber((event as any).version);
  const version =
    typeof versionCandidate === 'number' && Number.isFinite(versionCandidate) && versionCandidate > 0
      ? Math.trunc(versionCandidate)
      : 1;

  const price = coerceNumber(event.price as unknown);
  const qty = coerceNumber(event.qty as unknown);
  const derivedMv = price !== undefined && qty !== undefined ? price * qty : undefined;
  const rawMv = coerceNumber((event as any).notional ?? (event as any).mv);
  const mv =
    (rawMv === undefined || rawMv === 0) && derivedMv !== undefined && derivedMv !== 0
      ? derivedMv
      : rawMv;
  const fee = coerceNumber(event.fee as unknown);
  const fee_quote = coerceNumber((event as any).fee_quote);
  const gas_units = coerceNumber(
    (event as any).gas_units ?? (event as any).gas_used ?? (event as any).gas,
  );

  const instrumentId = String((event as any).instrument_id ?? '').trim();
  const symbolText = String((event as any).symbol ?? (instrumentId ? instrumentId.split('.')[0] : '') ?? '').trim();
  const coinText = String((event as any).coin ?? (event as any).asset ?? (event as any).base_currency ?? '').trim();
  const coin = coinText || deriveCoinFromSymbol(symbolText) || symbolText || '';
  const exchangeText = String((event as any).exchange ?? (event as any).venue ?? '').trim();
  const exchange = (exchangeText || deriveExchangeFromInstrument(instrumentId) || '').toLowerCase();
  const side = normalizeTradeSide((event as any).side);

  const tsMs =
    coerceTradeTsMs((event as any).ts_ms) ??
    coerceTradeTsMs((event as any).ts_event) ??
    coerceTradeTsMs((event as any).ts) ??
    coerceTradeTsMs((event as any).created_ts_ms);
  const ts =
    tsMs ??
    coerceNumber((event as any).ts_ms) ??
    coerceNumber((event as any).ts) ??
    coerceNumber((event as any).created_ts_ms) ??
    coerceNumber((event as any).seq) ??
    Date.now();
  const timeText = String((event as any).time ?? '').trim();
  const time = timeText || (tsMs !== undefined ? new Date(tsMs).toISOString() : '');

  const trade: TradeRow = {
    time,
    coin,
    exchange,
    venue: coerceOptionalText((event as any).venue),
    symbol: coerceOptionalText((event as any).symbol),
    instrument_uid: coerceOptionalText((event as any).instrument_uid),
    instrument_id: instrumentId || undefined,
    venue_root: coerceOptionalText((event as any).venue_root),
    product_type: coerceOptionalText((event as any).product_type),
    market_type: coerceOptionalText((event as any).market_type),
    contract_type: coerceOptionalText((event as any).contract_type),
    raw_symbol: coerceOptionalText((event as any).raw_symbol),
    base_asset: coerceOptionalText((event as any).base_asset),
    quote_asset: coerceOptionalText((event as any).quote_asset),
    pair: coerceOptionalText((event as any).pair),
    inventory_asset: coerceOptionalText((event as any).inventory_asset),
    display_name_short: coerceOptionalText((event as any).display_name_short),
    display_name_long: coerceOptionalText((event as any).display_name_long),
    side,
    price: price ?? null,
    qty: qty ?? null,
    mv: mv ?? null,
    fee: fee ?? null,
    fee_asset_raw: (event as any).fee_asset_raw ?? (event as any).fee_currency ?? null,
    fee_amount_raw: (event as any).fee_amount_raw ?? (event as any).fee_cost ?? null,
    fee_quote: fee_quote ?? null,
    // TX hash / exchange fill id: accept multiple shapes
    // - Snapshot API uses `exch_id`
    // - Delta/raw blotter rows may have `exchange_trade_id` or legacy `id`
    // - Some DEX producers include `tx_hash`/`hash`
    // - Socket legacy mapper may set `exec_id`
    exch_id:
      (event as any).exch_id ??
      (event as any).exec_id ??
      (event as any).exchange_trade_id ??
      (event as any).id ??
      (event as any).tx_hash ??
      (event as any).hash ??
      '',
    trade_id: (event as any).trade_id ?? '',
    signal_id: (event as any).signal_id ?? '',
    strategy_id: (event as any).strategy_id,
    order_id: (event as any).exchange_order_id ?? (event as any).order_id ?? '',
    decision: (event as any).decision,
    decision_timestamp: (event as any).decision_timestamp,
    gas_used: (event as any).gas_used ?? (event as any).gas,
    gas_units: gas_units ?? undefined,
    notes: (event as any).notes,
    explorer_url: (event as any).explorer_url,
    placeholder: Boolean(event.placeholder),
    row_id: event.row_id,
    version,
    seq,
    ts,
  };

  return trade;
}

function sortTrades(rows: Iterable<TradeRow>): TradeRow[] {
  return Array.from(rows).sort((a, b) => {
    if (a.ts === b.ts) {
      return b.seq - a.seq;
    }
    return b.ts - a.ts;
  });
}

const getRowTimestamp = (row: TradeRow): number => {
  if (typeof row.ts === 'number' && Number.isFinite(row.ts)) {
    return row.ts;
  }
  if (typeof row.seq === 'number' && Number.isFinite(row.seq)) {
    return row.seq;
  }
  return 0;
};

// Order trades by newest timestamp, then seq, then row_id for deterministic tie-breaks
const compareRowsDesc = (a: TradeRow, b: TradeRow): number => {
  const tsDiff = getRowTimestamp(a) - getRowTimestamp(b);
  if (tsDiff !== 0) {
    return tsDiff > 0 ? -1 : 1;
  }
  if (a.seq !== b.seq) {
    return a.seq > b.seq ? -1 : 1;
  }
  return a.row_id.localeCompare(b.row_id);
};

const findInsertIndex = (order: string[], byId: Map<string, TradeRow>, row: TradeRow): number => {
  let lo = 0;
  let hi = order.length;
  while (lo < hi) {
    const mid = (lo + hi) >>> 1;
    const midRow = byId.get(order[mid]);
    if (!midRow) {
      hi = mid;
      continue;
    }
    const cmp = compareRowsDesc(row, midRow);
    if (cmp < 0) {
      hi = mid;
    } else {
      lo = mid + 1;
    }
  }
  return lo;
};

const buildStateFromRowMap = (source: Map<string, TradeRow>, limit: number) => {
  const sorted = sortTrades(source.values());
  const trimmed = sorted.slice(0, limit);
  const byId = new Map<string, TradeRow>();
  const order: string[] = [];
  for (const row of trimmed) {
    byId.set(row.row_id, row);
    order.push(row.row_id);
  }
  return { rows: trimmed, byId, order };
};

const rebuildRowsFromOrder = (order: string[], byId: Map<string, TradeRow>): TradeRow[] => {
  const rows: TradeRow[] = [];
  for (const rowId of order) {
    const row = byId.get(rowId);
    if (row) {
      rows.push(row);
    }
  }
  return rows;
};

const removeRowFromOrder = (order: string[], rowId: string): boolean => {
  const idx = order.indexOf(rowId);
  if (idx >= 0) {
    order.splice(idx, 1);
    return true;
  }
  return false;
};

const upsertRowInState = (
  row: TradeRow,
  byId: Map<string, TradeRow>,
  order: string[],
  limit: number,
): boolean => {
  const existing = byId.get(row.row_id);
  if (!existing) {
    const insertIdx = findInsertIndex(order, byId, row);
    order.splice(insertIdx, 0, row.row_id);
    byId.set(row.row_id, row);
    if (order.length > limit) {
      const removed = order.splice(limit);
      removed.forEach((id) => byId.delete(id));
    }
    return true;
  }

  if (row.version <= existing.version) {
    return false;
  }

  byId.set(row.row_id, row);
  if (compareRowsDesc(row, existing) !== 0) {
    removeRowFromOrder(order, row.row_id);
    const insertIdx = findInsertIndex(order, byId, row);
    order.splice(insertIdx, 0, row.row_id);
  }
  if (order.length > limit) {
    const removed = order.splice(limit);
    removed.forEach((id) => byId.delete(id));
  }
  return true;
};

const deleteRowInState = (rowId: string, byId: Map<string, TradeRow>, order: string[]): boolean => {
  if (!byId.has(rowId)) {
    return false;
  }
  byId.delete(rowId);
  removeRowFromOrder(order, rowId);
  return true;
};

type ResyncStore = {
  resyncId: number;
  isResyncing: boolean;
  lastReason?: string;
  lastBumpAt?: number;
  appliedBy: Record<string, number>;
  bumpResync: (reason: string) => number;
  markResyncApplied: (consumer: string, resyncId: number) => void;
  resetResyncState: () => void;
};

export const RESYNC_ACK_CONSUMERS = ['trades', 'order-view'] as const;

const hasCurrentResyncAckFromAllConsumers = (
  resyncId: number,
  appliedBy: Record<string, number>,
): boolean => {
  if (resyncId <= 0) {
    return false;
  }
  return RESYNC_ACK_CONSUMERS.every((consumer) => (appliedBy[consumer] ?? 0) >= resyncId);
};

export const useResyncStore = create<ResyncStore>((set, get) => ({
  resyncId: 0,
  isResyncing: false,
  lastReason: undefined,
  lastBumpAt: undefined,
  appliedBy: {},

  bumpResync: (reason) => {
    const nextResyncId = get().resyncId + 1;
    set({
      resyncId: nextResyncId,
      isResyncing: true,
      lastReason: reason,
      lastBumpAt: Date.now(),
    });
    return nextResyncId;
  },

  markResyncApplied: (consumer, resyncId) =>
    set((state) => {
      const normalizedConsumer = typeof consumer === 'string' ? consumer.trim() : '';
      const normalizedResyncId = coerceResyncId(resyncId);
      if (!normalizedConsumer || normalizedResyncId === undefined) {
        return state;
      }

      const currentApplied = state.appliedBy[normalizedConsumer] ?? 0;
      if (normalizedResyncId <= currentApplied) {
        return state;
      }

      const appliedBy = { ...state.appliedBy, [normalizedConsumer]: normalizedResyncId };
      const isResyncComplete = hasCurrentResyncAckFromAllConsumers(state.resyncId, appliedBy);

      return {
        appliedBy,
        isResyncing: isResyncComplete ? false : state.isResyncing,
      };
    }),

  resetResyncState: () =>
    set({
      resyncId: 0,
      isResyncing: false,
      lastReason: undefined,
      lastBumpAt: undefined,
      appliedBy: {},
    }),
}));

export const selectResyncId = (state: ResyncStore) => state.resyncId;
export const selectResyncing = (state: ResyncStore) => state.isResyncing;
export const selectResyncLastReason = (state: ResyncStore) => state.lastReason;
export const selectResyncLastBumpAt = (state: ResyncStore) => state.lastBumpAt;
export const selectResyncAppliedBy = (state: ResyncStore) => state.appliedBy;

export const bumpGlobalResync = (reason: string): number =>
  useResyncStore.getState().bumpResync(reason);

export const markGlobalResyncApplied = (consumer: string, resyncId: number): void => {
  useResyncStore.getState().markResyncApplied(consumer, resyncId);
};

// Trades store with event-based reducer
const getTradesResyncFloor = (appliedResyncId: number): number => {
  const currentGlobalResyncId = coerceResyncId(useResyncStore.getState().resyncId) ?? 0;
  return Math.max(appliedResyncId, currentGlobalResyncId);
};

const isStaleIncomingResync = (incomingResyncId: number | undefined, floorResyncId: number): boolean =>
  incomingResyncId !== undefined && incomingResyncId < floorResyncId;

export type ApplySnapshotResult = {
  accepted: boolean;
  applied: boolean;
  staleRejected: boolean;
};

export type ApplyDeltaStats = {
  upserts: number;
  deletes: number;
  changed: boolean;
  newRows: number;
  staleRejected: number;
  accepted: boolean;
  applied: boolean;
};

type TradesStore = {
  rows: TradeRow[];
  byId: Map<string, TradeRow>;
  order: string[];
  lastSeq: number;
  appliedResyncId: number;
  lastUpdate?: number;  // Unix timestamp in milliseconds
  lastDataTs?: number;  // Last timestamp when row data changed
  lastReceiveTs?: number;  // Last timestamp when a payload was received
  setSnapshot: (events: TradeEvent[], limit?: number, resyncId?: number) => ApplySnapshotResult;
  applyDelta: (events: TradeEvent[], limit?: number, resyncId?: number) => ApplyDeltaStats;
  appendHistorical: (events: TradeEvent[], limit?: number, resyncId?: number) => void;
  clear: () => void;
};

export const useTradesStore = create<TradesStore>((set) => ({
  rows: [],
  byId: new Map(),
  order: [],
  lastSeq: 0,
  appliedResyncId: 0,
  lastUpdate: undefined,
  lastDataTs: undefined,
  lastReceiveTs: undefined,

  clear: () => set({
    rows: [],
    byId: new Map(),
    order: [],
    lastSeq: 0,
    appliedResyncId: 0,
    lastUpdate: Date.now(),
    lastDataTs: undefined,
    lastReceiveTs: undefined,
  }),

  setSnapshot: (events, limit, resyncId) => {
    const result: ApplySnapshotResult = {
      accepted: false,
      applied: false,
      staleRejected: false,
    };
    set((state) => {
      const nextTs = Date.now();
      const cap = Math.min(limit ?? STORE_LIMITS.TRADES, STORE_LIMITS.TRADES);
      const candidates = new Map<string, TradeRow>();
      let maxSeq = 0;
      const incomingResyncId = resolveIncomingResyncId(resyncId, events);
      const floorResyncId = getTradesResyncFloor(state.appliedResyncId);
      if (isStaleIncomingResync(incomingResyncId, floorResyncId)) {
        result.staleRejected = true;
        return state;
      }
      const appliedResyncId =
        incomingResyncId !== undefined
          ? Math.max(state.appliedResyncId, incomingResyncId)
          : state.appliedResyncId;
      result.accepted = true;
      result.applied = true;

      if (!events?.length) {
        const alreadyEmpty = state.rows.length === 0 && state.byId.size === 0 && state.order.length === 0;
        if (alreadyEmpty) {
          return {
            ...state,
            appliedResyncId,
            lastUpdate: nextTs,
            lastReceiveTs: nextTs,
          };
        }
        return {
          rows: [],
          byId: new Map(),
          order: [],
          lastSeq: state.lastSeq,
          appliedResyncId,
          lastUpdate: nextTs,
          lastDataTs: nextTs,
          lastReceiveTs: nextTs,
        };
      }

      for (const event of events) {
        if (!event || typeof event !== 'object' || event.op !== 'upsert') {
          continue;
        }
        const row = normalizeTrade(event);
        if (!row) continue;
        const current = candidates.get(row.row_id);
        if (!current || row.version > current.version) {
          candidates.set(row.row_id, row);
        }
        if (row.seq > maxSeq) {
          maxSeq = row.seq;
        }
      }

      const built = buildStateFromRowMap(candidates, cap);

      return {
        rows: built.rows,
        byId: built.byId,
        order: built.order,
        lastSeq: maxSeq,
        appliedResyncId,
        lastUpdate: nextTs,
        lastDataTs: nextTs,
        lastReceiveTs: nextTs,
      };
    });
    return result;
  },

  appendHistorical: (events, limit, resyncId) => set((state) => {
    const incomingResyncId = resolveIncomingResyncId(resyncId, events);
    const floorResyncId = getTradesResyncFloor(state.appliedResyncId);
    if (isStaleIncomingResync(incomingResyncId, floorResyncId)) {
      return state;
    }
    const appliedResyncId =
      incomingResyncId !== undefined
        ? Math.max(state.appliedResyncId, incomingResyncId)
        : state.appliedResyncId;

    if (!events?.length) {
      return state;
    }
    const nextTs = Date.now();

    const cap = Math.min(limit ?? STORE_LIMITS.TRADES, STORE_LIMITS.TRADES);
    const byId = new Map(state.byId);
    const order = state.order.slice();
    let maxSeq = state.lastSeq;
    let changed = false;

    for (const event of events) {
      if (!event || typeof event !== 'object' || event.op !== 'upsert') {
        continue;
      }
      const row = normalizeTrade(event);
      if (!row) continue;
      if (upsertRowInState(row, byId, order, cap)) {
        changed = true;
      }
      if (row.seq > maxSeq) {
        maxSeq = row.seq;
      }
    }

    if (!changed) {
      return {
        ...state,
        appliedResyncId,
        lastReceiveTs: nextTs,
      };
    }

    const rows = rebuildRowsFromOrder(order, byId);

    return {
      rows,
      byId,
      order,
      lastSeq: maxSeq,
      appliedResyncId,
      lastUpdate: nextTs,
      lastDataTs: nextTs,
      lastReceiveTs: nextTs,
    };
  }),

  applyDelta: (events, limit, resyncId) => {
    markPerf('trades.applyDelta:start');
    const stats: ApplyDeltaStats = {
      upserts: 0,
      deletes: 0,
      changed: false,
      newRows: 0,
      staleRejected: 0,
      accepted: false,
      applied: false,
    };
    const nextTs = Date.now();
    set((state) => {
      if (!events?.length) {
        return state;
      }

      const incomingResyncId = resolveIncomingResyncId(resyncId, events);
      const floorResyncId = getTradesResyncFloor(state.appliedResyncId);
      if (isStaleIncomingResync(incomingResyncId, floorResyncId)) {
        stats.staleRejected = events.length;
        return state;
      }
      stats.accepted = true;
      const appliedResyncId =
        incomingResyncId !== undefined
          ? Math.max(state.appliedResyncId, incomingResyncId)
          : state.appliedResyncId;

      const cap = Math.min(limit ?? STORE_LIMITS.TRADES, STORE_LIMITS.TRADES);
      const byId = new Map(state.byId);
      const order = state.order.slice();
      let maxSeq = state.lastSeq;
      let changed = false;

      for (const event of events) {
        if (!event || typeof event !== 'object') {
          continue;
        }

        const evSeq = coerceNumber((event as any).seq);
        if (evSeq === undefined) {
          continue;
        }
        const evOp = (event as any).op as unknown;
        const evRowId = (event as any).row_id as unknown;

        if (evOp === 'delete') {
          if (typeof evRowId === 'string' && deleteRowInState(evRowId, byId, order)) {
            changed = true;
            stats.deletes += 1;
          }
          if (evSeq > maxSeq) {
            maxSeq = evSeq;
          }
          continue;
        }

        if (evOp !== 'upsert') {
          if (evSeq > maxSeq) {
            maxSeq = evSeq;
          }
          continue;
        }

        const row = normalizeTrade(event as any);
        if (!row) {
          if (evSeq > maxSeq) {
            maxSeq = evSeq;
          }
          continue;
        }

        const current = byId.get(row.row_id);
        const isNewerThanLastSeq = row.seq > state.lastSeq;
        const isOlderOrEqual = !isNewerThanLastSeq;

        if (!current) {
          if (upsertRowInState(row, byId, order, cap)) {
            changed = true;
            stats.upserts += 1;
            // Only count as a truly new row (for sound/unread)
            // if its seq advances beyond the last seen seq.
            if (isNewerThanLastSeq) {
              stats.newRows += 1;
            }
            stats.changed = true;
          }
        } else if (row.version > current.version) {
          if (upsertRowInState(row, byId, order, cap)) {
            changed = true;
            stats.upserts += 1;
            stats.changed = true;
          }
        } else if (isOlderOrEqual) {
          // Ignore older duplicate updates
        } else {
          // Same or lower version but newer seq: keep current, only advance maxSeq
        }

        if (row.seq > maxSeq) {
          maxSeq = row.seq;
        }
      }

      const updatedSeq = Math.max(state.lastSeq, maxSeq);

      if (!changed) {
        stats.changed = false;
        stats.applied = false;
        return {
          ...state,
          lastSeq: updatedSeq,
          appliedResyncId,
          lastReceiveTs: nextTs,
        };
      }

      stats.changed = true;
      stats.applied = true;
      const rows = rebuildRowsFromOrder(order, byId);

      return {
        rows,
        byId,
        order,
        lastSeq: updatedSeq,
        appliedResyncId,
        lastUpdate: nextTs,
        lastDataTs: nextTs,
        lastReceiveTs: nextTs,
      };
    });
    measurePerf('trades.applyDelta', 'trades.applyDelta:start');
    return stats;
  },
}));

/**
 * Trades Store Selectors
 *
 * Optimized selector functions to prevent unnecessary re-renders.
 * Use with shallow comparison for array/object returns.
 *
 * @example
 * ```typescript
 * import { useTradesStore, selectTradesRows, selectRecentTrades, shallow } from './stores';
 *
 * // Only re-renders when rows array changes
 * const rows = useTradesStore(selectTradesRows, shallow);
 *
 * // Only re-renders when top 10 trades change
 * const top10 = useTradesStore(state => selectRecentTrades(state, 10), shallow);
 *
 * // Only re-renders when specific strategy's trades change
 * const stratTrades = useTradesStore(state => selectTradesByStrategy(state, 'my_strat'), shallow);
 *
 * // Only re-renders when lastUpdate changes (for panel staleness indicator)
 * const lastUpdate = useTradesStore(selectTradesLastUpdate);
 * ```
 */
export const selectTradesRows = (state: TradesStore) => state.rows;
export const selectTradesLastSeq = (state: TradesStore) => state.lastSeq;
export const selectTradesLastUpdate = (state: TradesStore) => state.lastUpdate;
export const selectTradesFreshnessTs = (state: TradesStore) =>
  state.lastDataTs ?? (Array.isArray(state.rows) && state.rows.length > 0 ? state.lastUpdate : undefined);
export const selectTradeById = (state: TradesStore, rowId: string) => state.byId.get(rowId);
export const selectRecentTrades = (state: TradesStore, limit: number) => state.rows.slice(0, limit);
export const selectTradesByStrategy = (state: TradesStore, signalId: string) =>
  state.rows.filter(r => r.signal_id === signalId);
export const selectTradesByExchange = (state: TradesStore, exchange: string) =>
  state.rows.filter(r => r.exchange === exchange);
export const selectTradesByCoin = (state: TradesStore, coin: string) =>
  state.rows.filter(r => r.coin === coin);
export const selectTradesBySide = (state: TradesStore, side: 'buy' | 'sell') =>
  state.rows.filter(r => r.side === side);

// FV store with symbol selection and realtime updates
type FvStore = {
  loading: boolean;
  error?: string;
  profile: string;
  profiles: string[];
  symbols: string[];
  symbol?: string;
  latest?: FvSnapshot;
  lastFetchMs?: number;
  auto: boolean;
  intervalMs: number;
  backoffMs: number;
  setLoading: (loading: boolean) => void;
  setError: (error?: string) => void;
  setProfile: (profile?: string) => void;
  setProfiles: (profiles: string[]) => void;
  setSymbols: (symbols: string[]) => void;
  setSymbol: (symbol?: string) => void;
  setLatest: (snapshot?: FvSnapshot) => void;
  setLastFetchMs: (ms: number) => void;
  setAuto: (on: boolean) => void;
  setIntervalMs: (ms: number) => void;
  setBackoffMs: (ms: number) => void;
  resetBackoff: () => void;
  increaseBackoff: () => void;
};

export const useFvStore = create<FvStore>((set) => ({
  loading: false,
  error: undefined,
  profile: 'fv1',
  profiles: ['fv1'],
  symbols: [],
  symbol: undefined,
  latest: undefined,
  lastFetchMs: undefined,
  auto: true,
  intervalMs: INTERVALS.FX_DEFAULT,
  backoffMs: INTERVALS.FX_DEFAULT,

  setLoading: (loading) => set({ loading }),
  setError: (error) => set({ error }),
  setProfile: (profile) =>
    set({
      profile: ((profile || 'fv1').trim().toLowerCase() || 'fv1'),
    }),
  setProfiles: (profiles) =>
    set((state) => {
      const normalized = Array.from(
        new Set((profiles || []).map((item) => (item || '').trim().toLowerCase()).filter(Boolean))
      );
      const nextProfiles = normalized.length ? normalized : ['fv1'];
      const nextProfile = nextProfiles.includes(state.profile) ? state.profile : nextProfiles[0];
      return {
        profiles: nextProfiles,
        profile: nextProfile,
      };
    }),
  setSymbols: (symbols) =>
    set((state) => ({
      symbols,
      symbol: state.symbol || symbols[0],
    })),
  setSymbol: (symbol) => set({ symbol }),
  setLatest: (snapshot) => set({ latest: snapshot, lastFetchMs: Date.now(), error: undefined }),
  setLastFetchMs: (ms) => set({ lastFetchMs: ms }),
  setAuto: (on) => set({ auto: on }),
  setIntervalMs: (ms) => set({ intervalMs: Math.max(ms, INTERVALS.FX_MIN) }),
  setBackoffMs: (ms) => set({ backoffMs: ms }),
  resetBackoff: () => set((state) => ({ backoffMs: state.intervalMs })),
  increaseBackoff: () => set((state) => ({ backoffMs: Math.min(state.backoffMs * 2, INTERVALS.FX_BACKOFF_MAX) })),
}));

export const selectFvSymbols = (state: FvStore) => state.symbols;
export const selectFvSymbol = (state: FvStore) => state.symbol;
export const selectFvProfile = (state: FvStore) => state.profile;
export const selectFvProfiles = (state: FvStore) => state.profiles;
export const selectFvLatest = (state: FvStore) => state.latest;
export const selectFvLoading = (state: FvStore) => state.loading;
export const selectFvError = (state: FvStore) => state.error;
export const selectFvLastFetch = (state: FvStore) => state.lastFetchMs;

// FX store with auto-refresh control and backoff
type FxStore = {
  loading: boolean;
  error?: string;
  data?: FxDashboard;
  lastFetchMs?: number;
  auto: boolean;
  intervalMs: number;
  backoffMs: number;
  setLoading: (loading: boolean) => void;
  setError: (error?: string) => void;
  setData: (data: FxDashboard) => void;
  setLastFetchMs: (ms: number) => void;
  setAuto: (on: boolean) => void;
  setIntervalMs: (ms: number) => void;
  setBackoffMs: (ms: number) => void;
  resetBackoff: () => void;
  increaseBackoff: () => void;
};

export const useFxStore = create<FxStore>((set) => ({
  loading: false,
  error: undefined,
  data: undefined,
  lastFetchMs: undefined,
  auto: true,
  intervalMs: INTERVALS.FX_DEFAULT,
  backoffMs: INTERVALS.FX_DEFAULT,

  setLoading: (loading) => set({ loading }),
  setError: (error) => set({ error }),
  setData: (data) => set({ data, lastFetchMs: Date.now(), error: undefined }),
  setLastFetchMs: (ms) => set({ lastFetchMs: ms }),
  setAuto: (on) => set({ auto: on }),
  setIntervalMs: (ms) => set({ intervalMs: Math.max(ms, INTERVALS.FX_MIN) }),
  setBackoffMs: (ms) => set({ backoffMs: ms }),
  resetBackoff: () => set((state) => ({ backoffMs: state.intervalMs })),
  increaseBackoff: () => set((state) => ({ backoffMs: Math.min(state.backoffMs * 2, INTERVALS.FX_BACKOFF_MAX) }))
}));

/**
 * FX Store Selectors
 *
 * Optimized selector functions to prevent unnecessary re-renders.
 *
 * @example
 * ```typescript
 * import { useFxStore, selectFxData, selectFxLoading, selectFxRate } from './stores';
 *
 * // Only re-renders when FX data changes
 * const fxData = useFxStore(selectFxData);
 *
 * // Only re-renders when loading state changes
 * const loading = useFxStore(selectFxLoading);
 *
 * // Only re-renders when specific pair changes
 * const ethPair = useFxStore(state => selectFxRate(state, 'ETH/USDT'));
 * ```
 */
export const selectFxData = (state: FxStore) => state.data;
export const selectFxLoading = (state: FxStore) => state.loading;
export const selectFxError = (state: FxStore) => state.error;
export const selectFxAuto = (state: FxStore) => state.auto;
export const selectFxLastFetch = (state: FxStore) => state.lastFetchMs;
export const selectFxRate = (state: FxStore, pair: string) => {
  // FxDashboard has 'pairs' array with 'pair' property
  return state.data?.pairs?.find(p => p.pair === pair);
};

// Signal store for strategy health monitoring
type SignalStore = {
  rows: SignalStrategy[];
  lastUpdate?: number;  // Unix timestamp in milliseconds
  setRows: (rows: SignalStrategy[]) => void;
  mergeStrategy: (strategy: SignalStrategy) => void;
  mergeStrategies: (strategies: SignalStrategy[]) => void;
};

const normalizeSignalTrade = (trade: SignalStrategy['last_trade']): SignalStrategy['last_trade'] => {
  if (trade === undefined) return undefined as any;
  if (trade === null) return null;
  const notional = coerceNumber((trade as any).notional);
  const realizedBps = coerceNumber((trade as any).realized_bps);
  const price = coerceNumber((trade as any).price);
  const qty = coerceNumber((trade as any).qty);
  return {
    ...trade,
    notional: notional ?? undefined,
    realized_bps: realizedBps ?? undefined,
    price: price ?? undefined,
    qty: qty ?? undefined,
  };
};

const normalizeSignalStrategy = (strategy: SignalStrategy): SignalStrategy => {
  if (!strategy) return strategy;
  const normalizedLastTrade =
    strategy.last_trade === undefined
      ? undefined
      : normalizeSignalTrade(strategy.last_trade ?? null);
  return {
    ...strategy,
    last_trade: normalizedLastTrade,
  };
};

function mergeNestedRecord<T extends Record<string, any> | null | undefined>(
  prev: T,
  next: T,
): T {
  if (next === undefined) return prev;
  if (next === null) return next;
  if (prev === null || prev === undefined) return { ...next } as T;
  return { ...prev, ...next } as T;
}

function mergeArbSignalPayload<T extends { operator?: Record<string, any> | null; quote_snapshot?: Record<string, any> | null } | null | undefined>(
  prev: T,
  next: T,
): T {
  if (next === undefined) return prev;
  if (next === null) return next;
  if (prev === null || prev === undefined) {
    return {
      ...next,
      operator: mergeNestedRecord(undefined, next.operator),
      quote_snapshot: next.quote_snapshot
        ? {
            ...next.quote_snapshot,
            maker_leg: mergeNestedRecord(undefined, next.quote_snapshot.maker_leg as any),
            hedge_leg: mergeNestedRecord(undefined, next.quote_snapshot.hedge_leg as any),
            ref_leg: mergeNestedRecord(undefined, next.quote_snapshot.ref_leg as any),
          }
        : next.quote_snapshot,
    } as T;
  }
  const nextQuoteSnapshot = next.quote_snapshot;
  const prevQuoteSnapshot = prev.quote_snapshot;
  return {
    ...prev,
    ...next,
    operator: mergeNestedRecord(
      prev.operator,
      next.operator
        ? {
            ...next.operator,
            hedge_policy: mergeNestedRecord(prev.operator?.hedge_policy, next.operator.hedge_policy),
            fee_assumptions: mergeNestedRecord(prev.operator?.fee_assumptions, next.operator.fee_assumptions),
            hedge_backlog: mergeNestedRecord(prev.operator?.hedge_backlog, next.operator.hedge_backlog),
          }
        : next.operator,
    ),
    quote_snapshot: mergeNestedRecord(
      prevQuoteSnapshot,
      nextQuoteSnapshot
        ? {
            ...nextQuoteSnapshot,
            maker_leg: mergeNestedRecord(prevQuoteSnapshot?.maker_leg as any, nextQuoteSnapshot.maker_leg as any),
            hedge_leg: mergeNestedRecord(prevQuoteSnapshot?.hedge_leg as any, nextQuoteSnapshot.hedge_leg as any),
            ref_leg: mergeNestedRecord(prevQuoteSnapshot?.ref_leg as any, nextQuoteSnapshot.ref_leg as any),
          }
        : nextQuoteSnapshot,
    ),
  } as T;
}

const mergeSignalStrategyRows = (
  rows: SignalStrategy[],
  newStrategy: SignalStrategy,
): SignalStrategy[] => {
  const idx = rows.findIndex(r => r.id === newStrategy.id);
  const normalizedIncoming = normalizeSignalStrategy(newStrategy);

  // Helper: prefer new value when defined, otherwise keep old
  const keepIfUndefined = <T,>(prev: T | undefined, next: T | undefined): T | undefined =>
    (next !== undefined ? next : prev);

  if (idx >= 0) {
    const prev = rows[idx];
    const normalizedPrevLastTrade = normalizeSignalTrade(prev.last_trade ?? null);

    // Deep merge legs generically by leg key:
    // undefined means no change, null means delete, object means patch.
    const mergedLegs = mergeSignalLegMaps(prev.legs, normalizedIncoming.legs);

    const mergedLastTrade = normalizedIncoming.last_trade === undefined
      ? normalizedPrevLastTrade
      : normalizedIncoming.last_trade;

    const mergedParams =
      (normalizedIncoming as any).params !== undefined
        ? { ...(prev.params ?? {}), ...(((normalizedIncoming as any).params ?? {}) as any) }
        : prev.params;

    // Sticky edge fields: when WS snapshot momentarily omits decision/edge2, keep last known
    const merged: SignalStrategy = {
      ...prev,
      ...normalizedIncoming,
      params: mergedParams as any,
      legs: mergedLegs,
      last_trade: mergedLastTrade,
      decision_edge_bps: keepIfUndefined(prev.decision_edge_bps, normalizedIncoming.decision_edge_bps),
      edge2_bps: keepIfUndefined(prev.edge2_bps, normalizedIncoming.edge2_bps),
      required_edge_bps: keepIfUndefined(prev.required_edge_bps, normalizedIncoming.required_edge_bps),
      edge2_case: keepIfUndefined(prev.edge2_case, normalizedIncoming.edge2_case),
      edge_case_details: keepIfUndefined(prev.edge_case_details as any, normalizedIncoming.edge_case_details as any),
      balance_readiness: keepIfUndefined(prev.balance_readiness, normalizedIncoming.balance_readiness),
      maker_quote_status: keepIfUndefined(prev.maker_quote_status, normalizedIncoming.maker_quote_status),
      quote_stacks: keepIfUndefined((prev as any).quote_stacks, (normalizedIncoming as any).quote_stacks),
      pricing_adjustments: keepIfUndefined(
        prev.pricing_adjustments,
        normalizedIncoming.pricing_adjustments
      ),
      equities_arb: mergeArbSignalPayload(prev.equities_arb, normalizedIncoming.equities_arb),
      maker_v4: mergeArbSignalPayload(prev.maker_v4, normalizedIncoming.maker_v4),
    };

    // Recompute edge2_bps to maintain invariant: edge2 = decision_edge - required_edge
    // This prevents stale edge2_bps when decision_edge_bps changes but edge2_bps was omitted in update
    if (merged.decision_edge_bps !== undefined && merged.required_edge_bps !== undefined) {
      merged.edge2_bps = merged.decision_edge_bps - merged.required_edge_bps;
    }

    const updated = [...rows];
    updated[idx] = merged;
    return updated;
  }

  // Add new strategy
  const incomingParams = (normalizedIncoming as any).params;
  const normalizedParams =
    incomingParams && typeof incomingParams === 'object'
      ? (incomingParams as any)
      : ({ bot_on: '0' } as any);
  return [...rows, { ...normalizedIncoming, params: normalizedParams } as SignalStrategy];
};

export const useSignalStore = create<SignalStore>((set) => ({
  rows: [],
  lastUpdate: undefined,
  setRows: (rows) => set({
    rows: (rows || []).map(normalizeSignalStrategy),
    lastUpdate: Date.now(),
  }),

  // Incremental update: merge single strategy without replacing entire array
  //
  // Merge semantics:
  // - undefined values are ignored (no change)
  // - null values explicitly delete keys
  // - Legs are deep merged to preserve existing properties
  // - Edge fields (decision_edge_bps, edge2_bps) are sticky (preserved if omitted)
  mergeStrategy: (newStrategy) => set((state) => ({
    rows: mergeSignalStrategyRows(state.rows, newStrategy),
    lastUpdate: Date.now(),
  })),

  // Batched update: merge multiple strategies in a single set() call
  // to reduce store writes/renders during large WS updates.
  mergeStrategies: (strategies) => set((state) => {
    if (!Array.isArray(strategies) || strategies.length === 0) {
      return state;
    }

    let mergedRows = state.rows;
    let mergedAny = false;

    for (const strategy of strategies) {
      try {
        mergedRows = mergeSignalStrategyRows(mergedRows, strategy);
        mergedAny = true;
      } catch (err) {
        if (import.meta.env?.DEV) {
          console.error('[signal-store] Failed to merge strategy in batch:', (strategy as any)?.id, err);
        }
      }
    }

    if (!mergedAny) {
      return state;
    }

    return { rows: mergedRows, lastUpdate: Date.now() };
  }),
}));

/**
 * Signal Store Selectors
 *
 * Optimized selector functions to prevent unnecessary re-renders.
 * Use with shallow comparison for array returns.
 *
 * @example
 * ```typescript
 * import { useSignalStore, selectSignalRows, selectActiveStrategies, shallow } from './stores';
 *
 * // Only re-renders when rows array changes
 * const rows = useSignalStore(selectSignalRows, shallow);
 *
 * // Only re-renders when active strategies change
 * const active = useSignalStore(selectActiveStrategies, shallow);
 *
 * // Only re-renders when specific strategy changes
 * const strategy = useSignalStore(state => selectSignalById(state, 'my_strat'));
 *
 * // Only re-renders when lastUpdate changes
 * const lastUpdate = useSignalStore(selectSignalLastUpdate);
 * ```
 */
export const selectSignalRows = (state: SignalStore) => state.rows;
export const selectSignalLastUpdate = (state: SignalStore) => state.lastUpdate;
export const selectSignalById = (state: SignalStore, id: string) =>
  state.rows.find(r => r.id === id);
export const selectActiveStrategies = (state: SignalStore) =>
  state.rows.filter(r => (r.params?.bot_on ?? '0') === '1');
export const selectSignalByEdge = (state: SignalStore, minEdge: number) =>
  state.rows.filter(r => (r.edge2_bps ?? 0) >= minEdge);

// Balances store with loading state
type BalancesStore = {
  rows: BalanceParentRow[];
  totals: BalancesTotals | null;
  totalCount: number;
  generatedAt?: string;
  loading: boolean;
  lastUpdate?: number;  // Unix timestamp in milliseconds
  lastDataTs?: number;  // Last timestamp when balance data changed
  lastReceiveTs?: number;  // Last timestamp when a payload was received
  riskGroups: RiskGroup[];
  riskSort: { column: 'underlying' | 'net_qty' | 'net_mv' | 'long_mv' | 'short_mv' | 'gross_mv' | 'sources'; direction: 'asc' | 'desc' };
  setData: (data: BalancesPayload) => void;
  setLoading: (loading: boolean) => void;
  setRiskSort: (column: BalancesStore['riskSort']['column'], direction: 'asc' | 'desc') => void;
};

function balanceChildMetadataEqual(a: BalanceChildRow, b: BalanceChildRow): boolean {
  return (
    a.coin === b.coin
    && (a.display_name_short ?? null) === (b.display_name_short ?? null)
    && (a.display_name_long ?? null) === (b.display_name_long ?? null)
    && (a.inventory_asset ?? null) === (b.inventory_asset ?? null)
    && (a.base_asset ?? null) === (b.base_asset ?? null)
    && (a.quote_asset ?? null) === (b.quote_asset ?? null)
    && (a.product_type ?? null) === (b.product_type ?? null)
    && (a.market_type ?? null) === (b.market_type ?? null)
    && (a.form ?? null) === (b.form ?? null)
    && (a.chain ?? null) === (b.chain ?? null)
    && (a.contract ?? null) === (b.contract ?? null)
    && (a.venue ?? null) === (b.venue ?? null)
    && (a.wallet ?? null) === (b.wallet ?? null)
    && (a.label ?? null) === (b.label ?? null)
    && (a.address ?? null) === (b.address ?? null)
    && (a.risk_key ?? null) === (b.risk_key ?? null)
    && (a.risk_label ?? null) === (b.risk_label ?? null)
  );
}

function balanceParentMetadataEqual(a: BalanceParentRow, b: BalanceParentRow): boolean {
  return (
    a.coin === b.coin
    && a.canonical === b.canonical
    && a.is_parent === b.is_parent
    && a.stable === b.stable
    && (a.time_display ?? null) === (b.time_display ?? null)
    && (a.time_iso ?? null) === (b.time_iso ?? null)
  );
}

function balanceParentsEqual(prev: BalanceParentRow[], next: BalanceParentRow[]): boolean {
  if (prev.length !== next.length) return false;
  for (let i = 0; i < prev.length; i += 1) {
    const a = prev[i];
    const b = next[i];
    if (!b) return false;
    if (
      a.id !== b.id ||
      a.qty_raw !== b.qty_raw ||
      a.mv_raw !== b.mv_raw ||
      (a.mark_raw ?? null) !== (b.mark_raw ?? null) ||
      (a.last_ts ?? null) !== (b.last_ts ?? null) ||
      !balanceParentMetadataEqual(a, b) ||
      a.children.length !== b.children.length
    ) {
      return false;
    }
    for (let j = 0; j < a.children.length; j += 1) {
      const ca = a.children[j];
      const cb = b.children[j];
      if (
        !cb ||
        ca.id !== cb.id ||
        ca.qty_raw !== cb.qty_raw ||
        ca.mv_raw !== cb.mv_raw ||
        (ca.mark_raw ?? null) !== (cb.mark_raw ?? null) ||
        (ca.last_ts ?? null) !== (cb.last_ts ?? null) ||
        !balanceChildMetadataEqual(ca, cb)
      ) {
        return false;
      }
    }
  }
  return true;
}

function balancesTotalsEqual(prev: BalancesTotals | null, next: BalancesTotals | null): boolean {
  if (prev === next) return true;
  if (!prev || !next) return false;

  return (
    prev.mv_raw === next.mv_raw
    && prev.mv_display === next.mv_display
    && (prev.net_mv_raw ?? null) === (next.net_mv_raw ?? null)
    && (prev.net_mv_display ?? null) === (next.net_mv_display ?? null)
    && (prev.long_mv_raw ?? null) === (next.long_mv_raw ?? null)
    && (prev.long_mv_display ?? null) === (next.long_mv_display ?? null)
    && (prev.short_mv_raw ?? null) === (next.short_mv_raw ?? null)
    && (prev.short_mv_display ?? null) === (next.short_mv_display ?? null)
    && (prev.gross_mv_raw ?? null) === (next.gross_mv_raw ?? null)
    && (prev.gross_mv_display ?? null) === (next.gross_mv_display ?? null)
    && (prev.stable_mv_raw ?? null) === (next.stable_mv_raw ?? null)
    && (prev.stable_mv_display ?? null) === (next.stable_mv_display ?? null)
    && (prev.non_stable_mv_raw ?? null) === (next.non_stable_mv_raw ?? null)
    && (prev.non_stable_mv_display ?? null) === (next.non_stable_mv_display ?? null)
    && (prev.account_equity_raw ?? null) === (next.account_equity_raw ?? null)
    && (prev.account_equity_display ?? null) === (next.account_equity_display ?? null)
    && (prev.withdrawable_raw ?? null) === (next.withdrawable_raw ?? null)
    && (prev.withdrawable_display ?? null) === (next.withdrawable_display ?? null)
  );
}

function riskGroupsEqual(prev: RiskGroup[], next: RiskGroup[]): boolean {
  if (prev === next) return true;
  if (prev.length !== next.length) return false;

  for (let i = 0; i < prev.length; i += 1) {
    const a = prev[i];
    const b = next[i];
    if (!b) return false;
    if (
      a.risk_key !== b.risk_key
      || a.label !== b.label
      || (a.net_qty ?? null) !== (b.net_qty ?? null)
      || (a.net_mv ?? null) !== (b.net_mv ?? null)
      || (a.long_mv ?? null) !== (b.long_mv ?? null)
      || (a.short_mv ?? null) !== (b.short_mv ?? null)
      || (a.gross_mv ?? null) !== (b.gross_mv ?? null)
      || (a.abs_net_mv ?? null) !== (b.abs_net_mv ?? null)
      || (a.hedge_ratio ?? null) !== (b.hedge_ratio ?? null)
    ) {
      return false;
    }

    const aSources = a.sources ?? [];
    const bSources = b.sources ?? [];
    if (aSources.length !== bSources.length) return false;
    for (let j = 0; j < aSources.length; j += 1) {
      if (aSources[j] !== bSources[j]) {
        return false;
      }
    }

    const aRows = a.rows ?? [];
    const bRows = b.rows ?? [];
    if (aRows.length !== bRows.length) return false;
    for (let j = 0; j < aRows.length; j += 1) {
      const ar = aRows[j];
      const br = bRows[j];
      if (
        !br
        || (ar.row_id ?? null) !== (br.row_id ?? null)
        || ar.venue !== br.venue
        || ar.coin !== br.coin
        || ar.qty_raw !== br.qty_raw
        || ar.mv_raw !== br.mv_raw
        || (ar.mark_raw ?? null) !== (br.mark_raw ?? null)
        || (ar.time_display ?? null) !== (br.time_display ?? null)
        || (ar.label ?? null) !== (br.label ?? null)
        || (ar.wallet ?? null) !== (br.wallet ?? null)
        || (ar.address ?? null) !== (br.address ?? null)
      ) {
        return false;
      }
    }
  }

  return true;
}

export const useBalancesStore = create<BalancesStore>((set) => ({
  rows: [],
  totals: null,
  totalCount: 0,
  generatedAt: undefined,
  loading: false,
  lastUpdate: undefined,
  lastDataTs: undefined,
  lastReceiveTs: undefined,
  riskGroups: [],
  riskSort: { column: 'gross_mv', direction: 'desc' },

  setData: (data) =>
    set((state) => {
      const capped = (data.rows ?? []).slice(0, STORE_LIMITS.BALANCES);
      // Guard against out-of-order responses (older snapshots arriving after newer ones)
      const currentTs = state.generatedAt ? Date.parse(state.generatedAt) : 0;
      const incomingTs = data.generated_at ? Date.parse(data.generated_at) : Date.now();
      if (Number.isFinite(currentTs) && Number.isFinite(incomingTs) && incomingTs < currentTs) {
        return state;  // Ignore stale payload
      }

      const noChange = balanceParentsEqual(state.rows, capped);
      const nextGeneratedAt = data.generated_at ?? state.generatedAt;
      const nextTotals = data.totals ?? state.totals;
      const nextTotalCount = typeof data.total === 'number' ? data.total : state.totalCount;
      const nextRiskGroups = data.risk_groups ?? state.riskGroups ?? [];
      const dataChanged =
        !noChange
        || nextGeneratedAt !== state.generatedAt
        || !balancesTotalsEqual(state.totals, nextTotals)
        || nextTotalCount !== state.totalCount
        || !riskGroupsEqual(state.riskGroups, nextRiskGroups);
      const nextLastUpdate = Date.now();
      return {
        rows: noChange ? state.rows : capped,
        totals: nextTotals,
        totalCount: nextTotalCount,
        generatedAt: nextGeneratedAt,
        lastUpdate: nextLastUpdate,
        lastDataTs: dataChanged ? nextLastUpdate : state.lastDataTs,
        lastReceiveTs: nextLastUpdate,
        riskGroups: nextRiskGroups,
      };
    }),

  setLoading: (loading) => set({ loading }),
  setRiskSort: (column, direction) => set({ riskSort: { column, direction } }),
}));

/**
 * Selector for balances last update timestamp
 * Usage: const lastUpdate = useBalancesStore(selectBalancesLastUpdate);
 */
export const selectBalancesLastUpdate = (state: BalancesStore) => state.lastUpdate;
export const selectBalancesFreshnessTs = (state: BalancesStore) => state.lastDataTs ?? state.lastUpdate;

/**
 * Balances Store Selectors
 *
 * Optimized selector functions to prevent unnecessary re-renders.
 * Use with shallow comparison for array returns.
 *
 * @example
 * ```typescript
 * import { useBalancesStore, selectBalancesRows, selectBalancesByExchange, shallow } from './stores';
 *
 * // Only re-renders when rows array changes
 * const rows = useBalancesStore(selectBalancesRows, shallow);
 *
 * // Only re-renders when loading state changes
 * const loading = useBalancesStore(selectBalancesLoading);
 *
 * // Only re-renders when specific exchange's balances change
 * const bybitBalances = useBalancesStore(state => selectBalancesByExchange(state, 'bybit'), shallow);
 *
 * // Only re-renders when specific coin's total balance changes
 * const ethTotal = useBalancesStore(state => selectTotalBalance(state, 'ETH'));
 * ```
 */
export const selectBalancesRows = (state: BalancesStore) => state.rows;
export const selectBalancesLoading = (state: BalancesStore) => state.loading;
export const selectBalancesTotals = (state: BalancesStore) => state.totals;
export const selectBalancesGeneratedAt = (state: BalancesStore) => state.generatedAt;
export const selectBalancesTotalCount = (state: BalancesStore) => state.totalCount;
export const selectBalancesRiskGroups = (state: BalancesStore) => state.riskGroups;
export const selectBalancesRiskSort = (state: BalancesStore) => state.riskSort;
export const selectBalancesByExchange = (
  state: BalancesStore,
  exchange: string,
): BalanceChildRow[] => {
  const match = exchange.trim().toLowerCase();
  if (!match) return [];
  const matches: BalanceChildRow[] = [];
  state.rows.forEach((parent) => {
    parent.children.forEach((child) => {
      if ((child.venue || '').toLowerCase() === match) {
        matches.push(child);
      }
    });
  });
  return matches;
};
export const selectBalancesByCoin = (state: BalancesStore, coin: string) =>
  state.rows.filter((r) => r.canonical === coin || r.coin === coin);
export const selectTotalBalance = (state: BalancesStore, coin: string) =>
  state.rows
    .filter((r) => r.canonical === coin || r.coin === coin)
    .reduce((sum, r) => sum + (r.raw?.qty ?? 0), 0);

// Params store with persisted UI preferences
type ParamsViewMode = 'compact' | 'full';
type ParamsSortDirection = 'asc' | 'desc' | null;

export type ParamsSortState = {
  key: string | null;
  direction: ParamsSortDirection;
};

type ParamsColumnPrefs = {
  order: string[];
  visibility: Record<string, boolean>;
};

type ParamsColumnPrefsByProfile = Record<ParamsProfileId, ParamsColumnPrefs>;

type ParamsFocusedCell = {
  strategyId: string;
  paramKey: string;
} | null;

type ParamsStore = {
  auto: boolean;
  viewMode: ParamsViewMode;
  activeProfile: ParamsProfileId;
  columnPrefsByProfile: ParamsColumnPrefsByProfile;
  // Compatibility mirror for existing selectors/components.
  columnPrefs: ParamsColumnPrefs;
  sortState: ParamsSortState;
  selectedStrategies: string[];
  lastFocusedCell: ParamsFocusedCell;
  lastUpdate?: number;
  setAuto: (on: boolean) => void;
  setViewMode: (mode: ParamsViewMode) => void;
  setActiveProfile: (profile: ParamsProfileId) => void;
  setColumnOrder: (order: string[]) => void;
  setColumnVisibility: (key: string, visible: boolean) => void;
  resetColumnVisibility: () => void;
  setSortState: (state: ParamsSortState) => void;
  clearSort: () => void;
  setSelectedStrategies: (strategyIds: string[]) => void;
  clearSelection: () => void;
  setLastFocusedCell: (cell: ParamsFocusedCell) => void;
  setLastUpdate: (ms?: number) => void;
};

const PARAMS_UI_STORAGE_KEY = 'fluxboard:params:ui:v1';

const createDefaultSortState = (): ParamsSortState => ({
  key: null,
  direction: null
});

const createDefaultColumnPrefs = (): ParamsColumnPrefs => ({
  order: [],
  visibility: {}
});

const normalizeColumnPrefs = (prefs?: Partial<ParamsColumnPrefs> | null): ParamsColumnPrefs => ({
  order: Array.isArray(prefs?.order) ? [...prefs.order] : [],
  visibility: prefs?.visibility && typeof prefs.visibility === 'object'
    ? { ...prefs.visibility }
    : {},
});

const createDefaultColumnPrefsByProfile = (): ParamsColumnPrefsByProfile => ({
  taker: createDefaultColumnPrefs(),
  maker_v2: createDefaultColumnPrefs(),
  maker_v3: createDefaultColumnPrefs(),
  equities_maker: createDefaultColumnPrefs(),
  equities_taker: createDefaultColumnPrefs(),
  maker_v4: createDefaultColumnPrefs(),
});

function normalizeParamsProfileId(value: unknown): ParamsProfileId {
  const normalized = String(value || '').trim().toLowerCase();
  if (normalized === 'maker_v2') return 'maker_v2';
  if (normalized === 'maker_v3') return 'maker_v3';
  if (normalized === 'equities_maker') return 'equities_maker';
  if (normalized === 'equities_taker') return 'equities_taker';
  if (normalized === 'maker_v4') return 'maker_v4';
  return 'taker';
}

const normalizeColumnPrefsByProfile = (
  prefs?: Partial<Record<string, Partial<ParamsColumnPrefs>>> | null
): ParamsColumnPrefsByProfile => {
  const defaults = createDefaultColumnPrefsByProfile();
  const takerExplicit = normalizeColumnPrefs(prefs?.taker ?? defaults.taker);
  const takerLegacy = normalizeColumnPrefs(
    (prefs as Partial<Record<string, Partial<ParamsColumnPrefs>>> | null | undefined)?.taker_task ?? defaults.taker
  );
  return {
    taker:
      takerExplicit.order.length > 0 || Object.keys(takerExplicit.visibility).length > 0
        ? takerExplicit
        : takerLegacy,
    maker_v2: normalizeColumnPrefs(prefs?.maker_v2 ?? defaults.maker_v2),
    maker_v3: normalizeColumnPrefs(prefs?.maker_v3 ?? defaults.maker_v3),
    equities_maker: normalizeColumnPrefs(prefs?.equities_maker ?? defaults.equities_maker),
    equities_taker: normalizeColumnPrefs(prefs?.equities_taker ?? defaults.equities_taker),
    maker_v4: normalizeColumnPrefs(prefs?.maker_v4 ?? defaults.maker_v4),
  };
};

export const useParamsStore = create<ParamsStore>()(
  persist(
    (set) => ({
      auto: true,
      viewMode: 'compact',
      activeProfile: 'taker',
      columnPrefsByProfile: createDefaultColumnPrefsByProfile(),
      columnPrefs: createDefaultColumnPrefsByProfile().taker,
      sortState: createDefaultSortState(),
      selectedStrategies: [],
      lastFocusedCell: null,
      lastUpdate: undefined,

      setAuto: (on) => set({ auto: on }),
      setViewMode: (mode) => set({ viewMode: mode }),
      setActiveProfile: (profile) =>
        set((state) => {
          const prefs = state.columnPrefsByProfile[profile] || createDefaultColumnPrefs();
          return {
            activeProfile: profile,
            columnPrefs: {
              order: [...(prefs.order || [])],
              visibility: { ...(prefs.visibility || {}) },
            },
          };
        }),
      setColumnOrder: (order) =>
        set((state) => ({
          columnPrefsByProfile: {
            ...state.columnPrefsByProfile,
            [state.activeProfile]: {
              ...state.columnPrefsByProfile[state.activeProfile],
              order: Array.isArray(order)
                ? [...order]
                : state.columnPrefsByProfile[state.activeProfile]?.order || [],
            },
          },
          columnPrefs: {
            ...state.columnPrefs,
            order: Array.isArray(order) ? [...order] : state.columnPrefs.order,
          },
        })),
      setColumnVisibility: (key, visible) =>
        set((state) => ({
          columnPrefsByProfile: {
            ...state.columnPrefsByProfile,
            [state.activeProfile]: {
              ...state.columnPrefsByProfile[state.activeProfile],
              visibility: {
                ...(state.columnPrefsByProfile[state.activeProfile]?.visibility || {}),
                [key]: visible,
              },
            },
          },
          columnPrefs: {
            ...state.columnPrefs,
            visibility: {
              ...state.columnPrefs.visibility,
              [key]: visible,
            },
          },
        })),
      resetColumnVisibility: () =>
        set((state) => ({
          columnPrefsByProfile: {
            ...state.columnPrefsByProfile,
            [state.activeProfile]: {
              ...state.columnPrefsByProfile[state.activeProfile],
              visibility: {},
            },
          },
          columnPrefs: {
            ...state.columnPrefs,
            visibility: {},
          },
        })),
      setSortState: (sortState) => set({ sortState }),
      clearSort: () => set({ sortState: createDefaultSortState() }),
      setSelectedStrategies: (strategyIds) =>
        set({ selectedStrategies: Array.from(new Set(strategyIds)) }),
      clearSelection: () => set({ selectedStrategies: [] }),
      setLastFocusedCell: (cell) => set({ lastFocusedCell: cell }),
      setLastUpdate: (ms) => set({ lastUpdate: ms ?? Date.now() })
    }),
    {
      name: PARAMS_UI_STORAGE_KEY,
      version: 4,
      migrate: (persistedState: any, version: number) => {
        if (!persistedState || typeof persistedState !== 'object') {
          return persistedState;
        }
        const legacyColumnPrefs =
          persistedState.columnPrefs && typeof persistedState.columnPrefs === 'object'
            ? {
                order: Array.isArray(persistedState.columnPrefs.order)
                  ? [...persistedState.columnPrefs.order]
                  : [],
                visibility:
                  persistedState.columnPrefs.visibility &&
                  typeof persistedState.columnPrefs.visibility === 'object'
                    ? { ...persistedState.columnPrefs.visibility }
                    : {},
              }
            : createDefaultColumnPrefs();
        const cloneLegacyPrefs = (): ParamsColumnPrefs => ({
          order: [...legacyColumnPrefs.order],
          visibility: { ...legacyColumnPrefs.visibility },
        });
        const normalizedPrefsByProfile = normalizeColumnPrefsByProfile(
          version < 2 ? undefined : persistedState.columnPrefsByProfile,
        );
        if (version < 2) {
          normalizedPrefsByProfile.taker = cloneLegacyPrefs();
          normalizedPrefsByProfile.maker_v2 = cloneLegacyPrefs();
          normalizedPrefsByProfile.maker_v3 = cloneLegacyPrefs();
          normalizedPrefsByProfile.equities_maker = cloneLegacyPrefs();
          normalizedPrefsByProfile.equities_taker = cloneLegacyPrefs();
          normalizedPrefsByProfile.maker_v4 = cloneLegacyPrefs();
        }
        if (version < 4) {
          const legacyMakerV4Prefs = normalizeColumnPrefs(normalizedPrefsByProfile.maker_v4);
          const hasEquitiesMakerPrefs =
            normalizedPrefsByProfile.equities_maker.order.length > 0
            || Object.keys(normalizedPrefsByProfile.equities_maker.visibility).length > 0;
          const hasEquitiesTakerPrefs =
            normalizedPrefsByProfile.equities_taker.order.length > 0
            || Object.keys(normalizedPrefsByProfile.equities_taker.visibility).length > 0;
          if (!hasEquitiesMakerPrefs) {
            normalizedPrefsByProfile.equities_maker = {
              order: [...legacyMakerV4Prefs.order],
              visibility: { ...legacyMakerV4Prefs.visibility },
            };
          }
          if (!hasEquitiesTakerPrefs) {
            normalizedPrefsByProfile.equities_taker = {
              order: [...legacyMakerV4Prefs.order],
              visibility: { ...legacyMakerV4Prefs.visibility },
            };
          }
        }
        let activeProfile = normalizeParamsProfileId(
          version < 3
            ? String(persistedState.activeProfile || '')
                .trim()
                .toLowerCase()
                .replace('taker_task', 'taker')
            : persistedState.activeProfile,
        );
        const activePrefs = normalizedPrefsByProfile[activeProfile] || createDefaultColumnPrefs();

        return {
          ...persistedState,
          activeProfile,
          columnPrefsByProfile: normalizedPrefsByProfile,
          columnPrefs: {
            order: [...activePrefs.order],
            visibility: { ...activePrefs.visibility },
          },
          viewMode: persistedState.viewMode || 'compact',
          auto: typeof persistedState.auto === 'boolean' ? persistedState.auto : true,
          sortState: persistedState.sortState || createDefaultSortState(),
        };
      },
      merge: (persistedState, currentState) => {
        const state = {
          ...currentState,
          ...(persistedState as Partial<ParamsStore>),
        };
        const activeProfile = normalizeParamsProfileId(state.activeProfile);
        const columnPrefsByProfile = normalizeColumnPrefsByProfile(state.columnPrefsByProfile as any);
        const activePrefs = columnPrefsByProfile[activeProfile] || createDefaultColumnPrefs();
        return {
          ...state,
          activeProfile,
          columnPrefsByProfile,
          columnPrefs: {
            order: [...activePrefs.order],
            visibility: { ...activePrefs.visibility },
          },
        };
      },
      partialize: (state) => ({
        auto: state.auto,
        viewMode: state.viewMode,
        activeProfile: state.activeProfile,
        columnPrefsByProfile: state.columnPrefsByProfile,
        sortState: state.sortState
      })
    }
  )
);

/**
 * Params Store Selectors
 *
 * Optimized selector functions to prevent unnecessary re-renders.
 * Use with shallow comparison for object/array returns.
 *
 * @example
 * ```typescript
 * import { useParamsStore, selectParamsAuto, selectParamsViewMode, shallow } from './stores';
 *
 * // Only re-renders when auto state changes
 * const auto = useParamsStore(selectParamsAuto);
 *
 * // Only re-renders when view mode changes
 * const viewMode = useParamsStore(selectParamsViewMode);
 *
 * // Only re-renders when column preferences change
 * const columnPrefs = useParamsStore(selectParamsColumnPrefs, shallow);
 *
 * // Only re-renders when sort state changes
 * const sortState = useParamsStore(selectParamsSortState, shallow);
 *
 * // Only re-renders when selection changes
 * const selected = useParamsStore(selectParamsSelectedStrategies, shallow);
 * ```
 */
export const selectParamsAuto = (state: ParamsStore) => state.auto;
export const selectParamsViewMode = (state: ParamsStore) => state.viewMode;
export const selectParamsActiveProfile = (state: ParamsStore) => state.activeProfile;
export const selectParamsColumnPrefs = (state: ParamsStore) => state.columnPrefs;
export const selectParamsSortState = (state: ParamsStore) => state.sortState;
export const selectParamsSelectedStrategies = (state: ParamsStore) => state.selectedStrategies;
export const selectParamsLastFocusedCell = (state: ParamsStore) => state.lastFocusedCell;
export const selectParamsColumnVisibility = (state: ParamsStore, key: string) =>
  state.columnPrefs.visibility[key];

// Alerts store with loading, auto-refresh, and individual dismiss
type AlertsStore = {
  rows: Alert[];
  loading: boolean;
  auto: boolean;
  lastUpdate?: number;  // Unix timestamp in milliseconds
  lastDataTs?: number;  // Last timestamp when alert rows changed
  lastReceiveTs?: number;  // Last timestamp when a payload was received
  dismissedIds: Set<string>;  // Track dismissed alerts (persisted to localStorage)
  setRows: (rows: Alert[]) => void;
  setLoading: (loading: boolean) => void;
  setAuto: (on: boolean) => void;
  appendAlert: (alert: Alert) => void;
  removeAlert: (id: string) => void;
  dismissAlert: (id: string) => void;
  clearAlerts: () => void;
};

function normalizeForStableSerialization(value: any, seen: WeakSet<object>): any {
  if (value === null || value === undefined) {
    return value;
  }
  if (typeof value !== 'object') {
    return value;
  }
  if (seen.has(value)) {
    return '[Circular]';
  }
  seen.add(value);

  if (Array.isArray(value)) {
    return value.map((item) => normalizeForStableSerialization(item, seen));
  }

  const normalized: Record<string, any> = {};
  for (const key of Object.keys(value).sort()) {
    normalized[key] = normalizeForStableSerialization((value as Record<string, any>)[key], seen);
  }
  return normalized;
}

function stableSerializeAlert(alert: Alert): string {
  try {
    const seen = new WeakSet<object>();
    return JSON.stringify(normalizeForStableSerialization(alert, seen));
  } catch (_err) {
    try {
      return JSON.stringify(alert);
    } catch {
      return String(alert?.id ?? 'unknown-alert');
    }
  }
}

function alertsEquivalent(prev: Alert[], next: Alert[]): boolean {
  if (prev === next) {
    return true;
  }
  if (prev.length !== next.length) {
    return false;
  }

  const prevMap = new Map<string, string>();
  for (const alert of prev) {
    if (!alert?.id) {
      return false;
    }
    prevMap.set(alert.id, stableSerializeAlert(alert));
  }

  for (const alert of next) {
    if (!alert?.id) {
      return false;
    }
    const prevSerialized = prevMap.get(alert.id);
    if (prevSerialized === undefined) {
      return false;
    }
    if (prevSerialized !== stableSerializeAlert(alert)) {
      return false;
    }
  }

  return true;
}

function loadDismissedIds(): Set<string> {
  try {
    const NEW_KEY = 'alerts:dismissed:v2';
    const LEGACY_KEY = 'dismissedAlertIds';
    const stored = localStorage.getItem(NEW_KEY) || localStorage.getItem(LEGACY_KEY);
    if (!stored) return new Set();
    const arr = JSON.parse(stored);
    return Array.isArray(arr) ? new Set(arr.filter((x: unknown) => typeof x === 'string')) : new Set();
  } catch {
    return new Set();
  }
}

export const useAlertsStore = create<AlertsStore>((set) => ({
  rows: [],
  loading: false,
  auto: true,
  lastUpdate: undefined,
  lastDataTs: undefined,
  lastReceiveTs: undefined,
  dismissedIds: loadDismissedIds(),

  setRows: (rows) => set((state) => {
    const nextTs = Date.now();
    const limited = rows.slice(0, STORE_LIMITS.ALERTS);
    if (alertsEquivalent(state.rows, limited)) {
      return {
        ...state,
        lastReceiveTs: nextTs,
      };
    }
    return {
      rows: limited,
      lastUpdate: nextTs,
      lastDataTs: nextTs,
      lastReceiveTs: nextTs,
    };
  }),

  setLoading: (loading) => set((state) => (state.loading === loading ? state : { loading })),

  setAuto: (on) => set((state) => (state.auto === on ? state : { auto: on })),

  appendAlert: (newAlert) => set((state) => {
    const nextTs = Date.now();
    // Check for duplicate by ID
    if (state.rows.some(r => r.id === newAlert.id)) {
      console.warn('[alerts] Duplicate alert id:', newAlert.id);
      return { ...state, lastReceiveTs: nextTs }; // No change
    }

    // Prepend and cap to prevent memory bloat
    const updated = [newAlert, ...state.rows];
    return {
      rows: updated.slice(0, STORE_LIMITS.ALERTS),
      lastUpdate: nextTs,
      lastDataTs: nextTs,
      lastReceiveTs: nextTs,
    };
  }),

  removeAlert: (id) => set((state) => {
    const nextTs = Date.now();
    return {
      rows: state.rows.filter(r => r.id !== id),
      lastUpdate: nextTs,
      lastDataTs: nextTs,
      lastReceiveTs: nextTs,
    };
  }),

  dismissAlert: (id) => set((state) => {
    const newDismissedIds = new Set(state.dismissedIds);
    newDismissedIds.add(id);
    // Persist to localStorage
    try {
      localStorage.setItem('alerts:dismissed:v2', JSON.stringify(Array.from(newDismissedIds)));
    } catch (e) {
      console.error('[alerts] Failed to persist dismissed IDs:', e);
    }
    return { dismissedIds: newDismissedIds };
  }),

  clearAlerts: () => set((state) => ({
    rows: [],
    dismissedIds: new Set(),
    lastUpdate: Date.now(),
    lastDataTs: state.lastDataTs,
    lastReceiveTs: state.lastReceiveTs,
  }))
}));

/**
 * Alerts Store Selectors
 *
 * Optimized selector functions to prevent unnecessary re-renders.
 * Use with shallow comparison for array/set returns.
 *
 * @example
 * ```typescript
 * import { useAlertsStore, selectAlertsRows, selectAlertsBySeverity, shallow } from './stores';
 *
 * // Only re-renders when rows array changes
 * const rows = useAlertsStore(selectAlertsRows, shallow);
 *
 * // Only re-renders when loading state changes
 * const loading = useAlertsStore(selectAlertsLoading);
 *
 * // Only re-renders when critical alerts change
 * const critical = useAlertsStore(state => selectAlertsBySeverity(state, 'CRITICAL'), shallow);
 *
 * // Only re-renders when undismissed alerts change
 * const undismissed = useAlertsStore(selectUndismissedAlerts, shallow);
 *
 * // Only re-renders when lastUpdate changes
 * const lastUpdate = useAlertsStore(selectAlertsLastUpdate);
 * ```
 */
export const selectAlertsRows = (state: AlertsStore) => state.rows;
export const selectAlertsLoading = (state: AlertsStore) => state.loading;
export const selectAlertsAuto = (state: AlertsStore) => state.auto;
export const selectAlertsLastUpdate = (state: AlertsStore) => state.lastUpdate;
export const selectAlertsFreshnessTs = (state: AlertsStore) => state.lastDataTs ?? state.lastUpdate;
export const selectAlertsDismissedIds = (state: AlertsStore) => state.dismissedIds;
export const selectAlertsBySeverity = (state: AlertsStore, severity: string) =>
  state.rows.filter(r => r.level === severity);
export const selectUndismissedAlerts = (state: AlertsStore) =>
  state.rows.filter(r => !state.dismissedIds.has(r.id));
export const selectAlertsCount = (state: AlertsStore) => state.rows.length;
export const selectAlertById = (state: AlertsStore, id: string) =>
  state.rows.find(r => r.id === id);
